use serde::{Deserialize, Serialize};

use crate::ui;

#[derive(Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct Config {
    #[serde(default)]
    pub general: General,
    #[serde(default)]
    pub server: blackbird_shared::config::Server,
    #[serde(default)]
    pub style: ui::Style,
    #[serde(default)]
    pub last_playback: blackbird_shared::config::LastPlayback,
    #[serde(default)]
    pub keybindings: Keybindings,
}
impl Config {
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
#[serde(default)]
pub struct General {
    pub repaint_secs: f32,
    pub window_position_x: i32,
    pub window_position_y: i32,
    pub window_width: u32,
    pub window_height: u32,
    pub volume: f32,
    pub incremental_search_timeout_ms: u64,
}
impl Default for General {
    fn default() -> Self {
        Self {
            repaint_secs: 1.0,
            window_position_x: 0,
            window_position_y: 0,
            window_width: 640,
            window_height: 1280,
            volume: 1.0,
            incremental_search_timeout_ms: 5000,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Keybindings {
    /// Global hotkey to toggle search window (works even when app is not focused)
    /// Format: "Ctrl+Alt+Shift+F" where modifiers are Ctrl, Alt, Shift, Super, Win, or Cmd
    /// "Cmd" is platform-aware: maps to Command on macOS, Ctrl on Linux/Windows
    /// Key can be a letter (A-Z) or function key (F1-F12)
    pub global_search: String,

    /// Global hotkey to toggle mini-library window (works even when app is not focused)
    /// Shows a smaller library view centered on the currently playing track
    pub global_mini_library: String,

    /// Local keybindings (work only when app window is focused)
    /// Format: "Cmd+F" where Cmd is Ctrl on Linux/Windows and Command on macOS
    pub local_search: String,
    pub local_lyrics: String,

    /// Mouse button bindings for track navigation
    /// Valid values: "Extra1" (button 4), "Extra2" (button 5), or "None" to disable
    pub mouse_previous_track: String,
    pub mouse_next_track: String,
}

impl Default for Keybindings {
    fn default() -> Self {
        Self {
            global_search: "Cmd+Alt+Shift+F".to_string(),
            global_mini_library: "Ctrl+Alt+Shift+G".to_string(),
            local_search: "Cmd+F".to_string(),
            local_lyrics: "Cmd+L".to_string(),
            mouse_previous_track: "Extra1".to_string(),
            mouse_next_track: "Extra2".to_string(),
        }
    }
}

impl Keybindings {
    /// Parse a global hotkey string into (Code, Modifiers) for global-hotkey crate
    pub fn parse_global_hotkey(
        &self,
        binding: &str,
    ) -> Option<(
        global_hotkey::hotkey::Code,
        global_hotkey::hotkey::Modifiers,
    )> {
        use global_hotkey::hotkey::{Code, Modifiers};

        let parts: Vec<&str> = binding.split('+').collect();
        if parts.is_empty() {
            return None;
        }

        let mut modifiers = Modifiers::empty();
        let key_str = parts.last()?;

        for part in parts.iter().take(parts.len() - 1) {
            match part.trim() {
                "Ctrl" => modifiers |= Modifiers::CONTROL,
                "Alt" => modifiers |= Modifiers::ALT,
                "Shift" => modifiers |= Modifiers::SHIFT,
                "Super" | "Win" => modifiers |= Modifiers::SUPER,
                // "Cmd" is platform-aware: Super on macOS, Control on Linux/Windows
                "Cmd" => {
                    #[cfg(target_os = "macos")]
                    {
                        modifiers |= Modifiers::SUPER;
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        modifiers |= Modifiers::CONTROL;
                    }
                }
                _ => return None,
            }
        }

        let code = match key_str.trim() {
            "A" => Code::KeyA,
            "B" => Code::KeyB,
            "C" => Code::KeyC,
            "D" => Code::KeyD,
            "E" => Code::KeyE,
            "F" => Code::KeyF,
            "G" => Code::KeyG,
            "H" => Code::KeyH,
            "I" => Code::KeyI,
            "J" => Code::KeyJ,
            "K" => Code::KeyK,
            "L" => Code::KeyL,
            "M" => Code::KeyM,
            "N" => Code::KeyN,
            "O" => Code::KeyO,
            "P" => Code::KeyP,
            "Q" => Code::KeyQ,
            "R" => Code::KeyR,
            "S" => Code::KeyS,
            "T" => Code::KeyT,
            "U" => Code::KeyU,
            "V" => Code::KeyV,
            "W" => Code::KeyW,
            "X" => Code::KeyX,
            "Y" => Code::KeyY,
            "Z" => Code::KeyZ,
            "F1" => Code::F1,
            "F2" => Code::F2,
            "F3" => Code::F3,
            "F4" => Code::F4,
            "F5" => Code::F5,
            "F6" => Code::F6,
            "F7" => Code::F7,
            "F8" => Code::F8,
            "F9" => Code::F9,
            "F10" => Code::F10,
            "F11" => Code::F11,
            "F12" => Code::F12,
            _ => return None,
        };

        Some((code, modifiers))
    }

    /// Parse a local keybinding string into egui::Key
    pub fn parse_local_key(&self, binding: &str) -> Option<egui::Key> {
        let parts: Vec<&str> = binding.split('+').collect();
        let key_str = parts.last()?.trim();

        match key_str {
            "A" => Some(egui::Key::A),
            "B" => Some(egui::Key::B),
            "C" => Some(egui::Key::C),
            "D" => Some(egui::Key::D),
            "E" => Some(egui::Key::E),
            "F" => Some(egui::Key::F),
            "G" => Some(egui::Key::G),
            "H" => Some(egui::Key::H),
            "I" => Some(egui::Key::I),
            "J" => Some(egui::Key::J),
            "K" => Some(egui::Key::K),
            "L" => Some(egui::Key::L),
            "M" => Some(egui::Key::M),
            "N" => Some(egui::Key::N),
            "O" => Some(egui::Key::O),
            "P" => Some(egui::Key::P),
            "Q" => Some(egui::Key::Q),
            "R" => Some(egui::Key::R),
            "S" => Some(egui::Key::S),
            "T" => Some(egui::Key::T),
            "U" => Some(egui::Key::U),
            "V" => Some(egui::Key::V),
            "W" => Some(egui::Key::W),
            "X" => Some(egui::Key::X),
            "Y" => Some(egui::Key::Y),
            "Z" => Some(egui::Key::Z),
            _ => None,
        }
    }

    /// Check if a local keybinding requires the command modifier (Cmd on Mac, Ctrl elsewhere)
    pub fn requires_command(&self, binding: &str) -> bool {
        binding.contains("Cmd")
    }

    /// Parse a mouse button string into egui::PointerButton
    pub fn parse_mouse_button(&self, binding: &str) -> Option<egui::PointerButton> {
        match binding.trim() {
            "Extra1" => Some(egui::PointerButton::Extra1),
            "Extra2" => Some(egui::PointerButton::Extra2),
            "None" => None,
            _ => None,
        }
    }
}
