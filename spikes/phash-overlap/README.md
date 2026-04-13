# pHash Overlap Analysis Spike

Empirically measures the capacity penalty imposed by pHash preservation on
content-adaptive JPEG DCT embedding. Part of the Phantasm pre-implementation
spike series (Phase -1, Spike B).

## Setup

```bash
cd spikes/phash-overlap
python3 -m venv venv
venv/bin/pip install -r requirements.txt
```

## Corpus

Download 50–100 JPEG photos into `corpus/`. The run that produced `results.json`
used 60 images downloaded from https://picsum.photos (Lorem Picsum, CC0-licensed
photos from Unsplash). Images were resized to 512×512 and re-saved at QF=85 by
the analysis script.

The `corpus/` directory is excluded from git (see `.gitignore`).

## Running

```bash
venv/bin/python analyze.py \
  --corpus corpus \
  --output results.json \
  [--stealth-percentages 1,5,10,20,30,50] \
  [--limit N]   # limit to first N images, useful for testing
```

Output is written to `results.json` (raw per-image numbers + aggregate summary).

## What it measures

For each image:

1. Normalizes to 512×512 grayscale JPEG at QF=85.
2. Splits into 8×8 pixel blocks, computes JPEG DCT for each block.
3. Computes pHash (32×32 downsample → 2D DCT → top-left 8×8 block → median threshold).
4. For each of the 63 AC DCT positions (u,v), determines whether embedding into
   that position across a stealth budget of N% of all blocks would flip any pHash bit.
   This is the "cumulative" sensitivity model (see REPORT.md for why single-block
   perturbation is the wrong question to ask).
5. For each stealth budget N%, computes the fraction of cheap-to-embed coefficients
   (ranked by local variance and gradient proxies) that are pHash-critical.

See REPORT.md for findings.
