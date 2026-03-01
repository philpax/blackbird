pub mod util;

pub use blackbird_state;
use blackbird_state::{AlbumId, CoverArtId, Track, TrackId};
pub use blackbird_subsonic as bs;
use smol_str::SmolStr;

use std::{
    sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard},
    time::Duration,
};

mod render;
pub use render::VisibleGroupSet;

mod playback_thread;
use playback_thread::{LogicToPlaybackMessage, PlaybackThread, TrackLoadMode};
pub use playback_thread::{PlaybackState, PlaybackToLogicMessage, PlaybackToLogicRx};

mod tokio_thread;
use tokio_thread::TokioThread;

pub(crate) mod queue;

mod app_state;
pub use app_state::{
    AppState, AppStateError, PlaybackMode, ScrobbleState, SortOrder, TrackAndPosition,
};

mod library;
pub use library::Library;

pub struct Logic {
    // N.B. `playback_thread` must be declared before `tokio_thread` so that it
    // drops first. `TokioThread` drop blocks while spawned tasks (which hold
    // `PlaybackThreadSendHandle` clones) complete; if that runs before
    // `PlaybackThread::Drop` sends `Shutdown`, audio keeps playing until the
    // runtime finishes shutting down.
    playback_thread: PlaybackThread,
    tokio_thread: TokioThread,

    playback_to_logic_rx: PlaybackToLogicRx,

    logic_request_tx: LogicRequestHandle,
    logic_request_rx: std::sync::mpsc::Receiver<LogicRequestMessage>,

    cover_art_loaded_tx: std::sync::mpsc::Sender<CoverArt>,
    lyrics_loaded_tx: std::sync::mpsc::Sender<LyricsData>,
    library_populated_tx: std::sync::mpsc::Sender<()>,

    /// Guards against duplicate in-flight lyrics requests for the same track.
    last_requested_lyrics_track: std::sync::Mutex<Option<TrackId>>,

    state: Arc<RwLock<AppState>>,
    client: Arc<bs::Client>,
    transcode: bool,
}
#[derive(Debug, Clone)]
pub enum LogicRequestMessage {
    PlayCurrent,
    PauseCurrent,
    ToggleCurrent,
    StopCurrent,
    Seek(Duration),
    SeekBy { seconds: i64 },
    Next,
    Previous,
    NextGroup,
    PreviousGroup,
}
#[derive(Clone)]
pub struct LogicRequestHandle(std::sync::mpsc::Sender<LogicRequestMessage>);
impl LogicRequestHandle {
    pub fn send(&self, message: LogicRequestMessage) {
        self.0.send(message).unwrap();
    }
}

#[derive(Debug, Clone)]
pub struct CoverArt {
    pub cover_art_id: CoverArtId,
    pub cover_art: Vec<u8>,
    /// The size that was requested from the server, or `None` for full resolution.
    pub requested_size: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct LyricsData {
    pub track_id: TrackId,
    pub lyrics: Option<bs::StructuredLyrics>,
}

#[derive(Debug, Clone)]
pub struct TrackDisplayDetails {
    pub album_id: AlbumId,
    pub album_name: SmolStr,
    pub album_artist: SmolStr,
    pub cover_art_id: Option<CoverArtId>,
    pub track_id: TrackId,
    pub track_title: SmolStr,
    pub track_artist: Option<SmolStr>,
    pub track_duration: Duration,
    pub track_position: Duration,
    pub show_time: bool,
    pub starred: bool,
}
impl TrackDisplayDetails {
    pub fn from_track_and_position(
        track_and_position: &TrackAndPosition,
        state: &AppState,
    ) -> Option<TrackDisplayDetails> {
        let track = state.library.track_map.get(&track_and_position.track_id)?;
        let album = state.library.albums.get(track.album_id.as_ref()?)?;
        Some(TrackDisplayDetails {
            album_id: album.id.clone(),
            album_name: album.name.clone(),
            album_artist: album.artist.clone(),
            cover_art_id: album.cover_art_id.clone(),
            track_id: track.id.clone(),
            track_title: track.title.clone(),
            track_artist: track.artist.clone(),
            track_duration: Duration::from_secs(track.duration.unwrap_or(1) as u64),
            track_position: track_and_position.position,
            show_time: true,
            starred: track.starred,
        })
    }

