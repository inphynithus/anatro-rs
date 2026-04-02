//! FFmpeg adapter for audio extraction.

use crate::domain::DomainError;
use crate::domain::audio::AudioBuffer;
use crate::domain::traits::AudioExtractor;
use dialoguer::Select;
use ffmpeg_next::format::context::Input;
use ffmpeg_next::format::sample::{Sample, Type};
use ffmpeg_next::software::resampling;
use ffmpeg_next::{frame, media, util};
use std::path::Path;

/// Target sample rate for Chromaprint.
const TARGET_SAMPLE_RATE: u32 = 11025;
/// Mono channel.
const TARGET_CHANNELS: u16 = 1;

/// Adapter for FFmpeg operations.
#[derive(Debug, Default)]
pub struct FfmpegAdapter;

impl FfmpegAdapter {
    /// Creates a new FfmpegAdapter.
    pub fn new() -> Self {
        Self
    }

    /// Selects an audio stream from the media file.
    fn select_audio_stream(&self, ictx: &Input) -> Result<usize, DomainError> {
        let streams: Vec<_> = ictx
            .streams()
            .filter(|s| s.parameters().medium() == media::Type::Audio)
            .collect();

        if streams.is_empty() {
            return Err(DomainError::ExtractionError(
                "No audio streams found".to_string(),
            ));
        }

        if streams.len() == 1 {
            return Ok(streams[0].index());
        }

        // Multiple streams: prompt user
        let options: Vec<String> = streams
            .iter()
            .map(|s| {
                // For simplicity, just show stream index.
                // In a real app, we might want to extract more info from the decoder context.
                format!("Stream {}: Audio", s.index())
            })
            .collect();

        let selection = Select::new()
            .with_prompt("Multiple audio tracks found. Please select one:")
            .items(&options)
            .default(0)
            .interact()
            .map_err(|e| DomainError::InputError(e.to_string()))?;

        Ok(streams[selection].index())
    }
}

impl AudioExtractor for FfmpegAdapter {
    fn extract_audio(&self, path: &Path) -> Result<AudioBuffer, DomainError> {
        ffmpeg_next::init().map_err(|e| DomainError::ExtractionError(e.to_string()))?;

        let mut ictx = ffmpeg_next::format::input(&path)
            .map_err(|e| DomainError::ExtractionError(format!("Failed to open file: {}", e)))?;

        let stream_index = self.select_audio_stream(&ictx)?;
        let stream = ictx
            .stream(stream_index)
            .ok_or_else(|| DomainError::ExtractionError("Invalid stream index".to_string()))?;

        let context = ffmpeg_next::codec::context::Context::from_parameters(stream.parameters())
            .map_err(|e| DomainError::ExtractionError(e.to_string()))?;
        let mut decoder = context
            .decoder()
            .audio()
            .map_err(|e| DomainError::ExtractionError(e.to_string()))?;

        let mut resampler = resampling::context::Context::get(
            decoder.format(),
            decoder.channel_layout(),
            decoder.rate(),
            Sample::I16(Type::Packed),
            util::channel_layout::ChannelLayout::MONO,
            TARGET_SAMPLE_RATE,
        )
        .map_err(|e| DomainError::ExtractionError(format!("Failed to create resampler: {}", e)))?;

        let mut audio_samples: Vec<i16> = Vec::new();
        let mut decoded = frame::Audio::empty();

        for (stream, packet) in ictx.packets() {
            if stream.index() == stream_index {
                decoder
                    .send_packet(&packet)
                    .map_err(|e| DomainError::ExtractionError(e.to_string()))?;
                while decoder.receive_frame(&mut decoded).is_ok() {
                    let mut resampled = frame::Audio::empty();
                    resampled.set_format(Sample::I16(Type::Packed));
                    resampled.set_channel_layout(util::channel_layout::ChannelLayout::MONO);
                    resampled.set_rate(TARGET_SAMPLE_RATE);

                    resampler.run(&decoded, &mut resampled).map_err(|e| {
                        DomainError::ExtractionError(format!("Resampling failed: {}", e))
                    })?;

                    let data = resampled.data(0);
                    // SAFETY: resampled.data(0) is a byte slice, we need to convert it to i16 slice
                    let i16_data = unsafe {
                        std::slice::from_raw_parts(data.as_ptr() as *const i16, data.len() / 2)
                    };
                    audio_samples.extend_from_slice(i16_data);
                }
            }
        }

        Ok(AudioBuffer::new(
            audio_samples,
            TARGET_SAMPLE_RATE,
            TARGET_CHANNELS,
        ))
    }
}
