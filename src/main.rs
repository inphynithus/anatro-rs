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
use anatro_rs::domain::pipeline::SourceMedia;
use anatro_rs::domain::traits::{AudioExtractor, PcmExporter, SampleExporter};
use anatro_rs::infrastructure::chromaprint::ChromaprintAdapter;
use anatro_rs::infrastructure::symphonia_adapter::SymphoniaAdapter;
use anyhow::Result;
use clap::Parser;
use std::env;

/// The main entry point of the application.
pub fn main() -> Result<()> {
    // Initialize logging from environment variable (default to info)
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Scan { sample } => {
            let extractor = SymphoniaAdapter::new();
            let chromaprint = ChromaprintAdapter::new();

            log::info!("Processing media file: {}", sample.display());

            let source = SourceMedia::new(sample);

            // Pipeline execution:
            // 1. Extract Audio (includes track selection and mono/downsampling)
            let extracted = source.extract_audio(&extractor)?;

            log::info!(
                "Audio extracted successfully ({} samples).",
                extracted.buffer().samples().len()
            );

            // 2. Generate Fingerprint
            let fingerprinted = extracted.generate_fingerprint(&chromaprint)?;

            log::info!(
                "Fingerprint generated successfully ({} hashes).",
                fingerprinted.fingerprint().len()
            );
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

            extractor.export_sample(&target, &final_output, &range)?;

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

            let buffer = if range.contains('-') {
                let parts: Vec<&str> = range.split('-').collect();
                if parts.len() != 2 {
                    return Err(anyhow::anyhow!(
                        "Range must be in 'HH:MM:SS-HH:MM:SS' format"
                    ));
                }
                let start_sec = extractor.hms_to_seconds(parts[0])?;
                let end_sec = extractor.hms_to_seconds(parts[1])?;
                extractor.extract_audio_range(&target, start_sec, end_sec)?
            } else if range.contains(',') {
                let parts: Vec<&str> = range.split(',').collect();
                if parts.len() != 2 {
                    return Err(anyhow::anyhow!("Relative range must be 'start,end' floats"));
                }
                let start_percent: f64 = parts[0].parse()?;
                let end_percent: f64 = parts[1].parse()?;
                extractor.extract_audio_relative(&target, start_percent, end_percent)?
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
