#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StealthTier {
    Max,
    High,
    Medium,
    Low,
}

impl StealthTier {
    /// Target bits-per-pixel range for this tier (min, max).
    pub fn bpp_range(&self) -> (f64, f64) {
        match self {
            StealthTier::Max => (0.01, 0.05),
            StealthTier::High => (0.05, 0.20),
            StealthTier::Medium => (0.20, 0.40),
            StealthTier::Low => (0.40, 0.60),
        }
    }

    /// Estimated detection error P_E lower bound for this tier.
    pub fn min_detection_error(&self) -> f64 {
        match self {
            StealthTier::Max => 0.49,
            StealthTier::High => 0.40,
            StealthTier::Medium => 0.25,
            StealthTier::Low => 0.10,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            StealthTier::Max => "max",
            StealthTier::High => "high",
            StealthTier::Medium => "medium",
            StealthTier::Low => "low",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "max" => Some(StealthTier::Max),
            "high" => Some(StealthTier::High),
            "medium" => Some(StealthTier::Medium),
            "low" => Some(StealthTier::Low),
            _ => None,
        }
    }
}

impl std::str::FromStr for StealthTier {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        StealthTier::from_str(s).ok_or(())
    }
}
