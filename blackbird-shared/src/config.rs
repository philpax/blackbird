//! Configuration types and loaders shared between blackbird clients and tools.
use std::path::PathBuf;

use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// Filename used for every blackbird config inside the platform config dir.
pub const CONFIG_FILENAME: &str = "config.toml";

/// Trait implemented by every config-typed view of `~/.config/blackbird/config.toml`
/// (or the platform equivalent).
///
/// Default methods provide a consistent load/save/path implementation across
/// every client and tool. Each consumer can define its own struct exposing
/// only the fields it cares about — unknown sections written by other clients
/// are ignored on load.
pub trait ConfigFile: Default + Serialize + DeserializeOwned {
    /// Full path to the config file inside the user's config dir.
    fn path() -> PathBuf {
        crate::paths::config_dir().join(CONFIG_FILENAME)
    }

    /// Load from disk, returning [`Self::default()`] if the file doesn't exist.
    ///
    /// Panics on parse errors or unexpected I/O errors so that misconfiguration
    /// is surfaced loudly rather than silently producing a default config.
    fn load() -> Self {
        let path = Self::path();
        match std::fs::read_to_string(&path) {
            Ok(contents) => match toml::from_str(&contents) {
                Ok(config) => config,
                Err(e) => panic!("failed to parse {}: {e}", path.display()),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::info!(
                    "no config file found at {}, using default config",
                    path.display()
                );
                Self::default()
            }
            Err(e) => panic!("failed to read {}: {e}", path.display()),
        }
    }

    /// Serialize to TOML and write to [`Self::path()`], creating the parent
    /// directory if needed.
    fn save(&self) {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, toml::to_string(self).unwrap()).unwrap();
        tracing::info!("saved config to {}", path.display());
    }
}

/// Server connection settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Server {
    pub base_url: String,
    pub username: String,
    pub password: String,
    pub transcode: bool,
}
impl Default for Server {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:4533".to_string(),
            username: "YOUR_USERNAME".to_string(),
            password: "YOUR_PASSWORD".to_string(),
            transcode: false,
        }
    }
}
