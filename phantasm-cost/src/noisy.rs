//! Passphrase-randomized cost-noise wrapper for any base distortion function.
//!
//! Wraps a base [`DistortionFunction`] (typically `Uerd` or `Juniward`) and
//! applies a deterministic, passphrase-derived multiplicative noise to every
//! per-coefficient cost. Different passphrases produce different noise
//! patterns, which means STC routes the embedding modifications through a
//! different set of candidate positions for each passphrase.
//!
//! Why this exists: even with a constant cost function, phantasm's modification
//! pattern is identical across passphrases (the cost map is a function of the
//! cover content alone). An attacker training a CNN steganalyzer on phantasm
//! output therefore only needs to learn one underlying modification
//! distribution, regardless of how many passphrase variants they collect. By
//! making the cost map itself depend on the passphrase, we fragment the
//! attacker's training distribution: each passphrase exposes a different
//! "modification fingerprint", so an attacker needs training data covering
//! the entire space of possible passphrase-derived noise patterns to converge.
//!
//! See `ML_STEGANALYSIS.md` § Update 5 for the empirical evaluation.
//!
//! ## Cost transformation
//!
//! For each non-DC coefficient at `(block_row, block_col, dct_pos)`:
//!
//! ```text
//! noise[pos]  = uniform_in[-1, +1] derived from SHA-256(seed || pos)
//! cost'[pos]  = base_cost[pos] * (1 + noise_amp * noise[pos])
//! ```
//!
//! `noise_amp` is the only knob. At `0.0`, the wrapper is the identity. At
//! `1.0`, costs are wiggled in the range `[0, 2 * base_cost]`. Higher values
//! push costs to extremes and start to dominate the natural cost structure.
//!
//! ## Why this doesn't break extract
//!
//! STC's decoder reads parities at passphrase-derived positions and recovers
//! the syndrome — it never consults the cost map. So the embed-side cost can
//! be randomized freely as long as the receiver derives the same position
//! permutation from the same passphrase, which phantasm already does. There
//! is no envelope-format break.

use crate::{CostMap, DistortionFunction};
use phantasm_image::jpeg::JpegCoefficients;
use sha2::{Digest, Sha256};

/// Recommended maximum noise amplitude. Above this value, the noise begins to
/// dominate the natural cost structure and stego stealth degrades faster than
/// the per-passphrase fragmentation defends.
pub const MAX_NOISE_AMPLITUDE: f64 = 2.0;

/// A wrapper around any base `DistortionFunction` that applies passphrase-derived
/// multiplicative cost noise. See module docs for the full design.
pub struct Noisy<D: DistortionFunction> {
    base: D,
    noise_amp: f64,
    seed: [u8; 32],
    name: String,
}

impl<D: DistortionFunction> Noisy<D> {
    /// Construct a noisy wrapper around `base` with the given amplitude and seed.
    ///
    /// `noise_amp` should be in `[0.0, MAX_NOISE_AMPLITUDE]`. Values outside
    /// the recommended range are accepted but the orchestrator should warn
    /// the user.
    pub fn new(base: D, noise_amp: f64, seed: [u8; 32]) -> Self {
        let name = format!("{}+noise{}", base.name(), noise_amp);
        Self {
            base,
            noise_amp,
            seed,
            name,
        }
    }

    /// Construct a noisy wrapper using a passphrase string (hashed to derive
    /// the noise seed). Convenience for callers that have a passphrase rather
    /// than a raw seed.
    pub fn from_passphrase(base: D, noise_amp: f64, passphrase: &str) -> Self {
        let mut h = Sha256::new();
        h.update(b"phantasm-cost-noise-v1");
        h.update(passphrase.as_bytes());
        let digest = h.finalize();
        let mut seed = [0u8; 32];
        seed.copy_from_slice(&digest);
        Self::new(base, noise_amp, seed)
    }
}

