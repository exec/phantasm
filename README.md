<p align="center">
  <img src="phantasm.png" alt="phantasm" />
</p>

**Content-adaptive JPEG steganography in Rust.** v1.0.0.

Phantasm hides authenticated, encrypted payloads in the DCT coefficients of
JPEG images. Embedding is driven by the J-UNIWARD content-adaptive cost
function (Holub & Fridrich 2014) routed through published-table syndrome-
trellis coding (Filler 2011, DDE Lab H̃ tables), sealed in an authenticated
cryptographic envelope (Argon2id + XChaCha20-Poly1305 + HMAC-SHA256), and
optionally constrained to preserve a perceptual hash of the cover.

```bash
phantasm embed --input cover.jpg --payload secret.txt --passphrase-env PASS --output stego.jpg
phantasm extract --input stego.jpg --passphrase-env PASS --output recovered.txt
```

## What phantasm is (and is not)

**Phantasm defends the confidentiality of a payload that an adversary can
see exists.** The cryptographic envelope (L2 + L3 below) is the load-bearing
layer: a passphrase-keyed AEAD that returns `AuthFailed` cleanly on a wrong
passphrase or any tampering, and prevents recovery of the payload by
anyone without the passphrase. This is what phantasm v1 stakes its
security claim on.

**Phantasm is NOT a plausible-deniability tool against a phantasm-aware
adversary.** The L1 layer (statistical undetectability of the stego JPEG
itself) degrades gracefully against off-the-shelf modern CNN detectors
but fails against a CNN trained specifically on phantasm output. If your
threat model requires *"the adversary should not be able to tell this image
contains hidden data at all,"* phantasm is the wrong tool against a
well-resourced ML adversary. Read the [three-layer threat model](#three-layer-threat-model)
below for the exact boundaries.

## Headline numbers

Tested on a 198-cover Picsum research corpus at QF=85, ~720px short-side,
~0.2 bpnzac payload (`phantasm-stc` published-table H̃, h=10 trellis,
`--cost-function j-uniward`):

| Detector | Cover false-positive | phantasm J-UNIWARD detection |
|---|---:|---:|
| **JIN-SRNet** (Butora/Yousfi/Fridrich 2021, ImageNet+J-UNIWARD pretrained) | 7.1% | **16.2%** |
| EffNet-B0 (Aletheia, ALASKA2 pretrained) | 88.9% (cover-source mismatch) | paired-evade |

JIN-SRNet at the typical phantasm payload sits ~9 percentage points above
the cover false-positive floor — at the diagnostic level, **49.5% of
phantasm J-UNIWARD stegos score *lower* P(stego) than their own cover.**

Caveats applicable to every number above:

1. Numbers are **specific to** the Picsum research corpus, ~720px JPEGs, QF=85,
   and a payload of ~0.2 bpnzac. Other cover-source distributions, payload
   ratios, and detector training sets will produce different numbers.
2. JIN-SRNet was trained at 0.4-0.6 bpnzac; phantasm at typical payload
   sits below the training distribution. Detection rises as payload
   approaches training distribution.
3. **These are off-the-shelf-detector numbers.** Against a phantasm-aware
   adversary that has fine-tuned a CNN on phantasm output (5 passphrases per
   cover, ~2k pairs, EfficientNet-B0 on a 500-cover corpus), measured
   detection is **96.8% on the held-out split**. The L1 layer does not
   defend against phantasm-aware ML.

See `archive/ML_STEGANALYSIS.md` for the full evaluation history.

## Three-layer threat model

Phantasm's security has three independent layers. Read these in order — v1's
posture rests on the strength of L2 and L3, with L1 explicitly scoped down.

**L1 — Detection.** *Can an adversary tell the JPEG contains hidden data?*
Off-the-shelf modern CNNs do not reliably detect phantasm J-UNIWARD output
at typical payload (16.2% on JIN-SRNet at the headline corpus, vs 7.1%
cover false-positive floor). A phantasm-aware CNN — trained on actual
phantasm stego output — does (96.8%+). **L1 is the weak layer and degrades
gracefully against capable adversaries.**

**L2 — Position recovery.** *Given an adversary suspects steganography, can
they recover where the bits are without the passphrase?* The position
permutation is keyed by ChaCha12 with a HKDF-Argon2id-derived salt; without
the passphrase, an adversary attempting to brute-force position recovery
faces a 2^256 search space gated by Argon2id (default: 256 MB memory, 4
iterations, parallelism 1). **L2 is intact.**

**L3 — AEAD decryption.** *Given an adversary recovered the position
permutation, can they decrypt the payload without the passphrase?* The
envelope uses XChaCha20-Poly1305 with a 24-byte nonce and a HMAC-SHA256-16
permutation MAC over the version, salt, nonce, and ciphertext. The HMAC
is verified before any payload parsing, so a wrong passphrase always
returns `AuthFailed` cleanly — no oracle, no length-confusion. AEAD and
HMAC keys are derived from the master key via two **independent**
HKDF-extract calls (separate salts), so cross-key attacks are impossible
by construction. **L3 is intact.**

## Five technical pillars

Phantasm v1 integrates five capabilities. Most steganography tools ship
one or two; phantasm composes all five.

1. **Content-adaptive cost function.** `--cost-function j-uniward`. J-UNIWARD
   (Holub & Fridrich 2014) computes per-coefficient embedding costs from
   wavelet-domain relative distortion of the spatial-domain image,
   producing a cost map that prefers modifications in textured regions
   over smooth regions.
2. **Syndrome-trellis coding.** Published DDE Lab H̃ tables (Filler 2011)
   for h ∈ [7, 12], w ∈ [2, 20]. Conditional-probability double-layer
   decomposition at 0.995× bits/L1.
3. **Modern AEAD envelope.** Argon2id (default 256 MB, t=4, p=1) +
   XChaCha20-Poly1305 + HMAC-SHA256-16 MAC + HKDF key split with
   independent extract per output key. Envelope FORMAT_VERSION 3.
4. **Channel-adaptive preprocessing.** `--channel-adapter twitter`. MINICER
   (per-coefficient minimum-iterative-error stabilization) + ROAST
   (block-level overflow alleviation) + Reed-Solomon ECC for share-and-
   recompress survival. **Status: experimental.** End-to-end recovery
   under `image`-crate QF=85 round-trip is not yet reliable at default RS
   parameters; treat as a research preview, not a production deliverability
   feature.
5. **Perceptual-hash preservation.** `--hash-guard {phash,dhash}`. Marks
   coefficients whose modification would flip the selected perceptual-hash
   bits as wet-paper, preserving the cover's pHash or dHash through embed.

## Quickstart

```bash
git clone https://github.com/exec/phantasm
cd phantasm
cargo build --release

# Embed (using --passphrase-env to keep the passphrase out of argv)
PASS="correct horse battery staple" \
  ./target/release/phantasm embed \
    --input cover.jpg \
    --payload secret.txt \
    --passphrase-env PASS \
    --output stego.jpg

# Extract
PASS="correct horse battery staple" \
  ./target/release/phantasm extract \
    --input stego.jpg \
    --passphrase-env PASS \
    --output recovered.txt
```

Use `--passphrase-env VAR` or `--passphrase-fd N` in production.
The `--passphrase` flag exists for ergonomic testing but exposes the
passphrase via `/proc/<pid>/cmdline`.

## What v1 cuts vs v0.x

v1 deliberately reduces surface area:

- **JPEG only.** PNG / S-UNIWARD spatial-domain MVP from v0.3 is removed.
  PNG steganalysis is a different threat model (different cover sources,
  different sharing channels, different detectors); v1 doesn't try to
  serve both.
- **J-UNIWARD only.** UERD (the v0.1 default) is removed. J-UNIWARD
  measurably wins against modern CNN detectors; UERD's only winning case
  is classical Fridrich RS, which isn't a v1-relevant adversary in 2026.
- **No `--cost-noise`, no `--cost-subset`.** The v0.2-era position-
  randomization research closed both directions negatively against
  phantasm-aware CNN attackers; the flags shipped as research-only and
  are now removed.
- **No `phantasm bench` subcommand.** The standalone `phantasm-bench`
  research-evaluation crate (used to produce the headline numbers above)
  is preserved on a separate `research/phantasm-bench-archive` branch.
- **No "research-grade" self-ID string.** v1 owns its scope.

## License

Dual-licensed under MIT or Apache-2.0. See `LICENSE-MIT` and `LICENSE-APACHE`.

## Status, audits, and history

- `STATUS.md` — current build/test/security state.
- `CHANGELOG.md` — version history.
- `audits/` — internal/external audit reports.
- `archive/` — pre-v1 design documents and the v0.2-v0.4 ML-steganalysis
  research log.
