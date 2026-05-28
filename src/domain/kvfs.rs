use anyhow::Result;
use serde::Serialize;
use std::fs;
use std::path::PathBuf;

/// Implements the FNV-1a 64-bit hash algorithm and returns a 16-character hex string.
pub fn fnv1a_64_hex(data: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    let prime: u64 = 0x100000001b3;

    for &byte in data.as_bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(prime);
    }

    format!("{:016X}", hash) // Upper case hex
}

/// A parsed record of a scanned file natively residing in KV-FS
#[derive(Debug, Clone, Serialize)]
pub struct KvFsEntry {
    pub intro_start: Option<f64>,
    pub outro_start: Option<f64>,
    pub intro_duration: f64,
    pub outro_duration: f64,
}

/// Key-Value File System (KV-FS) logic for persisting file scanning outcomes.
#[derive(Clone)]
pub struct KvFs {
    dir: PathBuf,
}

impl KvFs {
    /// Initializes KV-FS by ensuring the storage directory exists.
    pub fn new() -> Result<Self> {
        let mut dir = dirs::config_dir().unwrap_or_else(|| PathBuf::from(".config"));
        dir.push("anatro-rs");
        dir.push("cache");

        if !dir.exists() {
            fs::create_dir_all(&dir)?;
        }

        Ok(Self { dir })
    }

    /// Generates the `.tmp` path for a given hash.
    fn tmp_path(&self, hash: &str) -> PathBuf {
        self.dir.join(format!("{}.tmp", hash))
    }

    /// Generates the final finalized path for a given hash.
    fn final_path(&self, hash: &str) -> PathBuf {
        self.dir.join(hash)
    }

    /// Creates an empty `.tmp` file, signaling the file is under processing.
    pub fn mark_processing(&self, hash: &str) -> Result<()> {
        let tmp = self.tmp_path(hash);
        fs::File::create(&tmp)?;
        Ok(())
    }

    /// Reads a cached entry from KV-FS if it exists.
    pub fn read_entry(&self, hash: &str) -> Option<KvFsEntry> {
        let path = self.final_path(hash);
        if !path.exists() {
            return None;
        }

        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                log::warn!(
                    "KV-FS cache read error: Failed to read '{}': {}",
                    path.display(),
                    e
                );
                return None;
            }
        };

        let parts: Vec<&str> = content.trim_end().split('\t').collect();
        if parts.len() != 4 {
            log::error!(
                "KV-FS schema mismatch in file '{}': expected 4 tab-separated values, found {}.",
                path.display(),
                parts.len()
            );
            return None;
        }

        let parse_val = |p: &str| -> Option<f64> { if p == "-" { None } else { p.parse().ok() } };

        let entry = KvFsEntry {
            intro_start: parse_val(parts[0]),
            outro_start: parse_val(parts[1]),
            intro_duration: parse_val(parts[2]).unwrap_or(0.0),
            outro_duration: parse_val(parts[3]).unwrap_or(0.0),
        };

        log::info!("Successfully loaded KV-FS cache entry for hash: {}", hash);

        Some(entry)
    }

    /// Writes the results to the `.tmp` file using the ASCII schema, then renames to finalize it.
    /// Schema: `<intro_start>\t<outro_start>\t<intro_duration>\t<outro_duration>`
    pub fn finalize(&self, hash: &str, entry: &KvFsEntry) -> Result<()> {
        let tmp = self.tmp_path(hash);

        let intro_str = entry
            .intro_start
            .map(|v| format!("{:.6}", v))
            .unwrap_or_else(|| "-".to_string());
        let outro_str = entry
            .outro_start
            .map(|v| format!("{:.6}", v))
            .unwrap_or_else(|| "-".to_string());
        let intro_dur_str = if entry.intro_duration > 0.0 {
            format!("{:.6}", entry.intro_duration)
        } else {
            "-".to_string()
        };
        let outro_dur_str = if entry.outro_duration > 0.0 {
            format!("{:.6}", entry.outro_duration)
        } else {
            "-".to_string()
        };

        let content = format!(
            "{}\t{}\t{}\t{}\n",
            intro_str, outro_str, intro_dur_str, outro_dur_str
        );

        // Write to tmp
        fs::write(&tmp, content)?;

        // Rename (this acts as a commit)
        let final_path = self.final_path(hash);
        fs::rename(&tmp, &final_path)?;

        log::info!("Successfully wrote KV-FS cache entry for hash: {}", hash);

        Ok(())
    }

    /// Removes a .tmp file if the process failed midway.
    pub fn cleanup_tmp(&self, hash: &str) -> Result<()> {
        let tmp = self.tmp_path(hash);
        if tmp.exists() {
            fs::remove_file(&tmp)?;
        }
        Ok(())
    }
}
