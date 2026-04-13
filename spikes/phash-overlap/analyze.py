#!/usr/bin/env python3
"""
pHash overlap analysis spike for Phantasm.

Measures the capacity penalty imposed by pHash preservation on content-adaptive
JPEG DCT embedding. See REPORT.md for methodology and findings.

Key finding (see REPORT.md): pHash operates on a 32x32 downsampled image.
Single-coefficient perturbations at QF=85 contribute <0.25 units to the
32x32 downsampled pixel values, while pHash threshold margins average ~90 DCT
units. The hash-critical overlap question therefore has two regimes:
  (a) Single-coefficient view: effectively 0% overlap
  (b) Cumulative embedding view: low-frequency AC positions accumulate
      across many blocks and eventually could shift pHash — measured here
"""

import argparse
import io
import json
import os
import sys
import tempfile
import warnings
from pathlib import Path

import numpy as np
from PIL import Image
from scipy.fft import dctn, idctn
from scipy.ndimage import sobel

warnings.filterwarnings("ignore")

# ── QF85 quantization table ─────────────────────────────────────────────────

JPEG_LUMINANCE_Q = np.array([
    [16, 11, 10, 16,  24,  40,  51,  61],
    [12, 12, 14, 19,  26,  58,  60,  55],
    [14, 13, 16, 24,  40,  57,  69,  56],
    [14, 17, 22, 29,  51,  87,  80,  62],
    [18, 22, 37, 56,  68, 109, 103,  77],
    [24, 35, 55, 64,  81, 104, 113,  92],
    [49, 64, 78, 87, 103, 121, 120, 101],
    [72, 92, 95, 98, 112, 100, 103,  99],
], dtype=np.float64)

_scale = 200 - 2 * 85  # = 30 for QF=85
QT = np.floor((JPEG_LUMINANCE_Q * _scale + 50) / 100).clip(1, 255)

# ── pHash ───────────────────────────────────────────────────────────────────

def compute_phash(img: Image.Image) -> tuple[np.ndarray, np.ndarray, float]:
    """
    Standard 64-bit pHash.
    Returns (bits[64], dct_vals[64], median).
    """
    small = img.convert("L").resize((32, 32), Image.LANCZOS)
    arr = np.array(small, dtype=np.float64)
    d = dctn(arr, norm="ortho")
    block = d[:8, :8].flatten()       # 64 coefficients
    median = float(np.median(block[1:]))  # median of 63 AC values
    bits = block > median
    return bits, block, median


def phash_bits(img: Image.Image) -> np.ndarray:
    bits, _, _ = compute_phash(img)
    return bits


def phash_distance(a: np.ndarray, b: np.ndarray) -> int:
    return int(np.sum(a != b))


# ── Image loading and DCT ───────────────────────────────────────────────────

def normalize_image(img_path: Path, size: int = 512) -> Path:
    """Re-save image as QF=85 JPEG at `size x size` to a temp file."""
    orig = Image.open(img_path).convert("L")
    orig = orig.resize((size, size), Image.LANCZOS)
    buf = io.BytesIO()
    orig.save(buf, format="JPEG", quality=85)
    buf.seek(0)
    with tempfile.NamedTemporaryFile(suffix=".jpg", delete=False) as tf:
        tf.write(buf.getvalue())
        return Path(tf.name)


