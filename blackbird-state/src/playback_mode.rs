use serde::{Deserialize, Serialize};

/// The playback mode for the player.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PlaybackMode {
    /// Plays tracks sequentially.
    #[default]
    Sequential,
    /// Repeats the current track.
    RepeatOne,
    /// Repeats the current group.
    GroupRepeat,
    /// Shuffles all tracks.
    Shuffle,
    /// Shuffles only liked tracks.
    LikedShuffle,
    /// Shuffles groups and plays them in order.
    GroupShuffle,
    /// Shuffles groups with liked tracks and plays them in order.
    LikedGroupShuffle,
}

impl PlaybackMode {
    /// Returns a human-readable name for the mode.
    pub fn as_str(&self) -> &'static str {
        match self {
            PlaybackMode::Sequential => "Sequential",
            PlaybackMode::RepeatOne => "Repeat One",
            PlaybackMode::GroupRepeat => "Group Repeat",
            PlaybackMode::Shuffle => "Shuffle",
            PlaybackMode::LikedShuffle => "Liked Shuffle",
            PlaybackMode::GroupShuffle => "Group Shuffle",
            PlaybackMode::LikedGroupShuffle => "Liked Group Shuffle",
        }
    }
}

impl std::fmt::Display for PlaybackMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
