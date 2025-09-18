use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    sync::{Arc, RwLock},
    time::{SystemTime, UNIX_EPOCH},
};

use blackbird_state::{Group, TrackId};
use blackbird_subsonic::ClientResult;

use crate::{
    AppState, Logic, PlaybackMode,
    app_state::AppStateError,
    playback_thread::{LogicToPlaybackMessage, PlaybackThreadSendHandle},
};

// Queue-specific state stored under AppState
#[derive(Default)]
pub struct QueueState {
    pub shuffle_seed: u64,
    pub audio_cache: HashMap<TrackId, Vec<u8>>,
    pub pending_audio_requests: HashMap<TrackId, u64>,
    pub request_counter: u64,
    pub current_target: Option<TrackId>,
    pub current_target_request_id: Option<u64>,
    pub pending_skip_after_error: bool,
    pub group_shuffle_seed: u64,
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
            ..Default::default()
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
            self.schedule_play_track(&next);
        } else {
            tracing::warn!("No next track available to advance to");
        }
    }

    pub(super) fn schedule_previous_track(&self) {
        if let Some(prev) = self.compute_previous_track_id() {
            tracing::debug!("Advancing to previous track {}", prev.0);
            self.schedule_play_track(&prev);
        } else {
            tracing::warn!("No previous track available to advance to");
        }
    }

    pub(super) fn schedule_play_track(&self, track_id: &TrackId) {
        self.write_state().last_requested_track_for_ui_scroll = Some(track_id.clone());

        // Set target and show loading indicator
        let req_id = {
            let mut st = self.write_state();
            st.started_loading_track = Some(std::time::Instant::now());
            st.queue.current_target = Some(track_id.clone());
            st.queue.request_counter = st.queue.request_counter.wrapping_add(1);

            let req_id = st.queue.request_counter;
            st.queue.current_target_request_id = Some(req_id);
            tracing::debug!("Scheduling track {} (req_id={})", track_id.0, req_id);
            req_id
        };

        // If already cached, play immediately
        let cached_track = self.read_state().queue.audio_cache.get(track_id).cloned();
        if let Some(data) = cached_track {
            tracing::debug!("Playing from cache: {}", track_id.0);
            self.playback_thread
                .send_handle()
                .send(LogicToPlaybackMessage::PlayTrack(track_id.clone(), data));
        } else {
            tracing::debug!("Loading track {} (req_id={})", track_id.0, req_id);
            self.load_track_internal(track_id.clone(), req_id, true);
        }

        // Also ensure nearby cache is populated
        self.ensure_cache_window(track_id);
    }

    pub(super) fn load_track_internal(
        &self,
        track_id: TrackId,
        request_id: u64,
        for_playback: bool,
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
            handle_load_response(
                response,
                state,
                playback_tx,
                track_id,
                request_id,
                for_playback,
            );
        });

        fn handle_load_response(
            response: ClientResult<Vec<u8>>,
            state: Arc<RwLock<AppState>>,
            playback_tx: PlaybackThreadSendHandle,
            track_id: TrackId,
            request_id: u64,
            for_playback: bool,
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

                    // If this request was for immediate playback and is still the current target, play it
                    if for_playback && is_current_target {
                        tracing::debug!(
                            "Load complete and current: playing {} (req_id={})",
                            track_id.0,
                            request_id
                        );
                        playback_tx.send(LogicToPlaybackMessage::PlayTrack(track_id.clone(), data));
                    } else {
                        tracing::debug!(
                            "Load complete but not current (for_playback={for_playback}) for {track_id} (req_id={request_id})"
                        );
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
    }

    pub(super) fn compute_next_track_id(&self) -> Option<TrackId> {
        let st = self.read_state();
        compute_neighbour(
            &st.track_ids,
            &st.groups,
            &st.track_to_group_index,
            &st.track_to_group_track_index,
            &st.queue,
            st.playback_mode,
            &st.current_track_and_position.as_ref()?.track_id,
            Neighbour::Next,
        )
    }

    pub(super) fn compute_previous_track_id(&self) -> Option<TrackId> {
        let st = self.read_state();
        compute_neighbour(
            &st.track_ids,
            &st.groups,
            &st.track_to_group_index,
            &st.track_to_group_track_index,
            &st.queue,
            st.playback_mode,
            &st.current_track_and_position.as_ref()?.track_id,
            Neighbour::Prev,
        )
    }

    pub(super) fn ensure_cache_window(&self, center: &TrackId) {
        let window = {
            let st = self.read_state();
            compute_window(
                &st.track_ids,
                &st.groups,
                &st.track_to_group_index,
                &st.track_to_group_track_index,
                &st.queue,
                st.playback_mode,
                center,
                2,
            )
        };

        self.write_state()
            .queue
            .audio_cache
            .retain(|key, _| window.contains(key));

        // Prefetch in window order (center first)
        let mut scheduled = 0usize;
        for sid in window {
            let already = {
                let st = self.read_state();
                st.queue.audio_cache.contains_key(&sid)
                    || st.queue.pending_audio_requests.contains_key(&sid)
            };
            if !already {
                let req_id = {
                    let mut st = self.write_state();
                    st.queue.request_counter = st.queue.request_counter.wrapping_add(1);
                    st.queue.request_counter
                };
                self.load_track_internal(sid, req_id, false);
                scheduled += 1;
            }
        }
        tracing::debug!(
            "Cache window ensured around {}: scheduled={}",
            center.0,
            scheduled
        );
    }
}

