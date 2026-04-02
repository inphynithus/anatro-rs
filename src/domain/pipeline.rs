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

impl SourceMedia {
    /// Creates a new SourceMedia state.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Transitions to ExtractedAudio state by extracting audio using the provided extractor.
    pub fn extract_audio<E: AudioExtractor>(
        self,
        extractor: &E,
    ) -> Result<ExtractedAudio, DomainError> {
        let buffer = extractor.extract_audio(&self.path)?;
        Ok(ExtractedAudio {
            path: self.path,
            buffer,
        })
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
}
