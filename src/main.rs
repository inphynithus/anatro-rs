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
//! A Rust CLI application for audio fingerprinting and matching.

use anatro_rs::cli::{Cli, Commands};
use anatro_rs::domain::pipeline::SourceMedia;
use anatro_rs::infrastructure::chromaprint::ChromaprintAdapter;
use anatro_rs::infrastructure::ffmpeg::FfmpegAdapter;
use anyhow::Result;
use clap::Parser;

/// The main entry point of the application.
pub fn main() -> Result<()> {
    let cli = Cli::parse();

    #[allow(unused_results)]
    {
        println!("anatro-rs initialized.");
    }

    match cli.command {
        Commands::Scan { sample } => {
            let ffmpeg = FfmpegAdapter::new();
            let chromaprint = ChromaprintAdapter::new();

            #[allow(unused_results)]
            {
                println!("Processing media file: {}", sample.display());
            }

            let source = SourceMedia::new(sample);

            // Pipeline execution:
            // 1. Extract Audio (includes track selection and mono/downsampling)
            let extracted = source.extract_audio(&ffmpeg)?;

            #[allow(unused_results)]
            {
                println!(
                    "Audio extracted successfully ({} samples).",
                    extracted.buffer().samples().len()
                );
            }

            // 2. Generate Fingerprint
            let fingerprinted = extracted.generate_fingerprint(&chromaprint)?;

            #[allow(unused_results)]
            {
                println!(
                    "Fingerprint generated successfully ({} hashes).",
                    fingerprinted.fingerprint().len()
                );
            }
        }
    }

    Ok(())
}
