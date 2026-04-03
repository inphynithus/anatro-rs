//! Symphonia adapter for audio extraction.

use crate::domain::DomainError;
use crate::domain::audio::AudioBuffer;
use crate::domain::traits::{AudioExtractor, PcmExporter, PcmExtractor, SampleExporter};
use dialoguer::Select;
use log::{debug, warn};
use rubato::{
    Async, FixedAsync, Resampler, SincInterpolationParameters, SincInterpolationType,
    WindowFunction, audioadapter_buffers::direct::SequentialSliceOfVecs,
};
use std::fs::File;
use std::io::Write;
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;

/// Target sample rate for Chromaprint.
const TARGET_SAMPLE_RATE: u32 = 11025;
/// Mono channel.
const TARGET_CHANNELS: u16 = 1;

/// Adapter for Symphonia operations.
#[derive(Debug, Default)]
pub struct SymphoniaAdapter;

impl SymphoniaAdapter {
    /// Creates a new SymphoniaAdapter.
    pub fn new() -> Self {
        Self
    }

    /// Selects an audio stream from the media file.
    fn select_audio_stream(&self, format: &dyn FormatReader) -> Result<u32, DomainError> {
        let tracks: Vec<_> = format
            .tracks()
            .iter()
            .filter(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .collect();

        if tracks.is_empty() {
            return Err(DomainError::ExtractionError(
                "No audio tracks found".to_string(),
            ));
        }

        if tracks.len() == 1 {
            return Ok(tracks[0].id);
        }

        // Multiple streams: prompt user
        let options: Vec<String> = tracks
            .iter()
            .map(|t| {
                format!(
                    "Track {}: {} channels, {} Hz",
                    t.id,
                    t.codec_params.channels.map(|c| c.count()).unwrap_or(0),
                    t.codec_params.sample_rate.unwrap_or(0)
                )
            })
            .collect();

        let selection = Select::new()
            .with_prompt("Multiple audio tracks found. Please select one:")
            .items(&options)
            .default(0)
            .interact()
            .map_err(|e| DomainError::InputError(e.to_string()))?;

        Ok(tracks[selection].id)
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

    /// Helper to probe a file and return format, track ID, native rate, and native channels.
    fn probe_file(
        &self,
        path: &Path,
    ) -> Result<(Box<dyn FormatReader>, u32, u32, u16), DomainError> {
        let file = File::open(path)
            .map_err(|e| DomainError::ExtractionError(format!("Failed to open file: {}", e)))?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }

        let probed = symphonia::default::get_probe()
            .format(
                &hint,
                mss,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .map_err(|e| DomainError::ExtractionError(format!("Failed to probe format: {}", e)))?;

        let format = probed.format;
        let track_id = self.select_audio_stream(format.as_ref())?;

        let track = format
            .tracks()
            .iter()
            .find(|t| t.id == track_id)
            .ok_or_else(|| DomainError::ExtractionError("Selected track not found".to_string()))?;

        let native_rate = track
            .codec_params
            .sample_rate
            .ok_or_else(|| DomainError::ExtractionError("Track has no sample rate".to_string()))?;
        let native_channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(1) as u16;

        Ok((format, track_id, native_rate, native_channels))
    }
}

impl AudioExtractor for SymphoniaAdapter {
    fn extract_audio(&self, path: &Path) -> Result<AudioBuffer, DomainError> {
        debug!("Initializing Symphonia for full track extraction with resampling");
        let (mut format, track_id, native_rate, native_channels) = self.probe_file(path)?;

        let track = format.tracks().iter().find(|t| t.id == track_id).unwrap();

        let mut decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .map_err(|e| {
                DomainError::ExtractionError(format!("Failed to create decoder: {}", e))
            })?;

        // Rubato resampler setup
        let resample_ratio = TARGET_SAMPLE_RATE as f64 / native_rate as f64;
        let params = SincInterpolationParameters {
            sinc_len: 256,
            f_cutoff: 0.95,
            interpolation: SincInterpolationType::Linear,
            oversampling_factor: 128,
            window: WindowFunction::BlackmanHarris2,
        };

        let chunk_size = 1024;
        let mut resampler = Async::<f32>::new_sinc(
            resample_ratio,
            2.0, // max resample ratio relative
            &params,
            chunk_size,
            TARGET_CHANNELS as usize,
            FixedAsync::Input,
        )
        .map_err(|e| DomainError::ExtractionError(format!("Failed to create resampler: {}", e)))?;

        let mut audio_samples: Vec<i16> = Vec::new();
        let mut sample_buf = None;

        // Internal buffer for rubato (planar format)
        let mut resampler_input_data = vec![vec![0.0f32; chunk_size]; TARGET_CHANNELS as usize];
        let mut input_pos = 0;

        loop {
            let packet = match format.next_packet() {
                Ok(packet) => packet,
                Err(SymphoniaError::IoError(err))
                    if err.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    break;
                }
                Err(err) => return Err(DomainError::ExtractionError(err.to_string())),
            };

            if packet.track_id() != track_id {
                continue;
            }

            match decoder.decode(&packet) {
                Ok(audio_buf) => {
                    if sample_buf.is_none() {
                        let spec = *audio_buf.spec();
                        let duration = audio_buf.capacity() as u64;
                        sample_buf = Some(SampleBuffer::<f32>::new(duration, spec));
                    }

                    if let Some(buf) = &mut sample_buf {
                        buf.copy_interleaved_ref(audio_buf);
                        let samples = buf.samples();

                        let mut i = 0;
                        while i < samples.len() {
                            // Mix down to mono if necessary
                            let mut mono_sample = 0.0f32;
                            for _ in 0..native_channels {
                                if i < samples.len() {
                                    mono_sample += samples[i];
                                    i += 1;
                                }
                            }
                            mono_sample /= native_channels as f32;

                            resampler_input_data[0][input_pos] = mono_sample;
                            input_pos += 1;

                            if input_pos == chunk_size {
                                let input_adapter = SequentialSliceOfVecs::new(
                                    &resampler_input_data,
                                    TARGET_CHANNELS as usize,
                                    chunk_size,
                                )
                                .unwrap();

                                let resampled =
                                    resampler.process(&input_adapter, 0, None).map_err(|e| {
                                        DomainError::ExtractionError(format!(
                                            "Resampling error: {}",
                                            e
                                        ))
                                    })?;

                                for s in resampled.take_data() {
                                    audio_samples
                                        .push((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
                                }
                                input_pos = 0;
                            }
                        }
                    }
                }
                Err(SymphoniaError::DecodeError(err)) => {
                    warn!("Decode error: {}", err);
                    continue;
                }
                Err(err) => return Err(DomainError::ExtractionError(err.to_string())),
            }
        }

        // Flush remaining samples in resampler if any
        if input_pos > 0 {
            // Pad with zeros to complete the chunk
            for sample in resampler_input_data[0].iter_mut().skip(input_pos) {
                *sample = 0.0;
            }
            let input_adapter = SequentialSliceOfVecs::new(
                &resampler_input_data,
                TARGET_CHANNELS as usize,
                chunk_size,
            )
            .unwrap();

            let resampled = resampler.process(&input_adapter, 0, None).map_err(|e| {
                DomainError::ExtractionError(format!("Resampling error during flush: {}", e))
            })?;

            // Only take the relevant part of the resampled buffer
            let resampled_count = (input_pos as f64 * resample_ratio).round() as usize;
            for s in resampled.take_data().into_iter().take(resampled_count) {
                audio_samples.push((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
            }
        }

        Ok(AudioBuffer::new(
            audio_samples,
            TARGET_SAMPLE_RATE,
            TARGET_CHANNELS,
        ))
    }
}

impl PcmExtractor for SymphoniaAdapter {
    fn extract_pcm_secs(
        &self,
        path: &Path,
        start_sec: f64,
        end_sec: f64,
    ) -> Result<AudioBuffer, DomainError> {
        debug!(
            "Initializing Symphonia for PCM extraction: start {:.3}s, end {:.3}s",
            start_sec, end_sec
        );

        if start_sec >= end_sec {
            return Err(DomainError::InputError(
                "End time must be after start time".to_string(),
            ));
        }

        let (mut format, track_id, native_rate, native_channels) = self.probe_file(path)?;

        let track = format.tracks().iter().find(|t| t.id == track_id).unwrap();

        let time_base = track
            .codec_params
            .time_base
            .ok_or_else(|| DomainError::ExtractionError("Track has no time base".to_string()))?;

        let mut decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .map_err(|e| {
                DomainError::ExtractionError(format!("Failed to create decoder: {}", e))
            })?;

        // Calculate timestamps manually using time_base (which is denom / numer seconds)
        let start_pts =
            (start_sec * time_base.denom as f64 / time_base.numer as f64).round() as u64;
        let end_pts = (end_sec * time_base.denom as f64 / time_base.numer as f64).round() as u64;

        debug!("Target PTS range: {} to {}", start_pts, end_pts);

        // Seek backward
        format
            .seek(
                SeekMode::Coarse,
                SeekTo::Time {
                    time: Time::from(start_sec),
                    track_id: Some(track_id),
                },
            )
            .map_err(|e| DomainError::ExtractionError(format!("Seek failed: {}", e)))?;

        let mut audio_samples: Vec<i16> = Vec::new();
        let mut sample_buf = None;

        loop {
            let packet = match format.next_packet() {
                Ok(packet) => packet,
                Err(SymphoniaError::IoError(err))
                    if err.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    break;
                }
                Err(err) => return Err(DomainError::ExtractionError(err.to_string())),
            };

            if packet.track_id() != track_id {
                continue;
            }

            let packet_pts = packet.ts();
            let packet_dur = packet.dur();

            if packet_pts + packet_dur < start_pts {
                // Entire packet is before start_pts
                continue;
            }

            if packet_pts >= end_pts {
                // Packet is after end_pts, we're done
                break;
            }

            match decoder.decode(&packet) {
                Ok(audio_buf) => {
                    let mut current_pts = packet_pts;

                    if sample_buf.is_none() {
                        let spec = *audio_buf.spec();
                        let duration = audio_buf.capacity() as u64;
                        sample_buf = Some(SampleBuffer::<i16>::new(duration, spec));
                    }

                    if let Some(buf) = &mut sample_buf {
                        buf.copy_interleaved_ref(audio_buf);
                        let samples = buf.samples();

                        let mut sample_idx = 0;
                        while sample_idx < samples.len() {
                            let frame_pts = current_pts;

                            // A Symphonia packet usually contains multiple frames.
                            // Calculate ticks per frame using time_base and native_rate.
                            let time_base_factor = (native_rate as f64 * time_base.numer as f64
                                / time_base.denom as f64)
                                .round() as u64;
                            let ticks_per_frame = time_base_factor.max(1);

                            if frame_pts >= end_pts {
                                break;
                            }

                            let end_idx =
                                (sample_idx + native_channels as usize).min(samples.len());
                            let frame_samples = &samples[sample_idx..end_idx];
                            sample_idx = end_idx;

                            if frame_pts >= start_pts {
                                audio_samples.extend_from_slice(frame_samples);
                            }

                            current_pts += ticks_per_frame;
                        }
                    }
                }
                Err(SymphoniaError::DecodeError(err)) => {
                    warn!("Decode error: {}", err);
                    continue;
                }
                Err(err) => return Err(DomainError::ExtractionError(err.to_string())),
            }
        }

        Ok(AudioBuffer::new(
            audio_samples,
            native_rate,
            native_channels,
        ))
    }

    fn extract_pcm_range(&self, path: &Path, range: &str) -> Result<AudioBuffer, DomainError> {
        let (start_hms, end_hms) = self.parse_range(range)?;
        let start_sec = self.hms_to_seconds(start_hms)?;
        let end_sec = self.hms_to_seconds(end_hms)?;
        self.extract_pcm_secs(path, start_sec, end_sec)
    }

    fn extract_pcm_relative(
        &self,
        path: &Path,
        start_percent: f64,
        end_percent: f64,
    ) -> Result<AudioBuffer, DomainError> {
        let (format, track_id, _, _) = self.probe_file(path)?;

        let track = format.tracks().iter().find(|t| t.id == track_id).unwrap();

        let time_base = track
            .codec_params
            .time_base
            .unwrap_or(symphonia::core::units::TimeBase::new(1, 1));
        let duration_frames = track.codec_params.n_frames.unwrap_or(0);
        let duration_secs = time_base.calc_time(duration_frames).seconds as f64
            + time_base.calc_time(duration_frames).frac;

        if duration_secs == 0.0 {
            return Err(DomainError::ExtractionError(
                "Could not determine track duration".to_string(),
            ));
        }

        let start_sec = duration_secs * start_percent;
        let end_sec = duration_secs * end_percent;

        self.extract_pcm_secs(path, start_sec, end_sec)
    }
}

impl PcmExporter for SymphoniaAdapter {
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

        // Write PCM data (little-endian)
        for &sample in &buffer.samples {
            file.write_all(&sample.to_le_bytes())
                .map_err(|e| DomainError::MediaError(e.to_string()))?;
        }

        file.flush()
            .map_err(|e| DomainError::MediaError(e.to_string()))?;
        Ok(())
    }
}

impl SampleExporter for SymphoniaAdapter {
    fn export_sample(&self, input: &Path, output: &Path, range: &str) -> Result<(), DomainError> {
        let buffer = if range.contains('-') {
            // It's a standard HMS range
            self.extract_pcm_range(input, range)?
        } else if range.contains(',') {
            // It's a relative range e.g. 0.25,0.4
            let parts: Vec<&str> = range.split(',').collect();
            if parts.len() != 2 {
                return Err(DomainError::InputError(
                    "Relative range must be 'start,end' floats".to_string(),
                ));
            }
            let start_percent: f64 = parts[0]
                .parse()
                .map_err(|_| DomainError::InputError("Invalid start float".to_string()))?;
            let end_percent: f64 = parts[1]
                .parse()
                .map_err(|_| DomainError::InputError("Invalid end float".to_string()))?;
            self.extract_pcm_relative(input, start_percent, end_percent)?
        } else {
            return Err(DomainError::InputError(
                "Range format not recognized".to_string(),
            ));
        };

        self.export_wav(&buffer, output)?;

        Ok(())
    }
}
