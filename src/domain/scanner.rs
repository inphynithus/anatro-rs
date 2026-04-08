//! Orchestration logic for scanning media files.

use crate::domain::matcher::{SlidingWindowMatcher, TICK_DURATION};
use crate::domain::pipeline::{SearchSpace, SourceMedia};
use crate::domain::result::{FileResult, ScanResults};
use crate::domain::traits::{FineMatcher, FingerprintMatcher};
use crate::infrastructure::chromaprint::ChromaprintAdapter;
use crate::infrastructure::cross_correlate_adapter::CrossCorrelationAdapter;
use crate::infrastructure::symphonia_adapter::SymphoniaAdapter;
use anyhow::Result;
use rayon::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;

/// Options for the scan operation.
pub struct ScanOptions {
    pub sample_intro: Option<String>,
    pub sample_outro: Option<String>,
    pub sample_reference: String,
    pub sample_size: f64,
    pub offset: f64,
    pub auto_offset: bool,
    pub length: f64,
    pub progress: bool,
    pub threads: usize,
}

/// The main orchestrator for the scanning process.
#[derive(Default)]
pub struct Scanner {
    extractor: SymphoniaAdapter,
    chromaprint: ChromaprintAdapter,
}

impl Scanner {
    /// Creates a new Scanner.
    pub fn new() -> Self {
        Self {
            extractor: SymphoniaAdapter::new(),
            chromaprint: ChromaprintAdapter::new(),
        }
    }

