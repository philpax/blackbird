/// Configuration types shared between the egui and TUI clients.
pub mod config {
    use blackbird_core::{PlaybackMode, blackbird_state::TrackId};
    use serde::{Deserialize, Serialize};

    /// Shared configuration fields used by both the egui and TUI clients.
    #[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
    #[serde(default)]
    pub struct Config {
        /// Server connection settings.
        #[serde(default)]
        pub server: blackbird_shared::config::Server,
        /// Last playback state, persisted across sessions.
        #[serde(default)]
        pub last_playback: LastPlayback,
    }

    /// Last playback state, persisted across sessions.
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    #[serde(default)]
    pub struct LastPlayback {
        /// The track that was playing when the client was last closed.
        pub track_id: Option<TrackId>,
        /// The position within the track, in seconds.
        pub track_position_secs: f64,
        /// The playback mode that was active.
        pub playback_mode: PlaybackMode,
    }
    impl Default for LastPlayback {
        fn default() -> Self {
            Self {
                track_id: None,
                track_position_secs: 0.0,
                playback_mode: PlaybackMode::default(),
            }
        }
    }
}

/// Alphabet scroll indicator logic shared between egui and TUI clients.
pub mod alphabet_scroll {
    /// Computes alphabet letter positions as fractions of total content.
    ///
    /// Takes an iterator of (first_letter, line_count) pairs for each group/entry,
    /// and returns a list of (letter, fraction) pairs representing where each
    /// alphabetical section starts in the library.
    ///
    /// The `cluster_threshold` parameter controls how close letters can be before
    /// they are merged (typically 0.015 for GUI, or 1.0/visible_height for TUI).
    pub fn compute_positions(
        entries: impl Iterator<Item = (char, usize)>,
        cluster_threshold: f32,
    ) -> Vec<(char, f32)> {
        // Collect letter positions with counts for clustering
        let mut letter_positions: Vec<(char, f32, usize)> = Vec::new();
        let mut current_line = 0usize;
        let mut last_letter: Option<char> = None;

        let entries: Vec<_> = entries.collect();
        let total_lines: usize = entries.iter().map(|(_, lines)| lines).sum();

        if total_lines == 0 {
            return Vec::new();
        }

        for (first_char, line_count) in entries {
            let letter = first_char.to_uppercase().next().unwrap_or(first_char);

            if last_letter != Some(letter) {
                let fraction = current_line as f32 / total_lines as f32;
                letter_positions.push((letter, fraction, 1));
                last_letter = Some(letter);
            } else if let Some(last) = letter_positions.last_mut() {
                // Increment count for clustering (prefer letters with more entries)
                last.2 += 1;
            }

            current_line += line_count;
        }

        if letter_positions.is_empty() {
            return Vec::new();
        }

        // Cluster nearby letters to avoid overlap
        cluster_letters(letter_positions, cluster_threshold)
    }

    /// Clusters letters that are too close together, keeping the one with highest count.
    fn cluster_letters(positions: Vec<(char, f32, usize)>, threshold: f32) -> Vec<(char, f32)> {
        let mut clustered: Vec<(char, f32)> = Vec::new();
        let mut i = 0;

        while i < positions.len() {
            let mut cluster_end = i + 1;

            // Find all letters within threshold distance
            while cluster_end < positions.len() {
                let distance = positions[cluster_end].1 - positions[i].1;
                if distance >= threshold {
                    break;
                }
                cluster_end += 1;
            }

            // Select the letter with the highest count in this cluster
            let best = positions[i..cluster_end]
                .iter()
                .max_by_key(|(_, _, count)| count)
                .unwrap();

            clustered.push((best.0, best.1));
            i = cluster_end;
        }

        clustered
    }

