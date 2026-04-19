"""Multi-passphrase J-UNIWARD fine-tune on the diversity-500 cover corpus.

Direct fork of ~/phantasm-eval/multi/finetune_multi.py with a few changes:
  - data root: corpus-diversity-500/data/ (500 unique covers, 5 passphrases)
  - split: first 75 test seeds, next 75 val, remaining 350 train (proportional
    to Update 3's 3/3/16 of 22 seeds; keeps seed-level discipline)
  - target: fixed to juniward (UERD not part of this follow-up)
  - seed filename regex matches `d500_NNNN.jpg`
  - output dir: corpus-diversity-500/runs/juw_d500_v1/

Hyperparameters unchanged: Adam lr=1e-5, batch 16, 10 epochs, crop 256x256,
random flip, JIN-SRNet init.

Usage:
    python finetune_d500.py [out_subdir]
"""
import os, sys, glob, json, random, re, time
import torch
import torch.nn as nn
import torch.nn.functional as F
from torch.utils.data import Dataset, DataLoader
import numpy as np
from PIL import Image

TARGET = "juniward"
OUT_SUB = sys.argv[1] if len(sys.argv) > 1 else "runs/juw_d500_v1"

JIN_SRC = os.path.expanduser("~/phantasm-eval/jin-srnet/JIN_SRNet")
JIN_CKPT = os.path.join(JIN_SRC, "epoch=56_val_wAUC=0.8921.pt")
DATA_DIR = os.path.expanduser("~/phantasm-eval/corpus-diversity-500/data")
OUT_DIR = os.path.expanduser(f"~/phantasm-eval/corpus-diversity-500/{OUT_SUB}")
N_PASSPHRASES = 5
os.makedirs(OUT_DIR, exist_ok=True)

sys.path.insert(0, JIN_SRC)
from SRNet import SRNet

SEED = 42
random.seed(SEED); np.random.seed(SEED); torch.manual_seed(SEED); torch.cuda.manual_seed_all(SEED)

device = torch.device("cuda")
print(f"device: {torch.cuda.get_device_name(0)}")
print(f"target: {TARGET}, n_passphrases: {N_PASSPHRASES}, out: {OUT_DIR}")

# Matches d500_NNNN.jpg (also matches qf75_1024_NNNN.jpg patterns for
# compatibility).
SEED_RE = re.compile(r"_(\d{4})\.jpg$")

def seed_of(name):
    m = SEED_RE.search(name)
    return m.group(1) if m else None

cover_paths = sorted(glob.glob(os.path.join(DATA_DIR, "cover", "*.jpg")))
print(f"unique covers: {len(cover_paths)}")
assert len(cover_paths) >= 400, f"expected >= 400 covers, got {len(cover_paths)}"

pairs = []
for cover_path in cover_paths:
    name = os.path.basename(cover_path)
    s = seed_of(name)
    for i in range(N_PASSPHRASES):
        stego_path = os.path.join(DATA_DIR, f"{TARGET}_p{i}", name)
        if os.path.isfile(stego_path):
            pairs.append((cover_path, stego_path, s))
print(f"total (cover, stego) pairs: {len(pairs)}")

seeds = sorted({p[2] for p in pairs if p[2]})
print(f"unique seeds: {len(seeds)}")

# Proportional split matching Update 3's 3/3/16-of-22 shape.
# With 500 seeds: 75 test / 75 val / 350 train = 15% / 15% / 70%.
rng = random.Random(SEED)
shuffled = seeds[:]
rng.shuffle(shuffled)
n_test = round(len(seeds) * 3 / 22)
n_val = round(len(seeds) * 3 / 22)
test_seeds = set(shuffled[:n_test])
val_seeds = set(shuffled[n_test:n_test + n_val])
train_seeds = set(shuffled[n_test + n_val:])
print(f"split: train={len(train_seeds)} val={len(val_seeds)} test={len(test_seeds)}")

def items_for(seed_set):
    out = []
    for cover, stego, s in pairs:
        if s in seed_set:
            out.append((cover, 0))
            out.append((stego, 1))
    return out

train_items = items_for(train_seeds)
val_items = items_for(val_seeds)
test_items = items_for(test_seeds)
print(f"items: train={len(train_items)} val={len(val_items)} test={len(test_items)}")

CROP = 256

class JpegDataset(Dataset):
    def __init__(self, items, train=False):
        self.items = items; self.train = train
    def __len__(self): return len(self.items)
    def __getitem__(self, idx):
        path, label = self.items[idx]
        img = Image.open(path).convert("RGB")
        arr = np.asarray(img, dtype=np.float32) / 255.0
        H, W, _ = arr.shape
        if self.train:
            if H >= CROP and W >= CROP:
                top = random.randint(0, H - CROP); left = random.randint(0, W - CROP)
                arr = arr[top:top+CROP, left:left+CROP, :]
            if random.random() < 0.5:
                arr = arr[:, ::-1, :].copy()
        else:
            if H >= CROP and W >= CROP:
                top = (H - CROP) // 2; left = (W - CROP) // 2
                arr = arr[top:top+CROP, left:left+CROP, :]
        return torch.from_numpy(arr).permute(2, 0, 1), label

