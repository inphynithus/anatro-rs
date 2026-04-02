//! Ports (traits) for external adapters.

use crate::domain::DomainError;
use crate::domain::audio::AudioBuffer;
use std::path::Path;

/// Port for extracting audio from media files.
pub trait AudioExtractor {
    /// Extracts and resamples audio from the given file.
    ///
    /// Implementations should handle track selection if multiple tracks are present.
    fn extract_audio(&self, path: &Path) -> Result<AudioBuffer, DomainError>;
}

/// Port for generating fingerprints from audio buffers.
pub trait Fingerprinter {
    /// Generates a fingerprint from the given audio buffer.
    fn generate_fingerprint(&self, buffer: &AudioBuffer) -> Result<Vec<u32>, DomainError>;
}
