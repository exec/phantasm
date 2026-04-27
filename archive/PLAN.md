# Phantasm — Project Planning Document

**Version:** 0.1.1-draft
**Date:** April 12, 2026
**Status:** Early Architecture / Pre-implementation

---

## 1. Vision

Phantasm is a compression-resilient, steganalysis-resistant image steganography tool written in Rust. It bridges the gap between sophisticated academic steganographic methods — which exist only in MATLAB research code — and practical open-source tools, which still rely on 2005-era LSB embedding with weak cryptography.

Phantasm's core thesis: **no single existing tool combines channel-adaptive preprocessing, content-adaptive distortion minimization, syndrome-trellis coding, perceptual hash preservation, and modern authenticated encryption.** Phantasm integrates all five.

### Design Principles

- **Security by default.** Every payload is encrypted with authenticated encryption before embedding. There is no "unencrypted" mode.
- **Robustness is a first-class parameter.** Users specify a target channel (e.g., `--channel twitter`) and Phantasm automatically configures preprocessing, embedding domain, and error correction for that channel's compression pipeline.
- **Stealth degrades gracefully.** Phantasm computes a stealth budget per image and warns the user when payload size pushes detectability above configurable thresholds.
- **No magic headers.** Embedding locations are derived from the passphrase. Without the correct passphrase, there is no evidence that Phantasm was used — no signatures, no magic bytes, no fixed offsets.
- **Plausible deniability.** Optional multi-layer payloads: different passphrases reveal different messages from the same image.

---

## 2. Threat Model

### What Phantasm defends against

| Threat | Defense |
|--------|---------|
| Visual inspection | Content-adaptive embedding in perceptually insignificant regions |
| Statistical steganalysis (SRNet, Yedroudj-Net, HILL detectors) | Adversarial cost optimization minimizing detectability against ensemble models |
| JPEG recompression (social media upload) | Channel-adaptive preprocessing + DCT-domain embedding + error correction |
| Format conversion (PNG → JPEG) | Cross-format embedding strategy with channel simulation |
| Perceptual hash matching (platform dedup/flagging) | pHash/PDQ preservation as an embedding constraint |
| Brute-force passphrase attacks | Argon2id with tunable memory/iteration parameters |
| Ciphertext analysis of payload | ChaCha20-Poly1305 AEAD with random padding to fixed block sizes |
| Payload existence detection via file size | No file size inflation — embedding replaces existing image data |

### What Phantasm does NOT defend against

- Targeted forensic analysis by a state-level actor with access to the original unmodified image (cover-stego pair comparison).
- Active adversaries who modify or destroy the image to prevent extraction.
- Side-channel attacks on the embedding machine itself.
- Rubber-hose cryptanalysis. (Multi-layer deniability helps, but has limits.)

---

## 3. Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                        CLI Interface                            │
│  phantasm embed | phantasm extract | phantasm analyze | ...     │
└──────────────┬──────────────────────────────────────────────────┘
               │
┌──────────────▼──────────────────────────────────────────────────┐
│                      Orchestrator                               │
│  Coordinates the pipeline, manages stealth budget, selects      │
│  channel profile, reports capacity/detectability estimates       │
└──────┬────────┬─────────┬──────────┬────────────┬───────────────┘
       │        │         │          │            │
┌──────▼──┐ ┌───▼────┐ ┌──▼───┐ ┌────▼─────┐ ┌───▼──────────┐
│ Channel │ │  Cost  │ │ Hash │ │   STC    │ │   Crypto     │
│ Adapter │ │ Engine │ │Guard │ │  Coder   │ │   Envelope   │
└─────────┘ └────────┘ └──────┘ └──────────┘ └──────────────┘
```

### Module Breakdown

#### 3.1 — CLI Interface (`phantasm-cli`)

The user-facing binary. Subcommands:

- `phantasm embed` — Hide a message or file in a cover image.
- `phantasm extract` — Recover a hidden payload from a stego image.
- `phantasm analyze` — Report an image's estimated embedding capacity at each stealth tier, channel robustness characteristics, and perceptual hash values.
- `phantasm channels` — List available channel profiles and their parameters.
- `phantasm bench` — Run steganalysis self-test against embedded images (requires optional ML feature flag).

Example usage:

```bash
# Embed with automatic channel detection
phantasm embed --input photo.jpg --payload secret.txt --passphrase "..." --output stego.jpg

# Embed targeting a specific platform
phantasm embed -i photo.jpg -p secret.txt --passphrase "..." --channel twitter -o stego.jpg

# Embed with explicit stealth tier
phantasm embed -i photo.jpg -p secret.txt --passphrase "..." --stealth max -o stego.jpg

# Multi-layer deniable embedding
phantasm embed -i photo.jpg \
  --layer "passphrase1:decoy.txt" \
  --layer "passphrase2:real_secret.txt" \
  -o stego.jpg

