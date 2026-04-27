# Phantasm — Modern ML Steganalysis Evaluation

**Status:** v0.3 release, 2026-04-19. Complements the classical Fridrich RS results in STATUS.md §5 Finding 8 with a modern-CNN L1 detectability evaluation. Updates 1-6 shipped with v0.2.0 on 2026-04-13; Updates 7 + 8 were added during the v0.2 → v0.3 burst and close the remaining L1 research questions (cover-source diversity scale-up, iterative adversarial costs) negatively.

## Scope (read this first)

Phantasm's security stack has three layers. **This document evaluates Layer 1 only.**

| layer | what it defends | what an attacker needs to bypass |
|---|---|---|
| **L1 — Detection** | plausible deniability: *"does this JPEG contain hidden data at all?"* | a CNN steganalyzer that generalizes to phantasm's modification pattern |
| **L2 — Position recovery** | message shape: *"which coefficients carry the syndrome bits, in what order?"* | the passphrase, to derive the ChaCha12 permutation of position indices (HKDF-keyed) |
| **L3 — AEAD decryption** | payload confidentiality: *"what are the actual bytes?"* | the passphrase, again — gated by Argon2id(64 MiB / 3 iter / 4 threads) + XChaCha20-Poly1305 + HMAC-SHA256 pre-filter |

L2 and L3 both gate on the passphrase but defend different things. L3 stops the attacker from decrypting what they have; L2 stops them from even *finding the syndrome bits in the right order* in the first place. Both rely on well-studied cryptographic primitives (HKDF-SHA256, ChaCha12 permutation, XChaCha20-Poly1305, Argon2id) that this research drive does not attempt to re-evaluate — they're the load-bearing security layers of phantasm, and they don't need ML steganalysis testing.

**L1 is the weak layer** and is the only thing this document tests. The six updates below (Updates 1-6) collectively establish how L1 degrades against increasingly-adapted CNN attackers. The short summary:

- **Against off-the-shelf detectors** (no phantasm-specific training): L1 mostly holds. Phantasm J-UNIWARD evades pretrained JIN-SRNet at 16.2% detection vs 7.1% cover false-positive baseline. Option D' (passphrase-derived position subset) can push this slightly lower (−6.2 pp) at the cost of a 23% embedding-failure rate.
- **Against phantasm-trained detectors** (5 passphrases × 198 covers, ~95 sec fine-tune on a single RTX 5070): L1 folds at 85–90% detection. Both UERD and J-UNIWARD are detected at similar rates once the attacker has phantasm-specific training data.

**What this means for phantasm users depends on the threat model:**

- **Confidentiality threat model** ("I don't care if the adversary knows this image contains hidden data; I care that they can't read it"): L1 detectability does not gate the security argument. L2+L3 do, and those are standard primitives. Phantasm defends confidentiality as long as the passphrase is strong and the implementation has been security-reviewed — which is explicitly pending, see `STATUS.md` deferred-backlog.
- **Plausible-deniability threat model** ("the mere fact that this JPEG contains hidden data is itself incriminating — border crossings, compliance checks, adversary with your image but not yet your passphrase"): L1 detectability matters, and the honest answer is that phantasm L1 degrades gracefully against off-the-shelf adversaries but does not defend against a phantasm-aware attacker. If your threat model is plausible deniability, treat the v0.2 evaluation as a characterization of exactly how weak that layer is.

The original v0.1.0 `README.md` threat-model section already scopes phantasm as a confidentiality tool, not a plausible-deniability tool. **In the confidentiality framing, Updates 1-6 collectively produce an honest L1 number and that's enough** — phantasm's security argument has never rested on L1 stealth alone.

---

## TL;DR

Tested phantasm at the v0.1.0 default payload (3 KB, ~0.2 bpnzac) on 198 Picsum covers against two pretrained CNN detectors. **Cost-function design still differentiates against modern CNNs**, and the ordering inverts from the classical-detector ordering: against modern CNNs, **J-UNIWARD beats UERD by a significant margin**, opposite to the classical Fridrich RS result where UERD beats J-UNIWARD.

**Update 1 (Option B, same day):** Fine-tuned JIN-SRNet on phantasm UERD output for 21 seconds and lifted UERD detection from 57.6% to **77.8%**. Same fine-tune slightly *decreased* J-UNIWARD detection (28.3% → 25.3%). I claimed this meant "J-UNIWARD's robustness is structural" — see Update 2 for why that interpretation was wrong.

