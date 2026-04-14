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
use anatro_rs::domain::preset::PresetManager;
use anatro_rs::domain::scanner::{ScanOptions, Scanner};
use anatro_rs::domain::traits::{SampleExporter, TrackSelector};
use anatro_rs::infrastructure::symphonia_adapter::SymphoniaAdapter;
use anyhow::{Context, Result};
use clap::Parser;
use std::env;

/// The main entry point of the application.
pub fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging: disabled by default, enabled with --log [level].
    // RUST_LOG env var always takes precedence for advanced users.
    let default_filter = match cli.log.as_deref() {
        Some(level) => format!("{level},symphonia=warn"),
        None => "off".to_string(),
    };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(&default_filter))
        .init();

    match cli.command {
        Commands::Scan {
            target,
            file,
            json,
            sample_intro,
            sample_outro,
            sample_reference,
            sample_size,
            offset,
            force,
            preset,
            progress,
            threads,
            track,
        } => {
            if sample_intro.is_none() && sample_outro.is_none() {
                return Err(anyhow::anyhow!(
                    "At least one of --sample-intro or --sample-outro must be provided."
                ));
            }

            let mut files = Vec::new();

            if let Some(ref single_file) = file {
                if !single_file.exists() {
                    return Err(anyhow::anyhow!("File not found: {}", single_file.display()));
                }
                files.push(single_file.clone());
            } else if let Some(target_dir) = target {
                if !target_dir.is_dir() {
                    return Err(anyhow::anyhow!(
                        "Target must be a directory. Found: {}",
                        target_dir.display()
                    ));
                }

                for entry in
                    std::fs::read_dir(&target_dir).context("Failed to read target directory")?
                {
                    let entry = entry?;
                    let path = entry.path();
                    if path.is_file() {
                        let ext = path
                            .extension()
                            .and_then(|e| e.to_str())
                            .unwrap_or("")
                            .to_lowercase();
                        if ext == "mkv" || ext == "mp4" {
                            files.push(path);
                        }
                    }
                }
                files.sort();
            } else {
                return Err(anyhow::anyhow!(
                    "Either --target <DIR> or --file <FILE> must be provided."
                ));
            }

            if files.is_empty() {
                log::warn!("No media files found to process.");
                return Ok(());
            }

            let scanner = Scanner::new();
            let preset_manager = PresetManager::load_or_create()?;
            let (preset_name, preset_config) = preset_manager.get_preset(preset.as_deref())?;
            log::info!("Using configuration preset: {}", preset_name);

            let options = ScanOptions {
                sample_intro,
                sample_outro,
                sample_reference,
                sample_size,
                offset,
                force,
                json,
                progress,
                threads,
                preset: preset_config,
                track,
            };

            let scan_results = scanner.run_scan(files, options)?;

            if json {
                println!("{}", serde_json::to_string_pretty(&scan_results)?);
            }
        }
        Commands::Debug {
            file,
            expected,
            sample_intro,
            sample_outro,
            sample_reference,
            sample_size,
            track,
        } => {
            let scanner = Scanner::new();
            let preset_manager = PresetManager::load_or_create()?;
            let (preset_name, preset_config) = preset_manager.get_preset(None)?;
            log::info!("Using configuration preset: {} (debug mode)", preset_name);
            let options = ScanOptions {
                sample_intro,
                sample_outro,
                sample_reference: sample_reference.to_string_lossy().to_string(),
                sample_size,
                offset: 0.0,
                force: true,
                json: true,
                progress: false,
                threads: 1,
                preset: preset_config,
                track,
            };
            scanner.run_debug(file, options, expected)?;
        }
        Commands::SampleExtract {
            target,
            range,
            output,
            track,
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

            log::info!("Initializing Sample Extraction");
            log::info!(" -> Target audio: {}", target.display());
            log::info!(" -> Range: {}", range);
            log::info!(" -> Export format: Sample-Accurate PCM to WAV");

            let track_id = extractor.select_track(&target, track)?;
            extractor.export_sample(&target, track_id, &final_output, &range)?;

            log::info!(
                "Successfully exported sample to: {}",
                final_output.display()
            );
        }
    }

    Ok(())
}
