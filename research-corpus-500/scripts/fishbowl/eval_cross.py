"""Evaluate the diversity-500 J-UW checkpoint on:
  (i)  the original eval-198 set (cover/uerd/juniward/uniform, 1 passphrase)
       --- same set used by Update 3's five_way_eval.py --- for direct
       comparison with Update 3's 89.9% J-UW detection number.
  (ii) the diversity-500 held-out test split (the 68 test seeds, 5 passphrases,
       already reported in training log as 329/340 = 96.76%; re-computed here
       for self-consistency).
  (iii) the 198-cover 5-pass data's held-out test split from Update 3's multi/
        directory (juniward_p{0..4}, same 3 test seeds as Update 3) --- this
        is the closest apples-to-apples number to Update 3's 89.9%.

Writes a single summary JSON.
"""
import os, sys, glob, json, random, re
import torch
import torch.nn.functional as F
import numpy as np
from PIL import Image

JIN_SRC = os.path.expanduser("~/phantasm-eval/jin-srnet/JIN_SRNet")
CKPT = os.path.expanduser("~/phantasm-eval/corpus-diversity-500/runs/juw_d500_v1/juniward_d500_best.pt")
EVAL_198 = os.path.expanduser("~/phantasm-eval/eval-198")
MULTI_198 = os.path.expanduser("~/phantasm-eval/multi")
D500_DATA = os.path.expanduser("~/phantasm-eval/corpus-diversity-500/data")
OUT_JSON = os.path.expanduser("~/phantasm-eval/corpus-diversity-500/runs/juw_d500_v1/cross_eval.json")

sys.path.insert(0, JIN_SRC)
from SRNet import SRNet

device = torch.device("cuda")
print(f"device: {torch.cuda.get_device_name(0)}")

SEED_RE = re.compile(r"_(\d{4})\.jpg$")
CROP = 256

def load_jpeg_batched(paths, batch=16):
    batch_tensors = []
    for path in paths:
        img = Image.open(path).convert("RGB")
        arr = np.asarray(img, dtype=np.float32) / 255.0
        H, W, _ = arr.shape
        if H >= CROP and W >= CROP:
            top = (H - CROP) // 2; left = (W - CROP) // 2
            arr = arr[top:top+CROP, left:left+CROP, :]
        batch_tensors.append(torch.from_numpy(arr).permute(2, 0, 1))
        if len(batch_tensors) == batch:
            yield torch.stack(batch_tensors).to(device)
            batch_tensors = []
    if batch_tensors:
        yield torch.stack(batch_tensors).to(device)

def infer_ps_stego(model, paths):
    model.train(False)
    ps = []
    with torch.no_grad():
        for x in load_jpeg_batched(paths, batch=16):
            logits = model(x)
            probs = F.softmax(logits, dim=1)[:, 1].cpu().numpy()
            ps.extend(probs.tolist())
    return ps

def summarize(ps):
    if not ps:
        return {"n": 0}
    arr = np.asarray(ps)
    return {"n": len(ps), "mean": float(arr.mean()), "median": float(np.median(arr)),
            "det": int((arr > 0.5).sum()), "det_rate": float((arr > 0.5).mean())}

# Load model
model = SRNet(3, 2)
model.load_state_dict(torch.load(CKPT, map_location="cpu"))
model = model.to(device)

results = {"checkpoint": CKPT, "update3_number": 0.899}

# === (i) eval-198 (Update 3's direct comparison set) ===
print("\n=== (i) eval-198 (apples-to-apples with Update 3 five_way_eval) ===")
results["eval_198"] = {}
for group in ("cover", "uniform", "uerd", "juniward"):
    paths = sorted(glob.glob(os.path.join(EVAL_198, group, "*.jpg")))
    ps = infer_ps_stego(model, paths)
    s = summarize(ps)
    print(f"  {group:<10} n={s['n']:>3} det={s['det']:>3}/{s['n']:<3} ({100*s['det_rate']:.1f}%) mean={s.get('mean',0):.3f}")
    results["eval_198"][group] = s

# === (ii) d500 held-out test seeds ===
print("\n=== (ii) diversity-500 held-out (68 test seeds x 5 passphrases) ===")
# Reproduce the exact train split used by finetune_d500.py
cover_paths = sorted(glob.glob(os.path.join(D500_DATA, "cover", "*.jpg")))
all_seeds = sorted({SEED_RE.search(os.path.basename(p)).group(1) for p in cover_paths})
rng = random.Random(42)
shuffled = all_seeds[:]
rng.shuffle(shuffled)
n_test = round(len(all_seeds) * 3 / 22)
n_val = round(len(all_seeds) * 3 / 22)
test_seeds = set(shuffled[:n_test])
print(f"  test seeds: {len(test_seeds)}")

test_cover_paths = [p for p in cover_paths if SEED_RE.search(os.path.basename(p)).group(1) in test_seeds]
ps_cover = infer_ps_stego(model, test_cover_paths)
results["d500_heldout"] = {"cover": summarize(ps_cover)}
print(f"  cover      n={len(ps_cover):>3} det={sum(1 for p in ps_cover if p > 0.5):>3}/{len(ps_cover)} ({100*np.mean([1 if p > 0.5 else 0 for p in ps_cover]):.1f}%)")

