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

impl EnumerableEnum for LyricsDisplay {
    const ALL: &'static [LyricsDisplay] = LyricsDisplay::ALL;
    fn as_str(&self) -> &'static str {
        LyricsDisplay::as_str(self)
    }
}

/// Controls how lyrics are displayed in the client UI.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LyricsDisplay {
    /// Lyrics are not shown.
    Off,
    /// Lyrics are shown inline at the bottom of the content area.
    #[default]
    Inline,
    /// Lyrics are shown in a sidebar on the left side of the content area.
    Left,
    /// Lyrics are shown in a sidebar on the right side of the content area.
    Right,
}

impl LyricsDisplay {
    /// All variants for UI display/cycling.
    pub const ALL: &[LyricsDisplay] = &[
        LyricsDisplay::Off,
        LyricsDisplay::Inline,
        LyricsDisplay::Left,
        LyricsDisplay::Right,
    ];

    /// Returns a human-readable label for display in UI.
    pub fn as_str(&self) -> &'static str {
        match self {
            LyricsDisplay::Off => "off",
            LyricsDisplay::Inline => "inline",
            LyricsDisplay::Left => "left",
            LyricsDisplay::Right => "right",
        }
    }

    /// Returns `true` if lyrics should be shown in a sidebar.
    pub fn is_sidebar(self) -> bool {
        matches!(self, LyricsDisplay::Left | LyricsDisplay::Right)
    }
}

/// Layout configuration for the library and player UI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Layout {
    /// How lyrics are displayed in the UI.
    #[serde(default)]
    pub lyrics_display: LyricsDisplay,
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
            lyrics_display: LyricsDisplay::default(),
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
    fn lyrics_display_roundtrip() {
        for variant in LyricsDisplay::ALL {
            let layout = Layout {
                lyrics_display: *variant,
                ..Default::default()
            };
            let toml_str = toml::to_string(&layout).unwrap();
            let parsed: Layout = toml::from_str(&toml_str).unwrap();
            assert_eq!(
                layout.lyrics_display, parsed.lyrics_display,
                "roundtrip failed for {variant:?}"
            );
        }
    }

    #[test]
    fn lyrics_display_default_is_inline() {
        assert_eq!(LyricsDisplay::default(), LyricsDisplay::Inline);
    }

    #[test]
    fn layout_default_has_inline_lyrics() {
        let layout = Layout::default();
        assert_eq!(layout.lyrics_display, LyricsDisplay::Inline);
    }

    #[test]
    fn layout_with_old_show_inline_lyrics_field_parses() {
        // Old configs with `show_inline_lyrics` should still parse. The old
        // field is silently ignored (caught by serde's default on the new
        // `lyrics_display` field), and `lyrics_display` defaults to `Inline`.
        let toml_str = r#"
show_inline_lyrics = false
"#;
        let layout: Layout = toml::from_str(toml_str).unwrap();
        assert_eq!(layout.lyrics_display, LyricsDisplay::Inline);
    }

    #[test]
    fn layout_with_lyrics_display_right_parses() {
        let toml_str = r#"
lyrics_display = "right"
"#;
        let layout: Layout = toml::from_str(toml_str).unwrap();
        assert_eq!(layout.lyrics_display, LyricsDisplay::Right);
    }

    #[test]
    fn layout_with_lyrics_display_off_parses() {
        let toml_str = r#"
lyrics_display = "off"
"#;
        let layout: Layout = toml::from_str(toml_str).unwrap();
        assert_eq!(layout.lyrics_display, LyricsDisplay::Off);
    }

    #[test]
    fn layout_with_lyrics_display_left_parses() {
        let toml_str = r#"
lyrics_display = "left"
"#;
        let layout: Layout = toml::from_str(toml_str).unwrap();
        assert_eq!(layout.lyrics_display, LyricsDisplay::Left);
    }

    #[test]
    fn lyrics_display_is_sidebar() {
        assert!(!LyricsDisplay::Off.is_sidebar());
        assert!(!LyricsDisplay::Inline.is_sidebar());
        assert!(LyricsDisplay::Left.is_sidebar());
        assert!(LyricsDisplay::Right.is_sidebar());
    }
}
