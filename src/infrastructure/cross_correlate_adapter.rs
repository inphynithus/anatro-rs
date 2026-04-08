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

        let correlation = Correlate::create_real_f32(src.len(), dst.len(), mode).map_err(|e| {
            DomainError::ExtractionError(format!("Correlation creation failed: {:?}", e))
        })?;

        let corr = correlation
            .correlate_managed(&src, &dst)
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

        // In 'Full' mode, the lag is calculated as max_idx as isize - (src.len() as isize - 1)
        let lag = max_idx as isize - (src.len() as isize - 1);
        Ok(Some(lag))
    }
}
