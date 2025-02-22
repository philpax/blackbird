use egui::{ecolor::Hsva, Color32};
use serde::{Deserialize, Serialize};

macro_rules! style_fields {
    ($(($field:ident, $fn_name:ident, $default:expr)),* $(,)?) => {
        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        pub struct Style {
            $(
                /// Colour for $field.
                $field: [f32; 3],
            )*
        }
        impl Default for Style {
            fn default() -> Self {
                Self {
                    $($field: $default,)*
                }
            }
        }
        impl Style {
            $(
                pub fn $fn_name(&self) -> Color32 {
                    hsv_to_color32(self.$field)
                }
            )*
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
];

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
