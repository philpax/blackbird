use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use blackbird_state::{Album, AlbumId, Group, Song, SongId};
use serde::{Deserialize, Serialize};

use crate::queue::QueueState;

#[derive(Default)]
pub struct AppState {
    pub song_ids: Vec<SongId>,
    pub song_map: HashMap<SongId, Song>,
    pub groups: Vec<Arc<Group>>,
    pub albums: HashMap<AlbumId, Album>,
    pub cover_art_cache: HashMap<String, (Vec<u8>, std::time::Instant)>,
    pub pending_cover_art_requests: HashSet<String>,
    pub has_loaded_all_songs: bool,

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
    pub song_id: SongId,
    pub position: Duration,
}
