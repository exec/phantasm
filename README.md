# phantasm

**Content-adaptive JPEG steganography in Rust.** Research-grade alpha.

Phantasm hides data in the DCT coefficients of JPEG images using a content-adaptive distortion minimization scheme (UERD) driven through syndrome-trellis coding, sealed in an authenticated cryptographic envelope (Argon2id + XChaCha20-Poly1305 + HMAC-SHA256). The thesis of the project is that combining channel-adaptive preprocessing, content-adaptive distortion, syndrome-trellis coding, perceptual-hash preservation, and modern authenticated encryption in a single tool is a capability no existing steganography tool offers. As of `v0.1.0-alpha`, three of those five legs are working end-to-end.

## Headline research result

On a 198-image seed-regenerable research corpus with a fixed 3,723-byte payload at ~27% raw capacity:

| Metric | Uniform embedding | UERD content-adaptive | Δ |
|---|---:|---:|---:|
| **Fridrich RS detection rate** (Aletheia-faithful port, threshold 0.05) | **73% – 75%** | **31%** | **−44 pp / 2.4× reduction** |
| Mean Fridrich RS max_rate | 0.48 | 0.05 | 8.8× reduction |
| **SRM-lite L2 distance** (mean) | 0.649 | 0.189 | **3.4× lower** |
| **SSIM win rate** (paired) | — | — | **198/198 (100%)** |
| Mean file-size inflation (cover → stego) | +10,189 B | +3,057 B | 3.3× smaller |
| File-size inflation paired win rate | — | — | 196/198 (99%) |

**Paired (same image, 198 pairs):** UERD beats Uniform on Fridrich RS in 194–196 of 198 images, on SRM-lite L2 in 198 of 198, and on file-size inflation in 196 of 198. Numbers reproduce across day-1, day-2, and day-3 runs within measurement noise (~2 percentage points for detection rate, sub-byte for file size).

Detector: native Rust port of Aletheia's Fridrich 2001 RS attack, validated to reproduce the Aletheia reference detection of `0.053` on its own sample stego at `0.0513` (within 0.002 tolerance).

## What phantasm does

- **Encode arbitrary payload bytes into a JPEG cover** using content-adaptive DCT coefficient perturbation
- **Decode the payload out of a stego JPEG** given the same passphrase
- **Content-adaptive cost minimization** via UERD (Guo/Ni/Shi 2015): modifications are redistributed into textured, high-frequency regions of the image, away from smooth skies and flat surfaces where human perception and steganalysis are most sensitive
- **Syndrome-trellis coding** (Filler/Judas/Fridrich 2011) at rate 1/4, constraint height 7, for rate-distortion-optimal payload embedding
- **Authenticated encryption envelope**: Argon2id(64 MiB / 3 iterations / 4 threads) → HKDF-SHA256 split into `(aead_key, mac_key)` → XChaCha20-Poly1305 AEAD + HMAC-SHA256 MAC (16-byte truncated) for fast wrong-passphrase detection
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

# Compare against uniform (non-content-adaptive) embedding
./target/release/phantasm embed \
    --cost-function uniform \
    --input cover.jpg \
    --payload secret.txt \
    --passphrase "correct-horse-battery-staple" \
    --output stego-uniform.jpg

# Extract
./target/release/phantasm extract \
    --input stego.jpg \
    --passphrase "correct-horse-battery-staple" \
    --output recovered.txt

