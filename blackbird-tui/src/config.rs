use blackbird_core::{PlaybackMode, blackbird_state::TrackId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct Config {
    #[serde(default)]
    pub general: General,
    #[serde(default)]
    pub server: blackbird_shared::config::Server,
    #[serde(default)]
    pub last_playback: LastPlayback,
}
impl Config {
    pub const FILENAME: &str = "tui-config.toml";

    pub fn load() -> Self {
        match std::fs::read_to_string(Self::FILENAME) {
            Ok(contents) => match toml::from_str(&contents) {
                Ok(config) => config,
                Err(e) => panic!("Failed to parse {}: {e}", Self::FILENAME),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::info!("no config file found, creating default config");
                Config::default()
            }
            Err(e) => {
                panic!("Failed to read {}: {e}", Self::FILENAME)
            }
        }
    }

    pub fn save(&self) {
        std::fs::write(Self::FILENAME, toml::to_string(self).unwrap()).unwrap();
        tracing::info!("saved config to {}", Self::FILENAME);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct General {
    pub volume: f32,
    pub tick_rate_ms: u64,
}
impl Default for General {
    fn default() -> Self {
        Self {
            volume: 1.0,
            tick_rate_ms: 100,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
