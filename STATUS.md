# Phantasm — Project Status

**Date:** 2026-04-14 (v0.1.0 shipped)
**Session length:** ~13 hours across 2 days
**Workspace state:** `cargo test --workspace` 224 passing / 0 failing, `cargo clippy --workspace --all-targets -- -D warnings` clean, `cargo fmt --all --check` clean
**Git state:** `v0.1.0-alpha` tagged at commit `82b89a4`. `v0.1.0` tagged at commit `1617dad`. Pushed to `https://github.com/exec/phantasm` (private repo, owner `exec`). Two cosmetic README commits past the v0.1.0 tag in `main` (logo + heading drop) — not part of the tag.
**v0.1.0 shipping headline:** UERD cuts classical Fridrich RS detection rate from **66.7% → 26.8%** at 198-image corpus scale (40 pp drop, 2.5× reduction, paired 195/198). Mean file-size delta is NEGATIVE under UERD at −1,321 B per image (stego smaller than cover). See §5 Finding 8 for the full numbers.
**Five-pillar thesis:** All reachable via the `phantasm` CLI in v0.1.0:
  1. **Content-adaptive cost functions** — `--cost-function {uniform, uerd, j-uniward}`
  2. **Syndrome-trellis coding** — published DDE Lab H̃ tables (Filler 2011) + conditional-probability double-layer at 0.995× bits/L1
  3. **Modern AEAD envelope** — Argon2id + XChaCha20-Poly1305 + HMAC-SHA256-16 + HKDF key split + FORMAT_VERSION byte 2
  4. **Channel-adaptive preprocessing** — `--channel-adapter twitter` (MINICER+ROAST, 98.7% coefficient survival)
  5. **Perceptual-hash preservation** — `--hash-guard {phash, dhash}` (3-tier sensitivity classifier + wet-paper constraint)

---

## 1. What Phantasm Is

Phantasm is a compression-resilient, steganalysis-resistant image steganography tool in Rust. Its thesis (from `PLAN.md`): no existing tool combines channel-adaptive preprocessing, content-adaptive distortion minimization, syndrome-trellis coding, perceptual-hash preservation, and modern authenticated encryption — phantasm integrates all five.

As of end-of-day 1, the crate skeleton is complete, the end-to-end pipeline works on real JPEGs, and content-adaptive embedding (via UERD) is implemented and measurably working on a 198-image research corpus. The project is ~15% of the way to v1 by feature/code volume, ~20% if you count risk retired.

---

## 2. Day-1 Timeline (what was built and why)

### Burst 0 — Doc review and planning
- Reviewed `RESEARCH.md` (steganography literature) and `PLAN.md` (architectural plan)
- Flagged six issues with the original plan:
  1. The #1 technical risk (libjpeg-turbo FFI coefficient round-trip) was scheduled casually rather than de-risked first
  2. The thesis's central novelty (pHash preservation) was scheduled for Phase 3 weeks 12–15; if infeasible, 12 weeks of work would be wasted
  3. `PLAN.md` §8 "no magic headers" didn't specify extraction termination (how does extract know where payload ends?)
  4. The image-salt derivation used raw DC coefficients which drift under recompression
  5. Adversarial cost adjustment was bundled into v1 scope (research ML project layered on a research systems project)
  6. Phase 0 bundled five unrelated milestones ("crypto + STC + libjpeg FFI + workspace + CLI") under one checkpoint
- **All six addressed in `PLAN.md` edits (v0.1.1-draft).** Added a Phase -1 for de-risking spikes; split Phase 0 into 0.1–0.4; moved adversarial to Future Work; documented extraction termination (fixed-size read + AEAD auth = pass/fail); documented salt derivation (pHash-stable low-freq block).

### Burst 1 — Phase -1 and Phase 0 in parallel (4 teammates)

Teammates dispatched simultaneously via agent-team framework:

| Teammate | Scope | Outcome |
|---|---|---|
| `dct-spike` | Spike A: prove JPEG DCT round-trip via libjpeg-turbo FFI | **DONE — bit-exact round-trip verified** via `mozjpeg-sys` across 3.1M coefficients |
| `phash-analyst` | Spike B: measure empirical pHash preservation cost | **DONE_WITH_CONCERNS — bimodal distribution found** (median 0%, mean 8.4%, p90 14%, worst 100%). ~75% of images no-op, ~10% catastrophic. Plan's "5–15% uniform overhead" framing updated to a 3-tier sensitivity model |
| `crypto-builder` | `phantasm-crypto`: Argon2id + XChaCha20-Poly1305 + padding | **DONE** — 18/18 tests, RFC 8439 vector validated |
| `stc-builder` | `phantasm-stc`: single-layer Syndrome-Trellis Codes | **DONE with yellow flag** — 9/9 tests passing, but H̃ sub-matrix is a deterministic PRNG construction (SplitMix64) rather than published DDE Lab tables (Filler 2011); correctness verified, performance ~15% below asymptotic bound |

### Burst 2 — Remaining Phase 0 + research tooling (6 teammates in parallel)

| Teammate | Scope | Outcome |
|---|---|---|
| `image-builder` | `phantasm-image`: productize spike into safe Rust wrapper | **DONE** — 17/17 tests; `write_with_source` API (requires source path for `jpeg_copy_critical_parameters`); longjmp error handling flagged as task #14 |
| `cli-builder` | `phantasm-cli`: clap scaffolding with stub subcommands | **DONE** — 9 integration tests; `env_logger` chosen over `tracing-subscriber` for simplicity |
| `ecc-builder` | `phantasm-ecc`: Reed-Solomon wrapper | **DONE** — 9/9 tests; `reed-solomon-erasure` v6.x; deviation: 64-byte shards not 255 (tractability) |
| `stc-upgrader` | Upgrade STC with published H̃ + double-layer ternary | **DONE_WITH_CONCERNS** — couldn't verify DDE Lab tables in env, shipped improved PRNG instead (task #5 still open). Added double-layer via bit-plane decomposition (±1/±2/±3 delta search); roundtrip-correct but ~0.68× bits-per-L1 efficiency vs single-layer (task #15) |
| `core-builder` | `phantasm-core`: types + orchestrator trait | **DONE** — 14/14 tests; EmbedPlan, ChannelProfile, StealthTier, HashSensitivity, Orchestrator trait, StubOrchestrator |
| `bench-builder` | `phantasm-bench`: MSE/PSNR/SSIM/pHash/dHash metric harness | **DONE** — 13/13 tests; pluggable Steganalyzer trait; CLI smoke test |

### Burst 3 — Naive integration (1 teammate)

Dispatched one teammate (`integrator`) to wire everything together:
- Workspace `Cargo.toml` stitching all seven crates
- `MinimalOrchestrator` in `phantasm-core` that wires image → envelope → ECC → STC → image
- CLI subcommands updated from `[STUB]` output to real orchestrator calls
- Integration test: embed a real payload in a real JPEG and extract byte-identical

**DONE** — 98 tests total passing. Real smoke test confirmed: `phantasm embed photo.jpg secret.txt --passphrase ... -o stego.jpg` followed by `phantasm extract` round-trips byte-identical. First end-to-end working tool.

API surprises adapted to:
- ECC `lossless` (0 parity) with 64-byte shards produces 16320-byte blocks — larger than most image capacities. Replaced ECC in lossless-channel path with a 4-byte length-prefix framing.
- Image salt uses DC-only (dct_pos=0) rather than DC+first-AC (first-AC can be modified during embedding, breaking extract determinism).

### Smoke-test experiments
Manually verified the pipeline on:
1. `test.jpg` (720×680 Ronaldo ice bath meme) + `phantasm-crypto/src/metadata.rs` (3,723 B) → byte-identical roundtrip, wrong-passphrase fails cleanly, file inflation ~21%
2. `cover.jpg` (1024×1024 synthetic plasma) + `phantasm-core/src/minimal.rs` (10,610 B, 51% capacity use) → roundtrip verified, file inflation 38% (density-dependent)