diff secret.txt recovered.txt   # byte-identical
```

The `--cost-function` flag accepts `uniform` or `uerd`. UERD is the shipping default — the uniform option exists for comparison benchmarking, not for production embedding.

## What doesn't work yet

`v0.1.0-alpha` is a research checkpoint, not a finished tool. The following are explicitly NOT implemented:

- **No compression resilience.** If you upload a stego JPEG to Facebook, Instagram, Twitter, or any service that re-encodes uploaded images, the embedded data will be destroyed. Phase 2 of the plan adds channel-adaptive coefficient stabilization (MINICER + ROAST) for specific social-media compression profiles; until then, treat phantasm as a file-in-file tool, not a post-in-a-post tool.
- **No perceptual hash preservation.** At low embedding rates pHash/dHash are usually unchanged (the change cost is bimodal — ~75% of images are "robust" with 0% cost, per the Spike B analysis), but there's no guarantee at higher densities. Phase 3 adds a per-image sensitivity classifier and wet-paper cost constraints for hash-critical coefficients.
- **No SRNet / EfficientNet / modern ML steganalysis evaluation.** Current detection evaluation uses classical detectors (Fridrich RS, SRM-lite L2, chi-square, sample pairs). A well-trained modern CNN will almost certainly outperform these and is expected to detect `v0.1.0-alpha` stego at higher rates.
- **Single-layer STC only** in the main embedding pipeline. Double-layer (ternary) STC exists but isn't wired.
- **STC H̃ submatrix is a deterministic PRNG construction,** not the published Filler 2011 / DDE Lab reference tables. Correctness is verified but rate-distortion performance is ~15% below asymptotic bound.
- **Envelope format is unstable.** `v0.1.0-alpha` uses envelope format v2. Expect format breaks before `v0.1.0` stable.
- **CLI is unstable.** Flag names and subcommand shape may change without deprecation.

## Threat model

Phantasm is intended for scenarios where:

1. You want to send a confidential payload over a channel that allows JPEG images but is untrusted (e.g., casual adversaries, automated scanners, content-inspection middleboxes).
2. The adversary has access to the stego image and can run classical and statistical steganalysis on it.
3. The adversary does NOT have access to the cover, nor to any out-of-band channel keyed to the sender/receiver.
4. The adversary does NOT re-encode or compress the image in transit. (If they do, see "no compression resilience" above.)

Phantasm is NOT suitable for:

- Defeating a well-resourced nation-state adversary running modern deep-learning steganalysis on your image
- Defeating a service that re-encodes uploaded JPEGs (most social media)
- Long-term archival where envelope-format stability matters
- Any situation where the confidentiality of the payload is life-critical

The cryptographic primitives (Argon2id, XChaCha20-Poly1305, HMAC-SHA256, HKDF-SHA256) are used via the established `argon2`, `chacha20poly1305`, `hmac`, `sha2`, and `hkdf` crates. The composition, key schedule, and envelope layout are project-specific and have NOT been externally reviewed. Do not treat `v0.1.0-alpha` as production-ready cryptography.

## Project layout

```
phantasm/
├── phantasm-image/   # JPEG DCT coefficient I/O via mozjpeg-sys
├── phantasm-crypto/  # Argon2id + XChaCha20-Poly1305 + HMAC + HKDF envelope
├── phantasm-stc/     # Syndrome-Trellis Codes (single- + double-layer)
├── phantasm-ecc/     # Reed-Solomon error correction wrapper
├── phantasm-cost/    # DistortionFunction trait + Uniform + UERD
├── phantasm-core/    # Orchestrators that compose image + crypto + STC + cost
├── phantasm-cli/     # `phantasm` binary
├── phantasm-bench/   # `phantasm-bench` binary (metrics + corpus evaluation)
├── spikes/           # Phase -1 de-risking experiments (DCT FFI, pHash cost)
└── research-corpus/  # 198 Picsum JPEGs (gitignored, manifest.json committed)
```

## More detail

- **[PLAN.md](PLAN.md)** — Architectural plan, phases, five-pillar thesis, and future work
- **[RESEARCH.md](RESEARCH.md)** — Literature review and academic background for the techniques phantasm uses
- **[STATUS.md](STATUS.md)** — Living session-by-session development log, research findings, and outstanding work. Read this if you want to know what's really built vs. planned.
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

# Corpus-scale evaluation (needs a corpus of JPEGs, e.g. research-corpus/)
cargo run --release -p phantasm-bench -- eval-corpus \
    --corpus path/to/jpeg/dir \
    --cost-functions uniform,uerd \
    --payload /path/to/payload.bin

# Single-file stealth analysis against classical detectors
cargo run --release -p phantasm-bench -- analyze-stealth \
    --cover cover.jpg \
    stego.jpg
```

## License

Dual-licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option. Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in phantasm by you shall be dual licensed as above, without any additional terms or conditions.

## Status

`v0.1.0-alpha` is the first public release. Expect breakage. Expect format changes. Don't use it for anything important yet. Read [STATUS.md](STATUS.md) for the full picture of what works and what's planned.