    /// Returns the artist name, or the album artist if the track artist is not set.
    pub fn artist(&self) -> &str {
        self.track_artist.as_deref().unwrap_or(&self.album_artist)
    }

    /// Sets whether to show the time in the string report.
    pub fn set_show_time(mut self, show_time: bool) -> Self {
        self.show_time = show_time;
        self
    }

    /// Assumes a position of 0
    pub fn from_track_id(track_id: &TrackId, state: &AppState) -> Option<TrackDisplayDetails> {
        TrackDisplayDetails::from_track_and_position(
            &TrackAndPosition {
                track_id: track_id.clone(),
                position: Duration::from_secs(0),
            },
            state,
        )
    }

    /// Returns a string representation of the track, including the album artist, track title, and duration,
    /// or the track ID if no information is available.
    pub fn string_report_without_time(track_id: &TrackId, state: &AppState) -> String {
        TrackDisplayDetails::from_track_id(track_id, state)
            .map(|i| i.set_show_time(false).to_string())
            .unwrap_or_else(|| format!("unknown track {track_id}"))
    }
}
impl std::fmt::Display for TrackDisplayDetails {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let artist = self.artist();
        write!(f, "{} - {}", artist, self.track_title)?;
        if artist != self.album_artist {
            write!(f, " ({})", self.album_artist)?;
        }
        if self.show_time {
            write!(
                f,
                " [{}/{}]",
                util::seconds_to_hms_string(self.track_position.as_secs() as u32, false),
                util::seconds_to_hms_string(self.track_duration.as_secs() as u32, false)
            )?;
        }
        Ok(())
    }
}

pub struct LogicArgs {
    pub base_url: String,
    pub username: String,
    pub password: String,
    pub transcode: bool,
    pub volume: f32,
    pub sort_order: SortOrder,
    pub playback_mode: PlaybackMode,
    pub last_playback: Option<(TrackId, Duration)>,
    pub cover_art_loaded_tx: std::sync::mpsc::Sender<CoverArt>,
    pub lyrics_loaded_tx: std::sync::mpsc::Sender<LyricsData>,
    pub library_populated_tx: std::sync::mpsc::Sender<()>,
}

impl Logic {
    pub fn new(
        LogicArgs {
            base_url,
            username,
            password,
            transcode,
            volume,
            sort_order,
            playback_mode,
            last_playback,
            cover_art_loaded_tx,
            lyrics_loaded_tx,
            library_populated_tx,
        }: LogicArgs,
    ) -> Self {
        let state = Arc::new(RwLock::new(AppState {
            volume,
            sort_order,
            playback_mode,
            ..AppState::default()
        }));
        let client = Arc::new(bs::Client::new(
            base_url,
            username,
            password,
            "blackbird".to_string(),
        ));

        let tokio_thread = TokioThread::new();
        let playback_thread = PlaybackThread::new(volume);
        let playback_to_logic_rx = playback_thread.subscribe();

        let (logic_request_tx, logic_request_rx) =
            std::sync::mpsc::channel::<LogicRequestMessage>();

        // Set the scroll target to the last played track so the UI scrolls to it.
        if let Some((ref track_id, _)) = last_playback {
            state.write().unwrap().last_requested_track_for_ui_scroll = Some(track_id.clone());
        }

        let logic = Logic {
            tokio_thread,

            playback_thread,
            playback_to_logic_rx,

            logic_request_tx: LogicRequestHandle(logic_request_tx),
            logic_request_rx,

            cover_art_loaded_tx,
            lyrics_loaded_tx,
            library_populated_tx,

            last_requested_lyrics_track: std::sync::Mutex::new(None),

            state,
            client,
            transcode,
        };
        logic.initial_fetch(last_playback);
        logic
    }

