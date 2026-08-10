use serde::{Deserialize, Serialize};

/// Config is read from `config.toml`.
/// Unknown fields (keybindings from the retired GUI, GUI style fields, etc.) are
/// preserved via `#[serde(flatten)]` catch-all fields, allowing safe
/// roundtripping between clients.
///
/// Fields from the shared config (`server`, `last_playback`, `layout`) are declared
/// explicitly here rather than via `#[serde(flatten)]` so that `layout` can be replaced
/// with the TUI-specific [`Layout`] wrapper.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct Config {
    #[serde(default)]
    pub general: General,
    #[serde(default)]
    pub style: blackbird_client_shared::style::Style,
    /// Layout settings, extending the shared layout with TUI-specific fields.
    #[serde(default)]
    pub layout: Layout,
    /// Server connection settings.
    #[serde(default)]
    pub server: blackbird_shared::config::Server,
    /// Last playback state, persisted across sessions.
    #[serde(default)]
    pub last_playback: blackbird_client_shared::config::LastPlayback,
    /// Playback-related settings shared across clients.
    #[serde(default)]
    pub playback: blackbird_client_shared::config::Playback,
    /// Catch-all for unknown top-level sections (e.g. keybindings from GUI).
    #[serde(flatten)]
    pub extra: toml::Table,
}

/// Controls how album art is rendered in the TUI.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AlbumArtProtocol {
    /// Use a graphics protocol (Kitty/iTerm2/Sixel) if detected, otherwise
    /// fall back to the existing half-block rendering.
    #[default]
    Auto,
    /// Always use ratatui-image, which uses a graphics protocol if detected
    /// and otherwise renders full-resolution half-blocks (higher fidelity
    /// than the existing quantized 4×4 / 16-row grids).
    Image,
    /// Always use the existing hand-rolled half-block rendering.
    Halfblock,
}

/// TUI layout configuration, extending the shared [`blackbird_client_shared::config::Layout`]
/// with TUI-specific fields. Unknown fields from other clients are preserved via the catch-all.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Layout {
    /// Use the terminal's native background color instead of the configured one.
    #[serde(default)]
    pub use_terminal_background: bool,
    /// Controls how album art is rendered (graphics protocol vs. half-blocks).
    #[serde(default)]
    pub album_art_protocol: AlbumArtProtocol,
    /// Width of the sidebar in terminal columns, when visible. (Formerly
    /// `lyrics_sidebar_width`; the old key still parses via `alias`.)
    #[serde(default = "default_sidebar_width", alias = "lyrics_sidebar_width")]
    pub sidebar_width: u16,
    /// Width of the settings sidebar (the settings list) in columns.
    #[serde(default = "default_settings_sidebar_width")]
    pub settings_sidebar_width: u16,
    /// Whether the inline lyrics overlay shows at the bottom of the content
    /// area. Independent of the sidebar: `lyrics_display` decides sidebar
    /// position, and this flag additionally shows the overlay whenever synced
    /// lyrics are available. Kept for backwards compatibility with the retired
    /// GUI's `show_inline_lyrics` key.
    #[serde(default)]
    pub show_inline_lyrics: bool,
    /// Shared layout settings.
    #[serde(flatten)]
    pub base: blackbird_client_shared::config::Layout,
    /// Catch-all for unknown fields from other clients.
    #[serde(flatten)]
    pub extra: toml::Table,
}
impl Default for Layout {
    fn default() -> Self {
        Self {
            use_terminal_background: false,
            album_art_protocol: AlbumArtProtocol::default(),
            sidebar_width: default_sidebar_width(),
            settings_sidebar_width: default_settings_sidebar_width(),
            show_inline_lyrics: false,
            base: blackbird_client_shared::config::Layout::default(),
            extra: toml::Table::new(),
        }
    }
}

fn default_sidebar_width() -> u16 {
    30
}

fn default_settings_sidebar_width() -> u16 {
    40
}

