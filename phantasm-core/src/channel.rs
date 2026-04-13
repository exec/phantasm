#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChromaSub {
    C444,
    C422,
    C420,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverflowStrategy {
    None,
    Clamp,
    BoundaryOnly,
    Full,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChannelProfile {
    pub name: String,
    pub jpeg_quality: Option<u8>,
    pub max_dimension: Option<u32>,
    pub chroma_subsampling: ChromaSub,
    pub applies_enhancement: bool,
    pub strips_metadata: bool,
    pub overflow_strategy: OverflowStrategy,
}

impl ChannelProfile {
    /// Built-in channel profiles from PLAN §3.3.
    pub fn builtin(name: &str) -> Option<Self> {
        let profile = match name {
            "lossless" => ChannelProfile {
                name: "lossless".to_string(),
                jpeg_quality: None,
                max_dimension: None,
                chroma_subsampling: ChromaSub::None,
                applies_enhancement: false,
                strips_metadata: false,
                overflow_strategy: OverflowStrategy::None,
            },
            "facebook" => ChannelProfile {
                name: "facebook".to_string(),
                jpeg_quality: Some(72),
                max_dimension: Some(2048),
                chroma_subsampling: ChromaSub::C420,
                applies_enhancement: true,
                strips_metadata: true,
                overflow_strategy: OverflowStrategy::BoundaryOnly,
            },
            "twitter" => ChannelProfile {
                name: "twitter".to_string(),
                jpeg_quality: Some(85),
                max_dimension: Some(4096),
                chroma_subsampling: ChromaSub::C420,
                applies_enhancement: false,
                strips_metadata: true,
                overflow_strategy: OverflowStrategy::Clamp,
            },
            "instagram" => ChannelProfile {
                name: "instagram".to_string(),
                jpeg_quality: Some(75),
                max_dimension: Some(1080),
                chroma_subsampling: ChromaSub::C420,
                applies_enhancement: true,
                strips_metadata: true,
                overflow_strategy: OverflowStrategy::Clamp,
            },
            "whatsapp-photo" => ChannelProfile {
                name: "whatsapp-photo".to_string(),
                jpeg_quality: Some(60),
                max_dimension: Some(1600),
                chroma_subsampling: ChromaSub::C420,
                applies_enhancement: false,
                strips_metadata: true,
                overflow_strategy: OverflowStrategy::Full,
            },
            "whatsapp-doc" => ChannelProfile {
                name: "whatsapp-doc".to_string(),
                jpeg_quality: None,
                max_dimension: None,
                chroma_subsampling: ChromaSub::None,
                applies_enhancement: false,
                strips_metadata: false,
                overflow_strategy: OverflowStrategy::None,
            },
            "signal" => ChannelProfile {
                name: "signal".to_string(),
                jpeg_quality: None,
                max_dimension: None,
                chroma_subsampling: ChromaSub::None,
                applies_enhancement: false,
                strips_metadata: false,
                overflow_strategy: OverflowStrategy::None,
            },
            "generic-75" => ChannelProfile {
                name: "generic-75".to_string(),
                jpeg_quality: Some(75),
                max_dimension: None,
                chroma_subsampling: ChromaSub::C420,
                applies_enhancement: false,
                strips_metadata: false,
                overflow_strategy: OverflowStrategy::Clamp,
            },
            _ => return None,
        };
        Some(profile)
    }

    /// Return all built-in profile names.
    pub fn all_builtin_names() -> &'static [&'static str] {
        &[
            "lossless",
            "facebook",
            "twitter",
            "instagram",
            "whatsapp-photo",
            "whatsapp-doc",
            "signal",
            "generic-75",
        ]
    }
}
