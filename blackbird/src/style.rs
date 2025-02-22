use egui::ecolor::Hsva;

const fn hsv(h: f32, s: f32, v: f32) -> Hsva {
    Hsva { h, s, v, a: 1.0 }
}

/// Background colour for the main window.
pub const BACKGROUND_COLOUR: Hsva = hsv(0.65, 0.4, 0.01);

/// Default text colour.
pub const TEXT_COLOUR: Hsva = hsv(0.0, 0.0, 1.0);

/// Colour for albums.
pub const ALBUM_COLOUR: Hsva = hsv(0.6, 0.7, 0.4);

/// Colour for album years.
pub const ALBUM_YEAR_COLOUR: Hsva = hsv(0.0, 0.0, 0.4);

/// Colour for track numbers.
pub const TRACK_NUMBER_COLOUR: Hsva = hsv(0.65, 0.8, 0.7);

/// Colour for track lengths.
pub const TRACK_LENGTH_COLOUR: Hsva = hsv(0.6, 0.7, 0.4);

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
