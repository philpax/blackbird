/// Configuration types shared between the blackbird clients.
use std::time::Duration;

use blackbird_core::{PlaybackMode, SortOrder, blackbird_state::TrackId};
use serde::{Deserialize, Serialize};

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

/// Trait for enums that can be displayed in a settings dropdown.
/// Implemented for config enums that have a fixed set of variants with
/// human-readable labels.
pub trait EnumerableEnum: Copy + PartialEq + 'static {
    /// All variants for UI display/cycling.
    const ALL: &'static [Self];
    /// Returns a human-readable label for display in UI.
    fn as_str(&self) -> &'static str;
}

impl EnumerableEnum for AlbumArtStyle {
    const ALL: &'static [AlbumArtStyle] = AlbumArtStyle::ALL;
    fn as_str(&self) -> &'static str {
        AlbumArtStyle::as_str(self)
    }
}

/// A component that can be shown in the current-track sidebar.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SidebarComponent {
    /// The lyrics for the currently playing track.
    Lyrics,
    /// Songs similar to the currently playing track, from the server's
    /// OpenSubsonic surface.
    SimilarSongs,
}

impl SidebarComponent {
    /// All sidebar components in the default display order.
    pub const ALL: &[SidebarComponent] =
        &[SidebarComponent::Lyrics, SidebarComponent::SimilarSongs];

    /// Returns a human-readable label for display in UI.
    pub fn as_str(&self) -> &'static str {
        match self {
            SidebarComponent::Lyrics => "lyrics",
            SidebarComponent::SimilarSongs => "similar songs",
        }
    }
}

impl EnumerableEnum for SidebarComponent {
    const ALL: &'static [SidebarComponent] = SidebarComponent::ALL;
    fn as_str(&self) -> &'static str {
        SidebarComponent::as_str(self)
    }
}

/// Which side the current-track sidebar sits on. The sidebar's *existence* is
/// controlled by `SidebarSettings::enabled`; this only records its side.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SidebarPosition {
    /// Sidebar on the left of the library.
    Left,
    /// Sidebar on the right of the library.
    #[default]
    Right,
}

impl SidebarPosition {
    /// All positions in cycle order.
    pub const ALL: &[SidebarPosition] = &[SidebarPosition::Left, SidebarPosition::Right];

    /// Returns a human-readable label for display in UI.
    pub fn as_str(&self) -> &'static str {
        match self {
            SidebarPosition::Left => "left",
            SidebarPosition::Right => "right",
        }
    }
}

impl EnumerableEnum for SidebarPosition {
    const ALL: &'static [SidebarPosition] = SidebarPosition::ALL;
    fn as_str(&self) -> &'static str {
        SidebarPosition::as_str(self)
    }
}

/// Settings for the current-track sidebar.
///
/// The sidebar's existence is controlled by [`Self::enabled`]; [`SidebarPosition`]
/// only records which side it sits on.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SidebarSettings {
    /// Whether the sidebar is visible.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Which side the sidebar sits on.
    #[serde(default)]
    pub position: SidebarPosition,
    /// The ordered list of components shown in the sidebar, top to bottom.
    /// An empty list renders a placeholder. Defaults to `[Lyrics, SimilarSongs]`.
    #[serde(default = "default_sidebar_components")]
    pub components: Vec<SidebarComponent>,
    /// The number of similar songs to request from the server (clamped to
    /// 1–100 by the settings UI).
    #[serde(default = "default_similar_songs_count")]
    pub similar_songs_count: usize,
    /// The proportional height of each component, in the same order as
    /// `components`. Values are fractions of the sidebar height summing to 1.
    /// Defaults to equal shares (100/N for N components). When the component
    /// list changes, `heights` is rebalanced to equal shares.
    #[serde(default = "default_sidebar_heights")]
    pub heights: Vec<f32>,
}

/// The effective sidebar position (the explicit position; no derivation).
pub fn effective_sidebar_position(position: SidebarPosition) -> SidebarPosition {
    position
}

impl Default for SidebarSettings {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            position: SidebarPosition::default(),
            components: default_sidebar_components(),
            similar_songs_count: default_similar_songs_count(),
            heights: default_sidebar_heights(),
        }
    }
}

impl SidebarSettings {
    /// Rebalances `heights` to equal shares for the current component list.
    /// Call whenever `components` changes (config load, settings edit).
    pub fn rebalance_heights(&mut self) {
        let count = self.components.len();
        if count == 0 {
            self.heights.clear();
            return;
        }
        let share = 1.0 / count as f32;
        self.heights = vec![share; count];
        // Clamp accumulated rounding error so the fractions sum to exactly 1.
        let last = self.heights.len() - 1;
        let sum: f32 = self.heights.iter().sum();
        self.heights[last] += 1.0 - sum;
    }
}

fn default_sidebar_components() -> Vec<SidebarComponent> {
    vec![SidebarComponent::Lyrics, SidebarComponent::SimilarSongs]
}

fn default_similar_songs_count() -> usize {
    20
}

fn default_sidebar_heights() -> Vec<f32> {
    vec![0.5, 0.5]
}

