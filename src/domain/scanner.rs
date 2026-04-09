//! Orchestration logic for scanning media files.

use crate::domain::kvfs::{KvFs, fnv1a_64_hex};
use crate::domain::matcher::{CHROMAPRINT_HOP_SAMPLES, SlidingWindowMatcher, TICK_DURATION};
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
use std::sync::mpsc;

/// Options for the scan operation.
pub struct ScanOptions {
    pub sample_intro: Option<String>,
    pub sample_outro: Option<String>,
    pub sample_reference: String,
    pub sample_size: f64,
    pub offset: f64,
    pub json: bool,
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

        let sweep_tgt =
            target_selected
                .clone()
                .extract_audio_range(&self.extractor, sweep_start, sweep_end)?;
        self.extractor.export_wav(
            sweep_tgt.buffer(),
            &PathBuf::from("debug_sweep_target_11k.wav"),
        )?;
        log::info!(
            "Exported debug_sweep_target_11k.wav (window: {:.1}s to {:.1}s)",
            sweep_start,
            sweep_end
        );

        // Pattern: first 2s of 11k reference
        let pattern_len = (2.0 * 11025.0) as usize;
        let pattern =
            &ref_audio.buffer().samples()[..pattern_len.min(ref_audio.buffer().samples().len())];

        // Manual peak analysis
        let src_f32: Vec<f32> = pattern.iter().map(|&x| x as f32).collect();
        let dst_f32: Vec<f32> = sweep_tgt
            .buffer()
            .samples()
            .iter()
            .map(|&x| x as f32)
            .collect();

        // Apply pre-processing identical to the adapter
        let norm_src = self.normalize_f32(&self.high_pass_f32(&src_f32));
        let norm_dst = self.normalize_f32(&self.high_pass_f32(&dst_f32));

        let correlation = cross_correlate::Correlate::create_real_f32(
            norm_dst.len(),
            norm_src.len(),
            cross_correlate::CrossCorrelationMode::Full,
        )
        .map_err(|e| anyhow::anyhow!("Correlation creation failed: {:?}", e))?;
        let corr = correlation
            .correlate_managed(&norm_dst, &norm_src)
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

        let best_peak_ts =
            sweep_start + ((peaks[0].0 as f64 - (norm_src.len() as f64 - 1.0)) / 11025.0);
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

        log::debug!("Resolved reference media: {}", ref_path.display());

        let ref_source = SourceMedia::new(ref_path.clone());
        let ref_selected = ref_source.select_track(&self.extractor)?;

        // 2. Extract Reference Fingerprints and Buffers
        //
        // Instead of independently extracting and resampling a 10-second clip (which
        // produces a different sample grid than the targets), we extract the reference
        // file's full segmented audio for each search space, fingerprint it, then
        // slice the fingerprint and audio buffer at the position indicated by the user.
        // This ensures the reference fingerprint is a direct slice of the same pipeline
        // used for matching, guaranteeing self-scans produce exact matches.
        let mut intro_fp = None;
        let mut intro_buf = None;
        if let Some(ref intro_hms) = options.sample_intro {
            log::info!("Processing reference intro starting at {}...", intro_hms);
            let start_sec = self.extractor.hms_to_seconds(intro_hms)?;

            // Extract the reference file's segmented audio for the Intro search space,
            // using the same pipeline that targets use.
            let ref_segmented = ref_selected
                .clone()
                .extract_segmented_audio(&self.extractor, SearchSpace::Intro)?;
            let ref_seg_fps = ref_segmented.generate_segmented_fingerprints(&self.chromaprint)?;

            // Convert the user-supplied timestamp into an index within the segmented
            // fingerprint array and a sample offset within the audio buffer.
            let local_sec = start_sec - ref_seg_fps.offset_sec();
            let fp_start = (local_sec / TICK_DURATION).round() as usize;
            let sample_size_ticks = (options.sample_size / TICK_DURATION).round() as usize;
            let fp_end = (fp_start + sample_size_ticks).min(ref_seg_fps.fingerprint().len());

            if fp_start >= ref_seg_fps.fingerprint().len() {
                return Err(anyhow::anyhow!(
                    "sample_intro {:.3}s is outside the Intro search space (0..{:.1}s)",
                    start_sec,
                    ref_seg_fps.offset_sec()
                        + ref_seg_fps.fingerprint().len() as f64 * TICK_DURATION
                ));
            }

            intro_fp = Some(ref_seg_fps.fingerprint()[fp_start..fp_end].to_vec());

            // Slice the corresponding audio samples.
            let audio_start = (local_sec * 11025.0).round() as usize;
            let audio_end = ((local_sec + options.sample_size) * 11025.0).round() as usize;
            let audio_end = audio_end.min(ref_seg_fps.buffer().samples().len());
            if audio_start < audio_end {
                intro_buf = Some(Arc::new(
                    ref_seg_fps.buffer().samples()[audio_start..audio_end].to_vec(),
                ));
            }

            log::debug!(
                "Reference intro successfully extracted: FP[{}..{}] ({} values), audio[{}..{}] ({} samples)",
                fp_start,
                fp_end,
                fp_end - fp_start,
                audio_start,
                audio_end,
                audio_end - audio_start
            );
        }

