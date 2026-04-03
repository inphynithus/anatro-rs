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
    /// Core extraction method based on start and end time in seconds.
    fn extract_pcm_secs(
        &self,
        path: &Path,
        start_sec: f64,
        end_sec: f64,
    ) -> Result<AudioBuffer, DomainError>;

    /// Extracts raw PCM data using HH:MM:SS strings for the timestamp range.
    fn extract_pcm_range(&self, path: &Path, range: &str) -> Result<AudioBuffer, DomainError>;

    /// Extracts raw PCM data using relative percentages of the track's total duration (0.0 to 1.0).
    fn extract_pcm_relative(
        &self,
        path: &Path,
        start_percent: f64,
        end_percent: f64,
    ) -> Result<AudioBuffer, DomainError>;
}

/// Port for exporting extracted PCM data to a WAV file.
pub trait PcmExporter {
    /// Exports the audio buffer to a WAV file.
    fn export_wav(&self, buffer: &AudioBuffer, output: &Path) -> Result<(), DomainError>;
}
