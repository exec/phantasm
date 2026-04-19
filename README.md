<p align="center">
  <img src="phantasm.png" alt="phantasm" />
</p>

**Content-adaptive JPEG (and PNG) steganography in Rust.** Research-grade v0.3.0.

Phantasm hides data in the DCT coefficients of JPEG images — or, as of v0.3.0, the pixel LSBs of grayscale PNG images via S-UNIWARD — using a content-adaptive distortion minimization scheme driven through syndrome-trellis coding, sealed in an authenticated cryptographic envelope (Argon2id + XChaCha20-Poly1305 + HMAC-SHA256), optionally stabilized against social-media re-encoding (Twitter channel adapter, MINICER + ROAST + Reed-Solomon ECC), and optionally constrained to preserve perceptual hashes (pHash/dHash wet-paper guard). The thesis of the project is that combining channel-adaptive preprocessing, content-adaptive distortion, syndrome-trellis coding, perceptual-hash preservation, and modern authenticated encryption in a single tool is a capability no existing steganography tool offers.

### What phantasm is (and is not)

**Phantasm defends the confidentiality of a payload that an adversary can see exists.** It is NOT a plausible-deniability tool against an adversary who has trained a CNN on phantasm output — that layer degrades gracefully against casual/off-the-shelf detection but fails against phantasm-aware ML. The [three-layer threat model](#three-layer-defense-framing) below makes the exact boundaries explicit. If your threat model requires "the adversary should not be able to tell this image contains hidden data at all," phantasm is the wrong tool against a well-resourced adversary.

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

### Modern CNN steganalysis (v0.2 + v0.3 evaluation)

A second evaluation against pretrained CNN steganalyzers — JIN-SRNet (Butora/Yousfi/Fridrich 2021, PyTorch) and Aletheia EfficientNet-B0 J-UNIWARD (Daniel Lerch, Keras→ONNX) — shows phantasm evades *off-the-shelf* CNN detectors with `--cost-function j-uniward`, and with an important reversal vs the classical result: **the optimal cost function inverts.** Against *phantasm-aware* CNNs (ones trained on phantasm stego output), L1 detection folds — see [L1 folds against phantasm-trained CNNs](#attacker-adaptation-l1-folds-against-phantasm-trained-cnns).

| Detector | Cover (FP) | Uniform | UERD | J-UNIWARD |
|---|---:|---:|---:|---:|
| Fridrich RS (classical, above) | — | 66.7% | **26.8%** | 30.3% |
| **JIN-SRNet** detection rate (P>0.5) | 7.1% | **98.0%** | 46.5% | **16.2%** |

Against classical Fridrich RS, **UERD wins**. Against modern JIN-SRNet, **J-UNIWARD wins**. **49.5% of phantasm J-UNIWARD stegos score lower P(stego) than their own cover** — JIN-SRNet sees the stego as more cover-like than the original cover. The Aletheia EffNet-B0 detector (despite severe cover-source mismatch on Picsum) agrees with JIN-SRNet on the ordering in paired-per-cover analysis. **For a modern (deep-learning) threat model, use `--cost-function j-uniward`. For a classical adversary, UERD remains the default.** See [ML_STEGANALYSIS.md § Findings](ML_STEGANALYSIS.md#findings-in-order-of-importance) for the full breakdown.

<a id="three-layer-defense-framing"></a>
#### Three-layer defense framing (canonical as of v0.3)

Phantasm's security stack has **three layers**, and the modern CNN steganalysis research tests only the first one:

| Layer | Defends | Gates on | v0.3 status |
|---|---|---|---|
| **L1 — Detection** | plausible deniability: *"does this JPEG/PNG contain hidden data?"* | a CNN steganalyzer that generalizes to phantasm's modification pattern | **holds against off-the-shelf (J-UNIWARD evades JIN-SRNet at 16.2%); fails against phantasm-aware (96.8% det at 500-cover scale)** |
| **L2 — Position recovery** | message shape: *"which coefficients carry the syndrome bits, in what order?"* | passphrase (Argon2id + HKDF + ChaCha12-keyed permutation) | **intact, standard primitives** |
| **L3 — AEAD decryption** | payload confidentiality: *"what are the actual bytes?"* | passphrase (Argon2id + XChaCha20-Poly1305 + HMAC-SHA256 pre-filter) | **intact, standard primitives** |

**L2 and L3 are the load-bearing layers** and use well-studied cryptographic primitives. L1 is the weak layer, and it's the only one the CNN steganalysis research tests. For a confidentiality threat model (which is how phantasm's threat model is scoped — see below), L1 detectability does not gate the security argument. For a plausible-deniability threat model, L1 matters and phantasm's honest position is: **L1 protects against casual detection, not against a well-resourced CNN-equipped adversary.**

#### Attacker adaptation: L1 folds against phantasm-trained CNNs

A series of fine-tune experiments (Updates 1-8 in [ML_STEGANALYSIS.md](ML_STEGANALYSIS.md)) establishes how phantasm's L1 holds up against adversaries of increasing capability:

| Adversary class | J-UNIWARD L1 detection rate | Source |
|---|---:|---|
| Classical Fridrich RS (v0.1.0 baseline) | 30.3% | v0.1.0 |
| Off-the-shelf CNN (pretrained JIN-SRNet, no phantasm training) | 28.3% | v0.2 Update 1 |
| Off-the-shelf CNN + `--cost-subset 0.6` | **22.1%** | v0.2 Update 6 |
| Phantasm-trained CNN, single-passphrase fine-tune | 54.5% | v0.2 Update 2 |
| Phantasm-trained CNN, multi-passphrase fine-tune (198 covers) | 89.9% | v0.2 Update 3 |
| **Phantasm-trained CNN, multi-passphrase fine-tune (500 covers)** | **96.8% (d500) / 97.5% (eval-198)** | **v0.3 Update 7** |

**L1 gracefully degrades across the adversary spectrum.** Against off-the-shelf detectors, J-UNIWARD + the `--cost-subset 0.6` optional flag produces the lowest detection rate we've measured (22.1%) — a 6.2 pp improvement over plain J-UNIWARD. Against a phantasm-aware attacker with multi-passphrase training data and 500 unique covers, L1 detection reaches ~97% regardless of cost-function choice. The v0.3 cover-source-diversity scale-up (Update 7) and iterative adversarial-costs failure (Update 8) together confirm: **every lever we have inside a hand-designed cost-function paradigm is exhausted for defending L1 against a phantasm-aware CNN.** Remaining L1 defense directions (v0.4+) are structurally different — end-to-end differentiable embedding, ADV-EMB/ADV-IMB STC-aware attacks that jointly optimize coefficient *choice* (not just per-coefficient cost), multi-cover payload spreading.

**Phantasm's v0.3 security argument explicitly does not rest on L1** — it rests on L2+L3, which are standard cryptographic primitives that the ML steganalysis research does not attempt to re-evaluate.

#### v0.2 research infrastructure (hidden flags)

Three new hidden research flags on `phantasm embed` for users who want to experiment with L1 hardening:

- **`--cost-subset <keep_fraction>`** — deterministically marks `(1 - keep_fraction)` of non-DC positions as wet (forbidden to STC) based on the passphrase, so different passphrases route STC through different candidate position sets. Default `1.0` (identity). `0.6` is the lowest value that fits a typical payload before STC infeasibility (23% embed-failure rate at `0.6`; use `--stealth low` to recover headroom). Produces a 6.2 pp L1 improvement against off-the-shelf CNN detectors and a per-stego density penalty against phantasm-trained detectors. See [ML_STEGANALYSIS.md § Update 6](ML_STEGANALYSIS.md#update-6--option-d-passphrase-derived-position-subset-mixed-result-ships-as-optional-l1-hardening-knob) for the full characterization.
- **`--cost-noise <amplitude>`** — deterministic passphrase-keyed multiplicative cost noise. Default `0.0` (identity). Characterized as **not defending** against either off-the-shelf or phantasm-trained detectors in [ML_STEGANALYSIS.md § Update 5](ML_STEGANALYSIS.md#update-5--option-d-passphrase-randomized-cost-noise-failed). Preserved for composition experiments and v0.3 research.
- **`--cost-sidecar <path>`** + **`--cost-function from-sidecar`** — load per-coefficient costs from an out-of-tree binary sidecar file (PHCOST v2/v3 format). Infrastructure for Option C iterative refinement, adversarial cost pipelines, and any future research that needs to compute costs in Python and consume them from the Rust embed pipeline. See [ML_STEGANALYSIS.md § Update 4](ML_STEGANALYSIS.md#update-4--option-c-single-step-adversarial-costs-failed).

These flags default to identity behavior. Existing scripts that don't set them continue to produce byte-identical output.

#### v0.3 L1 research updates (both closed negatively)

- **Option B''' — cover-source diversity hardening (v0.3 Update 7).** Scaled the multi-pass J-UNIWARD fine-tune from 198 covers to 500 unique Picsum covers. Result: detection rate *rises* from 89.9% to **96.8%** on d500 held-out split (97.5% on eval-198). The 89.9% number was NOT a Picsum-corpus overfit artifact; more cover diversity produces a stronger phantasm-aware detector, not a weaker one.
- **Option C-iter — iterative PGD adversarial costs (v0.3 Update 8).** Built on Update 4's single-step-failed infrastructure with PGD-style iteration. Result: J-UW-multi detection climbed 91.9% → 100.0% across T=0..4 iterations; every hyperparameter config in the 6-way sweep was worse than the J-UNIWARD baseline. **Per-coefficient cost-function adjustment is exhausted as an L1 defense direction.**

#### What's deferred to v0.4+

- **End-to-end differentiable embedding** — replace STC with a differentiable layer so the whole chain can be optimized together. Most ambitious.
- **ADV-EMB / ADV-IMB STC-aware iterative attacks** — jointly optimize coefficient *choice* (not just per-coefficient cost), aware of STC structure.
- **Multi-cover payload spreading** — spread the syndrome across N covers; defense in depth for the lossy-channel story.
- **Channel-adapter RS parameter tuning** — current 30/100 parity/data ratio isn't strong enough for `image` crate QF=85 recompression; next step is 200%+ overhead or bit-level FEC.
- **Non-AEAD mode for lossy channels** — AEAD's all-or-nothing recovery is brittle under sub-100% bit survival.

For the security-capacity curve, both detectors' detailed results, cross-detector consistency analysis, and the full caveats list, see [ML_STEGANALYSIS.md](ML_STEGANALYSIS.md). Direct links:
- [TL;DR](ML_STEGANALYSIS.md#tldr)
- [What we ran](ML_STEGANALYSIS.md#what-we-ran)
- [Detailed results — fixed payload](ML_STEGANALYSIS.md#detailed-results--fixed-payload-198-covers-3-kb---stealth-high)
- [Update 1 — UERD fine-tune](ML_STEGANALYSIS.md#update-1--uerd-fine-tune-option-b-complete--superseded-by-updates-2--3)
- [Update 2 — Symmetric J-UNIWARD fine-tune](ML_STEGANALYSIS.md#update-2--symmetric-j-uniward-fine-tune-option-b-validation--superseded-by-update-3)
- [Update 3 — Multi-passphrase fine-tunes](ML_STEGANALYSIS.md#update-3--multi-passphrase-fine-tunes-option-b-complete--supersedes-update-2s-gap-claim)
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

- **Encode arbitrary payload bytes into a JPEG or PNG cover.** JPEG uses content-adaptive DCT coefficient perturbation; PNG uses spatial-domain S-UNIWARD per-pixel LSB modification (v0.3 MVP, grayscale only).
- **Decode the payload out of a stego JPEG or PNG** given the same passphrase. Format auto-dispatched from the file extension.
- **Four content-adaptive cost functions**: Uniform (baseline), UERD (Guo/Ni/Shi 2015, divisibility-based redistribution into textured regions), J-UNIWARD (Holub & Fridrich 2014, Daubechies-8 wavelet residual-based DCT-domain), and **S-UNIWARD (Holub-Fridrich 2014, spatial-domain, v0.3 PNG path)**. Selectable via `--cost-function {uniform,uerd,j-uniward}` on JPEG; PNG auto-uses S-UNIWARD.
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

# Embed a payload into a JPEG cover (UERD is the default).
# Prefer --passphrase-env VAR or --passphrase-fd N over --passphrase; the latter
# leaves the secret visible in /proc/<pid>/cmdline and in shell history.
PHANTASM_PASSPHRASE="correct-horse-battery-staple" \
  ./target/release/phantasm embed \
    --input cover.jpg \
    --payload secret.txt \
    --passphrase-env PHANTASM_PASSPHRASE \
    --output stego.jpg

# Or via file descriptor (robust against env-var leakage through /proc/<pid>/environ):
./target/release/phantasm embed \
    --input cover.jpg \
    --payload secret.txt \
    --passphrase-fd 0 \
    --output stego.jpg <<< "correct-horse-battery-staple"

# Use J-UNIWARD against modern CNN steganalysis (off-the-shelf threat model).
# For a phantasm-aware adversary, L1 folds regardless — see "Attacker adaptation" above.
./target/release/phantasm embed \
    --cost-function j-uniward \
    --input cover.jpg \
    --payload secret.txt \
    --passphrase-env PHANTASM_PASSPHRASE \
    --output stego.jpg

# Grayscale PNG cover via spatial-domain S-UNIWARD (v0.3 MVP).
# The CLI auto-dispatches by .png extension on both embed and extract.
./target/release/phantasm embed \
    --input cover.png \
    --payload secret.txt \
    --passphrase-env PHANTASM_PASSPHRASE \
    --output stego.png

# Stabilize against Twitter re-encoding (research-only in v0.3 — see caveats below).
./target/release/phantasm embed \
    --cost-function uerd \
    --channel-adapter twitter \
    --input cover.jpg \
    --payload secret.txt \
    --passphrase-env PHANTASM_PASSPHRASE \
    --output stego.jpg

# Preserve pHash via wet-paper constraint (no-op on Robust covers, which is ~75%)
./target/release/phantasm embed \
    --cost-function uerd \
    --hash-guard phash \
    --input cover.jpg \
    --payload secret.txt \
    --passphrase-env PHANTASM_PASSPHRASE \
    --output stego.jpg

# Check a cover before embedding
./target/release/phantasm analyze cover.jpg
# Prints: capacity estimate, JPEG metadata, Sensitivity tier (Robust/Marginal/Sensitive),
#         Hash-guard (pHash) wet positions count

# Extract (must match the embed's --channel-adapter and --hash-guard flags)
./target/release/phantasm extract \
    --input stego.jpg \
    --passphrase-env PHANTASM_PASSPHRASE \
    --output recovered.txt

diff secret.txt recovered.txt   # byte-identical
```

- **Passphrase flags** (pick one): `--passphrase-env VAR` reads from a named env var; `--passphrase-fd N` reads from a file descriptor (recommended for pipeline use); `--passphrase "literal"` is still accepted but places the secret in `argv` and is explicitly research-only.
- `--cost-function` accepts `uniform`, `uerd` (default), or `j-uniward` on JPEG. On PNG, `uniform` passes through, `uerd` / `j-uniward` silently fall back to S-UNIWARD (spatial-domain Holub-Fridrich 2014), and `from-sidecar` returns a clean error.
- `--channel-adapter` accepts `none` (default) or `twitter`. **Honest v0.3 current state:** the adapter is an *architectural improvement* (Reed-Solomon ECC is now wired into the lossy path as of commit `cf8b1ac`), not a turnkey deliverability feature. Default RS parameters (30/100 parity/data, ~30% overhead, ~15 byte-error corrections per 3200-byte block) are **not yet strong enough to survive realistic JPEG recompression** — `phantasm-bench ber-sweep` shows 0/40 exact matches at 100/500/1000/3000-byte payloads on 720-pixel covers through `image` crate QF=85. Manual no-recompression round-trip with the adapter works at 55% capacity. **Treat `--channel-adapter` as research-only until v0.4 FEC tuning.**
- `--hash-guard` accepts `none` (default), `phash`, or `dhash`. JPEG only in v0.3; PNG hash-guard errors out with a clear message.
- `phantasm extract` accepts `--channel-adapter` and `--hash-guard` flags for forward compatibility, but they are no-ops: extract derives coefficient positions geometrically from the stego image (keyed on the passphrase + the pHash-stable image salt), and does not need to know which cost function, channel adapter, or hash-guard was used at embed time.

## What doesn't work yet

`v0.3.0` is a research checkpoint, not a finished tool. The following are explicitly NOT implemented or are MVP-level:

- **Channel-adapter deliverability is research-only.** The `--channel-adapter twitter` path now has Reed-Solomon ECC wired end-to-end (v0.3, commit `cf8b1ac`), but default RS parameters (30/100 parity/data) are not strong enough to survive realistic QF=85 recompression — `phantasm-bench ber-sweep` shows 0/40 exact-match extracts both pre- and post-ECC-wiring on the default config. Manual no-recompression round-trips work. Realistic delivery needs stronger RS parameters (200%+ overhead), bit-level FEC, or non-AEAD mode — all v0.4 scope. Instagram, Facebook, and other channels need dedicated profiles; MINICER doesn't generalize without them.
- **PNG support is grayscale-only MVP.** RGB PNGs are flattened to luma via ITU-R BT.601 on read; the output is an 8-bit grayscale PNG. Channel adapter on PNG, pHash-stable PNG salt, RGB color preservation, and PNG in `phantasm analyze` are all deferred.
- **pHash/dHash only, no PDQ.** Facebook's PDQ algorithm (which underlies CSAM-matching databases) is NOT implemented.
- **No pre-nudge for Sensitive covers.** About 10% of images are classified as pHash-Sensitive by the 3-tier classifier. On these, the hash guard falls back to large wet sets that may exhaust effective capacity.
- **L1 folds against phantasm-aware CNNs.** The v0.3 cover-source-diversity scale-up (Update 7) pushes detection to 96.8% at 500-cover scale; the iterative adversarial-costs experiment (Update 8) failed to defend. Every lever inside a hand-designed cost-function paradigm is exhausted. **Do NOT use phantasm for plausible-deniability against an adversary who can train a CNN on phantasm output.** The confidentiality of the payload (L2 + L3) is intact; the existence of the payload is not hidden from a phantasm-aware ML adversary.
- **Envelope format is still considered pre-stable.** `v0.3.0` uses envelope format v2 (HMAC-SHA256-16 MAC + HKDF key split + FORMAT_VERSION byte). The next envelope revision is expected to add auto-detection of `--channel-adapter` and `--hash-guard` configuration, which will be a format break.
- **CLI is pre-stable.** Flag names may still shift before `v1.0.0`.
- **No external security review.** Cryptographic primitives are used via established crates (`argon2`, `chacha20poly1305`, `hmac`, `sha2`, `hkdf`) but the composition, envelope layout, and integration have not been reviewed by anyone outside the project.

## Threat model

Phantasm is a **confidentiality** tool, not a plausible-deniability tool. See the [three-layer defense framing](#three-layer-defense-framing) for the exact scope.

Phantasm is intended for scenarios where:

1. You want to send a confidential payload over a channel that allows JPEG or PNG images.
2. The adversary may see the stego image and may know (or suspect) it contains hidden data — **we do not try to hide this from a phantasm-aware ML adversary.**
3. The adversary does NOT have your passphrase. Cryptographic confidentiality (L3) and position-recovery (L2) both gate on the passphrase via Argon2id + HKDF + XChaCha20-Poly1305 + HMAC-SHA256.
4. The adversary does NOT re-encode or compress the image in transit (or the channel adapter's ECC is strong enough to survive it — **which at default parameters in v0.3 it is not**; see `--channel-adapter` caveats above).

Note on cost-function selection within this threat model: against casual / off-the-shelf adversaries, cost-function choice genuinely differentiates. Against classical statistical detectors (Fridrich RS, SRM-lite), `--cost-function uerd` is the right choice and reduces detection rate by 2.5×. Against off-the-shelf modern CNN steganalysis (JIN-SRNet / Aletheia EffNet-B0), `--cost-function j-uniward` is the right choice — JIN-SRNet fires on phantasm J-UNIWARD output at only 16.2% above its 7.1% cover false-positive baseline. **Against a phantasm-aware CNN (Updates 3 + 7), neither cost function defends** — detection reaches ~97% at 500-cover training scale.

Phantasm is NOT suitable for:

- **Plausible deniability against a phantasm-aware adversary.** Any entity with the ability to train a CNN on phantasm output can detect phantasm stego at ~97%. Border crossings, compliance checks, or any setting where the mere fact of hidden data is incriminating are out of scope.
- **Services that re-encode uploaded JPEGs** other than in the no-recompression case. Even `--channel-adapter twitter` at default RS parameters does not survive `image` crate QF=85 recompression in our BER sweep. Realistic delivery through Twitter/Facebook/Instagram/WhatsApp is v0.4 scope.
- **Long-term archival** where envelope-format stability matters (envelope format v2 will break before v1.0).
- **Life-critical confidentiality.** There is no external security review.

The cryptographic primitives (Argon2id, XChaCha20-Poly1305, HMAC-SHA256, HKDF-SHA256) are used via the established `argon2`, `chacha20poly1305`, `hmac`, `sha2`, and `hkdf` crates. The composition, key schedule, and envelope layout are project-specific and have NOT been externally reviewed. Do not treat `v0.3.0` as production-ready cryptography.

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

`v0.3.0` ships: (1) **audit-driven CLI passphrase-handling fixes** — `--passphrase-env` and `--passphrase-fd` added, argv-visibility and log-file-persistence closed; (2) **PNG / S-UNIWARD spatial-domain MVP** — grayscale PNG cover support with the spatial-domain cost function; (3) **Reed-Solomon ECC wired into the channel-adapter lossy path** — architectural plumbing exists, default parameters are not yet strong enough to survive realistic JPEG recompression (v0.4 scope); (4) **two L1 research workstreams closed negatively** — cover-source diversity at 500 covers pushes detection UP to 96.8% (Update 7); iterative PGD adversarial costs produced 100% detection across all swept configs (Update 8); per-coefficient cost-function adjustment is exhausted as an L1 defense direction. **No breaking changes** to the v0.2.0 envelope format (still v2) or existing CLI flags. The CLI self-identifies as `phantasm 0.3.0 — research-grade`.

This is still a research-grade checkpoint, not production-ready cryptographic software: expect envelope-format breaks before `v1.0.0`, expect CLI flag shifts, and don't use it for anything where the confidentiality of the payload is life-critical. Read [STATUS.md](STATUS.md) for the full picture, [ML_STEGANALYSIS.md](ML_STEGANALYSIS.md) for the L1 research record (now through Update 8), and [CHANGELOG.md](CHANGELOG.md) for detailed release notes.
