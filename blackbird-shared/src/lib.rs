/// Configuration types shared between the egui and TUI clients.
pub mod config {
    use blackbird_state::{PlaybackMode, TrackId};
    use serde::{Deserialize, Serialize};

    /// Server connection settings.
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    #[serde(default)]
    pub struct Server {
        pub base_url: String,
        pub username: String,
        pub password: String,
        pub transcode: bool,
    }
    impl Default for Server {
        fn default() -> Self {
            Self {
                base_url: "http://localhost:4533".to_string(),
                username: "YOUR_USERNAME".to_string(),
                password: "YOUR_PASSWORD".to_string(),
                transcode: false,
            }
        }
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
