//! Infrastructure adapter for high-precision cross-correlation matching.

use crate::domain::DomainError;
use crate::domain::traits::FineMatcher;
use cross_correlate::{Correlate, CrossCorrelationMode};

/// Adapter for the `cross_correlate` crate.
#[derive(Debug, Default)]
pub struct CrossCorrelationAdapter;

impl CrossCorrelationAdapter {
    /// Creates a new CrossCorrelationAdapter.
    pub fn new() -> Self {
        Self
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
        let src: Vec<f32> = reference.iter().map(|&x| x as f32).collect();
        let dst: Vec<f32> = target.iter().map(|&x| x as f32).collect();

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