// Deterministic shuffle key based on seed and id
fn shuffle_key(id: impl Hash, seed: u64) -> u64 {
    let mut hasher = DefaultHasher::new();
    id.hash(&mut hasher);
    let id_hash = hasher.finish();
    // Mix with seed using xorshift-like mixing
    let mut x = id_hash ^ seed;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    x
}

fn next_seed(seed: u64) -> u64 {
    // Simple xorshift* progression to derive next deterministic seed
    let mut x = seed;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    x.wrapping_mul(2685821657736338717)
}

#[allow(clippy::too_many_arguments)]
fn compute_window(
    ordered_tracks: &[TrackId],
    groups: &[Arc<Group>],
    track_to_group_index: &HashMap<TrackId, usize>,
    track_to_group_track_index: &HashMap<TrackId, usize>,
    queue: &QueueState,
    mode: PlaybackMode,
    center: &TrackId,
    radius: usize,
) -> Vec<TrackId> {
    let mut out = Vec::with_capacity(1 + radius * 2);
    out.push(center.clone());

    // Collect neighbours in each direction in a single pass when possible
    let prevs = compute_neighbours(
        ordered_tracks,
        groups,
        track_to_group_index,
        track_to_group_track_index,
        queue,
        mode,
        center,
        Neighbour::Prev,
        radius,
    );
    out.extend(prevs);

    let nexts = compute_neighbours(
        ordered_tracks,
        groups,
        track_to_group_index,
        track_to_group_track_index,
        queue,
        mode,
        center,
        Neighbour::Next,
        radius,
    );
    out.extend(nexts);

    out
}

