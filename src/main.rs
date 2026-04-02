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

use anyhow::Result;
use chromaprint::Chromaprint;
use clap::Parser;

/// Arguments for the CLI application.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Sample file path to process.
    #[arg(short, long)]
    pub file: Option<String>,
}

/// The main entry point of the application.
///
/// # Errors
///
/// Returns an error if the argument parsing fails or if there is an issue with Chromaprint.
pub fn main() -> Result<()> {
    let args = Args::parse();

    #[allow(unused_results)]
    {
        println!("anatro-rs initialized.");
    }

    if let Some(file) = args.file {
        #[allow(unused_results)]
        {
            println!("Received file argument: {}", file);
        }
    }

    let version = Chromaprint::version();
    #[allow(unused_results)]
    {
        println!("Chromaprint version: {}", version);
    }

    Ok(())
}