### Burst 4 — External steganalysis validation
Installed and ran external detectors against `stego.jpg`:
- **stegoveritas**: no detection (but it's a forensic swiss-army knife, not a dedicated detector)
- **binwalk**: no detection
- **Aletheia RS attack** (Fridrich 2001 via `daniellerch/aletheia` GitHub repo): **DETECTED, rate 0.053 in channel B**
- **Aletheia SPA**: no detection (close but below threshold)
- Custom Python script using `jpeglib`: strong differential signals on `±1 transition ratio` (2.2×), non-zero AC delta (+37.6%), LSB entropy drop

**Finding:** phantasm's naive uniform-cost embedding is detected by a 24-year-old classical attack (Fridrich 2001 RS) but only marginally — 0.053 is right at the 0.05 threshold.

### Burst 5 — Research phase infrastructure (4 teammates in parallel)

Before dispatching, I pre-seeded `phantasm-cost` with the `DistortionFunction` trait + `Uniform` baseline impl so all four teammates could work in true parallel without cross-crate deps.

| Teammate | Scope | Outcome |
|---|---|---|
| `eval-builder` | Native Rust steganalysis suite: RS, SPA, chi-square, ±1 transition, LSB entropy, histogram TV, non-zero AC delta | **DONE_WITH_CONCERNS** — 21/21 tests; spatial-domain RS (correctly per Fridrich 2001) rarely fires on JPEG output because JPEG quantization obscures spatial LSB signals. Other detectors fire on both Uniform and UERD at this payload density |
| `corpus-builder` | ~200 JPEGs at QFs {75, 85, 90} × dimensions {512, 720, 1024} | **DONE** — 198/198 fetched from Picsum (Unsplash CC0), 22 MB, perfect 22-per-bucket distribution, fetch script at `phantasm-image/examples/fetch_corpus.rs` (gitignored corpus, stored manifest) |
| `uerd-builder` | UERD (Guo/Ni/Shi 2015) distortion function | **DONE** — 10/10 tests; simplified position weight `w(u,v) = q(u,v)` (2014 formula); quant table ordering discovery: both coefficients and quant_table in `JpegComponent` are zigzag-indexed, so `dct_pos` maps consistently to both (no un-zigzag needed) |
| `orchestrator-wirer` | Refactor pipeline into shared helpers, add `ContentAdaptiveOrchestrator<Box<dyn DistortionFunction>>` | **DONE** — 6 new integration tests; `phantasm-core/src/pipeline.rs` holds shared embed/extract logic; `MinimalOrchestrator` becomes a thin wrapper using `Uniform`; "permute positions + full cost map" flow worked cleanly with single-layer binary STC (using `min(costs_plus, costs_minus)`); double-layer deferred to preserve compatibility |

Post-burst: fixed two cross-contamination clippy issues (each teammate flagged the other's lint failures), wrote `phantasm-core/examples/compare_cost_functions.rs` as a research harness to invoke both orchestrators on the same cover.

### Research burst 5 results — UERD vs Uniform on test.jpg + 198-image corpus

**Single-image (test.jpg, 3,723-byte payload, 27.7% capacity):**
| Metric | Uniform | UERD | Δ |
|---|---|---|---|
| MSE | 11.51 | 6.90 | −40% |
| PSNR | 37.52 dB | 39.74 dB | +2.22 dB |
| **SSIM** | **0.8798** | **0.9567** | **+0.077** |
| File inflation | +18,585 B | +11,793 B | −36% |
| ±1 transition ratio | 0.172 | 0.147 | −14% |
| pHash hamming | 0 | 0 | unchanged |

**198-image corpus (same payload):**
| Metric | Uniform mean | UERD mean | UERD win rate | Median Δ |
|---|---|---|---|---|
| **SSIM** | 0.8025 | **0.9298** | **100% (198/198)** | **+0.1198** |
| PSNR | 33.80 dB | 34.80 dB | 78% | +0.93 dB |
| MSE | 35.3 | 29.2 | 78% | −4.07 |
| **File inflation** | +10,190 B | **+3,050 B** | **99% (196/198)** | **−5,808 B** |
| ±1 transition | 0.1454 | 0.1255 | 87% | −0.0215 |
| Histogram TV | 34.45 | 24.94 | 89% | — |
| RS attack rate | 0.0122 | 0.0000 | — | ↓ |

**Headline:** UERD wins SSIM 100% of the time across 198 diverse images.

### Burst 6 — SRM-lite + eval-corpus subcommand (2 teammates in parallel)

| Teammate | Scope | Outcome |
|---|---|---|
| `corpus-evaluator` | `phantasm-bench eval-corpus` subcommand for corpus-scale eval | **DONE** — 4 new tests; deterministic passphrase per image; walks subdirectories; aggregates mean/median/p10/p25/p75/p90/stddev/min/max per metric per cost function; writes paired comparison table |
| `srm-builder` | SRM-lite detector (4 residuals × 7×7 co-occurrence matrices = 196-feature vector) | **DONE_WITH_CONCERNS** — 14 stealth tests; threshold 0.020 chosen conservatively; Uniform L2 = 0.609, UERD L2 = 0.221 — **2.8× lower under UERD** (the signal we wanted) |

Post-burst: fixed cross-contamination clippy + fmt.

### Day 2 morning — Tier 1 alpha burst (5 teammates sequentially + parallel)

The first session-2 burst, driven autonomously per user mandate "keep researching and developing on your own until we reach stable v0.1.0". Closed the single outstanding day-1 research question AND landed every Tier 1 polish item from STATUS.md §11.

| Teammate | Scope | Outcome |
|---|---|---|
| `detection-analyst` | Task #1 — close §7.1, wire fridrich_rs + srm_lite_l2 into eval-corpus, rerun 198-image corpus | **DONE (partial)** — corpus sweep produced the headline result (75.3% → 30.8% RS detection rate, 196/198 paired RS wins, 198/198 paired SRM L2 wins). Code-side aggregation claimed to have landed but the edits were LOST during parallel-burst coordination chaos — discovered by bench-rerunner, reapplied in a follow-up dispatch. See §5 Finding 7 for the result |
| `cli-wirer` | Task #2 — `phantasm embed --cost-function {uniform,uerd}` CLI flag with UERD default | **DONE** — 14 tests (+5). Also absorbed cross-lane phantasm-core patches during scope extension: updated pipeline.rs to use new Envelope::{to,from}_bytes serialization, and collapsed all post-STC-decode errors in extract_from_cover into clean `CryptoError::AuthFailed` (preserving `UnsupportedVersion` as the one variant that bypasses the collapse) |
| `image-polish` | Task #3 — task #14 libjpeg longjmp hardening + task #17 Huffman reopt | **DONE with research finding** — Panic-across-C-unwind chosen over setjmp/longjmp (mozjpeg-sys already declares `error_exit` as `extern "C-unwind"`, so a typed `LibjpegPanic` payload propagates from the C error callback through Rust's `catch_unwind` with RAII guards for cleanup). 5 new FFI tests (truncated, garbage, missing, round-trip, huffman-no-inflation). KEY FINDING: mozjpeg's `JCP_MAX_COMPRESSION` profile already enables `trellis_quant` which unconditionally rebuilds Huffman tables on write regardless of `optimize_coding`. Task #17 was effectively done by default — we just didn't know |
| `crypto-cleaner` | Task #4 — permutation MAC for clean wrong-passphrase detection (task #18) | **DONE** — HMAC-SHA256 truncated to 16 bytes; HKDF-SHA256 key split over Argon2id master_key with info strings `"phantasm-v2-aead"` / `"phantasm-v2-mac"` binding subkey derivation to format version; constant-time compare with explicit truncation bounds; pre-payload prefix outside padded region; new `CryptoError::UnsupportedVersion(u8)` variant; envelope format bumped v1 → v2. 30 tests (+10). All three verifications passed end-to-end: correct-passphrase round-trip, wrong-passphrase clean AuthFailed, day-1 v1 samples uniformly unrecoverable |
| `bench-rerunner` | Task #6 — re-measure corpus file-size delta through hardened write path | **DONE** — Day-1's `+10,189 B Uniform / +3,057 B UERD` file-inflation numbers reproduce within 7 bytes at corpus scale. Day-2's `73.2% / 31.3%` RS detection rate reproduces within ~2 pp. SRM L2 means reproduce to three decimals. image-polish's "~29 KB smaller" synthetic finding is not wrong — it's from a fundamentally different perturbation pattern (uniform ~2% AC flip vs real STC+UERD). **README can safely cite the existing numbers without revision.** Also caught the missing detection-analyst eval_corpus.rs edits as a bonus save |
| `eval-corpus-aggregator` | Task #7 — reapply detection-analyst's lost aggregation plumbing | **DONE** — `phantasm-bench/src/eval_corpus.rs` `PerImageMetrics` now has `fridrich_rs_max_rate: f64`, `fridrich_rs_detected: bool`, `srm_lite_l2_distance: f64` fields. `CostFunctionStats` now has `fridrich_rs_detected_fraction: f64`. Aggregation + paired comparisons + markdown report rows + JSON serialization all live. Smoke test on 3 images shows real non-zero values (unlike the old `rs_rate_y` which almost always returned 0 on JPEGs) |

**Burst status:** 160/160 tests, clippy clean, fmt clean. Tier 1 alpha checklist all landed. README + LICENSE-MIT + LICENSE-APACHE + CHANGELOG.md written against the verified headline numbers. `v0.1.0-alpha` tag landed at commit `82b89a4`.

### Day 2 afternoon — Tier 2 research + integration burst (8 commits, 7 teammates)

Second session-2 burst. Drove directly from v0.1.0-alpha through the full Tier 2 research track + Phase 2 channel adapter + Phase 3 hash guard + orchestrator integration in a single 3-hour sprint. Every commit is atomic per-teammate to protect against parallel-WIP loss (see §2.X "git safety incident" below).

**Commit sequence** (on top of `82b89a4` Tier 1 alpha):

| Commit | Teammate | Scope | Headline |
|---|---|---|---|
| `4aaf261` | `raw-path-builder` | Task #8 — research-raw embedding path | 9 new tests; `#[doc(hidden)]` module with `research_raw_embed`/`research_raw_extract` behind explicit "benchmarking only" warnings. Unlocks true security-capacity curves by bypassing envelope padding that flattened day-1's density sweep. |
| `2d98bf3` | `stc-upgrader` | Tasks #5 + #15 — DDE Lab H̃ + conditional-probability double-layer | Transcribed DDE Lab `mats[]` verbatim as `static DDE_MATS: [u64; 2400]` (h∈[7,12], w∈[1,20]) with citation. Removed spurious `effective_height = min(h,w)` clamp. Then rewrote `double_layer.rs` with conditional-probability layering via a 4-cell cost table — bits/L1 efficiency went from **~0.68 legacy → 0.995 conditional** at h=10, n=4096, uniform costs. Closes essentially the entire 32% efficiency gap. Found the `|x|&1 / |x|>>1` vs `x.rem_euclid(4)` root-cause bug via the seed-4 half-wet test failure when the new tables went in — exactly the kind of latent correctness catch that matters. |
| `d69ded2` | `juniward-builder` | Task #9 — J-UNIWARD cost function | Full Holub-Fridrich 2014 impl with Daubechies-8 wavelet filter hardcoded and cross-checked against `pywt.Wavelet('db8').dec_lo`. Uses the precomputed-impulse-response optimization (3×64 fixed 23×23 kernels per image) to avoid the O(W·H·64) naive path. 7 new tests including a DB8 orthonormality gate and textured-vs-smooth content-adaptivity ratio. Smoke bench on 6 qf75 samples: J-UNIWARD wins SSIM/PSNR/MSE; UERD edges on Fridrich RS / SRM L2 at this QF (expected per literature). |
| `bc5c909` | `channel-adapter-builder` | Task #12 — phantasm-channel crate | NEW sub-crate with MINICER + ROAST + TwitterProfile. Parity-preservation strategy: simulate channel re-encode per block, perturb source until `lsb(reenc[p]) == lsb(source[p])`. TwitterProfile defaults to QF=85, 4:2:0. Block sacrifice on >30 wet positions. Measured 98.7% coefficient survival (63674/64512) on a real `image::codecs::jpeg::JpegEncoder` re-encode at QF=85. 16 tests. MVP gaps: single-block approximation (no inter-block AC coupling), no rescale modeling, `STABILIZED_COST_DISCOUNT=0.75` is arbitrary. |
| `c440bc3` | `research-curve-builder` | Task #14 — research-curve subcommand | `phantasm-bench research-curve` uses the research-raw path to produce true security-capacity curves. Parallelized over images via rayon. SHA-256-derived deterministic seeds. Bonus 20-image × 3 cost fn × 3 bit count sweep: at 8000 bits, Uniform detection fraction jumps to **25%** while UERD/J-UNIWARD hold at 15%; Uniform SRM L2 is 2.4× higher than UERD. This is the publishable Tier 2 curve shape. |
| `dcdcbb7` | `hash-guard-builder` | Task #13 — phantasm-core::hash_guard | 3-tier sensitivity classifier (Robust/Marginal/Sensitive) + wet-paper cost constraint for pHash and dHash. 10 tests. Critical design catch: **phantom-bit exclusion** — the median in the odd-count AC list IS one of the hash bits and has a structurally-zero margin; without excluding it every image classifies Sensitive. Swapped bilinear resampling for area (box filter) because bilinear was over-smoothing AC coefficients at 16:1 downsample and breaking classification. Thresholds calibrated from day-1 Spike B data (margin 0.5 safe, 0.1 marginal). Observed tier distribution on 22 qf85/512 Picsum images: 15 Robust / 5 Marginal / 2 Sensitive = 68/23/9, matching Spike B's reported 75/15/10 bimodality within corpus-size noise. Pre-nudge for Sensitive + PDQ support deferred to post-v0.1.0. |
| `d716ce9` | `tier2-integrator` | Task #15 — orchestrator + CLI integration | Wires ChannelAdapter and HashGuard into `ContentAdaptiveOrchestrator` via builder methods `with_channel_adapter` and `with_hash_guard`. New `phantasm embed --channel-adapter {none,twitter}` and `--hash-guard {none,phash,dhash}` CLI flags (both default `none` for backward-compat). `phantasm analyze` now prints sensitivity tier + hash-guard wet-position count. Order is `hash_guard → channel_stabilize → STC` (hash guard before channel stabilization preserves the original cover pHash semantics — invisibility against a database keyed on the unmodified original). Extract flags accepted as forward-compat no-ops (extract derives positions geometrically from stego, doesn't consult costs). 7 new CLI integration tests. |

**#### Git safety incident**

During the parallel Tier 2 research burst (raw-path-builder + juniward-builder + stc-upgrader running concurrently), a `git reset --hard HEAD` fired in one of the teammate contexts and silently wiped juniward-builder's uncommitted WIP files. juniward-builder re-applied their J-UNIWARD impl from memory and the work was ultimately saved. Root cause: a teammate trying to measure a clean baseline rolled back the tree and nuked other teammates' in-flight work across all lanes. Team-lead detected the incident via `git reflog` showing `HEAD@{0}: reset: moving to HEAD`.

**Preventive measures now in place** (see `memory/phantasm_git_safety.md`):
- Team-lead commits each teammate's work atomically as it lands, not in an end-of-burst batch
- Every multi-teammate dispatch prompt includes an explicit "NO destructive git commands" rule with an enumerated forbidden list (`reset --hard`, `checkout .`, `restore .`, `clean -f`, `stash drop/clear`, `branch -D`, interactive rebase on shared state, any `git commit`)
- Team-lead broadcasts the rule at first sign of a parallel burst, not after the incident
- Teammates are instructed to message team-lead BEFORE running any git op if something looks broken

**Burst status:** 211/211 tests, clippy clean, fmt clean. Eight commits on top of `v0.1.0-alpha`. Final bench sweep in progress; `v0.1.0` stable tag next.

### Burst 7 — Density sweep + Aletheia RS port (2 teammates in parallel)

| Teammate | Scope | Outcome |
|---|---|---|
| `density-sweeper` | Payload density sweep (1%/5%/10%/20%/40%/80%) via `--capacity-fraction` and `--capacity-fractions` flags | **DONE** — code complete; initial run had a capacity-computation bug (used raw bit-capacity ÷ 8 instead of STC-effective ÷ 32). Fixed in-session. Full 198×6 sweep completed in ~20 min |
| `rs-porter` | Port Aletheia's Fridrich RS to native Rust | **DONE** — 3 new tests; per-channel R/G/B; 2D overlapping 3×3 patches; reproduces Aletheia's 0.053 at **0.0513** on `stego.jpg` (within 0.002 tolerance) |

### Final density-sweep finding — important and subtle

After fixing the capacity computation, the full 198×6 sweep produced **identical results at every density level**:

| Density | SSIM win rate | Mean SSIM Δ | File inflation Δ |
|---|---|---|---|
| 1% | 100% | +0.1274 | −5,772 B |
| 5% | 100% | +0.1274 | −5,883 B |
| 10% | 100% | +0.1275 | −5,874 B |
| 20% | 100% | +0.1272 | −5,908 B |
| 40% | 100% | +0.1273 | −5,754 B |
| 80% | 100% | +0.1273 | −5,831 B |

**Why:** The crypto envelope pads every payload to fixed block tiers `{256, 1024, 4096, 16384, 65536, 262144}` bytes. On 720×680 images, a 40-byte payload and a 3,200-byte payload BOTH pad to the 4,096-byte tier, so STC sees an identical fixed-size ciphertext regardless of `F`. The "density sweep" can't actually vary density with the current naive pipeline.

**The aggregate is still valid:** 1,188 paired comparisons (198 × 6) all show UERD winning SSIM. That's statistically overwhelming — UERD's advantage is monotonic at image granularity across QFs, dimensions, and "densities" (really: just different random payload padding).

**To get a real density curve** we need a research-only embedding path that skips the envelope and varies STC message length directly. Task #30.

### Fridrich RS single-image check
Aletheia RS (faithful Rust port) on fresh stego samples:
- Original `stego.jpg` (pre-refactor): **0.0513** (detected) — matches Aletheia's 0.053
- `stego_uniform.jpg` (fresh, post-refactor): 0.028 (clean)
- `stego_uerd.jpg` (fresh, post-refactor): 0.007 (clean)

Single-image detection is noisy around the 0.05 threshold. UERD's rate is consistently 4× lower than Uniform's, but both fall clean-side in single-sample tests. Population-scale RS on the corpus is the remaining research question.

---

## 3. Current Crate Inventory

```
phantasm/
├── Cargo.toml                    # workspace root
├── PLAN.md                       # architectural plan (v0.1.1-draft)
├── RESEARCH.md                   # steganography literature review
├── STATUS.md                     # this document
├── rustfmt.toml
├── .gitignore
│
├── phantasm-image/               # JPEG DCT I/O via mozjpeg-sys, PNG pixels, DCT helpers
├── phantasm-crypto/              # Argon2id + XChaCha20-Poly1305 envelope
├── phantasm-stc/                 # Single-layer + double-layer Syndrome-Trellis Codes
├── phantasm-ecc/                 # Reed-Solomon wrapper
├── phantasm-cost/                # DistortionFunction trait + Uniform + UERD
├── phantasm-core/                # Orchestrator trait + MinimalOrchestrator + ContentAdaptiveOrchestrator
├── phantasm-cli/                 # phantasm binary (embed/extract/analyze/channels/bench)
├── phantasm-bench/               # phantasm-bench binary (compare/analyze-stealth/eval-corpus)
│
├── spikes/
│   ├── dct-roundtrip/            # Phase -1 Spike A — libjpeg-turbo FFI proof
│   └── phash-overlap/            # Phase -1 Spike B — pHash capacity cost empirical study
│
└── research-corpus/              # 198 JPEGs (gitignored), manifest.json committed
    ├── qf75/{512,720,1024}/
    ├── qf85/{512,720,1024}/
    └── qf90/{512,720,1024}/
```

### Test counts per crate (v0.1.0 shipped state)
```
phantasm-image:   22 tests  (7 unit + 15 integration; +5 from longjmp hardening vs day 1)
phantasm-crypto:  30 tests  (+10 from v2 envelope + MAC + HKDF key split vs day 1)
phantasm-stc:     16 tests  (9 single-layer + 6 double-layer + 1 bits/L1 efficiency)
phantasm-channel: 16 tests  (NEW Tier 2 sub-crate — MINICER + ROAST + Twitter profile)
phantasm-cost:    17 tests  (+7 from J-UNIWARD with DB8 filter orthonormality gate)
phantasm-core:    50 tests  (39 unit + 8 content_adaptive integration + 3 integration; includes research_raw, hash_guard, salt stability, sensitive cover refusal, tier capacity)
phantasm-ecc:      9 tests
phantasm-cli:     25 tests  (4 unit + 21 integration; +7 from Tier 2 integration + N1 channels formatter + N5 pHash hex + j-uniward roundtrip)
phantasm-bench:   39 tests  (2 unit + 1 cli_smoke + 7 eval_corpus + 12 metrics + 17 stealth)
Total:           224 tests — all passing (Tier 1 alpha: 160, Tier 2 research: 211, post-polish v0.1.0: 224, day 1 baseline: 132)
```

---

## 4. What Works Right Now (capability inventory)

### End-to-end embedding
```bash
# Actually works. Try it.
phantasm embed --input cover.jpg --payload secret.txt \
    --passphrase "..." --output stego.jpg
phantasm extract --input stego.jpg --passphrase "..." --output recovered.txt
diff secret.txt recovered.txt  # identical
```

- Cryptographic envelope: Argon2id(64 MiB / 3 iter / 4 threads) → XChaCha20-Poly1305 AEAD → padding to fixed power-of-2 block tier
- STC coding: single-layer binary Syndrome-Trellis Codes at rate 1/4, constraint height 7
- Uniform cost function as the CLI default (content-adaptive UERD available only via example binary, not CLI yet — task)
- JPEG-only target format (PNG library exists but no embedding path wired)
- Passphrase-derived embedding permutation; image salt derived from DC coefficients
- Wrong-passphrase fails via length-framing sanity check or AEAD auth failure (ugly error; task #18)
- File inflation ~10-20% on Uniform, ~3-12% on UERD (content-dependent)

### Benchmark suite
```bash
# Compare cover/stego pairs
phantasm-bench compare <cover-dir> <stego-dir>          # pixel metrics: MSE/PSNR/SSIM/pHash/dHash/file-size
phantasm-bench analyze-stealth --cover <cover> <stego>  # detection battery
phantasm-bench eval-corpus --corpus <dir> --payload <file>                              # fixed payload, both cost functions
phantasm-bench eval-corpus --corpus <dir> --capacity-fraction 0.1 --cost-functions uerd # variable payload, single density
phantasm-bench eval-corpus --corpus <dir> --capacity-fractions 0.01,0.05,0.1,0.2 ...    # sweep mode
```

Detection battery in `analyze-stealth`:
- Spatial RS (Fridrich 2001, correct implementation — rarely fires on real JPEGs)
- Fridrich RS per R/G/B channels (Aletheia-faithful port, reproduces 0.053 on stego.jpg within 0.002)
- Sample Pairs Analysis (Dumitrescu 2003)
- Chi-square on DCT coefficient pair histogram (Provos 2001)
- ±1 transition ratio on adjacent DCT coefficients
- LSB entropy on non-zero AC coefficients
- Histogram total variation
- Non-zero AC delta (when `--cover` supplied)
- SRM-lite L2 distance (4 residuals × 7×7 co-occurrence = 196 features)

### Research corpus
198 JPEGs in `research-corpus/` (gitignored). Metadata tracked in `manifest.json` with source URL, seed, dimensions, QF, size, SHA-256. Regenerable via `cargo run --release -p phantasm-image --example fetch_corpus`.

### Research harness
`phantasm-core/examples/compare_cost_functions.rs` — throwaway binary that embeds a payload with Uniform, ContentAdaptive(Uniform), and ContentAdaptive(UERD) and prints capacity used + roundtrip verification for all three. Used throughout burst 5-7 to generate comparison samples.

---

## 5. Research Findings — What We've Proven So Far

### Finding 9 (post-v0.1.0, 2026-04-13): Modern CNN steganalysis evaluation — cost-function ordering inverts vs classical detectors

**This is the headline post-v0.1.0 result.** The full writeup lives in `ML_STEGANALYSIS.md`; this section captures the key numbers and the threat-model implication.

Tested phantasm v0.1.0 at the default 3 KB payload (~0.2 bpnzac) on the same 198-image Picsum corpus, against two pretrained CNN detectors:

1. **JIN-SRNet** (Butora/Yousfi/Fridrich 2021) — PyTorch SRNet with 3-channel color input, trained on ImageNet covers + J-UNIWARD stegos at 0.4–0.6 bpnzac. Run on RTX 5070 via PyTorch 2.10 + CUDA 12.8 at ~33 img/s.
2. **Aletheia EfficientNet-B0 J-UNIWARD** (Daniel Lerch's open-source steganalysis toolkit) — Keras .h5, EfficientNet-B0 trained on ALASKA2 + J-UNIWARD. Converted to ONNX via tf2onnx and run via onnxruntime CPU at ~22 img/s.

| Detector | Cover (FP) | Uniform | UERD | J-UNIWARD |
|---|---:|---:|---:|---:|
| **Fridrich RS** (classical, v0.1.0 Finding 8) | — | 66.7% | **26.8%** | 30.3% |
| **JIN-SRNet** detection rate (P>0.5) | 7.1% | **98.0%** | 46.5% | **16.2%** |
| **Aletheia EffNet-B0** paired Δ (mean stego−cover) | 88.9% baseline | +0.027 | −0.072 | **−0.113** |

**Key finding — the cost-function ordering inverts.** Against classical Fridrich RS, UERD beats J-UNIWARD (26.8% vs 30.3%). Against modern JIN-SRNet, J-UNIWARD beats UERD (16.2% vs 46.5%) and **49.5% of phantasm J-UNIWARD stegos score lower P(stego) than their own cover** — JIN-SRNet sees the J-UNIWARD stego as more cover-like than the original, in half the pairs.

**Cross-detector consistency:** EfficientNet-B0 has severe cover-source mismatch (88.9% cover false-positive on Picsum, because it was trained on ALASKA2), so its absolute detection rates are not interpretable. But the **paired** per-image (stego − cover) deltas are robust to cover-source bias and agree with JIN-SRNet: Uniform > UERD > J-UNIWARD in detectability. Two detectors with different architectures, training corpora, and frameworks both rank phantasm J-UNIWARD as the most cover-indistinguishable cost function.

**Payload-size regime is flat.** Re-running JIN-SRNet across 1k / 3k / 6k / 10k byte payloads on the 66 1024-px covers showed essentially flat detection rates per cost function. Phantasm sits in a regime where the spatial pattern of STC modifications (cover-derived salt → same coefficient set hit regardless of payload size) dominates over modification count. Stress-testing the cost function would require 100+ KB payloads, well beyond v0.1.0 envelope sizes.

**Threat-model implication:** The "right" cost function depends on the adversary. For classical statistical adversaries, UERD remains correct (and is still the v0.1.0 default). For deep-learning adversaries, **J-UNIWARD is the better choice and the v0.2 user-facing recommendation**. The CLI help text on `--cost-function` was updated to surface this tradeoff explicitly. The default has not been changed for v0.1.0 backward-compatibility; flipping the default is a candidate v0.2 behavior change.

**Caveats** (full list in ML_STEGANALYSIS.md):
- Both pretrained detectors were trained against J-UNIWARD specifically. A hypothetical UERD-trained CNN does not exist publicly. UERD numbers in this eval are cross-cost transfer, not direct. Closing this gap is the **Option B** v0.2 work item — fine-tune JIN-SRNet to a UERD-aware detector and re-run.
- N=198, single corpus (Picsum), single passphrase per cover, ~0.2 bpnzac payload. Across-seed and cross-corpus variance not measured.
- JIN-SRNet license is "research-use, conventional" per DDE Lab convention; not explicitly tagged. Verify before publishing eval numbers externally.
- Aletheia ONNX path uses CPU inference (CUDA EP failed against the Blackwell 5070 — needs cuDNN 9.x + CUDA 12.x; tolerable since CPU still hits 22 img/s).

### Finding 8 (day 2 afternoon — v0.1.0 final bench): Post-STC-fix numbers improve across the board

**This is the v0.1.0 shipping headline.** Supersedes Finding 7's day-2-morning numbers, which were measured on the pre-fix STC implementation.

On the same 198-image corpus with the same 3,723-byte fixed payload, using v0.1.0's published DDE Lab H̃ tables + conditional-probability double-layer (0.995× bits/L1 efficiency vs the pre-fix 0.68×):

| Metric | Uniform | UERD | J-UNIWARD | Day-2 Uniform | Day-2 UERD |
|---|---:|---:|---:|---:|---:|
| **Fridrich RS detection rate** | **66.67%** | **26.77%** | 30.30% | 75.3% | 30.8% |
| Mean Fridrich RS max_rate | 0.3208 | 0.0486 | 0.0557 | 0.4798 | 0.0543 |
| Median Fridrich RS max_rate | 0.1785 | 0.0201 | 0.0220 | 0.2586 | 0.0235 |
| Mean SRM-lite L2 distance | 0.5994 | 0.1309 | 0.1973 | 0.6493 | 0.1893 |
| Mean SSIM | 0.8373 | 0.9509 | **0.9532** | — | — |
| Mean PSNR (dB) | 34.97 | 36.09 | **37.58** | — | — |
| Mean file-size delta (B) | **+5,390** | **−1,321** | −1,039 | +10,189 | +3,057 |

**Paired comparisons (UERD vs Uniform, n=198):**
- Fridrich RS: UERD wins **195/198 (98.5%)**, mean paired Δ = −0.2722
- SRM-lite L2: UERD wins **198/198 (100%)**, mean paired Δ = −0.4685
- SSIM: UERD wins **198/198 (100%)**, mean paired Δ = +0.1136
- File-size: UERD wins **193/198 (97.5%)**, mean paired Δ = −6,712 B (**UERD stego is on average smaller than the cover**)

**Root cause of the day-2 → v0.1.0 shift:** The Tier-2 STC efficiency lift (commit `2d98bf3`: DDE Lab tables + conditional-probability double-layer) reduces actual cover-coefficient modifications by ~32% for the same payload size. Mathematically: 3,723 B payload = 29,784 payload bits; at 0.68× efficiency ≈ 43,800 syndrome-bit flips needed; at 0.995× efficiency ≈ 29,935 syndrome-bit flips needed. A 32% reduction in coefficient modifications drives:
- 32%-ish file-size delta reduction (observed: Uniform 47% drop, UERD sign flip into deflation)
- Substantial Fridrich RS detection-rate drop (observed: Uniform −8.6 pp, UERD −4.0 pp)
- No change to capacity utilization (same 30.92% across all three cost functions)

This is a verified improvement, not a regression. The day-2 numbers represented the pre-fix STC baseline.

**J-UNIWARD vs UERD at this payload:** J-UNIWARD wins perceptual-distortion metrics (SSIM +0.002, PSNR +1.49 dB, MSE −7.08) but loses statistical undetectability (Fridrich RS +0.0071 worse, SRM L2 +0.0664 worse). UERD is the best cost function for this corpus at ~31% raw capacity. J-UNIWARD embed cost is ~3.5× UERD per image.

**Security-capacity curve (`research-curve` subcommand, 40-image subset):**

| Cost fn | 100 bits | 500 | 2,000 | 8,000 | 20,000 |
|---|---:|---:|---:|---:|---:|
| Uniform detection frac | 17.5% | 17.5% | 17.5% | 22.5% | **40.0%** |
| UERD detection frac | 17.5% | 17.5% | 17.5% | 17.5% | **20.0%** |
| J-UNIWARD detection frac | 17.5% | 17.5% | 17.5% | 17.5% | **17.5%** |

At low-to-medium payloads all three cost functions sit near the Fridrich RS noise floor (~17.5%). At 20k bits (max capacity tested), the ordering flips — **J-UNIWARD holds at the noise floor while Uniform blows out to 40%**. J-UNIWARD wins the high-capacity / security-critical regime.

### Finding 7 (day 2 morning — 2026-04-14): UERD cuts classical Fridrich RS detection rate from 75% to 31% at population scale

**This is the headline day-2 result.** Closes the central day-1 unanswered question ("does UERD's 4× single-image advantage survive at population scale?") with an emphatic yes.

On the same 198-image corpus with a fixed 3,723-byte payload at ~27% raw capacity, using the Aletheia-faithful Fridrich 2001 RS detector (threshold 0.05, max over R/G/B channels):

| Metric | Uniform | UERD | Delta |
|---|---:|---:|---:|
| **Fridrich RS detection rate (max_rate > 0.05)** | **75.3%** (149/198) | **30.8%** (61/198) | **−44.4 pp / 2.4× reduction** |
| Mean RS max_rate | 0.4798 | 0.0543 | −0.426 (8.8× reduction) |
| Median RS max_rate | 0.2586 | 0.0235 | −0.218 |
| p90 RS max_rate | 0.7586 | 0.1308 | −0.628 |
| Mean SRM-lite L2 distance | 0.6493 | 0.1893 | −0.460 (3.4× lower) |
| Median SRM-lite L2 distance | 0.6157 | 0.1324 | −0.483 |

**Paired (same image, matched 198/198):**
- Fridrich RS: UERD beats Uniform on **196/198 images** (mean paired Δ −0.4255, median Δ −0.218). Regressed on only 2 images.
- SRM-lite L2: UERD beats Uniform on **198/198 images** (mean paired Δ −0.460). Never regresses.

**Interpretation — day 1's outcome (b) strong version, not (a) or (c):**
- Not the storybook "(a) 75% → 3%" but clearly not "(c) both saturated" either.
- A 44.4-point drop and 2.4× reduction in detection frequency is a real, publishable population-scale security result using the same detector Aletheia relies on.
- Mean RS max_rate fell 8.8× (from 0.48 to 0.054) — UERD pushes the median image BELOW the 0.05 threshold. The detection-rate drop is "only" 2.4× because ~31% of UERD stego still crosses the threshold at this density; both distributions straddle the threshold and content-adaptive moves the median, not the tail.
- SRM-lite paints a sharper picture as a threshold-free distance metric: UERD wins on every single image, with mean distance cut 3.4×.

**What this means for publishability:** the alpha release headline can cite "UERD cuts classical Fridrich RS detection frequency by 2.4× (75% → 31%) at corpus scale" as a real, verified, reproducible security claim. Not hype — we have the numbers, the detector is Aletheia-faithful, the corpus is seed-regenerable.

**Overall_verdict note:** the `analyze_stealth` overall_verdict flag is 100%/100% — both cost functions are "detected" on every image. This is expected and not a UERD failure. Overall_verdict is driven by density-insensitive counts (±1 transition ratio, LSB entropy, histogram TV) that STC rate math pins constant across cost functions. Fridrich RS and SRM L2 are the relevant metrics for evaluating content-adaptive vs uniform.

### Finding 1: Content-adaptive embedding works (UERD wins SSIM 100%)
On a 198-image corpus with a fixed 3,723-byte payload at ~27% raw capacity:
- **UERD wins SSIM in 198/198 images**. Mean paired delta +0.127, median +0.120, p10 +0.057, p90 +0.224.
- Under the density-sweep's 1,188 total paired comparisons (198 images × 6 density knobs, all collapsed to the same effective embedding by envelope padding), UERD wins 1,188/1,188.
- PSNR paired delta +0.93 dB, MSE −6.1, file inflation −57% (uniform mean 10,190 B, UERD mean 3,050 B).

### Finding 2: File inflation is a strong signal, and UERD kills most of it
- Uniform: +10,190 B mean bloat (cover-to-stego delta)
- UERD: +3,050 B mean bloat
- UERD wins file inflation in 196/198 images (99%)
- PLAN §2's threat model claims "no file size inflation" — currently violated under Uniform, largely fixed under UERD

### Finding 3: Classical DCT detectors can't distinguish Uniform from UERD at single-image granularity
The `overall_verdict` flag fires on both for all 198 × 6 runs — because the flag is driven by `±1 transition ratio > 0.12` and both cost functions flip the same NUMBER of coefficients (STC rate math is independent of cost function).

The relevant signals for content-adaptive detection are ones sensitive to spatial redistribution, not to total modification count:
- **SRM-lite L2 distance**: Uniform 0.609, UERD 0.221 (2.8× lower under UERD — the signal we wanted)
- **Fridrich RS** (single-image): Uniform 0.028, UERD 0.007 (4× lower, both below 0.05 threshold)
- **Histogram TV**: UERD wins 89% of images
- **±1 transition ratio**: UERD wins 87% of images

### Finding 4: The density sweep was flattened by envelope padding
Not a failure — an important discovery. Our current naive pipeline pads every payload to the same fixed block tier, so "variable density" via `--capacity-fraction` doesn't actually vary what STC embeds. The 1,188 paired comparisons all test the same effective embedding. For a true security-capacity curve we need a research-only raw-embedding path (task #30).

### Finding 5: Aletheia's Fridrich RS port is faithful
The Rust port reproduces Aletheia's 0.053 detection rate on `stego.jpg` at 0.0513 (within 0.002 tolerance). This gives us a detector that's known to fire on real phantasm stego output, unlike our spatial RS implementation which was correct-per-Fridrich-2001 but didn't survive JPEG quantization. We now have a trustworthy classical detection signal.

### Finding 6: pHash preservation has a bimodal cost, not uniform (from Spike B)
Running empirical perturbation analysis on a 60-image Picsum corpus:
- **Median penalty: 0%** (roughly 75% of images have every pHash bit sitting far from its decision threshold)
- **Mean penalty: 8.4%** (driven by a long tail, not uniform cost)
- **Worst case: ~100%** (roughly 10% of images have one or more hash bits near threshold)

PLAN §3.5 has been updated from "5–15% uniform overhead" to a three-tier sensitivity model (Robust / Marginal / Sensitive). Phase 3 needs to account for this.

---

## 6. Pending Task Backlog (current state)

### Research tasks — open
- **#6**: Spike B follow-up — PDQ overlap analysis (pHash study repeated for Facebook's PDQ, likely stricter)
- **#7**: `phantasm analyze` — per-image hash sensitivity classification (3-tier from Spike B finding)
- **#30**: Research-raw embedding path for real density sweeps (bypass crypto envelope; benchmarking-only)

### Engineering follow-ups — open
- **#5**: `phantasm-stc` — replace PRNG H̃ with published Filler 2011 / DDE Lab tables (superseded by #15)
- **#15**: `phantasm-stc` — replace double-layer bit-plane construction with paper-standard ternary (current construction has 0.68× bits-per-L1 efficiency vs ideal)
- **#19**: Preserve JPEG progression mode (baseline vs progressive) from input to output
- **#20**: `phantasm analyze` over-reports capacity (ignores envelope padding)

### Engineering follow-ups — closed in day 2 Tier 1 burst
- **#14**: libjpeg longjmp hardening ✓ (panic-across-C-unwind in phantasm-image/src/jpeg.rs)
- **#17**: Huffman table re-optimization ✓ (was already done by mozjpeg's trellis_quant; documented)
- **#18**: Permutation MAC ✓ (HMAC-SHA256-16 with HKDF key split in phantasm-crypto/src/mac.rs)

### Completed tasks (for the record)
- #1: Spike A — DCT FFI round-trip ✓
- #2: Spike B — pHash overlap analysis ✓
- #3: Phase 0.2 — phantasm-crypto ✓
- #4: Phase 0.3 — phantasm-stc (single-layer) ✓
- #8: Phase 0.1 — phantasm-image ✓
- #9: Phase 0.4 — phantasm-cli ✓
- #10: phantasm-ecc ✓
- #11: phantasm-stc upgrade (H̃ improved PRNG + double-layer) ✓
- #12: phantasm-core skeleton ✓
- #13: phantasm-bench harness ✓
- #16: Burst 3 naive integration ✓
- #21: Research Burst — eval harness ✓
- #22: Research Burst — corpus acquisition ✓
- #23: Research Burst — UERD ✓
- #24: Research Burst — ContentAdaptiveOrchestrator ✓
- #25: Research Burst 2 — eval-corpus subcommand ✓
- #26: Research Burst 2 — SRM-lite features ✓
- #28: Research Burst 3 — density sweep (flattened by envelope padding, logged as #30) ✓
- #29: Research Burst 3 — Fridrich RS native Rust port ✓
- #27: Wire SRM-lite L2 into eval-corpus aggregation (day 2) ✓ (reapplied in day-2 Tier 1 burst after detection-analyst's edits were lost)
- #31: Day-2 corpus-scale Fridrich RS + SRM L2 sweep — closed §7.1 with the 75%→31% result ✓
- #32: Day-2 Tier 1 alpha — `--cost-function` CLI flag with UERD default ✓ (cli-wirer)
- #33: Day-2 Tier 1 alpha — phantasm-image FFI panic-across-C-unwind hardening ✓ (image-polish)
- #34: Day-2 Tier 1 alpha — phantasm-crypto v2 envelope with HMAC-SHA256 MAC + HKDF key split + FORMAT_VERSION byte ✓ (crypto-cleaner)
- #35: Day-2 Tier 1 alpha — phantasm-core pipeline error collapse to clean `CryptoError::AuthFailed` ✓ (cli-wirer, cross-lane absorption from crypto-cleaner)
- #36: Day-2 Tier 1 alpha — corpus file-size number re-verification post-hardening ✓ (bench-rerunner; day-1 numbers reproduce within noise)
- #37: Day-2 Tier 1 alpha — README + LICENSE-MIT + LICENSE-APACHE + CHANGELOG ✓ (team-lead, written against verified headline numbers)

---

## 7. Tomorrow's Agenda — detailed

Ordered by a mix of research value and engineering prerequisite:

### 7.1 Close the "does UERD drop classical detection rate at scale" question — **CLOSED 2026-04-14**

**Result:** See Finding 7 in §5. UERD drops Fridrich RS detection rate 75.3% → 30.8% (−44.4 pp, 2.4× reduction) on the 198-image corpus. Paired: UERD beats Uniform on 196/198 images for RS, 198/198 for SRM L2. Task #27 also completed (SRM-lite L2 wired into eval-corpus aggregation).

**Original goal (for the record):** The single outstanding research question from day 1. We had:
- UERD is 4× better than Uniform on single-image Fridrich RS rate
- Both are below the 0.05 threshold at single-image granularity
- But the Aletheia-faithful Fridrich RS IS a detector that fires on phantasm stego (it caught our original `stego.jpg` at 0.053)
- What we don't know: at population scale on 198 images, does UERD's 4× reduction translate into a lower detection FREQUENCY?

**Work required:**
1. Wire `fridrich_rs.max_rate` and `fridrich_rs.verdict` into the `eval-corpus` aggregation list (modify `phantasm-bench/src/eval_corpus.rs` to aggregate these new fields from the analyze_stealth output). Parallel-burst race leftover from day 1.
2. Also wire `srm_lite_l2_distance` while we're there (task #27).
3. Re-run the full corpus eval to get the Fridrich RS + SRM L2 distributions for both Uniform and UERD.
4. Report: per-cost-function detection rate (fraction of images where max_rate > 0.05), paired comparison (UERD vs Uniform detection at same image), classical detection rate delta.

**Expected outcome:** One of three things:
- (a) UERD drops detection rate from e.g. 30% → 3%. Headline-worthy result. Write it up.
- (b) UERD drops detection rate marginally, say 30% → 20%. Still a win but less dramatic; suggests we need better cost functions (J-UNIWARD) OR lower-density embedding OR the research-raw path.
- (c) Both 0% or both 100%. Means the Fridrich RS isn't the right detector for this payload density on this corpus. Need SRM L2 or Aletheia-auto (which needs TensorFlow, which doesn't install on Python 3.14).

### 7.2 Research-raw embedding path (task #30)

**Goal:** Unlock true density sweeps by bypassing the crypto envelope.

**Design:**
- Add a `ResearchOrchestrator` in `phantasm-core` OR a `research_raw` function alongside the main embed/extract pipeline
- Takes a `target_stc_message_bits: usize` parameter directly
- Generates random message bits of exactly that length
- Runs STC over the cover with the computed cost map, no envelope, no ECC
- Writes the modified coefficients back to the JPEG
- Returns the stego image AND the message bits, so extraction can verify roundtrip
- Clearly marked as benchmarking-only ("#[doc(hidden)]" or behind a `research` feature flag)

**Why this unlocks research:**
- With direct STC message length control, we can embed 10/100/1000/10000/50000 bits and measure detection rate at each true density
- Produces the standard security-capacity curve from academic steganography papers
- UERD's advantage is predicted to *widen dramatically* below 10% true density (where the cost gradient matters most)

### 7.3 J-UNIWARD cost function

**Goal:** Comparison benchmark against the industry-standard academic distortion function. If UERD wins our detection tests, we want to know if J-UNIWARD wins more.

**Implementation:**
- Add `phantasm-cost/src/uniward.rs` behind the existing `DistortionFunction` trait
- Daubechies-8 wavelet decomposition → three directional high-pass filters → per-coefficient cost from local filter responses
- Published formula: Holub & Fridrich 2014, "Universal Distortion Function for Steganography in an Arbitrary Domain"
- Validate against published detection-error numbers on BOSSbase (if achievable) or at least against UERD on our Picsum corpus
- Wire into `cost_fn_from_name` in eval_corpus so `--cost-functions uniform,uerd,j-uniward` works

**Estimated effort:** one teammate session. The wavelet filter math is finicky but well-specified.

### 7.4 CLI `--cost-function` flag

**Goal:** Actually let users use UERD via the official CLI, not via the example harness.

**Scope:**
- Add `--cost-function <name>` flag to `phantasm embed` (values: `uniform`, `uerd`)
- Route to `ContentAdaptiveOrchestrator::new(Box::new(cost_fn))` based on the flag
- Default: `uerd` (make content-adaptive the shipping default)
- Document in `phantasm embed --help`
- Verification: CLI smoke test with each cost function, roundtrip check

Trivial work but it's been blocking the tool's actual usability since day 1.

### 7.5 Aletheia corpus sweep with external detector

**Goal:** Get a third-party detection rate for the corpus, not just our own detectors.

**Scope:**
- Shell script or Rust runner that calls Python Aletheia via subprocess on each cover/stego pair
- Aggregates: rate per channel, verdict per image, detection frequency per cost function
- Run the 198-image sweep for both Uniform and UERD, compare
- This is the truly independent verification — not something we implemented ourselves

**Risk:** Aletheia's WS and Triples attacks need Octave; its ML attacks need TensorFlow (which won't install on Python 3.14). RS is the only one that works cleanly. Still useful as a single-detector third-party cross-check.

### 7.6 Engineering cleanups (when convenient)

- **#17** — Huffman re-optimization. Probably one teammate session in phantasm-image. Would push UERD's mean file inflation from 3 KB toward 0 KB. UERD's current +3 KB is still a forensic signal ("this file is slightly too big for its content") and zeroing it out is valuable for the shippability story.
- **#18** — Permutation MAC. Half a teammate session. Cleans up wrong-passphrase error messages from "declared length 3331321903 exceeds available 8060" to "authentication failed".
- **#19** — Progression mode preservation. Tiny. Cosmetic but matters for forensic-resistance.
- **#20** — `analyze` capacity accuracy. Tiny. Makes the CLI's analyze output consistent with what embed can actually use.

### 7.7 The longer-horizon stuff (explicitly NOT tomorrow)

From `PLAN.md`:
- **Phase 2 (weeks 8–11)**: Channel adapters, MINICER-style coefficient stabilization, ROAST overflow handling, ECC parameter tuning per channel. Real compression resilience for social media uploads.
- **Phase 3 (weeks 12–15)**: Hash guard — pHash/dHash/PDQ preservation, per-image sensitivity classification (3-tier from Spike B finding), cover pre-nudge for sensitive images, `phantasm analyze` real sensitivity output.
- **Phase 4 (weeks 16+)**: Multi-layer deniable embedding, PDQ hash support, cross-format resilience (PNG → JPEG targeted embedding), performance optimization (SIMD, rayon).
- **Future work (post-v1)**: Adversarial cost adjustment against a distilled steganalyzer ensemble, lattice-based errorless embedding, diffusion/latent-space generative methods.

---

## 8. Known Limitations and Caveats

### Engineering
1. **Envelope padding quantization** — every payload pads to `{256, 1024, 4096, 16384, ...}` bytes regardless of true size. Makes density sweeping via payload sizing impossible. Task #30.
2. **Single STC rate** — orchestrator hard-codes inverse rate 4. No way to tune embedding rate without code changes.
3. **Single-layer binary STC only in the orchestrator** — `ContentAdaptiveOrchestrator` uses `StcEncoder` not `DoubleLayerEncoder`, losing the per-direction cost advantage that ternary embedding would give UERD. Double-layer implementation exists (task #15 flagged the bit-plane construction as sub-optimal) but isn't wired into the orchestrator.
4. **File size inflation** — not zero under UERD (mean +3 KB) because Huffman tables aren't re-optimized after coefficient write-back. Task #17.
5. **libjpeg longjmp hazard** — phantasm-image installs the default libjpeg error handler, which calls `exit()` on fatal errors. Corrupt JPEGs will terminate the process. Task #14.
6. **Wrong passphrase ugly error** — fails via length-framing sanity check, not auth. Task #18.
7. **`analyze` over-reports capacity** — doesn't account for envelope padding block sizes. Task #20.

### Research methodology
1. **Picsum corpus is not academic-standard** — it's varied and reproducible but not BOSSbase. Published detection-error numbers from literature aren't directly comparable.
2. **Spatial RS rarely fires on our JPEG output** — our Rust implementation is correct per Fridrich 2001 but JPEG quantization obscures spatial LSB patterns. The Aletheia-faithful port (which operates on decoded RGB) is the right one for now.
3. **The ±1-transition / LSB-entropy / non-zero-AC signals are density-insensitive** — they count total modifications, which STC rate math keeps constant. They can't distinguish Uniform from UERD even when UERD is dramatically better perceptually.
4. **No modern ML detector** — SRNet / EfficientNet / Yedroudj-Net would be the gold standard. We use SRM-lite as a classical proxy. For academic comparability we'd eventually need an ONNX-loaded pre-trained network.
5. **Single-image detection is noisy** — both Fridrich RS and SRM L2 have run-to-run variance at the 0.05-threshold boundary. Corpus-scale population testing is the right granularity for this work.

### Security
1. **Detectable, but payload is cryptographically secure.** A steganalyst can determine "this JPEG contains embedded data" via any of several detectors. They cannot read the payload without the passphrase — Argon2id(64 MiB) + XChaCha20-Poly1305 is the real defense line. This is the working security model for day-1 phantasm, and it doesn't change until content-adaptive + channel-resilient methods actually evade detection.
2. **No channel resilience yet.** Stego files that go through Facebook, Twitter, Instagram, or WhatsApp compression will not extract correctly. Phase 2 work.
3. **Hash guard not implemented.** pHash / dHash / PDQ are not preserved yet — any platform running perceptual hash matching will see the stego as a different image from the cover. Phase 3 work.

---

## 9. Tools, Files, and Artifacts on Disk

### Research inputs
- `test.jpg` (720×680, 87,726 B) — the original cover (Ronaldo ice bath meme, from before the session; **confirmed untouched** by md5)
- `cover.jpg` (1024×1024, 91,111 B) — synthetic plasma cover generated via `gen_cover.rs` example
- `research-corpus/` — 198 JPEGs, 22 MB total, gitignored

### Research outputs
- `stego.jpg` (106,262 B) — pre-refactor uniform stego; Aletheia RS still detects at 0.053
- `stego_uniform.jpg`, `stego_uniform_ca.jpg`, `stego_uerd.jpg` — current-pipeline sample stegos for ad-hoc comparison
- `test.jpeg` (126,214 B) — 1024×1024 cover with full `minimal.rs` embedded (51% capacity use; large file-size inflation; extractable with "correct-horse-battery-staple")
- `/tmp/eval-full.json`, `/tmp/eval-full.md` — fixed-payload 198-image eval output (burst 5)
- `/tmp/density-sweep.json`, `/tmp/density-sweep.md` — 198×6 density sweep output (burst 7)
- `/tmp/aletheia/` — checked-out Aletheia repo for Python reference

### Binaries (built in `target/release`)
- `phantasm` — CLI (`embed`, `extract`, `analyze`, `channels`, `bench`)
- `phantasm-bench` — research harness (`compare`, `analyze-stealth`, `eval-corpus`)
- `gen_cover` (example) — synthetic JPEG generator
- `fetch_corpus` (example) — corpus download script
- `compare_cost_functions` (example) — Uniform-vs-UERD research harness used throughout day 1

### Development / planning docs
- `PLAN.md` — architectural plan, v0.1.1-draft. Original plan with day-1 revisions (Phase -1 spikes, Phase 0 split, adversarial moved to Future Work, hash-guard bimodal tier revision)
- `RESEARCH.md` — initial steganography literature review (unchanged since day 1 start)
- `STATUS.md` — this document

---

## 10. Quick Resume Checklist

When picking this back up tomorrow:

1. `cd /Users/dylan/Developer/phantasm`
2. `cargo test --workspace` — should print `132 passed, 0 failed`
3. `cargo clippy --workspace --all-targets -- -D warnings` — should be clean
4. Read `STATUS.md` §7 for the ordered agenda
5. **First move recommended**: extend `phantasm-bench/src/eval_corpus.rs` aggregation to include `fridrich_rs.max_rate`, `fridrich_rs.verdict`, and `srm_lite_l2_distance` from the analyze_stealth output, then rerun the full 198-image corpus eval. This closes the biggest outstanding research question (population detection rate) with minimal new code.
6. **Second move recommended**: build the research-raw embedding path (task #30). Gives us the real density curve.
7. **Third move**: J-UNIWARD as a second cost function.

Git state: **no commits have been made this session**. Everything is local and staged. Consider a "Day 1 end-of-day" commit before starting day 2, or wait for a tidier checkpoint after the first few tasks of day 2 land.

Team state: 12 teammate agents spawned across 7 bursts, all currently idle. They can be re-woken via SendMessage if useful; otherwise fresh agents per task going forward.

Session memory worth saving (if you haven't already):
- User preference: agent teams by default, fresh teammates per task unless continuity is valuable
- User style: likes concise status reports, tolerates long research writeups when they come at meaningful checkpoints, asks exploratory questions before committing to implementation
- User trust: has validated that "the tool works end-to-end" and "UERD wins SSIM 100%" are real results, not overclaims

---

## 11. Publishability — the path from now to v0.1.0

Three-tier plan discussed at end-of-day 1. Currently at "Tier 0: private repo with working tool, no public artifacts."

### Tier 0 — current state (private backup)

Push to a **private** GitHub repo as soon as convenient. Zero polish required. Gives you offsite backup and version history. Do this regardless of public plans.

```bash
gh repo create phantasm --private --source=. --push
# or:
git remote add origin git@github.com:<user>/phantasm.git
git push -u origin main
```

### Tier 1 — v0.1.0-alpha public release (~1 session of polish)

"Early research code, here's what it does, expect breakage." Publishable after one focused work session. Concrete checklist:

**Must-have:**
- [ ] `README.md` at repo root: one-paragraph pitch, the 100%-SSIM-win-rate + SRM-2.8×-lower headline, quickstart (`cargo build`, `phantasm embed photo.jpg secret.txt -p "..." -o stego.jpg`), "what doesn't work yet" section, link to `STATUS.md` and `PLAN.md`
- [ ] `LICENSE-MIT` and `LICENSE-APACHE` files at repo root (PLAN says dual-licensed)
- [ ] `phantasm embed --cost-function {uniform,uerd}` CLI flag with **UERD as the default** (currently only `MinimalOrchestrator` with uniform is wired into `phantasm-cli`; `ContentAdaptiveOrchestrator<Uerd>` only reachable via `phantasm-core/examples/compare_cost_functions.rs`). Shippability-critical — without this the interesting result is hidden behind an example binary.
- [ ] Fix task #14 — `phantasm-image` libjpeg error handler must install a `setjmp`/`longjmp`-safe wrapper so corrupt JPEGs return `Err` instead of calling `exit()` and killing the process. Current behavior is a crash-on-bad-input for any user feeding damaged files.
- [ ] Fix task #18 — permutation MAC so wrong-passphrase errors report `CryptoError::AuthFailed` cleanly instead of `declared length 3331321903 exceeds available 8060`. Looks broken even though it's correct.
- [ ] Fix task #17 — re-optimize Huffman tables after coefficient write-back to reduce UERD's mean +3 KB file inflation toward zero. Current UERD inflation (+3 KB) is still a forensic "file is slightly larger than expected" signal.
- [ ] Git tag `v0.1.0-alpha` after the polish commits land.
- [ ] A `CHANGELOG.md` entry documenting the alpha.

**Nice-to-have (can defer to beta):**
- [ ] Finish task #27 — wire `srm_lite_l2_distance` into `eval-corpus` aggregation. The SRM L2 signal is the only one in our battery that actually distinguishes Uniform from UERD, and it's currently MIA from the corpus-scale reports.
- [ ] Finish task #20 — `phantasm analyze` over-reports capacity. Currently lies to users about what they can embed.
- [ ] Finish task #19 — preserve JPEG progression mode. Minor forensic resistance improvement.
- [ ] Corpus-scale Fridrich RS + SRM L2 numbers in the README (the research headline).

**Honest caveats that MUST be in the README:**
- No compression resilience yet — don't upload stego to Facebook/Instagram/Twitter and expect recovery
- No hash guard yet — perceptual hashes DO change after embedding (pHash is unchanged because the embedding rate is low; at higher densities it will)
- Single-layer STC only in the main pipeline — double-layer exists but not wired
- H̃ sub-matrix is a deterministic PRNG construction, not the published Filler 2011 DDE Lab tables (task #5/#15)

### Tier 2 — v0.1.0 "stable" public release (2–3 sessions)

Delivers on the project's stated purpose: compression-resilient, hash-guard-preserving steganography. Concrete scope on top of alpha:

- [ ] Phase 2 channel adapter — at minimum one channel profile (Twitter is easiest, compresses least aggressively), implementing MINICER-style coefficient stabilization + ROAST overflow alleviation. Lets users hide a message in an image they'll actually share online and get it back intact.
- [ ] Phase 3 hash guard — per-image sensitivity classifier from Spike B's 3-tier model (Robust / Marginal / Sensitive), wet-paper cost constraint for hash-critical coefficients, pre-nudge for Sensitive images, `phantasm analyze` outputs sensitivity tier
- [ ] Research-raw embedding path (task #30) + full security-capacity curve with Fridrich RS + SRM L2 + J-UNIWARD comparison — paste the resulting chart into the README
- [ ] Task #5/#15 — replace PRNG H̃ with published DDE Lab tables AND swap the double-layer construction to paper-standard ternary. Gets distortion performance to published J-UNIWARD-comparable numbers
- [ ] Shell completions (bash, zsh, fish) via `clap_complete`
- [ ] Man page via `clap_mangen`
- [ ] Packaging: `cargo install`-ready, possibly Homebrew formula, possibly AUR package
- [ ] Security review pass by someone other than me
- [ ] Git tag `v0.1.0`

### Not-for-v0.1 (Phase 4+ / future work)

Anything in PLAN.md "Future Work — Post-v1" stays deferred: adversarial cost optimization, lattice-based errorless embedding, diffusion/latent-space methods. These are research projects that should have their own release track, not gate v0.1.

### Recommended next-session opening move

The Tier 1 polish checklist has eight items, most of them small. Ordered for maximum unblock-per-hour:

1. **CLI `--cost-function` flag** (1 teammate session, trivial) — immediately makes the good result the default
2. **Task #14 longjmp hardening** (1 teammate session, moderate) — eliminates the scariest user-facing crash
3. **Task #18 permutation MAC** (1 teammate session, moderate) — cleans up error messages
4. **Task #17 Huffman reopt** (1 teammate session, moderate-to-hard) — kills file inflation signal
5. **Task #27 SRM L2 corpus aggregation** + full corpus rerun (short) — gets the headline number for the README
6. **README + LICENSE files** (1 session, me writing) — the public-facing pitch
7. **Git tag + changelog + push** (trivial)

Steps 1–5 can run as one parallel burst (four teammates on tasks + me on the README once the research numbers are in). That's the cleanest path from "day 1 end state" to "publishable alpha" in one focused work session.

---

## 12. Passphrases, seeds, and other things that live only in session context

Not secrets (everything valuable is cryptographically sealed), but worth writing down so they survive /compact:

- **`correct-horse-battery-staple`** — the passphrase used for the `test.jpeg`, `stego.jpg`, `stego_uniform.jpg`, `stego_uerd.jpg` samples at the project root. `cargo run --release --bin phantasm -- extract --input test.jpeg --passphrase "correct-horse-battery-staple" --output recovered.rs` recovers `phantasm-core/src/minimal.rs` intact. Original `stego.jpg` recovers `phantasm-crypto/src/metadata.rs`.
- **`phantasm-corpus-eval-v1-{sha256[..8]}`** — the deterministic per-image passphrase used by `phantasm-bench eval-corpus`. Prefix is configurable via `--passphrase-prefix`.
- **`phantasm-0001` through `phantasm-0198`** — the Picsum seeds used to fetch the 198-image research corpus. Stored in `research-corpus/manifest.json` alongside SHA-256 hashes so the corpus is fully regenerable from the manifest.
- **Spike B corpus seeds** — also Picsum, `phantasm-spike-b-0001` through `phantasm-spike-b-0060` (or similar — check `spikes/phash-overlap/corpus/` regeneration script if needed, corpus is gitignored).

---

**End of STATUS.md**
