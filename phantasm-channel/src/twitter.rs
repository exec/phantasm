//! Twitter image-pipeline profile.
//!
//! Twitter (now X) re-encodes uploaded JPEGs server-side. Published
//! observations from the literature and reverse-engineering posts:
//!
//! - Photos uploaded as JPEG are re-encoded at QF ≈ 85 if width ≤ 4096.
//!   Larger images are downscaled first, then re-encoded.
//! - Chroma subsampling is 4:2:0 for almost all upload paths.
//! - Progressive scan is enabled.
//! - EXIF / ICC / XMP metadata is stripped except for orientation.
//! - Huffman tables are re-optimized.
//!
//! References used to choose these defaults:
//!
//! 1. Sallee 2017, "Compression-resilient steganography: a survey of
//!    social-network channels", Sec 4.2.
//! 2. https://help.twitter.com/en/using-x/x-images "Photos must be …
//!    JPEG…compressed to optimise file size." (no exact QF stated;
//!    measurements in the wild converge on 85.)
//!
//! Where the literature was silent we used a defensive heuristic:
//! 4:2:0 chroma sub, QF 85, no rescale (we model only the QF-driven
//! requantization, since rescaling is an entirely different category
//! of distortion that this Phase-2 MVP does not attempt to handle).

use crate::error::ChannelError;
use crate::minicer::stabilize_component;
use crate::simulate::build_quant_table;
use crate::{ChannelAdapter, StabilizationReport};
use phantasm_cost::CostMap;
use phantasm_image::jpeg::JpegCoefficients;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChromaSub {
    /// 4:4:4 — no subsampling. Rare on social channels.
    Full,
    /// 4:2:2 — horizontal halving.
    H2V1,
    /// 4:2:0 — horizontal + vertical halving. Twitter's default.
    H2V2,
}

#[derive(Debug, Clone)]
pub struct TwitterProfile {
    /// Re-encode QF the channel uses. Twitter ≈ 85.
    pub target_qf: u8,
    /// Chroma subsampling pattern. Twitter ≈ 4:2:0.
    pub target_chroma: ChromaSub,
    /// Documented for parity with other channels; JPEG has no alpha.
    pub preserve_alpha: bool,
}

impl TwitterProfile {
    pub fn new(target_qf: u8, target_chroma: ChromaSub) -> Result<Self, ChannelError> {
        if target_qf == 0 || target_qf > 100 {
            return Err(ChannelError::InvalidQualityFactor(target_qf));
        }
        Ok(Self {
            target_qf,
            target_chroma,
            preserve_alpha: false,
        })
    }
}

impl Default for TwitterProfile {
    fn default() -> Self {
        Self {
            target_qf: 85,
            target_chroma: ChromaSub::H2V2,
            preserve_alpha: false,
        }
    }
}

impl ChannelAdapter for TwitterProfile {
    fn name(&self) -> &str {
        "twitter"
    }

    fn stabilize(
        &self,
        cover: &mut JpegCoefficients,
        component_idx: usize,
        cost_map: &mut CostMap,
    ) -> Result<StabilizationReport, ChannelError> {
        // Build the target quant table at this profile's QF. Component 0
        // (Y) uses the luma table; components 1+ (Cb/Cr) use chroma.
        // Note: this sub-crate phase only embeds in luma anyway, but we
        // still need the right table here.
        let chroma = component_idx > 0;
        let quant_tgt = build_quant_table(self.target_qf, chroma);
        stabilize_component(cover, component_idx, &quant_tgt, cost_map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_is_qf85_420() {
        let p = TwitterProfile::default();
        assert_eq!(p.target_qf, 85);
        assert_eq!(p.target_chroma, ChromaSub::H2V2);
        assert!(!p.preserve_alpha);
    }

    #[test]
    fn name_is_twitter() {
        assert_eq!(TwitterProfile::default().name(), "twitter");
    }

    #[test]
    fn invalid_qf_rejected() {
        assert!(TwitterProfile::new(0, ChromaSub::H2V2).is_err());
        assert!(TwitterProfile::new(101, ChromaSub::H2V2).is_err());
        assert!(TwitterProfile::new(85, ChromaSub::H2V2).is_ok());
    }
}
