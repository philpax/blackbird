/// Configuration types shared between the egui and TUI clients.
pub mod config {
    use blackbird_state::{PlaybackMode, TrackId};
    use serde::{Deserialize, Serialize};

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
