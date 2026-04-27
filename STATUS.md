# Phantasm — Project Status

**Date:** 2026-04-25 (v1.0.0 shipped)
**Workspace state:** `cargo test --workspace` **204 passing / 0 failing**, `cargo clippy` clean, `cargo fmt` clean
**Git state:** `v0.1.0-alpha` at `82b89a4`, `v0.1.0` at `1617dad`, `v0.2.0` at `8432cf7`, `v0.3.0` at `e9b3d03`, `v0.4.0` at `09aa010`, **`v1.0.0` at the release commit of this update**. Pushed to `https://github.com/exec/phantasm`.

**v1.0.0 shipping headline:** Focused, JPEG-only, J-UNIWARD-only steganography with a clearly-scoped threat model. **Confidentiality of a payload an adversary can see exists** is the security claim, backed by the L2/L3 cryptographic envelope (Argon2id + XChaCha20-Poly1305 + HMAC-SHA256, independent-extract HKDF key separation, FORMAT_VERSION = 3). Plausible-deniability framing is explicitly dropped — L1 detection rates are documented honestly with all caveats (16.2% on JIN-SRNet at typical payload on the headline corpus; 96.8% against a phantasm-aware CNN at d500 scale). Code surface dropped ~9k LOC vs v0.4.0: UERD, S-UNIWARD/PNG, the cost-noise + position-subset research flags, and the phantasm-bench research crate are removed (phantasm-bench preserved on the `research/phantasm-bench-archive` branch). Envelope bumped 2 → 3 to absorb the SALT_QUANT_STEP fix (16 → 256, closes a 42.5% pHash-block drift bug discovered in v0.4 diagnostic tooling) and the strengthened HKDF key separation (independent-extract per output key, closes MINIMAX_AUDIT Finding 5). v0.x envelopes are NOT readable by v1; this is intentional. Audit follow-throughs: Findings 6 (double-layer head/tail coupling property test), 8 (DCT-II orthonormality lock-in), and 10 (PRNG fallback structural-properties + determinism) all close.

**v1.0.0 positioning sentence:** Phantasm v1 is an authenticated-encryption-into-a-JPEG tool. It does NOT defend against an adversary who has trained a CNN on phantasm output. Use it where you need confidentiality of a known-stego payload, not where you need plausible deniability.

**Five-pillar thesis (v1, JPEG-only):**
  1. **Content-adaptive cost function** — `--cost-function j-uniward`
  2. **Syndrome-trellis coding** — published DDE Lab H̃ tables (Filler 2011) + conditional-probability double-layer at 0.995× bits/L1
  3. **Modern AEAD envelope** — Argon2id + XChaCha20-Poly1305 + HMAC-SHA256-16 + independent-extract HKDF key separation + FORMAT_VERSION = 3
  4. **Channel-adaptive preprocessing** — `--channel-adapter twitter` (MINICER + ROAST + Reed-Solomon ECC). **Experimental** — does not yet survive `image`-crate QF=85 round-trip at default RS parameters.
  5. **Perceptual-hash preservation** — `--hash-guard {phash, dhash}` (3-tier sensitivity classifier + wet-paper constraint)

---

## What v1 ships (current state)

### Cryptographic envelope (FORMAT_VERSION = 3)

- **Key derivation**: `argon2id(passphrase, salt, m=256MB, t=4, p=1)` produces a 32-byte master key. The salt is a 32-byte CSPRNG output stored in the envelope.
- **Key separation**: master key feeds two **independent** HKDF-extract calls (different salts: `phantasm-v3-aead-salt`, `phantasm-v3-mac-salt`), each followed by HKDF-expand to 32-byte output keys (`phantasm-v3-aead`, `phantasm-v3-mac`). Independent PRKs make cross-key attacks impossible by construction.
- **AEAD**: XChaCha20-Poly1305 with 24-byte nonce.
- **Permutation MAC**: HMAC-SHA256 truncated to 16 bytes, computed over `version || salt || nonce || ciphertext`. Verified before any payload parsing — wrong passphrase always returns `AuthFailed` cleanly.
- **Envelope wire format**: `[version: u8][salt: 32][nonce: 24][mac: 16][ciphertext: ..]`. Version mismatch returns `UnsupportedVersion` cleanly.

