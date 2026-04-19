"""eval_juniward_diversity500.py — measure the diversity-500 J-UW
checkpoint's detection rate on (i) the original 198-cover set from
Update 3 and (ii) the diversity-500 held-out test split.

The output JSON is the input to the verdict rule:
  Update 3 J-UW detection was 89.9% on the 198-cover set.
  - within 5 pp  (>= 84.9%)  --> corpus-robust
  - substantially lower      --> Picsum cover-pool artifact

Usage:
  python eval_juniward_diversity500.py \\
      --checkpoint ./runs/juw_diversity500/juw_div500_best.pt \\
      --u3-data ~/phantasm-eval/finetune-multi-juniward/data \\
      --d500-data ./data \\
      --out-json ./runs/juw_diversity500/five_way_summary.json
"""

from __future__ import annotations

import argparse
import json
import sys
from dataclasses import dataclass
from pathlib import Path

import torch
from torch.utils.data import DataLoader

from train_juniward_diversity500 import (
    PairedCoverStegoDataset,
    _import_jin_srnet,
    build_pairs as build_d500_pairs,
    measure_det_rate,
    seed_split,
)


@dataclass(frozen=True)
class Pair198:
    cover_path: Path
    stego_path: Path
    passphrase: str
    seed_num: int
    label: int


def build_u3_pairs(data_root: Path) -> list[Pair198]:
    """Build (cover, stego) pairs for the Update-3 198-cover matrix.
    Layout on fishbowl:
      data_root/qf{75,85,90}/{512,720,1024}/<NNNN>.jpg           covers
      data_root/stego/ml-multi-pass-<0..4>/qf*/*/<NNNN>.jpg      stegos
    """
    pairs: list[Pair198] = []
    for qf_dir in sorted(data_root.glob("qf*")):
        for dim_dir in sorted(qf_dir.glob("*")):
            if not dim_dir.is_dir():
                continue
            for cover_path in sorted(dim_dir.glob("[0-9]*.jpg")):
                bucket_id = int(cover_path.stem)
                for pass_idx in range(5):
                    passphrase = f"ml-multi-pass-{pass_idx}"
                    stego_path = (
                        data_root / "stego" / passphrase
                        / qf_dir.name / dim_dir.name / cover_path.name
                    )
                    if not stego_path.exists():
                        continue
                    pairs.append(
                        Pair198(
                            cover_path=cover_path,
                            stego_path=stego_path,
                            passphrase=passphrase,
                            seed_num=bucket_id,
                            label=1,
                        )
                    )
    return pairs


def load_model(checkpoint_path: Path, device: torch.device):
    JinSRNet = _import_jin_srnet()
    model = JinSRNet().to(device)
    state = torch.load(checkpoint_path, map_location=device)
    if isinstance(state, dict) and "state_dict" in state:
        state = state["state_dict"]
    model.load_state_dict(state, strict=False)
    model.eval()
    return model


def main(args: argparse.Namespace) -> None:
    device = torch.device("cuda" if torch.cuda.is_available() else "cpu")
    print(f"Device: {device}")

    model = load_model(Path(args.checkpoint), device)
    print(f"Loaded checkpoint: {args.checkpoint}")

    results = {"checkpoint": str(args.checkpoint)}

    # --- Update-3 198-cover set ---
    if args.u3_data:
        data_u3 = Path(args.u3_data)
        if not data_u3.exists():
            print(f"WARN: u3 data missing at {data_u3}, skipping", file=sys.stderr)
        else:
            pairs_u3 = build_u3_pairs(data_u3)
            print(f"u3 set: {len(pairs_u3)} pairs")
            loader_u3 = DataLoader(
                PairedCoverStegoDataset(pairs_u3, augment=False),
                batch_size=args.batch_size,
                shuffle=False,
                num_workers=args.workers,
                pin_memory=True,
            )
            metrics_u3 = measure_det_rate(model, loader_u3, device)
            print(f"u3 metrics: {metrics_u3}")
            results["update3_198"] = metrics_u3

    # --- diversity-500 held-out test split ---
    data_d500 = Path(args.d500_data)
    pairs_d500 = build_d500_pairs(data_d500)
    _, _, test_d500, seed_triple = seed_split(pairs_d500, seed=args.split_seed)
    train_seeds, val_seeds, test_seeds = seed_triple
    print(
        f"diversity-500 split: train {len(train_seeds)} val {len(val_seeds)} test {len(test_seeds)} seeds"
    )
    loader_d500 = DataLoader(
        PairedCoverStegoDataset(test_d500, augment=False),
        batch_size=args.batch_size,
        shuffle=False,
        num_workers=args.workers,
        pin_memory=True,
    )
    metrics_d500 = measure_det_rate(model, loader_d500, device)
    print(f"diversity-500 held-out metrics: {metrics_d500}")
    results["diversity500_heldout"] = metrics_d500
    results["diversity500_seeds"] = {
        "train": train_seeds,
        "val": val_seeds,
        "test": test_seeds,
    }

    # --- verdict ---
    UPDATE3_NUMBER = 0.899
    if "update3_198" in results:
        results["delta_vs_update3_on198"] = (
            results["update3_198"]["stego_det@0.5"] - UPDATE3_NUMBER
        )
    delta_500 = results["diversity500_heldout"]["stego_det@0.5"] - UPDATE3_NUMBER
    results["delta_vs_update3_on_heldout"] = delta_500
    if delta_500 >= -0.05:
        verdict = "corpus-robust"
    elif delta_500 < -0.10:
        verdict = "likely Picsum artifact"
    else:
        verdict = "inconclusive"
    results["verdict"] = verdict
    print(f"\nVerdict: {verdict} (delta vs 89.9% = {delta_500*100:+.1f} pp on held-out)")

    out_path = Path(args.out_json)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(results, indent=2))
    print(f"Wrote results to {out_path}")


def build_argparser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--checkpoint", required=True)
    p.add_argument("--u3-data", default=None, help="Path to Update 3's data/ dir on fishbowl")
    p.add_argument("--d500-data", required=True, help="Path to diversity-500 data/ dir")
    p.add_argument("--out-json", required=True)
    p.add_argument("--batch-size", type=int, default=32)
    p.add_argument("--workers", type=int, default=4)
    p.add_argument("--split-seed", type=int, default=0)
    return p


if __name__ == "__main__":
    main(build_argparser().parse_args())
