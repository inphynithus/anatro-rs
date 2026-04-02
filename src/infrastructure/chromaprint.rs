//! Chromaprint adapter for fingerprinting.

use crate::domain::DomainError;
use crate::domain::audio::AudioBuffer;
use crate::domain::traits::Fingerprinter;
use chromaprint::Chromaprint;

/// Adapter for Chromaprint operations.
#[derive(Debug, Default)]
pub struct ChromaprintAdapter;

impl ChromaprintAdapter {
    /// Creates a new ChromaprintAdapter.
    pub fn new() -> Self {
        Self
    }
}

impl Fingerprinter for ChromaprintAdapter {
    fn generate_fingerprint(&self, buffer: &AudioBuffer) -> Result<Vec<u32>, DomainError> {
        let mut ctx = Chromaprint::new();

        if !ctx.start(buffer.sample_rate as i32, buffer.channels as i32) {
            return Err(DomainError::FingerprintError(
                "Failed to start chromaprint".to_string(),
            ));
        }

        if !ctx.feed(buffer.samples()) {
            return Err(DomainError::FingerprintError(
                "Failed to feed samples to chromaprint".to_string(),
            ));
        }

        if !ctx.finish() {
            return Err(DomainError::FingerprintError(
                "Failed to finish chromaprint".to_string(),
            ));
        }

        let raw_fingerprint = ctx.raw_fingerprint().ok_or_else(|| {
            DomainError::FingerprintError("Failed to get raw fingerprint".to_string())
        })?;

        // Convert Vec<i32> to Vec<u32>
        let u32_fingerprint = raw_fingerprint.into_iter().map(|x| x as u32).collect();

        Ok(u32_fingerprint)
    }
}
