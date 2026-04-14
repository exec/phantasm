//! Passphrase-derived position-subset wrapper.
//!
//! Wraps any base [`DistortionFunction`] and deterministically marks a
//! pseudo-random fraction of non-DC positions as wet (cost = ∞) based on the
//! passphrase. Different passphrases mark different position subsets, so the
//! candidate set that STC operates on genuinely varies per-passphrase — not
//! just the cost ranking within a fixed set (which is what [`super::Noisy`]
//! does).
//!
//! ## Why this is different from cost-noise
//!
//! [`super::Noisy`] perturbs the cost VALUES but leaves the candidate position
//! list identical across passphrases. STC's choices differ between passphrases
//! because the cost ranking shifts, but the chosen positions are still drawn
//! from the same content-adaptive distribution — the higher-level statistical
//! signature of "what does a phantasm-modified DCT block look like" is
//! preserved, and that signature is what a CNN steganalyzer learns. Empirically,
//! cost-noise alone does NOT defend (see `ML_STEGANALYSIS.md` § Update 5).
//!
//! `PassphraseSubset` instead changes WHICH POSITIONS ARE EVEN AVAILABLE to
//! STC. With `keep_fraction = 0.5`, half the non-DC positions are forbidden
//! per-passphrase. Different passphrases forbid different halves. This shifts
//! the actual position distribution per-passphrase, not just the cost ranking,
//! which (by hypothesis) should fragment a CNN's training distribution at the
//! level the CNN actually learns.
//!
//! ## Why this doesn't break extract
//!
//! Extract is geometric: it derives positions from the passphrase and reads
//! parities at all of them. STC's wet-paper marks are encoder-side constraints
//! — the decoder doesn't see them. As long as the receiver uses the same
//! passphrase, the position list is identical between embed and extract, and
//! decoding works regardless of which positions the encoder was constrained
//! away from.

use crate::{CostMap, DistortionFunction};
use phantasm_image::jpeg::JpegCoefficients;
use sha2::{Digest, Sha256};

/// Recommended minimum keep-fraction. Below this, STC may run out of usable
/// capacity for a typical payload.
pub const MIN_KEEP_FRACTION: f64 = 0.10;

/// A wrapper around any base `DistortionFunction` that marks `(1 - keep_fraction)`
/// of non-DC positions as wet (cost = ∞), with the wet mask deterministically
/// derived from the passphrase. See module docs for the design.
pub struct PassphraseSubset<D: DistortionFunction> {
    base: D,
    keep_fraction: f64,
    seed: [u8; 32],
    name: String,
}

impl<D: DistortionFunction> PassphraseSubset<D> {
    /// Construct a subset wrapper around `base` with the given `keep_fraction`
    /// in `[0.0, 1.0]` (1.0 keeps all positions, 0.0 keeps none) and a seed.
    pub fn new(base: D, keep_fraction: f64, seed: [u8; 32]) -> Self {
        let name = format!("{}+subset{}", base.name(), keep_fraction);
        Self {
            base,
            keep_fraction,
            seed,
            name,
        }
    }

    /// Convenience constructor that derives the seed from a passphrase string.
    pub fn from_passphrase(base: D, keep_fraction: f64, passphrase: &str) -> Self {
        let mut h = Sha256::new();
        h.update(b"phantasm-cost-subset-v1");
        h.update(passphrase.as_bytes());
        let digest = h.finalize();
        let mut seed = [0u8; 32];
        seed.copy_from_slice(&digest);
        Self::new(base, keep_fraction, seed)
    }
}

impl<D: DistortionFunction> DistortionFunction for PassphraseSubset<D> {
    fn compute(&self, jpeg: &JpegCoefficients, component_idx: usize) -> CostMap {
        let mut map = self.base.compute(jpeg, component_idx);
        if self.keep_fraction >= 1.0 {
            return map;
        }
        // Threshold for keep_fraction in u64 space: positions with hash u64
        // value below `threshold` are kept; the rest are marked wet.
        let threshold = (self.keep_fraction.clamp(0.0, 1.0) * (u64::MAX as f64)) as u64;
        for (i, &(br, bc, dp)) in map.positions.iter().enumerate() {
            let mut h = Sha256::new();
            h.update(self.seed);
            h.update((br as u32).to_le_bytes());
            h.update((bc as u32).to_le_bytes());
            h.update((dp as u32).to_le_bytes());
            let digest = h.finalize();
            let val = u64::from_le_bytes(digest[0..8].try_into().unwrap());
            if val >= threshold {
                map.costs_plus[i] = f64::INFINITY;
                map.costs_minus[i] = f64::INFINITY;
            }
        }
        map
    }

    fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Uniform;
    use phantasm_image::jpeg::{JpegCoefficients, JpegComponent};

    fn fake_jpeg(blocks_high: usize, blocks_wide: usize) -> JpegCoefficients {
        let coefficients = vec![0i16; blocks_high * blocks_wide * 64];
        let component = JpegComponent {
            id: 1,
            blocks_high,
            blocks_wide,
            quant_table: [1u16; 64],
            coefficients,
            h_samp_factor: 1,
            v_samp_factor: 1,
        };
        JpegCoefficients {
            width: (blocks_wide * 8) as u32,
            height: (blocks_high * 8) as u32,
            components: vec![component],
            quality_estimate: Some(85),
            markers: vec![],
        }
    }

    #[test]
    fn keep_fraction_one_is_identity() {
        let jpeg = fake_jpeg(8, 8);
        let baseline = Uniform.compute(&jpeg, 0);
        let subset = PassphraseSubset::new(Uniform, 1.0, [0u8; 32]).compute(&jpeg, 0);
        assert_eq!(baseline.costs_plus, subset.costs_plus);
        assert_eq!(baseline.costs_minus, subset.costs_minus);
    }

    #[test]
    fn keep_fraction_half_marks_about_half_wet() {
        let jpeg = fake_jpeg(16, 16);
        let subset = PassphraseSubset::new(Uniform, 0.5, [42u8; 32]).compute(&jpeg, 0);
        let n = subset.costs_plus.len();
        let wet = subset.costs_plus.iter().filter(|c| c.is_infinite()).count();
        let frac = wet as f64 / n as f64;
        // Should be ~50% with statistical variance. 16x16x63 = 16128 positions.
        assert!(
            frac > 0.45 && frac < 0.55,
            "expected ~50% wet, got {:.3}",
            frac
        );
    }

    #[test]
    fn keep_fraction_zero_marks_all_wet() {
        let jpeg = fake_jpeg(4, 4);
        let subset = PassphraseSubset::new(Uniform, 0.0, [0u8; 32]).compute(&jpeg, 0);
        for c in &subset.costs_plus {
            assert!(c.is_infinite());
        }
    }

    #[test]
    fn different_seeds_select_different_subsets() {
        let jpeg = fake_jpeg(8, 8);
        let a = PassphraseSubset::new(Uniform, 0.5, [1u8; 32]).compute(&jpeg, 0);
        let b = PassphraseSubset::new(Uniform, 0.5, [2u8; 32]).compute(&jpeg, 0);
        let mut diffs = 0;
        for i in 0..a.costs_plus.len() {
            let a_wet = a.costs_plus[i].is_infinite();
            let b_wet = b.costs_plus[i].is_infinite();
            if a_wet != b_wet {
                diffs += 1;
            }
        }
        // With two independent ~50% selections, expected disagreement is ~50%.
        let frac = diffs as f64 / a.costs_plus.len() as f64;
        assert!(
            frac > 0.4 && frac < 0.6,
            "expected ~50% disagreement, got {:.3}",
            frac
        );
    }

    #[test]
    fn from_passphrase_is_deterministic() {
        let jpeg = fake_jpeg(4, 4);
        let a = PassphraseSubset::from_passphrase(Uniform, 0.5, "hunter2").compute(&jpeg, 0);
        let b = PassphraseSubset::from_passphrase(Uniform, 0.5, "hunter2").compute(&jpeg, 0);
        let c = PassphraseSubset::from_passphrase(Uniform, 0.5, "different").compute(&jpeg, 0);
        let a_wet: Vec<bool> = a.costs_plus.iter().map(|c| c.is_infinite()).collect();
        let b_wet: Vec<bool> = b.costs_plus.iter().map(|c| c.is_infinite()).collect();
        let c_wet: Vec<bool> = c.costs_plus.iter().map(|c| c.is_infinite()).collect();
        assert_eq!(a_wet, b_wet);
        assert_ne!(a_wet, c_wet);
    }

    #[test]
    fn name_includes_keep_fraction() {
        let s = PassphraseSubset::new(Uniform, 0.5, [0u8; 32]);
        assert!(s.name().contains("uniform"));
        assert!(s.name().contains("0.5"));
    }
}
