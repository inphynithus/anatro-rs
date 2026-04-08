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
use anatro_rs::domain::matcher::{SlidingWindowMatcher, TICK_DURATION};
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
            file,
            json,
            sample_intro,
            sample_outro,
            sample_reference,
            sample_size,
            offset,
            auto_offset,
            length,
            progress,
            threads,
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

            // Reference file resolution
            let ref_path = if file.is_some() || sample_reference.contains(std::path::MAIN_SEPARATOR)
            {
                // If it's a single file scan OR it's clearly a path, use it directly
                PathBuf::from(&sample_reference)
            } else {
                // Otherwise, look for the filename in the collected files from the target directory
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
                        "Reference file '{}' not found in processed files.",
                        sample_reference
                    )
                })?
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
            let mut intro_audio_buffer = None;
            if let Some(ref intro) = sample_intro {
                log::info!(
                    "Extracting intro sample ({:.1}s) from reference at {}",
                    sample_size,
                    intro
                );
                let start_sec = extractor.hms_to_seconds(intro)?;
                let extracted = ref_selected.clone().extract_audio_range(
                    &extractor,
                    start_sec,
                    start_sec + sample_size,
                )?;
                let fp = extracted.generate_fingerprint(&chromaprint)?;
                intro_fingerprint = Some(fp.fingerprint().to_vec());
                intro_audio_buffer = Some(std::sync::Arc::new(fp.buffer().samples().to_vec()));
            }

            let mut outro_fingerprint = None;
            let mut outro_audio_buffer = None;
            if let Some(ref outro) = sample_outro {
                log::info!(
                    "Extracting outro sample ({:.1}s) from reference at {}",
                    sample_size,
                    outro
                );
                let start_sec = extractor.hms_to_seconds(outro)?;
                let extracted = ref_selected.clone().extract_audio_range(
                    &extractor,
                    start_sec,
                    start_sec + sample_size,
                )?;
                let fp = extracted.generate_fingerprint(&chromaprint)?;
                outro_fingerprint = Some(fp.fingerprint().to_vec());
                outro_audio_buffer = Some(std::sync::Arc::new(fp.buffer().samples().to_vec()));
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
                        let fine_matcher =
                            anatro_rs::domain::matcher::CrossCorrelationMatcher::new();

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
                                    let mut start_total =
                                        segmented_fps.offset_sec() + (idx as f64 * TICK_DURATION);

                                    // Fine Match
                                    if let Some(ref ref_buf) = intro_audio_buffer {
                                        let target_buf = segmented_fps.buffer().samples();
                                        let coarse_sample =
                                            (idx as f64 * TICK_DURATION * 11025.0) as usize;
                                        let window_start = coarse_sample.saturating_sub(5 * 11025);
                                        let window_end =
                                            (coarse_sample + ref_buf.len() + 5 * 11025)
                                                .min(target_buf.len());
                                        if window_start < window_end
                                            && let Ok(Some(lag)) = fine_matcher.find_fine_match(
                                                ref_buf,
                                                &target_buf[window_start..window_end],
                                            )
                                        {
                                            start_total = segmented_fps.offset_sec()
                                                + ((window_start as isize + lag) as f64 / 11025.0);
                                        }
                                    }

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
                                    let mut start_total =
                                        segmented_fps.offset_sec() + (idx as f64 * TICK_DURATION);

                                    // Fine Match
                                    if let Some(ref ref_buf) = outro_audio_buffer {
                                        let target_buf = segmented_fps.buffer().samples();
                                        let coarse_sample =
                                            (idx as f64 * TICK_DURATION * 11025.0) as usize;
                                        let window_start = coarse_sample.saturating_sub(5 * 11025);
                                        let window_end =
                                            (coarse_sample + ref_buf.len() + 5 * 11025)
                                                .min(target_buf.len());
                                        if window_start < window_end
                                            && let Ok(Some(lag)) = fine_matcher.find_fine_match(
                                                ref_buf,
                                                &target_buf[window_start..window_end],
                                            )
                                        {
                                            start_total = segmented_fps.offset_sec()
                                                + ((window_start as isize + lag) as f64 / 11025.0);
                                        }
                                    }

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

            // Calculate Auto-Offset based on the processed results of the reference file
            let mut final_intro_offset = offset;
            let mut final_outro_offset = offset;

            if auto_offset {
                let ref_filename = ref_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if let Some(ref_res) = results.iter().find(|r| r.filename == ref_filename) {
                    if let (Some(detected), Some(hms)) = (ref_res.intro_start, &sample_intro) {
                        let optimal = extractor.hms_to_seconds(hms)?;
                        let auto_diff = optimal - detected;
                        log::info!("Auto-offset intro: {:.3}s", auto_diff);
                        final_intro_offset += auto_diff;
                    }
                    if let (Some(detected), Some(hms)) = (ref_res.outro_start, &sample_outro) {
                        let optimal = extractor.hms_to_seconds(hms)?;
                        let auto_diff = optimal - detected;
                        log::info!("Auto-offset outro: {:.3}s", auto_diff);
                        final_outro_offset += auto_diff;
                    }
                }
            }

            // Apply offsets to ALL results
            let final_results: Vec<FileResult> = results
                .into_iter()
                .map(|mut r| {
                    r.intro_start = r
                        .intro_start
                        .map(|s| if s > 0.0 { s + final_intro_offset } else { s });
                    r.outro_start = r.outro_start.map(|s| s + final_outro_offset);
                    r
                })
                .collect();

            let scan_results = ScanResults {
                intro_duration: intro_fingerprint.map(|_| length).unwrap_or(0.0),
                outro_duration: outro_fingerprint.map(|_| length).unwrap_or(0.0),
                files: final_results,
            };

            if json {
                println!("{}", serde_json::to_string_pretty(&scan_results)?);
            } else {
                let out_file =
                    File::create("results.json").context("Failed to create results.json")?;
                serde_json::to_writer_pretty(out_file, &scan_results)
                    .context("Failed to write to results.json")?;
                log::info!("Results successfully written to results.json");
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
    }

    Ok(())
}
