//! CLI definitions for anotro-rs.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// The main command line interface for anotro-rs.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// The subcommand to execute.
    #[command(subcommand)]
    pub command: Commands,
}

/// The available subcommands for the application.
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Scans a target media file to find a match for a given reference sample.
    ///
    /// The target file is processed using search space heuristics (Intro: 0.0-0.25, Outro: 0.7-1.0).
    Scan {
        /// The path to the media file to process (e.g., an MKV or MP4 episode).
        #[arg(short = 't', long = "target", value_name = "FILE")]
        target: PathBuf,
        /// The path to the reference sample to search for (e.g., intro_sample.wav).
        #[arg(short = 's', long = "sample", value_name = "FILE")]
        sample: PathBuf,
        /// Positive or negative offset in seconds to apply to the match result.
        #[arg(short = 'f', long = "offset", default_value_t = 0.0)]
        offset: f64,
        /// The assumed length of the intro/outro in seconds for reporting.
        #[arg(short = 'l', long = "length", default_value_t = 90.0)]
        length: f64,
    },
    /// Extracts an audio sample from a media file for a given timestamp range.
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
    },
    /// Extracts an audio sample, downmixes to mono and resamples to 11025Hz for testing.
    SampleTest {
        /// The path to the media file to process (e.g., an MKV or MP4 episode).
        #[arg(short = 't', long = "target", value_name = "FILE")]
        target: PathBuf,
        /// The timestamp range for extraction (e.g., '00:01:00-00:02:00').
        #[arg(short = 'r', long = "range", value_name = "HH:MM:SS-HH:MM:SS")]
        range: String,
        /// The path to save the extracted sample.
        #[arg(short = 'o', long = "output", value_name = "FILE")]
        output: PathBuf,
    },
}
