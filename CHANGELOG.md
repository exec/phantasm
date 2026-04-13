# Changelog

All notable changes to phantasm will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/dylan/phantasm/compare/v0.1.0-alpha...HEAD
[0.1.0-alpha]: https://github.com/dylan/phantasm/releases/tag/v0.1.0-alpha