        let mut outro_fp = None;
        let mut outro_buf = None;
        if let Some(ref outro_hms) = options.sample_outro {
            log::info!("Processing reference outro starting at {}...", outro_hms);
            let start_sec = self.extractor.hms_to_seconds(outro_hms)?;

            let ref_segmented = ref_selected
                .clone()
                .extract_segmented_audio(&self.extractor, SearchSpace::Outro)?;
            let ref_seg_fps = ref_segmented.generate_segmented_fingerprints(&self.chromaprint)?;

            let local_sec = start_sec - ref_seg_fps.offset_sec();
            let fp_start = (local_sec / TICK_DURATION).round() as usize;
            let sample_size_ticks = (options.sample_size / TICK_DURATION).round() as usize;
            let fp_end = (fp_start + sample_size_ticks).min(ref_seg_fps.fingerprint().len());

            if fp_start >= ref_seg_fps.fingerprint().len() {
                return Err(anyhow::anyhow!(
                    "sample_outro {:.3}s is outside the Outro search space ({:.1}s..end)",
                    start_sec,
                    ref_seg_fps.offset_sec()
                ));
            }

            outro_fp = Some(ref_seg_fps.fingerprint()[fp_start..fp_end].to_vec());

            let audio_start = (local_sec * 11025.0).round() as usize;
            let audio_end = ((local_sec + options.sample_size) * 11025.0).round() as usize;
            let audio_end = audio_end.min(ref_seg_fps.buffer().samples().len());
            if audio_start < audio_end {
                outro_buf = Some(Arc::new(
                    ref_seg_fps.buffer().samples()[audio_start..audio_end].to_vec(),
                ));
            }

            log::debug!(
                "Reference outro successfully extracted: FP[{}..{}] ({} values), audio[{}..{}] ({} samples)",
                fp_start,
                fp_end,
                fp_end - fp_start,
                audio_start,
                audio_end,
                audio_end - audio_start
            );
        }

        // 3. Setup Thread Pool & Progress
        let num_threads = options.threads.min(files.len().max(1));
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(num_threads)
            .build()?;

        let multi_progress = indicatif::MultiProgress::new();

        let mut thread_bars = Vec::new();
        if options.progress {
            let num_spinners = num_threads.min(8);
            for i in 0..num_spinners {
                let spinner = multi_progress.add(indicatif::ProgressBar::new_spinner());
                spinner.set_style(
                    indicatif::ProgressStyle::default_spinner()
                        .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
                        .template("[{elapsed_precise}] {spinner:.green} Worker {prefix:>2}: {msg}")
                        .unwrap_or_else(|_| indicatif::ProgressStyle::default_spinner()),
                );
                spinner.set_prefix(format!("{}", i + 1));
                spinner.set_message("Waiting...");
                spinner.enable_steady_tick(std::time::Duration::from_millis(150));
                thread_bars.push(spinner);
            }
        }

