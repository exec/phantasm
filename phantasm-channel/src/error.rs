use thiserror::Error;

#[derive(Debug, Error)]
pub enum ChannelError {
    #[error("component index {0} out of range (jpeg has {1} components)")]
    ComponentIndexOutOfRange(usize, usize),
    #[error("cost map has {cost_map} positions but jpeg component has {jpeg} coefficient slots")]
    CostMapMismatch { cost_map: usize, jpeg: usize },
    #[error("cost map references block ({br}, {bc}) outside component bounds ({bw}×{bh})")]
    CostMapPositionOutOfBounds {
        br: usize,
        bc: usize,
        bw: usize,
        bh: usize,
    },
    #[error("invalid quality factor {0} (must be 1..=100)")]
    InvalidQualityFactor(u8),
}
