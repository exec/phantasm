use std::path::Path;

use crate::error::BenchError;

pub trait Steganalyzer: Send + Sync {
    fn name(&self) -> &str;
    fn detect(&self, image_path: &Path) -> Result<f64, BenchError>;
}

pub struct NullDetector;

impl Steganalyzer for NullDetector {
    fn name(&self) -> &str {
        "null"
    }

    fn detect(&self, _: &Path) -> Result<f64, BenchError> {
        Ok(0.5)
    }
}
