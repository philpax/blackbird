use std::{collections::HashMap, sync::Arc, time::Duration};

use blackbird_state::{Album, AlbumId, Group, Track, TrackId};
use serde::{Deserialize, Serialize};

use crate::{TrackDisplayDetails, queue::QueueState};

#[derive(Default)]
pub struct AppState {
    pub library: Library,

    pub current_track_and_position: Option<TrackAndPosition>,
    pub started_loading_track: Option<std::time::Instant>,
    // bit ugly but cbf plumbing it better
    pub last_requested_track_for_ui_scroll: Option<TrackId>,
    pub playback_mode: PlaybackMode,
    pub queue: QueueState,
    pub volume: f32,

    pub error: Option<AppStateError>,
}

#[derive(Default)]
pub struct Library {
    pub track_ids: Vec<TrackId>,
    pub track_map: HashMap<TrackId, Track>,
    pub groups: Vec<Arc<Group>>,
    pub albums: HashMap<AlbumId, Album>,
    pub has_loaded_all_tracks: bool,

    // Reverse lookup maps
    pub album_to_group_index: HashMap<AlbumId, usize>,
    pub track_to_group_index: HashMap<TrackId, usize>,
    pub track_to_group_track_index: HashMap<TrackId, usize>,
}
impl Library {
    pub fn populate(
        &mut self,
        track_ids: Vec<TrackId>,
        track_map: HashMap<TrackId, Track>,
        groups: Vec<Arc<Group>>,
        albums: HashMap<AlbumId, Album>,
    ) {
        self.albums = albums;
        self.track_map = track_map;
        self.track_ids = track_ids;

        // Populate reverse lookup maps for efficient group shuffle navigation
        self.track_to_group_index.clear();
        self.track_to_group_track_index.clear();
        for (group_idx, group) in groups.iter().enumerate() {
            for (track_idx, track_id) in group.tracks.iter().enumerate() {
                self.track_to_group_index
                    .insert(track_id.clone(), group_idx);
                self.track_to_group_track_index
                    .insert(track_id.clone(), track_idx);
            }
            self.album_to_group_index
                .insert(group.album_id.clone(), group_idx);
        }

        self.groups = groups;
        self.has_loaded_all_tracks = true;
    }

    pub fn set_track_starred(&mut self, track_id: &TrackId, starred: bool) -> Option<bool> {
        let mut old_starred = None;
        if let Some(track) = self.track_map.get_mut(track_id) {
            old_starred = Some(track.starred);
            track.starred = starred;
        }
        old_starred
    }

    pub fn set_album_starred(&mut self, album_id: &AlbumId, starred: bool) -> Option<bool> {
        let mut old_starred = None;

        if let Some(album) = self.albums.get_mut(album_id) {
            old_starred = Some(album.starred);
            album.starred = starred;
        }
        if let Some(group_idx) = self.album_to_group_index.get(album_id)
            && let Some(group) = self.groups.get(*group_idx)
        {
            let group = Group {
                starred,
                ..(**group).clone()
            };
            self.groups[*group_idx] = Arc::new(group);
        }

        old_starred
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AppStateError {
    InitialFetchFailed { error: String },
    CoverArtFetchFailed { cover_art_id: String, error: String },
    LoadTrackFailed { track_id: TrackId, error: String },
    DecodeTrackFailed { track_id: TrackId, error: String },
    StarTrackFailed { track_id: TrackId, error: String },
    UnstarTrackFailed { track_id: TrackId, error: String },
    StarAlbumFailed { album_id: AlbumId, error: String },
    UnstarAlbumFailed { album_id: AlbumId, error: String },
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
                    TrackDisplayDetails::string_report(track_id, state)
                )
            }
            AppStateError::DecodeTrackFailed { track_id, error } => {
                format!(
                    "Track `{}` failed to decode: {error}",
                    TrackDisplayDetails::string_report(track_id, state)
                )
            }
            AppStateError::StarTrackFailed { track_id, error } => {
                format!(
                    "Failed to star track `{}`: {error}",
                    TrackDisplayDetails::string_report(track_id, state)
                )
            }
            AppStateError::UnstarTrackFailed { track_id, error } => {
                format!(
                    "Failed to unstar track `{}`: {error}",
                    TrackDisplayDetails::string_report(track_id, state)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PlaybackMode {
    #[default]
    Sequential,
    RepeatOne,
    GroupRepeat,
    Shuffle,
    GroupShuffle,
}
impl PlaybackMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            PlaybackMode::Sequential => "Sequential",
            PlaybackMode::RepeatOne => "Repeat One",
            PlaybackMode::GroupRepeat => "Group Repeat",
            PlaybackMode::Shuffle => "Shuffle",
            PlaybackMode::GroupShuffle => "Group Shuffle",
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