/// Layout configuration for the library and player UI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Layout {
    /// How album art is displayed in the library view.
    #[serde(default)]
    pub album_art_style: AlbumArtStyle,
    /// Number of blank rows between albums in the library view.
    #[serde(default = "default_album_spacing")]
    pub album_spacing: usize,
    /// Scroll multiplier for mouse wheel scrolling.
    #[serde(default = "default_scroll_multiplier")]
    pub scroll_multiplier: f32,
    /// Settings for the current-track sidebar.
    #[serde(default)]
    pub sidebar: SidebarSettings,
}
impl Default for Layout {
    fn default() -> Self {
        Self {
            album_art_style: AlbumArtStyle::default(),
            album_spacing: default_album_spacing(),
            scroll_multiplier: default_scroll_multiplier(),
            sidebar: SidebarSettings::default(),
        }
    }
}

fn default_scroll_multiplier() -> f32 {
    50.0
}

fn default_album_spacing() -> usize {
    1
}

/// Shared configuration fields used by the blackbird clients.
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
    /// Playback-related settings shared across clients.
    #[serde(default)]
    pub playback: Playback,
}

fn default_true() -> bool {
    true
}

/// Playback-related settings shared across clients.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Playback {
    /// Whether ReplayGain volume adjustments should be applied during playback.
    #[serde(default = "default_true")]
    pub apply_replaygain: bool,
    /// Preamp added on top of the ReplayGain-computed gain, in dB. Useful for
    /// compensating for ReplayGain's ~−18 LUFS reference level, which can feel
    /// quiet next to unprocessed modern masters. Clipping protection still
    /// applies, so tracks with high peaks may be attenuated below this value.
    #[serde(default)]
    pub replaygain_preamp_db: f32,
}
impl Default for Playback {
    fn default() -> Self {
        Self {
            apply_replaygain: true,
            replaygain_preamp_db: 0.0,
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidebar_settings_defaults() {
        let settings = SidebarSettings::default();
        assert!(settings.enabled);
        assert_eq!(
            settings.components,
            vec![SidebarComponent::Lyrics, SidebarComponent::SimilarSongs]
        );
        assert_eq!(settings.similar_songs_count, 20);
    }

    #[test]
    fn sidebar_component_serializes_snake_case() {
        // Bare enums can't be serialized standalone by toml; verify the
        // snake_case names through the full Layout roundtrip.
        let layout = Layout {
            sidebar: SidebarSettings {
                enabled: true,
                components: vec![SidebarComponent::SimilarSongs, SidebarComponent::Lyrics],
                similar_songs_count: 20,
                heights: vec![0.5, 0.5],
                ..Default::default()
            },
            ..Default::default()
        };
        let toml_str = toml::to_string(&layout).unwrap();
        assert!(
            toml_str.contains("components = [\"similar_songs\", \"lyrics\"]"),
            "expected snake_case component names, got:\n{toml_str}"
        );
    }

    #[test]
    fn sidebar_config_roundtrip() {
        let layout = Layout {
            sidebar: SidebarSettings {
                enabled: false,
                components: vec![SidebarComponent::SimilarSongs],
                similar_songs_count: 7,
                heights: vec![1.0],
                ..Default::default()
            },
            ..Default::default()
        };
        let toml_str = toml::to_string(&layout).unwrap();
        let parsed: Layout = toml::from_str(&toml_str).unwrap();
        assert_eq!(layout.sidebar, parsed.sidebar);
        assert!(toml_str.contains("enabled = false"));
        assert!(toml_str.contains("similar_songs_count = 7"));
    }

    #[test]
    fn rebalance_heights_distributes_equal_shares() {
        let mut settings = SidebarSettings {
            components: vec![
                SidebarComponent::Lyrics,
                SidebarComponent::SimilarSongs,
                SidebarComponent::SimilarSongs,
            ],
            ..Default::default()
        };
        settings.rebalance_heights();
        assert_eq!(settings.heights.len(), 3);
        let sum: f32 = settings.heights.iter().sum();
        assert!(
            (sum - 1.0).abs() < 1e-6,
            "heights should sum to 1, got {sum}"
        );
        for h in &settings.heights {
            assert!(
                (h - 1.0 / 3.0).abs() < 1e-3,
                "expected equal shares, got {h}"
            );
        }
    }

    #[test]
    fn old_config_without_heights_parses() {
        // Configs written before the `heights` field existed parse with the
        // default equal shares.
        let toml_str = r#"
[sidebar]
enabled = true
components = ["lyrics", "similar_songs"]
similar_songs_count = 20
"#;
        let settings: SidebarSettings = toml::from_str(toml_str).unwrap();
        assert_eq!(settings.heights, vec![0.5, 0.5]);
    }

    #[test]
    fn effective_sidebar_position_is_authoritative() {
        assert_eq!(
            effective_sidebar_position(SidebarPosition::Left),
            SidebarPosition::Left
        );
        assert_eq!(
            effective_sidebar_position(SidebarPosition::Right),
            SidebarPosition::Right
        );
    }

    #[test]
    fn sidebar_position_defaults_to_right() {
        assert_eq!(SidebarPosition::default(), SidebarPosition::Right);
        let settings = SidebarSettings::default();
        assert_eq!(settings.position, SidebarPosition::Right);
    }

    #[test]
    fn old_config_parses_with_sidebar_defaults() {
        // Old configs with only `lyrics_display` (no `sidebar` key) parse with
        // the sidebar enabled and lyrics + similar songs in that order.
        let toml_str = r#"
"#;
        let layout: Layout = toml::from_str(toml_str).unwrap();
        assert!(
            layout.sidebar.enabled,
            "old configs should default to the sidebar being enabled"
        );
        assert_eq!(
            layout.sidebar.components,
            vec![SidebarComponent::Lyrics, SidebarComponent::SimilarSongs]
        );
    }
}
