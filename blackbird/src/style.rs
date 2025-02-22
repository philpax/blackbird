use egui::{ecolor::Hsva, Color32};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct Style {
    /// Background colour for the main window.
    pub background_hsv: [f32; 3],

    /// Default text colour.
    pub text_hsv: [f32; 3],

    /// Colour for albums.
    pub album_hsv: [f32; 3],

    /// Colour for album years.
    pub album_year_hsv: [f32; 3],

    /// Colour for track numbers.
    pub track_number_hsv: [f32; 3],

    /// Colour for track lengths.
    pub track_length_hsv: [f32; 3],
}
impl Default for Style {
    fn default() -> Self {
        Self {
            background_hsv: [0.65, 0.40, 0.01],
            text_hsv: [0.0, 0.0, 1.0],
            album_hsv: [0.58, 0.90, 0.60],
            album_year_hsv: [0.0, 0.0, 0.40],
            track_number_hsv: [0.60, 0.5, 0.90],
            track_length_hsv: [0.60, 0.90, 0.70],
        }
    }
}
impl Style {
    pub fn background(&self) -> Color32 {
        hsv_to_color32(self.background_hsv)
    }
    pub fn text(&self) -> Color32 {
        hsv_to_color32(self.text_hsv)
    }
    pub fn album(&self) -> Color32 {
        hsv_to_color32(self.album_hsv)
    }
    pub fn album_year(&self) -> Color32 {
        hsv_to_color32(self.album_year_hsv)
    }
    pub fn track_number(&self) -> Color32 {
        hsv_to_color32(self.track_number_hsv)
    }
    pub fn track_length(&self) -> Color32 {
        hsv_to_color32(self.track_length_hsv)
    }
}

fn hsv_to_color32(hsv: [f32; 3]) -> Color32 {
    Hsva {
        h: hsv[0],
        s: hsv[1],
        v: hsv[2],
        a: 1.0,
    }
    .into()
}

/// Hashes a string and produces a pleasing colour from that hash.
pub fn string_to_colour(s: &str) -> Hsva {
    use std::hash::Hash;
    use std::hash::Hasher;

    const DISTINCT_COLOURS: u64 = 36_000;

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut hasher);
    let hash = hasher.finish();
    let hue = (hash % DISTINCT_COLOURS) as f32 / DISTINCT_COLOURS as f32;

    Hsva::new(hue, 0.75, 0.75, 1.0)
}
