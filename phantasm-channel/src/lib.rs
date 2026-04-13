//! Channel-aware embedding adapters for phantasm.
//!
//! When a stego JPEG is uploaded to a service like Twitter, Facebook, or
//! Instagram, the service decodes the image to spatial pixels, possibly
//! resizes, and re-encodes at its own quality factor. The re-encode
//! requantizes every DCT coefficient — destroying any embedded payload
//! that depends on exact coefficient values.
//!
//! A [`ChannelAdapter`] *stabilizes* a cover so that selected coefficients
//! survive a known re-encode pipeline. Non-stabilizable positions are
//! flagged as wet (cost = ∞) so the wet-paper STC coder routes around them.
//!
//! ## Algorithm summary
//!
//! 1. **MINICER (Minimum-Iterative Coefficient Error Robust)** — for each
//!    AC coefficient, simulate the channel's re-encode and check whether
//!    the post-encode value carries the same parity as the source value.
//!    If not, perturb the source by ±1, ±2, … and re-simulate until it
//!    stabilizes or we abandon. Abandoned positions are wet.
//!
//! 2. **ROAST (Robust Overflow Alleviation for STego)** — when the
//!    perturbation would push a coefficient outside the valid coefficient
//!    range, mark it wet immediately (no further iteration). If a single
//!    block accumulates more than [`ROAST_BLOCK_WET_THRESHOLD`] wet
//!    positions we mark the *whole* block wet — sacrificing one block
//!    is cheaper than persisting a many-coefficient distortion that the
//!    cover never had.
//!
//! 3. The cost map passed in is mutated: wet positions get
//!    `f64::INFINITY`, stabilized positions are scaled down by
//!    [`STABILIZED_COST_DISCOUNT`] (they're now robust → cheaper to use).
//!
//! Today only [`twitter::TwitterProfile`] ships, but the [`ChannelAdapter`]
//! trait is structured so a future Facebook / Instagram / WhatsApp profile
//! can drop in.

mod error;
mod minicer;
mod simulate;
pub mod twitter;
mod zigzag;

pub use error::ChannelError;
pub use twitter::{ChromaSub, TwitterProfile};

use phantasm_cost::CostMap;
use phantasm_image::jpeg::JpegCoefficients;

/// A whole block in any 8×8 component is sacrificed (all positions
/// flagged wet) once it accumulates more than this many uncodeable
/// positions. ROAST overflow alleviation: keeps the cover image
/// undistorted at the cost of dropping a few percent of capacity.
pub const ROAST_BLOCK_WET_THRESHOLD: usize = 30;

/// Multiplicative discount applied to the cost of a successfully
/// stabilized position. The position is now robust to recompression,
/// so the STC coder should prefer it over an unstabilized neighbour.
/// 1.0 = no discount; 0.5 = stabilized positions are half as costly.
pub const STABILIZED_COST_DISCOUNT: f64 = 0.75;

/// MINICER iteration cap. After this many ±k perturbations we give up
/// and mark the position wet. Larger values raise survival rate but
/// hurt visual fidelity (the perturbation scales with k).
pub const MINICER_MAX_ITERATIONS: usize = 4;

/// Report from a stabilization pass.
#[derive(Debug, Clone, Default)]
pub struct StabilizationReport {
    /// `(component, block_idx, dct_pos)` tuples flagged uncodeable. Each
    /// matching entry in the supplied [`CostMap`] now has both costs set
    /// to `f64::INFINITY`.
    pub wet_positions: Vec<(usize, usize, usize)>,
    /// Number of positions that were verified or made robust to the
    /// channel's re-encode (a strict subset of the input positions).
    pub stabilized_count: usize,
    /// Of [`Self::wet_positions`], how many were marked wet by ROAST
    /// (overflow) rather than by MINICER (iteration cap exhausted).
    pub overflow_alleviated_count: usize,
    /// Number of *whole blocks* sacrificed by ROAST because they
    /// exceeded [`ROAST_BLOCK_WET_THRESHOLD`].
    pub sacrificed_blocks: usize,
    /// Estimate of the fraction of stabilized positions that will
    /// survive a real channel re-encode. Computed from the simulation
    /// loop, not measured against an external encoder.
    pub survival_rate_estimate: f64,
}

/// A channel adapter takes a cover JPEG plus its cost map and stabilizes
/// the coefficients so they survive the channel's re-encode pipeline.
///
/// Implementations mutate the [`JpegCoefficients`] in place (perturbing
/// individual coefficient values within the [`JpegComponent::coefficients`]
/// arrays) and mutate the [`CostMap`] to flag wet positions.
///
/// [`JpegComponent::coefficients`]: phantasm_image::jpeg::JpegComponent::coefficients
pub trait ChannelAdapter {
    fn name(&self) -> &str;

    fn stabilize(
        &self,
        cover: &mut JpegCoefficients,
        component_idx: usize,
        cost_map: &mut CostMap,
    ) -> Result<StabilizationReport, ChannelError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_default_is_empty() {
        let r = StabilizationReport::default();
        assert!(r.wet_positions.is_empty());
        assert_eq!(r.stabilized_count, 0);
        assert_eq!(r.sacrificed_blocks, 0);
        assert_eq!(r.survival_rate_estimate, 0.0);
    }
}
