use serde::{Deserialize, Serialize};

use crate::ui;

#[derive(Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct Config {
    #[serde(default)]
    pub general: General,
    #[serde(default)]
    pub server: blackbird_shared::config::Server,
    #[serde(default)]
    pub style: ui::Style,
}
impl Config {
    pub const FILENAME: &str = "config.toml";

    pub fn load() -> Self {
        match std::fs::read_to_string(Self::FILENAME) {
            Ok(contents) => {
                // Config exists, try to parse it
                match toml::from_str(&contents) {
                    Ok(config) => config,
                    Err(e) => panic!("Failed to parse {}: {e}", Self::FILENAME),
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // No config exists, create default
                tracing::info!("no config file found, creating default config");
                Config::default()
            }
            Err(e) => {
                // Some other IO error occurred while reading
                panic!("Failed to read {}: {e}", Self::FILENAME)
            }
        }
    }

    pub fn save(&self) {
        std::fs::write(Self::FILENAME, toml::to_string(self).unwrap()).unwrap();
        tracing::info!("saved config to {}", Self::FILENAME);
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct General {
    pub album_art_enabled: bool,
    pub repaint_secs: f32,
    pub track_cache_size: usize,
    pub default_shuffle: bool,
    pub window_width: f32,
    pub window_height: f32,
}
impl Default for General {
    fn default() -> Self {
        Self {
            album_art_enabled: true,
            repaint_secs: 1.0,
            track_cache_size: 5,
            default_shuffle: false,
            window_width: 640.0,
            window_height: 1280.0,
        }
    }
}