    pub fn update(&mut self) {
        while let Ok(event) = self.playback_to_logic_rx.try_recv() {
            match event {
                PlaybackToLogicMessage::TrackStarted(track_and_position) => {
                    tracing::debug!(
                        "TrackStarted: {}",
                        TrackDisplayDetails::string_report_without_time(
                            &track_and_position.track_id,
                            &self.state.read().unwrap(),
                        )
                    );
                    self.ensure_cache_window();

                    let mut st = self.write_state();
                    st.current_track_and_position = Some(track_and_position.clone());
                    st.started_loading_track = None;

                    // Sync current_target with the actual current track.
                    // This is important for detecting pending track changes in gapless logic.
                    st.queue.current_target = Some(track_and_position.track_id.clone());

                    // Advance current_index if this was a gapless transition (the
                    // playback thread moved to the next track without going through
                    // schedule_next_track, so the index is stale).
                    let ordered = &st.queue.ordered_tracks;
                    if !ordered.is_empty() {
                        let next_index = (st.queue.current_index + 1) % ordered.len();
                        if ordered[next_index] == track_and_position.track_id {
                            st.queue.current_index = next_index;
                        }
                    }

                    // Reset next track append tracking for gapless playback.
                    st.queue.next_track_appended = None;

                    // Reset scrobble state for new track
                    st.scrobble_state = ScrobbleState {
                        track_id: Some(track_and_position.track_id.clone()),
                        has_scrobbled: false,
                        accumulated_listening_time: Duration::ZERO,
                        last_position: Duration::ZERO,
                    };
                    tracing::debug!(
                        "Scrobble state reset for track: {}",
                        track_and_position.track_id.0
                    );
                }
                PlaybackToLogicMessage::PositionChanged(track_and_duration) => {
                    self.write_state().current_track_and_position =
                        Some(track_and_duration.clone());
                    self.update_scrobble_state(&track_and_duration);
                }
                PlaybackToLogicMessage::TrackEnded => {
                    tracing::debug!("TrackEnded: scheduling advance to next track");
                    self.handle_track_end_advance();
                }
                PlaybackToLogicMessage::FailedToPlayTrack(track_id, error) => {
                    tracing::error!(
                        "Failed to play track `{}`: {error}",
                        TrackDisplayDetails::string_report_without_time(
                            &track_id,
                            &self.state.read().unwrap()
                        )
                    );
                    self.write_state().error =
                        Some(AppStateError::DecodeTrackFailed { track_id, error });
                    self.schedule_next_track();
                }
                PlaybackToLogicMessage::PlaybackStateChanged(s) => {
                    self.write_state().playback_state = s;
                }
            }
        }

        // Handle deferred auto-skip after load error
        let should_skip = self.read_state().queue.pending_skip_after_error;
        if should_skip {
            self.schedule_next_track();
            self.write_state().queue.pending_skip_after_error = false;
        }

        while let Ok(event) = self.logic_request_rx.try_recv() {
            match event {
                LogicRequestMessage::PlayCurrent => self.play_current(),
                LogicRequestMessage::PauseCurrent => self.pause_current(),
                LogicRequestMessage::ToggleCurrent => self.toggle_current(),
                LogicRequestMessage::StopCurrent => self.stop_current(),
                LogicRequestMessage::Seek(duration) => self.seek_current(duration),
                LogicRequestMessage::SeekBy { seconds } => {
                    let Some(playing_info) = self.get_track_display_details() else {
                        continue;
                    };
                    let current_position = playing_info.track_position;
                    let duration = Duration::from_secs(seconds.unsigned_abs());
                    self.seek_current(if seconds > 0 {
                        current_position + duration
                    } else {
                        current_position.saturating_sub(duration)
                    });
                }
                LogicRequestMessage::Next => {
                    tracing::debug!("User requested Next");
                    self.next()
                }
                LogicRequestMessage::Previous => {
                    tracing::debug!("User requested Previous");
                    self.previous()
                }
                LogicRequestMessage::NextGroup => {
                    tracing::debug!("User requested NextGroup");
                    self.next_group()
                }
                LogicRequestMessage::PreviousGroup => {
                    tracing::debug!("User requested PreviousGroup");
                    self.previous_group()
                }
            }
        }

        // Gapless playback: Try to append next track if available
        // Only do this if there's no pending track change (i.e., current_target matches current track)
        if let Some(current_id) = self.get_playing_track_id() {
            let pending_track_change = {
                let st = self.read_state();
                st.queue.current_target.as_ref() != Some(&current_id)
            };

            // Don't append if we're in the middle of changing tracks
            if !pending_track_change && let Some(next_id) = self.compute_next_track_id() {
                let (already_appended, audio_data) = {
                    let st = self.read_state();
                    (
                        st.queue.next_track_appended.as_ref() == Some(&next_id),
                        st.queue.audio_cache.get(&next_id).cloned(),
                    )
                };

                if !already_appended && let Some(data) = audio_data {
                    tracing::debug!("Appending next track for gapless playback: {}", next_id.0);
                    self.playback_thread
                        .send(LogicToPlaybackMessage::AppendNextTrack(
                            next_id.clone(),
                            data,
                        ));
                    self.write_state().queue.next_track_appended = Some(next_id);
                }
            }
        }
    }
}
impl Logic {
    pub fn play_current(&self) {
        self.playback_thread.send(LogicToPlaybackMessage::Play);
    }

