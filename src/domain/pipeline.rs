//! Typestate pattern for invariant integrity in the processing pipeline.

use crate::domain::DomainError;
use crate::domain::audio::AudioBuffer;
use crate::domain::traits::{AudioExtractor, Fingerprinter};
use std::path::PathBuf;

/// Initial state: a source media file path.
#[derive(Debug)]
pub struct SourceMedia {
    pub(crate) path: PathBuf,
}

/// State after track selection.
#[derive(Debug)]
pub struct SelectedTrack {
    pub(crate) path: PathBuf,
    pub(crate) track_id: u32,
}

/// State after audio extraction: contains buffered audio for intro and outro search spaces.
#[derive(Debug)]
pub struct SegmentedAudio {
    pub(crate) path: PathBuf,
    pub(crate) intro_buffer: AudioBuffer,
    pub(crate) outro_buffer: AudioBuffer,
}

/// State after fingerprint generation: contains fingerprints for intro and outro search spaces.
#[derive(Debug)]
pub struct SegmentedFingerprints {
    pub(crate) path: PathBuf,
    pub(crate) intro_fingerprint: Vec<u32>,
    pub(crate) outro_fingerprint: Vec<u32>,
}

/// State after audio extraction: contains raw samples.
#[derive(Debug)]
pub struct ExtractedAudio {
    pub(crate) path: PathBuf,
    pub(crate) buffer: AudioBuffer,
}

/// State after fingerprint generation: contains the fingerprint.
#[derive(Debug)]
pub struct FingerprintedMedia {
    pub(crate) path: PathBuf,
    pub(crate) fingerprint: Vec<u32>,
}

/// State after a match operation has been attempted.
#[derive(Debug)]
pub struct MatchResult {
    pub path: PathBuf,
    pub match_index: Option<usize>,
}

impl SourceMedia {
    /// Creates a new SourceMedia state.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Transitions to SelectedTrack state by probing the file and selecting a track.
    pub fn select_track<S: crate::domain::traits::TrackSelector>(
        self,
        selector: &S,
    ) -> Result<SelectedTrack, DomainError> {
        let track_id = selector.select_track(&self.path)?;
        Ok(SelectedTrack {
            path: self.path,
            track_id,
        })
    }
}

impl SelectedTrack {
    /// Transitions to ExtractedAudio state by extracting audio using the provided extractor.
    pub fn extract_audio<E: AudioExtractor>(
        self,
        extractor: &E,
    ) -> Result<ExtractedAudio, DomainError> {
        let buffer = extractor.extract_audio(&self.path, self.track_id)?;
        Ok(ExtractedAudio {
            path: self.path,
            buffer,
        })
    }

    /// Transitions to SegmentedAudio state using intro (0.0-0.25) and outro (0.7-1.0) heuristics.
    pub fn extract_segmented_audio<E: AudioExtractor>(
        self,
        extractor: &E,
    ) -> Result<SegmentedAudio, DomainError> {
        let intro_buffer =
            extractor.extract_audio_relative(&self.path, self.track_id, 0.0, 0.25)?;
        let outro_buffer = extractor.extract_audio_relative(&self.path, self.track_id, 0.7, 1.0)?;
        Ok(SegmentedAudio {
            path: self.path,
            intro_buffer,
            outro_buffer,
        })
    }
}

impl SegmentedAudio {
    /// Transitions to SegmentedFingerprints state by generating fingerprints for both segments.
    pub fn generate_segmented_fingerprints<F: Fingerprinter>(
        self,
        fingerprinter: &F,
    ) -> Result<SegmentedFingerprints, DomainError> {
        let intro_fingerprint = fingerprinter.generate_fingerprint(&self.intro_buffer)?;
        let outro_fingerprint = fingerprinter.generate_fingerprint(&self.outro_buffer)?;
        Ok(SegmentedFingerprints {
            path: self.path,
            intro_fingerprint,
            outro_fingerprint,
        })
    }
}

impl SegmentedFingerprints {
    /// Returns a reference to the intro fingerprint.
    pub fn intro_fingerprint(&self) -> &[u32] {
        &self.intro_fingerprint
    }

    /// Returns a reference to the outro fingerprint.
    pub fn outro_fingerprint(&self) -> &[u32] {
        &self.outro_fingerprint
    }

    /// Returns a reference to the source path.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl ExtractedAudio {
    /// Transitions to FingerprintedMedia state by generating a fingerprint.
    pub fn generate_fingerprint<F: Fingerprinter>(
        self,
        fingerprinter: &F,
    ) -> Result<FingerprintedMedia, DomainError> {
        let fingerprint = fingerprinter.generate_fingerprint(&self.buffer)?;
        Ok(FingerprintedMedia {
            path: self.path,
            fingerprint,
        })
    }

    /// Returns a reference to the audio buffer.
    pub fn buffer(&self) -> &AudioBuffer {
        &self.buffer
    }
}

impl FingerprintedMedia {
    /// Returns a reference to the fingerprint.
    pub fn fingerprint(&self) -> &[u32] {
        &self.fingerprint
    }

    /// Returns a reference to the source path.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Transitions to MatchResult state by attempting to find a sub-fingerprint match.
    pub fn find_match<M: crate::domain::traits::FingerprintMatcher>(
        &self,
        matcher: &M,
        reference: &[u32],
        threshold: u32,
    ) -> Result<MatchResult, DomainError> {
        let match_index = matcher.find_match(reference, &self.fingerprint, threshold);
        Ok(MatchResult {
            path: self.path.clone(),
            match_index,
        })
    }
}
