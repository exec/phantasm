<p align="center">
  <img src="phantasm.png" alt="phantasm" />
</p>

**Content-adaptive JPEG steganography in Rust.** Research-grade v0.1.0 stable.

Phantasm hides data in the DCT coefficients of JPEG images using a content-adaptive distortion minimization scheme (UERD or J-UNIWARD) driven through syndrome-trellis coding, sealed in an authenticated cryptographic envelope (Argon2id + XChaCha20-Poly1305 + HMAC-SHA256), optionally stabilized against social-media re-encoding (Twitter channel adapter, MINICER + ROAST), and optionally constrained to preserve perceptual hashes (pHash/dHash wet-paper guard). The thesis of the project is that combining channel-adaptive preprocessing, content-adaptive distortion, syndrome-trellis coding, perceptual-hash preservation, and modern authenticated encryption in a single tool is a capability no existing steganography tool offers. As of `v0.1.0`, **all five pillars are reachable via the main CLI**.

## Headline research result

On a 198-image seed-regenerable Picsum research corpus with a fixed 3,723-byte payload at ~31% raw capacity, using syndrome-trellis coding with published DDE Lab H̃ tables (Filler 2011) and conditional-probability double-layer decomposition:

| Metric | Uniform embedding | UERD content-adaptive | J-UNIWARD | Δ (UERD vs Uniform) |
|---|---:|---:|---:|---:|
| **Fridrich RS detection rate** (Aletheia-faithful, threshold 0.05) | **66.7%** | **26.8%** | 30.3% | **−40 pp / 2.5× reduction** |
| Mean Fridrich RS `max_rate` | 0.3208 | 0.0486 | 0.0557 | 6.6× reduction |
| **SRM-lite L2 distance** (mean) | 0.599 | **0.131** | 0.197 | **4.6× lower** |
| Mean SSIM | 0.8373 | 0.9509 | **0.9532** | +0.114 |
| Mean PSNR (dB) | 34.97 | 36.09 | **37.58** | +1.12 dB |
| Mean MSE | 26.94 | 22.28 | **15.20** | −4.66 |
| Mean file-size delta (cover → stego) | +5,390 B | **−1,321 B** | −1,039 B | −6,712 B (net deflation) |

**UERD beats Uniform on Fridrich RS in 195/198 images (98.5%), on SRM-lite L2 in 198/198 (100%), and on SSIM in 198/198 (100%).**

UERD stego is, on average, **smaller** than the cover — a consequence of mozjpeg's trellis-quant Huffman rebuild compressing the content-adaptive distortion distribution better than the original cover's entropy. The "stego file is slightly larger than expected" forensic signal that classical LSB-matching implementations leave behind is inverted by phantasm: stego is typically slightly smaller.

### Security-capacity curve

Using the research-raw embedding path (`phantasm-bench research-curve`) to measure detection rate at controlled STC message lengths, bypassing envelope padding:

| Payload bits | Uniform det.frac | UERD det.frac | J-UNIWARD det.frac |
|---:|---:|---:|---:|
| 100 | 17.5% | 17.5% | 17.5% |
| 2,000 | 17.5% | 17.5% | 17.5% |
| 8,000 | 22.5% | 17.5% | 17.5% |
| 20,000 | **40.0%** | 20.0% | **17.5%** |

All three cost functions sit near the Fridrich RS false-positive floor (~17.5%) at low payloads. At 20k-bit payloads, Uniform blows out to 40% detection while UERD holds at 20% and J-UNIWARD at the noise floor — J-UNIWARD wins the high-capacity / security-critical regime on this corpus.

### Modern CNN steganalysis (post-v0.1.0 evaluation)

A second evaluation against pretrained CNN steganalyzers — JIN-SRNet (Butora/Yousfi/Fridrich 2021, PyTorch) and Aletheia EfficientNet-B0 J-UNIWARD (Daniel Lerch, Keras→ONNX) — confirms phantasm holds up against modern deep-learning adversaries, but with an important reversal: **the optimal cost function inverts.**

| Detector | Cover (FP) | Uniform | UERD | J-UNIWARD |
|---|---:|---:|---:|---:|
| Fridrich RS (classical, above) | — | 66.7% | **26.8%** | 30.3% |
| **JIN-SRNet** detection rate (P>0.5) | 7.1% | **98.0%** | 46.5% | **16.2%** |