    pub fn pause_current(&self) {
        self.playback_thread.send(LogicToPlaybackMessage::Pause);
    }

    pub fn toggle_current(&self) {
        self.playback_thread
            .send(LogicToPlaybackMessage::TogglePlayback);
    }

    pub fn stop_current(&self) {
        self.playback_thread
            .send(LogicToPlaybackMessage::StopPlayback);
    }

    pub fn seek_current(&self, position: Duration) {
        self.playback_thread
            .send(LogicToPlaybackMessage::Seek(position));
    }

    pub fn next(&self) {
        self.schedule_next_track();
    }

    pub fn previous(&self) {
        // If we're past 5 seconds of the track, seek to the start instead of going to previous track
        // I bristle a little at pulling in unnecessary information just to get the position / duration,
        // but this is getting called once every few minutes, so I'll get over it.
        if let Some(details) = self.get_track_display_details()
            && details.track_position > Duration::from_secs(5)
        {
            self.seek_current(Duration::from_secs(0));
            return;
        }
        self.schedule_previous_track();
    }

    pub fn next_group(&self) {
        self.schedule_next_group();
    }

    pub fn previous_group(&self) {
        self.schedule_previous_group();
    }
}
impl Logic {
    pub fn request_handle(&self) -> LogicRequestHandle {
        self.logic_request_tx.clone()
    }
    pub fn subscribe_to_playback_events(&self) -> PlaybackToLogicRx {
        self.playback_thread.subscribe()
    }
}
impl Logic {
    pub fn request_cover_art(&self, cover_art_id: &CoverArtId, size: Option<usize>) {
        let client = self.client.clone();
        let state = self.state.clone();
        let cover_art_id = cover_art_id.clone();
        let cover_art_loaded_tx = self.cover_art_loaded_tx.clone();
        self.tokio_thread.spawn(async move {
            match client.get_cover_art(cover_art_id.0.as_str(), size).await {
                Ok(cover_art) => {
                    cover_art_loaded_tx
                        .send(CoverArt {
                            cover_art_id: cover_art_id.clone(),
                            cover_art,
                            requested_size: size,
                        })
                        .unwrap();
                }
                Err(e) => {
                    let mut state = state.write().unwrap();
                    state.error = Some(AppStateError::CoverArtFetchFailed {
                        cover_art_id: cover_art_id.clone(),
                        error: e.to_string(),
                    });
                }
            }
        });
    }