    /// Executes a detailed debug scan on a specific file.
    pub fn run_debug(
        &self,
        target_path: PathBuf,
        options: ScanOptions,
        expected_sec: f64,
    ) -> Result<()> {
        log::info!(
            "Starting detailed debug analysis for: {}",
            target_path.display()
        );
        log::info!("Expected timestamp: {:.3}s", expected_sec);

        // 1. Resolve Reference
        let ref_path = PathBuf::from(&options.sample_reference);
        if !ref_path.exists() {
            return Err(anyhow::anyhow!(
                "Reference file not found: {}",
                ref_path.display()
            ));
        }

        let ref_source = SourceMedia::new(ref_path.clone());
        let ref_selected = ref_source.select_track(&self.extractor)?;
        let target_source = SourceMedia::new(target_path.clone());
        let target_selected = target_source.select_track(&self.extractor)?;

        // 2. Extract Reference
        let (ref_hms, space) = if let Some(ref h) = options.sample_intro {
            (h, SearchSpace::Intro)
        } else if let Some(ref h) = options.sample_outro {
            (h, SearchSpace::Outro)
        } else {
            return Err(anyhow::anyhow!(
                "Either --sample-intro or --sample-outro must be provided for debug."
            ));
        };

        let start_sec = self.extractor.hms_to_seconds(ref_hms)?;
        log::info!(
            "Extracting reference sample from {} at {}",
            ref_path.display(),
            ref_hms
        );

        let ref_audio = ref_selected.clone().extract_audio_range(
            &self.extractor,
            start_sec,
            start_sec + options.sample_size,
        )?;

        // Export debug WAVs
        self.extractor
            .export_wav(ref_audio.buffer(), &PathBuf::from("debug_ref_11k.wav"))?;
        log::info!("Exported debug_ref_11k.wav");

        // 3. Coarse Match pass
        log::info!("--- Stage 1: Coarse Matching (Chromaprint) ---");
        let segmented_audio = target_selected
            .clone()
            .extract_segmented_audio(&self.extractor, space)?;
        let segmented_fps = segmented_audio.generate_segmented_fingerprints(&self.chromaprint)?;
        let fp_ref = ref_audio.clone().generate_fingerprint(&self.chromaprint)?;

        let matcher = SlidingWindowMatcher::new();
        let threshold = (fp_ref.fingerprint().len() as u32 * 32) / 5;

        if let Some(coarse_idx) =
            matcher.find_match(fp_ref.fingerprint(), segmented_fps.fingerprint(), threshold)
        {
            let coarse_start = segmented_fps.offset_sec() + (coarse_idx as f64 * TICK_DURATION);
            log::info!(
                "Coarse Match found at: {:.3}s (Diff from expected: {:.3}s)",
                coarse_start,
                (coarse_start - expected_sec).abs()
            );
        } else {
            log::warn!("COARSE MATCH FAILED at default threshold!");
        }

        // 4. Wide-Window Native Sweep
        log::info!("--- Stage 2: Wide 11kHz Sweep ---");
        // We search in a +/- 15s window around the EXPECTED timestamp to see if the peak exists there
        let sweep_start = (expected_sec - 15.0).max(0.0);
        let sweep_end = expected_sec + 15.0;

        let sweep_tgt = target_selected
            .clone()
            .extract_audio_range(&self.extractor, sweep_start, sweep_end)?;
        self.extractor
            .export_wav(sweep_tgt.buffer(), &PathBuf::from("debug_sweep_target_11k.wav"))?;
        log::info!(
            "Exported debug_sweep_target_11k.wav (window: {:.1}s to {:.1}s)",
            sweep_start,
            sweep_end
        );

        // Pattern: first 2s of 11k reference
        let pattern_len = (2.0 * 11025.0) as usize;
        let pattern = &ref_audio.buffer().samples()[..pattern_len.min(ref_audio.buffer().samples().len())];

        // Manual peak analysis
        let src_f32: Vec<f32> = pattern.iter().map(|&x| x as f32).collect();
        let dst_f32: Vec<f32> = sweep_tgt.buffer().samples().iter().map(|&x| x as f32).collect();

        // Apply pre-processing identical to the adapter
        let norm_src = self.normalize_f32(&self.high_pass_f32(&src_f32));
        let norm_dst = self.normalize_f32(&self.high_pass_f32(&dst_f32));

        let correlation = cross_correlate::Correlate::create_real_f32(
            norm_src.len(),
            norm_dst.len(),
            cross_correlate::CrossCorrelationMode::Full,
        )
        .map_err(|e| anyhow::anyhow!("Correlation creation failed: {:?}", e))?;
        let corr = correlation
            .correlate_managed(&norm_src, &norm_dst)
            .map_err(|e| anyhow::anyhow!("Correlation failed: {:?}", e))?;

        // Find top 5 peaks
        let mut peaks: Vec<(usize, f32)> = corr.iter().enumerate().map(|(i, &v)| (i, v)).collect();
        peaks.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        log::info!("Top 5 Correlation Peaks in Sweep Window:");
        for (i, &(idx, val)) in peaks.iter().enumerate().take(5) {
            let lag = idx as f64 - (norm_src.len() as f64 - 1.0);
            let timestamp = sweep_start + (lag / 11025.0);
            log::info!(
                "  Peak #{}: Value: {:.2}, Timestamp: {:.6}s (Diff from expected: {:.6}s)",
                i + 1,
                val,
                timestamp,
                (timestamp - expected_sec).abs()
            );
        }

        let best_peak_ts = sweep_start
            + ((peaks[0].0 as f64 - (norm_src.len() as f64 - 1.0)) / 11025.0);
        if (best_peak_ts - expected_sec).abs() < 0.1 {
            log::info!("SUCCESS: The true peak is the strongest in the 30s sweep window.");
        } else {
            log::warn!("FAILURE: The strongest peak is NOT at the expected timestamp.");
            // Check if expected is even in top 5
            let mut found_expected = false;
            for (idx, _) in &peaks[..100.min(peaks.len())] {
                let lag = *idx as f64 - (norm_src.len() as f64 - 1.0);
                let ts = sweep_start + (lag / 11025.0);
                if (ts - expected_sec).abs() < 0.1 {
                    found_expected = true;
                    break;
                }
            }
            if found_expected {
                log::info!("The expected peak EXISTS in the top 100 but was out-competed.");
            } else {
                log::error!(
                    "The expected peak was NOT found even in top 100 of the sweep window. Audio might be too different."
                );
            }
        }

        Ok(())
    }

    fn normalize_f32(&self, data: &[f32]) -> Vec<f32> {
        let mean = data.iter().sum::<f32>() / data.len() as f32;
        let variance = data.iter().map(|&x| (x - mean).powi(2)).sum::<f32>() / data.len() as f32;
        let std_dev = variance.sqrt().max(1e-6);
        data.iter().map(|&x| (x - mean) / std_dev).collect()
    }

