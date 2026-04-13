mod aead;
mod envelope;
mod kdf;
mod metadata;
mod padding;

pub use aead::{decrypt, encrypt};
pub use envelope::{open, seal, Envelope};
pub use kdf::{derive_key, derive_locations_key, KdfParams};
pub use metadata::{ContentType, PayloadMetadata};
pub use padding::{pad, unpad, BLOCK_SIZES};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("authentication failed")]
    AuthFailed,
    #[error("payload too large: exceeds maximum envelope size of 256 KiB")]
    PayloadTooLarge,
    #[error("invalid data: {0}")]
    InvalidData(String),
}

pub type Result<T> = std::result::Result<T, CryptoError>;
