use crate::channel::ChannelProfile;
use crate::stealth::StealthTier;

/// Per-image hash-guard classification from Spike B's finding (PLAN §3.5).
/// Three tiers based on pHash margin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashSensitivity {
    /// ~75%: hash guard is a no-op
    Robust,
    /// ~15%: per-block cost ceilings
    Marginal,
    /// ~10%: pre-nudge or refuse
    Sensitive,
}

/// The full plan for embedding a payload in a specific cover image.
/// Produced by the orchestrator; consumed by the pipeline.
#[derive(Debug, Clone)]
pub struct EmbedPlan {
    pub channel: ChannelProfile,
    pub stealth_tier: StealthTier,
    pub capacity_bits: usize,
    pub payload_bits: usize,
    pub ecc_bits: usize,
    pub estimated_detection_error: f64,
    pub hash_constrained_positions: usize,
    pub hash_sensitivity: HashSensitivity,
}
