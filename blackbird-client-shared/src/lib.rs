use blackbird_core::PlaybackMode;

/// Volume adjustment step (5%).
pub const VOLUME_STEP: f32 = 0.05;

/// Seek step in seconds.
pub const SEEK_STEP_SECS: i64 = 5;

pub mod alphabet_scroll;
pub mod config;
pub mod cover_art_cache;
pub mod lyrics;
pub mod style;

#[cfg(feature = "media-controls")]
pub mod controls;

#[cfg(feature = "tray-icon")]
pub mod tray;

/// Returns the next playback mode in the cycle.
pub fn next_playback_mode(current: PlaybackMode) -> PlaybackMode {
    match current {
        PlaybackMode::Sequential => PlaybackMode::RepeatOne,
        PlaybackMode::RepeatOne => PlaybackMode::GroupRepeat,
        PlaybackMode::GroupRepeat => PlaybackMode::Shuffle,
        PlaybackMode::Shuffle => PlaybackMode::LikedShuffle,
        PlaybackMode::LikedShuffle => PlaybackMode::GroupShuffle,
        PlaybackMode::GroupShuffle => PlaybackMode::LikedGroupShuffle,
        PlaybackMode::LikedGroupShuffle => PlaybackMode::Sequential,
    }
}

/// Load the application icon as an RGBA image.
pub fn load_icon() -> image::RgbaImage {
    image::load_from_memory(include_bytes!("../assets/icon.png"))
        .expect("failed to load embedded icon")
        .to_rgba8()
}
