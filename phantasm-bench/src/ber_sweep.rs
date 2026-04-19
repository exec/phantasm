//! End-to-end BER measurement through a simulated Twitter recompression.
//!
//! For each cover in a corpus subset, we embed a fixed-size payload twice:
//!   - Arm A: with the Twitter channel adapter enabled (MINICER+ROAST).
//!   - Arm B: with no adapter (control).
//!
//! We then simulate Twitter's server-side re-encode by decoding the stego
//! to RGB pixels via the `image` crate and re-encoding as JPEG at QF=85.
//! This is the same "channel surrogate" the adapter's own survival test
//! (phantasm-channel/tests/survival.rs) validates against, so both arms
//! are tested against the exact recompressor the adapter's model targets.
//!
//! Note: the `image` crate's JpegEncoder defaults to 4:4:4 chroma, not
//! Twitter's documented 4:2:0. Since phantasm only embeds in the luma
//! component, chroma subsampling does not affect parity of the embedded
//! AC coefficients, and the `image`-crate re-encode is the exact model
//! the adapter is designed against. A future adapter revision targeting
//! real Twitter output (mozjpeg + 4:2:0 + progressive) could use a
//! different recompressor here.
//!
//! Extraction is attempted on the recompressed stego. If extract returns
//! Ok(bytes) we compute bit error rate vs the original payload. If extract
//! returns any error (auth failure, framing corruption, etc.) we count
//! that as an extract failure (bucketed separately from successful
//! extracts with nonzero BER).
//!
//! Usage:
//!   phantasm-bench ber-sweep \
//!       --corpus /Users/dylan/Developer/phantasm/research-corpus \
//!       --limit 40 \
//!       --payload-size 3000 \
//!       --output /tmp/phantasm-ber-results.md

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rand::rngs::StdRng;
use rand::{RngCore, SeedableRng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use phantasm_core::channel::ChannelProfile;
use phantasm_core::content_adaptive::ContentAdaptiveOrchestrator;
use phantasm_core::orchestrator::Orchestrator;
use phantasm_core::plan::{EmbedPlan, HashSensitivity};
use phantasm_core::stealth::StealthTier;
use phantasm_core::{ChannelAdapter, TwitterProfile};
use phantasm_cost::juniward::Juniward;

use crate::eval_corpus::walk_jpeg_files;

#[derive(Debug, Clone)]
pub struct BerSweepArgs {
    pub corpus: PathBuf,
    pub limit: usize,
    pub payload_size: usize,
    pub passphrase_prefix: String,
    pub recompress_qf: u8,
    pub output: Option<PathBuf>,
    pub json_output: Option<PathBuf>,
}

impl Default for BerSweepArgs {
    fn default() -> Self {
        Self {
            corpus: PathBuf::from("."),
            limit: 40,
            payload_size: 3000,
            passphrase_prefix: "phantasm-ber-sweep-v1".into(),
            recompress_qf: 85,
            output: None,
            json_output: None,
        }
    }
}

/// Per-image, per-arm outcome.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArmTrial {
    pub image: String,
    /// `true` if `embed` succeeded.
    pub embed_ok: bool,
    /// Only present if `embed_ok`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embed_error: Option<String>,
    /// `true` if `extract` returned Ok with any bytes (even partial / wrong length).
    pub extract_ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extract_error: Option<String>,
    /// Bits different between extracted bytes and original payload, divided by
    /// 8 * payload_size. Only meaningful when `extract_ok == true` AND the
    /// extracted byte length equals the payload length.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ber: Option<f64>,
    /// `true` if extracted bytes exactly equal the payload.
    pub exact_match: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArmSummary {
    pub name: String,
    pub images_attempted: usize,
    pub embed_failures: usize,
    pub extract_failures: usize,
    /// Of the extract successes, how many returned bytes of exactly `payload_size`.
    pub length_matches: usize,
    /// Of the length-matches, how many are byte-for-byte exact.
    pub exact_matches: usize,
    /// BER statistics across length-matched extract-successes (where BER is defined).
    pub ber_mean: f64,
    pub ber_median: f64,
    pub ber_p90: f64,
    pub ber_min: f64,
    pub ber_max: f64,
    /// Extract success rate = extract_ok / images_attempted (does not require exact match).
    pub extract_success_rate: f64,
    /// Exact-match rate = exact_matches / images_attempted.
    pub exact_match_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BerSweepResult {
    pub generated_at: String,
    pub corpus: String,
    pub images_used: usize,
    pub payload_size: usize,
    pub recompress_qf: u8,
    pub adapter_on: ArmSummary,
    pub adapter_off: ArmSummary,
    pub adapter_on_trials: Vec<ArmTrial>,
    pub adapter_off_trials: Vec<ArmTrial>,
}

// ── timestamp helper (reuse eval_corpus's epoch formatter via a small clone) ──

fn now_iso8601() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let sec = secs % 60;
    let min = (secs / 60) % 60;
    let hour = (secs / 3600) % 24;
    let days = secs / 86400;
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{hour:02}:{min:02}:{sec:02}Z")
}

fn deterministic_passphrase(prefix: &str, image_path: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(image_path.to_string_lossy().as_bytes());
    let hash = hasher.finalize();
    let hex_prefix: String = hash[..4].iter().map(|b| format!("{b:02x}")).collect();
    format!("{prefix}-{hex_prefix}")
}

fn make_embed_plan(payload_bytes: usize) -> EmbedPlan {
    EmbedPlan {
        channel: ChannelProfile::builtin("twitter").expect("twitter channel always exists"),
        stealth_tier: StealthTier::High,
        capacity_bits: 0,
        payload_bits: payload_bytes * 8,
        ecc_bits: 0,
        estimated_detection_error: 0.5,
        hash_constrained_positions: 0,
        hash_sensitivity: HashSensitivity::Robust,
    }
}

/// Simulate Twitter's server-side recompression: decode to RGB pixels,
/// re-encode as JPEG at `qf`. Uses the `image` crate's baseline JPEG
/// encoder — the same surrogate the channel adapter's survival test
/// exercises.
fn recompress_like_twitter(input: &Path, output: &Path, qf: u8) -> Result<()> {
    let img = image::open(input)
        .with_context(|| format!("decoding {}", input.display()))?
        .to_rgb8();
    let mut out =
        std::fs::File::create(output).with_context(|| format!("creating {}", output.display()))?;
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, qf);
    img.write_with_encoder(encoder)
        .with_context(|| format!("encoding {}", output.display()))?;
    Ok(())
}

