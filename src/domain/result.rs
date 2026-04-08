//! Result types for audio scanning.

use serde::Serialize;

/// Container for all scan results.
#[derive(Debug, Serialize)]
pub struct ScanResults {
    /// The assumed length of the intro in seconds.
    pub intro_duration: f64,
    /// The assumed length of the outro in seconds.
    pub outro_duration: f64,
    /// The list of results for each processed file.
    pub files: Vec<FileResult>,
}

/// Result for a single media file.
#[derive(Debug, Serialize, Clone)]
pub struct FileResult {
    /// The name of the file.
    pub filename: String,
    /// The start time of the intro in seconds, if found.
    pub intro_start: Option<f64>,
    /// The start time of the outro in seconds, if found.
    pub outro_start: Option<f64>,
}
