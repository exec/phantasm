use thiserror::Error;

#[derive(Debug, Error)]
pub enum BenchError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Image error: {0}")]
    Image(#[from] image::ImageError),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Buffer length mismatch: {0} vs {1}")]
    LengthMismatch(usize, usize),
    #[error("Empty buffer")]
    EmptyBuffer,
    #[error("{0}")]
    Other(String),
}
