"""train_juniward_diversity500.py — replicate the Update 3 J-UNIWARD
multi-pass fine-tune recipe on the diversity-500 cover corpus.

Hyperparameters match Update 3 verbatim:
- optimizer: Adam, lr = 1e-5
- epochs: 10
- augmentation: random 256x256 crop + random horizontal flip
- init: JIN-SRNet checkpoint
- batch size: 32
- training pairs: every (cover, stego) pair at each of the 5 passphrases

Corpus layout expected under --data-root:
  qf85/720/0001.jpg ... 0500.jpg           (covers)
  stego/ml-multi-pass-0/qf85/720/*.jpg     (stegos, pass 0)
  stego/ml-multi-pass-1/qf85/720/*.jpg
  stego/ml-multi-pass-2/qf85/720/*.jpg
  stego/ml-multi-pass-3/qf85/720/*.jpg
  stego/ml-multi-pass-4/qf85/720/*.jpg

Seed-level split: 400 train / 50 val / 50 test (proportional to the
16/3/3 seed split used in Update 3, scaled 25x by cover count, giving
2500 train / 250 val / 250 test stegos + equal covers).

Usage:
  python train_juniward_diversity500.py \\
      --data-root /path/to/data \\
      --jin-srnet /path/to/JIN_SRNet.pt \\
      --out-dir ./runs/juw_diversity500
"""

from __future__ import annotations

import argparse
import json
import random
import sys
from dataclasses import dataclass
from pathlib import Path

import numpy as np
import torch
import torch.nn as nn
import torch.nn.functional as F
from PIL import Image
from torch.utils.data import DataLoader, Dataset


# ----------------------------------------------------------------------
# JIN-SRNet architecture (imported from the Update 3 directory on
# fishbowl — matches the implementation already vetted there).
# ----------------------------------------------------------------------


def _import_jin_srnet():
    """JIN-SRNet module is defined in the existing Update 3 directory on
    fishbowl. We add that directory to sys.path and reuse the definition
    so this script is a drop-in fork of the Update 3 pipeline."""
    update3_dir = Path.home() / "phantasm-eval" / "finetune-multi-juniward"
    if update3_dir.exists():
        sys.path.insert(0, str(update3_dir))
    try:
        from jin_srnet import JinSRNet  # type: ignore

        return JinSRNet
    except ImportError as err:
        print(
            f"ERROR: could not import JinSRNet from {update3_dir}. "
            f"Verify the Update 3 training directory is intact: {err}",
            file=sys.stderr,
        )
        raise


# ----------------------------------------------------------------------
# Dataset
# ----------------------------------------------------------------------


@dataclass(frozen=True)
class CoverStegoPair:
    cover_path: Path
    stego_path: Path
    passphrase: str
    seed_num: int
    label: int  # 0 = cover, 1 = stego (assigned per sample at __getitem__)


