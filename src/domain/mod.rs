//! Domain error types for anotro-rs.

use thiserror::Error;

pub mod audio;
pub mod pipeline;
pub mod traits;

/// Domain-specific errors.
#[derive(Debug, Error)]
pub enum DomainError {
    /// Error during audio extraction.
    #[error("Audio extraction failed: {0}")]
    ExtractionError(String),

    /// Error during fingerprint generation.
    #[error("Fingerprint generation failed: {0}")]
    FingerprintError(String),

    /// Error during media file processing.
    #[error("Media processing failed: {0}")]
    MediaError(String),

    /// Error related to user input.
    #[error("User input error: {0}")]
    InputError(String),
}
