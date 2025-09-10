use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap},
    hash::{Hash, Hasher},
    sync::{Arc, RwLock},
    time::{SystemTime, UNIX_EPOCH},
};

use blackbird_state::TrackId;
use blackbird_subsonic::ClientResult;

use crate::{
    AppState, Logic, PlaybackMode,
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
}

impl QueueState {
    pub fn new() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        QueueState {
            shuffle_seed: next_seed((now.as_secs() << 32) ^ (now.subsec_nanos() as u64)),
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
                        st.error = Some(e.to_string());
                        st.queue.pending_skip_after_error = true;
                        tracing::warn!(
                            "Load error for current target {track_id} (req_id={request_id}): {}",
                            st.error.as_deref().unwrap_or("")
                        );
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
        compute_neighbor(
            &st.track_ids,
            &st.current_track_and_position.as_ref()?.track_id,
            st.playback_mode,
            st.queue.shuffle_seed,
            Neighbor::Next,
        )
    }

    pub(super) fn compute_previous_track_id(&self) -> Option<TrackId> {
        let st = self.read_state();
        compute_neighbor(
            &st.track_ids,
            &st.current_track_and_position.as_ref()?.track_id,
            st.playback_mode,
            st.queue.shuffle_seed,
            Neighbor::Prev,
        )
    }

    pub(super) fn ensure_cache_window(&self, center: &TrackId) {
        let window = {
            let st = self.read_state();
            compute_window(
                &st.track_ids,
                center,
                st.playback_mode,
                st.queue.shuffle_seed,
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

// Deterministic shuffle key based on seed and TrackId
fn shuffle_key(track_id: &TrackId, seed: u64) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    track_id.0.hash(&mut hasher);
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

fn compute_window(
    ordered_tracks: &[TrackId],
    center: &TrackId,
    mode: PlaybackMode,
    seed: u64,
    radius: usize,
) -> Vec<TrackId> {
    let mut out = Vec::with_capacity(1 + radius * 2);
    out.push(center.clone());

    // Collect neighbors in each direction in a single pass when possible
    let prevs = compute_neighbors(ordered_tracks, center, mode, seed, Neighbor::Prev, radius);
    out.extend(prevs);

    let nexts = compute_neighbors(ordered_tracks, center, mode, seed, Neighbor::Next, radius);
    out.extend(nexts);

    out
}

#[derive(Clone, Copy)]
enum Neighbor {
    Prev,
    Next,
}
fn compute_neighbor(
    ordered_tracks: &[TrackId],
    center: &TrackId,
    mode: PlaybackMode,
    seed: u64,
    dir: Neighbor,
) -> Option<TrackId> {
    compute_neighbors(ordered_tracks, center, mode, seed, dir, 1)
        .first()
        .cloned()
}
fn compute_neighbors(
    ordered_tracks: &[TrackId],
    center: &TrackId,
    mode: PlaybackMode,
    seed: u64,
    dir: Neighbor,
    count: usize,
) -> Vec<TrackId> {
    match mode {
        PlaybackMode::RepeatOne => Vec::new(),
        PlaybackMode::Sequential => {
            let Some(idx) = ordered_tracks.iter().position(|s| s == center) else {
                return vec![];
            };
            match dir {
                Neighbor::Prev => {
                    let mut v = Vec::new();
                    let mut remaining = count;
                    let mut current_idx = idx;

                    // Wrap around from the end if we need more tracks
                    while remaining > 0 && !ordered_tracks.is_empty() {
                        current_idx = if current_idx == 0 {
                            ordered_tracks.len() - 1
                        } else {
                            current_idx - 1
                        };

                        // Don't include the center track itself
                        if current_idx != idx {
                            v.push(ordered_tracks[current_idx].clone());
                            remaining -= 1;
                        }

                        // Prevent infinite loop if there's only one track
                        if current_idx == idx && remaining > 0 {
                            break;
                        }
                    }
                    v
                }
                Neighbor::Next => {
                    let mut v = Vec::new();
                    let mut remaining = count;
                    let mut current_idx = idx;

                    // Wrap around from the beginning if we need more tracks
                    while remaining > 0 && !ordered_tracks.is_empty() {
                        current_idx = (current_idx + 1) % ordered_tracks.len();

                        // Don't include the center track itself
                        if current_idx != idx {
                            v.push(ordered_tracks[current_idx].clone());
                            remaining -= 1;
                        }

                        // Prevent infinite loop if there's only one track
                        if current_idx == idx && remaining > 0 {
                            break;
                        }
                    }
                    v
                }
            }
        }
        PlaybackMode::Shuffle => {
            let cur_key = shuffle_key(center, seed);
            match dir {
                Neighbor::Prev => {
                    // Keep k largest keys below cur_key using a min-heap (via Reverse)
                    let mut heap: BinaryHeap<(Reverse<u64>, TrackId)> = BinaryHeap::new();
                    for s in ordered_tracks {
                        if s == center {
                            continue;
                        }
                        let k = shuffle_key(s, seed);
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
                Neighbor::Next => {
                    // Keep k smallest keys above cur_key using a max-heap
                    let mut heap: BinaryHeap<(u64, TrackId)> = BinaryHeap::new();
                    for s in ordered_tracks {
                        if s == center {
                            continue;
                        }
                        let k = shuffle_key(s, seed);
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
    }
}