    pub fn set_track_starred(&self, track_id: &TrackId, starred: bool) {
        let client = self.client.clone();
        let state = self.state.clone();
        let track_id = track_id.clone();

        self.tokio_thread.spawn(async move {
            // Immediately update the track in the UI to avoid latency, and assume
            // the server will confirm the operation.
            let old_starred = {
                let mut st = state.write().unwrap();
                let old = st.library.set_track_starred(&track_id, starred);
                // Recompute the queue if the current mode depends on liked status.
                if matches!(
                    st.playback_mode,
                    PlaybackMode::LikedShuffle | PlaybackMode::LikedGroupShuffle
                ) {
                    queue::recompute_queue_on_state(&mut st, None);
                }
                old
            };

            let operation = if starred {
                client.star([track_id.0.clone()], [], []).await
            } else {
                client.unstar([track_id.0.clone()], [], []).await
            };

            let Err(e) = operation else {
                return;
            };

            let track_id = track_id.clone();
            let error = e.to_string();

            if let Some(old_starred) = old_starred {
                state
                    .write()
                    .unwrap()
                    .library
                    .set_track_starred(&track_id, old_starred);
            }

            state.write().unwrap().error = Some(if starred {
                AppStateError::StarTrackFailed { track_id, error }
            } else {
                AppStateError::UnstarTrackFailed { track_id, error }
            });
        });
    }

    pub fn set_album_starred(&self, album_id: &AlbumId, starred: bool) {
        let client = self.client.clone();
        let state = self.state.clone();
        let album_id = album_id.clone();

        self.tokio_thread.spawn(async move {
            // Immediately update the album in the UI to avoid latency, and assume
            // the server will confirm the operation.
            let old_starred = {
                let mut st = state.write().unwrap();
                let old = st.library.set_album_starred(&album_id, starred);
                // Recompute the queue if the current mode depends on liked status.
                if matches!(
                    st.playback_mode,
                    PlaybackMode::LikedShuffle | PlaybackMode::LikedGroupShuffle
                ) {
                    queue::recompute_queue_on_state(&mut st, None);
                }
                old
            };
            let operation = if starred {
                client.star([], [album_id.0.to_string()], []).await
            } else {
                client.unstar([], [album_id.0.to_string()], []).await
            };

            let Err(e) = operation else {
                return;
            };

            let album_id = album_id.clone();
            let error = e.to_string();

            if let Some(old_starred) = old_starred {
                state
                    .write()
                    .unwrap()
                    .library
                    .set_album_starred(&album_id, old_starred);
            }

            state.write().unwrap().error = Some(if starred {
                AppStateError::StarAlbumFailed { album_id, error }
            } else {
                AppStateError::UnstarAlbumFailed { album_id, error }
            });
        });
    }

    pub fn request_lyrics(&self, track_id: &TrackId) {
        // Skip if we already have an in-flight request for this track.
        {
            let mut last = self.last_requested_lyrics_track.lock().unwrap();
            if last.as_ref() == Some(track_id) {
                return;
            }
            *last = Some(track_id.clone());
        }

        let client = self.client.clone();
        let track_id = track_id.clone();
        let lyrics_loaded_tx = self.lyrics_loaded_tx.clone();

        self.tokio_thread.spawn(async move {
            match client.get_lyrics_by_song_id(&track_id.0).await {
                Ok(mut lyrics_list) => {
                    // Get the first synced lyrics if available, otherwise first lyrics
                    let lyrics = {
                        let synced_idx =
                            lyrics_list.structured_lyrics.iter().position(|l| l.synced);

                        if let Some(idx) = synced_idx {
                            Some(lyrics_list.structured_lyrics.swap_remove(idx))
                        } else {
                            lyrics_list.structured_lyrics.into_iter().next()
                        }
                    };

                    lyrics_loaded_tx
                        .send(LyricsData {
                            track_id: track_id.clone(),
                            lyrics,
                        })
                        .unwrap();
                }
                Err(e) => {
                    tracing::debug!("Failed to fetch lyrics for track {}: {}", track_id.0, e);
                    // Send None to indicate no lyrics available
                    lyrics_loaded_tx
                        .send(LyricsData {
                            track_id: track_id.clone(),
                            lyrics: None,
                        })
                        .unwrap();
                }
            }
        });
    }
}
impl Logic {
    pub fn get_playing_track_and_position(&self) -> Option<TrackAndPosition> {
        self.read_state().current_track_and_position.clone()
    }

