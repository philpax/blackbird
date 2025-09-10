use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use blackbird_state::{Album, AlbumId, Group, Track, TrackId};
use serde::{Deserialize, Serialize};

use crate::queue::QueueState;

#[derive(Default)]
pub struct AppState {
    pub track_ids: Vec<TrackId>,
    pub track_map: HashMap<TrackId, Track>,
    pub groups: Vec<Arc<Group>>,
    pub albums: HashMap<AlbumId, Album>,
    pub cover_art_cache: HashMap<String, (Vec<u8>, std::time::Instant)>,
    pub pending_cover_art_requests: HashSet<String>,
    pub has_loaded_all_tracks: bool,

    pub current_track_and_position: Option<TrackAndPosition>,
    pub started_loading_track: Option<std::time::Instant>,
    pub playback_mode: PlaybackMode,
    pub queue: QueueState,

    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PlaybackMode {
    #[default]
    Sequential,
    Shuffle,
    RepeatOne,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackAndPosition {
    pub track_id: TrackId,
    pub position: Duration,
}
