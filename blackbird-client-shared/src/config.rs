/// Configuration types shared between the egui and TUI clients.
use std::time::Duration;

use blackbird_core::{PlaybackMode, SortOrder, blackbird_state::TrackId};
use serde::{Deserialize, Serialize};

/// Load a TOML config file, returning `T::default()` if the file doesn't exist.
/// Panics on parse errors or unexpected I/O errors.
pub fn load_config<T: Default + serde::de::DeserializeOwned>(filename: &str) -> T {
    match std::fs::read_to_string(filename) {
        Ok(contents) => match toml::from_str(&contents) {
            Ok(config) => config,
            Err(e) => panic!("Failed to parse {filename}: {e}"),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::info!("no config file found, creating default config");
            T::default()
        }
        Err(e) => panic!("Failed to read {filename}: {e}"),
    }
}

/// Shared configuration fields used by both the egui and TUI clients.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    /// Server connection settings.
    #[serde(default)]
    pub server: blackbird_shared::config::Server,
    /// Last playback state, persisted across sessions.
    #[serde(default)]
    pub last_playback: LastPlayback,
    /// Whether to show the current synced lyric line inline in the player UI.
    #[serde(default = "default_true")]
    pub show_inline_lyrics: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            server: Default::default(),
            last_playback: Default::default(),
            show_inline_lyrics: true,
        }
    }
}

fn default_true() -> bool {
    true
}

/// Last playback state, persisted across sessions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct LastPlayback {
    /// The track that was playing when the client was last closed.
    pub track_id: Option<TrackId>,
    /// The position within the track, in seconds.
    pub track_position_secs: f64,
    /// The playback mode that was active.
    pub playback_mode: PlaybackMode,
    /// The library sort order that was active.
    pub sort_order: SortOrder,
}
impl LastPlayback {
    /// Returns the track ID and position if a track was saved, suitable for
    /// passing to `LogicArgs::last_playback`.
    pub fn as_track_and_position(&self) -> Option<(TrackId, Duration)> {
        self.track_id
            .clone()
            .map(|id| (id, Duration::from_secs_f64(self.track_position_secs)))
    }
}
impl Default for LastPlayback {
    fn default() -> Self {
        Self {
            track_id: None,
            track_position_secs: 0.0,
            playback_mode: PlaybackMode::default(),
            sort_order: SortOrder::default(),
        }
    }
}
