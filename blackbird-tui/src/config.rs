use serde::{Deserialize, Serialize};

/// Config is read from the same `config.toml` as the egui client.
/// Unknown fields (keybindings, egui-specific style fields, etc.) are preserved via
/// `#[serde(flatten)]` catch-all fields, allowing safe roundtripping between clients.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct Config {
    #[serde(default)]
    pub general: General,
    #[serde(default)]
    pub style: blackbird_client_shared::style::Style,
    #[serde(flatten)]
    pub shared: blackbird_client_shared::config::Config,
    /// Catch-all for unknown top-level sections (e.g. keybindings from GUI).
    #[serde(flatten)]
    pub extra: toml::Table,
}
impl Config {
    pub const FILENAME: &str = "config.toml";

    pub fn load() -> Self {
        blackbird_client_shared::config::load_config(Self::FILENAME)
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
