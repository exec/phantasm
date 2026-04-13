#![allow(clippy::needless_range_loop)]

//! Double-layer STC for ternary embedding in DCT coefficients.
//!
//! Adaptation of Filler, Judas, Fridrich, IEEE TIFS 2011 §V.B. Two parity
//! planes are extracted from each i16 coefficient's Euclidean mod-4 remainder
//! and embedded with two coupled STC passes.
//!
//! ## Two-plane decomposition
//!
//! For a coefficient `x`, let `r = x.rem_euclid(4) ∈ {0,1,2,3}`.
//!   - Plane 0: `r & 1`         (LSB-1, carries `m1`)
//!   - Plane 1: `(r >> 1) & 1`  (LSB-2, carries `m2`)
//!
//! Under floor modulo, ±1 always flips plane 0 and ±2 always flips plane 1
//! — this is the invariant the paper relies on and is sign-independent
//! (in contrast to the `|x|` convention, which aliases on sign crossings).
//!
//! ## Conditional-probability layering
//!
//! Instead of embedding the two planes with independent STC costs (`min(cp,cm)`
//! for layer 1 and `2·min(cp,cm)` for layer 2 — the legacy phantasm approach),
//! we build a per-position 4-cell cost table `c[k] = min_Δ cost(Δ)` where Δ
//! lands on mod-4 cell `k`, then embed in two coupled passes:
//!
//!   1. **Layer 2 (plane 1) with marginal cost**: STC pays
//!      `c2[i] = min(c[k] where k>>1 = ¬cover_p1)` — the cheapest way to reach
//!      *any* plane-0 representative of the flipped plane-1 cell.
//!   2. **Layer 1 (plane 0) with conditional cost** given layer 2's choice:
//!      let `nat_p0[i] = argmin_{p0} c[p0 + 2·target_p1[i]]` (the cheaper
//!      plane-0 sibling inside the committed plane-1 cell). STC pays
//!      `c1[i] = c[¬nat_p0 + 2·tp1] − c[nat_p0 + 2·tp1]` (≥ 0) to deviate.
//!
//! The key correctness claim: the total per-position cost paid by both STC
//! passes *exactly equals* the actual cost of the final delta, because the
//! cost table enumerates all 4 reachable mod-4 cells (Δ ∈ {−3..3} covers the
//! full cycle). This is the optimal two-step decomposition of ML2; the only
//! thing we still defer vs the full paper-standard ML2 is λ-tuned entropy
//! allocation between m1 and m2 (currently we split by `div_ceil(2)`).
//!
//! ## Wet paper
//!
//! - Fully wet position (cp = cm = ∞): `c[k] = ∞` for all k≠0, both layer
//!   costs are ∞, STC keeps the cover bits; final delta is 0.
//! - Half wet (cp or cm = ∞): `c[k]` populated only by the feasible direction;
//!   the cheaper plane-0 sibling inside each plane-1 cell is still selected.
//! - i16 saturation: deltas that would overflow `checked_add` are skipped,
//!   so extreme coefficients are treated as partially wet automatically.

use crate::encoder::{StcConfig, StcDecoder, StcEncoder};
use crate::error::StcError;

pub struct DoubleLayerEncoder {
    pub config: StcConfig,
}

pub struct DoubleLayerDecoder {
    pub config: StcConfig,
}

/// Plane-0 and plane-1 bits under the Euclidean mod-4 convention.
#[inline]
fn planes(x: i16) -> (u8, u8) {
    let r = x.rem_euclid(4) as u8;
    (r & 1, (r >> 1) & 1)
}

/// Per-position mod-4 cost table: `cost[k]` and best delta to reach mod-4
/// cell `k ∈ {0,1,2,3}` from cover `x`. Unreachable cells carry `f64::INFINITY`
/// cost and delta 0.
#[derive(Clone, Copy)]
struct CellTable {
    cost: [f64; 4],
    delta: [i16; 4],
}

/// Enumerate Δ ∈ {-3,..,3}, populate `cost[k]` with the minimum achievable
/// cost landing on mod-4 cell k and record the witness delta. Respects cp/cm
/// wet-paper sentinels and i16 saturation.
fn build_cell_table(x: i16, cp: f64, cm: f64) -> CellTable {
    let mut cost = [f64::INFINITY; 4];
    let mut delta = [0i16; 4];

    for &d in &[0i16, 1, -1, 2, -2, 3, -3] {
        let new_val = match x.checked_add(d) {
            Some(v) => v,
            None => continue,
        };
        let c = match d.cmp(&0) {
            std::cmp::Ordering::Equal => 0.0,
            std::cmp::Ordering::Greater => {
                if !cp.is_finite() {
                    continue;
                }
                cp * (d as f64)
            }
            std::cmp::Ordering::Less => {
                if !cm.is_finite() {
                    continue;
                }
                cm * (-d as f64)
            }
        };
        let (p0, p1) = planes(new_val);
        let k = (p0 | (p1 << 1)) as usize;
        if c < cost[k] {
            cost[k] = c;
            delta[k] = d;
        }
    }

    CellTable { cost, delta }
}

impl DoubleLayerEncoder {
    pub fn new(config: StcConfig) -> Self {
        Self { config }
    }