all_stego = []
for i in range(5):
    stego_dir = os.path.join(D500_DATA, f"juniward_p{i}")
    paths = sorted([p for p in glob.glob(os.path.join(stego_dir, "*.jpg"))
                    if SEED_RE.search(os.path.basename(p)).group(1) in test_seeds])
    ps = infer_ps_stego(model, paths)
    s = summarize(ps)
    results["d500_heldout"][f"juniward_p{i}"] = s
    print(f"  juw_p{i}     n={s['n']:>3} det={s['det']:>3}/{s['n']} ({100*s['det_rate']:.1f}%)")
    all_stego.extend(ps)
results["d500_heldout"]["juniward_all5"] = summarize(all_stego)
print(f"  juw_all5   n={len(all_stego):>3} det={sum(1 for p in all_stego if p > 0.5):>3}/{len(all_stego)} ({100*np.mean([1 if p > 0.5 else 0 for p in all_stego]):.1f}%)")

# === (iii) Update 3's 198-cover multi/ held-out test seeds ===
# Update 3 split: first 3 of 22 shuffled (seed=42) = test seeds.
print("\n=== (iii) Update 3 198-cover 5-pass held-out test split (apples-to-apples) ===")
u3_cover_paths = sorted(glob.glob(os.path.join(MULTI_198, "cover", "*.jpg")))
u3_all_seeds = sorted({SEED_RE.search(os.path.basename(p)).group(1) for p in u3_cover_paths})
rng2 = random.Random(42)
u3_shuffled = u3_all_seeds[:]
rng2.shuffle(u3_shuffled)
u3_test_seeds = set(u3_shuffled[:3])
print(f"  u3 test seeds: {sorted(u3_test_seeds)}")

results["u3_198_heldout"] = {}
u3_test_covers = [p for p in u3_cover_paths if SEED_RE.search(os.path.basename(p)).group(1) in u3_test_seeds]
ps_cover3 = infer_ps_stego(model, u3_test_covers)
results["u3_198_heldout"]["cover"] = summarize(ps_cover3)
print(f"  cover      n={len(ps_cover3):>3} det={sum(1 for p in ps_cover3 if p > 0.5):>3}/{len(ps_cover3)} ({100*np.mean([1 if p > 0.5 else 0 for p in ps_cover3]):.1f}%)")

u3_all_stego = []
for i in range(5):
    stego_dir = os.path.join(MULTI_198, f"juniward_p{i}")
    paths = sorted([p for p in glob.glob(os.path.join(stego_dir, "*.jpg"))
                    if SEED_RE.search(os.path.basename(p)).group(1) in u3_test_seeds])
    ps = infer_ps_stego(model, paths)
    s = summarize(ps)
    results["u3_198_heldout"][f"juniward_p{i}"] = s
    print(f"  juw_p{i}     n={s['n']:>3} det={s['det']:>3}/{s['n']} ({100*s['det_rate']:.1f}%)")
    u3_all_stego.extend(ps)
results["u3_198_heldout"]["juniward_all5"] = summarize(u3_all_stego)
print(f"  juw_all5   n={len(u3_all_stego):>3} det={sum(1 for p in u3_all_stego if p > 0.5):>3}/{len(u3_all_stego)} ({100*np.mean([1 if p > 0.5 else 0 for p in u3_all_stego]):.1f}%)")

# === Verdict ===
det_eval198_juw = results["eval_198"]["juniward"]["det_rate"]
det_d500 = results["d500_heldout"]["juniward_all5"]["det_rate"]
det_u3 = results["u3_198_heldout"]["juniward_all5"]["det_rate"]
results["summary"] = {
    "update3_juw_multi_on_eval198": 0.899,
    "d500_model_on_eval198_juw": det_eval198_juw,
    "d500_model_on_d500_heldout_juw": det_d500,
    "d500_model_on_u3_198_heldout_juw": det_u3,
    "delta_vs_update3_on_eval198": det_eval198_juw - 0.899,
    "delta_vs_update3_on_d500_heldout": det_d500 - 0.899,
}
# Verdict rule: primary metric is d500 held-out (most diverse, apples-to-oranges
# with update3's cover pool but same training recipe). Secondary: eval-198 number,
# for direct comparability to Update 3's five_way_eval.
delta = det_d500 - 0.899
if delta >= -0.05:
    verdict = "corpus-robust"
elif delta < -0.10:
    verdict = "likely Picsum artifact"
else:
    verdict = "inconclusive"
results["summary"]["verdict"] = verdict
print(f"\n=== SUMMARY ===")
print(f"Update 3 J-UW-multi on eval-198:               89.9%")
print(f"d500 model on eval-198 (juniward):             {100*det_eval198_juw:5.1f}%")
print(f"d500 model on d500 held-out (juniward all 5):  {100*det_d500:5.1f}%  (primary)")
print(f"d500 model on u3-198 held-out (juniward all 5): {100*det_u3:5.1f}%")
print(f"delta vs 89.9% on d500 held-out:               {100*delta:+.1f} pp")
print(f"VERDICT: {verdict}")

with open(OUT_JSON, "w") as f:
    json.dump(results, f, indent=2)
print(f"\nwrote {OUT_JSON}")