    pub fn get_playing_track_id(&self) -> Option<TrackId> {
        self.read_state()
            .current_track_and_position
            .as_ref()
            .map(|tp| tp.track_id.clone())
    }

    pub fn get_playing_position(&self) -> Option<Duration> {
        self.read_state()
            .current_track_and_position
            .as_ref()
            .map(|tp| tp.position)
    }

    pub fn is_track_loaded(&self) -> bool {
        self.read_state().current_track_and_position.is_some()
    }
    pub fn should_show_loading_indicator(&self) -> bool {
        self.read_state()
            .started_loading_track
            .is_some_and(|t| t.elapsed() > Duration::from_millis(100))
    }
    pub fn has_loaded_all_tracks(&self) -> bool {
        self.read_state().library.has_loaded_all_tracks
    }

    pub fn get_track_display_details(&self) -> Option<TrackDisplayDetails> {
        let track_and_position = self.read_state().current_track_and_position.clone()?;
        TrackDisplayDetails::from_track_and_position(
            &track_and_position,
            &self.state.read().unwrap(),
        )
    }

    pub fn get_error(&self) -> Option<AppStateError> {
        self.read_state().error.clone()
    }
    pub fn clear_error(&self) {
        self.write_state().error = None;
    }

    pub fn get_state(&self) -> Arc<RwLock<AppState>> {
        self.state.clone()
    }

    pub fn set_playback_mode(&self, mode: PlaybackMode) {
        tracing::debug!("Playback mode set to {mode:?}");
        let current_track_id = {
            let mut st = self.write_state();
            st.playback_mode = mode;

            // Reset gapless playback state since the next track may be different in the new mode
            st.queue.next_track_appended = None;

            st.current_track_and_position
                .as_ref()
                .map(|t| t.track_id.clone())
        };

        // Clear any queued next track by marking it for skip, so the new mode takes effect immediately.
        // The marked track will be skipped when it transitions to current, triggering playback based on new mode.
        self.playback_thread
            .send(LogicToPlaybackMessage::ClearQueuedNextTracks);

        self.recompute_queue(current_track_id.as_ref());

        if current_track_id.is_some() {
            self.ensure_cache_window();
        }
    }

    pub fn get_playback_state(&self) -> PlaybackState {
        self.read_state().playback_state
    }

    pub fn get_playback_mode(&self) -> PlaybackMode {
        self.read_state().playback_mode
    }

    pub fn set_sort_order(&self, order: SortOrder) {
        tracing::debug!("Sort order set to {order:?}");
        let current_track = {
            let mut st = self.write_state();
            st.sort_order = order;
            st.library.resort(order);
            st.current_track_and_position
                .as_ref()
                .map(|t| t.track_id.clone())
        };
        self.recompute_queue(current_track.as_ref());
    }

    pub fn get_sort_order(&self) -> SortOrder {
        self.read_state().sort_order
    }

    pub fn get_volume(&self) -> f32 {
        self.read_state().volume
    }

    pub fn set_volume(&self, volume: f32) {
        self.write_state().volume = volume;
        self.playback_thread
            .send(LogicToPlaybackMessage::SetVolume(volume));
    }

    /// Get cover art IDs for albums surrounding (and including) the next track in the queue.
    /// Returns an empty vector if there is no next track or if the library is not populated.
    pub fn get_next_track_surrounding_cover_art_ids(&self) -> Vec<CoverArtId> {
        let st = self.read_state();

        // Get the next track ID
        let Some(next_track_id) = self.compute_next_track_id() else {
            return vec![];
        };

        // Find the group index for the next track
        let Some(&next_group_idx) = st.library.track_to_group_index.get(&next_track_id) else {
            return vec![];
        };

        let mut cover_art_ids = vec![];
        let groups = &st.library.groups;

        // We would ostensibly include the groups before and after the next track's group here,
        // but the naive implementation doesn't work, and I have no interest in debugging it today.

        // Get the next track's group (center)
        if let Some(cover_art_id) = &groups[next_group_idx].cover_art_id {
            cover_art_ids.push(cover_art_id.clone());
        }

        cover_art_ids
    }

