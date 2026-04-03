//! FFmpeg adapter for audio extraction.

use crate::domain::DomainError;
use crate::domain::audio::AudioBuffer;
use crate::domain::traits::{AudioExtractor, PcmExporter, PcmExtractor, SampleExporter};
use dialoguer::Select;
use ffmpeg_next::format::context::Input;
use ffmpeg_next::format::sample::{Sample, Type};
use ffmpeg_next::software::resampling;
use ffmpeg_next::util::mathematics::Rescale;
use ffmpeg_next::{codec, frame, media, util};
use log::{debug, info};
use std::fs::File;
use std::io::Write;
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

    /// Parses a timestamp range in the format 'HH:MM:SS-HH:MM:SS' into (start, end) strings.
    fn parse_range<'a>(&self, range: &'a str) -> Result<(&'a str, &'a str), DomainError> {
        let parts: Vec<&str> = range.split('-').collect();
        if parts.len() != 2 {
            return Err(DomainError::InputError(
                "Range must be in 'HH:MM:SS-HH:MM:SS' format".to_string(),
            ));
        }
        Ok((parts[0], parts[1]))
    }

    /// Parses HH:MM:SS to seconds.
    fn hms_to_seconds(&self, hms: &str) -> Result<f64, DomainError> {
        let parts: Vec<&str> = hms.split(':').collect();
        if parts.len() != 3 {
            return Err(DomainError::InputError(format!(
                "Invalid timestamp format: {}. Expected HH:MM:SS",
                hms
            )));
        }
        let h: f64 = parts[0].parse().map_err(|_| {
            DomainError::InputError(format!("Invalid hours in timestamp: {}", parts[0]))
        })?;
        let m: f64 = parts[1].parse().map_err(|_| {
            DomainError::InputError(format!("Invalid minutes in timestamp: {}", parts[1]))
        })?;
        let s: f64 = parts[2].parse().map_err(|_| {
            DomainError::InputError(format!("Invalid seconds in timestamp: {}", parts[2]))
        })?;
        Ok(h * 3600.0 + m * 60.0 + s)
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

        let context = codec::context::Context::from_parameters(stream.parameters())
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

impl PcmExtractor for FfmpegAdapter {
    fn extract_pcm_range(
        &self,
        path: &Path,
        start: &str,
        duration: f64,
    ) -> Result<AudioBuffer, DomainError> {
        debug!("Initializing FFmpeg for sample-accurate PCM extraction");
        ffmpeg_next::init().map_err(|e| DomainError::ExtractionError(e.to_string()))?;

        let start_secs = self.hms_to_seconds(start)?;
        debug!(
            "Start seconds: {:.3}s, Duration: {:.3}s",
            start_secs, duration
        );

        let mut ictx = ffmpeg_next::format::input(&path)
            .map_err(|e| DomainError::ExtractionError(format!("Failed to open file: {}", e)))?;

        let (stream_index, in_time_base) = {
            let stream = ictx
                .streams()
                .best(media::Type::Audio)
                .ok_or_else(|| DomainError::ExtractionError("No audio stream found".to_string()))?;
            (stream.index(), stream.time_base())
        };

        info!("Selected input stream index: {}", stream_index);

        let decoder_params = ictx.stream(stream_index).unwrap().parameters();
        let decoder_context = codec::context::Context::from_parameters(decoder_params)
            .map_err(|e| DomainError::ExtractionError(e.to_string()))?;
        let mut decoder = decoder_context
            .decoder()
            .audio()
            .map_err(|e| DomainError::ExtractionError(e.to_string()))?;

        let native_rate = decoder.rate();
        let native_channels = decoder.channel_layout().channels();

        // Target: i16 Packed, native rate and channels
        let mut resampler = resampling::context::Context::get(
            decoder.format(),
            decoder.channel_layout(),
            decoder.rate(),
            Sample::I16(Type::Packed),
            decoder.channel_layout(),
            decoder.rate(),
        )
        .map_err(|e| DomainError::ExtractionError(format!("Failed to create resampler: {}", e)))?;

        let target_pts = (start_secs / f64::from(in_time_base)).round() as i64;
        debug!("Target PTS: {}", target_pts);

        // Seek backward to ensure we start before the target point
        ictx.seek(target_pts, ..target_pts)
            .map_err(|e| DomainError::ExtractionError(format!("Seek failed: {}", e)))?;

        let mut audio_samples: Vec<i16> = Vec::new();
        let mut decoded = frame::Audio::empty();
        let mut resampled = frame::Audio::empty();

        let target_sample_count = (duration * native_rate as f64).round() as usize;
        let mut collected_samples = 0usize;

        for (stream, in_packet) in ictx.packets() {
            if stream.index() == stream_index {
                decoder
                    .send_packet(&in_packet)
                    .map_err(|e| DomainError::ExtractionError(e.to_string()))?;

                while decoder.receive_frame(&mut decoded).is_ok() {
                    let frame_pts = decoded.pts().unwrap_or(0);
                    let frame_nb_samples = decoded.samples();
                    // Duration of frame in stream time base
                    let frame_duration_pts = (frame_nb_samples as i64).rescale(
                        util::rational::Rational::new(1, native_rate as i32),
                        in_time_base,
                    );
                    let frame_end_pts = frame_pts + frame_duration_pts;

                    if frame_end_pts < target_pts {
                        // Skip entire frame
                        continue;
                    }

                    resampler
                        .run(&decoded, &mut resampled)
                        .map_err(|e| DomainError::ExtractionError(e.to_string()))?;

                    let resampled_data = resampled.data(0);
                    let samples_ptr = resampled_data.as_ptr() as *const i16;
                    let total_frame_samples = resampled.samples(); // Number of samples PER CHANNEL

                    let (start_sample_offset, samples_to_copy) = if frame_pts < target_pts {
                        // Partial frame: calculate how many samples to skip
                        let pts_to_skip = target_pts - frame_pts;
                        let samples_to_skip = pts_to_skip.rescale(
                            in_time_base,
                            util::rational::Rational::new(1, native_rate as i32),
                        ) as usize;

                        let to_copy = total_frame_samples.saturating_sub(samples_to_skip);
                        (samples_to_skip, to_copy)
                    } else {
                        // Full frame or starting after target_pts
                        (0, total_frame_samples)
                    };

                    if samples_to_copy > 0 {
                        let actual_to_copy =
                            if collected_samples + samples_to_copy > target_sample_count {
                                target_sample_count - collected_samples
                            } else {
                                samples_to_copy
                            };

                        if actual_to_copy > 0 {
                            let channels = native_channels as usize;
                            let start_idx = start_sample_offset * channels;
                            let count = actual_to_copy * channels;

                            // SAFETY: resampled.data(0) is valid for total_frame_samples * channels
                            let i16_slice = unsafe {
                                std::slice::from_raw_parts(samples_ptr.add(start_idx), count)
                            };
                            audio_samples.extend_from_slice(i16_slice);
                            collected_samples += actual_to_copy;
                        }
                    }

                    if collected_samples >= target_sample_count {
                        break;
                    }
                }

                if collected_samples >= target_sample_count {
                    break;
                }
            }
        }

        debug!(
            "Extracted {} samples ({} per channel)",
            audio_samples.len(),
            collected_samples
        );

        Ok(AudioBuffer::new(
            audio_samples,
            native_rate,
            native_channels as u16,
        ))
    }
}

impl PcmExporter for FfmpegAdapter {
    fn export_wav(&self, buffer: &AudioBuffer, output: &Path) -> Result<(), DomainError> {
        let mut file = File::create(output)
            .map_err(|e| DomainError::MediaError(format!("Failed to create WAV file: {}", e)))?;

        let channels = buffer.channels;
        let sample_rate = buffer.sample_rate;
        let bits_per_sample = 16u16;
        let data_size = (buffer.samples.len() * 2) as u32; // i16 = 2 bytes
        let file_size = 36 + data_size;

        // RIFF header
        file.write_all(b"RIFF")
            .map_err(|e| DomainError::MediaError(e.to_string()))?;
        file.write_all(&file_size.to_le_bytes())
            .map_err(|e| DomainError::MediaError(e.to_string()))?;
        file.write_all(b"WAVE")
            .map_err(|e| DomainError::MediaError(e.to_string()))?;

        // fmt chunk
        file.write_all(b"fmt ")
            .map_err(|e| DomainError::MediaError(e.to_string()))?;
        file.write_all(&16u32.to_le_bytes())
            .map_err(|e| DomainError::MediaError(e.to_string()))?; // chunk size
        file.write_all(&1u16.to_le_bytes())
            .map_err(|e| DomainError::MediaError(e.to_string()))?; // PCM format
        file.write_all(&channels.to_le_bytes())
            .map_err(|e| DomainError::MediaError(e.to_string()))?;
        file.write_all(&sample_rate.to_le_bytes())
            .map_err(|e| DomainError::MediaError(e.to_string()))?;
        let byte_rate = sample_rate * channels as u32 * (bits_per_sample / 8) as u32;
        file.write_all(&byte_rate.to_le_bytes())
            .map_err(|e| DomainError::MediaError(e.to_string()))?;
        let block_align = channels * (bits_per_sample / 8);
        file.write_all(&block_align.to_le_bytes())
            .map_err(|e| DomainError::MediaError(e.to_string()))?;
        file.write_all(&bits_per_sample.to_le_bytes())
            .map_err(|e| DomainError::MediaError(e.to_string()))?;

        // data chunk
        file.write_all(b"data")
            .map_err(|e| DomainError::MediaError(e.to_string()))?;
        file.write_all(&data_size.to_le_bytes())
            .map_err(|e| DomainError::MediaError(e.to_string()))?;

        // Write PCM data
        // We need to convert Vec<i16> to Vec<u8> (little-endian)
        for &sample in &buffer.samples {
            file.write_all(&sample.to_le_bytes())
                .map_err(|e| DomainError::MediaError(e.to_string()))?;
        }

        file.flush()
            .map_err(|e| DomainError::MediaError(e.to_string()))?;
        Ok(())
    }
}

impl SampleExporter for FfmpegAdapter {
    fn export_sample(&self, input: &Path, output: &Path, range: &str) -> Result<(), DomainError> {
        let (start_hms, end_hms) = self.parse_range(range)?;
        let start_secs = self.hms_to_seconds(start_hms)?;
        let end_secs = self.hms_to_seconds(end_hms)?;
        let duration = end_secs - start_secs;

        if duration <= 0.0 {
            return Err(DomainError::InputError(
                "End time must be after start time".to_string(),
            ));
        }

        let buffer = self.extract_pcm_range(input, start_hms, duration)?;
        self.export_wav(&buffer, output)?;

        Ok(())
    }
}
