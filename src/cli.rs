//! CLI definitions for anotro-rs.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// The main command line interface for anotro-rs.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Enable logging output. Accepts an optional level: debug, info, warn.
    /// Defaults to 'info' when the flag is present without a value.
    #[arg(
        long = "log",
        global = true,
        num_args = 0..=1,
        default_missing_value = "info",
        value_name = "LEVEL"
    )]
    pub log: Option<String>,

    /// The subcommand to execute.
    #[command(subcommand)]
    pub command: Commands,
}

/// The available subcommands for the application.
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Scans target media files to find a match for a given reference sample.
    ///
    /// The search space is configured via a preset (see `--preset`). When no preset is
    /// specified, the first entry defined in `presets.json` is used as the default.
    /// Presets define the intro/outro search bounds and expected durations.
    ///
    /// To view or edit presets, see: `~/.config/anatro-rs/presets.json`
    Scan {
        /// The target directory containing media files (.mkv, .mp4) to process.
        #[arg(long = "target", value_name = "DIR")]
        target: Option<PathBuf>,
        /// A single target media file to process.
        #[arg(short = 'f', long = "file", value_name = "FILE")]
        file: Option<PathBuf>,
        /// Print the results as JSON to stdout.
        #[arg(long = "json")]
        json: bool,
        /// The timestamp (MM:SS) of the intro in the reference episode.
        #[arg(long = "sample-intro", value_name = "MM:SS")]
        sample_intro: Option<String>,
        /// The timestamp (MM:SS) of the outro in the reference episode.
        #[arg(long = "sample-outro", value_name = "MM:SS")]
        sample_outro: Option<String>,
        /// The reference episode file (name or path) to extract the samples from.
        #[arg(long = "sample-reference", value_name = "FILE")]
        sample_reference: String,
        /// The size of the reference sample to extract in seconds.
        #[arg(long = "sample-size", default_value_t = 10.0)]
        sample_size: f64,
        /// Positive or negative offset in seconds to apply to the match result.
        #[arg(long = "offset", default_value_t = 0.0)]
        offset: f64,
        /// Force re-scan and overwrite existing cached entries in KV-FS.
        #[arg(long = "force")]
        force: bool,
        /// Enable progress bar.
        #[arg(short = 'p', long = "progress")]
        progress: bool,
        /// The name of the preset to use for scanning.
        #[arg(long = "preset", value_name = "NAME")]
        preset: Option<String>,
        /// Number of worker threads to use for parallel scanning.
        #[arg(short = 't', long = "threads", default_value_t = 4)]
        threads: usize,
        /// The audio track index to use (e.g., 0 for the first audio track).
        #[arg(long = "track")]
        track: Option<usize>,
    },
    /// Detailed debugging of a specific match to find discrepancies.
    ///
    /// **Available only in dev builds** (`--features dev`).
    #[cfg(feature = "dev")]
    Debug {
        /// The target media file to process.
        #[arg(short = 'f', long = "file", value_name = "FILE")]
        file: PathBuf,
        /// The expected timestamp in seconds for verification.
        #[arg(short = 'e', long = "expected", value_name = "SECONDS")]
        expected: f64,
        /// The timestamp (HH:MM:SS or seconds) of the intro in the reference episode.
        #[arg(long = "sample-intro", value_name = "TIMESTAMP")]
        sample_intro: Option<String>,
        /// The timestamp (HH:MM:SS or seconds) of the outro in the reference episode.
        #[arg(long = "sample-outro", value_name = "TIMESTAMP")]
        sample_outro: Option<String>,
        /// The reference episode file (path) to extract the samples from.
        #[arg(long = "sample-reference", value_name = "FILE")]
        sample_reference: PathBuf,
        /// The size of the reference sample to extract in seconds.
        #[arg(long = "sample-size", default_value_t = 10.0)]
        sample_size: f64,
        /// The audio track index to use (e.g., 0 for the first audio track).
        #[arg(long = "track")]
        track: Option<usize>,
    },
    /// Extracts an audio sample from a media file for a given timestamp range.
    ///
    /// **Available only in dev builds** (`--features dev`).
    #[cfg(feature = "dev")]
    SampleExtract {
        /// The path to the media file to process (e.g., an MKV or MP4 episode).
        #[arg(short = 't', long = "target", value_name = "FILE")]
        target: PathBuf,
        /// The timestamp range for extraction (e.g., '00:01:00-00:02:00').
        #[arg(short = 'r', long = "range", value_name = "HH:MM:SS-HH:MM:SS")]
        range: String,
        /// The path to save the extracted sample.
        #[arg(short = 'o', long = "output", value_name = "FILE")]
        output: PathBuf,
        /// The audio track index to use (e.g., 0 for the first audio track).
        #[arg(long = "track")]
        track: Option<usize>,
    },
    /// Checks whether a media file has a cached entry in the KV-FS database.
    ///
    /// Computes the file's FNV-1a hash and looks it up in `~/.config/anatro-rs/cache/`.
    /// Exits with code `0` if the entry is found, `1` if it is not.
    Check {
        /// The media file to look up.
        #[arg(short = 'f', long = "file", value_name = "FILE")]
        file: PathBuf,
        /// Print the cached entry as JSON to stdout.
        #[arg(long = "json")]
        json: bool,
    },
}
