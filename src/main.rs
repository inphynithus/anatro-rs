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
use anatro_rs::domain::traits::{FingerprintMatcher, SampleExporter, TrackSelector};
use anatro_rs::infrastructure::chromaprint::ChromaprintAdapter;
use anatro_rs::infrastructure::symphonia_adapter::SymphoniaAdapter;
use anyhow::{Context, Result};
use clap::Parser;
use rayon::prelude::*;
use std::env;
use std::fs::File;
use std::path::PathBuf;

#[derive(serde::Serialize)]
struct ScanResults {
    intro_duration: f64,
    outro_duration: f64,
    files: Vec<FileResult>,
}

#[derive(serde::Serialize)]
struct FileResult {
    filename: String,
    intro_start: Option<f64>,
    outro_start: Option<f64>,
}

/// Helper to format seconds into MM:SS.
#[allow(dead_code)]
fn format_time(seconds: f64) -> String {
    let total_secs = seconds.round() as i64;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    format!("{:02}:{:02}", mins, secs)
}

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
        Commands::Scan {
            target,
            sample_intro,
            sample_outro,
            sample_reference,
            offset,
            length,
            progress,
            threads,
        } => {
            if sample_intro.is_none() && sample_outro.is_none() {
                return Err(anyhow::anyhow!(
                    "At least one of --sample-intro or --sample-outro must be provided."
                ));
            }

            if !target.is_dir() {
                return Err(anyhow::anyhow!(
                    "Target must be a directory. Found: {}",
                    target.display()
                ));
            }

            let mut files = Vec::new();
            for entry in std::fs::read_dir(&target).context("Failed to read target directory")? {
                let entry = entry?;
                let path = entry.path();
                if path.is_file()
                    && let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                        let ext = ext.to_lowercase();
                        if ext == "mkv" || ext == "mp4" {
                            files.push(path);
                        }
                    }
            }
            files.sort();

            if files.is_empty() {
                log::warn!("No .mkv or .mp4 files found in target directory.");
                return Ok(());
            }

            let ref_path = if !sample_reference.contains(std::path::MAIN_SEPARATOR) {
                let mut found = None;
                for f in &files {
                    let name = f.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if name == sample_reference {
                        found = Some(f.clone());
                        break;
                    }
                }
                found.ok_or_else(|| {
                    anyhow::anyhow!(
                        "Reference file '{}' not found in target directory.",
                        sample_reference
                    )
                })?
            } else {
                PathBuf::from(&sample_reference)
            };

            if !ref_path.exists() {
                return Err(anyhow::anyhow!(
                    "Reference file does not exist: {}",
                    ref_path.display()
                ));
            }

            log::info!("Using reference file: {}", ref_path.display());

            let extractor = SymphoniaAdapter::new();
            let chromaprint = ChromaprintAdapter::new();

            let ref_source = SourceMedia::new(ref_path.clone());
            let ref_selected = ref_source.select_track(&extractor)?;

            let mut intro_fingerprint = None;
            if let Some(ref intro) = sample_intro {
                log::info!("Extracting intro sample from reference at {}", intro);
                let start_sec = extractor.hms_to_seconds(intro)?;
                let extracted = ref_selected.clone().extract_audio_range(
                    &extractor,
                    start_sec,
                    start_sec + length,
                )?;
                let fp = extracted.generate_fingerprint(&chromaprint)?;
                intro_fingerprint = Some(fp.fingerprint().to_vec());
            }

            let mut outro_fingerprint = None;
            if let Some(ref outro) = sample_outro {
                log::info!("Extracting outro sample from reference at {}", outro);
                let start_sec = extractor.hms_to_seconds(outro)?;
                let extracted = ref_selected.clone().extract_audio_range(
                    &extractor,
                    start_sec,
                    start_sec + length,
                )?;
                let fp = extracted.generate_fingerprint(&chromaprint)?;
                outro_fingerprint = Some(fp.fingerprint().to_vec());
            }

            let num_threads = threads.min(files.len().max(1));
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(num_threads)
                .build()?;

            let multi_progress = indicatif::MultiProgress::new();
            let main_pb = if progress {
                let pb = multi_progress.add(indicatif::ProgressBar::new(files.len() as u64));
                pb.set_style(
                    indicatif::ProgressStyle::default_bar()
                        .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} files ({eta}) {msg}")
                        .unwrap_or_else(|e| {
                            log::warn!("Failed to set progress bar template: {}", e);
                            indicatif::ProgressStyle::default_bar()
                        }),
                );
                Some(pb)
            } else {
                None
            };

            let results: Vec<FileResult> = pool.install(|| {
                files
                    .into_par_iter()
                    .map(|file| {
                        let file_name = file
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("")
                            .to_string();
                        let mut file_res = FileResult {
                            filename: file_name.clone(),
                            intro_start: None,
                            outro_start: None,
                        };

                        let worker_extractor = SymphoniaAdapter::new();
                        let worker_chromaprint = ChromaprintAdapter::new();
                        let matcher = SlidingWindowMatcher::new();

                        let mut process_file = || -> Result<()> {
                            let source = SourceMedia::new(file.clone());
                            let selected_track = source.select_track(&worker_extractor)?;

                            if let Some(ref intro_fp) = intro_fingerprint {
                                let segmented_audio =
                                    selected_track.clone().extract_segmented_audio(
                                        &worker_extractor,
                                        SearchSpace::Intro,
                                    )?;
                                let segmented_fps = segmented_audio
                                    .generate_segmented_fingerprints(&worker_chromaprint)?;
                                let threshold = (intro_fp.len() as u32 * 32) / 5;
                                if let Some(idx) = matcher.find_match(
                                    intro_fp,
                                    segmented_fps.fingerprint(),
                                    threshold,
                                ) {
                                    let start_total =
                                        segmented_fps.offset_sec() + (idx as f64 * 0.128) + offset;
                                    file_res.intro_start = Some(start_total);
                                }
                            }

                            if let Some(ref outro_fp) = outro_fingerprint {
                                let segmented_audio = selected_track.extract_segmented_audio(
                                    &worker_extractor,
                                    SearchSpace::Outro,
                                )?;
                                let segmented_fps = segmented_audio
                                    .generate_segmented_fingerprints(&worker_chromaprint)?;
                                let threshold = (outro_fp.len() as u32 * 32) / 5;
                                if let Some(idx) = matcher.find_match(
                                    outro_fp,
                                    segmented_fps.fingerprint(),
                                    threshold,
                                ) {
                                    let start_total =
                                        segmented_fps.offset_sec() + (idx as f64 * 0.128) + offset;
                                    file_res.outro_start = Some(start_total);
                                }
                            }
                            Ok(())
                        };

                        if let Err(e) = process_file() {
                            log::warn!("Error processing file {}: {}", file_name, e);
                        }

                        if let Some(ref pb) = main_pb {
                            pb.inc(1);
                        }

                        file_res
                    })
                    .collect()
            });

            if let Some(pb) = main_pb {
                pb.finish_with_message("Done");
            }

            let scan_results = ScanResults {
                intro_duration: intro_fingerprint.map(|_| length).unwrap_or(0.0),
                outro_duration: outro_fingerprint.map(|_| length).unwrap_or(0.0),
                files: results,
            };

            let out_file = File::create("results.json").context("Failed to create results.json")?;
            serde_json::to_writer_pretty(out_file, &scan_results)
                .context("Failed to write to results.json")?;
            log::info!("Results successfully written to results.json");
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
    }

    Ok(())
}
