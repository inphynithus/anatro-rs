//! Audio types for the domain layer.

/// A buffer containing raw audio samples (typically i16 for Chromaprint).
#[derive(Debug, Clone)]
pub struct AudioBuffer {
    /// The actual audio samples.
    pub samples: Vec<i16>,
    /// The sample rate of the audio (e.g., 11025).
    pub sample_rate: u32,
    /// The number of channels (typically 1 for mono).
    pub channels: u16,
}

impl AudioBuffer {
    /// Creates a new AudioBuffer.
    pub fn new(samples: Vec<i16>, sample_rate: u32, channels: u16) -> Self {
        Self {
            samples,
            sample_rate,
            channels,
        }
    }

    /// Returns a reference to the samples.
    pub fn samples(&self) -> &[i16] {
        &self.samples
    }
}
