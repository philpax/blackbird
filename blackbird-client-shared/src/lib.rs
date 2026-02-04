pub mod alphabet_scroll;
pub mod config;
pub mod cover_art_cache;
pub mod lyrics;
pub mod style;

#[cfg(feature = "media-controls")]
pub mod controls;

#[cfg(feature = "tray-icon")]
pub mod tray;

/// Load the application icon as an RGBA image.
pub fn load_icon() -> image::RgbaImage {
    image::load_from_memory(include_bytes!("../assets/icon.png"))
        .expect("failed to load embedded icon")
        .to_rgba8()
}
