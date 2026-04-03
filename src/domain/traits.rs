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

/// Port for exporting an audio sample as a file.
pub trait SampleExporter {
    /// Extracts a portion of an audio track and saves it to a file.
    ///
    /// `range` is expected to be in the format 'HH:MM:SS-HH:MM:SS'.
    fn export_sample(&self, input: &Path, output: &Path, range: &str) -> Result<(), DomainError>;
}

/// Port for sample-accurate PCM extraction.
pub trait PcmExtractor {
    /// Extracts raw PCM data from a media file for a given start time and duration.
    fn extract_pcm_range(
        &self,
        path: &Path,
        start: &str,
        duration: f64,
    ) -> Result<AudioBuffer, DomainError>;
}

/// Port for exporting extracted PCM data to a WAV file.
pub trait PcmExporter {
    /// Exports the audio buffer to a WAV file.
    fn export_wav(&self, buffer: &AudioBuffer, output: &Path) -> Result<(), DomainError>;
}
