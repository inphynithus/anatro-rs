use anyhow::Result;
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

/// Key-Value File System (KV-FS) logic for persisting file scanning outcomes.
pub struct KvFs {
    dir: PathBuf,
}

impl KvFs {
    /// Initializes KV-FS by ensuring the storage directory exists.
    pub fn new() -> Result<Self> {
        let mut dir = dirs::config_dir().unwrap_or_else(|| PathBuf::from(".config"));
        dir.push("anatro-rs");

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
        self.dir.join(format!("{}", hash))
    }

    /// Creates an empty `.tmp` file, signaling the file is under processing.
    pub fn mark_processing(&self, hash: &str) -> Result<()> {
        let tmp = self.tmp_path(hash);
        fs::File::create(&tmp)?;
        Ok(())
    }

    /// Writes the results to the `.tmp` file using the ASCII schema, then renames to finalize it.
    /// Schema: `<intro_start>\t<outro_start>\t<intro_duration>\t<outro_duration>`
    pub fn finalize(
        &self,
        hash: &str,
        intro_start: Option<f64>,
        outro_start: Option<f64>,
        intro_dur: f64,
        outro_dur: f64,
    ) -> Result<()> {
        let tmp = self.tmp_path(hash);

        let intro_str = intro_start
            .map(|v| format!("{:.6}", v))
            .unwrap_or_else(|| "-".to_string());
        let outro_str = outro_start
            .map(|v| format!("{:.6}", v))
            .unwrap_or_else(|| "-".to_string());
        let intro_dur_str = if intro_dur > 0.0 {
            format!("{:.6}", intro_dur)
        } else {
            "-".to_string()
        };
        let outro_dur_str = if outro_dur > 0.0 {
            format!("{:.6}", outro_dur)
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