#[derive(Clone, Copy)]
enum Neighbour {
    Prev,
    Next,
}
#[allow(clippy::too_many_arguments)]
fn compute_neighbour(
    ordered_tracks: &[TrackId],
    groups: &[Arc<Group>],
    track_to_group_index: &HashMap<TrackId, usize>,
    track_to_group_track_index: &HashMap<TrackId, usize>,
    queue: &QueueState,
    mode: PlaybackMode,
    center: &TrackId,
    dir: Neighbour,
) -> Option<TrackId> {
    compute_neighbours(
        ordered_tracks,
        groups,
        track_to_group_index,
        track_to_group_track_index,
        queue,
        mode,
        center,
        dir,
        1,
    )
    .first()
    .cloned()
}
#[allow(clippy::too_many_arguments)]
fn compute_neighbours(
    ordered_tracks: &[TrackId],
    groups: &[Arc<Group>],
    track_to_group_index: &HashMap<TrackId, usize>,
    track_to_group_track_index: &HashMap<TrackId, usize>,
    queue: &QueueState,
    mode: PlaybackMode,
    center: &TrackId,
    dir: Neighbour,
    count: usize,
) -> Vec<TrackId> {
    match mode {
        PlaybackMode::RepeatOne => Vec::new(),
        PlaybackMode::Sequential => {
            let Some(idx) = ordered_tracks.iter().position(|s| s == center) else {
                tracing::warn!("Center track {center} not found in ordered tracks");
                return vec![];
            };

            if ordered_tracks.len() <= 1 {
                return vec![];
            }

            match dir {
                Neighbour::Prev => {
                    (0..count)
                        .map(|i| {
                            let pos = if idx > i {
                                idx - i - 1
                            } else {
                                // Wrap around: go to end and count backwards
                                ordered_tracks.len() - (i + 1 - idx)
                            };
                            ordered_tracks[pos].clone()
                        })
                        .collect()
                }
                Neighbour::Next => (1..=count)
                    .map(|i| {
                        let pos = (idx + i) % ordered_tracks.len();
                        ordered_tracks[pos].clone()
                    })
                    .collect(),
            }
        }
        PlaybackMode::Shuffle => {
            let cur_key = shuffle_key(center, queue.shuffle_seed);
            match dir {
                Neighbour::Prev => {
                    // Keep k largest keys below cur_key using a min-heap (via Reverse)
                    let mut heap: BinaryHeap<(Reverse<u64>, TrackId)> = BinaryHeap::new();
                    for s in ordered_tracks {
                        if s == center {
                            continue;
                        }
                        let k = shuffle_key(s, queue.shuffle_seed);
                        if k < cur_key {
                            heap.push((Reverse(k), s.clone()));
                            if heap.len() > count {
                                heap.pop();
                            }
                        }
                    }
                    // Extract and sort by key descending (closest first)
                    let mut items: Vec<(u64, TrackId)> =
                        heap.into_iter().map(|(Reverse(k), id)| (k, id)).collect();
                    items.sort_by_key(|(k, _)| Reverse(*k));
                    items.into_iter().map(|(_, id)| id).collect()
                }
                Neighbour::Next => {
                    // Keep k smallest keys above cur_key using a max-heap
                    let mut heap: BinaryHeap<(u64, TrackId)> = BinaryHeap::new();
                    for s in ordered_tracks {
                        if s == center {
                            continue;
                        }
                        let k = shuffle_key(s, queue.shuffle_seed);
                        if k > cur_key {
                            heap.push((k, s.clone()));
                            if heap.len() > count {
                                heap.pop();
                            }
                        }
                    }
                    // Extract and sort by key ascending (closest first)
                    let mut items: Vec<(u64, TrackId)> = heap.into_iter().collect();
                    items.sort_by_key(|(k, _)| *k);
                    items.into_iter().map(|(_, id)| id).collect()
                }
            }
        }
        PlaybackMode::GroupShuffle => {
            let (current_group_idx, current_track_idx) = {
                let group_idx = track_to_group_index.get(center).copied();
                let track_idx = track_to_group_track_index.get(center).copied();

                let Some(group_idx) = group_idx else {
                    tracing::warn!("Center track {center} not found in group index map");
                    return vec![];
                };
                (group_idx, track_idx.unwrap_or(0))
            };

            if current_group_idx >= groups.len() {
                return vec![];
            }

            let current_group = &groups[current_group_idx];
            let mut result = Vec::new();
            let mut remaining = count;

            match dir {
                Neighbour::Next => {
                    // Try to get next track in current group first
                    if current_track_idx + 1 < current_group.tracks.len() {
                        result.push(current_group.tracks[current_track_idx + 1].clone());
                        remaining -= 1;
                    }

                    // If we need more tracks, get next groups using shuffle-like ordering
                    if remaining > 0 && groups.len() > 1 {
                        let next_groups = get_groups_shuffle_order_impl(
                            groups.len(),
                            current_group_idx,
                            queue.group_shuffle_seed,
                            remaining,
                            |k| k,                    // identity mapping
                            |k, cur_key| k > cur_key, // filter: keys above current
                        );
                        for next_group_idx in next_groups {
                            if next_group_idx < groups.len()
                                && !groups[next_group_idx].tracks.is_empty()
                            {
                                result.push(groups[next_group_idx].tracks[0].clone());
                                remaining -= 1;
                                if remaining == 0 {
                                    break;
                                }
                            }
                        }
                    }
                }
                Neighbour::Prev => {
                    // Try to get previous track in current group first
                    if current_track_idx > 0 {
                        result.push(current_group.tracks[current_track_idx - 1].clone());
                        remaining -= 1;
                    }

                    // If we need more tracks, get previous groups using shuffle-like ordering
                    if remaining > 0 && groups.len() > 1 {
                        let prev_groups = get_groups_shuffle_order_impl(
                            groups.len(),
                            current_group_idx,
                            queue.group_shuffle_seed,
                            remaining,
                            Reverse,                  // reverse mapping for descending order
                            |k, cur_key| k < cur_key, // filter: keys below current
                        );
                        for prev_group_idx in prev_groups {
                            if prev_group_idx < groups.len()
                                && !groups[prev_group_idx].tracks.is_empty()
                            {
                                let last_track_idx = groups[prev_group_idx].tracks.len() - 1;
                                result.push(groups[prev_group_idx].tracks[last_track_idx].clone());
                                remaining -= 1;
                                if remaining == 0 {
                                    break;
                                }
                            }
                        }
                    }
                }
            }

            result
        }
    }
}

// Common implementation for finding groups in shuffle order
fn get_groups_shuffle_order_impl<K: Ord + Copy>(
    total_groups: usize,
    current_group_idx: usize,
    seed: u64,
    count: usize,
    key_mapper: impl Fn(u64) -> K,
    key_filter: impl Fn(u64, u64) -> bool,
) -> Vec<usize> {
    if total_groups <= 1 {
        return vec![];
    }

    let cur_key = shuffle_key(current_group_idx, seed);

    // Build heap with mapped keys
    let mut heap: BinaryHeap<(K, usize)> = BinaryHeap::new();
    for group_idx in 0..total_groups {
        if group_idx == current_group_idx {
            continue;
        }
        let k = shuffle_key(group_idx, seed);
        if key_filter(k, cur_key) {
            heap.push((key_mapper(k), group_idx));
            if heap.len() > count {
                heap.pop();
            }
        }
    }

    // Extract and sort (heap order is already correct for our needs)
    let mut items: Vec<(K, usize)> = heap.into_iter().collect();
    items.sort_by_key(|(k, _)| *k);
    items.into_iter().map(|(_, group_idx)| group_idx).collect()
}

