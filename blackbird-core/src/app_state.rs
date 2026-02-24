use std::time::Duration;

use blackbird_state::{AlbumId, CoverArtId, TrackId};
use serde::{Deserialize, Serialize};

use crate::{Library, PlaybackState, TrackDisplayDetails, queue::QueueState};

/// The sort order for displaying albums in the library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SortOrder {
    /// Sort albums alphabetically by artist name.
    #[default]
    Alphabetical,
    /// Sort albums by year, newest first.
    NewestFirst,
    /// Sort albums by when they were added to the library, most recent first.
    RecentlyAdded,
}

impl SortOrder {
    /// Returns a short human-readable label for the sort order.
    pub fn as_str(&self) -> &'static str {
        match self {
            SortOrder::Alphabetical => "a-z",
            SortOrder::NewestFirst => "newest",
            SortOrder::RecentlyAdded => "recent",
        }
    }
}

impl std::fmt::Display for SortOrder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

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
            PlaybackMode::Sequential => "sequential",
            PlaybackMode::RepeatOne => "repeat one",
            PlaybackMode::GroupRepeat => "group repeat",
            PlaybackMode::Shuffle => "shuffle",
            PlaybackMode::LikedShuffle => "liked shuffle",
            PlaybackMode::GroupShuffle => "group shuffle",
            PlaybackMode::LikedGroupShuffle => "liked group shuffle",
        }
    }
}

impl std::fmt::Display for PlaybackMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

pub struct AppState {
    pub library: Library,

    pub current_track_and_position: Option<TrackAndPosition>,
    pub started_loading_track: Option<std::time::Instant>,
    // bit ugly but cbf plumbing it better
    pub last_requested_track_for_ui_scroll: Option<TrackId>,
    pub playback_state: PlaybackState,
    pub playback_mode: PlaybackMode,
    pub sort_order: SortOrder,
    pub queue: QueueState,
    pub volume: f32,

    pub scrobble_state: ScrobbleState,

    pub error: Option<AppStateError>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            library: Library::default(),
            current_track_and_position: None,
            started_loading_track: None,
            last_requested_track_for_ui_scroll: None,
            playback_state: PlaybackState::Stopped,
            playback_mode: PlaybackMode::default(),
            sort_order: SortOrder::default(),
            queue: QueueState::new(),
            volume: 0.0,
            scrobble_state: ScrobbleState::default(),
            error: None,
        }
    }
}

/// Tracks scrobbling state for the currently playing track.
#[derive(Debug, Default, Clone)]
pub struct ScrobbleState {
    /// The track we're tracking scrobble state for
    pub track_id: Option<TrackId>,
    /// Whether we've already scrobbled this track in the current listening session
    pub has_scrobbled: bool,
    /// Total accumulated listening time for this track (handles pauses and seeks)
    pub accumulated_listening_time: Duration,
    /// The last position we observed (to detect seeks backward)
    pub last_position: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AppStateError {
    InitialFetchFailed {
        error: String,
    },
    CoverArtFetchFailed {
        cover_art_id: CoverArtId,
        error: String,
    },
    LoadTrackFailed {
        track_id: TrackId,
        error: String,
    },
    DecodeTrackFailed {
        track_id: TrackId,
        error: String,
    },
    StarTrackFailed {
        track_id: TrackId,
        error: String,
    },
    UnstarTrackFailed {
        track_id: TrackId,
        error: String,
    },
    StarAlbumFailed {
        album_id: AlbumId,
        error: String,
    },
    UnstarAlbumFailed {
        album_id: AlbumId,
        error: String,
    },
}
impl AppStateError {
    /// Should be paired with [`Self::display_message`]
    pub fn display_name(&self) -> &'static str {
        match self {
            AppStateError::InitialFetchFailed { .. } => "Failed to complete initial data fetch",
            AppStateError::CoverArtFetchFailed { .. } => "Failed to fetch cover art",
            AppStateError::LoadTrackFailed { .. } => "Failed to load track",
            AppStateError::DecodeTrackFailed { .. } => "Failed to decode track",
            AppStateError::StarTrackFailed { .. } => "Failed to star track",
            AppStateError::UnstarTrackFailed { .. } => "Failed to unstar track",
            AppStateError::StarAlbumFailed { .. } => "Failed to star album",
            AppStateError::UnstarAlbumFailed { .. } => "Failed to unstar album",
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
                    TrackDisplayDetails::string_report_without_time(track_id, state)
                )
            }
            AppStateError::DecodeTrackFailed { track_id, error } => {
                format!(
                    "Track `{}` failed to decode: {error}",
                    TrackDisplayDetails::string_report_without_time(track_id, state)
                )
            }
            AppStateError::StarTrackFailed { track_id, error } => {
                format!(
                    "Failed to star track `{}`: {error}",
                    TrackDisplayDetails::string_report_without_time(track_id, state)
                )
            }
            AppStateError::UnstarTrackFailed { track_id, error } => {
                format!(
                    "Failed to unstar track `{}`: {error}",
                    TrackDisplayDetails::string_report_without_time(track_id, state)
                )
            }
            AppStateError::StarAlbumFailed { album_id, error } => {
                format!("Failed to star album `{}`: {error}", album_id,)
            }
            AppStateError::UnstarAlbumFailed { album_id, error } => {
                format!("Failed to unstar album `{}`: {error}", album_id,)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackAndPosition {
    pub track_id: TrackId,
    pub position: Duration,
}
