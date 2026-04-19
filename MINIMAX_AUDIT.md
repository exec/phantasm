# MINIMAX Security Audit — Phantasm v0.2.0

**Reviewer model**: minimax-m2.7:cloud (independent external reviewer)
**Date**: 2026-04-13
**Scope**: Full codebase (9 crates)
**Target selected**: STC encoder/decoder + Double-Layer encoder + cryptographic envelope (highest asymmetry: HMAC authenticates envelope bytes but not stego encoding itself)

---

## Finding 1: STC Syndrome Computation Has an Off-by-One in the Column-Width Boundary Check

**Severity**: MEDIUM (correctness)
**File**: `phantasm-stc/src/encoder.rs`
**Lines**: 185–221 (`compute_syndrome`)
**Category**: Off-by-one in syndrome extraction boundary

### Description

The `compute_syndrome` function computes H·y (mod 2) for message extraction. The loop iterates over every bit position `i`, computes block index `b = i / w`, column index `j = i % w`, and for each set bit `k` in `hcol[j]`, contributes to syndrome row `r = b + k` if `r < message_len`.

```rust
for k in 0..h {
    if (h_col >> k) & 1 == 0 {
        continue;
    }
    let r = b + k;
    if r < message_len {
        syndrome[r] ^= 1;
    }
}
```

The check `r < message_len` correctly limits syndrome contributions to within the message length. However, the syndrome extraction conceptually should cover `message_len` rows indexed 0..message_len-1, and the boundary condition is correct here.

### Analysis

After thorough trace-through of the Filler 2011 syndrome-trellis structure with the DDE Lab H̃ tables, the `compute_syndrome` function is **correct**. The boundary check `r < message_len` is the right condition: syndrome bits indexed ≥ `message_len` are outside the valid syndrome range and should not be set.

**No bug found here.** This entry is documented to record that the initial suspicion of an off-by-one was investigated and resolved — the boundary check matches the paper's syndrome-trellis construction.

---

## Finding 2: `effective_height` Is a Stub That Doesn't Account for w < h

**Severity**: LOW (research-grade limitation, not exploitable in practice)
**File**: `phantasm-stc/src/parity.rs`
**Line**: 160–162
**Category**: Hardcoded stub

```rust
pub fn effective_height(h: u8, _w: usize) -> usize {
    h as usize
}
```

### Description

The function is a stub. The underscore prefix on `w` signals intentional non-use. The comment says:

> "Phantasm always operates the Viterbi trellis with the full configured constraint height. When w < h the H̃ matrix is rank-deficient (rank at most w) but the trellis is still valid."

In the paper's analysis, when `w < h`, the H̃ matrix is rank-deficient (max rank `w`), meaning only `w` syndrome rows are linearly independent. The paper shows the Viterbi is still valid because the DP explores the full 2^h state space — the rank deficiency just means some states are unreachable.

The stub `h` as `effective_height` is therefore **not a bug** — the code is correctly using full `h` states even when `w < h`. The stub exists as a future hook in case a more sophisticated adaptive height is needed.

### Analysis

No bug. The code intentionally uses full constraint height. The stub is documented and appropriate for the research-grade status.

---

## Finding 3: HMAC Pre-Check Before Decryption Is Correct Defense-in-Depth

**Severity**: INFO (design observation, not a vulnerability)
**File**: `phantasm-crypto/src/envelope.rs` / `phantasm-crypto/src/mac.rs`
**Lines**: Envelope `open()` lines 106–136; `verify_mac` lines 49–72
**Category**: Encrypt-then-MAC construction

### Description

The `open()` function calls `verify_mac()` before `decrypt()`. The HMAC covers `version || salt || nonce || ciphertext` and uses a **constant-time byte-by-byte XOR comparison** (no early-exit on first mismatch):

```rust
let mut diff: u8 = 0;
for (a, b) in full[..MAC_LEN].iter().zip(tag.iter()) {
    diff |= a ^ b;
}
if diff == 0 { Ok(()) } else { Err(CryptoError::AuthFailed) }
```

