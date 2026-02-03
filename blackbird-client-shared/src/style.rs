//! Style definitions shared between the egui and TUI clients.

use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};

/// HSV color representation (hue 0-1, saturation 0-1, value 0-1).
pub type Hsv = [f32; 3];

/// RGB color representation (0-255 per channel).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}
impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

/// Hashes a string and produces a pleasing colour from that hash.
pub fn string_to_hsv(s: &str) -> Hsv {
    const DISTINCT_COLOURS: u64 = 36_000;

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut hasher);
    let hash = hasher.finish();
    let hue = (hash % DISTINCT_COLOURS) as f32 / DISTINCT_COLOURS as f32;

    [hue, 0.75, 0.75]
}

macro_rules! style_fields {
    ($(($field:ident, $fn_name:ident, $default:expr)),* $(,)?) => {
        /// Style configuration with HSV colors for various UI elements.
        #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
        #[serde(default)]
        pub struct Style {
            $(
                #[doc = concat!("HSV colour for ", stringify!($field))]
                pub $field: Hsv,
            )*
            /// Scroll multiplier for mouse wheel scrolling.
            pub scroll_multiplier: f32,
        }
        impl Default for Style {
            fn default() -> Self {
                Self {
                    $($field: $default,)*
                    scroll_multiplier: 50.0,
                }
            }
        }
    }
}

style_fields![
    (background_hsv, background, [0.65, 0.40, 0.01]),
    (text_hsv, text, [0.0, 0.0, 1.0]),
    (album_hsv, album, [0.58, 0.90, 0.60]),
    (album_length_hsv, album_length, [0.0, 0.0, 0.75]),
    (album_year_hsv, album_year, [0.0, 0.0, 0.40]),
    (track_number_hsv, track_number, [0.60, 0.5, 0.90]),
    (track_length_hsv, track_length, [0.60, 0.90, 0.70]),
    (track_name_hsv, track_name, [0.0, 0.0, 1.0]),
    (track_name_hovered_hsv, track_name_hovered, [0.6, 0.6, 1.0]),
    (
        track_name_playing_hsv,
        track_name_playing,
        [0.55, 0.70, 1.0]
    ),
    (track_duration_hsv, track_duration, [0.0, 0.0, 0.5]),
];
