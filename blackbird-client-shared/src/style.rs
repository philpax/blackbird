//! Style definitions shared between the blackbird clients.

use serde::{Deserialize, Deserializer, Serialize};
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
pub struct FieldInfo {
    /// A human-readable label for settings UI.
    pub label: &'static str,
    /// The default HSV value.
    pub default: Hsv,
}

/// Metadata for one style group (a concept) shown in settings.
pub struct GroupInfo {
    /// The group name (settings section header).
    pub name: &'static str,
    /// The group's fields, in display order.
    pub fields: &'static [FieldInfo],
}

macro_rules! group {
    (
        $(#[$doc:meta])*
        $group:ident {
            $(
                $(#[$field_doc:meta])*
                $field:ident: [$($default:expr),+],
            )*
        }
    ) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
        #[serde(rename_all = "snake_case")]
        pub struct $group {
            $(
                #[doc = concat!("HSV colour for ", stringify!($field))]
                pub $field: Hsv,
            )*
        }
        impl Default for $group {
            fn default() -> Self {
                Self { $($field: [$($default),+],)* }
            }
        }
        impl $group {
            /// The fields in this group, in display order.
            pub const FIELDS: &'static [FieldInfo] = &[
                $(FieldInfo { label: stringify!($field), default: [$($default),+] },)*
            ];
            /// The number of fields in this group.
            pub const FIELD_COUNT: usize = 0 $(+ { let _ = stringify!($field); 1 })*;
            $(
                #[doc = concat!("Returns the gamma-corrected `Rgb` for ", stringify!($field), ".")]
                pub fn $field(&self) -> Rgb {
                    hsv_to_rgb(self.$field)
                }
            )*
        }
    };
}

group!(
    /// General app-wide colours (background, default text).
    General {
        /// The app background.
        background: [0.65, 0.40, 0.01],
        /// Default text colour.
        text: [0.0, 0.0, 1.0],
    }
);

group!(
    /// Library list colours.
    Library {
        /// Group/album header colour.
        album: [0.58, 0.90, 0.60],
        /// Album length colour.
        album_length: [0.0, 0.0, 0.75],
        /// Album year colour.
        album_year: [0.0, 0.0, 0.40],
        /// Track number colour.
        track_number: [0.60, 0.5, 0.90],
        /// Track length colour.
        track_length: [0.60, 0.90, 0.70],
        /// Track name colour.
        track_name: [0.0, 0.0, 1.0],
        /// Track name colour when hovered.
        track_name_hovered: [0.6, 0.6, 1.0],
        /// Track name colour when playing.
        track_name_playing: [0.55, 0.70, 1.0],
        /// Track duration colour.
        track_duration: [0.0, 0.0, 0.5],
        /// Library frame border colour.
        border: [0.12, 0.85, 0.75],
    }
);

group!(
    /// Current-track sidebar component colours.
    Sidebar {
        /// Lyrics text colour.
        lyrics_text: [0.0, 0.0, 1.0],
        /// Lyrics timestamp colour.
        lyrics_timestamp: [0.0, 0.0, 0.5],
        /// Similar-songs text colour.
        similar_text: [0.0, 0.0, 1.0],
        /// Lyrics panel frame border colour.
        lyrics_border: [0.55, 0.85, 0.75],
        /// Similar-songs panel frame border colour.
        similar_border: [0.35, 0.80, 0.70],
    }
);

group!(
    /// Now-playing bar and scrub bar colours.
    NowPlaying {
        /// Now-playing text colour.
        text: [0.0, 0.0, 1.0],
        /// Track name colour.
        track_name: [0.0, 0.0, 1.0],
        /// Playing-track highlight colour.
        track_name_playing: [0.55, 0.70, 1.0],
        /// Duration colour.
        duration: [0.0, 0.0, 0.5],
        /// Now-playing frame border colour.
        border: [0.75, 0.80, 0.70],
    }
);

group!(
    /// Other panel (search/queue/logs/settings) colours.
    Panels {
        /// Panel frame border colour.
        border: [0.30, 0.75, 0.80],
        /// Search input highlight colour.
        search_highlight: [0.55, 0.70, 1.0],
    }
);

/// All style groups, in display order.
pub const GROUPS: &[GroupInfo] = &[
    GroupInfo {
        name: "General",
        fields: General::FIELDS,
    },
    GroupInfo {
        name: "Library",
        fields: Library::FIELDS,
    },
    GroupInfo {
        name: "Sidebar",
        fields: Sidebar::FIELDS,
    },
    GroupInfo {
        name: "Now playing",
        fields: NowPlaying::FIELDS,
    },
    GroupInfo {
        name: "Panels",
        fields: Panels::FIELDS,
    },
];

/// Style configuration with HSV colors for various UI elements, grouped by
/// concept (general, library, sidebar, now-playing, panels).
///
/// The nested groups serialize as `[general]`, `[library]`, etc. A legacy flat
/// layout (`background_hsv`, `text_hsv`, ... at the top level) is accepted on
/// parse; the next save writes the grouped form.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Style {
    /// General app-wide colours.
    pub general: General,
    /// Library list colours.
    pub library: Library,
    /// Sidebar component colours.
    pub sidebar: Sidebar,
    /// Now-playing/scrub bar colours.
    pub now_playing: NowPlaying,
    /// Other panel colours.
    pub panels: Panels,
}

