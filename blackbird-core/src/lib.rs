pub mod util;

pub use blackbird_state;
use blackbird_state::{AlbumId, TrackId};
pub use blackbird_subsonic as bs;

use std::{
    sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard},
    time::Duration,
};

mod render;
pub use render::VisibleGroupSet;

mod playback_thread;
use playback_thread::{LogicToPlaybackMessage, PlaybackThread};
pub use playback_thread::{PlaybackState, PlaybackToLogicMessage, PlaybackToLogicRx};

mod tokio_thread;
use tokio_thread::TokioThread;

mod queue;

mod app_state;
pub use app_state::{AppState, AppStateError, PlaybackMode, TrackAndPosition};

mod library;
pub use library::Library;

pub struct Logic {
    tokio_thread: TokioThread,

    playback_thread: PlaybackThread,
    playback_to_logic_rx: PlaybackToLogicRx,

    logic_request_tx: LogicRequestHandle,
    logic_request_rx: std::sync::mpsc::Receiver<LogicRequestMessage>,

    cover_art_loaded_tx: std::sync::mpsc::Sender<CoverArt>,

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
    pub cover_art_id: String,
    pub cover_art: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct TrackDisplayDetails {
    pub album_id: AlbumId,
    pub album_name: String,
    pub album_artist: String,
    pub cover_art_id: Option<String>,
    pub track_id: TrackId,
    pub track_title: String,
    pub track_artist: Option<String>,
    pub track_duration: Duration,
    pub track_position: Duration,
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
        })
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
    pub fn string_report(track_id: &TrackId, state: &AppState) -> String {
        TrackDisplayDetails::from_track_id(track_id, state)
            .map(|i| i.to_string())
            .unwrap_or_else(|| format!("unknown track {track_id}"))
    }
}
impl std::fmt::Display for TrackDisplayDetails {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let artist = self.track_artist.as_ref().unwrap_or(&self.album_artist);
        write!(f, "{} - {}", artist, self.track_title)?;
        if artist != &self.album_artist {
            write!(f, " ({})", self.album_artist)?;
        }
        write!(
            f,
            " [{}/{}]",
            util::seconds_to_hms_string(self.track_position.as_secs() as u32, false),
            util::seconds_to_hms_string(self.track_duration.as_secs() as u32, false)
        )?;
        Ok(())
    }
}

pub struct LogicArgs {
    pub base_url: String,
    pub username: String,
    pub password: String,
    pub transcode: bool,
    pub volume: f32,
    pub cover_art_loaded_tx: std::sync::mpsc::Sender<CoverArt>,
}

impl Logic {
    pub fn new(
        LogicArgs {
            base_url,
            username,
            password,
            transcode,
            volume,
            cover_art_loaded_tx,
        }: LogicArgs,
    ) -> Self {
        let state = Arc::new(RwLock::new(AppState {
            volume,
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

        let logic = Logic {
            tokio_thread,

            playback_thread,
            playback_to_logic_rx,

            logic_request_tx: LogicRequestHandle(logic_request_tx),
            logic_request_rx,

            cover_art_loaded_tx,

            state,
            client,
            transcode,
        };
        logic.initial_fetch();
        logic
    }

    pub fn update(&mut self) {
        while let Ok(event) = self.playback_to_logic_rx.try_recv() {
            match event {
                PlaybackToLogicMessage::TrackStarted(track_and_position) => {
                    tracing::debug!(
                        "TrackStarted: {}",
                        TrackDisplayDetails::string_report(
                            &track_and_position.track_id,
                            &self.state.read().unwrap()
                        )
                    );
                    self.ensure_cache_window(&track_and_position.track_id);

                    let mut st = self.write_state();
                    st.current_track_and_position = Some(track_and_position);
                    st.started_loading_track = None;
                }
                PlaybackToLogicMessage::PositionChanged(track_and_duration) => {
                    self.write_state().current_track_and_position = Some(track_and_duration);
                }
                PlaybackToLogicMessage::TrackEnded => {
                    tracing::debug!("TrackEnded: scheduling advance to next track");
                    self.handle_track_end_advance();
                }
                PlaybackToLogicMessage::FailedToPlayTrack(track_id, error) => {
                    tracing::error!(
                        "Failed to play track `{}`: {error}",
                        TrackDisplayDetails::string_report(&track_id, &self.state.read().unwrap())
                    );
                    self.write_state().error =
                        Some(AppStateError::DecodeTrackFailed { track_id, error });
                    self.schedule_next_track();
                }
                PlaybackToLogicMessage::PlaybackStateChanged(_s) => {}
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
        self.schedule_previous_track();
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
    pub fn request_cover_art(&self, cover_art_id: &str, size: Option<usize>) {
        let client = self.client.clone();
        let state = self.state.clone();
        let cover_art_id = cover_art_id.to_string();
        let cover_art_loaded_tx = self.cover_art_loaded_tx.clone();
        self.tokio_thread.spawn(async move {
            match client.get_cover_art(&cover_art_id, size).await {
                Ok(cover_art) => {
                    cover_art_loaded_tx
                        .send(CoverArt {
                            cover_art_id: cover_art_id.clone(),
                            cover_art,
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
            let old_starred = state
                .write()
                .unwrap()
                .library
                .set_track_starred(&track_id, starred);

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
            let old_starred = state
                .write()
                .unwrap()
                .library
                .set_album_starred(&album_id, starred);
            let operation = if starred {
                client.star([], [album_id.0.clone()], []).await
            } else {
                client.unstar([], [album_id.0.clone()], []).await
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
}
impl Logic {
    pub fn get_playing_track_id(&self) -> Option<TrackId> {
        self.read_state()
            .current_track_and_position
            .as_ref()
            .map(|tp| tp.track_id.clone())
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
        self.write_state().playback_mode = mode;

        if let Some(track_id) = self.get_playing_track_id() {
            self.ensure_cache_window(&track_id);
        }
    }

    pub fn get_playback_mode(&self) -> PlaybackMode {
        self.read_state().playback_mode
    }

    pub fn get_volume(&self) -> f32 {
        self.read_state().volume
    }

    pub fn set_volume(&self, volume: f32) {
        self.write_state().volume = volume;
        self.playback_thread
            .send(LogicToPlaybackMessage::SetVolume(volume));
    }

    pub fn should_shutdown(&self) -> bool {
        self.tokio_thread.should_shutdown()
    }
}
impl Logic {
    pub fn request_play_track(&self, track_id: &TrackId) {
        // Public API used by UI: keep current playing until new track is ready
        self.schedule_play_track(track_id);
    }
}
impl Logic {
    fn initial_fetch(&self) {
        let client = self.client.clone();
        let state = self.state.clone();
        self.tokio_thread.spawn(async move {
            let future = {
                let state = state.clone();
                async move {
                    client.ping().await?;

                    let result = blackbird_state::fetch_all(&client, |batch_count, total_count| {
                        tracing::info!("Fetched {batch_count} tracks, total {total_count} tracks");
                    })
                    .await?;

                    state.write().unwrap().library.populate(
                        result.track_ids,
                        result.track_map,
                        result.groups,
                        result.albums,
                    );

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
