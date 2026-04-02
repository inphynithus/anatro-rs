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
    /// Scans a target media file to identify intro and outro sections.
    ///
    /// NOTE: This command assumes that `intro_sample.(ext)` and `outro_sample.(ext)`
    /// exist in the current directory. The `(ext)` should be one of the supported
    /// audio codecs (AAC, FLAC, MP3, OPUS, VORBIS). These samples are used as the
    /// reference fingerprints for the scan.
    Scan {
        /// The path to the media file to process (e.g., an MKV or MP4 episode).
        #[arg(short = 's', long = "sample", value_name = "FILE")]
        sample: PathBuf,
    },
}