# Analyze capacity
phantasm analyze photo.jpg
# Output:
#   Format:        JPEG (QF=92)
#   Dimensions:    2048x1536
#   Capacity:
#     Stealth MAX:   1.2 KB  (≈0.05 bpp, detection error ≈0.49)
#     Stealth HIGH:  4.8 KB  (≈0.15 bpp, detection error ≈0.45)
#     Stealth MED:   12.1 KB (≈0.30 bpp, detection error ≈0.35)
#     Stealth LOW:   24.3 KB (≈0.50 bpp, detection error ≈0.20)
#   pHash:         a3 f2 91 0c 8b 44 e7 1d
#   Channel robustness:
#     Twitter:     ✓ (QF≥85, no resize needed)
#     Facebook:    ✓ (will be recompressed to QF≈72)
#     Instagram:   ⚠ (resize to 1080px will reduce capacity ~40%)
#     WhatsApp:    ✓ (use document mode for full preservation)

# Extract
phantasm extract --input stego.jpg --passphrase "..." --output recovered.txt
```

#### 3.2 — Orchestrator (`phantasm-core`)

The central coordination crate. Responsibilities:

- Parse the cover image and determine format, quality factor, dimensions, color space.
- Select and apply the channel profile.
- Compute the stealth budget: how many bits can be embedded at each detectability tier.
- Coordinate the pipeline: preprocess → compute costs → apply hash constraints → encode via STC → write stego image.
- For extraction: derive embedding locations from passphrase → extract via STC → decrypt → return payload.

The orchestrator owns the `EmbedPlan` struct:

```rust
pub struct EmbedPlan {
    pub channel: ChannelProfile,
    pub stealth_tier: StealthTier,
    pub capacity_bits: usize,
    pub payload_bits: usize,        // actual message after encryption + padding
    pub ecc_bits: usize,            // Reed-Solomon overhead
    pub estimated_detection_error: f64,  // 0.5 = undetectable, 0.0 = trivially detected
    pub hash_constrained_positions: usize, // coefficients excluded for hash preservation
}
```

#### 3.3 — Channel Adapter (`phantasm-channel`)

Simulates and pre-compensates for platform-specific image processing pipelines.

Each channel profile encodes:

```rust
pub struct ChannelProfile {
    pub name: &'static str,
    pub jpeg_quality: Option<u8>,           // recompression QF (None = lossless)
    pub max_dimension: Option<u32>,         // resize threshold
    pub chroma_subsampling: ChromaSub,      // 4:4:4, 4:2:0, etc.
    pub applies_enhancement: bool,          // e.g., Facebook's undocumented filter
    pub strips_metadata: bool,
    pub overflow_strategy: OverflowStrategy,
    pub ecc_default: EccParams,
}
```

Built-in profiles (v0.1):

| Channel | QF | Max Dim | Enhancement | Chroma | Notes |
|---------|----|---------|-------------|--------|-------|
| `lossless` | — | — | No | — | No recompression assumed |
| `facebook` | 72 | 2048px | Yes | 4:2:0 | MINICER-style channel elimination |
| `twitter` | 85 | 4096px | No | 4:2:0 | Varies by file size |
| `instagram` | 75 | 1080px | Mild | 4:2:0 | Aggressive resize |
| `whatsapp-photo` | 60 | 1600px | No | 4:2:0 | Very lossy |
| `whatsapp-doc` | — | — | No | — | Document mode bypass |
| `signal` | — | — | No | — | Minimal processing |
| `generic-75` | 75 | — | No | 4:2:0 | Conservative default |

The channel adapter implements two key preprocessing steps:

1. **Overflow alleviation** (ROAST-style): Identify pixels that would overflow [0, 255] after IDCT→modify→DCT→requantize. Pre-adjust these pixels to create headroom. The boundary-preserving variant (Cheng 2024) targets only 8×8 block edge pixels for lower distortion.

2. **Channel simulation**: Compress the cover image through the target channel's pipeline, then embed in the *post-channel* DCT coefficients to ensure the embedding domain is stable under recompression. This is the MINICER insight — embed in what the channel will produce, not what you start with.

#### 3.4 — Cost Engine (`phantasm-cost`)

Computes per-coefficient embedding costs that determine *where* to embed for minimum detectability.

**Phase 1 (v0.1): Classical distortion functions**

Implement J-UNIWARD as the baseline. It computes costs by measuring the impact of each DCT coefficient change on a bank of directional wavelet filters (Daubechies-8). Changes in textured/noisy regions are cheap; changes in smooth regions are expensive.

```rust
pub trait DistortionFunction {
    /// Returns the cost of modifying coefficient at (block_row, block_col, dct_pos)
    /// by +1 and -1 respectively.
    fn cost(&self, image: &CoverImage, block: BlockCoord, pos: DctPos) -> (f64, f64);
}
```

Also implement:
- **HILL**: High-pass, low-pass, low-pass filter cascade. Simpler than UNIWARD, competitive security.
- **UERD**: Uniform Embedding Revisited Distortion. JPEG-native, faster than J-UNIWARD.

**Future work (post-v1): Adversarial cost adjustment**

Explicitly out of scope for v1. Adversarial optimization against a steganalysis ensemble is a research-grade ML subsystem layered on top of an already research-grade systems project — bundling the two into the initial release would roughly double the surface area and timeline without which the tool is already competitive with every shipping open-source alternative.

When revisited post-v1, the sketch is: feature flag `adversarial`, ships a distilled quantized steganalyzer ensemble (~5MB ONNX), adjusts costs via `adjusted_cost[i] = base_cost[i] * (1 + λ * ∂L/∂x[i])` where L is the ensemble detection loss and λ is a tunable strength parameter. Runs only at embed time; extraction never needs the model. Implementation will build on whatever shape the v1 cost engine has taken by then.

#### 3.5 — Hash Guard (`phantasm-hash`)

The novel contribution. Computes perceptual hashes of the cover image and constrains embedding to preserve them.

Supported hash algorithms:
- **pHash** (DCT-based, 64-bit): The most widely deployed perceptual hash.
- **PDQ** (Facebook's perceptual hash, 256-bit): Used by Facebook/Meta for content matching.
- **dHash** (difference hash, 64-bit): Gradient-based, simpler but less robust.

The hash guard works in three phases:

1. **Hash computation.** Compute the cover image's perceptual hash(es) using the standard algorithms.
2. **Margin analysis.** For each bit of the output hash, measure how close the underlying coefficient sits to its decision threshold (the median, in the case of pHash). Important correction from the original design: pHash does *not* share the DCT domain with JPEG. It downsamples the image to 32×32 and takes the 2D DCT of *that*, so the mapping from a given JPEG 8×8 DCT coefficient to a given pHash bit is non-analytic — it has to be measured empirically. The margin analysis simulates a small perturbation on each candidate JPEG block and checks whether any pHash bit flips.
3. **Classification and constraint application.** Images sort into two regimes. If every pHash bit is robust (large margin), the hash guard is a no-op and imposes zero capacity cost — the downsampling averages away any individual block perturbation. This is the common case. If one or more bits are sensitive (small margin), the guard falls back to one of two strategies: (a) use per-block perturbation budgets as cost ceilings in the STC step, or (b) pre-nudge the cover to move sensitive pHash bits clear of their thresholds before embedding begins. Strategy (a) is simpler; strategy (b) is required when even small perturbations on a single critical block cross a threshold. Empirically (see below), some images are entirely incompatible with coefficient exclusion alone and must use strategy (b) or be refused with a clear diagnostic.

```rust
pub struct HashGuard {
    pub algorithms: Vec<HashAlgorithm>,
    pub cover_hashes: HashMap<HashAlgorithm, Vec<u8>>,
    pub protected_coefficients: HashSet<(BlockCoord, DctPos)>,
}

