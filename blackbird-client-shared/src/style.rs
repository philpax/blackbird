//! Style definitions shared between the blackbird clients.

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

/// Converts an HSV colour to a gamma-corrected `Rgb` (adapted from the
/// retired GUI, fusing together hsv conversion and gamma correction).
pub fn hsv_to_rgb(hsv: Hsv) -> Rgb {
    /// All ranges in 0-1, rgb is linear.
    fn from_hsv([h, s, v]: Hsv) -> Rgb {
        #![allow(clippy::many_single_char_names)]
        let h = (h.fract() + 1.0).fract(); // wrap
        let s = s.clamp(0.0, 1.0);

        let f = h * 6.0 - (h * 6.0).floor();
        let p = v * (1.0 - s);
        let q = v * (1.0 - f * s);
        let t = v * (1.0 - (1.0 - f) * s);

        let [r, g, b] = match (h * 6.0).floor() as i32 % 6 {
            0 => [v, t, p],
            1 => [q, v, p],
            2 => [p, v, t],
            3 => [p, q, v],
            4 => [t, p, v],
            5 => [v, p, q],
            _ => unreachable!(),
        };

        fn gamma_u8_from_linear_f32(l: f32) -> u8 {
            if l <= 0.0 {
                0
            } else if l <= 0.0031308 {
                fast_round(3294.6 * l)
            } else if l <= 1.0 {
                fast_round(269.025 * l.powf(1.0 / 2.4) - 14.025)
            } else {
                255
            }
        }

        fn fast_round(r: f32) -> u8 {
            (r + 0.5) as _ // rust does a saturating cast since 1.45
        }

        Rgb::new(
            gamma_u8_from_linear_f32(r),
            gamma_u8_from_linear_f32(g),
            gamma_u8_from_linear_f32(b),
        )
    }
    from_hsv(hsv)
}

/// Metadata for one HSV style field within a group.
///
/// The accessors are the only way to reach a field generically; there is no
/// index arithmetic to keep in sync with the struct definitions.
#[derive(Debug)]
pub struct FieldInfo {
    /// A human-readable label for settings UI.
    pub label: &'static str,
    /// The default HSV value.
    pub default: Hsv,
    get_fn: fn(&Style) -> &Hsv,
    get_mut_fn: fn(&mut Style) -> &mut Hsv,
}

impl FieldInfo {
    /// The field's current value within `style`.
    pub fn get(&self, style: &Style) -> Hsv {
        *(self.get_fn)(style)
    }

    /// A mutable reference to the field's value within `style`.
    pub fn get_mut<'a>(&self, style: &'a mut Style) -> &'a mut Hsv {
        (self.get_mut_fn)(style)
    }

    /// Whether the field still holds its default value.
    pub fn is_default(&self, style: &Style) -> bool {
        self.get(style) == self.default
    }

    /// Restores the field to its default value.
    pub fn reset(&self, style: &mut Style) {
        *self.get_mut(style) = self.default;
    }
}

/// Metadata for one style group (a concept) shown in settings.
#[derive(Debug)]
pub struct GroupInfo {
    /// The group name (settings section header).
    pub name: &'static str,
    /// The group's fields, in display order.
    pub fields: &'static [FieldInfo],
}