def make_loader(items, train, batch_size):
    return DataLoader(JpegDataset(items, train=train), batch_size=batch_size, shuffle=train,
                      num_workers=4, pin_memory=True, drop_last=False)

train_loader = make_loader(train_items, True, batch_size=16)
val_loader = make_loader(val_items, False, batch_size=16)
test_loader = make_loader(test_items, False, batch_size=16)

model = SRNet(3, 2)
model.load_state_dict(torch.load(JIN_CKPT, map_location="cpu"))
model = model.to(device)

optimizer = torch.optim.Adam(model.parameters(), lr=1e-5)
criterion = nn.CrossEntropyLoss()

def evaluate(loader, name):
    model.train(False)
    p_stego_by_label = {0: [], 1: []}
    correct = 0; total = 0
    with torch.no_grad():
        for x, y in loader:
            x, y = x.to(device, non_blocking=True), y.to(device, non_blocking=True)
            logits = model(x)
            probs = F.softmax(logits, dim=1)
            pred = logits.argmax(dim=1)
            correct += (pred == y).sum().item(); total += y.size(0)
            for i in range(y.size(0)):
                p_stego_by_label[y[i].item()].append(probs[i, 1].item())
    acc = correct / total
    cover_p_mean = float(np.mean(p_stego_by_label[0])) if p_stego_by_label[0] else 0.0
    stego_p_mean = float(np.mean(p_stego_by_label[1])) if p_stego_by_label[1] else 0.0
    cover_det = sum(1 for p in p_stego_by_label[0] if p > 0.5)
    stego_det = sum(1 for p in p_stego_by_label[1] if p > 0.5)
    print(f"  {name}: acc={acc:.4f} | cover P(stego) mean={cover_p_mean:.3f} det={cover_det}/{len(p_stego_by_label[0])} | stego P(stego) mean={stego_p_mean:.3f} det={stego_det}/{len(p_stego_by_label[1])}")
    return {"acc": acc, "cover_p_mean": cover_p_mean, "stego_p_mean": stego_p_mean,
            "cover_det": cover_det, "n_cover": len(p_stego_by_label[0]),
            "stego_det": stego_det, "n_stego": len(p_stego_by_label[1])}

print("\n=== BASELINE (JIN-SRNet pretrained) ===")
baseline_test = evaluate(test_loader, "test ")
baseline_val = evaluate(val_loader, "val  ")

EPOCHS = 10
best_val_acc = baseline_val["acc"]
best_state = None
history = {"target": TARGET, "n_passphrases": N_PASSPHRASES,
           "split_counts": {"train": len(train_seeds), "val": len(val_seeds), "test": len(test_seeds)},
           "split_seeds": {"train": sorted(train_seeds), "val": sorted(val_seeds), "test": sorted(test_seeds)},
           "baseline": {"val": baseline_val, "test": baseline_test}, "epochs": []}

print(f"\n=== FINE-TUNING multi target={TARGET} (epochs={EPOCHS}, lr=1e-5, batch=16) ===")
for epoch in range(1, EPOCHS + 1):
    model.train(True)
    t0 = time.time()
    epoch_loss = 0.0; n_batches = 0
    for x, y in train_loader:
        x, y = x.to(device, non_blocking=True), y.to(device, non_blocking=True)
        optimizer.zero_grad()
        logits = model(x)
        loss = criterion(logits, y)
        loss.backward()
        optimizer.step()
        epoch_loss += loss.item(); n_batches += 1
    avg_loss = epoch_loss / max(1, n_batches)
    elapsed = time.time() - t0
    print(f"\nepoch {epoch}/{EPOCHS}: loss={avg_loss:.4f} ({elapsed:.1f}s)")
    val_metrics = evaluate(val_loader, "val  ")
    test_metrics = evaluate(test_loader, "test ")
    history["epochs"].append({"epoch": epoch, "train_loss": avg_loss,
                               "val": val_metrics, "test": test_metrics})
    if val_metrics["acc"] > best_val_acc:
        best_val_acc = val_metrics["acc"]
        best_state = {k: v.detach().cpu().clone() for k, v in model.state_dict().items()}
        print(f"  * new best val acc: {best_val_acc:.4f}")

if best_state is not None:
    model.load_state_dict(best_state)
print("\n=== FINAL (best val checkpoint) ===")
final_test = evaluate(test_loader, "test ")
history["final_test"] = final_test

with open(os.path.join(OUT_DIR, "history.json"), "w") as f:
    json.dump(history, f, indent=2)
if best_state is not None:
    torch.save(best_state, os.path.join(OUT_DIR, f"{TARGET}_d500_best.pt"))
print(f"\nsaved to {OUT_DIR}")