impl HashGuard {
    pub fn analyze(image: &CoverImage, algorithms: &[HashAlgorithm]) -> Self;
    pub fn apply_constraints(&self, costs: &mut CostMap);
    pub fn verify(&self, stego: &StegoImage) -> HashVerification;
}
```

**Why this is novel:** Perceptual hash preservation has not been explored as a steganographic constraint in published literature. It is practically valuable because platforms increasingly use perceptual hashes for content matching, deduplication, and CSAM detection. An image whose pHash changes after steganographic embedding might be flagged as modified; one whose hash is preserved will not.

**Capacity impact (measured, Phase -1 Spike B).** Measured empirically on a 60-image Picsum corpus (Lorem Picsum / Unsplash, 512×512, QF=85) at a 10% stealth budget. Full methodology and raw numbers live in `spikes/phash-overlap/REPORT.md`.

- **Median penalty: 0%.** Roughly 75% of images have every pHash bit sitting far from its decision threshold, so block-level perturbations never flip a bit. The hash guard is a no-op for these.
- **Mean penalty: 8.4%.** Lands inside the original 5–15% estimate, but for the wrong reason — the mean is driven by a long tail, not a uniform cost.
- **90th percentile: 14.0%.** Still within the original estimate.
- **Worst case: ~100%.** ~10% of images have one or more pHash bits within ~1 downsampled-pixel quantization step of the median threshold. On these, a single perturbed JPEG block anywhere can flip a hash bit, so coefficient exclusion alone doesn't work — they require strategy (b) (pre-nudge) or must be refused.

**Architectural implication.** The bimodal distribution means capacity estimates must be **per-image**, not a flat overhead. `phantasm analyze` classifies each cover into three sensitivity tiers:

| Tier | Observed share | Hash-guard behavior | Capacity impact |
|------|---------------|---------------------|-----------------|
| Robust | ~75% | No-op | 0% |
| Marginal | ~15% | Per-block cost ceilings (strategy a) | 0–30% |
| Sensitive | ~10% | Cover pre-nudge (strategy b) or refuse | 30%+ or N/A |

**PDQ caveat.** Spike B measured pHash only. PDQ uses a 64×64 pre-DCT stage and a Jarosz filter; its penalty distribution is likely tighter (more sensitive images, higher mean penalty). A follow-up PDQ spike on the same corpus must run before Phase 3 implementation begins — tracked as a follow-up task.

#### 3.6 — STC Coder (`phantasm-stc`)

Implements Syndrome-Trellis Codes for near-optimal embedding.

STC is the standard coding scheme in academic steganography. It uses a binary linear convolutional code defined by a parity-check matrix H, and finds the minimum-cost modification vector via the Viterbi algorithm on a trellis:

```
Find stego = argmin Σ cost[i] * |cover[i] - stego[i]|
subject to: H * stego = message (mod 2)
```

Implementation requirements:

- **Single-layer STC** for binary (±1) spatial-domain embedding.
- **Double-layer STC** for ternary (−1, 0, +1) JPEG DCT coefficient embedding. This is critical for JPEG — DCT coefficients can be incremented or decremented, and the optimal direction depends on the cost function.
- Constraint height h = 7–10 (configurable). Higher h = closer to optimal but slower. h=10 is standard in research.
- Wet paper coding: coefficients with infinite cost (from hash guard or overflow analysis) are excluded from the coding domain without reducing capacity from the remaining positions.

```rust
pub struct StcEncoder {
    pub constraint_height: u8,   // h, typically 7-10
    pub parity_check: ParityMatrix,
}

