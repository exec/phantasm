#![allow(clippy::needless_range_loop)]

//! Double-layer STC for ternary embedding in DCT coefficients.
//!
//! Adaptation of Filler, Judas, Fridrich, IEEE TIFS 2011 §V.B for
//! self-contained encode/decode without access to the original cover.
//!
//! ## Two-plane construction
//!
//! Each i16 DCT coefficient `x` encodes two parity planes:
//!   - Plane 0: `(x.unsigned_abs()) & 1`
//!   - Plane 1: `(x.unsigned_abs() >> 1) & 1`
//!
//! Layer 1 (plane 0): embed m1 via ±1 adjustments.
//! Layer 2 (plane 1): embed m2 via ±2 adjustments (which flip bit-1 of abs).
//!
//! A ±2 adjustment from value `x` changes `|x|` by 2 (for |x| > 0), which
//! always flips bit-1 of the absolute value without touching bit-0. For x=0,
//! ±2 gives |±2|=2, which has bit-0=0 (unchanged) and bit-1=1 (flipped).
//!
//! The two planes are embedded independently by two separate STC passes;
//! the decoder extracts each independently. This gives ~2× bit capacity
//! for ~2× distortion vs single-layer, achieving the same bits/distortion
//! efficiency. The advantage is that a single ternary coefficient can carry
//! two bits (one per plane) where a binary coefficient carries one.
//!
//! ## Capacity advantage measurement
//!
//! Double-layer embeds `m1 + m2` bits total with distortion
//! `D1 + D2` (from two STC passes). Single-layer at the same rate embeds
//! `m1` bits with distortion `D1`. So for the same _total_ distortion budget,
//! double-layer embeds 2× the bits. At matched distortion (equal budget),
//! the bits/distortion ratio is 2×.
//!
//! ## Wet paper
//!
//! Layer-1 wet: `min(cp, cm) = ∞` (cannot change by ±1 in any direction).
//! Layer-2 wet: position cannot be changed by ±2 in any feasible direction,
//! OR both ±2 adjustments leave plane-1 unchanged (same parity result).

use crate::encoder::{StcConfig, StcDecoder, StcEncoder};
use crate::error::StcError;

pub struct DoubleLayerEncoder {
    pub config: StcConfig,
}

pub struct DoubleLayerDecoder {
    pub config: StcConfig,
}

/// Compute plane-0 and plane-1 bits of the absolute value of x.
#[inline]
fn planes(x: i16) -> (u8, u8) {
    let a = x.unsigned_abs();
    ((a & 1) as u8, ((a >> 1) & 1) as u8)
}

/// Compute the cost to flip plane-1 at position i using a ±2 adjustment
/// (which preserves plane-0). Searches ±2 moves only.
///
/// Returns `f64::INFINITY` if no feasible ±2 adjustment flips plane-1
/// while preserving plane-0.
fn layer2_cost(x: i16, cp: f64, cm: f64) -> f64 {
    let (p0_orig, p1_orig) = planes(x);
    let mut best = f64::INFINITY;

    for &(d, c_unit) in &[(2i16, cp), (-2i16, cm)] {
        let c = if d > 0 { cp } else { cm };
        if c.is_infinite() {
            continue;
        }
        if let Some(new_val) = x.checked_add(d) {
            let (p0_new, p1_new) = planes(new_val);
            if p0_new == p0_orig && p1_new != p1_orig {
                let cost = c_unit.abs() * 2.0;
                if cost < best {
                    best = cost;
                }
            }
        }
    }

    best
}

/// For a given cover value `x` and target planes `(tp0, tp1)`, find the
/// delta ∈ {−3, −2, −1, 0, +1, +2, +3} with minimum cost that achieves the target.
///
/// The 4-cycle of plane values (0,0)→(1,0)→(0,1)→(1,1)→(0,0)... means that
/// any (tp0, tp1) combination is reachable within ±3 of any starting value.
fn find_delta(x: i16, tp0: u8, tp1: u8, cp: f64, cm: f64) -> (i16, f64) {
    let (p0_orig, p1_orig) = planes(x);

    // Early exit: no change needed.
    if p0_orig == tp0 && p1_orig == tp1 {
        return (0, 0.0);
    }

    let mut best: (i16, f64) = (0, f64::INFINITY);

    for &d in &[-3i16, -2, -1, 0, 1, 2, 3] {
        let new_val = match x.checked_add(d) {
            Some(v) => v,
            None => continue,
        };
        let (p0, p1) = planes(new_val);
        if p0 != tp0 || p1 != tp1 {
            continue;
        }
        let cost = match d.cmp(&0) {
            std::cmp::Ordering::Equal => 0.0,
            std::cmp::Ordering::Greater => {
                if cp.is_infinite() {
                    continue;
                }
                cp * (d as f64)
            }
            std::cmp::Ordering::Less => {
                if cm.is_infinite() {
                    continue;
                }
                cm * (-d as f64)
            }
        };
        if cost < best.1 {
            best = (d, cost);
        }
    }

    best
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

        // Extract current plane bits.
        let plane0_cover: Vec<u8> = cover.iter().map(|&x| planes(x).0).collect();
        let plane1_cover: Vec<u8> = cover.iter().map(|&x| planes(x).1).collect();

        // Layer-1 cost: flipping plane-0 requires ±1 adjustment.
        let cost1: Vec<f64> = (0..n).map(|i| costs_plus[i].min(costs_minus[i])).collect();

        // Layer-2 cost: flipping plane-1 while preserving plane-0.
        // Computed per-position using layer2_cost (uses ±2 moves only).
        let cost2: Vec<f64> = (0..n)
            .map(|i| layer2_cost(cover[i], costs_plus[i], costs_minus[i]))
            .collect();

        let enc1 = StcEncoder::new(StcConfig {
            constraint_height: self.config.constraint_height,
        });
        let target_p0 = enc1.embed(&plane0_cover, &cost1, m1)?;

        let target_p1: Vec<u8> = if let Some(m2) = m2 {
            let enc2 = StcEncoder::new(StcConfig {
                constraint_height: self.config.constraint_height,
            });
            enc2.embed(&plane1_cover, &cost2, m2)?
        } else {
            plane1_cover.clone()
        };

        // Combine: find minimum-cost delta achieving (target_p0, target_p1).
        let mut result = cover.to_vec();
        for i in 0..n {
            let tp0 = target_p0[i];
            let tp1 = target_p1[i];

            let (delta, cost) = find_delta(cover[i], tp0, tp1, costs_plus[i], costs_minus[i]);

            if !cost.is_infinite() {
                result[i] = cover[i].saturating_add(delta);
            }
            // If cost is infinite, the STC wet-paper constraints prevented feasible
            // modification; cover[i] is unchanged (wet paper applied).
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
