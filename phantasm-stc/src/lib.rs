mod error;
mod parity;

pub mod double_layer;
pub mod encoder;

pub use double_layer::{DoubleLayerDecoder, DoubleLayerEncoder};
pub use encoder::{StcConfig, StcDecoder, StcEncoder};
pub use error::StcError;

#[cfg(test)]
mod tests;