    /// Computes the position fraction (0.0-1.0) for a specific item in the library.
    ///
    /// Takes an iterator of (is_target, line_count) pairs and returns the fraction
    /// where the first item with is_target=true appears.
    pub fn compute_item_position(entries: impl Iterator<Item = (bool, usize)>) -> Option<f32> {
        let mut current_line = 0usize;
        let mut target_line = None;
        let mut total_lines = 0usize;

        for (is_target, line_count) in entries {
            if is_target && target_line.is_none() {
                target_line = Some(current_line);
            }
            current_line += line_count;
            total_lines += line_count;
        }

        let target = target_line?;
        if total_lines == 0 {
            return None;
        }

        Some(target as f32 / total_lines as f32)
    }
}

/// Style definitions shared between the egui and TUI clients.
pub mod style {
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

    /// Convert HSV to RGB.
    pub fn hsv_to_rgb(hsv: Hsv) -> Rgb {
        let [h, s, v] = hsv;
        let c = v * s;
        let h_prime = h * 6.0;
        let x = c * (1.0 - (h_prime % 2.0 - 1.0).abs());
        let m = v - c;

        let (r, g, b) = if h_prime < 1.0 {
            (c, x, 0.0)
        } else if h_prime < 2.0 {
            (x, c, 0.0)
        } else if h_prime < 3.0 {
            (0.0, c, x)
        } else if h_prime < 4.0 {
            (0.0, x, c)
        } else if h_prime < 5.0 {
            (x, 0.0, c)
        } else {
            (c, 0.0, x)
        };

        Rgb {
            r: ((r + m) * 255.0) as u8,
            g: ((g + m) * 255.0) as u8,
            b: ((b + m) * 255.0) as u8,
        }
    }

    /// Apply sRGB gamma correction (linear to sRGB).
    fn linear_to_srgb(linear: f32) -> f32 {
        if linear <= 0.0031308 {
            linear * 12.92
        } else {
            1.055 * linear.powf(1.0 / 2.4) - 0.055
        }
    }

    /// Convert HSV to gamma-corrected RGB for terminal display.
    /// egui's Hsva treats values as linear and applies gamma internally,
    /// so terminals need gamma-corrected values to match egui's appearance.
    pub fn hsv_to_rgb_gamma(hsv: Hsv) -> Rgb {
        let [h, s, v] = hsv;
        let c = v * s;
        let h_prime = h * 6.0;
        let x = c * (1.0 - (h_prime % 2.0 - 1.0).abs());
        let m = v - c;

        let (r, g, b) = if h_prime < 1.0 {
            (c, x, 0.0)
        } else if h_prime < 2.0 {
            (x, c, 0.0)
        } else if h_prime < 3.0 {
            (0.0, c, x)
        } else if h_prime < 4.0 {
            (0.0, x, c)
        } else if h_prime < 5.0 {
            (x, 0.0, c)
        } else {
            (c, 0.0, x)
        };

        Rgb {
            r: (linear_to_srgb(r + m) * 255.0) as u8,
            g: (linear_to_srgb(g + m) * 255.0) as u8,
            b: (linear_to_srgb(b + m) * 255.0) as u8,
        }
    }

    /// Hashes a string and produces a gamma-corrected RGB colour for terminal display.
    pub fn string_to_rgb_gamma(s: &str) -> Rgb {
        hsv_to_rgb_gamma(string_to_hsv(s))
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

    /// Hashes a string and produces a pleasing RGB colour from that hash.
    pub fn string_to_rgb(s: &str) -> Rgb {
        hsv_to_rgb(string_to_hsv(s))
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

            impl Style {
                $(
                    #[doc = concat!("Get RGB colour for ", stringify!($fn_name))]
                    pub fn $fn_name(&self) -> Rgb {
                        hsv_to_rgb(self.$field)
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
        (track_name_hsv, track_name, [0.0, 0.0, 1.0]),
        (track_name_hovered_hsv, track_name_hovered, [0.6, 0.6, 1.0]),
        (
            track_name_playing_hsv,
            track_name_playing,
            [0.55, 0.70, 1.0]
        ),
        (track_duration_hsv, track_duration, [0.0, 0.0, 0.5]),
    ];
}
