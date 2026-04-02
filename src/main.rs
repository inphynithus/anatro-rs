#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::clone_on_ref_ptr,
    clippy::todo,
    missing_docs,
    missing_debug_implementations,
    trivial_casts,
    trivial_numeric_casts,
    unused_results,
    improper_ctypes
)]

//! # anotro-rs
//!
//! A Rust environment with chromaprint and clap, configured with strict Clippy lints.
//! It supports MKV and MP4 containers and AAC, FLAC, MP3, OPUS, and VORBIS audio codecs.

use anyhow::Result;
use chromaprint::Chromaprint;
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

/// The main entry point of the application.
///
/// # Errors
///
/// Returns an error if the argument parsing fails or if there is an issue with Chromaprint or FFmpeg.
pub fn main() -> Result<()> {
    let cli = Cli::parse();

    #[allow(unused_results)]
    {
        println!("anatro-rs initialized.");
    }

    match cli.command {
        Commands::Scan { sample } => {
            #[allow(unused_results)]
            {
                println!("Initializing scan for file: {}", sample.display());
                println!(
                    "NOTE: Assuming `intro_sample.(ext)` and `outro_sample.(ext)` are present in the directory."
                );
            }

            // Placeholder for Typestate pattern execution:
            // 1. Initialize Ports (FFmpeg extraction adapter, Chromaprint generation adapter).
            // 2. let initial_state = ExtractionState::new(sample);
            // 3. let fingerprint_state = initial_state.extract_audio()?;
            // 4. let matching_state = fingerprint_state.generate_fingerprints()?;
            // 5. let result = matching_state.find_matches(intro_ref, outro_ref)?;

            #[allow(unused_results)]
            {
                println!("Scan placeholder logic executed successfully.");
            }
        }
    }

    let version = Chromaprint::version();
    #[allow(unused_results)]
    {
        println!("Chromaprint version: {}", version);
    }

    // Verify FFmpeg initialization
    ffmpeg_next::init()?;
    #[allow(unused_results)]
    {
        println!("FFmpeg initialized.");
    }

    // Verify Rayon (just a simple parallel check)
    let _ = rayon::join(|| (), || ());
    #[allow(unused_results)]
    {
        println!("Rayon initialized.");
    }

    Ok(())
}