        let main_pb = if options.progress {
            let pb = multi_progress.add(indicatif::ProgressBar::new(files.len() as u64));
            pb.set_style(
                indicatif::ProgressStyle::default_bar()
                    .template(
                        "[{elapsed_precise}]   Overall  : [{bar:40.green/white}] {pos}/{len} files ({eta})",
                    )
                    .unwrap_or_else(|e| {
                        log::warn!("Failed to set progress bar template: {}", e);
                        indicatif::ProgressStyle::default_bar()
                    })
                    .progress_chars("=>-"),
            );
            Some(pb)
        } else {
            None
        };

        let intro_dur = if intro_fp.is_some() {
            options.length
        } else {
            0.0
        };
        let outro_dur = if outro_fp.is_some() {
            options.length
        } else {
            0.0
        };

        enum ScannerEvent {
            Started(String),
            Finished(FileResult),
        }

        let (tx, rx) = mpsc::channel();
        let kvfs_enabled = !options.json;

        let kvfs_thread = std::thread::spawn(move || {
            let kvfs = if kvfs_enabled { KvFs::new().ok() } else { None };

            for msg in rx {
                match msg {
                    ScannerEvent::Started(filename) => {
                        if let Some(fs) = &kvfs {
                            let hash = fnv1a_64_hex(&filename);
                            let _ = fs.mark_processing(&hash);
                        }
                    }
                    ScannerEvent::Finished(res) => {
                        if let Some(fs) = &kvfs {
                            let hash = fnv1a_64_hex(&res.filename);
                            let _ = fs.finalize(
                                &hash,
                                res.intro_start,
                                res.outro_start,
                                intro_dur,
                                outro_dur,
                            );
                        }
                    }
                }
            }
        });

