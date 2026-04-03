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

/// Defines the search space heuristics for audio matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchSpace {
    /// Intro range: 0.0-0.25
    Intro,
    /// Outro range: 0.7-1.0
    Outro,
}

/// State after track selection.
#[derive(Debug)]
pub struct SelectedTrack {
    pub(crate) path: PathBuf,
    pub(crate) track_id: u32,
}

/// State after audio extraction: contains buffered audio for a specific search space.
#[derive(Debug)]
pub struct SegmentedAudio {
    pub(crate) path: PathBuf,
    pub(crate) buffer: AudioBuffer,
    pub(crate) space: SearchSpace,
    pub(crate) offset_sec: f64,
}

/// State after fingerprint generation: contains the fingerprint for a specific search space.
#[derive(Debug)]
pub struct SegmentedFingerprints {
    pub(crate) path: PathBuf,
    pub(crate) fingerprint: Vec<u32>,
    pub(crate) space: SearchSpace,
    pub(crate) offset_sec: f64,
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

    /// Transitions to SegmentedAudio state using a targeted search space heuristic.
    pub fn extract_segmented_audio<E: AudioExtractor>(
        self,
        extractor: &E,
        space: SearchSpace,
    ) -> Result<SegmentedAudio, DomainError> {
        let (start_percent, end_percent) = match space {
            SearchSpace::Intro => (0.0, 0.25),
            SearchSpace::Outro => (0.7, 1.0),
        };

        let total_duration = extractor.get_duration(&self.path, self.track_id)?;
        let offset_sec = total_duration * start_percent;

        let buffer = extractor.extract_audio_relative(
            &self.path,
            self.track_id,
            start_percent,
            end_percent,
        )?;

        Ok(SegmentedAudio {
            path: self.path,
            buffer,
            space,
            offset_sec,
        })
    }
}

impl SegmentedAudio {
    /// Transitions to SegmentedFingerprints state by generating a fingerprint for the segment.
    pub fn generate_segmented_fingerprints<F: Fingerprinter>(
        self,
        fingerprinter: &F,
    ) -> Result<SegmentedFingerprints, DomainError> {
        let fingerprint = fingerprinter.generate_fingerprint(&self.buffer)?;
        Ok(SegmentedFingerprints {
            path: self.path,
            fingerprint,
            space: self.space,
            offset_sec: self.offset_sec,
        })
    }
}

impl SegmentedFingerprints {
    /// Returns a reference to the fingerprint.
    pub fn fingerprint(&self) -> &[u32] {
        &self.fingerprint
    }

    /// Returns the search space.
    pub fn space(&self) -> SearchSpace {
        self.space
    }

    /// Returns the start offset of the search space in seconds.
    pub fn offset_sec(&self) -> f64 {
        self.offset_sec
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
