#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("not yet implemented: {0}")]
    NotImplemented(&'static str),
    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid plan: {0}")]
    InvalidPlan(String),
    #[error("payload too large: {size} bytes exceeds capacity {capacity}")]
    PayloadTooLarge { size: usize, capacity: usize },
    #[error("image error: {0}")]
    Image(#[from] phantasm_image::ImageError),
    #[error("crypto error: {0}")]
    Crypto(#[from] phantasm_crypto::CryptoError),
    #[error("ECC error: {0}")]
    Ecc(#[from] phantasm_ecc::EccError),
    #[error("STC error: {0}")]
    Stc(#[from] phantasm_stc::StcError),
    #[error("invalid data: {0}")]
    InvalidData(String),
}