impl Style {
    /// Number of HSV color fields across all groups.
    pub const FIELD_COUNT: usize = General::FIELD_COUNT
        + Library::FIELD_COUNT
        + Sidebar::FIELD_COUNT
        + NowPlaying::FIELD_COUNT
        + Panels::FIELD_COUNT;

    /// Returns a reference to the HSV field at the given global index (across
    /// all groups, in [`GROUPS`] order).
    pub fn field(&self, index: usize) -> &Hsv {
        let mut i = index;
        if i < General::FIELD_COUNT {
            return general_field(&self.general, i);
        }
        i -= General::FIELD_COUNT;
        if i < Library::FIELD_COUNT {
            return library_field(&self.library, i);
        }
        i -= Library::FIELD_COUNT;
        if i < Sidebar::FIELD_COUNT {
            return sidebar_field(&self.sidebar, i);
        }
        i -= Sidebar::FIELD_COUNT;
        if i < NowPlaying::FIELD_COUNT {
            return now_playing_field(&self.now_playing, i);
        }
        i -= NowPlaying::FIELD_COUNT;
        if i < Panels::FIELD_COUNT {
            return panels_field(&self.panels, i);
        }
        panic!("style field index out of bounds: {index}");
    }

    /// Returns a mutable reference to the HSV field at the given global index.
    pub fn field_mut(&mut self, index: usize) -> &mut Hsv {
        let mut i = index;
        if i < General::FIELD_COUNT {
            return general_field_mut(&mut self.general, i);
        }
        i -= General::FIELD_COUNT;
        if i < Library::FIELD_COUNT {
            return library_field_mut(&mut self.library, i);
        }
        i -= Library::FIELD_COUNT;
        if i < Sidebar::FIELD_COUNT {
            return sidebar_field_mut(&mut self.sidebar, i);
        }
        i -= Sidebar::FIELD_COUNT;
        if i < NowPlaying::FIELD_COUNT {
            return now_playing_field_mut(&mut self.now_playing, i);
        }
        i -= NowPlaying::FIELD_COUNT;
        if i < Panels::FIELD_COUNT {
            return panels_field_mut(&mut self.panels, i);
        }
        panic!("style field index out of bounds: {index}");
    }

    /// Returns the default value for the HSV field at the given global index.
    pub fn default_field(index: usize) -> Hsv {
        let mut i = index;
        if i < General::FIELD_COUNT {
            return General::FIELDS[i].default;
        }
        i -= General::FIELD_COUNT;
        if i < Library::FIELD_COUNT {
            return Library::FIELDS[i].default;
        }
        i -= Library::FIELD_COUNT;
        if i < Sidebar::FIELD_COUNT {
            return Sidebar::FIELDS[i].default;
        }
        i -= Sidebar::FIELD_COUNT;
        if i < NowPlaying::FIELD_COUNT {
            return NowPlaying::FIELDS[i].default;
        }
        i -= NowPlaying::FIELD_COUNT;
        if i < Panels::FIELD_COUNT {
            return Panels::FIELDS[i].default;
        }
        panic!("style field index out of bounds: {index}");
    }

    /// The global index of the first field of the `group_index`-th group in
    /// [`GROUPS`] (0-based).
    pub fn group_start(group_index: usize) -> usize {
        match group_index {
            0 => 0,
            1 => General::FIELD_COUNT,
            2 => General::FIELD_COUNT + Library::FIELD_COUNT,
            3 => General::FIELD_COUNT + Library::FIELD_COUNT + Sidebar::FIELD_COUNT,
            4 => {
                General::FIELD_COUNT
                    + Library::FIELD_COUNT
                    + Sidebar::FIELD_COUNT
                    + NowPlaying::FIELD_COUNT
            }
            _ => panic!("style group index out of bounds: {group_index}"),
        }
    }
}

/// Generates per-group `field_by_index`/`field_by_index_mut` helpers.
fn general_field(group: &General, index: usize) -> &Hsv {
    match index {
        0 => &group.background,
        1 => &group.text,
        _ => panic!("style field index out of bounds for General({index})"),
    }
}
fn general_field_mut(group: &mut General, index: usize) -> &mut Hsv {
    match index {
        0 => &mut group.background,
        1 => &mut group.text,
        _ => panic!("style field index out of bounds for General({index})"),
    }
}

fn library_field(group: &Library, index: usize) -> &Hsv {
    match index {
        0 => &group.album,
        1 => &group.album_length,
        2 => &group.album_year,
        3 => &group.track_number,
        4 => &group.track_length,
        5 => &group.track_name,
        6 => &group.track_name_hovered,
        7 => &group.track_name_playing,
        8 => &group.track_duration,
        9 => &group.border,
        _ => panic!("style field index out of bounds for Library({index})"),
    }
}
fn library_field_mut(group: &mut Library, index: usize) -> &mut Hsv {
    match index {
        0 => &mut group.album,
        1 => &mut group.album_length,
        2 => &mut group.album_year,
        3 => &mut group.track_number,
        4 => &mut group.track_length,
        5 => &mut group.track_name,
        6 => &mut group.track_name_hovered,
        7 => &mut group.track_name_playing,
        8 => &mut group.track_duration,
        9 => &mut group.border,
        _ => panic!("style field index out of bounds for Library({index})"),
    }
}