### Embedding

- **Cost function**: J-UNIWARD (Holub & Fridrich 2014). Wavelet-domain relative-distortion cost computed from spatial-domain decoding of the cover.
- **STC**: Published DDE Lab H̃ tables for `h ∈ [7, 12]`, `w ∈ [2, 20]`. PRNG fallback for `(h, w)` outside the table; structural-properties test added in v1.
- **Position permutation**: ChaCha12 keyed from a HKDF-derived locations key on the master key.
- **Salt derivation**: pHash-stable from the cover JPEG's 32×32 area-resampled luma DCT, quantized at step 256 (v1 setting; v0.x used step 16 which silently drifted on 42.5% of covers through QF=85 recompression).

### CLI

```
phantasm embed     — Hide an authenticated payload in a JPEG cover
phantasm extract   — Recover a payload from a stego JPEG
phantasm analyze   — Report capacity and characteristics of a JPEG
phantasm channels  — List available channel-adapter profiles
```

Hidden subcommands (research surface, not in `--help`):
- `phantasm dump-costs` — write a per-coefficient cost-map sidecar for research consumers

### Workspace

- 8 member crates: `phantasm-{cli, core, cost, channel, crypto, ecc, image, stc}`
- 204 unit + integration tests passing
- `cargo clippy --workspace --all-targets -- -D warnings` clean
- `cargo fmt --check` clean

---

## Audit state

See `audits/` for full reports. v1 closes the audit follow-throughs that were left open at v0.4.0:

| # | Finding | Severity | v1 status |
|---|---------|----------|-----------|
| MINIMAX-1 | STC syndrome boundary check | MEDIUM (false alarm) | confirmed correct, closed |
| MINIMAX-5 | Same-source HMAC/AEAD keys | LOW (theoretical) | **closed** — v3 uses independent-extract HKDF |
| MINIMAX-6 | Double-layer encoder coupling | LOW (most subtle) | **closed** — property-based test added |
| MINIMAX-8 | DCT-I vs DCT-II in hash_guard | LOW (numerical) | **closed** — verified DCT-II orthonormal, locked in by test |
| MINIMAX-10 | PRNG fallback untested | INFO | **closed** — structural-properties + determinism tests added |
| MINIMAX-11 | PNG decoder unused | INFO (dead code) | **closed** — PNG removed entirely from v1 |
| QWEN-1, 2 | CLI passphrase exposure | MEDIUM | closed in v0.3 (`--passphrase-env`, `--passphrase-fd`) |

**No external commercial security review has been commissioned.** The cryptographic primitives are used via established crates (`argon2`, `chacha20poly1305`, `sha2`, `hkdf`, `hmac`), and the composition is reviewed in the audits above. v1's security argument is **audit-grade, not assurance-grade.** If your threat model requires production-grade assurance, commission a dedicated review.

---

## Known limitations and deferred work

- **Channel adapter is experimental.** The Twitter MINICER + ROAST + Reed-Solomon ECC pipeline is plumbed but does not survive `image`-crate QF=85 round-trip at default RS parameters (0/40 exact-match extracts, measured in v0.3). Stronger RS overhead (200%+) or bit-level FEC would help; deferred to v1.x.
- **Multi-cover payload spreading**: not implemented. Spreading the syndrome across N covers so any single cover can tolerate higher BER would harden the lossy-channel story; deferred.
- **L1 against phantasm-aware CNN**: open and almost certainly unwinnable in the current cost-map family. The v0.4 research arc (HYDRA / CHAMELEON / DOPPELGÄNGER / PALIMPSEST, all on private experiment branches at v0.4.0) closed four cost-map-based defenses negatively. The remaining viable directions change the embedding *operator* (learned-generator GAN approaches, patch-synthesis approaches) rather than the cost map; these are research, not v1 deliverables.
- **External commercial security review**: not commissioned. The v1 README and this STATUS document are explicit about this.
- **Cargo-audit / cargo-outdated hygiene pass**: not run as part of the v1 cut. Recommended before any v1.x.

## License

Dual-licensed under MIT or Apache-2.0. See `LICENSE-MIT` and `LICENSE-APACHE`.
