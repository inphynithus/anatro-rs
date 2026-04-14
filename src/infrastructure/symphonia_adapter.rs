//! Symphonia adapter for audio extraction.

use crate::domain::DomainError;
use crate::domain::audio::AudioBuffer;
use crate::domain::traits::{AudioExtractor, SampleExporter, TrackSelector};
use dialoguer::Select;
use log::{debug, info, warn};
use rubato::{
    Async, FixedAsync, Resampler, SincInterpolationParameters, SincInterpolationType,
    WindowFunction,
};
use std::fs::File;
use std::io::Write;
use std::path::Path;
use symphonia::core::audio::{SampleBuffer, SignalSpec};
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

    /// Parses HH:MM:SS or a direct float string to seconds.
    pub fn hms_to_seconds(&self, hms: &str) -> Result<f64, DomainError> {
        if !hms.contains(':') {
            return hms.parse::<f64>().map_err(|_| {
                DomainError::InputError(format!(
                    "Invalid timestamp format: {}. Expected HH:MM:SS or seconds as float.",
                    hms
                ))
            });
        }

        let parts: Vec<&str> = hms.split(':').collect();
        match parts.len() {
            3 => {
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
            2 => {
                let m: f64 = parts[0].parse().map_err(|_| {
                    DomainError::InputError(format!("Invalid minutes in timestamp: {}", parts[0]))
                })?;
                let s: f64 = parts[1].parse().map_err(|_| {
                    DomainError::InputError(format!("Invalid seconds in timestamp: {}", parts[1]))
                })?;
                Ok(m * 60.0 + s)
            }
            _ => Err(DomainError::InputError(format!(
                "Invalid timestamp format: {}. Expected HH:MM:SS, MM:SS or seconds as float.",
                hms
            ))),
        }
    }

    /// Helper to probe a file and return format.
    fn probe_file(&self, path: &Path) -> Result<Box<dyn FormatReader>, DomainError> {
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

        Ok(probed.format)
    }

    /// Internal method to extract and optionally resample audio.
    fn extract_audio_range_internal(
        &self,
        mut format: Box<dyn FormatReader>,
        track_id: u32,
        start_sec: f64,
        end_sec: f64,
    ) -> Result<AudioBuffer, DomainError> {
        let track = format
            .tracks()
            .iter()
            .find(|t| t.id == track_id)
            .ok_or_else(|| DomainError::ExtractionError("Track not found".to_string()))?;

        let mut decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .map_err(|e| {
                DomainError::ExtractionError(format!("Failed to create decoder: {}", e))
            })?;

        let time_base = track
            .codec_params
            .time_base
            .unwrap_or(symphonia::core::units::TimeBase::new(1, 1));
        let start_pts =
            (start_sec * time_base.denom as f64 / time_base.numer as f64).round() as u64;
        let end_pts = (end_sec * time_base.denom as f64 / time_base.numer as f64).round() as u64;

        format
            .seek(
                SeekMode::Coarse,
                SeekTo::Time {
                    time: Time::from(start_sec),
                    track_id: Some(track_id),
                },
            )
            .map_err(|e| DomainError::ExtractionError(format!("Seek failed: {}", e)))?;

        let mut raw_samples: Vec<f32> = Vec::new();
        let mut actual_spec: Option<SignalSpec> = None;

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
            if packet_pts >= end_pts {
                break;
            }

            match decoder.decode(&packet) {
                Ok(audio_buf) => {
                    if actual_spec.is_none() {
                        actual_spec = Some(*audio_buf.spec());
                    }
                    let spec = actual_spec.unwrap_or_else(|| *audio_buf.spec());
                    let mut sample_buf =
                        SampleBuffer::<f32>::new(audio_buf.capacity() as u64, spec);
                    sample_buf.copy_interleaved_ref(audio_buf);
                    let samples = sample_buf.samples();

                    let channels = spec.channels.count();
                    let frames = samples.len() / channels;

                    for f in 0..frames {
                        let frame_pts = packet_pts + f as u64;
                        if frame_pts >= start_pts && frame_pts < end_pts {
                            raw_samples
                                .extend_from_slice(&samples[f * channels..(f + 1) * channels]);
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

        let spec = actual_spec.ok_or_else(|| {
            DomainError::ExtractionError("No frames decoded to determine spec".to_string())
        })?;

        // Downmix to Mono if necessary
        let mono_samples: Vec<f32> = if spec.channels.count() > 1 {
            let channels = spec.channels.count();
            raw_samples
                .chunks_exact(channels)
                .map(|chunk| chunk.iter().sum::<f32>() / channels as f32)
                .collect()
        } else {
            raw_samples
        };

        // Resample to 11025Hz
        let resampled_samples = if spec.rate != TARGET_SAMPLE_RATE {
            let params = SincInterpolationParameters {
                sinc_len: 256,
                f_cutoff: 0.95,
                interpolation: SincInterpolationType::Cubic,
                window: WindowFunction::BlackmanHarris2,
                oversampling_factor: 128,
            };
            let mut resampler = Async::<f32>::new_sinc(
                TARGET_SAMPLE_RATE as f64 / spec.rate as f64,
                1.0,
                &params,
                mono_samples.len(),
                1,
                FixedAsync::Input,
            )
            .map_err(|e| {
                DomainError::ExtractionError(format!("Failed to create resampler: {}", e))
            })?;

            use rubato::audioadapter_buffers::direct::SequentialSliceOfVecs;

            let input_data = vec![mono_samples];
            let input = SequentialSliceOfVecs::new(&input_data, 1, input_data[0].len())
                .map_err(|e| DomainError::ExtractionError(e.to_string()))?;

            let resampled = resampler
                .process(&input, 0, None)
                .map_err(|e| DomainError::ExtractionError(format!("Resampling failed: {}", e)))?;

            // In rubato 2.0.0, process returns InterleavedOwned<T>
            resampled.take_data()
        } else {
            mono_samples
        };

        // Convert to i16
        let audio_samples: Vec<i16> = resampled_samples
            .into_iter()
            .map(|s: f32| (s * i16::MAX as f32).clamp(i16::MIN as f32, i16::MAX as f32) as i16)
            .collect();

        Ok(AudioBuffer::new(
            audio_samples,
            TARGET_SAMPLE_RATE,
            TARGET_CHANNELS,
        ))
    }

    /// Exports the audio buffer to a WAV file.
    pub fn export_wav(&self, buffer: &AudioBuffer, output: &Path) -> Result<(), DomainError> {
        let mut file = File::create(output)
            .map_err(|e| DomainError::MediaError(format!("Failed to create WAV file: {}", e)))?;

        let channels = buffer.channels;
        let sample_rate = buffer.sample_rate;
        let bits_per_sample = 16u16;
        let data_size = (buffer.samples.len() * 2) as u32;
        let file_size = 36 + data_size;

        file.write_all(b"RIFF")
            .map_err(|e| DomainError::MediaError(e.to_string()))?;
        file.write_all(&file_size.to_le_bytes())
            .map_err(|e| DomainError::MediaError(e.to_string()))?;
        file.write_all(b"WAVE")
            .map_err(|e| DomainError::MediaError(e.to_string()))?;
        file.write_all(b"fmt ")
            .map_err(|e| DomainError::MediaError(e.to_string()))?;
        file.write_all(&16u32.to_le_bytes())
            .map_err(|e| DomainError::MediaError(e.to_string()))?;
        file.write_all(&1u16.to_le_bytes())
            .map_err(|e| DomainError::MediaError(e.to_string()))?;
        file.write_all(&channels.to_le_bytes())
            .map_err(|e| DomainError::MediaError(e.to_string()))?;
        file.write_all(&sample_rate.to_le_bytes())
            .map_err(|e| DomainError::MediaError(e.to_string()))?;
        let byte_rate = sample_rate * channels as u32 * 2;
        file.write_all(&byte_rate.to_le_bytes())
            .map_err(|e| DomainError::MediaError(e.to_string()))?;
        let block_align = channels * 2;
        file.write_all(&block_align.to_le_bytes())
            .map_err(|e| DomainError::MediaError(e.to_string()))?;
        file.write_all(&bits_per_sample.to_le_bytes())
            .map_err(|e| DomainError::MediaError(e.to_string()))?;
        file.write_all(b"data")
            .map_err(|e| DomainError::MediaError(e.to_string()))?;
        file.write_all(&data_size.to_le_bytes())
            .map_err(|e| DomainError::MediaError(e.to_string()))?;

        for &sample in &buffer.samples {
            file.write_all(&sample.to_le_bytes())
                .map_err(|e| DomainError::MediaError(e.to_string()))?;
        }

        file.flush()
            .map_err(|e| DomainError::MediaError(e.to_string()))?;
        Ok(())
    }
}

impl TrackSelector for SymphoniaAdapter {
    fn prompt_track_index(&self, path: &Path) -> Result<usize, DomainError> {
        let format = self.probe_file(path)?;

        // Filter only audio tracks
        let tracks: Vec<_> = format
            .tracks()
            .iter()
            .filter(|t| {
                t.codec_params.codec != CODEC_TYPE_NULL
                    && (t.codec_params.channels.is_some() || t.codec_params.sample_rate.is_some())
            })
            .collect();

        if tracks.is_empty() {
            return Err(DomainError::ExtractionError(
                "No audio tracks found".to_string(),
            ));
        }

        if tracks.len() == 1 {
            return Ok(0);
        }

        let options: Vec<String> = tracks
            .iter()
            .map(|t| {
                let lang = t.language.as_deref().unwrap_or("Unknown");
                format!(
                    "Track {} ({}): {} channels, {} Hz",
                    t.id,
                    lang,
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

        Ok(selection)
    }

    fn select_track(&self, path: &Path, track_index: Option<usize>) -> Result<u32, DomainError> {
        let format = self.probe_file(path)?;

        // Filter only audio tracks
        let tracks: Vec<_> = format
            .tracks()
            .iter()
            .filter(|t| {
                t.codec_params.codec != CODEC_TYPE_NULL
                    && (t.codec_params.channels.is_some() || t.codec_params.sample_rate.is_some())
            })
            .collect();

        if tracks.is_empty() {
            return Err(DomainError::ExtractionError(
                "No audio tracks found".to_string(),
            ));
        }

        let idx = match track_index {
            Some(i) => i,
            None => self.prompt_track_index(path)?,
        };

        if idx < tracks.len() {
            Ok(tracks[idx].id)
        } else {
            Err(DomainError::InputError(format!(
                "Track index {} is out of bounds. Found {} audio tracks.",
                idx,
                tracks.len()
            )))
        }
    }
}

impl AudioExtractor for SymphoniaAdapter {
    fn get_duration(&self, path: &Path, track_id: u32) -> Result<f64, DomainError> {
        let format = self.probe_file(path)?;
        let track = format
            .tracks()
            .iter()
            .find(|t| t.id == track_id)
            .ok_or_else(|| DomainError::ExtractionError("Track not found".to_string()))?;

        let time_base = track
            .codec_params
            .time_base
            .unwrap_or(symphonia::core::units::TimeBase::new(1, 1));
        let duration_frames = track.codec_params.n_frames.unwrap_or(0);
        let time = time_base.calc_time(duration_frames);
        Ok(time.seconds as f64 + time.frac)
    }

    fn hms_to_seconds(&self, hms: &str) -> Result<f64, DomainError> {
        self.hms_to_seconds(hms)
    }

    fn extract_audio(&self, path: &Path, track_id: u32) -> Result<AudioBuffer, DomainError> {
        self.extract_audio_relative(path, track_id, 0.0, 1.0)
    }

    fn extract_audio_range(
        &self,
        path: &Path,
        track_id: u32,
        start_sec: f64,
        end_sec: f64,
    ) -> Result<AudioBuffer, DomainError> {
        let format = self.probe_file(path)?;
        self.extract_audio_range_internal(format, track_id, start_sec, end_sec)
    }

    fn extract_audio_relative(
        &self,
        path: &Path,
        track_id: u32,
        start_percent: f64,
        end_percent: f64,
    ) -> Result<AudioBuffer, DomainError> {
        let format = self.probe_file(path)?;

        let track = format
            .tracks()
            .iter()
            .find(|t| t.id == track_id)
            .ok_or_else(|| DomainError::ExtractionError("Selected track not found".to_string()))?;

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

        self.extract_audio_range_internal(
            format,
            track_id,
            duration_secs * start_percent,
            duration_secs * end_percent,
        )
    }

    fn extract_pcm_secs(
        &self,
        path: &Path,
        track_id: u32,
        start_sec: f64,
        end_sec: f64,
    ) -> Result<AudioBuffer, DomainError> {
        debug!(
            "Initializing Symphonia for accurate PCM extraction: start {:.3}s, end {:.3}s",
            start_sec, end_sec
        );

        if start_sec >= end_sec {
            return Err(DomainError::InputError(
                "End time must be after start time".to_string(),
            ));
        }

        let mut format = self.probe_file(path)?;
        let track = format
            .tracks()
            .iter()
            .find(|t| t.id == track_id)
            .ok_or_else(|| DomainError::ExtractionError("Selected track not found".to_string()))?;
        let time_base = track
            .codec_params
            .time_base
            .ok_or_else(|| DomainError::ExtractionError("Track has no time base".to_string()))?;

        let mut decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .map_err(|e| {
                DomainError::ExtractionError(format!("Failed to create decoder: {}", e))
            })?;

        let start_pts =
            (start_sec * time_base.denom as f64 / time_base.numer as f64).round() as u64;
        let end_pts = (end_sec * time_base.denom as f64 / time_base.numer as f64).round() as u64;

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
        let mut actual_spec: Option<SignalSpec> = None;

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
                continue;
            }
            if packet_pts >= end_pts {
                break;
            }

            match decoder.decode(&packet) {
                Ok(audio_buf) => {
                    if actual_spec.is_none() {
                        let spec = *audio_buf.spec();
                        info!(
                            "Actual decoded spec: {} channels, {} Hz",
                            spec.channels.count(),
                            spec.rate
                        );
                        actual_spec = Some(spec);
                    }

                    let spec = actual_spec.ok_or_else(|| {
                        DomainError::ExtractionError(
                            "No frames decoded to determine spec".to_string(),
                        )
                    })?;
                    let channels = spec.channels.count();

                    if sample_buf.is_none() {
                        let duration = audio_buf.capacity() as u64;
                        sample_buf = Some(SampleBuffer::<i16>::new(duration, spec));
                    }

                    if let Some(buf) = &mut sample_buf {
                        buf.copy_interleaved_ref(audio_buf);
                        let samples = buf.samples();

                        let mut current_frame_pts = packet_pts;
                        let mut i = 0;
                        while i < samples.len() {
                            if current_frame_pts >= end_pts {
                                break;
                            }

                            let count = channels.min(samples.len() - i);
                            if current_frame_pts >= start_pts {
                                audio_samples.extend_from_slice(&samples[i..i + count]);
                            }
                            i += count;

                            current_frame_pts += 1;
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

        let spec = actual_spec.ok_or_else(|| {
            DomainError::ExtractionError("No frames decoded to determine spec".to_string())
        })?;

        Ok(AudioBuffer::new(
            audio_samples,
            spec.rate,
            spec.channels.count() as u16,
        ))
    }
}

impl SampleExporter for SymphoniaAdapter {
    fn export_sample(
        &self,
        input: &Path,
        track_id: u32,
        output: &Path,
        range: &str,
    ) -> Result<(), DomainError> {
        let buffer = if range.contains('-') {
            let parts: Vec<&str> = range.split('-').collect();
            if parts.len() != 2 {
                return Err(DomainError::InputError(
                    "Range must be in 'HH:MM:SS-HH:MM:SS' format".to_string(),
                ));
            }
            let start_sec = self.hms_to_seconds(parts[0])?;
            let end_sec = self.hms_to_seconds(parts[1])?;
            self.extract_pcm_secs(input, track_id, start_sec, end_sec)?
        } else if range.contains(',') {
            let parts: Vec<&str> = range.split(',').collect();
            if parts.len() != 2 {
                return Err(DomainError::InputError(
                    "Relative range must be 'start,end' floats".to_string(),
                ));
            }
            let start: f64 = parts[0]
                .parse()
                .map_err(|_| DomainError::InputError("Invalid start float".to_string()))?;
            let end: f64 = parts[1]
                .parse()
                .map_err(|_| DomainError::InputError("Invalid end float".to_string()))?;

            let total_duration = self.get_duration(input, track_id)?;
            self.extract_pcm_secs(
                input,
                track_id,
                total_duration * start,
                total_duration * end,
            )?
        } else {
            return Err(DomainError::InputError(
                "Range format not recognized".to_string(),
            ));
        };

        self.export_wav(&buffer, output)?;
        Ok(())
    }
}
