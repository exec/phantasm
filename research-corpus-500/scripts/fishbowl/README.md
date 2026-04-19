# Fishbowl training scripts — corpus-diversity-500

Scripts to fine-tune JIN-SRNet on the 500-cover diversity corpus and
measure J-UNIWARD detection rate against (i) the original Update 3
eval-198 set and (ii) a new held-out split from the 500-cover set.

## Provenance

Forked from the Update 3 training script that lives at
`~/phantasm-eval/finetune-multi-juniward/` on fishbowl. Hyperparameters
preserved verbatim (Adam lr=1e-5, 10 epochs, random 256×256 crop + flip,
init from JIN-SRNet) so that detection-rate differences between the
198-cover and 500-cover runs isolate cover-pool size as the variable.

The 198-cover Update 3 script is **NOT** deleted or modified by these
scripts — everything lives under a new directory.

## Deploy

From the phantasm repo root (macOS side), after the embed step has
produced stego files under `research-corpus-500/stego/`:

```sh
# Push covers + stegos to fishbowl.
rsync -av --include='manifest.json' --include='qf*/' --include='qf*/**' \
      --include='stego/' --include='stego/**' --exclude='*' \
      research-corpus-500/ \
      fishbowl:~/phantasm-eval/corpus-diversity-500/data/

# Push scripts.
rsync -av research-corpus-500/scripts/fishbowl/ \
      fishbowl:~/phantasm-eval/corpus-diversity-500/
```

## Run (on fishbowl)

```sh
cd ~/phantasm-eval/corpus-diversity-500/
python train_juniward_diversity500.py \
    --data-root ./data \
    --jin-srnet ~/phantasm-eval/checkpoints/JIN_SRNet.pt \
    --out-dir ./runs/juw_diversity500

python eval_juniward_diversity500.py \
    --checkpoint ./runs/juw_diversity500/juw_div500_best.pt \
    --u3-data ~/phantasm-eval/finetune-multi-juniward/data \
    --d500-data ./data \
    --out-json ./runs/juw_diversity500/five_way_summary.json
```

The eval JSON emits:

- `update3_198.stego_det@0.5`: J-UNIWARD detection on the original
  198-cover set (directly comparable to Update 3's 89.9% number).
- `diversity500_heldout.stego_det@0.5`: detection on the diversity-500
  held-out split (the **honest worst-case** number for the
  cover-robustness question).
- `verdict`: `corpus-robust`, `likely Picsum artifact`, or
  `inconclusive`, chosen by the rule below.

## Verdict rule (copy from team-lead brief)

- Within 5 pp of 89.9% (i.e. ≥ 84.9%): **corpus-robust** — Update 3's
  number generalizes.
- Substantially lower (< 80%): **Picsum artifact** — Update 3's number
  is inflated by overfitting to 22 seeds, and phantasm's true worst-case
  L1 detectability is meaningfully lower than reported.
