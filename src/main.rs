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
use anatro_rs::domain::pipeline::{SearchSpace, SourceMedia};
use anatro_rs::domain::traits::{
    AudioExtractor, FingerprintMatcher, Fingerprinter, PcmExporter, SampleExporter, TrackSelector,
};
use anatro_rs::infrastructure::chromaprint::ChromaprintAdapter;
use anatro_rs::infrastructure::symphonia_adapter::SymphoniaAdapter;
use anyhow::Result;
use clap::Parser;
use std::env;

/// The main entry point of the application.
pub fn main() -> Result<()> {
    // Initialize logging: default to 'info' for our app and 'warn' for noisy dependencies.
    // This can still be overridden by setting the RUST_LOG environment variable.
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,symphonia=warn"),
    )
    .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Scan { target, sample } => {
            let extractor = SymphoniaAdapter::new();
            let chromaprint = ChromaprintAdapter::new();
            let matcher = SlidingWindowMatcher::new();

            log::info!("Scanning target: {}", target.display());
            log::info!("Using reference sample: {}", sample.display());

            // 1. Determine Search Space based on sample name
            let sample_name = sample.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let space = if sample_name.to_lowercase().contains("intro") {
                SearchSpace::Intro
            } else if sample_name.to_lowercase().contains("outro") {
                SearchSpace::Outro
            } else {
                log::warn!("Sample name does not contain 'intro' or 'outro'. Defaulting to Intro.");
                SearchSpace::Intro
            };

            // 2. Process target episode search space (targeted)
            let source = SourceMedia::new(target);
            let selected_track = source.select_track(&extractor)?;
            let segmented_audio = selected_track.extract_segmented_audio(&extractor, space)?;
            let segmented_fingerprints =
                segmented_audio.generate_segmented_fingerprints(&chromaprint)?;

            log::info!(
                "Episode search space fingerprint generated for {:?} ({} hashes).",
                space,
                segmented_fingerprints.fingerprint().len()
            );

            // 3. Process Reference Sample (Load directly if WAV)
            let ref_fingerprinted = if sample.extension().and_then(|e| e.to_str()) == Some("wav") {
                log::info!(
                    "Loading reference sample directly from WAV: {}",
                    sample.display()
                );
                let buffer = extractor.load_wav(&sample)?;
                chromaprint.generate_fingerprint(&buffer)?
            } else {
                log::info!("Extracting reference sample: {}", sample.display());
                let ref_source = SourceMedia::new(sample.clone());
                let ref_selected = ref_source.select_track(&extractor)?;
                let ref_extracted = ref_selected.extract_audio(&extractor)?;
                chromaprint.generate_fingerprint(ref_extracted.buffer())?
            };

            log::info!(
                "Reference '{}' fingerprint generated ({} hashes).",
                sample_name,
                ref_fingerprinted.len()
            );

            // 4. Perform Matching
            // Use a conservative threshold: 20% bit error rate
            let threshold = (ref_fingerprinted.len() as u32 * 32) / 5;

            let match_index = matcher.find_match(
                &ref_fingerprinted,
                segmented_fingerprints.fingerprint(),
                threshold,
            );

            match match_index {
                Some(idx) => {
                    // Heuristic: each hash is approx 0.124s (11025 / 1365 or similar in chromaprint)
                    let tick_duration = 0.124;
                    let start_in_space = idx as f64 * tick_duration;
                    let start_total = segmented_fingerprints.offset_sec() + start_in_space;
                    let duration_sample = ref_fingerprinted.len() as f64 * tick_duration;

                    log::info!(
                        "MATCH FOUND for '{}'! Start: {:.2}s, End: {:.2}s (Total: {:.2}s)",
                        sample_name,
                        start_total,
                        start_total + duration_sample,
                        duration_sample
                    );
                }
                None => {
                    log::warn!("No suitable match found for '{}'.", sample_name);
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
