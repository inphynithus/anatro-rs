//! Ports (traits) for external adapters.

use crate::domain::DomainError;
use crate::domain::audio::AudioBuffer;
use std::path::Path;

/// Port for track selection.
pub trait TrackSelector {
    /// Evaluates the media file and prompts the user to select an audio track index if there are multiple.
    /// If there is only one, returns 0. If `None` tracks, errors out.
    fn prompt_track_index(&self, path: &Path) -> Result<usize, DomainError>;

    /// Selects an audio track from the media file. If `track_index` is provided, selects that specific 
    /// audio track index (0-based). If `None`, it should prompt the user if there are multiple tracks.
    /// Returns the internal track ID `u32`.
    fn select_track(&self, path: &Path, track_index: Option<usize>) -> Result<u32, DomainError>;
}

/// Port for extracting audio from media files.
pub trait AudioExtractor {
    /// Returns the total duration of the track in seconds.
    fn get_duration(&self, path: &Path, track_id: u32) -> Result<f64, DomainError>;

    /// Extracts and resamples audio from the given file.
    fn extract_audio(&self, path: &Path, track_id: u32) -> Result<AudioBuffer, DomainError>;

    /// Extracts and resamples a specific range of audio from the given file.
    fn extract_audio_range(
        &self,
        path: &Path,
        track_id: u32,
        start_sec: f64,
        end_sec: f64,
    ) -> Result<AudioBuffer, DomainError>;

    /// Extracts and resamples a specific range of audio using relative percentages.
    fn extract_audio_relative(
        &self,
        path: &Path,
        track_id: u32,
        start_percent: f64,
        end_percent: f64,
    ) -> Result<AudioBuffer, DomainError>;

    /// Converts HH:MM:SS format to seconds.
    fn hms_to_seconds(&self, hms: &str) -> Result<f64, DomainError>;

    /// Core extraction method based on start and end time in seconds.
    fn extract_pcm_secs(
        &self,
        path: &Path,
        track_id: u32,
        start_sec: f64,
        end_sec: f64,
    ) -> Result<AudioBuffer, DomainError>;
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
    fn export_sample(
        &self,
        input: &Path,
        track_id: u32,
        output: &Path,
        range: &str,
    ) -> Result<(), DomainError>;
}

/// Port for finding sub-fingerprint matches within a target fingerprint.
pub trait FingerprintMatcher {
    /// Finds the best match of the `reference` sub-fingerprint within the `target` fingerprint.
    ///
    /// Returns the index of the best match if the error is within the `threshold`,
    /// or `None` if no suitable match is found.
    fn find_match(&self, reference: &[u32], target: &[u32], threshold: u32) -> Option<usize>;
}

/// Port for high-precision fine matching using cross-correlation.
pub trait FineMatcher {
    /// Finds the exact lag (in samples) of the `reference` within the `target` PCM data.
    ///
    /// Returns the lag if a match is found, or `None`.
    fn find_fine_match(
        &self,
        reference: &[i16],
        target: &[i16],
    ) -> Result<Option<isize>, DomainError>;
}