fn bit_diff(a: &[u8], b: &[u8]) -> u64 {
    let n = a.len().min(b.len());
    let mut diffs = 0u64;
    for i in 0..n {
        diffs += (a[i] ^ b[i]).count_ones() as u64;
    }
    diffs
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = (p / 100.0 * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn summarize(name: &str, trials: &[ArmTrial], payload_size: usize) -> ArmSummary {
    let images_attempted = trials.len();
    let embed_failures = trials.iter().filter(|t| !t.embed_ok).count();
    let extract_failures = trials
        .iter()
        .filter(|t| t.embed_ok && !t.extract_ok)
        .count();
    let length_matches = trials.iter().filter(|t| t.ber.is_some()).count();
    let exact_matches = trials.iter().filter(|t| t.exact_match).count();

    let mut bers: Vec<f64> = trials.iter().filter_map(|t| t.ber).collect();
    bers.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let ber_mean = if bers.is_empty() {
        f64::NAN
    } else {
        bers.iter().sum::<f64>() / bers.len() as f64
    };
    let ber_median = percentile(&bers, 50.0);
    let ber_p90 = percentile(&bers, 90.0);
    let ber_min = bers.first().copied().unwrap_or(f64::NAN);
    let ber_max = bers.last().copied().unwrap_or(f64::NAN);

    let extract_success_rate = if images_attempted > 0 {
        trials.iter().filter(|t| t.extract_ok).count() as f64 / images_attempted as f64
    } else {
        0.0
    };
    let exact_match_rate = if images_attempted > 0 {
        exact_matches as f64 / images_attempted as f64
    } else {
        0.0
    };

    // payload_size is reported for downstream context (e.g. confirming bit budget)
    let _ = payload_size;

    ArmSummary {
        name: name.to_string(),
        images_attempted,
        embed_failures,
        extract_failures,
        length_matches,
        exact_matches,
        ber_mean,
        ber_median,
        ber_p90,
        ber_min,
        ber_max,
        extract_success_rate,
        exact_match_rate,
    }
}

fn run_one_trial(
    cover: &Path,
    payload: &[u8],
    passphrase: &str,
    adapter_on: bool,
    recompress_qf: u8,
) -> ArmTrial {
    let image_key = cover.to_string_lossy().to_string();
    let tmp_stego = tempfile::Builder::new()
        .suffix(".jpg")
        .tempfile()
        .expect("tempfile");
    let stego_path = tmp_stego.path().to_path_buf();

    let mut orchestrator = ContentAdaptiveOrchestrator::new(Box::new(Juniward));
    if adapter_on {
        let adapter: Box<dyn ChannelAdapter> = Box::new(TwitterProfile::default());
        orchestrator = orchestrator.with_channel_adapter(adapter);
    }

    let plan = make_embed_plan(payload.len());
    let embed_res = orchestrator.embed(cover, payload, passphrase, &plan, &stego_path);

    let embed_ok = embed_res.is_ok();
    let embed_error = embed_res.as_ref().err().map(|e| e.to_string());
    if !embed_ok {
        return ArmTrial {
            image: image_key,
            embed_ok: false,
            embed_error,
            extract_ok: false,
            extract_error: None,
            ber: None,
            exact_match: false,
        };
    }

    // Recompress through the Twitter surrogate.
    let tmp_reenc = tempfile::Builder::new()
        .suffix(".jpg")
        .tempfile()
        .expect("tempfile");
    let reenc_path = tmp_reenc.path().to_path_buf();
    if let Err(e) = recompress_like_twitter(&stego_path, &reenc_path, recompress_qf) {
        return ArmTrial {
            image: image_key,
            embed_ok: true,
            embed_error: None,
            extract_ok: false,
            extract_error: Some(format!("recompress: {e}")),
            ber: None,
            exact_match: false,
        };
    }

    // Extraction does not consult the distortion function, so any cost fn
    // works here. The adapter-presence flag *does* matter: it selects the
    // ECC route in the pipeline, which must match the embed side.
    let mut extractor = ContentAdaptiveOrchestrator::new(Box::new(phantasm_cost::Uniform));
    if adapter_on {
        let adapter: Box<dyn ChannelAdapter> = Box::new(TwitterProfile::default());
        extractor = extractor.with_channel_adapter(adapter);
    }
    let extract_res = extractor.extract(&reenc_path, passphrase);

    match extract_res {
        Ok(bytes) => {
            let (ber, exact_match) = if bytes.len() == payload.len() {
                let diff = bit_diff(&bytes, payload);
                let total_bits = payload.len() as u64 * 8;
                let ber = diff as f64 / total_bits as f64;
                (Some(ber), diff == 0)
            } else {
                // Extracted something, but length doesn't match — can't compute a
                // meaningful per-bit BER. Treat as extract-succeeded-but-corrupted.
                (None, false)
            };
            ArmTrial {
                image: image_key,
                embed_ok: true,
                embed_error: None,
                extract_ok: true,
                extract_error: None,
                ber,
                exact_match,
            }
        }
        Err(e) => ArmTrial {
            image: image_key,
            embed_ok: true,
            embed_error: None,
            extract_ok: false,
            extract_error: Some(e.to_string()),
            ber: None,
            exact_match: false,
        },
    }
}

pub fn run_ber_sweep(args: &BerSweepArgs) -> Result<BerSweepResult> {
    let mut images = walk_jpeg_files(&args.corpus)
        .with_context(|| format!("walking corpus {:?}", args.corpus))?;
    if images.is_empty() {
        anyhow::bail!("no JPEGs found under {}", args.corpus.display());
    }
    // Deterministic subset: we already sort in walk_jpeg_files, so truncation is stable.
    images.truncate(args.limit);
    let images_used = images.len();

    // Single deterministic payload reused across all images and both arms.
    // Seeded RNG so runs are comparable across time.
    let mut rng = StdRng::seed_from_u64(0xB3E_2024);
    let mut payload = vec![0u8; args.payload_size];
    rng.fill_bytes(&mut payload);

    let mut adapter_on_trials = Vec::with_capacity(images_used);
    let mut adapter_off_trials = Vec::with_capacity(images_used);

    for (idx, cover) in images.iter().enumerate() {
        eprintln!(
            "[{}/{}] {}",
            idx + 1,
            images_used,
            cover.file_name().unwrap_or_default().to_string_lossy()
        );
        let passphrase = deterministic_passphrase(&args.passphrase_prefix, cover);

        let trial_on = run_one_trial(cover, &payload, &passphrase, true, args.recompress_qf);
        let trial_off = run_one_trial(cover, &payload, &passphrase, false, args.recompress_qf);

        adapter_on_trials.push(trial_on);
        adapter_off_trials.push(trial_off);
    }

    let adapter_on = summarize("adapter=twitter", &adapter_on_trials, args.payload_size);
    let adapter_off = summarize("adapter=none", &adapter_off_trials, args.payload_size);

    let result = BerSweepResult {
        generated_at: now_iso8601(),
        corpus: args.corpus.to_string_lossy().to_string(),
        images_used,
        payload_size: args.payload_size,
        recompress_qf: args.recompress_qf,
        adapter_on,
        adapter_off,
        adapter_on_trials,
        adapter_off_trials,
    };

    if let Some(md_path) = &args.output {
        std::fs::write(md_path, build_markdown(&result))
            .with_context(|| format!("writing markdown {:?}", md_path))?;
    } else {
        print!("{}", build_markdown(&result));
    }

    if let Some(json_path) = &args.json_output {
        let json = serde_json::to_string_pretty(&result)?;
        std::fs::write(json_path, &json)
            .with_context(|| format!("writing json {:?}", json_path))?;
    }

    Ok(result)
}

fn fmt_ber(x: f64) -> String {
    if x.is_nan() {
        "—".to_string()
    } else {
        format!("{:.4}", x)
    }
}

fn build_markdown(r: &BerSweepResult) -> String {
    let mut md = String::new();
    md.push_str("# Phantasm channel-adapter BER sweep\n\n");
    md.push_str(&format!("- **Corpus:** `{}`\n", r.corpus));
    md.push_str(&format!("- **Images used:** {}\n", r.images_used));
    md.push_str(&format!("- **Payload size:** {} bytes\n", r.payload_size));
    md.push_str(&format!(
        "- **Recompress:** `image` crate JpegEncoder, QF={}\n",
        r.recompress_qf
    ));
    md.push_str("- **Cost function:** J-UNIWARD, stealth tier `High`\n");
    md.push_str(&format!("- **Generated:** {}\n\n", r.generated_at));

    md.push_str("## Outcomes per arm\n\n");
    md.push_str("| Arm | Attempted | Embed fail | Extract fail | Length mismatch | Exact matches | Extract success rate | Exact match rate |\n");
    md.push_str("|-----|-----------|------------|--------------|-----------------|---------------|----------------------|------------------|\n");
    for arm in [&r.adapter_on, &r.adapter_off] {
        let length_mismatches =
            arm.images_attempted - arm.embed_failures - arm.extract_failures - arm.length_matches;
        md.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {:.1}% | {:.1}% |\n",
            arm.name,
            arm.images_attempted,
            arm.embed_failures,
            arm.extract_failures,
            length_mismatches,
            arm.exact_matches,
            arm.extract_success_rate * 100.0,
            arm.exact_match_rate * 100.0,
        ));
    }

    md.push_str("\n## BER across length-matched extracts\n\n");
    md.push_str("(BER = bit_diff / (8 * payload_size). N = number of trials where extract returned bytes of exactly `payload_size`.)\n\n");
    md.push_str("| Arm | N | Mean | Median | p90 | Min | Max |\n");
    md.push_str("|-----|---|------|--------|-----|-----|-----|\n");
    for arm in [&r.adapter_on, &r.adapter_off] {
        md.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            arm.name,
            arm.length_matches,
            fmt_ber(arm.ber_mean),
            fmt_ber(arm.ber_median),
            fmt_ber(arm.ber_p90),
            fmt_ber(arm.ber_min),
            fmt_ber(arm.ber_max),
        ));
    }

    md.push_str("\n## Verdict\n\n");
    let on = &r.adapter_on;
    let off = &r.adapter_off;
    let exact_delta = on.exact_match_rate - off.exact_match_rate;
    let extract_delta = on.extract_success_rate - off.extract_success_rate;

    let verdict = if on.exact_matches == 0 && off.exact_matches == 0 {
        format!(
            "**Neutral / neither arm survives the channel.** \
             Zero exact matches on either arm across {n} images. \
             The AEAD envelope's 16-byte authentication tag means any single \
             coefficient-parity flip in the ciphertext region causes \
             `extract` to fail auth; the Reed-Solomon layer is \
             payload-inside-envelope and cannot repair ciphertext bit errors. \
             Conclusion: for AEAD-authenticated payloads, sub-100% coefficient \
             survival is equivalent to 0% extract success. The adapter's \
             98.7% block-survival figure does not translate to end-to-end \
             deliverability at this configuration.",
            n = r.images_used,
        )
    } else if exact_delta > 0.05 {
        format!(
            "**Channel adapter helps.** Exact-match rate {:.1}% (adapter on) vs \
             {:.1}% (adapter off), Δ = +{:.1} pp. Extract success rate \
             delta: {:+.1} pp.",
            on.exact_match_rate * 100.0,
            off.exact_match_rate * 100.0,
            exact_delta * 100.0,
            extract_delta * 100.0,
        )
    } else if exact_delta < -0.05 {
        format!(
            "**Channel adapter hurts.** Exact-match rate {:.1}% (adapter on) vs \
             {:.1}% (adapter off), Δ = {:.1} pp. Extract success rate \
             delta: {:+.1} pp.",
            on.exact_match_rate * 100.0,
            off.exact_match_rate * 100.0,
            exact_delta * 100.0,
            extract_delta * 100.0,
        )
    } else {
        format!(
            "**Neutral.** Exact-match rate {:.1}% (adapter on) vs {:.1}% (adapter off), \
             |Δ| ≤ 5 pp. No meaningful improvement from the channel adapter at this \
             configuration.",
            on.exact_match_rate * 100.0,
            off.exact_match_rate * 100.0,
        )
    };
    md.push_str(&verdict);
    md.push_str("\n\n");

    md.push_str("## Methodology notes\n\n");
    md.push_str("- Both arms embed an identical random 3000-byte payload using J-UNIWARD costs and `StealthTier::High`. A deterministic seed ensures the payload is reproducible across runs.\n");
    md.push_str("- Passphrase is derived deterministically per image (`SHA-256(path)[..4]` suffix on a fixed prefix) so the same passphrase is used to extract as to embed, and runs are reproducible.\n");
    md.push_str("- The Twitter surrogate is `image::codecs::jpeg::JpegEncoder::new_with_quality(_, 85)`. This is the exact recompressor the adapter's internal survival test (phantasm-channel/tests/survival.rs) is validated against, so the adapter is being tested against its own model — a best-case scenario for the adapter.\n");
    md.push_str("- `extract` failures are classified separately from length-matched extracts with high BER. Because phantasm uses an AEAD envelope, any ciphertext-region bit error typically manifests as an authentication failure (i.e., extract returns Err) rather than a length-preserving corrupted output.\n");
    md.push_str("- The subset is the first N images from `walk_jpeg_files` (sorted path order), giving a stable slice of the mixed-QF Picsum corpus.\n");

    md
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bit_diff_counts_differing_bits() {
        assert_eq!(bit_diff(&[0u8], &[0u8]), 0);
        assert_eq!(bit_diff(&[0xFF], &[0x00]), 8);
        assert_eq!(bit_diff(&[0b1010_1010], &[0b0101_0101]), 8);
        assert_eq!(bit_diff(&[0b1111_0000, 0x00], &[0b0000_0000, 0xFF]), 4 + 8);
    }

    #[test]
    fn percentile_handles_empty_and_singleton() {
        assert_eq!(percentile(&[], 50.0), 0.0);
        assert_eq!(percentile(&[0.42], 50.0), 0.42);
        assert_eq!(percentile(&[0.42], 90.0), 0.42);
    }

    #[test]
    fn percentile_monotone() {
        let v: Vec<f64> = (0..=100).map(|i| i as f64).collect();
        assert_eq!(percentile(&v, 0.0), 0.0);
        assert_eq!(percentile(&v, 50.0), 50.0);
        assert_eq!(percentile(&v, 90.0), 90.0);
        assert_eq!(percentile(&v, 100.0), 100.0);
    }

    #[test]
    fn summarize_empty() {
        let s = summarize("arm", &[], 3000);
        assert_eq!(s.images_attempted, 0);
        assert_eq!(s.length_matches, 0);
        assert_eq!(s.exact_matches, 0);
        assert!(s.ber_mean.is_nan());
        assert_eq!(s.extract_success_rate, 0.0);
    }

    #[test]
    fn summarize_counts_and_rates() {
        let trials = vec![
            ArmTrial {
                image: "a".into(),
                embed_ok: true,
                embed_error: None,
                extract_ok: true,
                extract_error: None,
                ber: Some(0.0),
                exact_match: true,
            },
            ArmTrial {
                image: "b".into(),
                embed_ok: true,
                embed_error: None,
                extract_ok: true,
                extract_error: None,
                ber: Some(0.1),
                exact_match: false,
            },
            ArmTrial {
                image: "c".into(),
                embed_ok: true,
                embed_error: None,
                extract_ok: false,
                extract_error: Some("auth".into()),
                ber: None,
                exact_match: false,
            },
            ArmTrial {
                image: "d".into(),
                embed_ok: false,
                embed_error: Some("tooBig".into()),
                extract_ok: false,
                extract_error: None,
                ber: None,
                exact_match: false,
            },
        ];
        let s = summarize("arm", &trials, 3000);
        assert_eq!(s.images_attempted, 4);
        assert_eq!(s.embed_failures, 1);
        assert_eq!(s.extract_failures, 1);
        assert_eq!(s.length_matches, 2);
        assert_eq!(s.exact_matches, 1);
        assert!((s.ber_mean - 0.05).abs() < 1e-9);
        assert_eq!(s.ber_min, 0.0);
        assert_eq!(s.ber_max, 0.1);
        // extract_success_rate = 2 of 4 extract_ok.
        assert!((s.extract_success_rate - 0.5).abs() < 1e-9);
        assert!((s.exact_match_rate - 0.25).abs() < 1e-9);
    }

    #[test]
    fn deterministic_passphrase_stable() {
        let p1 = deterministic_passphrase("pfx", Path::new("/x/y/z.jpg"));
        let p2 = deterministic_passphrase("pfx", Path::new("/x/y/z.jpg"));
        assert_eq!(p1, p2);
        let p3 = deterministic_passphrase("pfx", Path::new("/x/y/other.jpg"));
        assert_ne!(p1, p3);
    }
}