impl<D: DistortionFunction> DistortionFunction for Noisy<D> {
    fn compute(&self, jpeg: &JpegCoefficients, component_idx: usize) -> CostMap {
        let mut map = self.base.compute(jpeg, component_idx);
        if self.noise_amp == 0.0 {
            return map;
        }
        for (i, &(br, bc, dp)) in map.positions.iter().enumerate() {
            let noise = position_noise(&self.seed, br, bc, dp);
            let factor = (1.0 + self.noise_amp * noise).max(1e-6);
            map.costs_plus[i] = (map.costs_plus[i] * factor).max(1e-12);
            map.costs_minus[i] = (map.costs_minus[i] * factor).max(1e-12);
        }
        map
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// Map (seed, position) → uniform noise in `[-1, +1]` via SHA-256.
fn position_noise(seed: &[u8; 32], br: usize, bc: usize, dp: usize) -> f64 {
    let mut h = Sha256::new();
    h.update(seed);
    h.update((br as u32).to_le_bytes());
    h.update((bc as u32).to_le_bytes());
    h.update((dp as u32).to_le_bytes());
    let digest = h.finalize();
    let val = u64::from_le_bytes(digest[0..8].try_into().unwrap());
    // Map [0, u64::MAX] to [-1, +1]. The mapping is exact at the endpoints
    // (modulo float precision) and uniform in between.
    (val as f64 / u64::MAX as f64) * 2.0 - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Uniform;

    fn fake_jpeg(blocks_high: usize, blocks_wide: usize) -> JpegCoefficients {
        use phantasm_image::jpeg::{JpegCoefficients, JpegComponent};
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
    fn zero_noise_is_identity() {
        let jpeg = fake_jpeg(4, 4);
        let baseline = Uniform.compute(&jpeg, 0);
        let noisy = Noisy::new(Uniform, 0.0, [0u8; 32]).compute(&jpeg, 0);
        assert_eq!(baseline.positions, noisy.positions);
        assert_eq!(baseline.costs_plus, noisy.costs_plus);
        assert_eq!(baseline.costs_minus, noisy.costs_minus);
    }

    #[test]
    fn nonzero_noise_perturbs_costs() {
        let jpeg = fake_jpeg(4, 4);
        let baseline = Uniform.compute(&jpeg, 0);
        let noisy = Noisy::new(Uniform, 0.5, [42u8; 32]).compute(&jpeg, 0);
        let mut diffs = 0;
        for i in 0..baseline.costs_plus.len() {
            if (baseline.costs_plus[i] - noisy.costs_plus[i]).abs() > 1e-9 {
                diffs += 1;
            }
        }
        assert!(
            diffs > baseline.costs_plus.len() / 2,
            "expected at least half of costs to differ, got {}/{}",
            diffs,
            baseline.costs_plus.len()
        );
    }

    #[test]
    fn different_seeds_produce_different_noise() {
        let jpeg = fake_jpeg(4, 4);
        let a = Noisy::new(Uniform, 0.5, [1u8; 32]).compute(&jpeg, 0);
        let b = Noisy::new(Uniform, 0.5, [2u8; 32]).compute(&jpeg, 0);
        assert_ne!(a.costs_plus, b.costs_plus);
    }

    #[test]
    fn same_seed_is_deterministic() {
        let jpeg = fake_jpeg(4, 4);
        let a = Noisy::new(Uniform, 0.5, [7u8; 32]).compute(&jpeg, 0);
        let b = Noisy::new(Uniform, 0.5, [7u8; 32]).compute(&jpeg, 0);
        assert_eq!(a.costs_plus, b.costs_plus);
    }

    #[test]
    fn from_passphrase_is_deterministic() {
        let jpeg = fake_jpeg(4, 4);
        let a = Noisy::from_passphrase(Uniform, 0.5, "hunter2").compute(&jpeg, 0);
        let b = Noisy::from_passphrase(Uniform, 0.5, "hunter2").compute(&jpeg, 0);
        let c = Noisy::from_passphrase(Uniform, 0.5, "different").compute(&jpeg, 0);
        assert_eq!(a.costs_plus, b.costs_plus);
        assert_ne!(a.costs_plus, c.costs_plus);
    }

    #[test]
    fn costs_stay_positive() {
        let jpeg = fake_jpeg(4, 4);
        let noisy = Noisy::new(Uniform, 1.5, [99u8; 32]).compute(&jpeg, 0);
        for c in &noisy.costs_plus {
            assert!(*c > 0.0, "non-positive cost: {}", c);
        }
        for c in &noisy.costs_minus {
            assert!(*c > 0.0, "non-positive cost: {}", c);
        }
    }

    #[test]
    fn name_includes_noise_amp() {
        let n = Noisy::new(Uniform, 0.5, [0u8; 32]);
        assert!(n.name().contains("uniform"));
        assert!(n.name().contains("0.5"));
    }
}
