use std::{
    collections::{HashMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    sync::{Arc, RwLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use blackbird_state::TrackId;
use blackbird_subsonic::ClientResult;

use crate::{
    AppState, Logic, PlaybackMode, TrackLoadMode,
    app_state::AppStateError,
    library::Library,
    playback_thread::{LogicToPlaybackMessage, PlaybackThreadSendHandle},
};

/// How a loaded track should be handled after streaming.
pub(crate) enum TrackLoadBehavior {
    /// Play the track immediately.
    Play,
    /// Cache only, don't send to the playback thread.
    CacheOnly,
    /// Load into the playback thread paused at the given position.
    Paused(Duration),
}

// Queue-specific state stored under AppState.
pub struct QueueState {
    pub shuffle_seed: u64,
    pub audio_cache: HashMap<TrackId, Vec<u8>>,
    pub pending_audio_requests: HashMap<TrackId, u64>,
    pub request_counter: u64,
    pub current_target: Option<TrackId>,
    pub current_target_request_id: Option<u64>,
    pub pending_skip_after_error: bool,
    pub group_shuffle_seed: u64,
    pub next_track_appended: Option<TrackId>,

    /// The precomputed full playback ordering for the current mode.
    pub ordered_tracks: Vec<TrackId>,
    /// The index of the currently playing track within `ordered_tracks`.
    pub current_index: usize,
}

impl Default for QueueState {
    fn default() -> Self {
        Self::new()
    }
}

impl QueueState {
    pub fn new() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let seed = next_seed((now.as_secs() << 32) ^ (now.subsec_nanos() as u64));
        QueueState {
            shuffle_seed: seed,
            group_shuffle_seed: next_seed(seed),
            audio_cache: HashMap::new(),
            pending_audio_requests: HashMap::new(),
            request_counter: 0,
            current_target: None,
            current_target_request_id: None,
            pending_skip_after_error: false,
            next_track_appended: None,
            ordered_tracks: vec![],
            current_index: 0,
        }
    }
}

impl Logic {
    pub(super) fn handle_track_end_advance(&self) {
        let mode = self.get_playback_mode();
        tracing::debug!("End-of-track advance handling; mode={:?}", mode);
        match mode {
            PlaybackMode::RepeatOne => {
                if let Some(current) = self.get_playing_track_id() {
                    tracing::debug!("RepeatOne: replaying current track {}", current.0);
                    self.schedule_play_track(&current);
                }
            }
            _ => {
                self.schedule_next_track();
            }
        }
    }

    pub(super) fn schedule_next_track(&self) {
        if let Some(next) = self.compute_next_track_id() {
            tracing::debug!("Advancing to next track {}", next.0);
            // Advance the index before scheduling.
            {
                let mut st = self.write_state();
                let len = st.queue.ordered_tracks.len();
                if len > 0 {
                    st.queue.current_index = (st.queue.current_index + 1) % len;
                }
            }
            self.schedule_play_track(&next);
        } else {
            tracing::warn!("No next track available to advance to");
        }
    }

    pub(super) fn schedule_previous_track(&self) {
        if let Some(prev) = self.compute_previous_track_id() {
            tracing::debug!("Advancing to previous track {}", prev.0);
            // Decrement the index before scheduling.
            {
                let mut st = self.write_state();
                let len = st.queue.ordered_tracks.len();
                if len > 0 {
                    st.queue.current_index = (st.queue.current_index + len - 1) % len;
                }
            }
            self.schedule_play_track(&prev);
        } else {
            tracing::warn!("No previous track available to advance to");
        }
    }

    pub(super) fn schedule_play_track(&self, track_id: &TrackId) {
        self.write_state().last_requested_track_for_ui_scroll = Some(track_id.clone());

        // Set target and show loading indicator.
        let req_id = {
            let mut st = self.write_state();
            st.started_loading_track = Some(std::time::Instant::now());
            st.queue.current_target = Some(track_id.clone());
            st.queue.request_counter = st.queue.request_counter.wrapping_add(1);

            let req_id = st.queue.request_counter;
            st.queue.current_target_request_id = Some(req_id);

            // Reset gapless playback state since we're manually changing tracks.
            st.queue.next_track_appended = None;

            tracing::debug!("Scheduling track {} (req_id={})", track_id.0, req_id);
            req_id
        };

        // If already cached, play immediately.
        let cached_track = self.read_state().queue.audio_cache.get(track_id).cloned();
        if let Some(data) = cached_track {
            tracing::debug!("Playing from cache: {}", track_id.0);
            self.playback_thread
                .send_handle()
                .send(LogicToPlaybackMessage::LoadTrack(
                    track_id.clone(),
                    data,
                    TrackLoadMode::Play,
                ));
        } else {
            tracing::debug!("Loading track {} (req_id={})", track_id.0, req_id);
            self.load_track_internal(track_id.clone(), req_id, TrackLoadBehavior::Play);
        }

        // Also ensure nearby cache is populated.
        self.ensure_cache_window();
    }

    pub(super) fn load_track_internal(
        &self,
        track_id: TrackId,
        request_id: u64,
        behavior: TrackLoadBehavior,
    ) {
        let client = self.client.clone();
        let state = self.state.clone();
        let playback_tx = self.playback_thread.send_handle();
        let transcode = self.transcode;

        state
            .write()
            .unwrap()
            .queue
            .pending_audio_requests
            .insert(track_id.clone(), request_id);

        self.tokio_thread.spawn(async move {
            tracing::debug!(
                "Starting load request for {} (req_id={})",
                track_id.0,
                request_id
            );
            let response = client
                .stream(&track_id.0, transcode.then(|| "mp3".to_string()), None)
                .await;
            handle_load_response(response, state, playback_tx, track_id, request_id, behavior);
        });
    }

    pub(super) fn schedule_next_group(&self) {
        let target = {
            let st = self.read_state();
            find_next_group_start(&st).map(|idx| (idx, st.queue.ordered_tracks[idx].clone()))
        };
        self.play_group_target("next", target);
    }

    pub(super) fn schedule_previous_group(&self) {
        let target = {
            let st = self.read_state();
            find_previous_group_start(&st).map(|idx| (idx, st.queue.ordered_tracks[idx].clone()))
        };
        self.play_group_target("previous", target);
    }

    fn play_group_target(&self, direction: &str, target: Option<(usize, TrackId)>) {
        if let Some((idx, track_id)) = target {
            tracing::debug!("Advancing to {direction} group, track {}", track_id.0);
            self.write_state().queue.current_index = idx;
            self.schedule_play_track(&track_id);
        }
    }

    pub(super) fn compute_next_track_id(&self) -> Option<TrackId> {
        let st = self.read_state();
        let ordered = &st.queue.ordered_tracks;
        if ordered.is_empty() {
            return None;
        }
        let next_index = (st.queue.current_index + 1) % ordered.len();
        Some(ordered[next_index].clone())
    }

    pub(super) fn compute_previous_track_id(&self) -> Option<TrackId> {
        let st = self.read_state();
        let ordered = &st.queue.ordered_tracks;
        if ordered.is_empty() {
            return None;
        }
        let prev_index = (st.queue.current_index + ordered.len() - 1) % ordered.len();
        Some(ordered[prev_index].clone())
    }

    /// Ensures that the audio cache contains tracks surrounding the current queue position.
    pub(super) fn ensure_cache_window(&self) {
        let window = {
            let st = self.read_state();
            compute_window_from_queue(&st.queue, 2)
        };

        self.write_state()
            .queue
            .audio_cache
            .retain(|key, _| window.contains(key));

        // Prefetch in window order.
        let mut scheduled = 0usize;
        for sid in &window {
            let already = {
                let st = self.read_state();
                st.queue.audio_cache.contains_key(sid)
                    || st.queue.pending_audio_requests.contains_key(sid)
            };
            if !already {
                let req_id = {
                    let mut st = self.write_state();
                    st.queue.request_counter = st.queue.request_counter.wrapping_add(1);
                    st.queue.request_counter
                };
                self.load_track_internal(sid.clone(), req_id, TrackLoadBehavior::CacheOnly);
                scheduled += 1;
            }
        }
        tracing::debug!(
            "Cache window ensured around index {}: scheduled={}",
            self.read_state().queue.current_index,
            scheduled
        );
    }

    /// Recomputes the playback queue ordering for the current mode
    /// and sets `current_index` to the position of `current_track` (or 0 if not found).
    pub fn recompute_queue(&self, current_track: Option<&TrackId>) {
        let mut st = self.write_state();
        recompute_queue_on_state(&mut st, current_track);
    }

    /// Returns (tracks_before, current_track, tracks_after) by slicing `ordered_tracks`
    /// around `current_index`, limited to `radius` entries in each direction.
    pub fn get_queue_window(&self, radius: usize) -> (Vec<TrackId>, Option<TrackId>, Vec<TrackId>) {
        let st = self.read_state();
        let ordered = &st.queue.ordered_tracks;
        if ordered.is_empty() {
            return (vec![], None, vec![]);
        }

        let idx = st.queue.current_index.min(ordered.len() - 1);
        let current = Some(ordered[idx].clone());

        let len = ordered.len();
        let mut before = Vec::with_capacity(radius);
        let mut after = Vec::with_capacity(radius);

        for i in 1..=radius {
            if i >= len {
                break;
            }
            let prev_idx = (idx + len - i) % len;
            before.push(ordered[prev_idx].clone());
        }
        before.reverse();

        for i in 1..=radius {
            if i >= len {
                break;
            }
            let next_idx = (idx + i) % len;
            after.push(ordered[next_idx].clone());
        }

        (before, current, after)
    }
}

pub(crate) fn handle_load_response(
    response: ClientResult<Vec<u8>>,
    state: Arc<RwLock<AppState>>,
    playback_tx: PlaybackThreadSendHandle,
    track_id: TrackId,
    request_id: u64,
    behavior: TrackLoadBehavior,
) {
    match response {
        Ok(data) => {
            let is_current_target =
                state.read().unwrap().queue.current_target.as_ref() == Some(&track_id);
            state
                .write()
                .unwrap()
                .queue
                .audio_cache
                .insert(track_id.clone(), data.clone());

            match behavior {
                TrackLoadBehavior::Play if is_current_target => {
                    tracing::debug!(
                        "Load complete and current: playing {} (req_id={})",
                        track_id.0,
                        request_id
                    );
                    playback_tx.send(LogicToPlaybackMessage::LoadTrack(
                        track_id.clone(),
                        data,
                        TrackLoadMode::Play,
                    ));
                }
                TrackLoadBehavior::Paused(position) if is_current_target => {
                    tracing::debug!(
                        "Load complete and current: loading paused {} (req_id={})",
                        track_id.0,
                        request_id
                    );
                    playback_tx.send(LogicToPlaybackMessage::LoadTrack(
                        track_id.clone(),
                        data,
                        TrackLoadMode::Paused(position),
                    ));
                }
                _ => {
                    tracing::debug!(
                        "Load complete but not sending to playback for {track_id} (req_id={request_id})"
                    );
                }
            }

            state
                .write()
                .unwrap()
                .queue
                .pending_audio_requests
                .remove(&track_id);
        }
        Err(e) => {
            let mut st = state.write().unwrap();
            let is_current = st
                .queue
                .current_target_request_id
                .is_some_and(|rid| rid == request_id)
                && st.queue.current_target.as_ref() == Some(&track_id);

            if is_current {
                tracing::warn!(
                    "Load error for current target {track_id} (req_id={request_id}): {}",
                    e.to_string()
                );
                st.error = Some(AppStateError::LoadTrackFailed {
                    track_id,
                    error: e.to_string(),
                });
                st.queue.pending_skip_after_error = true;
            } else {
                tracing::debug!(
                    "Load error for stale/non-current {track_id} (req_id={request_id}): {e}"
                );
            }
        }
    }
}

/// Recomputes the queue ordering on a mutable `AppState` reference.
/// Useful when the state write lock is already held (e.g. during `initial_fetch`).
pub fn recompute_queue_on_state(st: &mut AppState, current_track: Option<&TrackId>) {
    st.queue.ordered_tracks =
        compute_full_ordering(&st.library, st.playback_mode, &st.queue, current_track);

    // Set current_index to the position of current_track (or 0 if not found).
    // If the current track isn't in the ordering (e.g. switching to LikedGroupShuffle
    // from a track not in any liked group), prepend it so that the queue accurately
    // reflects what's playing. It naturally falls behind once the user advances.
    let current = current_track.or(st.current_track_and_position.as_ref().map(|t| &t.track_id));
    if let Some(tid) = current {
        if let Some(pos) = st.queue.ordered_tracks.iter().position(|t| t == tid) {
            st.queue.current_index = pos;
        } else {
            st.queue.ordered_tracks.insert(0, tid.clone());
            st.queue.current_index = 0;
        }
    } else {
        st.queue.current_index = 0;
    }

    tracing::debug!(
        "Queue recomputed: {} tracks, current_index={}",
        st.queue.ordered_tracks.len(),
        st.queue.current_index,
    );
}

/// Computes the full playback ordering for a given mode.
fn compute_full_ordering(
    library: &Library,
    mode: PlaybackMode,
    queue: &QueueState,
    current_track: Option<&TrackId>,
) -> Vec<TrackId> {
    match mode {
        PlaybackMode::Sequential => library.track_ids.clone(),

        PlaybackMode::RepeatOne => {
            // The current track, or an empty queue if nothing is playing.
            let track = current_track.cloned();
            match track {
                Some(t) => vec![t],
                None => vec![],
            }
        }

        PlaybackMode::GroupRepeat => {
            // All tracks in the current track's group, or empty if nothing playing.
            let Some(tid) = current_track else {
                return vec![];
            };
            let Some(&group_idx) = library.track_to_group_index.get(tid) else {
                return vec![];
            };
            match library.groups.get(group_idx) {
                Some(group) => group.tracks.clone(),
                None => vec![],
            }
        }

        PlaybackMode::Shuffle => {
            let mut tracks = library.track_ids.clone();
            tracks.sort_by_key(|tid| shuffle_key(tid, queue.shuffle_seed));
            tracks
        }

        PlaybackMode::LikedShuffle => {
            let mut tracks: Vec<TrackId> = library
                .track_ids
                .iter()
                .filter(|tid| library.track_map.get(tid).is_some_and(|t| t.starred))
                .cloned()
                .collect();
            tracks.sort_by_key(|tid| shuffle_key(tid, queue.shuffle_seed));
            tracks
        }

        PlaybackMode::GroupShuffle => {
            // Sort group indices by shuffle key, then flatten each group's tracks.
            let mut group_indices: Vec<usize> = (0..library.groups.len()).collect();
            group_indices.sort_by_key(|&idx| shuffle_key(idx, queue.group_shuffle_seed));
            group_indices
                .into_iter()
                .flat_map(|idx| library.groups[idx].tracks.iter().cloned())
                .collect()
        }

        PlaybackMode::LikedGroupShuffle => {
            // Same as GroupShuffle but filtered to starred groups.
            let mut group_indices: Vec<usize> = library
                .groups
                .iter()
                .enumerate()
                .filter(|(_, g)| g.starred)
                .map(|(idx, _)| idx)
                .collect();
            group_indices.sort_by_key(|&idx| shuffle_key(idx, queue.group_shuffle_seed));
            group_indices
                .into_iter()
                .flat_map(|idx| library.groups[idx].tracks.iter().cloned())
                .collect()
        }
    }
}

/// Returns the group index for the track at `idx` in `ordered_tracks`, if available.
fn group_at(st: &AppState, idx: usize) -> Option<usize> {
    st.library
        .track_to_group_index
        .get(&st.queue.ordered_tracks[idx])
        .copied()
}

/// Scans from `from` in `direction` (+1 forward, -1 backward), wrapping around
/// `ordered_tracks`, and returns the first index whose group differs from the
/// group at `from`. Returns `None` if the entire queue is a single group.
fn scan_to_group_boundary(st: &AppState, from: usize, direction: isize) -> Option<usize> {
    let len = st.queue.ordered_tracks.len();
    let start_group = group_at(st, from);
    for offset in 1..len {
        let idx = (from as isize + direction * offset as isize).rem_euclid(len as isize) as usize;
        if group_at(st, idx) != start_group {
            return Some(idx);
        }
    }
    None
}

/// Scans forward from `current_index` to find the first track in a different group.
/// Returns `None` if the entire queue is one group.
fn find_next_group_start(st: &AppState) -> Option<usize> {
    scan_to_group_boundary(st, st.queue.current_index, 1)
}

/// Finds the first track of the previous group relative to `current_index`.
/// If the current position is not at the start of its group, returns the start
/// of the current group. Otherwise, returns the start of the preceding group.
fn find_previous_group_start(st: &AppState) -> Option<usize> {
    let len = st.queue.ordered_tracks.len();
    let current_idx = st.queue.current_index;

    // Scan backward: the boundary is the last track of the previous group.
    // One step forward from there is the start of the current group.
    let prev_group_end = scan_to_group_boundary(st, current_idx, -1)?;
    let start_of_current = (prev_group_end + 1) % len;

    if start_of_current != current_idx {
        // Not at the start of the current group — go there.
        Some(start_of_current)
    } else {
        // Already at the start — scan backward from the previous group's last track
        // to find where that group begins.
        let prev_prev_end = scan_to_group_boundary(st, prev_group_end, -1)?;
        Some((prev_prev_end + 1) % len)
    }
}

/// Computes a cache window of track IDs around `current_index` in the precomputed queue.
fn compute_window_from_queue(queue: &QueueState, radius: usize) -> Vec<TrackId> {
    let ordered = &queue.ordered_tracks;
    if ordered.is_empty() {
        return vec![];
    }

    let idx = queue.current_index.min(ordered.len() - 1);
    let len = ordered.len();
    let mut out = Vec::with_capacity(1 + radius * 2);

    // Center.
    out.push(ordered[idx].clone());

    // Previous tracks.
    for i in 1..=radius {
        if i >= len {
            break;
        }
        out.push(ordered[(idx + len - i) % len].clone());
    }

    // Next tracks.
    for i in 1..=radius {
        if i >= len {
            break;
        }
        out.push(ordered[(idx + i) % len].clone());
    }

    out
}

// Deterministic shuffle key based on seed and id.
fn shuffle_key(id: impl Hash, seed: u64) -> u64 {
    let mut hasher = DefaultHasher::new();
    id.hash(&mut hasher);
    let id_hash = hasher.finish();

    // Use a strong mixing function (murmurhash3 finalizer).
    // This provides excellent avalanche properties ensuring diverse shuffle orderings.
    let mut x = id_hash ^ seed;
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51afd7ed558ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ceb9fe1a85ec53);
    x ^= x >> 33;
    x
}

fn next_seed(seed: u64) -> u64 {
    // Use murmurhash3 finalizer for strong seed progression.
    let mut x = seed.wrapping_add(0x9e3779b97f4a7c15); // Add golden ratio to avoid trivial cycles.
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51afd7ed558ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ceb9fe1a85ec53);
    x ^= x >> 33;
    x
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use blackbird_state::{AlbumId, Group, Track, TrackId};
    use smol_str::SmolStr;

    use super::*;
    use crate::{Library, SortOrder};

    fn make_track(idx: usize) -> Track {
        Track {
            id: TrackId(format!("t{idx}")),
            title: SmolStr::new(format!("Track {idx}")),
            artist: None,
            track: None,
            year: None,
            _genre: None,
            duration: Some(180),
            disc_number: None,
            starred: idx % 3 == 0, // every 3rd track is starred
            play_count: None,
            album_id: None,
        }
    }

    fn make_group(g: usize, track_ids: Vec<TrackId>) -> Arc<Group> {
        Arc::new(Group {
            album_id: AlbumId(SmolStr::new(format!("album{g}"))),
            album: SmolStr::new(format!("Album {g}")),
            artist: SmolStr::new(format!("Artist {g}")),
            sort_artist: SmolStr::new(format!("Artist {g}")),
            year: None,
            duration: 0,
            tracks: track_ids,
            cover_art_id: None,
            starred: g % 2 == 0, // every other group is starred
        })
    }

    /// Creates a minimal library with `n` tracks spread across `group_count` groups.
    fn make_library(n: usize, group_count: usize) -> Library {
        let mut library = Library::default();
        let mut track_map = HashMap::new();
        let mut groups = Vec::new();

        let tracks_per_group = n / group_count.max(1);
        let mut track_idx = 0;

        for g in 0..group_count {
            let mut group_tracks = Vec::new();
            let count = if g == group_count - 1 {
                n - track_idx
            } else {
                tracks_per_group
            };
            for _ in 0..count {
                let track = make_track(track_idx);
                let tid = track.id.clone();
                track_map.insert(tid.clone(), track);
                group_tracks.push(tid);
                track_idx += 1;
            }
            groups.push(make_group(g, group_tracks));
        }

        library.populate(
            vec![],
            track_map,
            groups,
            HashMap::new(),
            SortOrder::Alphabetical,
        );
        library
    }

    fn make_queue() -> QueueState {
        let mut q = QueueState::new();
        // Use fixed seeds for determinism.
        q.shuffle_seed = 42;
        q.group_shuffle_seed = 99;
        q
    }

    #[test]
    fn sequential_ordering_matches_library_order() {
        let library = make_library(5, 1);
        let queue = make_queue();
        let ordering = compute_full_ordering(&library, PlaybackMode::Sequential, &queue, None);
        assert_eq!(ordering, library.track_ids);
    }

    #[test]
    fn repeat_one_single_track() {
        let library = make_library(5, 1);
        let queue = make_queue();
        let current = library.track_ids[2].clone();
        let ordering =
            compute_full_ordering(&library, PlaybackMode::RepeatOne, &queue, Some(&current));
        assert_eq!(ordering, vec![current]);
    }

    #[test]
    fn repeat_one_no_current_track() {
        let library = make_library(5, 1);
        let queue = make_queue();
        let ordering = compute_full_ordering(&library, PlaybackMode::RepeatOne, &queue, None);
        assert!(ordering.is_empty());
    }

    #[test]
    fn group_repeat_scoped_to_group() {
        let library = make_library(6, 2);
        let queue = make_queue();
        // Pick a track from the second group.
        let current = library.track_ids[4].clone();
        let ordering =
            compute_full_ordering(&library, PlaybackMode::GroupRepeat, &queue, Some(&current));
        // Should contain only tracks from the same group.
        let group_idx = library.track_to_group_index[&current];
        assert_eq!(ordering, library.groups[group_idx].tracks);
    }

    #[test]
    fn shuffle_deterministic_with_same_seed() {
        let library = make_library(10, 2);
        let queue = make_queue();
        let ord1 = compute_full_ordering(&library, PlaybackMode::Shuffle, &queue, None);
        let ord2 = compute_full_ordering(&library, PlaybackMode::Shuffle, &queue, None);
        assert_eq!(ord1, ord2);
    }

    #[test]
    fn shuffle_contains_all_tracks() {
        let library = make_library(10, 2);
        let queue = make_queue();
        let ordering = compute_full_ordering(&library, PlaybackMode::Shuffle, &queue, None);
        assert_eq!(ordering.len(), library.track_ids.len());
        for tid in &library.track_ids {
            assert!(ordering.contains(tid));
        }
    }

    #[test]
    fn liked_shuffle_filters_to_starred() {
        let library = make_library(10, 2);
        let queue = make_queue();
        let ordering = compute_full_ordering(&library, PlaybackMode::LikedShuffle, &queue, None);
        for tid in &ordering {
            assert!(library.track_map[tid].starred);
        }
        let expected_count = library
            .track_ids
            .iter()
            .filter(|tid| library.track_map[tid].starred)
            .count();
        assert_eq!(ordering.len(), expected_count);
    }

    #[test]
    fn liked_shuffle_empty_when_none_liked() {
        let mut library = make_library(5, 1);
        for track in library.track_map.values_mut() {
            track.starred = false;
        }
        let queue = make_queue();
        let ordering = compute_full_ordering(&library, PlaybackMode::LikedShuffle, &queue, None);
        assert!(ordering.is_empty());
    }

    #[test]
    fn group_shuffle_contains_all_tracks() {
        let library = make_library(10, 3);
        let queue = make_queue();
        let ordering = compute_full_ordering(&library, PlaybackMode::GroupShuffle, &queue, None);
        assert_eq!(ordering.len(), library.track_ids.len());
        for tid in &library.track_ids {
            assert!(ordering.contains(tid));
        }
    }

    #[test]
    fn liked_group_shuffle_filters_to_starred_groups() {
        let library = make_library(10, 4);
        let queue = make_queue();
        let ordering =
            compute_full_ordering(&library, PlaybackMode::LikedGroupShuffle, &queue, None);
        for tid in &ordering {
            let group_idx = library.track_to_group_index[tid];
            assert!(library.groups[group_idx].starred);
        }
    }

    #[test]
    fn empty_library_produces_empty_ordering() {
        let library = Library::default();
        let queue = make_queue();
        for mode in [
            PlaybackMode::Sequential,
            PlaybackMode::Shuffle,
            PlaybackMode::GroupShuffle,
            PlaybackMode::LikedShuffle,
            PlaybackMode::LikedGroupShuffle,
        ] {
            let ordering = compute_full_ordering(&library, mode, &queue, None);
            assert!(
                ordering.is_empty(),
                "mode {mode:?} should produce empty ordering"
            );
        }
    }

    #[test]
    fn single_track_library() {
        let library = make_library(1, 1);
        let queue = make_queue();
        let current = library.track_ids[0].clone();
        for mode in [
            PlaybackMode::Sequential,
            PlaybackMode::RepeatOne,
            PlaybackMode::GroupRepeat,
            PlaybackMode::Shuffle,
        ] {
            let ordering = compute_full_ordering(&library, mode, &queue, Some(&current));
            assert_eq!(ordering.len(), 1, "mode {mode:?} with single track");
            assert_eq!(ordering[0], current);
        }
    }

    #[test]
    fn wrapping_next_previous() {
        let library = make_library(3, 1);
        let queue = make_queue();
        let ordering = compute_full_ordering(&library, PlaybackMode::Sequential, &queue, None);

        let last_idx = ordering.len() - 1;
        let next_idx = (last_idx + 1) % ordering.len();
        assert_eq!(next_idx, 0);

        let prev_idx = (0 + ordering.len() - 1) % ordering.len();
        assert_eq!(prev_idx, last_idx);
    }

    #[test]
    fn recompute_queue_sets_current_index() {
        let library = make_library(5, 1);
        let mut st = AppState {
            library,
            ..AppState::default()
        };
        st.queue.shuffle_seed = 42;
        st.queue.group_shuffle_seed = 99;

        let target = st.library.track_ids[3].clone();
        recompute_queue_on_state(&mut st, Some(&target));

        assert_eq!(st.queue.ordered_tracks[st.queue.current_index], target);
    }

    #[test]
    fn out_of_mode_track_prepended_to_queue() {
        let library = make_library(6, 2);
        let mut st = AppState {
            library,
            playback_mode: PlaybackMode::LikedGroupShuffle,
            ..AppState::default()
        };
        st.queue.shuffle_seed = 42;
        st.queue.group_shuffle_seed = 99;

        // Unstar all groups so LikedGroupShuffle produces an empty ordering.
        for group in &mut st.library.groups {
            Arc::make_mut(group).starred = false;
        }

        // Pretend we're playing a track that won't appear in the filtered ordering.
        let playing = st.library.track_ids[0].clone();
        recompute_queue_on_state(&mut st, Some(&playing));

        // The playing track should be prepended as the sole entry.
        assert_eq!(st.queue.ordered_tracks.len(), 1);
        assert_eq!(st.queue.ordered_tracks[0], playing);
        assert_eq!(st.queue.current_index, 0);
    }

    #[test]
    fn compute_window_from_queue_basic() {
        let mut queue = make_queue();
        queue.ordered_tracks = vec![
            TrackId("a".to_string()),
            TrackId("b".to_string()),
            TrackId("c".to_string()),
            TrackId("d".to_string()),
            TrackId("e".to_string()),
        ];
        queue.current_index = 2; // "c"

        let window = compute_window_from_queue(&queue, 2);
        // Should contain c (center), a, b (prev), d, e (next).
        assert_eq!(window.len(), 5);
        assert_eq!(window[0], TrackId("c".to_string())); // center
    }

    #[test]
    fn compute_window_from_queue_empty() {
        let queue = make_queue();
        let window = compute_window_from_queue(&queue, 2);
        assert!(window.is_empty());
    }
}