fn sidebar_field(group: &Sidebar, index: usize) -> &Hsv {
    match index {
        0 => &group.lyrics_text,
        1 => &group.lyrics_timestamp,
        2 => &group.similar_text,
        3 => &group.lyrics_border,
        4 => &group.similar_border,
        _ => panic!("style field index out of bounds for Sidebar({index})"),
    }
}
fn sidebar_field_mut(group: &mut Sidebar, index: usize) -> &mut Hsv {
    match index {
        0 => &mut group.lyrics_text,
        1 => &mut group.lyrics_timestamp,
        2 => &mut group.similar_text,
        3 => &mut group.lyrics_border,
        4 => &mut group.similar_border,
        _ => panic!("style field index out of bounds for Sidebar({index})"),
    }
}

fn now_playing_field(group: &NowPlaying, index: usize) -> &Hsv {
    match index {
        0 => &group.text,
        1 => &group.track_name,
        2 => &group.track_name_playing,
        3 => &group.duration,
        4 => &group.border,
        _ => panic!("style field index out of bounds for NowPlaying({index})"),
    }
}
fn now_playing_field_mut(group: &mut NowPlaying, index: usize) -> &mut Hsv {
    match index {
        0 => &mut group.text,
        1 => &mut group.track_name,
        2 => &mut group.track_name_playing,
        3 => &mut group.duration,
        4 => &mut group.border,
        _ => panic!("style field index out of bounds for NowPlaying({index})"),
    }
}

fn panels_field(group: &Panels, index: usize) -> &Hsv {
    match index {
        0 => &group.border,
        1 => &group.search_highlight,
        _ => panic!("style field index out of bounds for Panels({index})"),
    }
}
fn panels_field_mut(group: &mut Panels, index: usize) -> &mut Hsv {
    match index {
        0 => &mut group.border,
        1 => &mut group.search_highlight,
        _ => panic!("style field index out of bounds for Panels({index})"),
    }
}

/// Accepts both the legacy flat layout and the nested grouped layout on parse.
impl<'de> Deserialize<'de> for Style {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize, Default)]
        struct Flat {
            background_hsv: Option<Hsv>,
            text_hsv: Option<Hsv>,
            album_hsv: Option<Hsv>,
            album_length_hsv: Option<Hsv>,
            album_year_hsv: Option<Hsv>,
            track_number_hsv: Option<Hsv>,
            track_length_hsv: Option<Hsv>,
            track_name_hsv: Option<Hsv>,
            track_name_hovered_hsv: Option<Hsv>,
            track_name_playing_hsv: Option<Hsv>,
            track_duration_hsv: Option<Hsv>,
            general: Option<General>,
            library: Option<Library>,
            sidebar: Option<Sidebar>,
            now_playing: Option<NowPlaying>,
            panels: Option<Panels>,
        }

        let flat = Flat::deserialize(deserializer)?;
        let default = Style::default();

        let general = flat.general.unwrap_or_else(|| General {
            background: flat.background_hsv.unwrap_or(default.general.background),
            text: flat.text_hsv.unwrap_or(default.general.text),
        });
        let library = flat.library.unwrap_or_else(|| Library {
            album: flat.album_hsv.unwrap_or(default.library.album),
            album_length: flat
                .album_length_hsv
                .unwrap_or(default.library.album_length),
            album_year: flat.album_year_hsv.unwrap_or(default.library.album_year),
            track_number: flat
                .track_number_hsv
                .unwrap_or(default.library.track_number),
            track_length: flat
                .track_length_hsv
                .unwrap_or(default.library.track_length),
            track_name: flat.track_name_hsv.unwrap_or(default.library.track_name),
            track_name_hovered: flat
                .track_name_hovered_hsv
                .unwrap_or(default.library.track_name_hovered),
            track_name_playing: flat
                .track_name_playing_hsv
                .unwrap_or(default.library.track_name_playing),
            track_duration: flat
                .track_duration_hsv
                .unwrap_or(default.library.track_duration),
            border: default.library.border,
        });
        let sidebar = flat.sidebar.unwrap_or(default.sidebar);
        let now_playing = flat.now_playing.unwrap_or(default.now_playing);
        let panels = flat.panels.unwrap_or(default.panels);

        Ok(Style {
            general,
            library,
            sidebar,
            now_playing,
            panels,
        })
    }
}

/// Serializes the nested grouped layout.
impl Serialize for Style {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("Style", 5)?;
        s.serialize_field("general", &self.general)?;
        s.serialize_field("library", &self.library)?;
        s.serialize_field("sidebar", &self.sidebar)?;
        s.serialize_field("now_playing", &self.now_playing)?;
        s.serialize_field("panels", &self.panels)?;
        s.end()
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