/// Declares the entire style tree: the group structs, their per-field `Rgb`
/// accessors, the [`Style`] aggregate, and the [`GROUPS`] metadata used to
/// drive settings UI.
macro_rules! style {
    (
        $(
            $(#[$group_doc:meta])*
            $group:ident: $Group:ident = $group_label:literal {
                $(
                    $(#[$field_doc:meta])*
                    $field:ident = $field_label:literal, [$($default:expr),+ $(,)?];
                )*
            }
        )*
    ) => {
        $(
            $(#[$group_doc])*
            #[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
            #[serde(default, rename_all = "snake_case")]
            pub struct $Group {
                $(
                    $(#[$field_doc])*
                    pub $field: Hsv,
                )*
            }

            impl Default for $Group {
                fn default() -> Self {
                    Self { $($field: [$($default),+],)* }
                }
            }

            impl $Group {
                $(
                    #[doc = concat!("The gamma-corrected `Rgb` for `", stringify!($field), "`.")]
                    pub fn $field(&self) -> Rgb {
                        hsv_to_rgb(self.$field)
                    }
                )*
            }
        )*

        /// Style configuration with HSV colours for various UI elements,
        /// grouped by concept. The groups serialize as `[general]`,
        /// `[library]`, etc., and each group tolerates missing fields.
        #[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
        #[serde(default)]
        pub struct Style {
            $(
                $(#[$group_doc])*
                pub $group: $Group,
            )*
        }

        /// All style groups, in display order.
        pub const GROUPS: &[GroupInfo] = &[
            $(
                GroupInfo {
                    name: $group_label,
                    fields: &[
                        $(
                            FieldInfo {
                                label: $field_label,
                                default: [$($default),+],
                                get_fn: {
                                    fn get(style: &Style) -> &Hsv {
                                        &style.$group.$field
                                    }
                                    get
                                },
                                get_mut_fn: {
                                    fn get_mut(style: &mut Style) -> &mut Hsv {
                                        &mut style.$group.$field
                                    }
                                    get_mut
                                },
                            },
                        )*
                    ],
                },
            )*
        ];
    };
}

style! {
    /// General app-wide colours (background, default text).
    general: General = "General" {
        /// The app background.
        background = "Background", [0.65, 0.40, 0.01];
        /// Default text colour.
        text = "Text", [0.0, 0.0, 1.0];
    }

    /// Library list colours.
    library: Library = "Library" {
        /// Group/album header colour.
        album = "Album", [0.58, 0.90, 0.60];
        /// Album length colour.
        album_length = "Album length", [0.0, 0.0, 0.75];
        /// Album year colour.
        album_year = "Album year", [0.0, 0.0, 0.40];
        /// Track number colour.
        track_number = "Track number", [0.60, 0.5, 0.90];
        /// Track length colour.
        track_length = "Track length", [0.60, 0.90, 0.70];
        /// Track name colour.
        track_name = "Track name", [0.0, 0.0, 1.0];
        /// Track name colour when hovered.
        track_name_hovered = "Track name (hovered)", [0.6, 0.6, 1.0];
        /// Track name colour when playing.
        track_name_playing = "Track name (playing)", [0.55, 0.70, 1.0];
        /// Track duration colour.
        track_duration = "Track duration", [0.0, 0.0, 0.5];
        /// Library frame border colour.
        border = "Border", [0.12, 0.85, 0.75];
    }

    /// Current-track sidebar component colours.
    sidebar: Sidebar = "Sidebar" {
        /// Lyrics text colour.
        lyrics_text = "Lyrics text", [0.0, 0.0, 1.0];
        /// Lyrics timestamp colour.
        lyrics_timestamp = "Lyrics timestamp", [0.0, 0.0, 0.5];
        /// Similar-songs text colour.
        similar_text = "Similar songs text", [0.0, 0.0, 1.0];
        /// Lyrics panel frame border colour.
        lyrics_border = "Lyrics border", [0.55, 0.85, 0.75];
        /// Similar-songs panel frame border colour.
        similar_border = "Similar songs border", [0.35, 0.80, 0.70];
    }

    /// Now-playing bar and scrub bar colours.
    now_playing: NowPlaying = "Now playing" {
        /// Now-playing text colour.
        text = "Text", [0.0, 0.0, 1.0];
        /// Track name colour.
        track_name = "Track name", [0.0, 0.0, 1.0];
        /// Playing-track highlight colour.
        track_name_playing = "Track name (playing)", [0.55, 0.70, 1.0];
        /// Duration colour.
        duration = "Duration", [0.0, 0.0, 0.5];
        /// Now-playing frame border colour.
        border = "Border", [0.75, 0.80, 0.70];
    }

    /// Other panel (search/queue/logs/settings) colours.
    panels: Panels = "Panels" {
        /// Panel frame border colour.
        border = "Border", [0.30, 0.75, 0.80];
        /// Search input highlight colour.
        search_highlight = "Search highlight", [0.55, 0.70, 1.0];
    }
}

/// Describes how a heart/star indicator should be displayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeartState {
    /// Unstarred + not hovered: invisible.
    Hidden,
    /// Unstarred + hovered: show red outline (preview what starring looks like).
    Preview,
    /// Starred + not hovered: show red filled.
    Active,
    /// Starred + hovered: show white outline (indicate "click to unstar").
    HoveredActive,
}

impl HeartState {
    pub fn from_interaction(starred: bool, hovered: bool) -> Self {
        match (starred, hovered) {
            (false, false) => Self::Hidden,
            (false, true) => Self::Preview,
            (true, false) => Self::Active,
            (true, true) => Self::HoveredActive,
        }
    }

    pub fn visible(&self) -> bool {
        !matches!(self, Self::Hidden)
    }

    pub fn is_red(&self) -> bool {
        matches!(self, Self::Preview | Self::Active)
    }

    pub fn filled(&self) -> bool {
        matches!(self, Self::Active)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The metadata accessors must point at the field they describe: mutating
    /// through `get_mut` has to be visible through `get`, and the recorded
    /// default has to match the one `Default` produces.
    #[test]
    fn field_metadata_matches_struct() {
        let mut style = Style::default();
        for (i, field) in GROUPS.iter().flat_map(|g| g.fields).enumerate() {
            assert!(field.is_default(&style), "{} is not default", field.label);

            let sentinel = [i as f32 / 1000.0, 0.123, 0.456];
            *field.get_mut(&mut style) = sentinel;
            assert_eq!(field.get(&style), sentinel);
        }

        // Every accessor addressed a distinct field, so nothing was clobbered.
        for (i, field) in GROUPS.iter().flat_map(|g| g.fields).enumerate() {
            assert_eq!(field.get(&style), [i as f32 / 1000.0, 0.123, 0.456]);
            field.reset(&mut style);
        }
        assert_eq!(style, Style::default());
    }

    #[test]
    fn labels_are_unique_within_a_group() {
        for group in GROUPS {
            for (i, field) in group.fields.iter().enumerate() {
                assert!(
                    !group.fields[..i].iter().any(|f| f.label == field.label),
                    "duplicate label {:?} in {}",
                    field.label,
                    group.name
                );
            }
        }
    }

    #[test]
    fn partial_tables_fall_back_to_defaults_per_field() {
        let style: Style = toml::from_str(
            r#"
            [library]
            album = [0.1, 0.2, 0.3]
            "#,
        )
        .unwrap();

        assert_eq!(style.library.album, [0.1, 0.2, 0.3]);
        assert_eq!(
            style.library.track_name,
            Style::default().library.track_name
        );
        assert_eq!(style.general, General::default());
    }

    #[test]
    fn roundtrips_through_toml() {
        let mut style = Style::default();
        style.panels.border = [0.9, 0.8, 0.7];
        let text = toml::to_string(&style).unwrap();
        assert_eq!(toml::from_str::<Style>(&text).unwrap(), style);
    }
}