    /// Embed a binary message into a ternary cover (DCT coefficients).
    ///
    /// `cover` is a slice of i16 DCT coefficients.
    /// `costs_plus[i]` is the cost of cover[i] -> cover[i] + 1.
    /// `costs_minus[i]` is the cost of cover[i] -> cover[i] - 1.
    /// Either cost may be f64::INFINITY to forbid that direction
    /// (e.g., when cover[i] = i16::MAX or when the hash guard forbids change).
    ///
    /// Returns the modified stego coefficient vector.
    pub fn embed(
        &self,
        cover: &[i16],
        costs_plus: &[f64],
        costs_minus: &[f64],
        message: &[u8],
    ) -> Result<Vec<i16>, StcError> {
        let n = cover.len();

        if message.is_empty() {
            return Ok(cover.to_vec());
        }
        if n == 0 {
            return Err(StcError::LengthMismatch {
                cover: 0,
                message: message.len(),
            });
        }

        let total_bits = message.len();
        let m1_bits = total_bits.div_ceil(2);
        let m2_bits = total_bits - m1_bits;

        if !n.is_multiple_of(m1_bits) {
            return Err(StcError::LengthMismatch {
                cover: n,
                message: total_bits,
            });
        }
        if m2_bits > 0 && !n.is_multiple_of(m2_bits) {
            return Err(StcError::LengthMismatch {
                cover: n,
                message: total_bits,
            });
        }

        let m1 = &message[..m1_bits];
        let m2 = if m2_bits > 0 {
            Some(&message[m1_bits..])
        } else {
            None
        };

        // Per-position 4-cell cost table. This enumerates Δ ∈ {-3..3} once;
        // everything below is a lookup.
        let tables: Vec<CellTable> = (0..n)
            .map(|i| build_cell_table(cover[i], costs_plus[i], costs_minus[i]))
            .collect();

        // Cover plane-1 (layer-2 input bits).
        let cover_p1: Vec<u8> = cover.iter().map(|&x| planes(x).1).collect();

        // Layer-2 marginal cost: cheapest way to reach *any* representative
        // of the flipped plane-1 cell. `min(c[0], c[2])` is the "stay" cost
        // (which equals 0 via c[cover_k]) and `min(c[1|2·¬p1], c[2|2·¬p1])`
        // is the "flip" cost. STC pays the difference.
        let cost2: Vec<f64> = (0..n)
            .map(|i| {
                let cp1 = cover_p1[i] as usize;
                let t = &tables[i].cost;
                let stay = t[2 * cp1].min(t[1 + 2 * cp1]);
                let flip = t[2 * (1 - cp1)].min(t[1 + 2 * (1 - cp1)]);
                // `stay` is 0 at any feasible cover (Δ=0 is always free),
                // so in practice flip - stay = flip. Subtracting defensively
                // for any hypothetical cost model where Δ=0 isn't free.
                flip - stay
            })
            .collect();

        // Layer 2: embed m2 into plane-1 with marginal cost.
        let target_p1: Vec<u8> = if let Some(m2) = m2 {
            let enc2 = StcEncoder::new(StcConfig {
                constraint_height: self.config.constraint_height,
            });
            enc2.embed(&cover_p1, &cost2, m2)?
        } else {
            cover_p1.clone()
        };

        // Layer 1 conditional: given the committed plane-1 target, pick the
        // cheaper plane-0 sibling as the "natural" cover bit and let STC pay
        // the marginal cost of swapping siblings.
        let mut nat_p0 = vec![0u8; n];
        let mut cost1 = vec![0.0f64; n];
        for i in 0..n {
            let tp1 = target_p1[i] as usize;
            let t = &tables[i].cost;
            let c0 = t[2 * tp1]; // plane0 = 0
            let c1 = t[1 + 2 * tp1]; // plane0 = 1
            if c0 <= c1 {
                nat_p0[i] = 0;
                cost1[i] = c1 - c0;
            } else {
                nat_p0[i] = 1;
                cost1[i] = c0 - c1;
            }
        }

        let enc1 = StcEncoder::new(StcConfig {
            constraint_height: self.config.constraint_height,
        });
        let target_p0 = enc1.embed(&nat_p0, &cost1, m1)?;

        // Compose final i16 deltas from the precomputed witness deltas.
        let mut result = cover.to_vec();
        for i in 0..n {
            let k = (target_p0[i] | (target_p1[i] << 1)) as usize;
            let cell_cost = tables[i].cost[k];
            if cell_cost.is_finite() {
                let d = tables[i].delta[k];
                result[i] = cover[i].saturating_add(d);
            }
            // Unreachable cell (only possible if STC was forced into a fully
            // wet position whose syndrome already matched): leave cover[i].
        }

        Ok(result)
    }
}

impl DoubleLayerDecoder {
    pub fn new(config: StcConfig) -> Self {
        Self { config }
    }

    /// Extract a binary message from a ternary stego vector.
    /// `message_len` is the bit length of the original message.
    pub fn extract(&self, stego: &[i16], message_len: usize) -> Vec<u8> {
        if message_len == 0 || stego.is_empty() {
            return vec![];
        }

        let m1_bits = message_len.div_ceil(2);
        let m2_bits = message_len - m1_bits;

        let dec = StcDecoder::new(StcConfig {
            constraint_height: self.config.constraint_height,
        });

        let plane0: Vec<u8> = stego.iter().map(|&x| planes(x).0).collect();
        let m1 = dec.extract(&plane0, m1_bits);

        if m2_bits == 0 {
            return m1;
        }

        let plane1: Vec<u8> = stego.iter().map(|&x| planes(x).1).collect();
        let dec2 = StcDecoder::new(StcConfig {
            constraint_height: self.config.constraint_height,
        });
        let m2 = dec2.extract(&plane1, m2_bits);

        let mut result = m1;
        result.extend_from_slice(&m2);
        result
    }
}