def load_blocks(path: Path) -> tuple[np.ndarray, int, int]:
    """Load JPEG, return pixel blocks (nrows, ncols, 8, 8) and dims."""
    img = Image.open(path).convert("L")
    w, h = img.size
    w8, h8 = (w // 8) * 8, (h // 8) * 8
    img = img.crop((0, 0, w8, h8))
    arr = np.array(img, dtype=np.float64)
    nrows, ncols = h8 // 8, w8 // 8
    blocks = arr.reshape(nrows, 8, ncols, 8).transpose(0, 2, 1, 3)
    return blocks, nrows, ncols


def blocks_to_dct(blocks: np.ndarray) -> np.ndarray:
    nrows, ncols = blocks.shape[:2]
    d = np.zeros_like(blocks)
    for i in range(nrows):
        for j in range(ncols):
            d[i, j] = dctn(blocks[i, j] - 128.0, norm="ortho")
    return d


def dct_to_pixels(dct_blocks: np.ndarray) -> np.ndarray:
    nrows, ncols = dct_blocks.shape[:2]
    arr = np.zeros((nrows * 8, ncols * 8), dtype=np.float64)
    for i in range(nrows):
        for j in range(ncols):
            arr[i*8:(i+1)*8, j*8:(j+1)*8] = idctn(dct_blocks[i, j], norm="ortho") + 128.0
    return arr.clip(0.0, 255.0)


# ── Sensitivity analysis ─────────────────────────────────────────────────────

def compute_phash_sensitivity(norm_path: Path, dct_blocks: np.ndarray) -> dict:
    """
    For each of the 63 AC DCT positions (u,v), compute:
      1. single_critical: does perturbing ONE representative block flip any pHash bit?
      2. cumulative_critical: does perturbing ALL blocks at that position flip any pHash bit?
      3. n_blocks_to_flip: minimum number of blocks (perturbed together, uniformly)
         that causes any pHash bit to flip. Binary search over [1, n_blocks].
      4. margin_ratio: for each AC position, estimated ratio of (cumulative perturbation
         needed to flip closest pHash bit) / (perturbation from full-image embedding).

    Returns dict with per-position arrays.
    """
    img_orig = Image.open(norm_path).convert("L")
    orig_bits, orig_dct_vals, orig_median = compute_phash(img_orig)

    nrows, ncols = dct_blocks.shape[:2]
    n_blocks = nrows * ncols

    ac_positions = [(u, v) for u in range(8) for v in range(8) if not (u == 0 and v == 0)]

    results = {
        "single_critical": {},
        "cumulative_critical": {},
        "n_blocks_to_flip": {},
        "margin_to_perturbation_ratio": {},
    }

    for u, v in ac_positions:
        key = f"{u},{v}"
        perturbation = QT[u, v]

        # 1. Single block (center of image)
        bi, bj = nrows // 2, ncols // 2
        mod_single = dct_blocks.copy()
        mod_single[bi, bj, u, v] += perturbation
        pix = dct_to_pixels(mod_single)
        single_bits = phash_bits(Image.fromarray(pix.astype(np.uint8), "L"))
        results["single_critical"][key] = bool(phash_distance(orig_bits, single_bits) >= 1)

        # 2. All blocks perturbed
        mod_all = dct_blocks.copy()
        mod_all[:, :, u, v] += perturbation
        pix_all = dct_to_pixels(mod_all)
        all_bits = phash_bits(Image.fromarray(pix_all.astype(np.uint8), "L"))
        results["cumulative_critical"][key] = bool(phash_distance(orig_bits, all_bits) >= 1)

        # 3. Binary search: minimum fraction of blocks to flip pHash
        # Perturb a fraction f of randomly-distributed blocks
        lo, hi = 1, n_blocks
        if not results["cumulative_critical"][key]:
            # Even all blocks don't flip pHash
            results["n_blocks_to_flip"][key] = n_blocks + 1  # "never"
        else:
            while lo < hi:
                mid = (lo + hi) // 2
                # Perturb 'mid' blocks uniformly spaced
                step = max(1, n_blocks // mid)
                block_indices = [(i, j) for i in range(0, nrows, step // ncols + 1)
                                 for j in range(0, ncols, max(1, step % ncols + 1))][:mid]
                # Simpler: perturb first 'mid' blocks in raster order
                mod = dct_blocks.copy()
                count = 0
                for bi2 in range(nrows):
                    for bj2 in range(ncols):
                        if count >= mid:
                            break
                        mod[bi2, bj2, u, v] += perturbation
                        count += 1
                    if count >= mid:
                        break
                pix_mid = dct_to_pixels(mod)
                mid_bits = phash_bits(Image.fromarray(pix_mid.astype(np.uint8), "L"))
                if phash_distance(orig_bits, mid_bits) >= 1:
                    hi = mid
                else:
                    lo = mid + 1
            results["n_blocks_to_flip"][key] = lo

        # 4. Margin ratio: how much accumulated perturbation vs. available margin
        # Estimate: each block contributes perturbation / (16*16) to one 32x32 pixel.
        # The nearest pHash margin determines how much is needed.
        # margin_ratio = needed_pixel_change_in_32x32 / max_available
        # min_margin is the smallest distance-to-threshold across all 64 pHash bits
        min_margin = float(np.abs(orig_dct_vals - orig_median).min())
        # Contribution per block to 32x32-image DCT (rough estimate):
        # Each 8x8 block maps to ~0.25 pixels in 32x32; perturbation sum in block
        # is qt[u,v] * |IDCT_basis_sum|
        delta = np.zeros((8, 8))
        delta[u, v] = 1.0
        basis_sum = float(abs(idctn(delta, norm="ortho")).sum())
        per_block_32x32_contribution = perturbation * basis_sum / 256.0  # 16x16 averaging
        total_contribution = per_block_32x32_contribution * n_blocks
        # Approximate effect on pHash DCT (32x32 DCT amplification factor ~1 for low freq)
        results["margin_to_perturbation_ratio"][key] = (
            min_margin / (total_contribution + 1e-9)
        )

    return results


# ── Cost proxies ─────────────────────────────────────────────────────────────

def block_variance(blocks: np.ndarray) -> np.ndarray:
    return blocks.var(axis=(2, 3))  # (nrows, ncols)


def block_gradient(blocks: np.ndarray) -> np.ndarray:
    nrows, ncols = blocks.shape[:2]
    g = np.zeros((nrows, ncols), dtype=np.float64)
    for i in range(nrows):
        for j in range(ncols):
            b = blocks[i, j]
            sx = sobel(b, axis=1)
            sy = sobel(b, axis=0)
            g[i, j] = np.sqrt(sx**2 + sy**2).mean()
    return g


def cost_from_variance(var: np.ndarray) -> np.ndarray:
    c = 1.0 / (var + 1e-6)
    return (c - c.min()) / (c.max() - c.min() + 1e-12)


def cost_from_gradient(grad: np.ndarray) -> np.ndarray:
    c = 1.0 / (grad + 1e-6)
    return (c - c.min()) / (c.max() - c.min() + 1e-12)


# ── Capacity penalty calculation ─────────────────────────────────────────────

def compute_capacity_penalties(
    sensitivity: dict,
    cost_var: np.ndarray,
    cost_grad: np.ndarray,
    nrows: int,
    ncols: int,
    stealth_percentages: list[float],
    n_blocks: int,
) -> tuple[dict, dict]:
    """
    For each stealth%, determine which blocks are in the cheap pool.
    Then determine: of those blocks, what fraction embed into "hash-critical"
    positions (positions where the CUMULATIVE effect across the stealth pool
    could flip a pHash bit)?

    A position (u,v) is "hash-critical at stealth N%":
      n_blocks_to_flip[u,v] <= N% * total_blocks
      i.e., using N% of blocks at that position would flip pHash.
    """
    ac_positions = [(u, v) for u in range(8) for v in range(8) if not (u == 0 and v == 0)]
    n_ac = len(ac_positions)  # 63
    total_coeffs = nrows * ncols * n_ac

    # Per-AC-position: is it hash-critical at each stealth%?
    # cumulative_critical = True means even using ALL blocks at this position flips pHash
    # n_blocks_to_flip = minimum number of blocks needed to flip pHash

    penalty_var = {}
    penalty_grad = {}

    # Also compute "strict" penalty: positions where even single-block embedding flips hash
    # And "loose" penalty: positions where full corpus embedding flips hash

    cost_var_flat = np.repeat(cost_var.flatten(), n_ac)
    cost_grad_flat = np.repeat(cost_grad.flatten(), n_ac)

    for pct in stealth_percentages:
        pct_str = str(pct)
        n_embedding = max(1, int(total_coeffs * pct / 100))
        n_embedding_blocks = max(1, int(n_blocks * pct / 100))

        # Which AC positions are "hash-critical" at this stealth budget?
        # A position is critical if n_blocks_to_flip <= n_embedding_blocks
        critical_at_pct = np.zeros(n_ac, dtype=bool)
        for idx, (u, v) in enumerate(ac_positions):
            key = f"{u},{v}"
            ntf = sensitivity["n_blocks_to_flip"].get(key, n_blocks + 1)
            if ntf <= n_embedding_blocks:
                critical_at_pct[idx] = True

        critical_flat = np.tile(critical_at_pct, nrows * ncols)

        # Variance proxy: cheapest n_embedding coefficients
        idx_var = np.argsort(cost_var_flat)[:n_embedding]
        penalty_var[pct_str] = float(critical_flat[idx_var].mean())

        # Gradient proxy
        idx_grad = np.argsort(cost_grad_flat)[:n_embedding]
        penalty_grad[pct_str] = float(critical_flat[idx_grad].mean())

    return penalty_var, penalty_grad


# ── Per-image analysis ────────────────────────────────────────────────────────

def analyze_image(
    img_path: Path,
    stealth_percentages: list[float],
) -> dict:
    """Full pipeline for one image."""
    norm_path = normalize_image(img_path)
    try:
        blocks, nrows, ncols = load_blocks(norm_path)
        dct_blocks_arr = blocks_to_dct(blocks)
        n_blocks = nrows * ncols

        var = block_variance(blocks)
        grad = block_gradient(blocks)
        cost_var = cost_from_variance(var)
        cost_grad = cost_from_gradient(grad)

        sensitivity = compute_phash_sensitivity(norm_path, dct_blocks_arr)

        n_cumulative_critical = sum(
            1 for v in sensitivity["cumulative_critical"].values() if v
        )
        n_single_critical = sum(
            1 for v in sensitivity["single_critical"].values() if v
        )

        penalty_var, penalty_grad = compute_capacity_penalties(
            sensitivity, cost_var, cost_grad,
            nrows, ncols, stealth_percentages, n_blocks,
        )

        # Variance of image (proxy for texture complexity)
        img_variance = float(blocks.var())

    finally:
        os.unlink(norm_path)

    return {
        "path": img_path.name,
        "nrows": nrows,
        "ncols": ncols,
        "n_blocks": int(n_blocks),
        "img_variance": img_variance,
        "n_single_critical_positions": int(n_single_critical),
        "n_cumulative_critical_positions": int(n_cumulative_critical),
        "n_blocks_to_flip_by_position": sensitivity["n_blocks_to_flip"],
        "margin_to_perturbation_ratio": sensitivity["margin_to_perturbation_ratio"],
        "capacity_penalty_var": penalty_var,
        "capacity_penalty_grad": penalty_grad,
    }


# ── Main ──────────────────────────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser(description="pHash overlap analysis for Phantasm")
    parser.add_argument("--corpus", default="corpus")
    parser.add_argument("--output", default="results.json")
    parser.add_argument("--stealth-percentages", default="1,5,10,20,30,50")
    parser.add_argument("--limit", type=int, default=None)
    args = parser.parse_args()

    stealth_pcts = [float(x) for x in args.stealth_percentages.split(",")]
    corpus_path = Path(args.corpus)
    images = sorted(
        p for p in list(corpus_path.glob("*.jpg")) + list(corpus_path.glob("*.jpeg"))
        if not p.name.startswith("_norm_")
    )
    if args.limit:
        images = images[:args.limit]

    if not images:
        print(f"ERROR: No JPEG images found in {corpus_path}", file=sys.stderr)
        sys.exit(1)

    print(f"Analyzing {len(images)} images at stealth budgets {stealth_pcts}%")
    all_results = []

    for idx, img_path in enumerate(images, 1):
        print(f"  [{idx:3d}/{len(images)}] {img_path.name}", end=" ", flush=True)
        try:
            result = analyze_image(img_path, stealth_pcts)
            all_results.append(result)
            p10_var = result["capacity_penalty_var"].get("10.0", float("nan"))
            p10_grad = result["capacity_penalty_grad"].get("10.0", float("nan"))
            print(
                f"— cumulative_crit={result['n_cumulative_critical_positions']}/63  "
                f"10% penalty(var)={p10_var:.1%} (grad)={p10_grad:.1%}"
            )
        except Exception as e:
            import traceback
            print(f"FAILED: {e}")
            traceback.print_exc()

    # Aggregate
    aggregate = {
        "stealth_percentages": stealth_pcts,
        "n_images": len(all_results),
        "per_image": all_results,
        "summary": {},
    }

    for pct in stealth_pcts:
        pct_str = str(pct)
        for proxy in ("var", "grad"):
            key_in = f"capacity_penalty_{proxy}"
            key_out = f"pct_{pct_str}_{proxy}"
            vals = [r[key_in][pct_str] for r in all_results if pct_str in r[key_in]]
            if not vals:
                continue
            arr = np.array(vals)
            aggregate["summary"][key_out] = {
                "mean": float(arr.mean()),
                "median": float(np.median(arr)),
                "p90": float(np.percentile(arr, 90)),
                "p10": float(np.percentile(arr, 10)),
                "min": float(arr.min()),
                "max": float(arr.max()),
                "n": len(arr),
            }

    n_cumul = [r["n_cumulative_critical_positions"] for r in all_results]
    n_single = [r["n_single_critical_positions"] for r in all_results]
    aggregate["summary"]["critical_positions"] = {
        "cumulative_mean": float(np.mean(n_cumul)),
        "cumulative_median": float(np.median(n_cumul)),
        "cumulative_max": int(max(n_cumul)),
        "single_mean": float(np.mean(n_single)),
        "single_median": float(np.median(n_single)),
        "single_max": int(max(n_single)),
        "out_of_63_ac": 63,
    }

    # Margin ratios
    all_ratios = []
    for r in all_results:
        all_ratios.extend(r["margin_to_perturbation_ratio"].values())
    aggregate["summary"]["margin_to_perturbation_ratio"] = {
        "mean": float(np.mean(all_ratios)),
        "median": float(np.median(all_ratios)),
        "min": float(np.min(all_ratios)),
        "p10": float(np.percentile(all_ratios, 10)),
    }

    output_path = Path(args.output)
    with open(output_path, "w") as f:
        json.dump(aggregate, f, indent=2)

    print(f"\nResults written to {output_path}")
    print("\n=== SUMMARY ===")
    print(f"Images analyzed: {len(all_results)}")
    print(f"Cumulative-critical AC positions: median={np.median(n_cumul):.0f}/63, max={max(n_cumul)}/63")
    print(f"Single-critical AC positions: median={np.median(n_single):.0f}/63, max={max(n_single)}/63")
    print()
    for pct in stealth_pcts:
        pct_str = str(pct)
        v = aggregate["summary"].get(f"pct_{pct_str}_var", {})
        g = aggregate["summary"].get(f"pct_{pct_str}_grad", {})
        print(
            f"  Stealth {pct:5.1f}%: "
            f"penalty(var) median={v.get('median', 0):.1%} p90={v.get('p90', 0):.1%} | "
            f"penalty(grad) median={g.get('median', 0):.1%} p90={g.get('p90', 0):.1%}"
        )


if __name__ == "__main__":
    main()
