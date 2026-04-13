use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum StcError {
    #[error("cover length ({cover}) must be a non-zero multiple of message length ({message})")]
    LengthMismatch { cover: usize, message: usize },

    #[error("embedding is infeasible: all positions are wet but the syndrome does not match the message")]
    InfeasibleWetPaper,
}