This is the correct pattern. The MAC is checked before any parsing or decryption, so wrong passphrase produces a clean `AuthFailed` with no panic or side channel leakage.

**The MAC key and AEAD key are derived from the same master via HKDF** (`split_keys`). This means both keys are potentially derived from the same source if Argon2id output is ever compromised. This is a minor theoretical concern (key separation principle) but not a practical vulnerability since:
1. Argon2id output is 32 bytes from a password hashing function — recovering the master from the output is as hard as breaking Argon2id itself
2. HKDF is a PRF; compromising one derived key doesn't automatically yield others

### Analysis

This is correct and sound. The Encrypt-then-MAC construction is the right order. The HMAC-first pre-check is defense-in-depth. The key separation is adequate for the threat model.

---

## Finding 4: Locations Key Derivation Chains HKDF Through Argon2id Output

**Severity**: LOW (not a vulnerability in practice; unusual but sound)
**File**: `phantasm-crypto/src/kdf.rs`
**Lines**: 42–58
**Category**: KDF construction

### Description

The locations key derivation uses an unusual chain:

```
image_salt = SHA256(quantized_DCT_coefficients)
ikm = Argon2id(passphrase, HKDF-SHA256(image_salt))
locations_key = HKDF-SHA256(ikm)
```

The `ikm` is the raw Argon2id output used directly as HKDF input material, then immediately zeroized. The HKDF is used in "expand only" mode (no "extract" phase) which is non-standard but not broken — HKDF-expand can be used directly with sufficient input entropy.

### Analysis

The construction is **sound** for the use case. The image_salt binds the locations key to the specific cover image. The Argon2id work factor (65536 KiB, 3 iterations, 4-way parallelism) is strong. The HKDF expand-only mode with 32-byte IKM provides adequate key material.

The unusual part (HKDF-expand used instead of HKDF-extract) doesn't introduce weakness because the IKM already has full 32-byte entropy from Argon2id. HKDF-extract would add nothing in this scenario.

No attack is feasible without breaking Argon2id preimage resistance or SHA-256.

---

## Finding 5: HMAC Key and AEAD Key Are Same-Source — Minor Theoretical Key Separation Concern

**Severity**: LOW (theoretical; not exploitable with current primitives)
**File**: `phantasm-crypto/src/kdf.rs` / `phantasm-crypto/src/mac.rs`
**Category**: Key separation

### Description

`split_keys` derives both `aead_key` and `mac_key` from the same 32-byte Argon2id output via HKDF-SHA256 with distinct `info` parameters:

```
master = Argon2id(passphrase, salt)
aead_key = HKDF-SHA256(master, info="phantasm-v2-aead")
mac_key = HKDF-SHA256(master, info="phantasm-v2-mac")
```

HKDF is a PRF under the ROM model; deriving two keys from the same IKM with distinct info strings is standard and safe. However, if the AEAD key were ever leaked through a side channel in ChaCha20-Poly1305 (hypothetical), the HMAC key would still be safe since HKDF isn't invertible.

The concern is **theoretical only**: in a specialized security proof, independent extract phases would provide cleaner key separation, but for practical steganography at research-grade maturity, this is not a vulnerability.

---

## Finding 6: Double-Layer Encoder Embeds m2 at Head Positions — Constraint Compatibility With m1 Embedding Is Implicit

**Severity**: LOW (correctness concern; documented as known limitation)
**File**: `phantasm-stc/src/double_layer.rs`
**Lines**: 148–246
**Category**: STC layer coupling

### Description

The double-layer encoder embeds `m2` (layer 2, plane 1) at **head positions** (block indices 0..m2_bits-1) and `m1` (layer 1, plane 0) at **tail positions** (block indices m1_bits..total_bits-1). The two STC passes are coupled through:

1. **Layer 2 modifies `target_p1`** at head positions — these bits become part of the cover for layer 1
2. **Layer 1's syndrome is affected by `target_p1` bits** at head positions (the head plane1 bits contribute to layer 1 syndrome via the `b+k` syndrome construction)
3. **Layer 1 encodes `m1` into tail plane0 bits** using conditional costs that depend on which plane0 value is "natural" within each committed plane1 cell

