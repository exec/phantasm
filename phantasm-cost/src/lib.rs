//! Content-adaptive distortion functions for JPEG steganography.
//!
//! A `DistortionFunction` translates a JPEG cover image into per-coefficient
//! embedding costs. Lower cost at position `i` means "safer to modify coefficient `i`"
//! — the STC coder uses these costs to minimize total detectability for a fixed payload.
//!
//! Implementations in this crate follow published academic distortion functions:
//!
//! - [`uerd::Uerd`] — Uniform Embedding Revisited Distortion (Guo, Ni, Shi 2015).
//!   JPEG-native, block-complexity-driven. Simpler than UNIWARD and competitive
//!   on security benchmarks.
//!
//! Future implementations may include J-UNIWARD (Holub & Fridrich 2014),
//! J-MiPOD (Cogranne, Giboulot, Bas 2020), HILL, etc.

pub mod uerd;
pub use uerd::Uerd;

use phantasm_image::jpeg::JpegCoefficients;

/// Per-coefficient embedding cost map for a single JPEG component.
///
/// `costs_plus[i]` and `costs_minus[i]` give the cost of modifying the coefficient
/// at `positions[i]` by +1 and −1 respectively. A cost of `f64::INFINITY` means
/// the position is forbidden to modify (wet-paper coding).
#[derive(Debug, Clone)]
pub struct CostMap {
    /// Cost of modifying each coefficient by +1. Same length as `positions`.
    pub costs_plus: Vec<f64>,
    /// Cost of modifying each coefficient by −1. Same length as `positions`.
    pub costs_minus: Vec<f64>,
    /// `(block_row, block_col, dct_pos)` for each coefficient, in the order used
    /// by `costs_plus` / `costs_minus`. `dct_pos` is the intra-block index 0..64
    /// in natural (row-major) order. Typically DC (`dct_pos == 0`) is excluded
    /// from the cost map entirely.
    pub positions: Vec<(usize, usize, usize)>,
}

impl CostMap {
    /// Returns the number of coefficients in the cost map.
    pub fn len(&self) -> usize {
        self.positions.len()
    }

    /// Returns whether the cost map is empty.
    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }
}

/// A content-adaptive distortion function that computes embedding costs
/// for a JPEG cover image.
///
/// Implementations should be `Send + Sync` so orchestrators can hold them
/// behind a `Box<dyn DistortionFunction>` and pass them across threads
/// if needed.
pub trait DistortionFunction: Send + Sync {
    /// Compute the cost map for `component_idx` in the given JPEG.
    /// Typical usage: pass `component_idx = 0` (the Y / luma component).
    /// Most implementations skip DC coefficients (position 0 in each block)
    /// and all saturated coefficients.
    fn compute(&self, jpeg: &JpegCoefficients, component_idx: usize) -> CostMap;

    /// Human-readable name for logging, reporting, and benchmark output.
    /// Examples: `"uerd"`, `"j-uniward"`, `"j-mipod"`, `"uniform"`.
    fn name(&self) -> &str;
}

/// A trivial uniform-cost distortion function that assigns cost 1.0 to every
/// non-DC coefficient. This is the baseline that `MinimalOrchestrator` uses
/// implicitly; exposing it here lets `ContentAdaptiveOrchestrator` use it
/// as a drop-in replacement for testing.
///
/// Not content-adaptive. Detectable by classical RS attack (Fridrich 2001).
/// Use a real distortion function like `Uerd` for anything that needs stealth.
pub struct Uniform;

impl DistortionFunction for Uniform {
    fn compute(&self, jpeg: &JpegCoefficients, component_idx: usize) -> CostMap {
        let component = &jpeg.components[component_idx];
        let mut positions = Vec::new();
        for br in 0..component.blocks_high {
            for bc in 0..component.blocks_wide {
                for dp in 1..64 {
                    positions.push((br, bc, dp));
                }
            }
        }
        let n = positions.len();
        CostMap {
            costs_plus: vec![1.0; n],
            costs_minus: vec![1.0; n],
            positions,
        }
    }

    fn name(&self) -> &str {
        "uniform"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_name() {
        assert_eq!(Uniform.name(), "uniform");
    }

    #[test]
    fn cost_map_empty() {
        let c = CostMap {
            costs_plus: vec![],
            costs_minus: vec![],
            positions: vec![],
        };
        assert!(c.is_empty());
        assert_eq!(c.len(), 0);
    }
}
