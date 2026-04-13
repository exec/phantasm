use thiserror::Error;

#[derive(Debug, Error)]
pub enum ImageError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid format: {0}")]
    InvalidFormat(String),

    #[error("unsupported: {0}")]
    Unsupported(String),

    #[error("FFI failure: {0}")]
    FfiFailure(String),
}
