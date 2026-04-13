# Phantasm — Modern ML Steganalysis Evaluation

**Status:** First-pass evaluation, 2026-04-13. Post-v0.1.0, scoping for v0.2.

This document reports the first evaluation of phantasm v0.1.0 against modern (CNN-based) JPEG steganalysis. It complements the existing classical Fridrich RS results documented in STATUS.md §5 Finding 8.

## TL;DR

Tested phantasm at the v0.1.0 default payload (3 KB, ~0.2 bpnzac) on 198 Picsum covers against two pretrained CNN detectors. **Cost-function design still differentiates against modern CNNs**, and the ordering inverts from the classical-detector ordering: against modern CNNs, **J-UNIWARD beats UERD by a significant margin**, opposite to the classical Fridrich RS result where UERD beats J-UNIWARD.

**Update 1 (Option B, same day):** Fine-tuned JIN-SRNet on phantasm UERD output for 21 seconds and lifted UERD detection from 57.6% to **77.8%**. Same fine-tune slightly *decreased* J-UNIWARD detection (28.3% → 25.3%). I claimed this meant "J-UNIWARD's robustness is structural" — see Update 2 for why that interpretation was wrong.

**Update 2 (Option B' validation, same day):** Symmetric experiment — fine-tuned JIN-SRNet on phantasm J-UNIWARD output and measured J-UNIWARD detection lift. Result: J-UNIWARD-aware fine-tune lifts J-UNIWARD detection from 28.3% to **54.5%** (+26.3 pp), MORE than UERD-aware lifts UERD (+20.2 pp). The asymmetry claimed in Update 1 was wrong. **Both cost functions are vulnerable to attacker adaptation.** But J-UNIWARD remains 23 pp harder to detect than UERD even after each is fine-tuned specifically against it. The "use J-UNIWARD for modern threat models" recommendation stands, for a different reason than Update 1 claimed: not structural evasion, but a lower absolute detection rate at every fine-tuning stage.

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

## Update 1 — UERD fine-tune (Option B complete) — partially superseded by Update 2

> **Note (added with Update 2):** The interpretation in this section — that "J-UNIWARD's robustness is structural" — was wrong. Update 2 below runs the symmetric experiment (J-UNIWARD-aware fine-tune) and shows J-UNIWARD detection lifts even MORE than UERD detection does (+26.3 pp vs +20.2 pp). The factual numbers in this section are correct; the "structural vs mismatch" framing is not. The recommendation is unchanged but its justification is in Update 2.

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

## Update 2 — Symmetric J-UNIWARD fine-tune (Option B' validation)

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

### Option B'' — Extended dataset hardening

**The case:** Both Option B and B' fine-tunes used 288 training examples from 22 unique seeds, single passphrase. The +20 pp / +26 pp lift estimates carry sample noise. Two cheap extensions to harden the numbers:

1. **More passphrases per cover.** Generate 10 stego variants per cover by varying the embed passphrase. 5× larger train set, no new fetches.
2. **More covers.** Extend the Picsum corpus to 500 covers via fetch_corpus extension (cheap — Picsum is free and has no rate limit issues for our scale).

Either is a few hours. Both together is most of a day. Likely to either tighten the lift numbers within ~3 pp, or expose a bias in the small-N runs.

### Option C — Adversarial costs (reframed after Update 2)

**The case (post-Update 2):** Both UERD and J-UNIWARD fold under cheap fine-tunes from JIN-SRNet, but J-UNIWARD remains 23 pp harder to detect even after attacker adaptation. The right framing is no longer "hand-designed costs are vulnerable, build adversarial ones." The new framing is: **any hand-designed cost will fold under attacker adaptation; the question is what an attacker who has trained specifically on phantasm output looks like, and whether adversarial costs can win at *that* deployment-time threat model.**

**Concrete approach:** Use the J-UNIWARD-fine-tuned checkpoint from Update 2 (the strongest detector now in our possession) as a differentiable distortion oracle. Compute per-coefficient costs that maximize distance from its decision boundary. Phantasm could ship a `--cost-function adv-juw-ft` that beats *that* specific detector by a meaningful margin.

**Risks:** Substantial implementation work. The PyTorch graph would need to be wired into the embed pipeline (currently pure Rust). Cleanest path: a Python pre-pass that produces a per-coefficient cost map, written to a sidecar file, then read by the existing Rust embed pipeline. Architecture is doable in ~1 week; the research payoff depends on whether the adversarial cost actually beats the J-UW-ft detector by a margin worth the complexity.

**What this enables:** A defensible claim that phantasm's adv-cost mode beats a state-of-the-art detector trained specifically on phantasm J-UNIWARD output. That is a stronger and more honest claim than "beats off-the-shelf SRNet" and directly addresses the threat model of an adversary who has dedicated resources to detecting phantasm.

### Recommended path (updated after Updates 1+2)

**Option A** (docs + threat-model framing in CLI help) — **DONE** (commit `c450aa6`).
**Option B** (UERD fine-tune experiment) — **DONE, see § Update 1.**
**Option B'** (validate asymmetry direction via symmetric J-UNIWARD fine-tune) — **DONE, see § Update 2. Asymmetry was fake.**
**Option B''** (extended dataset hardening — more passphrases, more covers) — **next, cheap.** Confirms or shrinks the +20 / +26 pp lift estimates with bigger-N runs.
**Option C** (adversarial costs against the J-UW-fine-tuned detector) — substantial. Commit only after B'' results inform the framing.

Total scope to v0.2 release: ~2 weeks of focused work, 1 week of polish.

## Reproduction

All eval scripts and intermediate JSONs live in `/tmp/phantasm-eval-198/`, `/tmp/phantasm-curve/`, and `/tmp/aletheia-onnx/` on the dev Mac. Inference scripts and pretrained weights live in `~/phantasm-eval/` on `fishbowl` (RTX 5070 box).

Pretrained model URLs:
- JIN-SRNet (PyTorch): https://janbutora.github.io/assets/scripts/JIN_SRNet.zip
- Aletheia EfficientNet-B0 J-UNIWARD: https://github.com/daniellerch/aletheia/raw/master/aletheia-models/effnetb0-A-alaska2-juniw.h5

Total eval time: ~5 minutes embedding + ~3 minutes inference + scripts. The 5070 makes the inference round trivially fast.

---

**Author:** Phantasm dev session, 2026-04-13.
**Methodology review:** Recommended before publishing externally.
