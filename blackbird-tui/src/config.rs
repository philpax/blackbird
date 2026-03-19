use serde::{Deserialize, Serialize};

/// Config is read from the same `config.toml` as the egui client.
/// Unknown fields (keybindings, egui-specific style fields, etc.) are preserved via
/// `#[serde(flatten)]` catch-all fields, allowing safe roundtripping between clients.
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
    /// Catch-all for unknown top-level sections (e.g. keybindings from GUI).
    #[serde(flatten)]
    pub extra: toml::Table,
}

/// TUI layout configuration, extending the shared [`blackbird_client_shared::config::Layout`]
/// with TUI-specific fields. Unknown fields from other clients are preserved via the catch-all.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Layout {
    /// Use the terminal's native background color instead of the configured one.
    #[serde(default)]
    pub use_terminal_background: bool,
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
            base: blackbird_client_shared::config::Layout::default(),
            extra: toml::Table::new(),
        }
    }
}

impl Config {
    pub const FILENAME: &str = "config.toml";

    pub fn load() -> Self {
        blackbird_client_shared::config::load_config(Self::FILENAME)
    }

    pub fn save(&self) {
        let path = blackbird_client_shared::config::config_path(Self::FILENAME);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, toml::to_string(self).unwrap()).unwrap();
        tracing::info!("saved config to {}", path.display());
    }
}

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
show_inline_lyrics = true
use_terminal_background = false
some_gui_only_field = 42
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.layout.base.show_inline_lyrics);
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
}
