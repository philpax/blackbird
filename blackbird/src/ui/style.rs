use blackbird_client_shared::style as shared_style;
use egui::{Color32, ecolor::Hsva};

/// Re-export the shared Style type.
pub use shared_style::Style;

/// Convert shared Rgb to egui Color32.
fn rgb_to_color32(rgb: shared_style::Rgb) -> Color32 {
    Color32::from_rgb(rgb.r, rgb.g, rgb.b)
}

/// Extension trait for Style to get egui Color32 values.
pub trait StyleExt {
    fn background_color32(&self) -> Color32;
    fn text_color32(&self) -> Color32;
    fn album_color32(&self) -> Color32;
    fn album_length_color32(&self) -> Color32;
    fn album_year_color32(&self) -> Color32;
    fn track_number_color32(&self) -> Color32;
    fn track_length_color32(&self) -> Color32;
    fn track_name_color32(&self) -> Color32;
    fn track_name_hovered_color32(&self) -> Color32;
    fn track_name_playing_color32(&self) -> Color32;
    fn track_duration_color32(&self) -> Color32;
}

impl StyleExt for Style {
    fn background_color32(&self) -> Color32 {
        rgb_to_color32(self.background())
    }
    fn text_color32(&self) -> Color32 {
        rgb_to_color32(self.text())
    }
    fn album_color32(&self) -> Color32 {
        rgb_to_color32(self.album())
    }
    fn album_length_color32(&self) -> Color32 {
        rgb_to_color32(self.album_length())
    }
    fn album_year_color32(&self) -> Color32 {
        rgb_to_color32(self.album_year())
    }
    fn track_number_color32(&self) -> Color32 {
        rgb_to_color32(self.track_number())
    }
    fn track_length_color32(&self) -> Color32 {
        rgb_to_color32(self.track_length())
    }
    fn track_name_color32(&self) -> Color32 {
        rgb_to_color32(self.track_name())
    }
    fn track_name_hovered_color32(&self) -> Color32 {
        rgb_to_color32(self.track_name_hovered())
    }
    fn track_name_playing_color32(&self) -> Color32 {
        rgb_to_color32(self.track_name_playing())
    }
    fn track_duration_color32(&self) -> Color32 {
        rgb_to_color32(self.track_duration())
    }
}

/// Hashes a string and produces a pleasing colour from that hash.
pub fn string_to_colour(s: &str) -> Hsva {
    let hsv = shared_style::string_to_hsv(s);
    Hsva::new(hsv[0], hsv[1], hsv[2], 1.0)
}