The encoding is **feasible only when the cover's natural values satisfy certain parity constraints** at head positions. For flat or adversarial covers, these constraints may not hold, potentially making the encoding infeasible or causing decoded bits to differ from embedded bits.

### Concrete Example

With hcol[0]=3 (bits 0 and 1 set for column 0), for block b (0..m1_bits-1):
- Layer 2 contributes `target_p1[b]` to syndrome bit `b` (via k=0)
- Layer 1's syndrome bit `b` is: `cover_plane0[b] ⊕ target_p0[b] ⊕ target_p1[b]` (via head position contributions from k=0 bits)

The encoder must satisfy: `cover_plane0[b] ⊕ target_p0[b] ⊕ target_p1[b] = m1[b]` for all b.

If `cover_plane0[b]` has the wrong value and both flip costs are infinite (wet), the encoding fails. This is the **expected behavior** (wet paper routing) but creates a coupling between layer feasibility that could cause unexpected `InfeasibleWetPaper` errors.

### Analysis

This is documented behavior and not a bug — the double-layer encoder is known to be suboptimal (the paper itself notes the λ-tuned joint optimization is deferred). The conditional-probability layering is a reasonable heuristic. The coupling is inherent to the two-pass approach.

The `InfeasibleWetPaper` error at encoder is the correct failure mode when the cover can't support the encoding.

---

## Finding 7: No Payload Authentication Beyond Envelope HMAC

**Severity**: LOW (design trade-off; not a vulnerability)
**Category**: Payload integrity

### Description

The HMAC-SHA256 covers `version || salt || nonce || ciphertext`. The **plaintext payload is not independently authenticated** — only the ciphertext is. After AEAD decryption and unframing, the only integrity check on the raw payload bytes is the AEAD tag (which used the AEAD key) and the outer HMAC (which used the MAC key).

If the AEAD key were compromised and an attacker could craft valid ciphertext that passes AEAD verification but contains attacker-controlled payload bytes, the outer HMAC would still reject it (since the HMAC key is separate from the AEAD key). So this is not exploitable in practice.

The design correctly separates AEAD key from HMAC key via HKDF.

---

## Finding 8: DCT-I Implementation in hash_guard.rs May Not Be Orthonormal

**Severity**: LOW (hash stability; not a security issue)
**File**: `phantasm-core/src/hash_guard.rs`
**Lines**: 387–401
**Category**: Numerical precision / hash stability

### Description

The `dct1d_32` function implements DCT-I (not DCT-II):

```rust
s += xi * (std::f64::consts::PI * k as f64 * (2 * i + 1) as f64 / (2 * N) as f64).cos();
```

The DCT-I basis functions have scaling factors of `1/√N` for k=0 and `1/2` for k>0, not the `√(1/N)` and `√(2/N)` of DCT-II. This is a **design choice**, not a bug — the hash guard only cares about relative magnitudes for classification, not absolute coefficient values. Since both the encoder and decoder use the same DCT-I implementation for salt derivation, any systematic scaling error cancels out.

However, the scaling discrepancy vs. the standard DCT-II used in image processing means:
1. Coefficient magnitudes are not directly comparable to published pHash literature values
2. The influence basis functions computed here are for DCT-I, not the DCT-II assumed in the spec

For hash guard purposes (classification into Robust/Marginal/Sensitive tiers), this is fine since the thresholds are calibrated empirically against the chosen DCT implementation.

---

## Finding 9: Image Salt Stability Against Adversarial Covers Is Documented But Worth Noting

**Severity**: INFO (documented limitation)
**File**: `phantasm-core/src/pipeline.rs`
**Lines**: 302–312 (comments) and 272–313 (constant docs)
**Category**: Salt stability

### Description

The `SALT_QUANT_STEP = 16` coarsens DCT coefficient quantization for salt derivation. The documentation correctly notes:

> "Adversarial cover limitation: if a cover has a low-frequency DCT coefficient whose pre-quantization value happens to lie within ~0.5 units of a `step × n` boundary AND the chosen cost function's embed perturbation pushes that coefficient across the boundary, the salt will drift and extract will fail with `AuthFailed`."

This is a **known denial-of-service vector for adversarial covers** but not a security vulnerability — it causes extraction to fail cleanly (`AuthFailed`) rather than to misdecode silently.

---

## Finding 10: PRNG Fallback in `htilde_for_rate` Has Not Been Tested Against DDE Lab Reference

**Severity**: INFO (research-grade)
**File**: `phantasm-stc/src/encoder.rs`
**Lines**: 62–120
**Category**: untested code path

### Description

The PRNG fallback path in `htilde_for_rate` (for h outside [7..12] or w outside [2..20]) uses a SplitMix64 construction with forced bits (bit 0 and bit h-1 always set) and rank repair. The DDE Lab published tables cover h=7..12 and w=2..20, which is the standard operating range. The PRNG fallback is untested against any reference implementation.

The construction is plausible (SplitMix64 is a solid bijection; rank repair ensures the matrix isn't rank-deficient). However, **no production configuration actually triggers this path** — all phantasm presets use h in [7..12] and w in [2..20]. The PRNG path is only reached for non-standard research configurations.

---

## Finding 11: `phantasm-image/src/png.rs` Decoder Exists But Is Not Wired Into Embed Pipeline

**Severity**: INFO (dead code / incomplete feature)
**File**: `phantasm-image/src/png.rs`
**Category**: Technical debt

### Description

The PNG decoder exists (`png.rs` with `read_png` function) but is not integrated into the embed pipeline. The `png` feature flag exists but `jpeg.rs` is the only active decoder path. This is noted in the codebase as incomplete.

This is not a security issue — unused code doesn't expand the attack surface in the active pipeline.

---

## Summary

| # | Finding | Severity | Category |
|---|---------|----------|----------|
| 1 | STC syndrome boundary check | MEDIUM (false alarm — correct) | correctness |
| 2 | `effective_height` stub | LOW (documented) | research-grade |
| 3 | HMAC pre-check before decrypt | INFO (correct design) | crypto |
| 4 | Locations key HKDF-Argon2id chain | LOW (sound) | crypto |
| 5 | Same-source HMAC/AEAD keys | LOW (theoretical) | key separation |
| 6 | Double-layer head/tail coupling | LOW (documented) | STC |
| 7 | No independent payload auth | LOW (design trade-off) | crypto |
| 8 | DCT-I vs DCT-II discrepancy | LOW (hash stability) | numerical |
| 9 | Salt stability for adversarial covers | INFO (documented) | robustness |
| 10 | PRNG fallback untested | INFO (unreached code) | testing |
| 11 | PNG decoder unused | INFO (dead code) | tech debt |

### What I'd prioritize for external review attention:

1. **Double-layer encoder coupling (Finding 6)** — the implicit coupling between layer 1 and layer 2 through head position plane1 bits is the most subtle correctness concern. If the research team hasn't already, I'd recommend a property-based test that embeds with layer-2 costs and verifies the extracted m2 bits exactly equal the embedded m2 bits across a diverse cover corpus.

2. **HMAC/AEAD key separation (Finding 5)** — minor theoretical concern. Switching to HKDF-extract for both keys (rather than HKDF-expand for the master output) would provide cleaner separation with no performance cost.

3. **DCT implementation choice (Finding 8)** — for hash guard stability comparisons against academic literature, clarify whether DCT-I is intentionally used instead of DCT-II and calibrate thresholds accordingly.

### No critical or high-severity findings.

The codebase is well-engineered. The cryptographic primitives are correctly composed (Encrypt-then-MAC, Argon2id + HKDF, constant-time MAC comparison). The STC implementation correctly traces the Filler 2011 syndrome-trellis structure. The envelope format is sound. The main limitations are inherent to the research-grade status and are appropriately documented.
