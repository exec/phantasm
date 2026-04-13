# pHash Overlap Analysis — Spike B Report

**Date:** 2026-04-12  
**Status:** DONE_WITH_CONCERNS  
**Spike directory:** `spikes/phash-overlap/`

---

## Corpus

- **Source:** Lorem Picsum (picsum.photos), CC0-licensed photos from Unsplash
- **Count:** 60 images
- **Processing:** Each image resized to 512x512 grayscale, re-saved as JPEG at QF=85
- **Block count per image:** 64x64 = 4,096 8x8 blocks
- **AC coefficients per image:** 4,096 x 63 = 258,048
- **Image variance range:** 102-7,928, median 3,374

---

## Methodology

### pHash computation

Standard 64-bit pHash implemented from scratch:
1. Resize to 32x32 grayscale (Lanczos)
2. 2D DCT (scipy.fft.dctn, ortho normalization)
3. Top-left 8x8 block = 64 coefficients
4. Median of 63 AC values (DC excluded from median)
5. Bit = 1 if coefficient > median

### Why single-block perturbation is the wrong framing

pHash operates on a 32x32 downsampled image. A 512x512 image has a 16:1 linear
downsampling factor, so each 32x32 output pixel averages a 16x16 input region
(4 8x8 JPEG blocks). A single-block DCT perturbation of +/-1 quantization step at
QF=85 contributes at most ~0.25 pixel-units to one downsampled pixel. The pHash
threshold margins average ~90 DCT units. Therefore, a single-block perturbation has
essentially zero probability of flipping a pHash bit in any real image.

### Cumulative sensitivity model (adopted)

A DCT position (u,v) is "hash-critical at stealth N%" if:
  n_blocks_to_flip(u,v) <= N% x total_blocks

where n_blocks_to_flip(u,v) is determined by binary search. This models the reality
that steganography embeds into many blocks simultaneously, not just one.

### Cost proxies

Two proxies (both reported, results identical):
- Variance proxy: cost = 1 / block_pixel_variance
- Gradient proxy: cost = 1 / mean_Sobel_gradient_magnitude

---

## Results

### Critical DCT position counts

| Metric | Value |
|--------|-------|
| Mean cumulative-critical positions per image | 11.3 / 63 |
| Median cumulative-critical positions per image | 0 / 63 |
| Max cumulative-critical positions | 63 / 63 (7 images) |
| Images with 0 cumulative-critical positions | 45 / 60 (75%) |

### Capacity penalty at each stealth budget

| Stealth budget | Mean | Median | p90 | Worst |
|----------------|------|--------|-----|-------|
| 1% | 7.4% | 0.0% | 4.0% | 100% |
| 5% | 7.5% | 0.0% | 6.8% | 100% |
| 10% | 8.4% | 0.0% | 14.0% | 100% |
| 20% | 9.5% | 0.0% | 40.6% | 100% |
| 30% | 10.6% | 0.0% | 49.7% | 100% |
| 50% | 10.6% | 0.0% | 49.7% | 100% |

(Gradient proxy results identical to variance proxy for all images.)

### Distribution at 10% stealth budget

- 75% of images: 0% penalty (pHash completely immune to embedding)
- 13.3% of images: >10% penalty
- 5% of images: 100% penalty (3 images)

### n_blocks_to_flip distribution

Across 60 images x 63 AC positions = 3,780 position-image pairs:
- 82.1%: never flip (requires more blocks than exist)
- 7.4%: flip with 1 block (degenerate near-threshold case)
- 10.5%: flip with 2-4,096 blocks

---

## Assessment: Is the 5-15% estimate supported?

**Verdict: The plan's 5-15% estimate is accidentally correct for the corpus mean
but for the wrong reason — the distribution is bimodal, not uniform.**

- 75% of images: ~0% cost. pHash is immune because JPEG DCT modifications at
  steganographic magnitudes are too localized to shift 32x32 downsampled image
  frequency content.
- ~10% of images: 50-100% cost. These have pHash bits sitting within one
  quantization step of the median threshold.
- The corpus mean of 8.4% at 10% stealth lands in the plan's 5-15% range, but
  this mean is driven by a pathological tail, not a consistent overhead.

---

## Limitations

1. Cost proxy, not J-UNIWARD. Real embedding distribution may differ.
2. Raster-order binary search, not embedding-priority order.
3. pHash only — PDQ is stricter and was not analyzed.
4. QF=85 only — lower QF may increase sensitivity.
5. 60-image corpus — tail behavior estimates may shift with larger corpus.
6. Binary search perturbs all blocks uniformly in one direction; real STC
   embedding uses +/-1 which may partially cancel.

---

## Recommendation

**Proceed with Phase 3 (hash-guard), but redesign the operating model:**

1. Add image pre-screening: compute pHash threshold margins before embedding.
   Images with any bit margin < ~50 DCT units are "sensitive" — require active
   hash-guard.

2. Non-sensitive images (75%): hash-guard can be a no-op. Capacity cost = 0%.
   Better than planned.

3. Sensitive images (~15-25%): coefficient exclusion alone may not work when
   n_blocks_to_flip=1. Hash-guard may need to implement hash-regeneration (slightly
   modify cover before embedding to move threshold margins to safety) rather than
   just reserving coefficients.

4. Analyze PDQ separately — do not assume pHash results transfer.

---

Raw data: `results.json`