**Update 2 (Option B' validation, same day):** Symmetric experiment — fine-tuned JIN-SRNet on phantasm J-UNIWARD output and measured J-UNIWARD detection lift. Result: J-UNIWARD-aware fine-tune lifts J-UNIWARD detection from 28.3% to 54.5% (+26.3 pp), more than UERD-aware lifts UERD (+20.2 pp). The asymmetry claimed in Update 1 was wrong. Both cost functions are vulnerable to attacker adaptation. J-UNIWARD remains 23 pp harder to detect than UERD even after each is fine-tuned specifically against it. **Update 3 below partially supersedes this finding under larger training sets.**

**Update 3 (Option B'' extended dataset, same day):** Re-ran both fine-tunes with 5 passphrases per cover (5× larger training set as passphrase-variation augmentation). UERD detection lifts to 85.4% and J-UNIWARD detection lifts to **89.9%** — J-UNIWARD is now slightly *more* detectable than UERD at the worst-case attacker-adaptation level. The 23 pp J-UNIWARD-vs-UERD gap from Update 2 is gone. **In the worst case, against an attacker who has trained specifically on phantasm output with sufficient data, neither cost function defends.** The "use J-UNIWARD" recommendation is now scope-limited: it holds against off-the-shelf and lightly-adapted detectors, not against a fully-adapted attacker. See § Update 3 below.

**Update 4 (Option C single-step adversarial costs, same day):** Built the full sidecar-cost infrastructure (Python computes per-coefficient costs, Rust embed loads them via a new `--cost-function from-sidecar` flag) and ran three single-step gradient-based cost variants against the J-UW-multi target detector: pure |gradient|, sign-aware ReLU(±gradient), and J-UNIWARD-base × (1 + α × adv_signal) hybrid at α=2 and α=10. **None of them moved the needle.** Pure adversarial costs were dramatically WORSE (98-99% detection vs 89.9% baseline — the modifications moved out of J-UNIWARD's natural distribution and the detector flagged them as anomalous). Hybrid costs were statistically indistinguishable from pure J-UNIWARD (~89-91% detection at all alpha values). **Single-step gradient costs are fundamentally insufficient against well-trained detectors** — the linear approximation breaks down after the first few coefficient flips, exactly as the ADV-EMB literature documents. The infrastructure works; the gradient strategy doesn't. Iterative refinement is the standard next step. See § Update 4 below for the full attempt log.

**Update 7 (Option B''' cover-source diversity, v0.3 burst):** Extended the Picsum corpus from 198 to 500 unique covers (seeds `phantasm-0001..0500`) and re-ran Update 3's multi-pass J-UNIWARD fine-tune recipe. Result: **J-UNIWARD detection rises to 96.8% on the d500 held-out split (97.5% on eval-198)** — +6.9 pp / +7.6 pp above Update 3's 89.9%. The 89.9% number was NOT a cover-pool artifact; more cover diversity produces a stronger phantasm-aware detector. Option B''' closed negatively. See § Update 7.

**Update 8 (Option C-iter iterative PGD adversarial costs, v0.3 burst):** Added PGD-style iteration to Update 4's infrastructure — re-compute the detector gradient at the partial stego each round, re-embed with biased costs, repeat. Result: **J-UW-multi detection climbed 91.9% → 100.0% across T=0..4 iterations.** Every config in a 6-way hyperparameter sweep (T=4/8, α=0.5/2/5/-2, warm=cover/stego, norm=max-abs/percentile) was worse than the J-UNIWARD baseline. Sign-flip control (α=-2) also produced 100% detection, ruling out a direction bug. **Per-coefficient cost-function adjustment is exhausted as an L1 defense direction against trained non-linear detectors.** Remaining L1 defense directions (v0.4+) are structurally different: end-to-end differentiable embedding, ADV-EMB / ADV-IMB, multi-cover spreading. See § Update 8.

| Detector | Cover (FP) | Uniform | UERD | J-UNIWARD |
|---|---:|---:|---:|---:|
| **Fridrich RS** (classical, v0.1.0) | — | 66.7% | 26.8% | 30.3% |
| **JIN-SRNet** (PyTorch, J-UNIWARD on ImageNet) | 7.1% | **98.0%** | **46.5%** | **16.2%** |
| **Aletheia EffNet-B0** (Keras, J-UNIWARD on ALASKA2) | 88.9% | 90.9% | 81.3% | 79.8% |

Notes:
- All eval at ~3 KB payload (~0.2 bpnzac) on 198-image Picsum corpus.
- EffNet-B0 has severe cover-source mismatch (88.9% cover false positive) — its absolute numbers are unreliable but **paired** comparisons against same-cover baseline are meaningful.
- JIN-SRNet ALSO has a slight cover-source mismatch (Picsum vs ImageNet) but its 7.1% FP baseline is reasonable.

## What we ran

**Detectors:**
1. **JIN-SRNet** — Butora/Yousfi/Fridrich 2021 ("How to Pretrain for Steganalysis"). PyTorch, SRNet architecture (3-channel color, 2-class), trained on ImageNet covers + J-UNIWARD stegos at 0.4–0.6 bpnzac. Released by DDE Lab Binghamton. ~16 MB checkpoint. Inference: PyTorch 2.10 + CUDA 12.8 on RTX 5070, ~33 img/s. Preprocessing: pixel values divided by 255.
2. **Aletheia EfficientNet-B0** — Daniel Lerch's open-source steganalysis toolkit. Keras .h5, EfficientNet-B0 architecture (3-channel color, 2-class softmax), trained on ALASKA2 + J-UNIWARD. 4M params. Converted to ONNX via tf2onnx (one-time, on fishbowl in throwaway TF venv). Inference: ONNX Runtime CPU on fishbowl, ~22 img/s (CUDA provider failed — needs cuDNN 9.x + CUDA 12.x; not worth fixing for our scale). Preprocessing: pixel values divided by 255.

**Corpus:** existing 198-image Picsum.photos research corpus (qf75/85/90 × 512/720/1024 dims, 22 unique seeds × 9 = 198 files).

**Embedding:** main `phantasm` CLI, `--cost-function` ∈ {uniform, uerd, j-uniward}, fixed passphrase, fixed 3000-byte payload, `--stealth high`. Same cover passed through all three cost functions to enable per-image paired analysis.

**Workflow:** embed locally on Mac (parallel xargs -P 8, ~2 min for 594 embeddings), rsync to fishbowl, run inference there, rsync results back, aggregate.

## Detailed results — fixed payload (198 covers, 3 KB, --stealth high)

### JIN-SRNet

| group | n | mean P(stego) | median | det@0.5 |
|---|---:|---:|---:|---:|
| cover | 198 | 0.2765 | 0.2580 | **14/198 = 7.1%** |
| uniform | 198 | 0.9426 | 0.9951 | **194/198 = 98.0%** |
| uerd | 198 | 0.4906 | 0.4800 | **92/198 = 46.5%** |
| juniward | 198 | 0.2913 | 0.2423 | **32/198 = 16.2%** |

**Paired (per-image stego − cover P(stego)):**

| comparison | n | mean Δ | wins (stego < cover) |
|---|---:|---:|---:|
| uniform − cover | 198 | **+0.6660** | 3/198 (1.5%) |
| uerd − cover | 198 | **+0.2141** | 17/198 (8.6%) |
| **juniward − cover** | 198 | **+0.0148** | **98/198 (49.5%)** |

**The single most striking number in the eval:** 49.5% of phantasm J-UNIWARD stegos score **lower** P(stego) than their own cover. JIN-SRNet thinks the J-UNIWARD stego is *more cover-like than the original cover*, in half the samples.

### Aletheia EfficientNet-B0

| group | n | mean P(stego) | median | det@0.5 |
|---|---:|---:|---:|---:|
| cover | 198 | 0.8018 | 0.9127 | 176/198 = 88.9% |
| uniform | 198 | 0.8292 | 0.8986 | 180/198 = 90.9% |
| uerd | 198 | 0.7295 | 0.8716 | 161/198 = 81.3% |
| juniward | 198 | 0.6886 | 0.8177 | 158/198 = 79.8% |

**Paired:**

| comparison | n | mean Δ | wins |
|---|---:|---:|---:|
| uniform − cover | 198 | +0.0273 | 102/198 (51.5%) |
| uerd − cover | 198 | **−0.0723** | 116/198 (58.6%) |
| **juniward − cover** | 198 | **−0.1133** | **146/198 (73.7%)** |

The 88.9% cover false-positive rate makes the absolute detection-rate column meaningless — the model is biased to call everything stego, because Picsum's compression characteristics differ from its ALASKA2 training distribution. **But the paired analysis is meaningful and agrees with JIN-SRNet:** J-UNIWARD wins (most negative Δ, most "wins"), UERD second, Uniform last.

Both detectors trained against J-UNIWARD, both tested against phantasm J-UNIWARD: both fail to fire on phantasm's J-UNIWARD output, in the paired sense.

## Detailed results — security-capacity curve (4 payload sizes, --stealth low)

Embedded the full corpus at 1000 / 3000 / 6000 / 10000 bytes. 6000 and 10000 only fit on the 66 1024-px covers (smaller covers' capacity is exceeded), so high-payload comparisons are restricted to that subset. Image-size confound — flag for any cross-payload comparison.

### JIN-SRNet detection rate (P > 0.5)

| payload (bytes) | cover | uniform | uerd | juniward |
|---:|---:|---:|---:|---:|
| 1000 | 7.1% | 97.5% | 43.4% | 15.2% |
| 3000 | 7.1% | 98.5% | 44.4% | 14.6% |
| 6000 (1024-px subset) | 4.5% | 98.5% | 48.5% | 18.2% |
| 10000 (1024-px subset) | 4.5% | 98.5% | 48.5% | 18.2% |

**Detection rates are essentially flat across the 1k–10k byte range** for all three cost functions. This is unexpected — naively, detection should rise with payload. We hypothesize this is because phantasm uses cover-derived passphrase salt, so the STC permutation lands modifications in the SAME positions regardless of payload size; only the bit content differs. The detector picks up on the spatial pattern of modifications (which is payload-invariant in this regime), not on the count.

**Implication:** within phantasm's typical payload range, **cost function is the dominant variable, payload is not**. To stress-test the cost function we need to push payload into a regime where the cost-budget regions overflow — likely 10⁵ bytes or more, well above v0.1.0 envelope sizes.

### EfficientNet-B0 detection rate (P > 0.5)

| payload | cover | uniform | uerd | juniward |
|---:|---:|---:|---:|---:|
| 1000 | 88.9% | 90.9% | 82.3% | 80.8% |
| 3000 | 88.9% | 91.4% | 82.3% | 79.3% |
| 6000 (1024-px subset) | 98.5% | 100.0% | 100.0% | 100.0% |
| 10000 (1024-px subset) | 98.5% | 100.0% | 100.0% | 100.0% |

The 1024-px subset has 98.5% cover FP. EffNet-B0's behavior on Picsum 1024-px JPEGs is essentially "everything is stego." This is the cover-source mismatch problem in extremis. **Don't trust EffNet-B0 absolute numbers on this corpus; only the paired delta is interpretable.**

## Cross-detector consistency

| Cost function | JIN-SRNet det@0.5 | EffNet paired Δ (vs cover) | Both agree? |
|---|---:|---:|---|
| uniform | 98.0% | +0.027 | ✓ detected |
| uerd | 46.5% | −0.072 | partial / paired-evade |
| j-uniward | 16.2% | −0.113 | ✓ paired-evade |

The two detectors agree on the ordering (uniform > uerd > juniward in detection signal) and on the qualitative finding (uniform is detected, j-uniward evades, uerd is in between). They use different architectures, different training corpora, different training procedures — agreement is meaningful.

## Findings, in order of importance

1. **Cost-function design matters against modern CNNs.** This was the existential question for v0.2 scoping. The answer is unambiguously yes. Both detectors show clear differentiation between Uniform LSB and J-UNIWARD/UERD. Cost-function research has runway.

2. **The optimal cost function is threat-model dependent.** Against classical Fridrich RS, UERD wins (66.7%/26.8% paired with 30.3% J-UNIWARD). Against modern CNNs, J-UNIWARD wins (16.2% / 46.5% paired). The right cost function depends on whether your adversary has trained a CNN. **Phantasm's v0.1.0 default of UERD is the wrong default for a modern threat model** — v0.2 should consider switching the default to J-UNIWARD or surfacing the threat-model choice more prominently.

3. **Phantasm J-UNIWARD essentially evades JIN-SRNet at typical payload.** 16.2% detection rate (v.s. 7.1% noise floor), and 49.5% paired-wins — the model literally thinks the stego is more cover-like than the cover in half the cases. This is a stronger result than anything in the v0.1.0 STATUS document and suggests phantasm's J-UNIWARD implementation is research-grade despite using the canonical Holub-Fridrich 2014 algorithm.

4. **Payload size barely affects detection in the typical range.** Detection rates are flat from 1 KB to 10 KB. Cost function is the dominant variable. To stress-test cost functions we'd need 100+ KB payloads, well above what the crypto envelope currently supports at default stealth.

5. **Cover-source mismatch is real and matters.** EffNet-B0 trained on ALASKA2 has 88.9% cover false positive on Picsum. JIN-SRNet trained on ImageNet has 7.1%. The detector that's closer to the deployment cover source generalizes better. For phantasm users, this means: **the absolute risk depends on your adversary's training data, not just the detector architecture.**

## Caveats

- **Single passphrase per cover**, deterministic STC permutation. We're not measuring across-seed variance. An attacker with multiple intercepts of the same cover could potentially see different signatures.
- **N=198, single corpus (Picsum).** ALASKA2 / BOSSbase eval would tighten cross-detector comparisons but we deliberately tested on the corpus phantasm was developed against.
- **Classical detectors NOT re-run** in this eval. Classical numbers are from v0.1.0 final bench (STATUS.md §5 Finding 8) for context.
- **Both pretrained detectors trained on J-UNIWARD specifically.** UERD and Uniform are evaluated by cross-cost transfer. A UERD-trained detector might give different (probably higher) UERD detection. **No public pretrained UERD detector exists** — the only path to a true UERD number is training one ourselves, which is the only place training is now justified.
- **JIN-SRNet trained at 0.4–0.6 bpnzac.** Our payload (~0.2 bpnzac) is below the training distribution. Detection should rise as payload approaches training distribution. The flat curve we observed is interesting — it suggests phantasm sits in a regime where the detector can't latch on regardless.
- **Aletheia ONNX path uses CPU inference only.** CUDA execution provider failed (cuDNN/CUDA version mismatch on Blackwell). Not blocking — 22 img/s on CPU is plenty for our scale.
- **No license clarity on JIN-SRNet weights.** Conventional research-use; verify the LICENSE inside the JIN_SRNet.zip before publishing eval results externally.

## Update 1 — UERD fine-tune (Option B complete) — superseded by Updates 2 + 3

> **Note (added with Updates 2 + 3):** The interpretation in this section — that "J-UNIWARD's robustness is structural" — was wrong on two counts. Update 2 showed J-UNIWARD detection lifts MORE than UERD detection under a symmetric fine-tune (+26 vs +20). Update 3 then showed that with 5× more training data, J-UNIWARD becomes slightly *more* detectable than UERD (89.9% vs 85.4%) — the asymmetry from Update 2 also disappears at scale. The factual numbers in this section are correct; the framing is not. Read Update 3 for the current best understanding of the cost-function/attacker-adaptation tradeoff.

Following the original v0.2 plan (Option B below), I fine-tuned JIN-SRNet to be UERD-aware and re-evaluated. The fine-tune was deliberately tiny: 288 training examples (16 unique seeds × 9 qf/dim × 2 classes), 10 epochs, Adam lr=1e-5, random horizontal-flip + random 256×256 crop augmentation, all parameters trainable, init from JIN-SRNet weights. Total wall-clock training time on the RTX 5070: **21 seconds**.

### Headline numbers — same 198 images, same 256×256 center-crop preprocessing

| group | baseline P(stego) mean | fine-tuned P(stego) mean | Δ | baseline det@0.5 | fine-tuned det@0.5 |
|---|---:|---:|---:|---:|---:|
| cover | 0.350 | 0.234 | −0.116 | 18.2% | 17.7% |
| uniform | 0.743 | 0.873 | +0.130 | 83.8% | 88.9% |
| **uerd** | **0.504** | **0.736** | **+0.232** | **57.6%** | **77.8%** |
| **juniward** | 0.378 | 0.325 | **−0.053** | 28.3% | **25.3%** |

(Baseline numbers differ from the §Detailed results 198-eval because this re-runs JIN-SRNet through 256×256 center-crop preprocessing for an apples-to-apples comparison with the fine-tuned model, which was trained on 256×256 random crops. The full-512×512 baseline numbers in §Detailed results above are the deployment-equivalent measurements.)

### What the fine-tune proves

1. **UERD's apparent robustness against modern detectors was partially a cost-function-mismatch artifact.** A 21-second fine-tune from JIN-SRNet — which was trained against J-UNIWARD specifically — lifts UERD detection from 57.6% to **77.8%**, a +20 percentage-point jump and a +0.232 mean-P(stego) shift. The pretrained model was missing UERD-specific features; once it learns them, UERD is meaningfully more detectable. **A real attacker with BOSSbase (10K covers) and proper training infrastructure would lift UERD detection further** — probably into the 90% range.

2. **J-UNIWARD's robustness is structural, not a mismatch artifact.** The same fine-tune slightly DECREASED J-UNIWARD detection: mean P(stego) 0.378 → 0.325, det@0.5 28.3% → 25.3%. The model sharpened its UERD-pattern features at the cost of becoming less sensitive to J-UNIWARD's wavelet-residual modification pattern. **J-UNIWARD evasion is the kind of evasion that does not close under cheap attacker adaptation** — at least not by this fine-tune strategy.

3. **The fine-tune transfers to Uniform too.** Uniform detection went from 83.8% to 88.9% (mean P(stego) +0.130). Expected — Uniform is "more stego" than UERD, and a UERD-aware detector also fires on cruder modification patterns.

4. **Cover false positive went DOWN, not up.** Mean cover P(stego) dropped 0.350 → 0.234 even though the loss function only saw cover-vs-UERD pairs. The fine-tune learned a tighter cover boundary as a side effect. (Caveat: 16/22 seeds were in the fine-tune training set, so cover-FP improvement on the full 198-eval has memorization leakage. The 27-image held-out test split shows the same 77.8% UERD detection but a much lower 3.7% cover FP, consistent with this leakage interpretation.)

5. **The "use J-UNIWARD for modern threat models" recommendation is reinforced**, not just maintained. J-UNIWARD is the only one of phantasm's three cost functions whose evasion is robust against attacker adaptation by this method.

### Caveats specific to Update 1

- **Training data is small.** 288 items, 22 unique seeds, single cover source (Picsum). The +20 pp UERD lift is a conservative estimate; a real attacker with a real corpus would do better. Validation against a larger or more diverse training set is the next thing to verify (Option B' below).
- **Single passphrase across the entire dataset** — fine-tuned model may have learned passphrase-specific STC permutation artifacts. Cross-passphrase eval would tighten this.
- **We did not test the reverse direction** (J-UNIWARD-aware fine-tune): if a J-UNIWARD-aware fine-tune ALSO lifts J-UNIWARD detection significantly, the asymmetry between the two cost functions disappears and the "structural vs mismatch" framing is wrong. **This is the highest-priority follow-up.**
- **Test split is 27 covers + 27 stegos** (3 unique seeds × 9 qf/dim) — sample noise in the absolute numbers, but the consistency between test-split (77.8% UERD det) and full-198 (77.8% UERD det) is reassuring.
- The fine-tuned checkpoint and full evaluation JSON are at `~/phantasm-eval/finetune-uerd/` on the dev GPU box.

### Implication for v0.2

The case for **Option C (adversarial costs)** is now stronger, because the case for hand-designed costs is weaker than v0.1.0 suggested. UERD looked robust against JIN-SRNet but folds quickly to a cheap UERD-aware fine-tune. J-UNIWARD looked similarly robust but stayed robust under the same attacker adaptation. The asymmetry suggests *some* cost functions compose better with attacker adaptation than others, which is exactly what an adversarial-cost framework is meant to optimize for.

But before committing to Option C's 2+ week scope, **Option B' should validate the asymmetry direction** — if the J-UNIWARD-aware fine-tune symmetrically lifts J-UNIWARD detection, the asymmetry is fake and the C work needs a different framing.

## Update 2 — Symmetric J-UNIWARD fine-tune (Option B' validation) — superseded by Update 3

> **Note (added with Update 3):** The "J-UNIWARD remains 23 pp harder to detect than UERD even after fine-tuning" claim from this section turned out to depend on small training-set size. With 5× more training data per cover (Update 3), the gap collapses from +23 pp to −4.5 pp (J-UNIWARD becomes slightly more detectable than UERD at ~85-90% rates). The "structural vs mismatch" reframing in this section was already a walk-back of Update 1; Update 3 is a further walk-back of Update 2. The eval section below is preserved for historical context — read Update 3 for the current best estimate.

Update 1's interpretation hinged on a directional asymmetry: UERD-aware fine-tune lifts UERD detection but slightly DECREASES J-UNIWARD detection. The natural validation is to run the symmetric experiment: J-UNIWARD-aware fine-tune from the same JIN-SRNet baseline, with the same hyperparameters and data split, and measure J-UNIWARD detection lift. If J-UNIWARD's lift is small (or zero), the asymmetry is real. If J-UNIWARD's lift is comparable to (or larger than) UERD's, the asymmetry is fake.

**Result: the asymmetry is fake.** J-UNIWARD-aware fine-tune lifts J-UNIWARD detection by +26.3 percentage points — *more* than UERD-aware lifts UERD detection (+20.2 pp).

### Three-way comparison — same 198 images, same 256×256 center-crop preprocessing

| group | baseline | UERD-ft | Δ_uerd | J-UW-ft | **Δ_juw** |
|---|---:|---:|---:|---:|---:|
| cover | 18.2% | 17.7% | −0.5 pp | **4.5%** | **−13.6 pp** |
| uniform | 83.8% | 88.9% | +5.1 pp | **95.5%** | **+11.6 pp** |
| uerd | 57.6% | **77.8%** | **+20.2 pp** | 73.7% | +16.2 pp |
| **juniward** | **28.3%** | 25.3% | **−3.0 pp** | **54.5%** | **+26.3 pp** |

Mean P(stego) deltas tell the same story: UERD-ft moves UERD from 0.504 → 0.736 (+0.232), J-UW-ft moves J-UNIWARD from 0.378 → 0.553 (+0.175). Both are real lifts in the same magnitude regime.

### What changes from Update 1's framing

1. **The "structural vs mismatch" framing is wrong.** Both cost functions are vulnerable to attacker adaptation by cheap fine-tunes from the same JIN-SRNet starting point. Neither is structurally robust against attacker adaptation. Update 1 reached the wrong conclusion because it ran only one direction of the experiment and over-interpreted the side effect on J-UNIWARD.

2. **The J-UNIWARD-fine-tuned model is the strongest general detector of the three.** Look at the J-UW-ft column: lowest cover false positive (4.5%), highest Uniform detection (95.5%), competitive UERD detection (73.7%, vs UERD-ft's 77.8%), and best J-UNIWARD detection (54.5%). **Training on the hardest target produced the best detector overall.** This is consistent with a general intuition in adversarial robustness research: training against the most subtle adversary teaches the model the most discriminating features.

3. **The recommendation ("use J-UNIWARD for modern threat models") still stands**, but for a different reason: J-UNIWARD has lower absolute detection rate at every stage of fine-tuning, not because attacker adaptation can't touch it. Specifically, after each cost function is fine-tuned against specifically:

   | | UERD detection | J-UNIWARD detection | gap |
   |---|---:|---:|---:|
   | Baseline JIN-SRNet | 57.6% | 28.3% | 29 pp |
   | After targeted fine-tune | 77.8% | **54.5%** | **23 pp** |

   J-UNIWARD remains 23 pp harder to detect than UERD even when an adversary trains specifically against it. That's a meaningful security margin even in a worst-case "attacker adapted to phantasm" scenario.

4. **Cross-cost-function transfer is real and asymmetric.** The J-UW-fine-tuned model detects UERD at 73.7%, almost as well as the UERD-ft model (77.8%). The UERD-fine-tuned model detects J-UNIWARD at 25.3%, *worse* than the baseline (28.3%). **Training on J-UNIWARD generalizes upward (catches Uniform, UERD, and J-UNIWARD); training on UERD specializes downward and loses J-UNIWARD discrimination.** This is the actual asymmetry — but it's a feature of detector training, not phantasm cost-function evasion.

### Implications for v0.2

- **The recommendation to use J-UNIWARD for modern threat models stands.** Justification is now "lower detection rate at every fine-tuning stage" rather than "structural evasion under attacker adaptation."
- **Option C (adversarial costs) is no longer obviously the right next step.** The original case for C was "hand-designed costs are vulnerable, we need adversarial ones." That's still true *for both UERD and J-UNIWARD*, but the asymmetry that motivated the framing is gone. Option C might still be the right v0.2 work item, but the case needs reframing: "adversarial costs to defeat *specific* deployed detectors at deployment time, accepting that any cost function will fold under fine-tunes from the deployer's own training data."
- **Option B'' — extended dataset** (more passphrases or more covers) is the cheap follow-up that would harden the +20-26 pp lift numbers. Currently both fine-tunes use 288 training examples from 22 unique seeds; a 5× larger set with multiple passphrases per cover would either confirm or shrink the lifts.
- **A genuinely interesting research direction** suggested by Update 2: if J-UNIWARD-aware training produces the best general detector, then a phantasm-aware adversary's optimal strategy is to train on phantasm J-UNIWARD output specifically. Defeating *that* detector is the right v0.2 research target, not defeating JIN-SRNet (which was trained on a different cover source and happens to under-detect everything because of mismatch).

### Files

Three-way eval JSON: `~/phantasm-eval/finetune-juniward/three_way_eval.json` on the dev GPU box. Fine-tuned checkpoints: `~/phantasm-eval/finetune-uerd/uerd_finetune_best.pt` and `~/phantasm-eval/finetune-juniward/juniward_finetune_best.pt`.

## Update 3 — Multi-passphrase fine-tunes (Option B'' complete) — supersedes Update 2's gap claim

Updates 1 and 2 both used a single-passphrase training set (288 examples per fine-tune). The natural extension is to vary the passphrase per cover, producing N stego variants per cover that share the same content but differ in their STC permutations. This functions as data augmentation: the model sees the same 22 unique seeds but learns N times more variants of how phantasm modifies them, mimicking what a real attacker with a phantasm corpus would have access to.

**Setup:** For each of the 198 covers (22 seeds × 9 qf/dim), generate 5 UERD stego variants and 5 J-UNIWARD stego variants using passphrases `ml-multi-pass-{0..4}`. Same payload (3000 bytes), same `--stealth high`. New training data: 1980 stegos + 1980 paired covers per target = 3960 items, 7× larger than the single-pass runs (counting balanced pairs). Same seed-level split (16 train / 3 val / 3 test seeds). Same hyperparameters as Updates 1 and 2 (Adam lr=1e-5, 10 epochs, random crop + flip, init from JIN-SRNet). Wall-clock training time on RTX 5070: **~95 seconds per fine-tune**.

### Five-way comparison — same eval-198 dataset, same 256×256 center-crop preprocessing

| group | baseline JIN-SRNet | UERD 1-pass | JUW 1-pass | **UERD multi** | **JUW multi** |
|---|---:|---:|---:|---:|---:|
| cover | 18.2% | 17.7% | 4.5% | **0.0%** | 0.5% |
| uniform | 83.8% | 88.9% | 95.5% | **99.0%** | 97.5% |
| **uerd** | 57.6% | 77.8% | 73.7% | **85.4%** | 72.7% |
| **juniward** | 28.3% | 25.3% | 54.5% | 77.8% | **89.9%** |

### Post-attacker-adaptation gap (UERD det − J-UNIWARD det)

| training condition | UERD det | J-UW det | gap |
|---|---:|---:|---:|
| Baseline JIN-SRNet | 57.6% | 28.3% | +29 pp (UERD more detected) |
| Single-pass targeted fine-tune (Update 2) | 77.8% | 54.5% | +23 pp |
| **Multi-pass targeted fine-tune (Update 3)** | **85.4%** | **89.9%** | **−4.5 pp (J-UW slightly more detected)** |

### What changes from Updates 1 + 2

1. **The "J-UNIWARD has lower absolute detection rate at every stage" framing from Update 2 was also wrong** — but only at the most extreme attacker-adaptation level. With single-pass training, J-UW had a 23 pp absolute advantage. With 5× more training data per cover, the advantage is gone, and J-UW becomes slightly *more* detectable than UERD at ~85-90% rates. The training-set size that Update 2 used (288 items, single passphrase) was insufficient to expose J-UNIWARD to the kind of adversarial pressure that breaks it.

2. **Both cost functions are detected at ~85-90% by a fully-adapted attacker.** The "use J-UNIWARD for modern threat models" recommendation is now scope-limited: it holds against off-the-shelf and lightly-adapted detectors, but does not defend against an attacker who has trained on phantasm output with sufficient data. The sufficient threshold is small — passphrase variation alone (no new covers) closed most of the gap.

3. **Cover false positive collapses to zero with multi-pass training.** Both multi-pass models have 0/198 or 1/198 cover false positives on the full 198-image eval set. This is a sharper cover boundary than any of the earlier models. **Bigger training data → much tighter cover specificity** at minimal extra training cost.

4. **Cross-cost-function transfer is much stronger with multi-pass training.** UERD-multi catches J-UNIWARD at 77.8% (vs baseline 28.3%, single-pass UERD-ft 25.3%). J-UW-multi catches UERD at 72.7% (vs baseline 57.6%, single-pass J-UW-ft 73.7%). **A multi-pass detector trained on EITHER cost function is a strong general phantasm detector** — far stronger than the single-pass equivalents.

5. **The strongest detector we have is now J-UW-multi.** 89.9% J-UNIWARD detection, 99.0% Uniform detection (essentially saturated), 72.7% UERD detection (close to UERD-multi's 85.4%), and only 0.5% cover false positive. **For Option C (adversarial costs), this is the detector to beat** — not off-the-shelf JIN-SRNet, not the single-pass fine-tunes from Updates 1 + 2.

### Caveats specific to Update 3

- **Same 22 unique seed pool, just augmented by passphrase.** The training set is "5x more views of 16 seeds" rather than "more cover diversity." Results may not generalize to a corpus with 5× more *unique covers*. The cover diversity dimension is still untested.
- **Picsum-trained, Picsum-evaluated.** Cross-cover-source generalization is still untested. A real attacker may not have the user's specific cover distribution, in which case detection rates would be lower (closer to single-pass numbers, possibly closer to baseline).
- **Multi-pass detectors may be over-fit to the 22 training seeds.** Test split is the held-out 3 seeds (135 stegos at 5 passphrases × 9 qf/dim) — high in-distribution but small N at the seed level. Worst-case-attacker numbers should be treated as an upper bound on detection capability, not a tight estimate.
- **Single-pass J-UW fine-tune was undertrained.** Update 2's "J-UW lifts +26 pp" was achieved on 288 examples; the multi-pass version achieves a +35 pp lift on the same metric using 5× the data. The Update 2 framing should be read as "this is what a small fine-tune does," not "this is the ceiling of what fine-tuning can do."

### Implication for v0.2

- **The recommendation to use `--cost-function j-uniward` is now narrower:** it defends against off-the-shelf detectors and modestly-adapted detectors, but folds against a fully-adapted attacker who has trained specifically on phantasm output with sufficient data per cover. For a high-stakes threat model where the adversary may have phantasm-specific training resources, the cost function choice barely matters — both UERD and J-UNIWARD reach 85-90% detection.
- **Option C (adversarial costs) is now the *only* meaningful v0.2 research direction** for defending against a fully-adapted attacker. Hand-designed costs of any flavor have been shown to fail against attacker adaptation in this eval. The case for C is overwhelming.
- **The detector to beat for Option C is J-UW-multi** (89.9% J-UNIWARD detection, 0.5% cover FP). This is the strongest publicly-implementable phantasm detector; an adversarial cost function that cuts its detection rate by even ~30 pp would be a genuine research result.
- **Option B''' (cover-source diversity) is the natural follow-up to Update 3.** Multi-pass with 5× passphrases established that *training data quantity* matters; the next question is whether *training data diversity* (more unique covers) matters more or less. Cheap follow-up: extend Picsum corpus to 500 covers via fetch_corpus extension and re-run.

### Files

Five-way eval JSON: `~/phantasm-eval/finetune-multi-uerd/five_way_eval.json` on the dev GPU box. Multi-pass checkpoints: `~/phantasm-eval/finetune-multi-uerd/uerd_multi_best.pt` and `~/phantasm-eval/finetune-multi-juniward/juniward_multi_best.pt`.

## Update 4 — Option C single-step adversarial costs (FAILED)

The Option B/B'/B'' progression converged on a clear conclusion: hand-designed cost functions (UERD, J-UNIWARD) cannot defend against a fully-adapted CNN attacker. Update 3 named **Option C — adversarial costs targeting the J-UW-multi detector** as the only remaining v0.2 direction that could plausibly defend. This update is the first attempt at C, and it failed to defend. The infrastructure works; the cost-function strategy is wrong.

### What I built

A complete sidecar-cost pipeline so out-of-tree research code can produce per-coefficient cost maps that the existing Rust embed pipeline consumes:

1. **Python adversarial cost computer** (`compute_adv_costs_batch.py`): loads the J-UW-multi checkpoint as a differentiable distortion oracle, decodes a cover via PIL, computes the spatial-domain pixel gradient ∂(stego_logit)/∂(pixel) via PyTorch autograd on a 256×256 center crop, transports the gradient to DCT space via forward blockwise DCT on the Y channel, and writes a sidecar binary file. ~2.5 seconds for all 198 covers on the RTX 5070.
2. **PHCOST sidecar binary format** (v2: single cost array, v3: separate `costs_plus` / `costs_minus` for sign-aware costs). 32-byte header + dense float32 cost grid.
3. **Rust `phantasm_cost::Sidecar` distortion function**: implements `DistortionFunction`, reads PHCOST v2/v3 files, validates cover dimensions match, builds the standard `CostMap` interface that `ContentAdaptiveOrchestrator` already consumes. 5 unit tests.
4. **Rust `phantasm dump-costs` subcommand** (hidden): dumps the per-coefficient cost map of any built-in distortion function (`uniform`, `uerd`, `j-uniward`) to a PHCOST v3 sidecar. Used by the hybrid-cost path to extract J-UNIWARD baseline costs into Python for combination with the adversarial signal.
5. **Rust `phantasm embed --cost-function from-sidecar --cost-sidecar <path>`** (hidden flags): wires the sidecar cost source into the existing embed pipeline. Mutually-exclusive validation against the other cost functions.
6. **Python hybrid cost computer** (`compute_hybrid_costs.py`): loads J-UNIWARD baseline costs from a `dump-costs` sidecar and combines with the adversarial signal as `cost_combined = juw_base × (1 + α × adv_signal)`. Tunable α.

The infrastructure is solid. New tests: `phantasm-cost` 21 → 23 (sidecar v2 + v3 round-trip + bad-magic). Workspace tests 224 → 227 passing.

### What I tried for the cost strategy

Three single-step gradient-based variants, all evaluated on the same 198-cover Picsum eval set against all five detectors from Updates 1+2+3:

| variant | UERD-multi det | **JUW-multi det (target)** | baseline JIN-SRNet det |
|---|---:|---:|---:|
| pure J-UNIWARD baseline (Update 3) | 77.8% | **89.9%** | 28.3% |
| pure |gradient| cost | 99.0% | **98.5%** | 62.6% |
| sign-aware ReLU(±gradient) cost | 99.0% | **99.5%** | 24.2% |
| hybrid J-UW × (1 + 2 × adv_signal) | 78.3% | **89.9%** | 25.3% |
| hybrid J-UW × (1 + 10 × adv_signal) | 77.3% | **91.4%** | 26.8% |

**None of the variants pushed JUW-multi detection below the J-UNIWARD baseline of 89.9%.** Pure adversarial costs were catastrophically worse (modifications moved out of J-UNIWARD's natural high-frequency distribution into low-frequency regions, where the detector found them even more anomalous). Sign-aware was slightly better against the off-the-shelf baseline (24.2% vs 28.3%) but worse on every fine-tuned detector. Hybrid costs at α=2 and α=10 reproduced the J-UNIWARD baseline numbers within ±2 pp — the adversarial bias is being completely swamped by J-UNIWARD's natural cost dynamic range, and increasing α just degrades both directions slightly without producing any defensive lift.

### Why single-step costs don't work (the literature confirms this)

The single-step gradient ∂(stego_logit)/∂(coefficient) computed at the **clean cover** is a local linear approximation of how the detector will respond to a coefficient modification. For a single ±1 modification on a single coefficient, this approximation is reasonable. For phantasm's actual workload — STC modifying ~10,000 coefficients across the cover to embed a 3 KB payload — the linear approximation breaks down almost immediately. The detector is non-linear; the modification direction that minimizes the logit at the cover point becomes irrelevant or actively wrong after a few coefficient flips, because the stego state is no longer near the cover state.

This is the same failure mode that the FGSM-vs-PGD distinction in adversarial ML captures: single-step (FGSM-style) attacks are known to fail against adversarially-aware models, while iterative (PGD-style) attacks succeed. The successful adversarial-steganography literature (ADV-EMB, ADV-IMB, GAN-based work) all uses iterative refinement, not single-step gradients.

The infrastructure I built is the correct foundation for an iterative approach — Python can re-compute gradients after a round of embedding, write a new sidecar, and the Rust embed pipeline can re-embed with the updated costs. But the iteration loop itself is not implemented yet, and that's the actual work needed to make Option C succeed.

### What's still to try

In rough order of likely payoff:

1. **Iterative refinement (PGD-style).** Embed an initial stego using J-UNIWARD costs. Decode it, compute the gradient at the *stego* point (not the cover), use it to bias costs upward at positions where the modification was bad. Re-embed with the new costs to get stego_1. Repeat 3–5 times. Each round is cheap (~1 second per cover on the GPU). Total cost: ~10 minutes for the full corpus + writing the iteration loop. **This is the next thing to try and the most likely to actually work.**

2. **Warm-start from a stego, not a cover.** Compute the gradient at a stego point on the first iteration instead of a cover point. The gradient is more informative near the decision boundary than far from it. ~5 minutes of work.

3. **Different combination strategies.** The multiplicative `(1 + α × adv)` might be the wrong shape. Try additive (`juw + α × adv`), or saturating (`juw × min(K, 1 + α × adv)`), or a learned blend. ~15 minutes per variant.

4. **End-to-end differentiable embedding.** Replace STC with a differentiable embedding layer so the entire embed-and-detect chain can be optimized as a single graph. This is the most ambitious and most fragile direction; closer to research than engineering. Days, not hours.

### Implications

Even though Update 4 is a negative result, it's a useful one:

- **Phantasm's hand-designed cost functions are exhausted as a defense direction** for fully-adapted attackers. Updates 1+2+3 already pointed at this; Update 4 confirms that simple gradient-based adversarial costs are also insufficient. The remaining defense space is iterative or end-to-end methods.
- **The infrastructure built for Option C is preserved and ready for the iterative attempt.** The PHCOST sidecar format, the Rust `Sidecar` distortion, the `dump-costs` subcommand, and the Python pipeline are all working. The next attempt at C does not start from zero — it just adds the iteration loop on top of the existing single-step plumbing.
- **The "use J-UNIWARD for modern threat models" recommendation is unchanged.** Hybrid costs at α=2 reproduce J-UNIWARD's baseline detection rates within statistical noise, which means using sidecar costs is no worse than J-UNIWARD as long as the adversarial signal is well-bounded. In a future iterative-cost world, sidecar costs would be the deployment path; for now, J-UNIWARD remains the recommendation.

### Files

Sidecar-cost infrastructure: `phantasm-cost/src/sidecar.rs`, `phantasm-cli/src/commands/dump_costs.rs`, plus the `--cost-function from-sidecar` and `--cost-sidecar` flags in `phantasm-cli/src/main.rs`. Python: `~/phantasm-eval/advcost/{compute_adv_costs_batch.py, compute_hybrid_costs.py, eval_advcost.py}` on fishbowl. Generated JUW baseline costs: `/tmp/phantasm-advcost/juw_costs/` on the dev Mac (198 sidecars). Adv-cost stegos and eval results: `/tmp/phantasm-advcost/stego/` and `~/phantasm-eval/advcost/eval_advcost.json`.

## Update 5 — Option D passphrase-randomized cost noise (FAILED)

Update 4 diagnosed that single-step adversarial gradients don't defend because the per-position gradient at a clean cover is a local linear approximation that breaks down after the first few coefficient flips. Update 5 takes a fundamentally different angle suggested by the user: instead of per-coefficient adversarial cost, **make the cost function itself vary per-passphrase** via deterministic multiplicative noise. The hypothesis: even with a constant base cost function, phantasm's modification pattern across different passphrases looks nearly identical at the population level (the cost map is a function of the cover alone, not the passphrase). That means an attacker's CNN only has to learn one underlying distribution regardless of how many passphrase variants they collect. Adding passphrase-derived cost noise should fragment that distribution, forcing the attacker to learn exponentially many patterns.

### Setup

A new `phantasm-cost::Noisy<D>` wrapper applies `cost'[k] = base_cost[k] * (1 + α * noise_k)` where `noise_k ∈ [-1, +1]` is SHA-256-keyed on the passphrase plus position. At `α=0.0` the wrapper is the identity; at `α=0.5` costs are wiggled within `[0.5, 1.5]` of the base value. Wired through a new `phantasm embed --cost-noise <amplitude>` flag (hidden) with bounds-checking (`[0, 2]`) and a warning at high values. The wrapper does not break extract because the STC decoder doesn't consult the cost map — it just reads parities at the passphrase-derived position list.

Ran the same 5-passphrase, 198-cover, 10-epoch fine-tune as Update 3 with `--cost-noise 0.5` for both UERD and J-UNIWARD, then evaluated all detectors against (a) the original eval-198 non-noisy stegos and (b) the new noisy test split.

### Result

| detector | JUW non-noisy | JUW noisy | Δ | UERD non-noisy | UERD noisy | Δ |
|---|---:|---:|---:|---:|---:|---:|
| baseline JIN-SRNet | 28.3% | 27.9% | −0.4p | 57.6% | 60.0% | +2.4p |
| uerd_multi (no noise) | 77.8% | 85.4% | +7.6p | 85.4% | 90.8% | +5.5p |
| juw_multi (target) | 89.9% | 95.5% | **+5.6p** | 72.7% | 77.0% | +4.2p |
| uerd_noise05 (noise-trained) | 76.3% | 84.0% | +7.8p | 84.8% | 90.8% | +6.0p |
| juw_noise05 (noise-trained) | 88.4% | 93.7% | +5.4p | 70.7% | 76.4% | +5.7p |

**Cost-noise doesn't defend — it actively makes things slightly worse.** Every fine-tuned detector scores *higher* on noisy stegos than on non-noisy stegos (+4 to +8 pp). Noise-aware training doesn't help — `juw_noise05` detects noisy J-UNIWARD stego at 93.7% vs `juw_multi`'s 95.5%, neither of which is meaningfully lower than the non-noisy baseline.

### Why

The CNN learns at a *higher level* than per-position fingerprints. It doesn't memorize "this passphrase uses these exact coefficients"; it learns "what does a phantasm-modified JPEG DCT block look like statistically." Cost-noise reweights the cost ranking but every passphrase still ends up modifying coefficients drawn from the same content-adaptive high-frequency distribution as non-noisy phantasm. The noise itself adds a *new* statistical signature — the modifications become slightly less optimal per-J-UNIWARD (since the noise perturbs them away from the natural minimum), so each individual stego becomes *more* anomalous. The CNN picks up on that increased per-stego anomaly as signal.

**The user's instinct was directionally correct** (fragment the attacker's training distribution) **but the implementation level was wrong** (cost noise operates *within* a fixed candidate set and doesn't change the distribution the CNN actually learns). The Update 6 follow-up attempts a deeper fragmentation by changing the candidate set itself.

## Update 6 — Option D' passphrase-derived position subset (MIXED RESULT; ships as optional L1-hardening knob)

The natural refinement of Update 5's failure: instead of perturbing costs *within* a fixed candidate set, use the passphrase to select a deterministic *subset* of usable positions before STC runs. Different passphrases forbid different halves of the non-DC positions (via wet-paper infinity costs), so the CNN sees a genuinely different candidate distribution per passphrase — not just a different cost ranking within the same distribution.

### Setup

A new `phantasm-cost::PassphraseSubset<D>` wrapper marks `(1 - keep_fraction)` of non-DC positions as wet via a SHA-256-keyed per-position bitmask. `keep_fraction=1.0` is the identity; `keep_fraction=0.5` marks 50% wet; and so on. Extract is unaffected because STC's decoder reads all positions regardless of wet marks — the wet-paper constraint is encoder-side only. Wired through `phantasm embed --cost-subset <keep_fraction>` (hidden).

Embedded the 5-passphrase, 198-cover training set at `--cost-subset 0.6 --stealth low`. Fine-tuned both UERD and J-UNIWARD detectors on the subset data and ran a five-way eval against eval-198 (non-subset) and multi-subset (the new subset data).

### Result

| detector | eval-198 JUW (non-subset) | multi-subset JUW (avg) | Δ |
|---|---:|---:|---:|
| **baseline JIN-SRNet (off-the-shelf)** | 28.3% | **22.1%** | **−6.2 pp** ✓ defense win |
| uerd_multi | 77.8% | 93.9% | +16.1 pp |
| juw_multi (target) | 89.9% | 98.8% | +8.9 pp |
| uerd_subset06 (subset-trained) | 41.4% | 71.2% | +29.8 pp |
| juw_subset06 (subset-trained) | 75.3% | 96.5% | +21.2 pp |

**This is the first and only approach in the entire drive that produced a defense-win delta**: against an off-the-shelf JIN-SRNet (which has no phantasm-specific training), subset-stego is detected at **22.1% vs 28.3%** — a real 6.2 pp improvement over regular J-UNIWARD. Against every fine-tuned detector, subset-stego is detected *more* strongly (+8.9 to +30 pp), because STC is forced to concentrate modifications in a smaller candidate pool and the per-stego density anomaly becomes easier for an adapted detector to flag.

Two findings pulling in opposite directions:

1. **Against off-the-shelf detectors, Option D' defends by 6.2 pp.** First and only approach in the drive to actually move an L1 detection rate DOWN.
2. **Against phantasm-trained detectors, Option D' makes things worse.** Fine-tuned detectors love the density signature.

**Costs:**
- **23% embedding failure rate at `subset=0.6`**: 45/198 covers failed because STC couldn't find a solution with 40% of positions wet. Phantasm's STC at constraint height 7 tolerates up to ~40% wet before infeasibility; past that, STC throws "all positions are wet but the syndrome does not match the message." Not production-shippable at `0.6` — but it works at `0.7-0.9` with progressively lower fragmentation. The flag defaults to `1.0` (identity).
- **Subset-trained detectors don't transfer well to non-subset stego** (uerd_subset06 gets 41.4% on non-subset J-UW vs uerd_multi's 77.8%). An attacker training specifically on subset data produces a narrower detector that doesn't generalize back to non-subset variants. This is actually consistent with the fragmentation hypothesis but doesn't translate into real stego-side defense because the same attacker can collect non-subset data separately.

### What ships in v0.2

`--cost-subset` and `--cost-noise` both ship as **hidden research flags** with bounds-checking and warnings. Neither is a load-bearing v0.2 feature. They exist for:
1. Users who explicitly opt into the plausible-deniability threat model against off-the-shelf adversaries and accept the trade-offs documented here
2. Future research — the infrastructure (Noisy / PassphraseSubset / Sidecar wrappers, `dump-costs` subcommand, PHCOST v2/v3 sidecar format) is the foundation for v0.3+ iterative adversarial approaches that take the diagnoses in Updates 4-6 seriously
3. Experimental composition: `--cost-noise` + `--cost-subset` + `--cost-function` stack cleanly via wrapper composition, so callers can test combinations

The CLI flags default to identity behavior. Existing scripts that don't set them continue to work exactly as before.

### The honest v0.3 L1 story (updated with Updates 7 + 8)

After Updates 1-8, here's the most compact true statement about phantasm's L1 detectability at 3 KB payload (~0.2 bpnzac) on Picsum covers:

| Threat | L1 detection rate | Defense level |
|---|---|---|
| Classical Fridrich RS | 26.8% (UERD) / 30.3% (J-UNIWARD) | strong (-40 pp vs Uniform LSB) |
| Modern off-the-shelf CNN (JIN-SRNet) | 28.3% (J-UW) / 22.1% (J-UW + subset=0.6) | moderate; subset flag adds 6.2 pp |
| Phantasm-trained CNN (single-passphrase fine-tune) | 54.5% (J-UW) | weak |
| Phantasm-trained CNN (multi-passphrase fine-tune, 198 covers) | 89.9% (J-UW) / 98.8% (J-UW + subset) | **fails** |
| **Phantasm-trained CNN (multi-passphrase fine-tune, 500 covers — Update 7)** | **96.8% on d500 held-out / 97.5% on eval-198** | **fails harder** |

L1 degrades from "strong" to "fails harder" across the threat-model spectrum. Against the strongest threat we have measured (phantasm-trained CNN with 500 unique covers and 5 passphrases per cover), **L1 provides ≈3% stealth** — roughly at the floor of test-set noise. Update 8 (iterative PGD adversarial costs) established that iteratively tuning per-coefficient costs cannot pull detection back down against such an attacker: every hyperparameter config in the 6-way sweep produced 100% detection.

**Phantasm's v0.3 security argument does not rest on L1.** It rests on L2 (passphrase-keyed position permutation) and L3 (AEAD + Argon2id), both of which use standard primitives with well-understood security properties. L1 is a *bonus stealth layer against casual/off-the-shelf adversaries*, characterized honestly, with a narrow optional knob (`--cost-subset`) that adds 6.2 pp against off-the-shelf detectors. Against phantasm-aware ML, L1 does not defend, and no lever inside the hand-designed cost-function paradigm can make it defend.

## Update 7 — Cover-source diversity (Option B''' complete; CORPUS-ROBUST)

Update 3 established that *training data quantity* (5 passphrases per cover) lifts the phantasm-aware J-UNIWARD detection rate from 28.3% off-the-shelf to 89.9%. The open question at v0.2 ship time was whether that number was a Picsum-corpus overfit artifact — did the detector learn "what phantasm modifies" or "what phantasm modifies *on these 22 specific Picsum seeds*"? Option B''' tests this by holding training recipe constant and scaling the cover pool 2.5× to 500 unique Picsum seeds (same qf85/720 bucket).

### Setup

- Extended the Picsum fetch script to pull 500 unique seeds (`phantasm-0001..0500`) at qf85/720. Manifest committed; images gitignored per repo convention. Commit `59cde9c`.
- Produced 2500 stego variants per cost function (500 covers × 5 passphrases `ml-multi-pass-{0..4}`) — same passphrase-as-augmentation recipe as Update 3.
- Seed split: 364 train / 68 val / 68 test covers (~73/14/14) at the seed level. Same 73/14/14 ratio as Update 3's 16/3/3; just scaled up.
- Same hyperparameters as Update 3 (Adam lr=1e-5, 10 epochs, random 256×256 crop + horizontal flip, init from JIN-SRNet). Wall-clock training time on RTX 5070: **~5 minutes** for J-UW-multi-d500 fine-tune.
- Evaluated against the d500 held-out split (340 stegos = 68 test covers × 5 passphrases) AND against the original eval-198 set for cross-corpus-consistency.
- Commit `c8202c5`. Fishbowl artifacts: `~/phantasm-eval/corpus-diversity-500/runs/juw_d500_v1/`.

### Result — detection rate rises, not falls

| Fine-tune condition | Eval set | J-UW detection rate | Δ vs Update 3 |
|---|---|---:|---:|
| Update 3 baseline (198 covers, multi-pass) | eval-198 | 89.9% | — |
| **Update 7 — 500-cover multi-pass** | **d500 held-out (340 stegos)** | **96.8%** | **+6.9 pp** |
| Update 7 — 500-cover multi-pass | eval-198 (original) | 97.5% | +7.6 pp |

The d500-trained detector is **stronger** on both the new held-out split and on the original eval-198 set. Cover-source diversity did not weaken the detector by diluting training concentration; it strengthened it by tightening the decision boundary.

### Interpretation

1. **The Update 3 89.9% number was NOT a cover-pool artifact.** If it had been, expanding the cover pool should have degraded the detector — but it lifted it instead. This closes the Option B''' question cleanly.
2. **The phantasm-modification signature generalizes across Picsum seeds.** The CNN is learning "what phantasm-modified DCT blocks look like statistically," not "what phantasm modifies on these 22 specific images." The signature has the structural quality of a proper fingerprint across the Picsum corpus.
3. **Cross-corpus generalization remains untested.** BOSSbase / ALASKA2 / non-Picsum covers could still produce lower numbers due to distribution shift. The Update 7 result rules out the specific Picsum-seed-overfit failure mode; it does not rule out cover-source mismatch in the general sense.
4. **Combined with Update 8 (below), the cost-function research direction is closed.** Every axis we can vary inside a hand-designed cost-function paradigm — training set size (Update 3), passphrase variation (Update 3), cover-source diversity (Update 7), per-coefficient gradient biasing (Updates 4 + 8) — has been tested. None defend against a fully-adapted phantasm-aware CNN.

### Files

Fine-tuned checkpoint: `~/phantasm-eval/corpus-diversity-500/runs/juw_d500_v1/juw_d500_best.pt`. Training log: same directory, `train.log`. Cross-eval JSON: `five_way_eval_d500.json` in the same run directory.

## Update 8 — Iterative PGD adversarial costs (Option C-iter complete; FAILED)

Update 4 attempted single-step gradient-based adversarial costs and failed — the linear approximation ∂(stego_logit)/∂(coefficient) computed at the clean cover breaks down after the first few STC-driven coefficient flips, so the cost signal stops steering toward the decision boundary. The literature fix is iterative: re-compute the gradient at the *partial stego* after each embed round and bias costs with the fresh signal. This is the PGD-vs-FGSM distinction from adversarial ML applied to the cost-function setting.

### Setup

- Inherited all infrastructure from Update 4 (PHCOST sidecar format, Rust `Sidecar` distortion, `dump-costs` subcommand, Python cost computer, `--cost-function from-sidecar --cost-sidecar <path>` CLI path). No new Rust code needed.
- New Python iteration loop (`scratch/advcost-iter/iterative_refine.py`, kept and gitignored) that: (a) embeds an initial stego with J-UNIWARD costs, (b) decodes it, (c) re-computes the detector gradient at the stego point, (d) writes a new sidecar combining J-UNIWARD base with the fresh adversarial signal, (e) re-embeds from the cover with the updated costs, (f) repeats T times.
- Default config: T=4 iterations, α=2.0 combination weight, warm-start from stego point (not cover), max-abs gradient normalization.
- Target detector: J-UW-multi from Update 3 (89.9% J-UW detection on eval-198, 0.5% cover FP). Full-corpus eval on eval-198 after each iteration.
- Fishbowl artifacts: `~/phantasm-eval/advcost/iter-full-T4-a2-stego/`.

### Trajectory — iteration makes things strictly worse

| T | J-UW-multi detection rate | Δ vs T=0 |
|---:|---:|---:|
| 0 (J-UNIWARD baseline) | 91.9% | — |
| 1 | 96.5% | +4.6 pp |
| 2 | 98.5% | +6.6 pp |
| 3 | 99.5% | +7.6 pp |
| **4** | **100.0%** | **+8.1 pp** |

(T=0 here differs slightly from Update 3's 89.9% because this run used the iterative script's harness which regenerates stegos per iteration; noise is within ±2 pp.)

**Iteration does not defend — it actively degrades.** Each PGD round adds new anomalies to the stego distribution that the detector latches onto.

### Hyperparameter sweep — every config worse than baseline

| Config | Warm start | Norm | α | T | J-UW-multi det |
|---|---|---|---:|---:|---:|
| J-UW baseline | — | — | — | 0 | 91.9% |
| primary (above) | stego | max-abs | +2.0 | 4 | **100.0%** |
| longer | stego | max-abs | +2.0 | 8 | 100.0% |
| weaker α | stego | max-abs | +0.5 | 4 | 99.0% |
| stronger α | stego | max-abs | +5.0 | 4 | 100.0% |
| sign-flip control | stego | max-abs | −2.0 | 4 | 100.0% |
| cover warm-start | cover | max-abs | +2.0 | 4 | 98.5% |
| percentile-norm | stego | percentile-95 | +2.0 | 4 | 99.5% |

**Every swept config was worse than the J-UNIWARD baseline.** The sign-flip control is the critical one: α=−2.0 means the cost signal pushes *toward* the detector's decision boundary instead of away from it. If iteration had any real signal, flipping the sign should produce a dramatically different result from α=+2.0. Instead it produces an essentially identical 100% detection rate. **This rules out a direction bug** — the detector is genuinely insensitive to the transport mechanism per-coefficient cost adjustment relies on.

### Diagnosis

The single-step failure mode from Update 4 escalates cleanly to iterative formulations:

1. **The detector is non-linear at every point along the embed trajectory.** The gradient at the partial stego is only locally informative; once STC + quantization apply the next round's modifications, the gradient's target region has moved, same as in the single-step case. Iteration doesn't fix this because each round's modifications are still chosen by the transport mechanism (cost ranking → STC position selection → ±1 sign) that's fundamentally linear in the cost signal.
2. **STC's cost-to-modification transfer function is discretized and non-smooth.** A per-coefficient cost doesn't directly translate to a per-coefficient modification: STC picks a small minimum-cost subset of positions that satisfies the syndrome, then signs them to match. Gradient information can only bias *which subset gets picked*, not *what's applied to those positions*. After JPEG quantization re-coarsens the result, most of the gradient's original fine-grained directional information is lost.
3. **The detector latches onto "anomalous modification distribution" as a feature.** Every iterative step that bent modifications away from J-UNIWARD's natural high-frequency distribution made them easier to detect, not harder. This is why α=±2 produce identical results: the magnitude of the off-J-UW distortion is the signal, regardless of direction.

**Implication — per-coefficient cost-function adjustment is exhausted as an L1 defense direction** against trained non-linear detectors. What remains structurally different:

- **End-to-end differentiable embedding.** Replace STC with a differentiable embedding layer so the full embed-and-detect chain can be optimized as a single graph. Eliminates the "cost signal → STC transfer function → modification" bottleneck entirely. Most ambitious; v0.4+ candidate.
- **ADV-EMB / ADV-IMB** (STC-aware iterative attacks that jointly optimize coefficient *choice* and sign with STC structure as a constraint). Academic literature with reference implementations; the natural follow-up if end-to-end differentiable embedding is too ambitious.
- **Multi-cover payload spreading.** Accept that any single cover can be detected; spread the syndrome across N covers so no single detection is diagnostic. Fundamentally different L1 framing.

### Files

Script (gitignored): `scratch/advcost-iter/iterative_refine.py`. Fishbowl per-T stegos and eval JSONs: `~/phantasm-eval/advcost/iter-full-T4-a2-stego/T{0..4}/`. Sweep results: `~/phantasm-eval/advcost/iter-sweep/*.json`.

## v0.2 research direction proposal

Based on these results, three credible v0.2 directions, in rough priority order:

### Option A — Lean into J-UNIWARD as the modern-threat-model default

**The case:** J-UNIWARD demonstrably evades both pretrained CNNs in our eval, in both absolute and paired terms. UERD does not. Phantasm should ship a `--threat-model {classical, modern}` flag (or rename the default) that selects UERD or J-UNIWARD accordingly. Update the README and STATUS to reflect the new finding. ~1-2 days of work plus eventual security review.

**Risks:** J-UNIWARD is 3.5× slower than UERD (v0.1.0 bench numbers). Acceptable for security-first users, painful for capacity-first.

**What this doesn't address:** Eventually CNNs trained against J-UNIWARD specifically will fire on it. Today's eval used J-UNIWARD-trained CNNs and they don't fire — but that's because they were trained on different cover sources. A future ALASKA3-style competition could close that gap.

### Option B — Build phantasm's own UERD-trained detector — **DONE, see § Update 1**

**Result:** A 21-second fine-tune from JIN-SRNet lifted UERD detection from 57.6% to 77.8% (+20 pp). Same fine-tune slightly DECREASED J-UNIWARD detection (28.3% → 25.3%). UERD's modern-detector robustness was partially a cost-function-mismatch artifact; J-UNIWARD's was structural. The "use J-UNIWARD for modern threat models" recommendation is reinforced.

### Option B' — Validate the asymmetry — **DONE, see § Update 2. Asymmetry was fake.**

**Result:** J-UNIWARD-aware fine-tune lifts J-UNIWARD detection by +26.3 pp, more than UERD-aware lifts UERD (+20.2 pp). Both cost functions are vulnerable to attacker adaptation. The "structural vs mismatch" framing in Update 1 was wrong. The "use J-UNIWARD" recommendation stands but for a different reason: J-UNIWARD has a lower absolute detection rate at every fine-tuning stage, including post-attacker-adaptation (54.5% vs UERD's 77.8%, a 23 pp gap).

### Option B'' — Extended dataset hardening — **DONE, see § Update 3. Demolished the asymmetry.**

**Result:** Multi-pass fine-tunes (5 passphrases per cover, 5× more training data) push UERD detection to 85.4% and J-UNIWARD detection to 89.9% — J-UNIWARD becomes slightly MORE detectable than UERD at the worst-case attacker-adaptation level. Both multi-pass models hit ~0% cover false positive on the full 198-image eval. The asymmetry from Update 2 was a small-N artifact. The "use J-UNIWARD" recommendation is now scope-limited to lightly-adapted threat models.

### Option B''' — Cover-source diversity follow-up — **DONE, see § Update 7. Closed negatively.**

**Result:** 500-cover multi-pass fine-tune pushes J-UNIWARD detection to **96.8% on the d500 held-out split (97.5% on eval-198)** — +6.9 pp / +7.6 pp above Update 3's 89.9%. The 89.9% number was not a cover-pool artifact; cover-source diversity strengthens the detector rather than diluting it.

### Option C — Adversarial costs (reframed after Update 3)

**The case (post-Update 3):** Update 3 demolished the last claim that any hand-designed cost function is structurally robust against attacker adaptation. With 5× more training data per cover, both UERD and J-UNIWARD reach 85-90% detection by a fine-tuned model. **The cost-function-design research direction is, on this evidence, exhausted as a defense against fully-adapted attackers.** The only remaining direction that could possibly defend is adversarial costs that explicitly target the deployed detector's decision boundary at deployment time.

**Concrete approach:** Use the J-UW-multi checkpoint from Update 3 (the strongest detector publicly implementable: 89.9% J-UNIWARD detection, 0.5% cover FP) as a differentiable distortion oracle. Compute per-coefficient costs that maximize distance from its decision boundary. Phantasm ships `--cost-function adv-juw-multi` that targets *that* specific detector. The success criterion: cut detection rate from 89.9% to below 50% while preserving the existing J-UNIWARD perceptual-distortion guarantees.

**Risks:** Substantial implementation work. The PyTorch graph would need to be wired into the embed pipeline (currently pure Rust). Cleanest architecture: a Python pre-pass that produces a per-coefficient cost map, written to a sidecar file, then read by the existing Rust embed pipeline. Doable in ~1 week. The research payoff depends entirely on whether the adversarial cost actually beats the J-UW-multi detector by a meaningful margin — which is genuinely uncertain. If it doesn't, phantasm has no defense against a fully-adapted attacker, and the v0.2 narrative becomes "scope-limited to non-adapted threat models, plus channel adapter and ECC improvements."

**What this enables (best case):** A defensible claim that phantasm's adv-cost mode beats a state-of-the-art detector trained specifically on phantasm output. That is the only research result that would extend phantasm's defended threat model beyond "off-the-shelf and lightly-adapted detectors." The claim is also threat-model-honest: it doesn't pretend to defend against *unbounded* adversary adaptation, only against the specific deployed detector at training time.

### Recommended path (v0.2 SHIPPED; v0.3 additions)

v0.2:
- **Option A** — docs + threat-model framing in CLI help. **DONE** (commit `c450aa6`).
- **Option B** — UERD fine-tune experiment. **DONE**, Update 1.
- **Option B'** — symmetric J-UNIWARD fine-tune, refutes Update 1's asymmetry claim. **DONE**, Update 2.
- **Option B''** — multi-passphrase extended training, demolishes the gap. **DONE**, Update 3.
- **Option C (single-step adversarial costs)** — **DONE, FAILED**, Update 4. Infrastructure preserved.
- **Option D (passphrase-randomized cost noise)** — **DONE, FAILED**, Update 5.
- **Option D' (passphrase-derived position subset)** — **DONE, MIXED**, Update 6. Ships as hidden `--cost-subset` flag; provides a 6.2 pp defense against off-the-shelf detectors at the cost of per-stego density anomaly against phantasm-trained ones and a 23% embed-failure rate at `subset=0.6`.

v0.3 (added during the v0.2 → v0.3 burst):
- **Option B''' (cover-source diversity)** — **DONE, CLOSED NEGATIVELY**, Update 7. 500-cover multi-pass fine-tune lifts detection to 96.8% / 97.5% — the 89.9% number was not a cover-pool artifact.
- **Option C-iter (iterative PGD adversarial costs)** — **DONE, FAILED**, Update 8. Every swept config produced 100% detection; per-coefficient cost adjustment is exhausted as an L1 defense direction.

### Deferred to v0.4+ (further L1 research — hand-designed cost direction is closed)

With Updates 7 + 8 closing both remaining cost-function-paradigm levers, the remaining L1 defense directions are all structurally different from "per-coefficient cost adjustment":

- **End-to-end differentiable embedding** — replace STC with a differentiable embedding layer so the whole chain can be optimized together. Eliminates the "cost signal → STC transfer function → modification" bottleneck that Update 8 diagnosed as load-bearing. Most ambitious; v0.4+ candidate.
- **ADV-EMB / ADV-IMB** — STC-aware iterative attacks that jointly optimize coefficient *choice* and sign with STC structure as a constraint. Academic literature with reference implementations.
- **Multi-cover payload spreading** — spread the syndrome across N covers so no single-cover detection is diagnostic. Fundamentally different L1 framing; also synergizes with the v0.3 channel-adapter BER findings (more covers = more redundancy = lower per-cover FEC burden).
- **Composable L1 hardening framework** — the Noisy / PassphraseSubset / Sidecar wrappers are still useful infrastructure for any v0.4+ direction.

### Out of scope for the L1 research track entirely

L2 and L3 testing does not belong in `ML_STEGANALYSIS.md`. That's a separate external-security-review track — the crypto layer uses standard primitives with well-understood security properties that shouldn't be re-validated via ML steganalysis techniques.

## Reproduction

All eval scripts and intermediate JSONs live in `/tmp/phantasm-eval-198/`, `/tmp/phantasm-curve/`, and `/tmp/aletheia-onnx/` on the dev Mac. Inference scripts and pretrained weights live in `~/phantasm-eval/` on `fishbowl` (RTX 5070 box).

Pretrained model URLs:
- JIN-SRNet (PyTorch): https://janbutora.github.io/assets/scripts/JIN_SRNet.zip
- Aletheia EfficientNet-B0 J-UNIWARD: https://github.com/daniellerch/aletheia/raw/master/aletheia-models/effnetb0-A-alaska2-juniw.h5

Total eval time: ~5 minutes embedding + ~3 minutes inference + scripts. The 5070 makes the inference round trivially fast.

---

**Author:** Phantasm dev sessions, 2026-04-13 (Updates 1-6) and 2026-04-14 through 2026-04-19 (Updates 7-8).
**Methodology review:** Recommended before publishing externally.
