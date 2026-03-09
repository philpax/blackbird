/// Configuration types shared between the egui and TUI clients.
use std::path::PathBuf;
use std::time::Duration;

use blackbird_core::{PlaybackMode, SortOrder, blackbird_state::TrackId};
use serde::{Deserialize, Serialize};

/// Returns the full path to a config file inside the platform-specific config directory.
pub fn config_path(filename: &str) -> PathBuf {
    crate::paths::config_dir().join(filename)
}

/// Load a TOML config file from the platform-specific config directory,
/// returning `T::default()` if the file doesn't exist.
/// Panics on parse errors or unexpected I/O errors.
pub fn load_config<T: Default + serde::de::DeserializeOwned>(filename: &str) -> T {
    let path = config_path(filename);
    match std::fs::read_to_string(&path) {
        Ok(contents) => match toml::from_str(&contents) {
            Ok(config) => config,
            Err(e) => panic!("Failed to parse {}: {e}", path.display()),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::info!(
                "no config file found at {}, creating default config",
                path.display()
            );
            T::default()
        }
        Err(e) => panic!("Failed to read {}: {e}", path.display()),
    }
}

/// Controls how album art is displayed in the library view.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AlbumArtStyle {
    /// Small thumbnail to the left of the artist/album header.
    #[default]
    LeftOfAlbum,
    /// Large image below the header, to the left of the track list.
    BelowAlbum,
}

impl AlbumArtStyle {
    /// All variants for UI display/cycling.
    pub const ALL: &[AlbumArtStyle] = &[AlbumArtStyle::LeftOfAlbum, AlbumArtStyle::BelowAlbum];

    /// Returns a human-readable label for display in UI.
    pub fn as_str(&self) -> &'static str {
        match self {
            AlbumArtStyle::LeftOfAlbum => "left of album",
            AlbumArtStyle::BelowAlbum => "below album",
        }
    }
}

/// Layout configuration for the library and player UI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Layout {
    /// Whether to show the current synced lyric line inline in the player UI.
    #[serde(default = "default_true")]
    pub show_inline_lyrics: bool,
    /// How album art is displayed in the library view.
    #[serde(default)]
    pub album_art_style: AlbumArtStyle,
    /// Number of blank rows between albums in the library view.
    #[serde(default = "default_album_spacing")]
    pub album_spacing: usize,
    /// Scroll multiplier for mouse wheel scrolling.
    #[serde(default = "default_scroll_multiplier")]
    pub scroll_multiplier: f32,
}
impl Default for Layout {
    fn default() -> Self {
        Self {
            show_inline_lyrics: true,
            album_art_style: AlbumArtStyle::default(),
            album_spacing: default_album_spacing(),
            scroll_multiplier: default_scroll_multiplier(),
        }
    }
}

fn default_scroll_multiplier() -> f32 {
    50.0
}

fn default_album_spacing() -> usize {
    1
}

/// Shared configuration fields used by both the egui and TUI clients.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    /// Server connection settings.
    #[serde(default)]
    pub server: blackbird_shared::config::Server,
    /// Last playback state, persisted across sessions.
    #[serde(default)]
    pub last_playback: LastPlayback,
    /// Layout configuration for the library and player UI.
    #[serde(default)]
    pub layout: Layout,
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
