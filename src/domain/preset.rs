use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;

/// The structure for the bounds and duration of an intro payload.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IntroPreset {
    pub search_bounds: [f64; 2],
    #[serde(default = "default_duration")]
    pub intro_duration: f64,
    #[serde(default)]
    pub offset: f64,
}

/// The structure for the bounds and duration of an outro payload.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OutroPreset {
    pub search_bounds: [f64; 2],
    #[serde(default = "default_duration")]
    pub outro_duration: f64,
    #[serde(default)]
    pub offset: f64,
}

fn default_duration() -> f64 {
    90.0
}

/// The preset format.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Preset {
    pub intro: IntroPreset,
    pub outro: OutroPreset,
}

/// Type representing the presets JSON configuration payload.
pub type PresetsConfig = HashMap<String, Preset>;

/// Configuration manager for handling user presets.
#[derive(Debug)]
pub struct PresetManager {
    available_presets: PresetsConfig,
    first_key: Option<String>,
}

impl PresetManager {
    /// Loads presets from `dirs::config_dir()/anatro-rs/presets.json` or creates a default one.
    pub fn load_or_create() -> Result<Self> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not resolve system config directory"))?
            .join("anatro-rs");

        if !config_dir.exists() {
            fs::create_dir_all(&config_dir).with_context(|| {
                format!("Failed to create config directory at {:?}", config_dir)
            })?;
        }

        let presets_path = config_dir.join("presets.json");

        if !presets_path.exists() {
            log::info!(
                "presets.json not found, creating default at {:?}",
                presets_path
            );
            let default_config = Self::default_presets_config();
            let json = serde_json::to_string_pretty(&default_config)
                .context("Failed to serialize default presets")?;
            fs::write(&presets_path, json).with_context(|| {
                format!("Failed to write default presets to {:?}", presets_path)
            })?;

            return Ok(Self {
                available_presets: default_config,
                first_key: Some("anime_default".to_string()),
            });
        }

        let content = fs::read_to_string(&presets_path)
            .with_context(|| format!("Failed to read presets from {:?}", presets_path))?;

        // We use serde_json::from_str into a Map<String, Value> first to reliably extract the
        // "first" key parsed (thanks to `preserve_order` feature), then deserialize to the expected shape.
        let raw_map: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&content)
            .with_context(|| "Failed to parse presets.json as a JSON object")?;

        let first_key = raw_map.keys().next().cloned();

        let config: PresetsConfig = serde_json::from_str(&content)
            .with_context(|| "Failed to parse presets.json into PresetsConfig schema")?;

        if config.is_empty() {
            return Err(anyhow::anyhow!("presets.json is empty or invalid."));
        }

        Ok(Self {
            available_presets: config,
            first_key,
        })
    }

    /// Fetches a preset by name or falls back to the first one available.
    pub fn get_preset(&self, name_opt: Option<&str>) -> Result<(String, Preset)> {
        if let Some(name) = name_opt {
            if let Some(preset) = self.available_presets.get(name) {
                Ok((name.to_string(), preset.clone()))
            } else {
                let available: Vec<_> = self.available_presets.keys().cloned().collect();
                Err(anyhow::anyhow!(
                    "Preset '{}' not found. Available presets: {}",
                    name,
                    available.join(", ")
                ))
            }
        } else {
            let key = self
                .first_key
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("No presets available in configuration"))?;
            let preset = self.available_presets.get(key).unwrap(); // safe because first_key comes from the map
            Ok((key.clone(), preset.clone()))
        }
    }

    fn default_presets_config() -> PresetsConfig {
        let default_preset = Preset {
            intro: IntroPreset {
                search_bounds: [0.0, 0.25],
                intro_duration: 90.0,
                offset: 0.0,
            },
            outro: OutroPreset {
                search_bounds: [0.75, 1.0],
                outro_duration: 90.0,
                offset: 0.0,
            },
        };
        let mut config = HashMap::new();
        config.insert("anime_default".to_string(), default_preset);
        config
    }
}