        // 4. Parallel Processing
        let results: Vec<FileResult> = pool.install(|| {
            files
                .into_par_iter()
                .map_with(tx, |tx, file| {
                    let file_name = file
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_string();

                    let _ = tx.send(ScannerEvent::Started(file_name.clone()));

                    if !thread_bars.is_empty() {
                        let thread_idx = rayon::current_thread_index().unwrap_or(usize::MAX);
                        if thread_idx < thread_bars.len() {
                            let spinner = &thread_bars[thread_idx];
                            spinner.set_message(file_name.clone());
                            spinner.tick();
                        }
                    }

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

                                log::debug!(
                                    "[{}] Coarse: idx={}, time={:.3}s",
                                    file_name, idx, start_total
                                );

                                if let Some(ref ref_audio) = intro_buf {
                                    let target_audio = segmented_fps.buffer().samples();
                                    let coarse_sample = idx * CHROMAPRINT_HOP_SAMPLES;
                                    let window_start = coarse_sample.saturating_sub(5 * 11025);
                                    let window_end = (coarse_sample + ref_audio.len() + 5 * 11025)
                                        .min(target_audio.len());

                                    log::debug!(
                                        "[{}] Fine window: samples {}..{} ({:.3}s..{:.3}s), ref_len={}, target_len={}",
                                        file_name,
                                        window_start, window_end,
                                        segmented_fps.offset_sec() + window_start as f64 / 11025.0,
                                        segmented_fps.offset_sec() + window_end as f64 / 11025.0,
                                        ref_audio.len(),
                                        target_audio.len()
                                    );

                                    if window_start < window_end {
                                        match fine_matcher.find_fine_match(
                                            ref_audio,
                                            &target_audio[window_start..window_end],
                                        ) {
                                            Ok(Some(lag)) => {
                                                let fine_total = segmented_fps.offset_sec()
                                                    + ((window_start as isize + lag) as f64
                                                        / 11025.0);
                                                log::debug!(
                                                    "[{}] Fine: lag={}, time={:.6}s (correction={:.3}s)",
                                                    file_name, lag, fine_total,
                                                    fine_total - start_total
                                                );
                                                start_total = fine_total;
                                            }
                                            Ok(None) => {
                                                log::debug!(
                                                    "[{}] Fine matcher returned None, using coarse",
                                                    file_name
                                                );
                                            }
                                            Err(e) => {
                                                log::debug!(
                                                    "[{}] Fine matcher error: {}, using coarse",
                                                    file_name, e
                                                );
                                            }
                                        }
                                    }
                                }
                                file_res.intro_start = Some(if start_total > 0.0 { start_total + options.offset } else { start_total });
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

                                log::debug!(
                                    "[{}] Outro Coarse: idx={}, time={:.3}s",
                                    file_name, idx, start_total
                                );

                                if let Some(ref ref_audio) = outro_buf {
                                    let target_audio = segmented_fps.buffer().samples();
                                    let coarse_sample = idx * CHROMAPRINT_HOP_SAMPLES;
                                    let window_start = coarse_sample.saturating_sub(5 * 11025);
                                    let window_end = (coarse_sample + ref_audio.len() + 5 * 11025)
                                        .min(target_audio.len());

                                    log::debug!(
                                        "[{}] Outro Fine window: samples {}..{} ({:.3}s..{:.3}s), ref_len={}, target_len={}",
                                        file_name,
                                        window_start, window_end,
                                        segmented_fps.offset_sec() + window_start as f64 / 11025.0,
                                        segmented_fps.offset_sec() + window_end as f64 / 11025.0,
                                        ref_audio.len(),
                                        target_audio.len()
                                    );

                                    if window_start < window_end {
                                        match fine_matcher.find_fine_match(
                                            ref_audio,
                                            &target_audio[window_start..window_end],
                                        ) {
                                            Ok(Some(lag)) => {
                                                let fine_total = segmented_fps.offset_sec()
                                                    + ((window_start as isize + lag) as f64
                                                        / 11025.0);
                                                log::debug!(
                                                    "[{}] Outro Fine: lag={}, time={:.6}s (correction={:.3}s)",
                                                    file_name, lag, fine_total,
                                                    fine_total - start_total
                                                );
                                                start_total = fine_total;
                                            }
                                            Ok(None) => {
                                                log::debug!(
                                                    "[{}] Fine matcher returned None, using coarse",
                                                    file_name
                                                );
                                            }
                                            Err(e) => {
                                                log::warn!(
                                                    "[{}] Fine matcher error: {}, using coarse",
                                                    file_name, e
                                                );
                                            }
                                        }
                                    }
                                }
                                file_res.outro_start = Some(start_total + options.offset);
                            }
                        }
                        Ok(())
                    };

                    if let Err(e) = process_file() {
                        log::warn!("Error processing file {}: {}", file_name, e);
                    }

                    if !thread_bars.is_empty() {
                        let thread_idx = rayon::current_thread_index().unwrap_or(usize::MAX);
                        if thread_idx < thread_bars.len() {
                            let spinner = &thread_bars[thread_idx];
                            spinner.set_message("Waiting...");
                            spinner.tick();
                        }
                    }

                    if let Some(ref pb) = main_pb {
                        pb.inc(1);
                    }

                    let _ = tx.send(ScannerEvent::Finished(file_res.clone()));

                    file_res
                })
                .collect()
        });

        if let Some(pb) = main_pb {
            pb.finish_with_message("Done");
        }

        if options.progress {
            for spinner in thread_bars {
                spinner.finish_and_clear();
            }
        }

        // 5. Cleanup
        let _ = kvfs_thread.join();

        Ok(ScanResults {
            intro_duration: intro_dur,
            outro_duration: outro_dur,
            files: results,
        })
    }
}
