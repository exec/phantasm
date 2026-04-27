# Audits

External and internal audit reports of phantasm.

## Inventory

- **`MINIMAX_AUDIT.md`** — Internal audit by an independent reviewer (codename
  Minimax) covering the v0.2 codebase. 11 findings, 0 critical, 0 high. The
  most subtle correctness concern (Finding 6, double-layer encoder
  head/tail coupling) is closed by the property-based test added in v1.0.0
  (`phantasm-stc/src/tests.rs`). Finding 8 (DCT-II orthonormality in
  `hash_guard.rs`) is closed by the orthonormality test added in v1.0.0
  (`phantasm-core/src/hash_guard.rs`). Finding 10 (PRNG fallback in
  `htilde_for_rate` untested) is closed by the structural-properties +
  determinism tests added in v1.0.0.
- **`QWEN_AUDIT.md`** — Initial audit by an independent reviewer (codename
  Qwen) covering the v0.1.0 codebase. Closed two MEDIUM findings on CLI
  passphrase exposure (`--passphrase` argv visibility, `warn!()` log
  persistence) — the `--passphrase-env` and `--passphrase-fd` flags shipped
  in v0.3.0 to address them.
- **`QWEN_AUDIT_RE_MINIMAX.md`** — Qwen's verification of the Minimax audit's
  findings, with their independent assessment.

## What's not in here

**No external commercial security review has been commissioned.** The
cryptographic primitives are used via established crates
(argon2, chacha20poly1305, sha2, hkdf, hmac); the composition (Encrypt-then-
MAC, HKDF key separation, AEAD with associated-data binding the version /
salt / nonce) is reviewed in the audits above but has not been examined
by a paid third-party firm. Treat phantasm v1 accordingly: the security
argument is "audit-grade, not assurance-grade." If your threat model
requires production-grade assurance, commission a dedicated review before
deploying.
