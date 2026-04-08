//! Infrastructure adapter for high-precision cross-correlation matching.

use crate::domain::DomainError;
use crate::domain::traits::FineMatcher;
use cross_correlate::{Correlate, CrossCorrelationMode};

/// Adapter for the `cross_correlate` crate.
///
/// Applies high-pass filtering and z-score normalization before computing
/// cross-correlation, ensuring the peak reflects actual similarity rather
/// than signal energy.
#[derive(Debug, Default)]
pub struct CrossCorrelationAdapter;

impl CrossCorrelationAdapter {
    /// Creates a new CrossCorrelationAdapter.
    pub fn new() -> Self {
        Self
    }

    /// Applies a first-order IIR high-pass filter to remove DC offset and
    /// low-frequency content that would otherwise dominate the correlation.
    fn high_pass(data: &[f32]) -> Vec<f32> {
        let alpha = 0.9;
        let mut filtered = Vec::with_capacity(data.len());
        let mut prev_in: f32 = 0.0;
        let mut prev_out: f32 = 0.0;
        for &s in data {
            let out = alpha * (prev_out + s - prev_in);
            filtered.push(out);
            prev_in = s;
            prev_out = out;
        }
        filtered
    }

    /// Applies z-score normalization (mean=0, std=1) so that the
    /// cross-correlation measures shape similarity independently of amplitude.
    fn normalize(data: &[f32]) -> Vec<f32> {
        let len = data.len() as f32;
        let mean = data.iter().sum::<f32>() / len;
        let variance = data.iter().map(|&x| (x - mean).powi(2)).sum::<f32>() / len;
        let std_dev = variance.sqrt().max(1e-6);
        data.iter().map(|&x| (x - mean) / std_dev).collect()
    }
}

impl FineMatcher for CrossCorrelationAdapter {
    fn find_fine_match(
        &self,
        reference: &[i16],
        target: &[i16],
    ) -> Result<Option<isize>, DomainError> {
        if reference.is_empty() || target.is_empty() {
            return Ok(None);
        }

        // Convert i16 to f32 for processing
        let src_raw: Vec<f32> = reference.iter().map(|&x| x as f32).collect();
        let dst_raw: Vec<f32> = target.iter().map(|&x| x as f32).collect();

        // Pre-process: high-pass filter removes DC/low-freq dominance,
        // normalization makes the correlation energy-independent.
        let src = Self::normalize(&Self::high_pass(&src_raw));
        let dst = Self::normalize(&Self::high_pass(&dst_raw));

        let mode = CrossCorrelationMode::Full;

        // The crate's `correlate_managed(buffer, other)` matches `numpy.correlate(buffer, other)`.
        // To find where a short reference (`src`) appears inside a longer target (`dst`),
        // the target must be passed as `buffer` (first) and the reference as `other` (second).
        let correlation = Correlate::create_real_f32(dst.len(), src.len(), mode).map_err(|e| {
            DomainError::ExtractionError(format!("Correlation creation failed: {:?}", e))
        })?;

        let corr = correlation
            .correlate_managed(&dst, &src)
            .map_err(|e| DomainError::ExtractionError(format!("Correlation failed: {:?}", e)))?;

        // Find the peak correlation index
        let mut max_val = f32::MIN;
        let mut max_idx = 0;
        for (i, &val) in corr.iter().enumerate() {
            if val > max_val {
                max_val = val;
                max_idx = i;
            }
        }

        // In 'Full' mode with correlate_managed(target, reference), the lag that gives
        // the position in `target` where `reference` starts is: peak - (reference.len() - 1)
        let lag = max_idx as isize - (src.len() as isize - 1);
        Ok(Some(lag))
    }
}