impl StcEncoder {
    pub fn embed(
        &self,
        cover: &[i16],          // DCT coefficients
        message: &[u8],         // encrypted payload
        costs_plus: &[f64],     // cost of +1 modification
        costs_minus: &[f64],    // cost of -1 modification
    ) -> Result<Vec<i16>, StcError>;  // stego coefficients

    pub fn extract(
        &self,
        stego: &[i16],
    ) -> Vec<u8>;
}
```

This is a pure Rust port of the Binghamton DDE Lab's reference C++ implementation. The algorithmic core is well-defined — the challenge is correctness and performance optimization.

**Error correction integration:** Reed-Solomon codes are applied to the message *before* STC encoding. The RS parameters (redundancy level) are determined by the channel profile:

| Channel | RS Redundancy | Rationale |
|---------|---------------|-----------|
| `lossless` | None | No errors expected |
| `signal` | 5% | Minimal processing |
| `twitter` | 15% | Moderate recompression |
| `facebook` | 25% | Aggressive recompression + enhancement |
| `whatsapp-photo` | 35% | Very lossy compression |

#### 3.7 — Crypto Envelope (`phantasm-crypto`)

All payloads are encrypted before embedding. There is no bypass.

**Key derivation:**

```
salt ← random 32 bytes (embedded at predetermined positions derived from passphrase)
key ← Argon2id(passphrase, salt, memory=64MB, iterations=3, parallelism=4)
```

Argon2id is the current gold standard KDF, resistant to both GPU and side-channel attacks. Memory and iteration parameters are configurable for constrained environments.

**Encryption:**

```
nonce ← random 24 bytes
ciphertext ← ChaCha20-Poly1305(key, nonce, plaintext || metadata)
```

The metadata block (encrypted alongside the payload) contains:
- Original filename (optional, can be suppressed)
- Payload length in bytes
- Content type flag (raw bytes / UTF-8 text / file)
- Phantasm version (for forward compatibility)
- Padding to next block boundary

**Padding:** Ciphertext is padded with random bytes to a fixed set of block sizes (256B, 1KB, 4KB, 16KB, 64KB, 256KB) to prevent payload size inference from the number of modified coefficients.

**Multi-layer deniability:** When multiple layers are specified, each layer derives independent embedding locations from its own passphrase (via HKDF-SHA256). Layers are designed to overlap minimally — the orchestrator allocates non-overlapping coefficient subsets to each layer using passphrase-derived permutations. An extractor with passphrase A sees only layer A's message; the existence of layer B is indistinguishable from normal image noise.

---

## 4. Crate Structure

```
phantasm/
├── Cargo.toml                    # Workspace root
├── README.md
├── LICENSE                       # MIT OR Apache-2.0
│
├── phantasm-cli/                 # Binary crate
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs
│       ├── commands/
│       │   ├── embed.rs
│       │   ├── extract.rs
│       │   ├── analyze.rs
│       │   └── bench.rs
│       └── output.rs             # Terminal formatting, progress bars
│
├── phantasm-core/                # Orchestrator library
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── orchestrator.rs       # Pipeline coordination
│       ├── plan.rs               # EmbedPlan, capacity estimation
│       ├── stealth.rs            # Stealth tier definitions + budget calculation
│       └── error.rs
│
├── phantasm-image/               # Image I/O and DCT access
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── jpeg.rs               # JPEG decode/encode with raw DCT coefficient access
│       ├── png.rs                # PNG decode/encode
│       ├── dct.rs                # Forward/inverse DCT, quantization tables
│       ├── pixel.rs              # Pixel-level operations
│       └── color.rs              # Color space conversions (YCbCr, RGB)
│
├── phantasm-channel/             # Channel simulation and preprocessing
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── profiles.rs           # Built-in channel profiles
│       ├── simulator.rs          # Channel simulation engine
│       ├── overflow.rs           # ROAST-style overflow alleviation
│       └── stabilizer.rs         # MINICER-style coefficient stabilization
│
├── phantasm-cost/                # Distortion cost computation
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── uniward.rs            # J-UNIWARD implementation
│       ├── hill.rs               # HILL distortion
│       ├── uerd.rs               # UERD distortion
│       ├── costmap.rs            # CostMap data structure
│       └── adversarial.rs        # [feature: adversarial] Gradient-based cost adjustment
│
├── phantasm-hash/                # Perceptual hash guard
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── phash.rs              # pHash computation
│       ├── pdq.rs                # PDQ hash (Facebook/Meta)
│       ├── dhash.rs              # Difference hash
│       ├── guard.rs              # Constraint application to cost maps
│       └── verify.rs             # Post-embedding hash verification
│
├── phantasm-stc/                 # Syndrome-Trellis Codes
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── encoder.rs            # STC embedding (Viterbi on trellis)
│       ├── decoder.rs            # STC extraction (syndrome computation)
│       ├── parity.rs             # Parity-check matrix generation
│       ├── double_layer.rs       # Ternary embedding for JPEG
│       └── wet_paper.rs          # Wet paper coding for constrained positions
│
├── phantasm-crypto/              # Cryptographic envelope
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── kdf.rs                # Argon2id key derivation
│       ├── cipher.rs             # ChaCha20-Poly1305 AEAD
│       ├── padding.rs            # Random padding to block sizes
│       ├── metadata.rs           # Payload metadata (encrypted)
│       ├── location.rs           # Passphrase-derived embedding locations
│       └── multilayer.rs         # Multi-layer deniable embedding
│
├── phantasm-ecc/                 # Error correction codes
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       └── reed_solomon.rs       # RS encoding/decoding
│
└── tests/                        # Integration tests
    ├── roundtrip.rs              # Embed → extract correctness
    ├── compression.rs            # Embed → recompress → extract
    ├── capacity.rs               # Capacity estimation accuracy
    └── fixtures/                 # Test images (various formats, sizes, content types)
