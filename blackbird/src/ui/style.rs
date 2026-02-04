use blackbird_client_shared::style as shared_style;
use egui::{Color32, ecolor::Hsva};

/// Re-export the shared Style type.
pub use shared_style::Style;

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
fn hsv_to_egui_color32([h, s, v]: shared_style::Hsv) -> Color32 {
    Color32::from(Hsva::new(h, s, v, 1.0))
}
impl StyleExt for Style {
    fn background_color32(&self) -> Color32 {
        hsv_to_egui_color32(self.background_hsv)
    }
    fn text_color32(&self) -> Color32 {
        hsv_to_egui_color32(self.text_hsv)
    }
    fn album_color32(&self) -> Color32 {
        hsv_to_egui_color32(self.album_hsv)
    }
    fn album_length_color32(&self) -> Color32 {
        hsv_to_egui_color32(self.album_length_hsv)
    }
    fn album_year_color32(&self) -> Color32 {
        hsv_to_egui_color32(self.album_year_hsv)
    }
    fn track_number_color32(&self) -> Color32 {
        hsv_to_egui_color32(self.track_number_hsv)
    }
    fn track_length_color32(&self) -> Color32 {
        hsv_to_egui_color32(self.track_length_hsv)
    }
    fn track_name_color32(&self) -> Color32 {
        hsv_to_egui_color32(self.track_name_hsv)
    }
    fn track_name_hovered_color32(&self) -> Color32 {
        hsv_to_egui_color32(self.track_name_hovered_hsv)
    }
    fn track_name_playing_color32(&self) -> Color32 {
        hsv_to_egui_color32(self.track_name_playing_hsv)
    }
    fn track_duration_color32(&self) -> Color32 {
        hsv_to_egui_color32(self.track_duration_hsv)
    }
}

/// Hashes a string and produces a pleasing colour from that hash.
pub fn string_to_colour(s: &str) -> Hsva {
    let hsv = shared_style::string_to_hsv(s);
    Hsva::new(hsv[0], hsv[1], hsv[2], 1.0)
}
