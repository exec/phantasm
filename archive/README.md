# Archive

Historical / pre-release documents preserved for reference. Not load-bearing
for v1.

## Inventory

- **`PLAN.md`** — Pre-v0.1 architectural plan from project inception. The
  five-pillar thesis (content-adaptive cost, STC, AEAD envelope, channel-
  adaptive preprocessing, perceptual-hash preservation) and the original
  multi-phase build-out roadmap. Useful for understanding *why* phantasm is
  shaped the way it is. Some sections describe scope that was deferred or
  abandoned (Tier 2/Tier 3 plans, the original adversarial-cost research
  arc that was closed negatively in v0.3 / v0.4 — see `ML_STEGANALYSIS.md`).
- **`RESEARCH.md`** — Pre-v0.1 literature review. Survey of academic
  steganography (UNIWARD, UERD, MiPOD, STC, J-MiPOD), modern CNN
  steganalysis (SRNet, JIN-SRNet, Yedroudj-Net, ZhuNet), AEAD constructions,
  and channel-adaptive preprocessing. The reference list that informed the
  v0.1.0 architectural decisions. Some references (UERD, S-UNIWARD) describe
  cost functions that v1 dropped — see CHANGELOG for rationale.
- **`ML_STEGANALYSIS.md`** — Running research log of the v0.2 → v0.4 modern
  CNN steganalysis evaluation arc. Updates 1-8 cover the cost-function
  research that closed in v0.3 (per-coefficient cost adjustment is
  exhausted as an L1 defense direction against trained CNN attackers; v0.3
  d500-scale fully-adapted detection sits at 96.8%-100%). The v0.4 burst
  (HYDRA / CHAMELEON / DOPPELGÄNGER / PALIMPSEST L1-defense experiments)
  ran on private `experiment/*` branches in the original repo and is
  summarized in v0.4.0's CHANGELOG. The closing lesson — *the detectable
  signature is the FACT of modification, not WHERE phantasm modifies* —
  is what motivated v1 to commit honestly to the L2/L3-confidentiality
  framing rather than chasing further L1 defenses in the cost-map family.

## Why these are archived

v1 commits to a focused, JPEG-only, J-UNIWARD-only surface with a
clearly-scoped threat model (confidentiality of a payload an adversary can
see exists, not plausible deniability against a phantasm-aware CNN).
These documents were written before that scope was settled and reference
features, threat models, or research directions that are no longer
load-bearing for v1. They're preserved here because the historical record
is valuable to anyone trying to understand the project's posture.
