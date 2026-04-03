#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::clone_on_ref_ptr,
    clippy::todo,
    missing_docs,
    missing_debug_implementations,
    trivial_casts,
    trivial_numeric_casts,
    unused_results,
    improper_ctypes
)]

//! # anotro-rs
//!
//! A Rust CLI application for audio fingerprinting and matching.

use anatro_rs::cli::{Cli, Commands};
use anatro_rs::domain::matcher::SlidingWindowMatcher;
use anatro_rs::domain::pipeline::SourceMedia;
use anatro_rs::domain::traits::{AudioExtractor, PcmExporter, SampleExporter, TrackSelector};
use anatro_rs::infrastructure::chromaprint::ChromaprintAdapter;
use anatro_rs::infrastructure::symphonia_adapter::SymphoniaAdapter;
use anyhow::Result;
use clap::Parser;
use std::env;
use std::path::PathBuf;

/// Helper to find a reference file with one of the supported extensions.
fn find_reference_file(base_name: &str) -> Option<PathBuf> {
    let extensions = ["aac", "flac", "mp3", "opus", "vorbis", "ogg", "m4a", "wav"];
    for ext in extensions {
        let path = PathBuf::from(format!("{}.{}", base_name, ext));
        if path.exists() {
            return Some(path);
        }
    }
    None
}

/// The main entry point of the application.
pub fn main() -> Result<()> {
    // Initialize logging from environment variable (default to info)
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Scan { sample } => {
            let extractor = SymphoniaAdapter::new();
            let chromaprint = ChromaprintAdapter::new();
            let matcher = SlidingWindowMatcher::new();

            log::info!("Processing media file: {}", sample.display());

            // 1. Process target episode
            let source = SourceMedia::new(sample);
            let selected_track = source.select_track(&extractor)?;
            let extracted = selected_track.extract_audio(&extractor)?;
            let fingerprinted = extracted.generate_fingerprint(&chromaprint)?;

            log::info!(
                "Episode fingerprint generated ({} hashes).",
                fingerprinted.fingerprint().len()
            );

            // 2. Process Reference Samples
            let ref_bases = ["intro_sample", "outro_sample"];
            for base in ref_bases {
                if let Some(ref_path) = find_reference_file(base) {
                    log::info!("Found reference sample: {}", ref_path.display());
                    let ref_source = SourceMedia::new(ref_path);
                    let ref_selected = ref_source.select_track(&extractor)?;
                    let ref_extracted = ref_selected.extract_audio(&extractor)?;
                    let ref_fingerprinted = ref_extracted.generate_fingerprint(&chromaprint)?;

                    log::info!(
                        "Reference '{}' fingerprint generated ({} hashes).",
                        base,
                        ref_fingerprinted.fingerprint().len()
                    );

                    // 3. Perform Matching
                    // Use a conservative threshold: 20% bit error rate (32 bits * 0.20 = 6.4 bits per hash)
                    let threshold = (ref_fingerprinted.fingerprint().len() as u32 * 32) / 5;

                    let result = fingerprinted.find_match(
                        &matcher,
                        ref_fingerprinted.fingerprint(),
                        threshold,
                    )?;

                    match result.match_index {
                        Some(idx) => {
                            // Each Chromaprint hash usually represents ~0.1s of audio (depending on parameters)
                            // Our default is 11025Hz and default chromaprint window settings.
                            log::info!("MATCH FOUND for '{}' at index {}.", base, idx);
                        }
                        None => {
                            log::warn!("No suitable match found for '{}'.", base);
                        }
                    }
                } else {
                    log::warn!(
                        "Reference sample '{}.*' not found in current directory. Skipping.",
                        base
                    );
                }
            }
        }
        Commands::SampleExtract {
            target,
            range,
            output,
        } => {
            let extractor = SymphoniaAdapter::new();

            // Handle output path: if it's a simple name, use CWD.
            // Automatically append .wav for internal testing if no extension is provided.
            let mut final_output = output;

            if final_output.extension().is_none() {
                let _ = final_output.set_extension("wav");
            }

            if final_output.parent() == Some(std::path::Path::new("")) {
                let cwd = env::current_dir()?;
                final_output = cwd.join(final_output);
            }

            log::info!("Sample Extract initialized for file: {}", target.display());
            log::info!("Range requested: {}", range);
            log::info!("Output path: {}", final_output.display());
            log::info!("NOTE: Using sample-accurate PCM extraction with WAV export.");

            let track_id = extractor.select_track(&target)?;
            extractor.export_sample(&target, track_id, &final_output, &range)?;

            log::info!(
                "Sample extracted successfully to: {}",
                final_output.display()
            );
        }
        Commands::SampleTest {
            target,
            range,
            output,
        } => {
            let extractor = SymphoniaAdapter::new();

            // Handle output path: if it's a simple name, use CWD.
            let mut final_output = output;

            if final_output.extension().is_none() {
                let _ = final_output.set_extension("wav");
            }

            if final_output.parent() == Some(std::path::Path::new("")) {
                let cwd = env::current_dir()?;
                final_output = cwd.join(final_output);
            }

            log::info!("Sample Test initialized for file: {}", target.display());
            log::info!("Range requested: {}", range);
            log::info!("Output path: {}", final_output.display());
            log::info!("NOTE: Extracting in MONO and resampled to 11025Hz for quality testing.");

            let track_id = extractor.select_track(&target)?;

            let buffer = if range.contains('-') {
                let parts: Vec<&str> = range.split('-').collect();
                if parts.len() != 2 {
                    return Err(anyhow::anyhow!(
                        "Range must be in 'HH:MM:SS-HH:MM:SS' format"
                    ));
                }
                let start_sec = extractor.hms_to_seconds(parts[0])?;
                let end_sec = extractor.hms_to_seconds(parts[1])?;
                extractor.extract_audio_range(&target, track_id, start_sec, end_sec)?
            } else if range.contains(',') {
                let parts: Vec<&str> = range.split(',').collect();
                if parts.len() != 2 {
                    return Err(anyhow::anyhow!("Relative range must be 'start,end' floats"));
                }
                let start_percent: f64 = parts[0].parse()?;
                let end_percent: f64 = parts[1].parse()?;
                extractor.extract_audio_relative(&target, track_id, start_percent, end_percent)?
            } else {
                return Err(anyhow::anyhow!("Range format not recognized"));
            };

            extractor.export_wav(&buffer, &final_output)?;

            log::info!(
                "Sample test extracted successfully to: {}",
                final_output.display()
            );
        }
    }

    Ok(())
}