```

---

## 5. Key Dependencies (Rust Crates)

| Crate | Purpose | Notes |
|-------|---------|-------|
| `clap` | CLI argument parsing | v4, derive API |
| `image` | PNG decode/encode, pixel manipulation | |
| `jpeg-decoder` | JPEG decoding with DCT access | May need fork for raw coefficient access |
| `zune-jpeg` | Alternative JPEG decoder, faster | Evaluate vs jpeg-decoder for DCT access |
| `mozjpeg` | JPEG encoding (libjpeg-turbo compatible) | C binding, high-quality encoding |
| `ndarray` | N-dimensional arrays for DCT/filter ops | |
| `argon2` | Argon2id KDF | RustCrypto project |
| `chacha20poly1305` | AEAD encryption | RustCrypto project |
| `reed-solomon-erasure` | RS error correction | Evaluate vs custom impl |
| `indicatif` | Progress bars | CLI UX |
| `anyhow` / `thiserror` | Error handling | `thiserror` in libraries, `anyhow` in CLI |
| `rayon` | Parallel iteration | Cost computation is embarrassingly parallel |
| `tract-onnx` | ONNX model inference | [feature: adversarial] only |

### Critical dependency risk: Raw DCT coefficient access

The biggest implementation risk is JPEG DCT coefficient access. Most Rust JPEG libraries decode to pixels, discarding coefficient data. Options:

1. **Fork `jpeg-decoder`** to expose quantized DCT coefficients before dequantization and IDCT.
2. **Bind to `libjpeg-turbo`** via FFI — it exposes `jvirt_barray_ptr` for direct coefficient access. This is what every C/C++ stego tool uses.
3. **Write a minimal JPEG coefficient reader** from scratch. JPEG's DCT encoding is well-specified; a reader that only needs coefficients (not pixels) is simpler than a full decoder.

Recommendation: Start with option 2 (libjpeg-turbo FFI) for proven correctness, plan option 3 as a long-term pure-Rust goal.

---

## 6. Phased Roadmap

### Phase -1 — Critical De-risking Spikes (Week 0)

Two items can individually invalidate the project's architecture or core thesis. They must be validated in isolation before any substantive code is written on top of them.

**Spike A — DCT coefficient round-trip.** Standalone throwaway crate (`spikes/dct-roundtrip`) that uses `libjpeg-turbo` via FFI to:

1. Decode a JPEG to quantized DCT coefficients via `jpeg_read_coefficients()`.
2. Modify a single mid-frequency AC coefficient by ±1.
3. Write the modified coefficients back via `jpeg_write_coefficients()`.
4. Re-decode the output and assert bit-exact persistence of every coefficient.

Until this round trip works reliably on Rust stable with a vendored or system libjpeg-turbo, the rest of the architecture is speculative. Decision point: if FFI write-back proves unworkable, the project pivots to either (a) a minimal pure-Rust JPEG coefficient reader/writer or (b) a different file format entirely. This is the single highest-risk item in the plan and cannot be deferred.

**Spike B — pHash constraint overlap.** Standalone analysis (`spikes/phash-overlap/`) that empirically bounds the capacity cost of perceptual-hash preservation on real images:

1. Compute pHash on a corpus of ~50–100 JPEG photos with varied content (BOSSbase, a public CC dataset, or any representative image set).
2. For each image, analytically identify the DCT positions that contribute to the pHash output (pHash uses a low-frequency DCT block of the 32×32 downsampled luminance).
3. Report: fraction of coefficients excluded by the hash-preservation constraint, distribution of their intrinsic cost under a simple content-adaptive proxy (local gradient magnitude or variance), and estimated capacity loss vs. the unconstrained embedding domain.

The project's central novelty claim is that pHash preservation is a tractable constraint with modest capacity cost (estimated 5–15% in §3.5). If the empirical overlap between hash-critical and low-cost coefficients is severe (>30% capacity loss on typical photos), the thesis needs revision before investing in the surrounding stack.

**Milestone (Phase -1):** Both spikes complete with written reports in their respective directories. DCT round trip proven bit-exact. pHash constraint cost empirically bounded on a real corpus.

### Phase 0 — Foundations (Weeks 1–3)

Split into four independent sub-phases, each with its own milestone. Sub-phases 0.2, 0.3, and 0.4 have no dependencies on each other or on 0.1 (beyond the workspace skeleton) and can proceed in parallel.

#### Phase 0.1 — Workspace and image I/O

- Cargo workspace, CI (GitHub Actions), rustfmt/clippy config, basic `.gitignore`.
- `phantasm-image`: JPEG coefficient read/write productized from the Phase -1 spike, PNG pixel access via the `image` crate, forward/inverse DCT helpers, YCbCr↔RGB conversions.
- **Milestone:** `phantasm-image` round-trips JPEG coefficients bit-exact and reads/writes PNG pixels, with integration tests on a fixture image set.

#### Phase 0.2 — Crypto envelope

- `phantasm-crypto`: Argon2id KDF, ChaCha20-Poly1305 AEAD, random padding to fixed block sizes, metadata struct with authenticated serialization, HKDF-based location-key derivation.
- Test vectors from RFC 8439 (ChaCha20-Poly1305) and RFC 9106 (Argon2id).
- **Milestone:** Arbitrary payloads round-trip through the full envelope with RFC test vectors passing and padding behavior verified.

#### Phase 0.3 — STC coder

- `phantasm-stc`: single-layer STC encoder (Viterbi on trellis), decoder (syndrome computation), parity-check matrix generation, wet paper support for infinite-cost positions.
- Roundtrip tests with random messages and random cost vectors. Constraint height `h` configurable (default 7).
- Cross-validated against the pySTC reference implementation where feasible.
- **Milestone:** Single-layer STC correctly embeds and extracts messages at constraint heights 7–10, with distortion near the rate-distortion bound on random inputs.

#### Phase 0.4 — CLI scaffolding

- `phantasm-cli` binary with `clap` derive API, stub subcommands (`embed`, `extract`, `analyze`, `channels`, `bench`), structured error reporting via `anyhow`, progress-bar plumbing via `indicatif`.
- **Milestone:** `phantasm --help` lists every subcommand; each subcommand prints a clear "not yet implemented" message and exits cleanly.

### Phase 1 — Minimum Viable Steganography (Weeks 4–7)

- Implement J-UNIWARD distortion function in `phantasm-cost`.
- Implement double-layer STC for ternary JPEG embedding.
- Wire the pipeline: read JPEG → compute costs → STC embed → write JPEG.
- Implement `phantasm-ecc`: Reed-Solomon encoding/decoding.
- Basic `lossless` channel profile (no recompression handling).
- `phantasm embed` and `phantasm extract` working end-to-end for JPEG.
- **Milestone:** Can hide and recover an encrypted message in a JPEG image. Detectability competitive with academic J-UNIWARD implementations.

### Phase 2 — Compression Resilience (Weeks 8–11)

- Implement `phantasm-channel`: channel profiles for major platforms.
- Implement ROAST-style overflow alleviation.
- Implement MINICER-style channel simulation and coefficient stabilization.
- Add RS error correction with channel-adaptive parameters.
- Integration tests: embed → simulate platform recompression → extract.
- **Milestone:** Can survive Facebook-level recompression (QF=72 + enhancement) with <1% bit error rate after ECC.

### Phase 3 — Hash Guard + Stealth Tiers (Weeks 12–15)

- Implement `phantasm-hash`: pHash and dHash computation.
- Implement hash constraint application to cost maps.
- Implement stealth tier system and capacity analysis.
- `phantasm analyze` command.
- Add PNG support (spatial-domain embedding path, S-UNIWARD costs).
- Post-embedding verification: hash check, capacity check, stealth estimate.
- **Milestone:** Perceptual hash preservation verified. Stealth budget system operational.

### Phase 4 — Advanced Features (Weeks 16+)

- Multi-layer deniable embedding.
- PDQ hash support.
- `phantasm bench` self-test command.
- Cross-format resilience (PNG input → JPEG-targeted embedding).
- HILL and UERD alternative distortion functions.
- Performance optimization: SIMD for DCT, rayon parallelism for cost computation.
- Shell completions, man pages, packaging.

### Future Work — Post-v1

- **Adversarial cost adjustment** against a distilled steganalyzer ensemble (see §3.4). Explicitly deferred: this is an ML research subsystem and would roughly double the v1 timeline without which the tool is already competitive.
- **Lattice-based errorless embedding** (Butora, Puteaux, Bas 2022–2023) as an alternative robustness strategy for extremely high-value covers.
- **Diffusion / latent-space generative methods** (CRoSS, Pulsar, RoSteALS) as a second product track for users with GPU infrastructure.

---

## 7. Stealth Tier System

Phantasm exposes a simple four-tier system that maps to bits-per-pixel (bpp) ranges and estimated detection error rates. Detection error is measured as P_E = ½(P_FA + P_MD), where 0.5 means the steganalyzer is no better than random guessing.

| Tier | bpp Range | Detection Error (est.) | Use Case |
|------|-----------|----------------------|----------|
| `max` | 0.01–0.05 | ≈0.49–0.50 | Short text messages. Virtually undetectable. |
| `high` | 0.05–0.20 | ≈0.40–0.49 | Moderate messages. Safe against most automated analysis. |
| `medium` | 0.20–0.40 | ≈0.25–0.40 | Larger payloads. Detectable by targeted deep-learning steganalysis. |
| `low` | 0.40–0.60 | ≈0.10–0.25 | Maximum capacity. Detectable by competent analysis. |

The actual bpp boundaries are per-image — a highly textured photograph supports much higher bpp at a given stealth level than a smooth gradient. The `analyze` command reports image-specific capacity for each tier.

When a user's payload exceeds their selected tier's capacity, Phantasm:
1. Reports the capacity shortfall.
2. Shows what tier would accommodate the payload.
3. Asks for confirmation before proceeding at a lower stealth level.
4. Never silently downgrades stealth.

---

## 8. Embedding Location Derivation (No Headers)

Traditional stego tools embed a header at fixed positions containing payload length, format flags, etc. This is a forensic fingerprint — a steganalyst who knows the tool can check those positions directly.

Phantasm derives everything from the passphrase:

```
master_key ← Argon2id(passphrase, image_hash_salt)
location_key ← HKDF-SHA256(master_key, "phantasm-locations-v1")
embedding_permutation ← Fisher-Yates shuffle of coefficient indices, seeded by location_key
```

The first N positions in the permuted sequence carry the encrypted payload (which includes its own length in the authenticated metadata). Without the passphrase, the permutation is unknown and no subset of coefficients is distinguishable from any other.

The `image_hash_salt` is derived from the cover image's perceptual-hash-stable features — specifically, the quantized low-frequency luminance block that pHash uses (the 8×8 DCT of the 32×32 downsampled Y channel). These coefficients are (a) not modified during embedding because the hash guard protects them, and (b) stable under JPEG recompression because pHash is designed to be robust to exactly that class of transformation. This binds the embedding to a specific image without storing any additional data and without relying on raw DC coefficients, which drift under recompression. The extractor reproduces the same salt by running the same pHash pre-computation on the received stego image.

### Extraction termination

A header-free design raises a question: without a length field at a known offset, how does the extractor know where the payload ends? Phantasm resolves this without reintroducing a forensic fingerprint:

1. The extractor computes the image's theoretical maximum embeddable capacity — the same computation `phantasm analyze` performs, determined entirely from image dimensions, format, and quantization tables — and reads that many STC symbols into a candidate buffer. This number is a function of the image only, not of the payload size.
2. The candidate buffer is passed to the crypto envelope. ChaCha20-Poly1305 authentication either succeeds, in which case the authenticated metadata inside the plaintext provides the true payload length and content type, or it fails cleanly.
3. If authentication fails, one of three things is true: the passphrase is wrong, the image is not a Phantasm stego image, or the image has been damaged beyond ECC recovery. The extractor reports "no payload found" and exits; it cannot distinguish these cases, and that indistinguishability is a feature, not a limitation.

Because the extractor always reads the same number of symbols for a given image (determined only by the cover, not the hidden payload), extraction work and timing leak nothing about whether a payload exists or how large it is. The cost is a constant-size extraction pass regardless of true payload size, which is negligible compared to the DCT and STC work it sits on top of.

---

## 9. Testing Strategy

### Unit tests

Every module has unit tests for its core algorithms. Critical areas:

- **STC correctness:** Roundtrip tests with known parity matrices. Fuzz testing with random messages and cost arrays.
- **Crypto correctness:** Test vectors from RFC 8439 (ChaCha20-Poly1305) and RFC 9106 (Argon2).
- **Cost function correctness:** Compare J-UNIWARD output against reference MATLAB implementation on standard test images (BOSSbase).
- **Hash stability:** Verify that pHash of stego image matches pHash of cover after hash-guarded embedding.

### Integration tests

- **Roundtrip:** Embed → extract for every supported format, stealth tier, and channel.
- **Compression roundtrip:** Embed → recompress at various QFs → extract. Measure bit error rate before and after ECC.
- **Cross-tool validation:** Compare STC output against pySTC reference implementation.
- **Capacity accuracy:** Verify that `analyze` predictions match actual embeddable capacity within 10%.

### Steganalysis validation (Phase 4)

- Run SRNet and Yedroudj-Net against Phantasm stego images vs. clean covers.
- Compare detection error rates against published results for J-UNIWARD at equivalent bpp.
- Test on BOSSbase 1.01 (10,000 grayscale images) — the standard academic benchmark.

---

## 10. Open Questions and Research Risks

1. **Raw DCT coefficient write-back.** Writing modified coefficients back to a valid JPEG without re-encoding through the full DCT pipeline is essential. libjpeg-turbo supports this via `jpeg_write_coefficients()`, but the Rust binding quality is unknown. This is the single highest-risk implementation item.

2. **Hash guard capacity impact.** The theoretical estimate (5–15% capacity loss) needs empirical validation across diverse image types. Worst case: images with very flat regions where hash-critical coefficients overlap heavily with the low-cost embedding domain.

3. **Adversarial cost adjustment generalizability.** Adversarial optimization against one steganalyzer may not transfer to others. The ensemble approach (distilled SRNet + Yedroudj-Net) is designed to mitigate this, but needs validation.

4. **Channel profile maintenance.** Social media platforms change their compression pipelines without notice. Phantasm needs a mechanism for updating profiles — either a config file users can edit, or a calibration mode that empirically measures a platform's current pipeline.

5. **Multi-layer collision.** Deniable multi-layer embedding requires that layers don't destructively interfere. The passphrase-derived permutation approach should produce non-overlapping subsets with high probability, but worst-case collision rates need analysis.

6. **Legal considerations.** Steganography tools exist in a complex legal landscape. Phantasm should include clear documentation that it is designed for legitimate privacy use cases (journalist source protection, personal privacy, security research) and should not be used for illegal purposes.

---

## 11. Comparable Work and Differentiation

| Tool | Language | Embedding | Crypto | Robustness | Steganalysis Resistance |
|------|----------|-----------|--------|------------|------------------------|
| Steghide | C++ | Graph-theoretic | AES-128-CBC, MD5 KDF | None | None |
| OpenStego | Java | LSB | Weak password | None | None |
| F5 | Java | Matrix coding in DCT | None | Partial (DCT domain) | Resists chi-square only |
| stegano-rs | Rust | LSB in PNG | Sparse password | None | None |
| ST3GG | Python/JS | 112+ LSB methods | AES-256-GCM, PBKDF2 | None | None |
| **Phantasm** | **Rust** | **STC + J-UNIWARD in DCT** | **ChaCha20-Poly1305, Argon2id** | **Channel-adaptive + ECC** | **Content-adaptive + hash guard** |

Every existing tool fails on at least three of the five pillars. Phantasm is the first to attempt all five.

---

## 12. License

Dual-licensed under MIT and Apache 2.0, following Rust ecosystem convention. The adversarial ML component (feature-flagged) may carry additional model license terms depending on the training data used.
