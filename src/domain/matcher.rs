//! Matcher logic for audio fingerprints.

use crate::domain::traits::FingerprintMatcher;
use log::debug;

/// Chromaprint's internal hop size in samples at 11025 Hz.
///
/// Chromaprint uses a 4096-sample FFT frame with overlap of `FRAME_SIZE - FRAME_SIZE / 3`,
/// yielding a hop (advance) of `FRAME_SIZE / 3 = 4096 / 3 = 1365` samples (integer division).
/// Source: `test_chromaprint.cpp` in the upstream Chromaprint repository.
pub const CHROMAPRINT_HOP_SAMPLES: usize = 1365;

/// Duration of a single Chromaprint tick in seconds at 11025 Hz.
///
/// Derived from the actual hop size: `1365 / 11025 ≈ 0.12381s`.
pub const TICK_DURATION: f64 = CHROMAPRINT_HOP_SAMPLES as f64 / 11025.0;

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
