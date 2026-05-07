/// Volume adjustment step (5%).
pub const VOLUME_STEP: f32 = 0.05;

/// Seek step in seconds.
pub const SEEK_STEP_SECS: i64 = 5;

/// Fraction of the window/terminal width used for the album art overlay.
pub const OVERLAY_WIDTH_FRACTION: f32 = 0.9;

pub mod config;
pub mod cover_art_cache;
pub mod library_scroll;
pub mod lyrics;
pub mod paths;
pub mod style;

#[cfg(feature = "media-controls")]
pub mod controls;

#[cfg(feature = "tray-icon")]
pub mod tray;

/// Direction of cycling through an ordered list of values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Forward,
    Backward,
}

/// Returns the value adjacent to `current` in `values`, wrapping around at
/// either end. If `current` isn't in `values`, returns the first element.
pub fn cycle<T: Copy + PartialEq>(values: &[T], current: T, direction: Direction) -> T {
    let len = values.len();
    let idx = values.iter().position(|v| *v == current).unwrap_or(0);
    let next = match direction {
        Direction::Forward => (idx + 1) % len,
        Direction::Backward => (idx + len - 1) % len,
    };
    values[next]
}

/// Load the application icon as an RGBA image.
pub fn load_icon() -> image::RgbaImage {
    image::load_from_memory(include_bytes!("../assets/icon.png"))
        .expect("failed to load embedded icon")
        .to_rgba8()
}
