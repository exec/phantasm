use std::path::Path;

use crate::error::CoreError;
use crate::plan::{EmbedPlan, HashSensitivity};
use crate::stealth::StealthTier;

#[derive(Debug, Clone)]
pub enum CoverFormat {
    Jpeg { quality: u8 },
    Png,
    Other(String),
}

#[derive(Debug, Clone)]
pub struct ChannelCompatibility {
    pub channel: String,
    pub compatible: bool,
    pub note: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CoverAnalysis {
    pub format: CoverFormat,
    pub dimensions: (u32, u32),
    pub quality_estimate: Option<u8>,
    pub tier_capacities: Vec<(StealthTier, usize)>,
    pub hash_sensitivity: HashSensitivity,
    pub channel_compatibility: Vec<ChannelCompatibility>,
}

#[derive(Debug, Clone)]
pub struct EmbedResult {
    pub bytes_embedded: usize,
    pub capacity_used_ratio: f64,
    pub estimated_detection_error: f64,
}

/// The pipeline interface the CLI and bench will call.
/// Real implementations come in Phase 1 and Phase 2.
pub trait Orchestrator {
    fn analyze(&self, cover_path: &Path) -> Result<CoverAnalysis, CoreError>;

    fn embed(
        &self,
        cover_path: &Path,
        payload: &[u8],
        passphrase: &str,
        plan: &EmbedPlan,
        output_path: &Path,
    ) -> Result<EmbedResult, CoreError>;

    fn extract(&self, stego_path: &Path, passphrase: &str) -> Result<Vec<u8>, CoreError>;
}

/// Stub implementation that returns "not yet implemented" errors.
/// Real implementation lands in Phase 1.
pub struct StubOrchestrator;

impl Orchestrator for StubOrchestrator {
    fn analyze(&self, _cover_path: &Path) -> Result<CoverAnalysis, CoreError> {
        Err(CoreError::NotImplemented("analyze"))
    }

    fn embed(
        &self,
        _cover_path: &Path,
        _payload: &[u8],
        _passphrase: &str,
        _plan: &EmbedPlan,
        _output_path: &Path,
    ) -> Result<EmbedResult, CoreError> {
        Err(CoreError::NotImplemented("embed"))
    }

    fn extract(&self, _stego_path: &Path, _passphrase: &str) -> Result<Vec<u8>, CoreError> {
        Err(CoreError::NotImplemented("extract"))
    }
}
