# Changelog

All notable changes to phantasm will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] — 2026-04-14

First stable-tagged release. All five PLAN.md thesis pillars reachable via the main CLI. Research-grade, interface-unstable, envelope-format-unstable. Not for production.

### Added

- **J-UNIWARD cost function** (`phantasm-cost/src/juniward.rs`) — full Holub-Fridrich 2014 implementation with Daubechies-8 wavelet decomposition (16-tap filter, cross-checked against `pywt.Wavelet('db8').dec_lo`), three directional sub-bands, and the precomputed-impulse-response optimization (3×64 fixed 23×23 kernels per image). Wired into `phantasm-bench eval-corpus --cost-functions ... j-uniward` and the `phantasm embed --cost-function j-uniward` CLI.
- **Research-raw embedding path** (`phantasm-core/src/research_raw.rs`) — `#[doc(hidden)]` benchmarking-only path that bypasses the crypto envelope and takes an exact target STC message bit count. Unlocks true security-capacity curves; callers control the STC rate directly. Not exposed in the main CLI; used by the `research-curve` bench subcommand.
- **`phantasm-channel` sub-crate** — Phase 2 channel adapter for compression-resilient embedding. `trait ChannelAdapter` with `TwitterProfile` (QF=85, 4:2:0) implementing MINICER-style parity-preservation and ROAST overflow alleviation. Measured 98.7% coefficient survival rate on real mozjpeg re-encoding. 16 tests.
- **`phantasm-core::hash_guard` module** — Phase 3 perceptual-hash preservation. `classify_sensitivity(&JpegCoefficients) -> SensitivityTier` returns Robust/Marginal/Sensitive based on margin of each hash bit from its decision threshold, calibrated from day-1 Spike B empirical data. `apply_hash_guard(&mut CostMap, &JpegCoefficients, HashType)` adds wet-paper constraints (cost = infinity) on coefficients that would flip pHash or dHash bits. 10 tests.
- **`phantasm embed --channel-adapter {none,twitter}`** and **`--hash-guard {none,phash,dhash}`** CLI flags, routed to `ContentAdaptiveOrchestrator::with_channel_adapter` / `with_hash_guard` builder methods. Both default to `none` for backward compatibility.
- **`phantasm extract`** gains matching `--channel-adapter` and `--hash-guard` flags, accepted as forward-compat no-ops (extract derives positions geometrically from stego and doesn't consult costs; auto-detection is queued for v0.2).
- **`phantasm analyze`** now prints sensitivity tier and hash-guard wet-position count alongside existing capacity/JPEG metadata.
- **`phantasm-bench research-curve` subcommand** — security-capacity curve harness using the research-raw path. Takes `--cost-functions` and `--bit-counts` lists, runs 1:n:m iterations per image, aggregates detection rate + SRM L2 per (cost_fn, bit_count), emits JSON + markdown.
- **Published DDE Lab H̃ tables** in `phantasm-stc/src/parity.rs` — verbatim transcription of the Filler 2011 / DDE Lab `mats[]` array (2400 `u64` values, covers constraint heights 7–12, sub-matrix widths 1–20). Stored as `static DDE_MATS` to avoid `clippy::large_stack_arrays`. Replaces the day-1 deterministic PRNG construction.
- **Conditional-probability double-layer STC** in `phantasm-stc/src/double_layer.rs` — replaces day-1's independent bit-plane decomposition with a 4-cell cost table and cheaper-sibling cost coding. Measured bits/L1 efficiency at h=10, n=4096 under uniform costs: **0.995×** vs ~0.68× legacy. Essentially the asymptotic ML2 bound.

### Changed

- **Fridrich RS detection rate dropped vs alpha.** Day-2 alpha measurements reported 75.3% Uniform / 30.8% UERD at 198-image corpus scale. v0.1.0 measurements report **66.7% Uniform / 26.8% UERD** on the same corpus with the same payload. The ~9 pp / ~4 pp improvement is a direct consequence of the STC efficiency lift (0.68× → 0.995×): ~32% fewer actual coefficient modifications per embed → 32% fewer flips for Fridrich RS to detect. This is a verified improvement, not a regression.
- **Mean file-size delta under UERD is now negative.** v0.1.0 measurements report **−1,321 B mean delta** (stego is on average smaller than cover) under UERD on the 198-image corpus, vs day-1's +3,057 B mean inflation. Same mechanism: fewer STC-driven modifications + mozjpeg's trellis_quant Huffman rebuild compressing the modified coefficient distribution better than the original cover entropy. Uniform dropped from +10,189 B to +5,390 B.
- **`double_layer.rs::planes(x)`** uses `x.rem_euclid(4)` instead of `|x| & 1 / |x| >> 1`. The old convention aliased on negative x and broke the ±2 ⇒ flip-plane-1 invariant. Exposed by a half-wet ternary test (seed 4) after the DDE Lab H̃ tables went in. This was a latent correctness bug in the day-1 implementation.
- **`phantasm analyze` switched from `MinimalOrchestrator` to `ContentAdaptiveOrchestrator`** to access sensitivity classification and hash-guard wet-position estimation.

### Research findings

- **Updated headline:** UERD cuts classical Fridrich RS detection rate from **66.7% → 26.8%** at 198-image corpus scale (2.5× reduction, 40 percentage-point drop). UERD wins 195/198 paired RS comparisons (98.5%), 198/198 paired SRM-lite L2 (100%), and 198/198 paired SSIM (100%). Mean paired SSIM Δ = +0.114.
- **J-UNIWARD vs UERD (unpaired, same corpus):** J-UNIWARD wins perceptual distortion metrics (SSIM +0.002, PSNR +1.49 dB, MSE −7.08) but loses statistical undetectability (Fridrich RS +0.0071 worse, SRM-lite L2 +0.0664 worse) at this payload/capacity ratio. **UERD is the best cost function for this corpus at ~31% capacity.** J-UNIWARD embed cost is ~3.5× UERD per image.
- **Security-capacity curve inversion:** at 20,000-bit payloads, the ordering flips — Uniform detection fraction jumps to 40%, UERD holds at 20%, and **J-UNIWARD holds at the 17.5% noise floor**. J-UNIWARD wins the high-capacity / security-critical regime.
- **Channel adapter survival rate:** 98.7% of stabilized coefficients survive a real mozjpeg re-encode at QF=85 on the test fixture. Twitter profile is single-block approximation; inter-block AC coupling and rescale for >4096 px images are known limitations.
- **Hash sensitivity tier distribution on 22-image qf85/512 Picsum subset:** 15 Robust / 5 Marginal / 2 Sensitive = 68%/23%/9%. Matches day-1 Spike B's empirical bimodality (75/15/10) within corpus-size noise. Hash guard is a no-op on Robust covers (most common case).

### Known limitations (unchanged or new)

- No compression resilience for non-Twitter channels. Facebook, Instagram, etc. need dedicated profiles.
- No pre-nudge for Sensitive covers — wet-paper marks become large sets that may exhaust effective capacity. Queued for v0.2.
- No PDQ hash support (Facebook's perceptual hash for CSAM databases). Queued for v0.2.
- No SRNet / EfficientNet / ML-based steganalysis evaluation. Classical detectors only.
- Envelope format v2 is unstable. A v3 format break is expected in v0.2 to carry `--channel-adapter` / `--hash-guard` metadata for auto-detection.
- CLI is pre-stable. Flag names may shift before v1.0.0.
- No external security review has been performed.

### Deferred to post-v0.1.0

- Task #11: λ-tuned m1/m2 entropy-budget split in double-layer STC for skewed cost distributions. Current 50/50 split achieves 0.995× efficiency at uniform costs but leaves a few percent on the table under J-UNIWARD-shaped costs. Optional polish.
- Task #7: `phantasm analyze` per-image hash-sensitivity classification output (partially done — basic tier print is in; full per-bit margin report deferred).
- Task #19: Preserve JPEG progression mode from input to output.
- Task #20: `phantasm analyze` capacity computation ignores envelope padding.

## [0.1.0-alpha] — 2026-04-14

First publishable alpha release. Research code, interface-unstable, envelope-format-unstable. Not for production.

### Added

- **`phantasm embed --cost-function {uniform,uerd}` CLI flag** with `uerd` as the default. Content-adaptive UERD embedding is now the shipping default, reachable from the main CLI rather than only the `compare_cost_functions` example harness.
- **Corpus-scale Fridrich RS detection rate evaluation.** `phantasm-bench eval-corpus` now aggregates `fridrich_rs.max_rate`, a detection verdict (`max_rate > 0.05`), and `srm_lite_l2_distance` per cost function with paired comparison rows. Validated on 198 images.
- **Permutation MAC (HMAC-SHA256, truncated 16 bytes) in the crypto envelope.** Wrong-passphrase errors now surface cleanly as `authentication failed` instead of the earlier `declared length 3331321903 exceeds available 8060` framing-length garbage.
- **`phantasm-crypto` envelope format version byte (v2).** v1 envelopes are intentionally unrecoverable under v2 — bumps are not backward-compatible until format stabilizes post-alpha.
- **HKDF-SHA256 key split** over the Argon2id master key, producing independent `aead_key` and `mac_key`. Info strings bind subkey derivation to the envelope format version.
- **libjpeg FFI hardening via panic-across-C-unwind.** Custom `error_exit` panics a typed `LibjpegPanic` payload; public entry points wrap their bodies in `catch_unwind` + `AssertUnwindSafe` with RAII guards for `FILE*` and `jpeg_*_struct`. Corrupt JPEGs now return `Err`, not a process crash.
- **New FFI tests** for truncated, garbage, and missing JPEG inputs.
- **`phantasm-image/examples/measure_huffman_reopt.rs`** — benchmarking harness for the Huffman-rebuild path.

### Changed

- **Envelope format bumped from v1 to v2.** Not backward-compatible by design (pre-alpha research code; expect more breaks before v0.1.0 stable).
- **`phantasm-core::pipeline::bytes_to_envelope`** now delegates to `Envelope::from_bytes`, the canonical v2 serialization.
- **`phantasm-core::pipeline::extract_from_cover`** collapses all pre-`Envelope::open()` failure modes (unframe, envelope parse, MAC check) into a single clean `CryptoError::AuthFailed` surface. `CryptoError::UnsupportedVersion(_)` is preserved as the one variant that passes through the collapse.
- **`phantasm-cli` embed command** refactored to take an `EmbedArgs<'_>` struct to accommodate the new flag without tripping clippy's `too_many_arguments`.

### Research findings

- **UERD cuts classical Fridrich RS detection rate from 75.3% to 30.8% at population scale** (198-image corpus, 3,723-byte fixed payload, Aletheia-faithful RS detector). 44.4 percentage-point drop, 2.4× reduction. Mean RS `max_rate` 0.48 → 0.05 (8.8× reduction). Paired: UERD beats Uniform on 196/198 images.
- **UERD wins SSIM 198/198** on the same corpus (mean paired delta +0.127, median +0.120).
- **UERD wins SRM-lite L2 distance 198/198** (mean 0.649 → 0.189, 3.4× lower).
- **mozjpeg `JCP_MAX_COMPRESSION` profile** already enables `trellis_quant`, which rebuilds Huffman tables on write regardless of the `optimize_coding` flag. Task #17 was effectively done by default.

### Known limitations

- No compression resilience yet — don't upload stego to social media and expect recovery.
- No perceptual-hash preservation yet — pHash/dHash/PDQ may change after embedding at higher densities.
- Single-layer STC only in the main pipeline (double-layer exists but not wired).
- STC H̃ sub-matrix is a deterministic PRNG construction, not the published Filler 2011 / DDE Lab tables.
- Envelope format v2 is unstable — expect breaks before v0.1.0 stable.
- CLI interface is unstable — flag names and command shape may change.

### Security caveats

- No external security review has been performed. Cryptographic primitives (Argon2id, XChaCha20-Poly1305, HMAC-SHA256, HKDF-SHA256) are used via established crates (`argon2`, `chacha20poly1305`, `hmac`, `sha2`, `hkdf`), but the composition, key-split, and envelope layout are project-specific and unreviewed. Do not rely on phantasm alone to protect information from a well-resourced adversary.
- The passphrase KDF cost (Argon2id: 64 MiB, 3 iterations, 4 threads) is suitable for interactive use on modern hardware but may be inadequate against offline attackers with dedicated hardware. Tune as needed.

[Unreleased]: https://github.com/dylan/phantasm/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/dylan/phantasm/releases/tag/v0.1.0
[0.1.0-alpha]: https://github.com/dylan/phantasm/releases/tag/v0.1.0-alpha
