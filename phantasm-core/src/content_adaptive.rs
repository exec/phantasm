use std::path::Path;

use phantasm_channel::ChannelAdapter;
use phantasm_cost::DistortionFunction;
use phantasm_image::jpeg;

use crate::error::CoreError;
use crate::hash_guard::HashType;
use crate::orchestrator::{
    ChannelCompatibility, CoverAnalysis, CoverFormat, EmbedResult, Orchestrator,
};
use crate::pipeline::{embed_with_costs_and_hooks, extract_from_cover, usable_positions};
use crate::plan::EmbedPlan;
use crate::stealth::StealthTier;

pub struct ContentAdaptiveOrchestrator {
    distortion: Box<dyn DistortionFunction>,
    channel_adapter: Option<Box<dyn ChannelAdapter>>,
    hash_guard: Option<HashType>,
}

impl ContentAdaptiveOrchestrator {
    pub fn new(distortion: Box<dyn DistortionFunction>) -> Self {
        Self {
            distortion,
            channel_adapter: None,
            hash_guard: None,
        }
    }

    pub fn with_channel_adapter(mut self, adapter: Box<dyn ChannelAdapter>) -> Self {
        self.channel_adapter = Some(adapter);
        self
    }

    pub fn with_hash_guard(mut self, hash_type: HashType) -> Self {
        self.hash_guard = Some(hash_type);
        self
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
                compatible: true,
                note: Some("MINICER+ROAST, ~10-20% capacity cost".to_string()),
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

        let hash_sensitivity = crate::hash_guard::classify_sensitivity(&jpeg);

        Ok(CoverAnalysis {
            format: CoverFormat::Jpeg {
                quality: jpeg.quality_estimate.unwrap_or(0),
            },
            dimensions: (jpeg.width, jpeg.height),
            quality_estimate: jpeg.quality_estimate,
            tier_capacities: tiers,
            hash_sensitivity,
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
        embed_with_costs_and_hooks(
            cover_path,
            payload,
            passphrase,
            &costs,
            output_path,
            self.hash_guard,
            self.channel_adapter.as_deref(),
        )
    }

    fn extract(&self, stego_path: &Path, passphrase: &str) -> Result<Vec<u8>, CoreError> {
        extract_from_cover(stego_path, passphrase)
    }
}
