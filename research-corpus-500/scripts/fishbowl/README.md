# Fishbowl training scripts — corpus-diversity-500

Scripts that fine-tune JIN-SRNet on the 500-cover diversity corpus and
measure J-UNIWARD detection rate against three reference sets: (i) the
original eval-198 detection set, (ii) the diversity-500 held-out test
split, and (iii) Update 3's 198-cover 5-pass held-out split.

## Provenance

Forked from `~/phantasm-eval/multi/finetune_multi.py` (Update 3's
canonical training driver) and `~/phantasm-eval/multi/eval_five_way.py`.
Hyperparameters match Update 3 verbatim: Adam lr=1e-5, batch 16,
10 epochs, random 256×256 crop + flip, JIN-SRNet init. The only
differences vs Update 3's pipeline are:

- data root: `~/phantasm-eval/corpus-diversity-500/data/` (500 covers,
  5 passphrases) instead of `~/phantasm-eval/multi/` (198 covers),
- split: proportional scaling of Update 3's 3/3/16-of-22 shape —
  68 test / 68 val / 364 train seeds,
- target: J-UNIWARD only (UERD not part of this follow-up).

Update 3's on-fishbowl artifacts are not touched.

## Deploy (one-time)

The data layout on fishbowl is `cover/` + `juniward_p{0..4}/` flat
directories (same shape as `multi/`). Local file naming is
`{NNNN}.jpg`; rename on rsync to `d500_{NNNN}.jpg` so the canonical
`_(\d{4})\.jpg$` seed regex works.

```sh
# from the phantasm repo root, after embed_multipass.sh has produced
# research-corpus-500/stego/ml-multi-pass-{0..4}/qf85/720/*.jpg:

ssh fishbowl 'mkdir -p ~/phantasm-eval/corpus-diversity-500/data/{cover,juniward_p0,juniward_p1,juniward_p2,juniward_p3,juniward_p4}'

# stage with canonical naming
rm -rf /tmp/d500-stage && mkdir -p /tmp/d500-stage/{cover,juniward_p0,juniward_p1,juniward_p2,juniward_p3,juniward_p4}
for f in research-corpus-500/qf85/720/[0-9]*.jpg; do
    ln "$PWD/$f" "/tmp/d500-stage/cover/d500_$(basename "$f")"
done
for p in 0 1 2 3 4; do
    for f in research-corpus-500/stego/ml-multi-pass-$p/qf85/720/[0-9]*.jpg; do
        ln "$f" "/tmp/d500-stage/juniward_p$p/d500_$(basename "$f")"
    done
done

rsync -av /tmp/d500-stage/ fishbowl:~/phantasm-eval/corpus-diversity-500/data/
rsync -av research-corpus-500/scripts/fishbowl/ fishbowl:~/phantasm-eval/corpus-diversity-500/
```

## Run (on fishbowl)

```sh
ssh fishbowl
source ~/phantasm-eval/venv/bin/activate
cd ~/phantasm-eval/corpus-diversity-500

# train: ~4 minutes on RTX 5070 (10 epochs × ~24 s + baseline/final eval)
python -u finetune_d500.py runs/juw_d500_v1

# cross-eval against the three reference sets
python eval_cross.py
```

Outputs:

- `runs/juw_d500_v1/juniward_d500_best.pt` — best-val checkpoint
- `runs/juw_d500_v1/history.json` — per-epoch loss + val/test metrics
- `runs/juw_d500_v1/cross_eval.json` — the headline numbers: J-UW
  detection on all three comparison sets, delta vs 89.9%, and verdict

## Verdict rule

Update 3's reference number is **89.9%** J-UW detection on the
eval-198 set. The primary comparable number from this pipeline is the
d500 model's detection rate on the d500 held-out split.

- Delta ≥ −5 pp (i.e. ≥ 84.9%): **corpus-robust** — Update 3's
  number is not a Picsum cover-pool artifact.
- Delta < −10 pp: **likely Picsum artifact** — Update 3's number is
  inflated by overfitting to the small cover pool.
- Between: **inconclusive**.

## Observed result (2026-04-18)

**Verdict: corpus-robust (+6.9 pp).** The d500 model detects phantasm
J-UW at **96.8%** on the d500 held-out (68 seeds × 5 passphrases = 340
samples), **97.5%** on eval-198, and **99.3%** on Update 3's 198-cover
held-out split. Cover FP rate is 0/n across all three evals. More
unique covers produce a *stronger* detector, not a weaker one — the
89.9% number was not a small-pool overfit and is already an
underestimate of worst-case L1 detectability at this training regime.
