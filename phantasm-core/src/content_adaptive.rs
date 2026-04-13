use std::path::Path;

use phantasm_cost::DistortionFunction;
use phantasm_image::jpeg;

use crate::error::CoreError;
use crate::orchestrator::{
    ChannelCompatibility, CoverAnalysis, CoverFormat, EmbedResult, Orchestrator,
};
use crate::pipeline::{embed_with_costs, extract_from_cover, usable_positions};
use crate::plan::{EmbedPlan, HashSensitivity};
use crate::stealth::StealthTier;

pub struct ContentAdaptiveOrchestrator {
    distortion: Box<dyn DistortionFunction>,
}

impl ContentAdaptiveOrchestrator {
    pub fn new(distortion: Box<dyn DistortionFunction>) -> Self {
        Self { distortion }
    }

    pub fn distortion_name(&self) -> &str {
        self.distortion.name()
    }
}

impl Orchestrator for ContentAdaptiveOrchestrator {
    fn analyze(&self, cover_path: &Path) -> Result<CoverAnalysis, CoreError> {
        let jpeg = jpeg::read(cover_path)?;
        let positions = usable_positions(&jpeg);
        let capacity_bits = positions.len() / 4;
        let capacity_bytes = capacity_bits / 8;
        let overhead = 100usize;
        let net_bytes = capacity_bytes.saturating_sub(overhead);

        let tiers = vec![
            (StealthTier::Max, net_bytes),
            (StealthTier::High, net_bytes),
            (StealthTier::Medium, net_bytes),
            (StealthTier::Low, net_bytes),
        ];

        let channel_compatibility = vec![
            ChannelCompatibility {
                channel: "lossless".to_string(),
                compatible: true,
                note: None,
            },
            ChannelCompatibility {
                channel: "twitter".to_string(),
                compatible: false,
                note: Some("not yet implemented".to_string()),
            },
            ChannelCompatibility {
                channel: "facebook".to_string(),
                compatible: false,
                note: Some("not yet implemented".to_string()),
            },
            ChannelCompatibility {
                channel: "instagram".to_string(),
                compatible: false,
                note: Some("not yet implemented".to_string()),
            },
        ];

        Ok(CoverAnalysis {
            format: CoverFormat::Jpeg {
                quality: jpeg.quality_estimate.unwrap_or(0),
            },
            dimensions: (jpeg.width, jpeg.height),
            quality_estimate: jpeg.quality_estimate,
            tier_capacities: tiers,
            hash_sensitivity: HashSensitivity::Robust,
            channel_compatibility,
        })
    }

    fn embed(
        &self,
        cover_path: &Path,
        payload: &[u8],
        passphrase: &str,
        _plan: &EmbedPlan,
        output_path: &Path,
    ) -> Result<EmbedResult, CoreError> {
        let jpeg = jpeg::read(cover_path)?;
        let costs = self.distortion.compute(&jpeg, 0);
        embed_with_costs(cover_path, payload, passphrase, &costs, output_path)
    }

    fn extract(&self, stego_path: &Path, passphrase: &str) -> Result<Vec<u8>, CoreError> {
        extract_from_cover(stego_path, passphrase)
    }
}
