pub mod channel;
pub mod content_adaptive;
pub mod error;
pub mod hash_guard;
pub mod minimal;
pub mod orchestrator;
#[doc(hidden)]
pub mod pipeline;
pub mod plan;
#[doc(hidden)]
pub mod research_raw;
pub mod stealth;

pub use channel::{ChannelProfile, ChromaSub, OverflowStrategy};
pub use content_adaptive::ContentAdaptiveOrchestrator;
pub use error::CoreError;
pub use hash_guard::{HashGuardReport, HashType, SensitivityTier};
pub use minimal::MinimalOrchestrator;
pub use orchestrator::{
    ChannelCompatibility, CoverAnalysis, CoverFormat, EmbedResult, Orchestrator, StubOrchestrator,
};
pub use phantasm_channel::{ChannelAdapter, StabilizationReport, TwitterProfile};
pub use plan::{EmbedPlan, HashSensitivity};
pub use stealth::StealthTier;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrator::Orchestrator;
    use std::path::Path;

    // StealthTier tests
    #[test]
    fn stealth_tier_from_str_lowercase() {
        assert_eq!(StealthTier::from_str("max"), Some(StealthTier::Max));
        assert_eq!(StealthTier::from_str("high"), Some(StealthTier::High));
        assert_eq!(StealthTier::from_str("medium"), Some(StealthTier::Medium));
        assert_eq!(StealthTier::from_str("low"), Some(StealthTier::Low));
    }

    #[test]
    fn stealth_tier_from_str_case_insensitive() {
        assert_eq!(StealthTier::from_str("MAX"), Some(StealthTier::Max));
        assert_eq!(StealthTier::from_str("High"), Some(StealthTier::High));
        assert_eq!(StealthTier::from_str("MEDIUM"), Some(StealthTier::Medium));
    }

    #[test]
    fn stealth_tier_from_str_unknown() {
        assert_eq!(StealthTier::from_str("unknown"), None);
    }

    #[test]
    fn stealth_tier_bpp_range_max() {
        assert_eq!(StealthTier::Max.bpp_range(), (0.01, 0.05));
    }

    #[test]
    fn stealth_tier_bpp_range_all() {
        assert_eq!(StealthTier::High.bpp_range(), (0.05, 0.20));
        assert_eq!(StealthTier::Medium.bpp_range(), (0.20, 0.40));
        assert_eq!(StealthTier::Low.bpp_range(), (0.40, 0.60));
    }

    // ChannelProfile built-in tests
    #[test]
    fn channel_profile_facebook() {
        let p = ChannelProfile::builtin("facebook").unwrap();
        assert_eq!(p.jpeg_quality, Some(72));
        assert!(p.applies_enhancement);
        assert!(p.strips_metadata);
        assert_eq!(p.overflow_strategy, OverflowStrategy::BoundaryOnly);
    }

    #[test]
    fn channel_profile_unknown_returns_none() {
        assert!(ChannelProfile::builtin("nonsense").is_none());
    }

    #[test]
    fn channel_profile_all_builtin_names_count() {
        assert_eq!(ChannelProfile::all_builtin_names().len(), 8);
    }

    // HashSensitivity exhaustive match test (compile-time coverage)
    #[test]
    fn hash_sensitivity_exhaustive_match() {
        fn classify(s: HashSensitivity) -> &'static str {
            match s {
                HashSensitivity::Robust => "robust",
                HashSensitivity::Marginal => "marginal",
                HashSensitivity::Sensitive => "sensitive",
            }
        }
        assert_eq!(classify(HashSensitivity::Robust), "robust");
        assert_eq!(classify(HashSensitivity::Marginal), "marginal");
        assert_eq!(classify(HashSensitivity::Sensitive), "sensitive");
    }

    // StubOrchestrator tests
    #[test]
    fn stub_orchestrator_analyze_returns_not_implemented() {
        let o = StubOrchestrator;
        let err = o.analyze(Path::new("dummy.jpg")).unwrap_err();
        assert!(matches!(err, CoreError::NotImplemented(_)));
    }

    #[test]
    fn stub_orchestrator_embed_returns_not_implemented() {
        let o = StubOrchestrator;
        let channel = ChannelProfile::builtin("lossless").unwrap();
        let plan = EmbedPlan {
            channel,
            stealth_tier: StealthTier::Max,
            capacity_bits: 1000,
            payload_bits: 100,
            ecc_bits: 20,
            estimated_detection_error: 0.49,
            hash_constrained_positions: 0,
            hash_sensitivity: HashSensitivity::Robust,
        };
        let err = o
            .embed(
                Path::new("cover.jpg"),
                b"payload",
                "passphrase",
                &plan,
                Path::new("out.jpg"),
            )
            .unwrap_err();
        assert!(matches!(err, CoreError::NotImplemented(_)));
    }

    #[test]
    fn stub_orchestrator_extract_returns_not_implemented() {
        let o = StubOrchestrator;
        let err = o.extract(Path::new("stego.jpg"), "pass").unwrap_err();
        assert!(matches!(err, CoreError::NotImplemented(_)));
    }

    // EmbedPlan debug output regression test
    #[test]
    fn embed_plan_debug_contains_fields() {
        let channel = ChannelProfile::builtin("twitter").unwrap();
        let plan = EmbedPlan {
            channel,
            stealth_tier: StealthTier::High,
            capacity_bits: 8192,
            payload_bits: 512,
            ecc_bits: 128,
            estimated_detection_error: 0.42,
            hash_constrained_positions: 16,
            hash_sensitivity: HashSensitivity::Marginal,
        };
        let debug = format!("{plan:?}");
        assert!(debug.contains("capacity_bits: 8192"));
        assert!(debug.contains("payload_bits: 512"));
        assert!(debug.contains("ecc_bits: 128"));
        assert!(debug.contains("Marginal"));
        assert!(debug.contains("High"));
    }

    // Error variant display test
    #[test]
    fn error_payload_too_large_display() {
        let e = CoreError::PayloadTooLarge {
            size: 1024,
            capacity: 512,
        };
        let msg = e.to_string();
        assert!(msg.contains("1024"));
        assert!(msg.contains("512"));
    }
}
