use serde::{Deserialize, Serialize};

/// Config is read from the same `config.toml` as the egui client.
/// Unknown fields (style, keybindings, etc.) are preserved on save.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct Config {
    #[serde(default)]
    pub general: General,
    #[serde(default)]
    pub style: blackbird_client_shared::style::Style,
    #[serde(flatten)]
    pub shared: blackbird_client_shared::config::Config,
}
impl Config {
    pub const FILENAME: &str = "config.toml";

    pub fn load() -> Self {
        blackbird_client_shared::config::load_config(Self::FILENAME)
    }

    /// Saves config back to disk, preserving any fields the TUI doesn't know about
    /// (e.g. egui-specific style, keybindings).
    pub fn save(&self) {
        // Read the existing file and merge our fields into it.
        let mut doc: toml::Value = match std::fs::read_to_string(Self::FILENAME) {
            Ok(contents) => {
                toml::from_str(&contents).unwrap_or(toml::Value::Table(Default::default()))
            }
            Err(_) => toml::Value::Table(Default::default()),
        };

        let table = doc.as_table_mut().unwrap();

        // Merge `general` - only update fields we own.
        {
            let general = table
                .entry("general")
                .or_insert_with(|| toml::Value::Table(Default::default()));
            if let Some(g) = general.as_table_mut() {
                g.insert(
                    "volume".into(),
                    toml::Value::Float(self.general.volume as f64),
                );
            }
        }

        // Merge `server`.
        {
            let server = table
                .entry("server")
                .or_insert_with(|| toml::Value::Table(Default::default()));
            if let Some(s) = server.as_table_mut() {
                s.insert(
                    "base_url".into(),
                    toml::Value::String(self.shared.server.base_url.clone()),
                );
                s.insert(
                    "username".into(),
                    toml::Value::String(self.shared.server.username.clone()),
                );
                s.insert(
                    "password".into(),
                    toml::Value::String(self.shared.server.password.clone()),
                );
                s.insert(
                    "transcode".into(),
                    toml::Value::Boolean(self.shared.server.transcode),
                );
            }
        }

        // Merge `last_playback`.
        {
            let lp = table
                .entry("last_playback")
                .or_insert_with(|| toml::Value::Table(Default::default()));
            if let Some(l) = lp.as_table_mut() {
                match &self.shared.last_playback.track_id {
                    Some(id) => {
                        l.insert("track_id".into(), toml::Value::String(id.0.to_string()));
                    }
                    None => {
                        l.remove("track_id");
                    }
                }
                l.insert(
                    "track_position_secs".into(),
                    toml::Value::Float(self.shared.last_playback.track_position_secs),
                );
                l.insert(
                    "playback_mode".into(),
                    toml::Value::String(self.shared.last_playback.playback_mode.to_string()),
                );
            }
        }

        std::fs::write(Self::FILENAME, toml::to_string(&doc).unwrap()).unwrap();
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
