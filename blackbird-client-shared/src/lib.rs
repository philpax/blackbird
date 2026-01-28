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