Against classical Fridrich RS, **UERD wins**. Against modern JIN-SRNet, **J-UNIWARD wins**. **49.5% of phantasm J-UNIWARD stegos score lower P(stego) than their own cover** — JIN-SRNet sees the stego as more cover-like than the original cover. The Aletheia EffNet-B0 detector (despite severe cover-source mismatch on Picsum) agrees with JIN-SRNet on the ordering in paired-per-cover analysis. **For a modern (deep-learning) threat model, use `--cost-function j-uniward`. For a classical adversary, UERD remains the default.** See [ML_STEGANALYSIS.md § Findings](ML_STEGANALYSIS.md#findings-in-order-of-importance) for the full breakdown.

#### Attacker adaptation: a cheap fine-tune breaks UERD but not J-UNIWARD

A 21-second fine-tune of JIN-SRNet on phantasm UERD output (288 training examples, init from the public J-UNIWARD-trained checkpoint, run on a single RTX 5070) lifts UERD detection from 57.6% to **77.8%** — a 20 percentage-point jump. The same fine-tune *decreases* J-UNIWARD detection (28.3% → 25.3%).

| Cost function | JIN-SRNet baseline | After 21s UERD fine-tune | Δ |
|---|---:|---:|---:|
| **uerd** | 57.6% | **77.8%** | **+20.2 pp** |
| **juniward** | 28.3% | **25.3%** | **−3.0 pp** |

**UERD's modern-detector robustness was partially a cost-function-mismatch artifact. J-UNIWARD's is structural** — at least against this fine-tune strategy. A real attacker with a proper training corpus would lift UERD detection further (probably 90%+); J-UNIWARD's evasion does not close under cheap attacker adaptation by this method. This reinforces the **"use J-UNIWARD for modern threat models"** recommendation: J-UNIWARD is the only one of phantasm's three cost functions whose evasion holds up under cheap attacker adaptation. See [ML_STEGANALYSIS.md § Update 1](ML_STEGANALYSIS.md#update-1--uerd-fine-tune-option-b-complete) for the methodology and full numbers.

#### What's still open

- **Asymmetry validation.** The Update 1 finding hinges on UERD-aware fine-tunes lifting UERD detection while leaving J-UNIWARD alone. If a J-UNIWARD-aware fine-tune from a UERD baseline symmetrically lifts J-UNIWARD detection, the asymmetry is fake. Validation in progress (see [ML_STEGANALYSIS.md § v0.2 research direction proposal](ML_STEGANALYSIS.md#v02-research-direction-proposal)).
- **Adversarial costs.** Most ambitious v0.2 direction. Use a deployed CNN as a differentiable distortion oracle and compute embedding costs that maximize distance from its decision boundary. Gated on the asymmetry validation.
- **No public UERD-trained CNN baseline exists.** The fine-tuned UERD-aware model in Update 1 is phantasm's own contribution; cross-validation against a third-party checkpoint is not currently possible.

For the security-capacity curve, both detectors' detailed results, cross-detector consistency analysis, and the full caveats list, see [ML_STEGANALYSIS.md](ML_STEGANALYSIS.md). Direct links:
- [TL;DR](ML_STEGANALYSIS.md#tldr)
- [What we ran](ML_STEGANALYSIS.md#what-we-ran)
- [Detailed results — fixed payload](ML_STEGANALYSIS.md#detailed-results--fixed-payload-198-covers-3-kb---stealth-high)
- [Update 1 — UERD fine-tune](ML_STEGANALYSIS.md#update-1--uerd-fine-tune-option-b-complete)
- [Cross-detector consistency](ML_STEGANALYSIS.md#cross-detector-consistency)
- [Caveats](ML_STEGANALYSIS.md#caveats)
- [v0.2 research direction proposal](ML_STEGANALYSIS.md#v02-research-direction-proposal)

### Detectors and reproducibility

- **Fridrich RS**: native Rust port of Aletheia's reference Fridrich 2001 RS attack, validated to reproduce Aletheia's `0.053` detection on its own sample stego at `0.0513` (within 0.002 tolerance). Per-channel max over (R, G, B), threshold 0.05.
- **SRM-lite L2**: 4 residuals × 7×7 co-occurrence matrices = 196-feature vector, L2 distance between cover and stego feature vectors. Threshold-free distance metric.
- **Research corpus**: 198 Picsum.photos JPEGs with seeds `phantasm-0001` through `phantasm-0198`, across three quality factors (75/85/90) and three sizes (512×512, 720×680, 1024×1024). The image files themselves are gitignored — only the manifest (with source URL, seed, dimensions, QF, and SHA-256 per image) is tracked. Fetch the full corpus with:
  ```bash
  cargo run --release -p phantasm-image --example fetch_corpus
  ```
  See [`research-corpus/README.md`](research-corpus/README.md) for corpus-level details.
- All numbers reproduce within ~2 percentage points across day-1, day-2, and v0.1.0 runs.

## What phantasm does

- **Encode arbitrary payload bytes into a JPEG cover** using content-adaptive DCT coefficient perturbation
- **Decode the payload out of a stego JPEG** given the same passphrase
- **Three content-adaptive cost functions**: Uniform (baseline), UERD (Guo/Ni/Shi 2015, divisibility-based redistribution into textured regions), J-UNIWARD (Holub & Fridrich 2014, Daubechies-8 wavelet residual-based). Selectable via `--cost-function {uniform,uerd,j-uniward}`.
- **Syndrome-trellis coding** (Filler/Judas/Fridrich 2011) at rate 1/4, constraint height 7–12, with **published DDE Lab H̃ reference matrices** (2400 entries from common.cpp) and conditional-probability double-layer decomposition delivering 0.995× bits-per-L1 efficiency vs the asymptotic ML2 bound.
- **Authenticated encryption envelope**: Argon2id(64 MiB / 3 iterations / 4 threads) → HKDF-SHA256 split into `(aead_key, mac_key)` → XChaCha20-Poly1305 AEAD + HMAC-SHA256 MAC (16-byte truncated) for fast wrong-passphrase detection
- **Channel adapter (Twitter profile)** via `--channel-adapter twitter`: MINICER-style iterative coefficient stabilization at target QF=85/4:2:0 with ROAST overflow alleviation. 98.7% measured coefficient survival rate on real mozjpeg re-encoding.
- **Perceptual-hash guard** via `--hash-guard {phash,dhash}`: per-image 3-tier sensitivity classifier (Robust/Marginal/Sensitive) calibrated from empirical single-image perturbation analysis, with wet-paper cost constraints routing STC embedding around coefficients that would flip pHash or dHash bits.
- **Fixed-tier envelope padding** to `{256, 1024, 4096, 16384, 65536, 262144}` bytes to hide exact payload length from an observer with access to the stego
- **Corpus-scale benchmarking harness** (`phantasm-bench eval-corpus`) for comparing cost functions on a directory of cover images

## Quickstart

```bash
# Build the workspace
cargo build --release

# Embed a payload into a JPEG cover (UERD is the default)
./target/release/phantasm embed \
    --input cover.jpg \
    --payload secret.txt \
    --passphrase "correct-horse-battery-staple" \
    --output stego.jpg

# Use J-UNIWARD for high-capacity / security-critical embeds
./target/release/phantasm embed \
    --cost-function j-uniward \
    --input cover.jpg \
    --payload secret.txt \
    --passphrase "correct-horse-battery-staple" \
    --output stego.jpg

# Stabilize against Twitter re-encoding (experimental — increases modification count)
./target/release/phantasm embed \
    --cost-function uerd \
    --channel-adapter twitter \
    --input cover.jpg \
    --payload secret.txt \
    --passphrase "correct-horse-battery-staple" \
    --output stego.jpg

# Preserve pHash via wet-paper constraint (no-op on Robust covers, which is ~75%)
./target/release/phantasm embed \
    --cost-function uerd \
    --hash-guard phash \
    --input cover.jpg \
    --payload secret.txt \
    --passphrase "correct-horse-battery-staple" \
    --output stego.jpg

# Check a cover before embedding
./target/release/phantasm analyze cover.jpg
# Prints: capacity estimate, JPEG metadata, Sensitivity tier (Robust/Marginal/Sensitive),
#         Hash-guard (pHash) wet positions count

# Extract (must match the embed's --channel-adapter and --hash-guard flags)
./target/release/phantasm extract \
    --input stego.jpg \
    --passphrase "correct-horse-battery-staple" \
    --output recovered.txt

diff secret.txt recovered.txt   # byte-identical
```

- `--cost-function` accepts `uniform`, `uerd` (default), or `j-uniward`. The "right" choice is threat-model dependent: `uerd` wins against classical statistical detectors, `j-uniward` wins against modern CNN steganalysis. See [ML_STEGANALYSIS.md](ML_STEGANALYSIS.md).
- `--channel-adapter` accepts `none` (default) or `twitter`.
- `--hash-guard` accepts `none` (default), `phash`, or `dhash`.
- `phantasm extract` accepts `--channel-adapter` and `--hash-guard` flags for forward compatibility, but they are no-ops in `v0.1.0`: extract derives coefficient positions geometrically from the stego image (keyed on the passphrase + the pHash-stable image salt), and does not need to know which cost function, channel adapter, or hash-guard was used at embed time. Auto-detection metadata in the envelope is a post-v0.1.0 feature.

## What doesn't work yet

`v0.1.0` is a research checkpoint, not a finished tool. The following are explicitly NOT implemented or are MVP-level:

- **Only one channel profile (Twitter).** Instagram, Facebook, and other social media services have their own re-encode pipelines. `v0.1.0` ships with a Twitter profile only (QF=85, 4:2:0). Other channels need dedicated profiles; MINICER doesn't generalize without them. The Twitter profile is also a single-block approximation and does NOT model Twitter's rescale step for images larger than 4096 pixels.
- **pHash/dHash only, no PDQ.** Facebook's PDQ algorithm (which underlies CSAM-matching databases) is NOT implemented. If you need PDQ-preservation you must evaluate independently.
- **No pre-nudge for Sensitive covers.** About 10% of images are classified as pHash-Sensitive by the 3-tier classifier. On these, the hash guard falls back to large wet sets that may exhaust effective capacity. Pre-nudging the cover to move hash bits away from their decision thresholds before embedding is a known-good technique that is not yet implemented.
- **Limited ML steganalysis coverage.** A first-pass evaluation against two pretrained CNN detectors (JIN-SRNet, Aletheia EfficientNet-B0) is documented in [ML_STEGANALYSIS.md](ML_STEGANALYSIS.md) — both detectors confirm phantasm J-UNIWARD evades modern deep-learning attacks at typical payloads. **What's still missing:** a UERD-trained CNN detector (no public weights exist; v0.2 candidate work item), an adversarially-trained cost function targeting a specific detector, BOSSbase / ALASKA2 cross-corpus validation, and any evaluation against fine-tuned production-grade detectors (which an actual nation-state adversary would deploy).
- **Envelope format is still considered pre-stable.** `v0.1.0` uses envelope format v2 (HMAC-SHA256-16 MAC + HKDF key split + FORMAT_VERSION byte). The next envelope revision is expected to add auto-detection of `--channel-adapter` and `--hash-guard` configuration, which will be a format break.
- **CLI is pre-stable.** Flag names may still shift before `v1.0.0`.
- **JPEG covers only in `v0.1.0`.** Phantasm embeds in JPEG DCT coefficients via the content-adaptive cost path; PNG (or any other lossless format) is not accepted as an input cover. PNG support is scheduled for Phase 3 of the roadmap, which adds spatial-domain embedding with S-UNIWARD costs — see [PLAN.md](PLAN.md) §6 Phase 3. A PNG decode module exists in `phantasm-image` but is not wired into the embed pipeline in `v0.1.0`.
- **No external security review.** Cryptographic primitives are used via established crates (`argon2`, `chacha20poly1305`, `hmac`, `sha2`, `hkdf`) but the composition, envelope layout, and integration have not been reviewed by anyone outside the project.

## Threat model

Phantasm is intended for scenarios where:

1. You want to send a confidential payload over a channel that allows JPEG images but is untrusted (e.g., casual adversaries, automated scanners, content-inspection middleboxes).
2. The adversary has access to the stego image and can run classical and statistical steganalysis on it.
3. The adversary does NOT have access to the cover, nor to any out-of-band channel keyed to the sender/receiver.
4. The adversary does NOT re-encode or compress the image in transit — **with the caveat that the Twitter channel adapter (`--channel-adapter twitter`) is a working MVP for one specific re-encode profile (QF=85, 4:2:0) at 98.7% measured coefficient survival.** Other channels (Facebook, Instagram, WhatsApp, etc.) destroy the embedded payload.

Note on cost-function selection in this threat model: against classical statistical detectors (Fridrich RS, SRM-lite, sample pairs), `--cost-function uerd` (the default) is the right choice and reduces detection rate by 2.5×. Against modern CNN steganalysis, `--cost-function j-uniward` is the right choice — pretrained PyTorch JIN-SRNet and Keras Aletheia EfficientNet-B0 both fail to fire on phantasm J-UNIWARD output at typical payload (see [ML_STEGANALYSIS.md](ML_STEGANALYSIS.md)). The cost-function ordering inverts depending on the threat model.

Phantasm is NOT suitable for:

- Defeating a well-resourced nation-state adversary running modern deep-learning steganalysis on your image with the same cover source they trained on (the cover-source mismatch defense in our eval may not hold against a targeted adversary)
- Defeating services other than Twitter that re-encode uploaded JPEGs (Facebook, Instagram, WhatsApp, etc. need dedicated channel profiles that are not yet implemented)
- Long-term archival where envelope-format stability matters
- Any situation where the confidentiality of the payload is life-critical

The cryptographic primitives (Argon2id, XChaCha20-Poly1305, HMAC-SHA256, HKDF-SHA256) are used via the established `argon2`, `chacha20poly1305`, `hmac`, `sha2`, and `hkdf` crates. The composition, key schedule, and envelope layout are project-specific and have NOT been externally reviewed. Do not treat `v0.1.0` as production-ready cryptography.

## Project layout

```
phantasm/
├── phantasm-image/    # JPEG DCT coefficient I/O via mozjpeg-sys (panic-safe FFI)
├── phantasm-crypto/   # Argon2id + XChaCha20-Poly1305 + HMAC + HKDF envelope (v2)
├── phantasm-stc/      # Syndrome-Trellis Codes with published DDE Lab H̃ tables
├── phantasm-ecc/      # Reed-Solomon error correction wrapper
├── phantasm-cost/     # DistortionFunction trait + Uniform + UERD + J-UNIWARD
├── phantasm-channel/  # Channel adapters: MINICER + ROAST + TwitterProfile
├── phantasm-core/     # Orchestrators, research-raw, hash_guard, pipeline
├── phantasm-cli/      # `phantasm` binary
├── phantasm-bench/    # `phantasm-bench` binary (eval-corpus + research-curve)
├── spikes/            # Phase -1 de-risking experiments (DCT FFI, pHash cost)
└── research-corpus/   # 198 Picsum JPEGs (gitignored, manifest.json committed)
```

## More detail

- **[PLAN.md](PLAN.md)** — Architectural plan, phases, five-pillar thesis, and future work
- **[RESEARCH.md](RESEARCH.md)** — Literature review and academic background for the techniques phantasm uses
- **[STATUS.md](STATUS.md)** — Living session-by-session development log, research findings, and outstanding work. Read this if you want to know what's really built vs. planned.
- **[ML_STEGANALYSIS.md](ML_STEGANALYSIS.md)** — Post-v0.1.0 evaluation against pretrained CNN steganalyzers (JIN-SRNet, Aletheia EfficientNet-B0). Methodology, full numbers, caveats, and v0.2 research direction proposals.
- **[CHANGELOG.md](CHANGELOG.md)** — Release notes

## Building

Rust toolchain 1.75+ (edition 2021). Depends on `mozjpeg-sys` which requires a C compiler and `nasm` for libjpeg-turbo's optimized encoders. On macOS:

```bash
brew install nasm
cargo build --release
```

## Running the test suite and benchmarks

```bash
cargo test --workspace       # unit + integration tests across all crates
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check

# Fetch the 198-image research corpus (Picsum.photos, ~22 MB, ~2 min).
# Only needed once — the corpus is gitignored but regenerable from
# research-corpus/manifest.json.
cargo run --release -p phantasm-image --example fetch_corpus

# Corpus-scale evaluation against the 198-image research corpus.
cargo run --release -p phantasm-bench -- eval-corpus \
    --corpus research-corpus \
    --cost-functions uniform,uerd,j-uniward \
    --payload /path/to/payload.bin

# Single-file stealth analysis against classical detectors
cargo run --release -p phantasm-bench -- analyze-stealth \
    --cover cover.jpg \
    stego.jpg

# Security-capacity curve (uses the research-raw embedding path)
cargo run --release -p phantasm-bench -- research-curve \
    --corpus path/to/jpeg/dir \
    --cost-functions uniform,uerd,j-uniward \
    --bit-counts 100,500,2000,8000,20000 \
    --output curve.json --output-md curve.md
```

## License

Dual-licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option. Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in phantasm by you shall be dual licensed as above, without any additional terms or conditions.

## Status

`v0.1.0` is the first stable-tagged release. It is a research-grade checkpoint, not production-ready cryptographic software. Expect envelope-format breaks before `v1.0.0`, expect CLI flag shifts, and don't use it for anything where the confidentiality of the payload is life-critical. Read [STATUS.md](STATUS.md) for the full picture of what works and what's planned, and [CHANGELOG.md](CHANGELOG.md) for the detailed release notes.
