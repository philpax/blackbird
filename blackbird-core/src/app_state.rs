use std::{collections::HashMap, sync::Arc, time::Duration};

use blackbird_state::{Album, AlbumId, Group, Track, TrackId};
use serde::{Deserialize, Serialize};

use crate::{TrackDisplayDetails, queue::QueueState};

#[derive(Default)]
pub struct AppState {
    pub track_ids: Vec<TrackId>,
    pub track_map: HashMap<TrackId, Track>,
    pub groups: Vec<Arc<Group>>,
    pub albums: HashMap<AlbumId, Album>,
    pub has_loaded_all_tracks: bool,

    pub current_track_and_position: Option<TrackAndPosition>,
    pub started_loading_track: Option<std::time::Instant>,
    // bit ugly but cbf plumbing it better
    pub last_requested_track_for_ui_scroll: Option<TrackId>,
    pub playback_mode: PlaybackMode,
    pub queue: QueueState,
    pub volume: f32,

    pub error: Option<AppStateError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AppStateError {
    InitialFetchFailed { error: String },
    CoverArtFetchFailed { cover_art_id: String, error: String },
    LoadTrackFailed { track_id: TrackId, error: String },
    DecodeTrackFailed { track_id: TrackId, error: String },
}
impl AppStateError {
    /// Should be paired with [`Self::display_message`]
    pub fn display_name(&self) -> &'static str {
        match self {
            AppStateError::InitialFetchFailed { .. } => "Failed to complete initial data fetch",
            AppStateError::CoverArtFetchFailed { .. } => "Failed to fetch cover art",
            AppStateError::LoadTrackFailed { .. } => "Failed to load track",
            AppStateError::DecodeTrackFailed { .. } => "Failed to decode track",
        }
    }

    /// Should be paired with [`Self::display_name`]
    pub fn display_message(&self, state: &AppState) -> String {
        match self {
            AppStateError::InitialFetchFailed { error } => error.clone(),
            AppStateError::CoverArtFetchFailed {
                cover_art_id,
                error,
            } => format!("Cover art failed to load: {cover_art_id}: {error}"),
            AppStateError::LoadTrackFailed { track_id, error } => {
                format!(
                    "Track `{}` failed to load: {error}",
                    TrackDisplayDetails::string_report(track_id, state)
                )
            }
            AppStateError::DecodeTrackFailed { track_id, error } => {
                format!(
                    "Track `{}` failed to decode: {error}",
                    TrackDisplayDetails::string_report(track_id, state)
                )
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PlaybackMode {
    #[default]
    Sequential,
    Shuffle,
    RepeatOne,
}
impl PlaybackMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            PlaybackMode::Sequential => "Sequential",
            PlaybackMode::Shuffle => "Shuffle",
            PlaybackMode::RepeatOne => "Repeat One",
        }
    }
}
impl std::fmt::Display for PlaybackMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackAndPosition {
    pub track_id: TrackId,
    pub position: Duration,
}