class PairedCoverStegoDataset(Dataset):
    """Each item is either a cover or a stego drawn from the flat index.
    We emit balanced pairs so an epoch sees every (seed, passphrase)
    stego and its paired cover exactly once."""

    def __init__(self, pairs: list[CoverStegoPair], augment: bool):
        self.pairs = pairs
        self.augment = augment

    def __len__(self) -> int:
        # Two samples per pair (cover + stego).
        return 2 * len(self.pairs)

    def __getitem__(self, idx: int):
        pair_idx = idx // 2
        is_stego = idx % 2 == 1
        pair = self.pairs[pair_idx]
        path = pair.stego_path if is_stego else pair.cover_path
        label = 1 if is_stego else 0

        img = Image.open(path).convert("RGB")
        arr = np.asarray(img, dtype=np.float32) / 255.0  # H W C, match Update 3 preprocessing

        if self.augment:
            h, w, _ = arr.shape
            crop = 256
            if h >= crop and w >= crop:
                top = random.randint(0, h - crop)
                left = random.randint(0, w - crop)
                arr = arr[top : top + crop, left : left + crop, :]
            else:
                arr = arr[:crop, :crop, :]
            if random.random() < 0.5:
                arr = arr[:, ::-1, :].copy()
        else:
            h, w, _ = arr.shape
            crop = 256
            top = max(0, (h - crop) // 2)
            left = max(0, (w - crop) // 2)
            arr = arr[top : top + crop, left : left + crop, :]

        tensor = torch.from_numpy(arr).permute(2, 0, 1).contiguous()  # C H W
        return tensor, torch.tensor(label, dtype=torch.long)


def build_pairs(data_root: Path) -> list[CoverStegoPair]:
    cover_dir = data_root / "qf85" / "720"
    if not cover_dir.exists():
        raise FileNotFoundError(f"cover dir missing: {cover_dir}")

    covers = sorted(cover_dir.glob("[0-9]*.jpg"))
    pairs: list[CoverStegoPair] = []

    for cover_path in covers:
        seed_num = int(cover_path.stem)
        for pass_idx in range(5):
            passphrase = f"ml-multi-pass-{pass_idx}"
            stego_path = (
                data_root
                / "stego"
                / passphrase
                / "qf85"
                / "720"
                / cover_path.name
            )
            if not stego_path.exists():
                continue
            pairs.append(
                CoverStegoPair(
                    cover_path=cover_path,
                    stego_path=stego_path,
                    passphrase=passphrase,
                    seed_num=seed_num,
                    label=1,
                )
            )
    return pairs


def seed_split(pairs: list[CoverStegoPair], seed: int = 0) -> tuple[list, list, list, tuple]:
    """Seed-level train/val/test split proportional to Update 3's 16/3/3
    of 22 seeds."""
    rng = random.Random(seed)
    all_seeds = sorted({p.seed_num for p in pairs})
    rng.shuffle(all_seeds)
    n = len(all_seeds)
    n_train = round(n * 16 / 22)
    n_val = round(n * 3 / 22)
    train_seeds = set(all_seeds[:n_train])
    val_seeds = set(all_seeds[n_train : n_train + n_val])
    test_seeds = set(all_seeds[n_train + n_val :])

    train = [p for p in pairs if p.seed_num in train_seeds]
    val = [p for p in pairs if p.seed_num in val_seeds]
    test = [p for p in pairs if p.seed_num in test_seeds]
    return train, val, test, (sorted(train_seeds), sorted(val_seeds), sorted(test_seeds))


# ----------------------------------------------------------------------
# Training / measurement loop
# ----------------------------------------------------------------------


def measure_det_rate(model: nn.Module, loader: DataLoader, device: torch.device) -> dict:
    """Run the model in inference mode and return detection / false-positive
    rates at the standard P(stego) > 0.5 threshold."""
    model.eval()
    all_p_stego: list[float] = []
    all_labels: list[int] = []
    with torch.no_grad():
        for imgs, labels in loader:
            imgs = imgs.to(device, non_blocking=True)
            logits = model(imgs)
            p_stego = F.softmax(logits, dim=1)[:, 1].cpu().numpy()
            all_p_stego.extend(p_stego.tolist())
            all_labels.extend(labels.numpy().tolist())

    arr_p = np.asarray(all_p_stego)
    arr_y = np.asarray(all_labels)
    stego_mask = arr_y == 1
    cover_mask = arr_y == 0
    stego_det = float((arr_p[stego_mask] > 0.5).mean()) if stego_mask.any() else float("nan")
    cover_fp = float((arr_p[cover_mask] > 0.5).mean()) if cover_mask.any() else float("nan")
    return {
        "stego_det@0.5": stego_det,
        "cover_fp@0.5": cover_fp,
        "n_stego": int(stego_mask.sum()),
        "n_cover": int(cover_mask.sum()),
    }


def train(args: argparse.Namespace) -> None:
    device = torch.device("cuda" if torch.cuda.is_available() else "cpu")
    print(f"Device: {device}")

    JinSRNet = _import_jin_srnet()
    model = JinSRNet().to(device)

    state = torch.load(args.jin_srnet, map_location=device)
    if isinstance(state, dict) and "state_dict" in state:
        state = state["state_dict"]
    model.load_state_dict(state, strict=False)
    print(f"Loaded JIN-SRNet weights from {args.jin_srnet}")

    data_root = Path(args.data_root)
    pairs = build_pairs(data_root)
    print(f"Built {len(pairs)} (cover, stego) pairs from {data_root}")

    train_pairs, val_pairs, test_pairs, seed_triple = seed_split(pairs, seed=args.split_seed)
    train_seeds, val_seeds, test_seeds = seed_triple
    print(
        f"Split: {len(train_pairs)} train / {len(val_pairs)} val / {len(test_pairs)} test pairs"
    )
    print(
        f"Seed counts: {len(train_seeds)} / {len(val_seeds)} / {len(test_seeds)}"
    )

    train_ds = PairedCoverStegoDataset(train_pairs, augment=True)
    val_ds = PairedCoverStegoDataset(val_pairs, augment=False)
    test_ds = PairedCoverStegoDataset(test_pairs, augment=False)

    train_loader = DataLoader(
        train_ds,
        batch_size=args.batch_size,
        shuffle=True,
        num_workers=args.workers,
        pin_memory=True,
        drop_last=True,
    )
    val_loader = DataLoader(
        val_ds,
        batch_size=args.batch_size,
        shuffle=False,
        num_workers=args.workers,
        pin_memory=True,
    )
    test_loader = DataLoader(
        test_ds,
        batch_size=args.batch_size,
        shuffle=False,
        num_workers=args.workers,
        pin_memory=True,
    )

    optimizer = torch.optim.Adam(model.parameters(), lr=args.lr)
    criterion = nn.CrossEntropyLoss()

    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)

    best_val_acc = 0.0
    epoch_log: list[dict] = []

    for epoch in range(1, args.epochs + 1):
        model.train()
        total_loss = 0.0
        total_n = 0
        for imgs, labels in train_loader:
            imgs = imgs.to(device, non_blocking=True)
            labels = labels.to(device, non_blocking=True)
            logits = model(imgs)
            loss = criterion(logits, labels)
            optimizer.zero_grad()
            loss.backward()
            optimizer.step()
            total_loss += loss.item() * imgs.size(0)
            total_n += imgs.size(0)

        train_loss = total_loss / max(1, total_n)
        val_metrics = measure_det_rate(model, val_loader, device)
        val_acc = 0.5 * (val_metrics["stego_det@0.5"] + (1 - val_metrics["cover_fp@0.5"]))
        print(
            f"[epoch {epoch:2d}] train_loss={train_loss:.4f} "
            f"val stego_det={val_metrics['stego_det@0.5']:.3f} "
            f"val cover_fp={val_metrics['cover_fp@0.5']:.3f} "
            f"val_acc={val_acc:.3f}"
        )
        epoch_log.append(
            {
                "epoch": epoch,
                "train_loss": train_loss,
                "val": val_metrics,
                "val_acc": val_acc,
            }
        )

        if val_acc > best_val_acc:
            best_val_acc = val_acc
            ckpt_path = out_dir / "juw_div500_best.pt"
            torch.save({"state_dict": model.state_dict(), "epoch": epoch}, ckpt_path)
            print(f"  saved best checkpoint to {ckpt_path}")

    test_metrics = measure_det_rate(model, test_loader, device)
    print(f"\nFinal test metrics: {test_metrics}")

    summary = {
        "args": vars(args),
        "split": {
            "train_seeds": train_seeds,
            "val_seeds": val_seeds,
            "test_seeds": test_seeds,
        },
        "epoch_log": epoch_log,
        "best_val_acc": best_val_acc,
        "test": test_metrics,
    }
    summary_path = out_dir / "training_summary.json"
    summary_path.write_text(json.dumps(summary, indent=2))
    print(f"Wrote summary to {summary_path}")


def build_argparser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--data-root", required=True, help="Diversity-500 data root")
    p.add_argument("--jin-srnet", required=True, help="Path to JIN-SRNet .pt checkpoint")
    p.add_argument("--out-dir", required=True, help="Output directory for checkpoints + logs")
    p.add_argument("--epochs", type=int, default=10)
    p.add_argument("--batch-size", type=int, default=32)
    p.add_argument("--lr", type=float, default=1e-5)
    p.add_argument("--workers", type=int, default=4)
    p.add_argument("--split-seed", type=int, default=0)
    return p


if __name__ == "__main__":
    train(build_argparser().parse_args())
