use serde::{Deserialize, Serialize};

use crate::ui;
use blackbird_core::{PlaybackMode, blackbird_state::TrackId};

#[derive(Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct Config {
    #[serde(default)]
    pub general: General,
    #[serde(default)]
    pub server: blackbird_shared::config::Server,
    #[serde(default)]
    pub style: ui::Style,
    #[serde(default)]
    pub last_playback: LastPlayback,
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
    pub repaint_secs: f32,
    pub window_position_x: i32,
    pub window_position_y: i32,
    pub window_width: u32,
    pub window_height: u32,
    pub volume: f32,
}
impl Default for General {
    fn default() -> Self {
        Self {
            repaint_secs: 1.0,
            window_position_x: 0,
            window_position_y: 0,
            window_width: 640,
            window_height: 1280,
            volume: 1.0,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct LastPlayback {
    pub track_id: Option<TrackId>,
    pub track_position_secs: f64,
    pub playback_mode: PlaybackMode,
}
impl Default for LastPlayback {
    fn default() -> Self {
        Self {
            track_id: None,
            track_position_secs: 0.0,
            playback_mode: PlaybackMode::default(),
        }
    }
}
