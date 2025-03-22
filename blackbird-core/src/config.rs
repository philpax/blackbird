use serde::{Deserialize, Serialize, de::DeserializeOwned};

#[derive(Debug, Serialize, Deserialize, Default, PartialEq)]
pub struct Config<Style: Default> {
    #[serde(default)]
    pub general: General,
    #[serde(default)]
    pub style: Style,
}
impl<Style: Default + DeserializeOwned + Serialize> Config<Style> {
    pub const FILENAME: &str = "config.toml";

    pub fn load() -> Self {
        match std::fs::read_to_string(Self::FILENAME) {
            Ok(contents) => {
                // Config exists, try to parse it
                match toml::from_str(&contents) {
                    Ok(config) => config,
                    Err(e) => panic!("Failed to parse {}: {e}", Self::FILENAME),
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // No config exists, create default
                tracing::info!("no config file found, creating default config");
                Config::default()
            }
            Err(e) => {
                // Some other IO error occurred while reading
                panic!("Failed to read {}: {e}", Self::FILENAME)
            }
        }
    }

    pub fn save(&self) {
        std::fs::write(Self::FILENAME, toml::to_string(self).unwrap()).unwrap();
        tracing::info!("saved config to {}", Self::FILENAME);
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct General {
    pub base_url: String,
    pub username: String,
    pub password: String,
    pub album_art_enabled: bool,
    pub repaint_secs: f32,
}
impl Default for General {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:4533".to_string(),
            username: "YOUR_USERNAME".to_string(),
            password: "YOUR_PASSWORD".to_string(),
            album_art_enabled: true,
            repaint_secs: 1.0,
        }
    }
}