impl blackbird_shared::config::ConfigFile for Config {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct General {
    pub volume: f32,
    pub tick_rate_ms: u64,
    /// Catch-all for unknown fields (e.g. GUI-specific window settings).
    #[serde(flatten)]
    pub extra: toml::Table,
}
impl Default for General {
    fn default() -> Self {
        Self {
            volume: 1.0,
            tick_rate_ms: 100,
            extra: toml::Table::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_roundtrip() {
        let config = Config::default();
        let toml_str = toml::to_string(&config).unwrap();
        // Should not contain duplicate [layout] sections.
        assert_eq!(
            toml_str.matches("[layout]").count(),
            1,
            "expected exactly one [layout] section, got:\n{toml_str}"
        );
        // Should roundtrip cleanly.
        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(config, parsed);
    }

    #[test]
    fn config_roundtrip_with_tui_field() {
        let mut config = Config::default();
        config.layout.use_terminal_background = true;
        let toml_str = toml::to_string(&config).unwrap();
        assert!(toml_str.contains("use_terminal_background = true"));
        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(config, parsed);
    }

    #[test]
    fn config_preserves_unknown_layout_fields() {
        let toml_str = r#"
[layout]
lyrics_display = "right"
use_terminal_background = false
some_gui_only_field = 42
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        // `lyrics_display` (from the retired GUI) is now an unknown field and
        // must be preserved in the catch-all rather than erroring.
        assert_eq!(
            config.layout.extra.get("lyrics_display"),
            Some(&toml::Value::String("right".to_string()))
        );
        assert!(!config.layout.use_terminal_background);
        // The unknown field should be preserved in the catch-all.
        assert_eq!(
            config.layout.extra.get("some_gui_only_field"),
            Some(&toml::Value::Integer(42))
        );
        // And it roundtrips.
        let re_serialized = toml::to_string(&config).unwrap();
        assert!(re_serialized.contains("some_gui_only_field = 42"));
    }

    #[test]
    fn config_roundtrip_with_album_art_protocol() {
        for protocol in [
            AlbumArtProtocol::Auto,
            AlbumArtProtocol::Image,
            AlbumArtProtocol::Halfblock,
        ] {
            let mut config = Config::default();
            config.layout.album_art_protocol = protocol;
            let toml_str = toml::to_string(&config).unwrap();
            let parsed: Config = toml::from_str(&toml_str).unwrap();
            assert_eq!(config, parsed, "roundtrip failed for {protocol:?}");
        }
    }

    #[test]
    fn config_preserves_album_art_protocol() {
        let toml_str = r#"
[layout]
album_art_protocol = "image"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.layout.album_art_protocol, AlbumArtProtocol::Image);
    }

    #[test]
    fn config_roundtrip_sidebar_width() {
        let mut config = Config::default();
        config.layout.sidebar_width = 42;
        let toml_str = toml::to_string(&config).unwrap();
        assert!(toml_str.contains("sidebar_width = 42"));
        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(config, parsed);
    }

    #[test]
    fn config_sidebar_width_defaults_to_30() {
        let toml_str = r#"
[layout]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.layout.sidebar_width, 30);
    }

    #[test]
    fn config_parses_legacy_lyrics_sidebar_width() {
        // Old configs use `lyrics_sidebar_width`; it should alias into the
        // renamed `sidebar_width` field.
        let toml_str = r#"
[layout]
lyrics_sidebar_width = 42
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.layout.sidebar_width, 42);
    }

    #[test]
    fn config_parses_legacy_show_inline_lyrics() {
        // The retired GUI's `show_inline_lyrics` flag lives in `[layout]`; it
        // should parse into the explicit field (not the catch-all) so the TUI
        // honours it.
        let toml_str = r#"
[layout]
lyrics_display = "right"
show_inline_lyrics = true
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        // `lyrics_display` is obsolete and preserved as an unknown field.
        assert_eq!(
            config.layout.extra.get("lyrics_display"),
            Some(&toml::Value::String("right".to_string()))
        );
        assert!(config.layout.show_inline_lyrics);
        // And it roundtrips through the field, not the catch-all.
        let re_serialized = toml::to_string(&config).unwrap();
        assert!(re_serialized.contains("show_inline_lyrics = true"));
    }

    #[test]
    fn config_defaults_show_inline_lyrics_off() {
        let config = Config::default();
        assert!(!config.layout.show_inline_lyrics);
    }
}