    pub fn set_scroll_target(&self, track_id: &TrackId) {
        self.write_state().last_requested_track_for_ui_scroll = Some(track_id.clone());
    }

    pub fn should_shutdown(&self) -> bool {
        self.tokio_thread.should_shutdown()
    }
}
impl Logic {
    pub fn request_play_track(&self, track_id: &TrackId) {
        // Public API used by UI: keep current playing until new track is ready.
        self.schedule_play_track(track_id);
        self.recompute_queue(Some(track_id));
    }

    /// Updates the scrobble state based on current playback position.
    /// Scrobbles the track when criteria are met:
    /// - Minimum 10 seconds of listening time
    /// - Either 30 seconds OR 50% of track duration (whichever comes first)
    fn update_scrobble_state(&self, track_and_position: &TrackAndPosition) {
        let mut state = self.write_state();

        // Get track duration first (before taking mutable borrow)
        let Some(track_duration) = state
            .library
            .track_map
            .get(&track_and_position.track_id)
            .and_then(|track| track.duration)
            .map(|duration| Duration::from_secs(duration as u64))
        else {
            return;
        };

        let scrobble_state = &mut state.scrobble_state;

        // Ensure we're tracking the correct track
        if scrobble_state.track_id.as_ref() != Some(&track_and_position.track_id) {
            tracing::debug!(
                "Scrobble state track mismatch: expected {:?}, got {}",
                scrobble_state.track_id,
                track_and_position.track_id.0
            );
            return;
        }

        // If already scrobbled, nothing to do
        if scrobble_state.has_scrobbled {
            return;
        }

        let current_position = track_and_position.position;
        let last_position = scrobble_state.last_position;

        // Update accumulated listening time
        // If the position moved forward naturally (not a seek backward), add the difference
        if current_position >= last_position {
            let delta = current_position - last_position;
            scrobble_state.accumulated_listening_time += delta;
            tracing::trace!(
                "Scrobble: position advanced +{:.1}s, accumulated: {:.1}s",
                delta.as_secs_f32(),
                scrobble_state.accumulated_listening_time.as_secs_f32()
            );
        } else {
            tracing::debug!(
                "Scrobble: seek backward detected ({:.1}s -> {:.1}s), accumulated time unchanged: {:.1}s",
                last_position.as_secs_f32(),
                current_position.as_secs_f32(),
                scrobble_state.accumulated_listening_time.as_secs_f32()
            );
        }
        scrobble_state.last_position = current_position;

        let accumulated_time = scrobble_state.accumulated_listening_time;

        // Check scrobble criteria:
        // 1. Minimum 10 seconds of listening
        const MIN_LISTENING_TIME: Duration = Duration::from_secs(10);
        if accumulated_time < MIN_LISTENING_TIME {
            tracing::trace!(
                "Scrobble: minimum listening time not met ({:.1}s / {:.1}s)",
                accumulated_time.as_secs_f32(),
                MIN_LISTENING_TIME.as_secs_f32()
            );
            return;
        }

        // 2. Either 30 seconds OR 50% of track (whichever comes first)
        const SCROBBLE_TIME_THRESHOLD: Duration = Duration::from_secs(30);
        let half_duration = track_duration / 2;
        let scrobble_threshold = SCROBBLE_TIME_THRESHOLD.min(half_duration);

        tracing::debug!(
            "Scrobble: checking threshold - accumulated: {:.1}s, threshold: {:.1}s (50% of {:.1}s)",
            accumulated_time.as_secs_f32(),
            scrobble_threshold.as_secs_f32(),
            track_duration.as_secs_f32()
        );

        if accumulated_time >= scrobble_threshold {
            // Mark as scrobbled immediately to prevent duplicate scrobbles
            scrobble_state.has_scrobbled = true;

            // Get current timestamp in milliseconds since epoch
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;

            tracing::info!(
                "Scrobbling track: {} (listened: {:.1}s / {:.1}s)",
                track_and_position.track_id.0,
                accumulated_time.as_secs_f32(),
                track_duration.as_secs_f32()
            );

            // Make async API call
            self.tokio_thread.spawn({
                let client = self.client.clone();
                let state = self.state.clone();
                let track_id = track_and_position.track_id.clone();

                async move {
                    if let Err(e) = client
                        .scrobble(&track_id.0, Some(timestamp), Some(true))
                        .await
                    {
                        tracing::error!("Failed to scrobble track {}: {}", track_id.0, e);
                        // Note: We don't update the UI error state for scrobble failures
                        // as they're not critical to the user experience
                    }

                    // Reload track from API to update play count
                    match client.get_song(&track_id.0).await {
                        Ok(child) => {
                            let updated_track: Track = child.into();
                            if let Ok(mut state) = state.write() {
                                state
                                    .library
                                    .track_map
                                    .insert(track_id.clone(), updated_track);
                                tracing::debug!(
                                    "Updated track {} from API after scrobble",
                                    track_id.0
                                );
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Failed to reload track {} to update play count: {}",
                                track_id.0,
                                e
                            );
                        }
                    }
                }
            });
        }
    }
}
impl Logic {
    fn initial_fetch(&self, restore_track: Option<(TrackId, Duration)>) {
        let client = self.client.clone();
        let state = self.state.clone();
        let library_populated_tx = self.library_populated_tx.clone();
        let playback_tx = self.playback_thread.send_handle();
        let transcode = self.transcode;
        self.tokio_thread.spawn(async move {
            let future = {
                let client = client.clone();
                let state = state.clone();
                let library_populated_tx = library_populated_tx.clone();
                async move {
                    client.ping().await?;

                    let result = blackbird_state::fetch_all(&client, |batch_count, total_count| {
                        tracing::info!("Fetched {batch_count} tracks, total {total_count} tracks");
                    })
                    .await?;

                    let req_id;
                    {
                        let mut st = state.write().unwrap();
                        let sort_order = st.sort_order;
                        st.library.populate(
                            result.track_ids,
                            result.track_map,
                            result.groups,
                            result.albums,
                            sort_order,
                        );

                        // If restoring a track, recompute the queue with it as current
                        // so that the queue index is correct.
                        let restore_id = restore_track
                            .as_ref()
                            .filter(|(tid, _)| st.library.track_map.contains_key(tid))
                            .map(|(tid, _)| tid);
                        queue::recompute_queue_on_state(&mut st, restore_id);

                        if let Some(tid) = restore_id {
                            st.queue.current_target = Some(tid.clone());
                            st.queue.request_counter = st.queue.request_counter.wrapping_add(1);
                        }

                        req_id = st.queue.request_counter;
                    }

                    // Signal that library population is complete.
                    let _ = library_populated_tx.send(());

                    // Restore the last track in a paused state.
                    if let Some((track_id, position)) = restore_track.filter(|(tid, _)| {
                        state.read().unwrap().library.track_map.contains_key(tid)
                    }) {
                        tracing::info!(
                            "Restoring last track {} at {:.1}s",
                            track_id.0,
                            position.as_secs_f64()
                        );
                        let response = client
                            .stream(&track_id.0, transcode.then(|| "mp3".to_string()), None)
                            .await;
                        queue::handle_load_response(
                            response,
                            state,
                            playback_tx,
                            track_id,
                            req_id,
                            queue::TrackLoadBehavior::Paused(position),
                        );
                    }

                    bs::ClientResult::Ok(())
                }
            };

            if let Err(error) = future.await {
                state.write().unwrap().error = Some(AppStateError::InitialFetchFailed {
                    error: error.to_string(),
                });
            }
        })
    }

    fn write_state(&'_ self) -> RwLockWriteGuard<'_, AppState> {
        self.state.write().unwrap()
    }

    fn read_state(&'_ self) -> RwLockReadGuard<'_, AppState> {
        self.state.read().unwrap()
    }
}
