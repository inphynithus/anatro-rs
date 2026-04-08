//! Matcher logic for audio fingerprints.

use crate::domain::traits::FingerprintMatcher;
use log::debug;

/// Implements a sliding window discrete cross-correlation algorithm.
#[derive(Debug, Default)]
pub struct SlidingWindowMatcher;

impl SlidingWindowMatcher {
    /// Creates a new SlidingWindowMatcher.
    pub fn new() -> Self {
        Self
    }
}

impl FingerprintMatcher for SlidingWindowMatcher {
    fn find_match(&self, reference: &[u32], target: &[u32], threshold: u32) -> Option<usize> {
        if reference.is_empty() || target.is_empty() || reference.len() > target.len() {
            debug!(
                "Invalid lengths: ref {}, target {}",
                reference.len(),
                target.len()
            );
            return None;
        }

        let m = reference.len();
        let mut min_error = u32::MAX;
        let mut best_match_idx = None;

        // Iterate over the target fingerprint with a sliding window of size M.
        // N - M + 1 windows will be evaluated.
        for (k, window) in target.windows(m).enumerate() {
            // Calculate Hamming distance using bitwise XOR and count_ones
            let error: u32 = window
                .iter()
                .zip(reference.iter())
                .map(|(t, r)| (t ^ r).count_ones())
                .sum();

            if error < min_error {
                min_error = error;
                best_match_idx = Some(k);
            }
        }

        if let Some(idx) = best_match_idx {
            debug!(
                "Best match at index {} with error {} (Threshold: {})",
                idx, min_error, threshold
            );
            if min_error <= threshold {
                return Some(idx);
            } else {
                debug!(
                    "Match rejected. Minimum error {} exceeds threshold {}.",
                    min_error, threshold
                );
            }
        }

        None
    }
}

use cross_correlate::{Correlate, CrossCorrelateError, CrossCorrelationMode, FftExecutor};
use num_complex::Complex;
use rustfft::{Fft, FftPlanner};
use std::sync::Arc;

/// Finds the exact fine match using cross-correlation.
#[derive(Debug, Default)]
pub struct CrossCorrelationMatcher;

impl CrossCorrelationMatcher {
    pub fn new() -> Self {
        Self
    }

    /// Finds the exact lag (in samples) of the reference within the target window.
    pub fn find_fine_match(
        &self,
        reference: &[i16],
        target: &[i16],
    ) -> Result<Option<isize>, String> {
        if reference.is_empty() || target.is_empty() {
            return Ok(None);
        }

        // Convert i16 to f32
        let src: Vec<f32> = reference.iter().map(|&x| x as f32).collect();
        let dst: Vec<f32> = target.iter().map(|&x| x as f32).collect();

        let mode = CrossCorrelationMode::Full;
        let fft_size = mode.fft_size(&src, &dst);

        let mut planner = FftPlanner::<f32>::new();
        let fft_forward = planner.plan_fft_forward(fft_size);
        let fft_inverse = planner.plan_fft_inverse(fft_size);

        struct FftCorrelate {
            executor: Arc<dyn Fft<f32>>,
        }
        impl FftExecutor<f32> for FftCorrelate {
            fn process(&self, in_out: &mut [Complex<f32>]) -> Result<(), CrossCorrelateError> {
                self.executor.process(in_out);
                Ok(())
            }
            fn length(&self) -> usize {
                self.executor.len()
            }
        }

        let correlation = Correlate::create_real_f32(
            mode,
            Box::new(FftCorrelate {
                executor: fft_forward,
            }),
            Box::new(FftCorrelate {
                executor: fft_inverse,
            }),
        )
        .map_err(|e| format!("Correlation creation failed: {:?}", e))?;

        let corr = correlation
            .correlate_managed(&src, &dst)
            .map_err(|e| format!("Correlation failed: {:?}", e))?;

        // The correlate_managed returns an array of correlation values.
        // We need to find the peak.
        let mut max_val = f32::MIN;
        let mut max_idx = 0;
        for (i, &val) in corr.iter().enumerate() {
            if val > max_val {
                max_val = val;
                max_idx = i;
            }
        }

        // In 'Full' mode, the lag is calculated as max_idx - (src.len() - 1)
        let lag = max_idx as isize - (src.len() as isize - 1);
        Ok(Some(lag))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match() {
        let matcher = SlidingWindowMatcher::new();
        let target = vec![1, 2, 3, 4, 5, 6, 7];
        let reference = vec![3, 4, 5];

        let idx = matcher.find_match(&reference, &target, 0);
        assert_eq!(idx, Some(2));
    }

    #[test]
    fn test_partial_match_within_threshold() {
        let matcher = SlidingWindowMatcher::new();
        // 5 in binary is 0101, 4 is 0100 (distance = 1)
        let target = vec![1, 2, 5, 8, 9];
        let reference = vec![2, 4, 8];

        // The sub-slice [2, 5, 8] compared to [2, 4, 8] has error:
        // 2^2 = 0
        // 5^4 = 0101 ^ 0100 = 0001 (1 bit)
        // 8^8 = 0
        // Total error = 1.
        let idx = matcher.find_match(&reference, &target, 1);
        assert_eq!(idx, Some(1));
    }

    #[test]
    fn test_rejected_by_threshold() {
        let matcher = SlidingWindowMatcher::new();
        let target = vec![1, 2, 5, 8, 9];
        let reference = vec![2, 4, 8];

        // Total error is 1, but threshold is 0.
        let idx = matcher.find_match(&reference, &target, 0);
        assert_eq!(idx, None);
    }

    #[test]
    fn test_invalid_lengths() {
        let matcher = SlidingWindowMatcher::new();
        let target = vec![1, 2];
        let reference = vec![1, 2, 3];

        assert_eq!(matcher.find_match(&reference, &target, 10), None);
        assert_eq!(matcher.find_match(&[], &target, 10), None);
    }
}