    fn high_pass_f32(&self, data: &[f32]) -> Vec<f32> {
        let alpha = 0.9;
        let mut filtered = Vec::with_capacity(data.len());
        let mut prev_in = 0.0;
        let mut prev_out = 0.0;
        for &s in data {
            let out = alpha * (prev_out + s - prev_in);
            filtered.push(out);
            prev_in = s;
            prev_out = out;
        }
        filtered
    }

    /// Executes the scan process on a list of files.
    pub fn run_scan(&self, files: Vec<PathBuf>, options: ScanOptions) -> Result<ScanResults> {
        // 1. Resolve Reference File
        let ref_path = if !options.sample_reference.contains(std::path::MAIN_SEPARATOR) {
            let mut found = None;
            for f in &files {
                let name = f.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name == options.sample_reference {
                    found = Some(f.clone());
                    break;
                }
            }
            found.ok_or_else(|| {
                anyhow::anyhow!(
                    "Reference file '{}' not found in processed files.",
                    options.sample_reference
                )
            })?
        } else {
            PathBuf::from(&options.sample_reference)
        };

        if !ref_path.exists() {
            return Err(anyhow::anyhow!(
                "Reference file does not exist: {}",
                ref_path.display()
            ));
        }

        log::info!("Using reference file: {}", ref_path.display());

        let ref_source = SourceMedia::new(ref_path.clone());
        let ref_selected = ref_source.select_track(&self.extractor)?;

        // 2. Extract Reference Fingerprints and Buffers
        let mut intro_fp = None;
        let mut intro_buf = None;
        if let Some(ref intro_hms) = options.sample_intro {
            log::info!("Extracting intro sample from reference at {}", intro_hms);
            let start_sec = self.extractor.hms_to_seconds(intro_hms)?;
            let extracted = ref_selected.clone().extract_audio_range(
                &self.extractor,
                start_sec,
                start_sec + options.sample_size,
            )?;
            let fp = extracted.generate_fingerprint(&self.chromaprint)?;
            intro_fp = Some(fp.fingerprint().to_vec());
            intro_buf = Some(Arc::new(fp.buffer().samples().to_vec()));
        }

        let mut outro_fp = None;
        let mut outro_buf = None;
        if let Some(ref outro_hms) = options.sample_outro {
            log::info!("Extracting outro sample from reference at {}", outro_hms);
            let start_sec = self.extractor.hms_to_seconds(outro_hms)?;
            let extracted = ref_selected.clone().extract_audio_range(
                &self.extractor,
                start_sec,
                start_sec + options.sample_size,
            )?;
            let fp = extracted.generate_fingerprint(&self.chromaprint)?;
            outro_fp = Some(fp.fingerprint().to_vec());
            outro_buf = Some(Arc::new(fp.buffer().samples().to_vec()));
        }

        // 3. Setup Thread Pool & Progress
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(options.threads.min(files.len().max(1)))
            .build()?;

        let multi_progress = indicatif::MultiProgress::new();
        let main_pb = if options.progress {
            let pb = multi_progress.add(indicatif::ProgressBar::new(files.len() as u64));
            pb.set_style(
                indicatif::ProgressStyle::default_bar()
                    .template(
                        "[{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} files ({eta}) {msg}",
                    )
                    .unwrap_or_else(|e| {
                        log::warn!("Failed to set progress bar template: {}", e);
                        indicatif::ProgressStyle::default_bar()
                    }),
            );
            Some(pb)
        } else {
            None
        };

        // 4. Parallel Processing
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

                    // Workers need their own adapters (ports)
                    let worker_extractor = SymphoniaAdapter::new();
                    let worker_chromaprint = ChromaprintAdapter::new();
                    let matcher = SlidingWindowMatcher::new();
                    let fine_matcher = CrossCorrelationAdapter::new();

                    let mut process_file = || -> Result<()> {
                        let source = SourceMedia::new(file.clone());
                        let selected_track = source.select_track(&worker_extractor)?;

                        if let Some(ref ref_fp) = intro_fp {
                            let segmented_audio = selected_track
                                .clone()
                                .extract_segmented_audio(&worker_extractor, SearchSpace::Intro)?;
                            let segmented_fps = segmented_audio
                                .generate_segmented_fingerprints(&worker_chromaprint)?;
                            let threshold = (ref_fp.len() as u32 * 32) / 5;
                            if let Some(idx) =
                                matcher.find_match(ref_fp, segmented_fps.fingerprint(), threshold)
                            {
                                let mut start_total =
                                    segmented_fps.offset_sec() + (idx as f64 * TICK_DURATION);

                                if let Some(ref ref_audio) = intro_buf {
                                    let target_audio = segmented_fps.buffer().samples();
                                    let coarse_sample =
                                        (idx as f64 * TICK_DURATION * 11025.0) as usize;
                                    let window_start = coarse_sample.saturating_sub(5 * 11025);
                                    let window_end = (coarse_sample + ref_audio.len() + 5 * 11025)
                                        .min(target_audio.len());
                                    if window_start < window_end
                                        && let Ok(Some(lag)) = fine_matcher.find_fine_match(
                                            ref_audio,
                                            &target_audio[window_start..window_end],
                                        )
                                    {
                                        start_total = segmented_fps.offset_sec()
                                            + ((window_start as isize + lag) as f64 / 11025.0);
                                    }
                                }
                                file_res.intro_start = Some(start_total);
                            }
                        }

                        if let Some(ref ref_fp) = outro_fp {
                            let segmented_audio = selected_track
                                .extract_segmented_audio(&worker_extractor, SearchSpace::Outro)?;
                            let segmented_fps = segmented_audio
                                .generate_segmented_fingerprints(&worker_chromaprint)?;
                            let threshold = (ref_fp.len() as u32 * 32) / 5;
                            if let Some(idx) =
                                matcher.find_match(ref_fp, segmented_fps.fingerprint(), threshold)
                            {
                                let mut start_total =
                                    segmented_fps.offset_sec() + (idx as f64 * TICK_DURATION);

                                if let Some(ref ref_audio) = outro_buf {
                                    let target_audio = segmented_fps.buffer().samples();
                                    let coarse_sample =
                                        (idx as f64 * TICK_DURATION * 11025.0) as usize;
                                    let window_start = coarse_sample.saturating_sub(5 * 11025);
                                    let window_end = (coarse_sample + ref_audio.len() + 5 * 11025)
                                        .min(target_audio.len());
                                    if window_start < window_end
                                        && let Ok(Some(lag)) = fine_matcher.find_fine_match(
                                            ref_audio,
                                            &target_audio[window_start..window_end],
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

        // 5. Finalize Offsets
        let mut final_intro_offset = options.offset;
        let mut final_outro_offset = options.offset;

        if options.auto_offset {
            let ref_filename = ref_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if let Some(ref_res) = results.iter().find(|r| r.filename == ref_filename) {
                if let (Some(detected), Some(hms)) = (ref_res.intro_start, &options.sample_intro) {
                    let optimal = self.extractor.hms_to_seconds(hms)?;
                    let auto_diff = optimal - detected;
                    log::info!("Auto-offset intro: {:.3}s", auto_diff);
                    final_intro_offset += auto_diff;
                }
                if let (Some(detected), Some(hms)) = (ref_res.outro_start, &options.sample_outro) {
                    let optimal = self.extractor.hms_to_seconds(hms)?;
                    let auto_diff = optimal - detected;
                    log::info!("Auto-offset outro: {:.3}s", auto_diff);
                    final_outro_offset += auto_diff;
                }
            }
        }

        let final_files: Vec<FileResult> = results
            .into_iter()
            .map(|mut r| {
                r.intro_start = r
                    .intro_start
                    .map(|s| if s > 0.0 { s + final_intro_offset } else { s });
                r.outro_start = r.outro_start.map(|s| s + final_outro_offset);
                r
            })
            .collect();

        Ok(ScanResults {
            intro_duration: intro_fp.map(|_| options.length).unwrap_or(0.0),
            outro_duration: outro_fp.map(|_| options.length).unwrap_or(0.0),
            files: final_files,
        })
    }
}
