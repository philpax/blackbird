use std::time::Duration;

use blackbird_state::TrackId;

use crate::app_state::TrackAndPosition;

#[cfg(feature = "audio")]
use std::collections::VecDeque;

pub struct PlaybackThread {
    /// Wrapped in `Option` so that `Drop` can close the channel before joining
    /// the thread.
    logic_to_playback_tx: Option<PlaybackThreadSendHandle>,
    /// Kept to prevent the thread from being detached until the struct is
    /// dropped (at which point the process is exiting anyway).
    _playback_thread_handle: Option<std::thread::JoinHandle<()>>,
}

#[derive(Clone)]
pub struct PlaybackThreadSendHandle(std::sync::mpsc::Sender<LogicToPlaybackMessage>);
impl PlaybackThreadSendHandle {
    pub fn send(&self, message: LogicToPlaybackMessage) {
        self.0.send(message).unwrap();
    }
}
/// How a track should be loaded into the playback thread.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum TrackLoadMode {
    /// Start playing immediately from the beginning.
    Play,
    /// Load paused and seek to the given position (session restore).
    Paused(Duration),
}

/// A track's decoded-audio payload as sent to the playback thread.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TrackPlayback {
    pub track_id: TrackId,
    pub data: Vec<u8>,
    /// Optional linear amplification factor (e.g. from ReplayGain) applied on
    /// top of the decoded samples.
    pub gain: Option<f32>,
}

#[cfg(feature = "audio")]
impl TrackPlayback {
    /// Decodes the audio payload and appends it to `sink`, applying `gain` as
    /// an amplification factor when set. Returns the `TrackId` back for use
    /// in subsequent bookkeeping on success, or alongside the decode error on
    /// failure so the caller can report which track failed.
    fn decode_and_append(
        self,
        sink: &rodio::Sink,
    ) -> Result<TrackId, (TrackId, rodio::decoder::DecoderError)> {
        use rodio::Source as _;

        let decoder = rodio::decoder::DecoderBuilder::new()
            .with_byte_len(self.data.len() as u64)
            .with_data(std::io::Cursor::new(self.data))
            .build();
        let decoder = match decoder {
            Ok(d) => d,
            Err(e) => return Err((self.track_id, e)),
        };
        match self.gain {
            Some(factor) => sink.append(decoder.amplify(factor)),
            None => sink.append(decoder),
        }
        Ok(self.track_id)
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum LogicToPlaybackMessage {
    /// Load a track with the specified mode (play or paused at position).
    LoadTrack {
        track: TrackPlayback,
        mode: TrackLoadMode,
    },
    /// Append a track to the gapless queue.
    AppendNextTrack(TrackPlayback),
    ClearQueuedNextTracks,
    TogglePlayback,
    Play,
    Pause,
    StopPlayback,
    Seek(Duration),
    /// Seek without debouncing. Used on scrub bar release to ensure the
    /// final position is always applied.
    SeekImmediate(Duration),
    SetVolume(f32),
    /// Sent during shutdown to exit the playback loop immediately. Needed
    /// because cloned `PlaybackThreadSendHandle`s in tokio tasks keep the
    /// channel open, so disconnect alone is not reliable.
    Shutdown,
}

pub type PlaybackToLogicRx = tokio::sync::broadcast::Receiver<PlaybackToLogicMessage>;
#[derive(Debug, Clone)]
pub enum PlaybackToLogicMessage {
    TrackStarted(TrackAndPosition),
    PlaybackStateChanged(PlaybackState),
    PositionChanged(TrackAndPosition),
    TrackEnded,
    FailedToPlayTrack(TrackId, String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Playing,
    Paused,
    Stopped,
}

impl Drop for PlaybackThread {
    fn drop(&mut self) {
        // Send an explicit shutdown message. We can't rely on channel disconnect
        // because cloned PlaybackThreadSendHandles in tokio tasks keep the
        // channel open until those tasks complete.
        if let Some(tx) = self.logic_to_playback_tx.take() {
            let _ = tx.0.send(LogicToPlaybackMessage::Shutdown);
        }
        // Don't join — rodio's Sink/OutputStream cleanup can block while audio
        // buffers drain, which stalls the main thread and keeps audio playing.
        // The process exit will kill the thread and close audio devices immediately.
    }
}

impl PlaybackThread {
    /// Creates a new playback thread with the given volume and broadcast sender.
    /// The broadcast sender is used to send playback events back to the logic layer.
    pub fn new(
        volume: f32,
        playback_to_logic_tx: tokio::sync::broadcast::Sender<PlaybackToLogicMessage>,
    ) -> Self {
        let (logic_to_playback_tx, logic_to_playback_rx) =
            std::sync::mpsc::channel::<LogicToPlaybackMessage>();

        let playback_thread_handle = std::thread::spawn(move || {
            Self::run(logic_to_playback_rx, playback_to_logic_tx, volume);
        });

        Self {
            logic_to_playback_tx: Some(PlaybackThreadSendHandle(logic_to_playback_tx)),
            _playback_thread_handle: Some(playback_thread_handle),
        }
    }

    pub fn send(&self, message: LogicToPlaybackMessage) {
        if let Some(tx) = &self.logic_to_playback_tx {
            tx.send(message);
        }
    }

    pub fn send_handle(&self) -> PlaybackThreadSendHandle {
        self.logic_to_playback_tx
            .clone()
            .expect("playback thread is alive")
    }

    #[cfg(feature = "audio")]
    fn run(
        playback_rx: std::sync::mpsc::Receiver<LogicToPlaybackMessage>,
        logic_tx: tokio::sync::broadcast::Sender<PlaybackToLogicMessage>,
        volume: f32,
    ) {
        use LogicToPlaybackMessage as LTPM;
        use PlaybackToLogicMessage as PTLM;
        use rodio::cpal::traits::HostTrait as _;

        fn error_callback(err: rodio::cpal::StreamError) {
            tracing::warn!("audio stream error: {err}");
        }

        // Use a fixed buffer size to avoid underruns on machines where the
        // default ALSA buffer is too small for real-time resampling.
        let buffer_size = rodio::cpal::BufferSize::Fixed(2048);

        let mut stream_handle = rodio::OutputStreamBuilder::from_default_device()
            .and_then(|builder| {
                builder
                    .with_buffer_size(buffer_size)
                    .with_error_callback(error_callback as fn(_))
                    .open_stream()
            })
            .or_else(|original_err| {
                // Fallback: try other devices with their default configs.
                let devices = rodio::cpal::default_host()
                    .output_devices()
                    .map_err(|_| original_err)?;
                for device in devices {
                    if let Ok(builder) = rodio::OutputStreamBuilder::from_device(device)
                        && let Ok(handle) = builder
                            .with_buffer_size(buffer_size)
                            .with_error_callback(error_callback as fn(_))
                            .open_stream()
                    {
                        return Ok(handle);
                    }
                }
                Err(rodio::StreamError::NoDevice)
            })
            .unwrap();
        stream_handle.log_on_drop(false);
        let sink = rodio::Sink::connect_new(stream_handle.mixer());
        sink.set_volume(volume * volume);

        const SEEK_DEBOUNCE_DURATION: Duration = Duration::from_millis(250);

        let mut last_track_id = None;
        let mut last_seek_time = std::time::Instant::now();
        let mut last_position_update = std::time::Instant::now();

        let mut state = PlaybackState::Stopped;
        let mut queued_tracks: VecDeque<TrackId> = VecDeque::new();
        // Number of queued tracks to skip on the next transition (e.g. after
        // a playback mode change invalidates the gapless-appended tracks).
        let mut skip_on_transition: usize = 0;
        fn update_and_send_state(
            logic_tx: &tokio::sync::broadcast::Sender<PlaybackToLogicMessage>,
            state: &mut PlaybackState,
            new_state: PlaybackState,
        ) {
            *state = new_state;
            let _ = logic_tx.send(PTLM::PlaybackStateChanged(*state));
        }

        loop {
            // Process all available messages without blocking.
            // Detect channel disconnect to exit when the sender is dropped.
            loop {
                let msg = match playback_rx.try_recv() {
                    Ok(msg) => msg,
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
                };
                match msg {
                    LTPM::LoadTrack { track, mode } => {
                        // For paused loads, pause the sink *before* appending so that
                        // playback does not start automatically.
                        let paused_position = match &mode {
                            TrackLoadMode::Play => None,
                            TrackLoadMode::Paused(pos) => {
                                sink.pause();
                                Some(*pos)
                            }
                        };

                        // Append new track first, then clear old tracks.
                        // This ensures the sink is never completely empty.
                        let track_id = match track.decode_and_append(&sink) {
                            Ok(track_id) => track_id,
                            Err((track_id, err)) => {
                                // Send a dummy track-started to ensure core is aware of what the
                                // track was that caused the failure.
                                let _ = logic_tx.send(PTLM::TrackStarted(TrackAndPosition {
                                    track_id: track_id.clone(),
                                    position: Duration::from_secs(0),
                                }));
                                update_and_send_state(
                                    &logic_tx,
                                    &mut state,
                                    PlaybackState::Stopped,
                                );
                                let _ = logic_tx
                                    .send(PTLM::FailedToPlayTrack(track_id, err.to_string()));
                                continue;
                            }
                        };

                        // Skip all old tracks (everything except the one we just appended).
                        let tracks_to_skip = queued_tracks.len();
                        for _ in 0..tracks_to_skip {
                            sink.skip_one();
                        }

                        if let Some(position) = paused_position {
                            if let Err(e) = sink.try_seek(position) {
                                tracing::warn!(
                                    "Failed to seek restored track to {position:?}: {e}"
                                );
                            }
                        } else {
                            sink.play();
                        }

                        // Reset queue tracking — only this track is now queued.
                        queued_tracks.clear();
                        queued_tracks.push_back(track_id.clone());
                        skip_on_transition = 0;

                        last_track_id = Some(track_id.clone());
                        let position = paused_position.unwrap_or_else(|| Duration::from_secs(0));
                        let _ = logic_tx
                            .send(PTLM::TrackStarted(TrackAndPosition { track_id, position }));
                        let new_state = if paused_position.is_some() {
                            PlaybackState::Paused
                        } else {
                            PlaybackState::Playing
                        };
                        update_and_send_state(&logic_tx, &mut state, new_state);
                    }
                    LTPM::AppendNextTrack(track) => {
                        // Append to sink for gapless playback.
                        let track_id = match track.decode_and_append(&sink) {
                            Ok(track_id) => track_id,
                            Err((track_id, err)) => {
                                tracing::warn!(
                                    "Failed to decode next track {}: {}",
                                    track_id.0,
                                    err
                                );
                                let _ = logic_tx
                                    .send(PTLM::FailedToPlayTrack(track_id, err.to_string()));
                                continue;
                            }
                        };
                        queued_tracks.push_back(track_id.clone());
                        tracing::debug!(
                            "Appended next track {} (queue length: {})",
                            track_id.0,
                            queued_tracks.len()
                        );
                    }
                    LTPM::ClearQueuedNextTracks => {
                        // Mark all queued-but-not-yet-playing tracks for skipping.
                        // rodio doesn't support removing non-current sources, so we
                        // record how many to skip when a transition is detected.
                        let count = queued_tracks.len().saturating_sub(1);
                        skip_on_transition = count;
                        tracing::debug!(
                            "Marked {} queued track(s) for skipping on transition",
                            count,
                        );
                    }
                    LTPM::TogglePlayback => {
                        if sink.is_paused() {
                            sink.play();
                            update_and_send_state(&logic_tx, &mut state, PlaybackState::Playing);
                        } else {
                            sink.pause();
                            update_and_send_state(&logic_tx, &mut state, PlaybackState::Paused);
                        }
                    }
                    LTPM::Play => {
                        sink.play();
                        update_and_send_state(&logic_tx, &mut state, PlaybackState::Playing);
                    }
                    LTPM::Pause => {
                        sink.pause();
                        update_and_send_state(&logic_tx, &mut state, PlaybackState::Paused);
                    }
                    LTPM::StopPlayback => {
                        sink.pause();
                        update_and_send_state(&logic_tx, &mut state, PlaybackState::Stopped);

                        let position = Duration::ZERO;
                        if let Err(e) = sink.try_seek(position) {
                            tracing::warn!("Failed to seek to position {position:?}: {e}");
                        } else if let Some(track_id) = last_track_id.clone() {
                            let _ = logic_tx.send(PTLM::PositionChanged(TrackAndPosition {
                                track_id,
                                position,
                            }));
                        }
                    }
                    LTPM::Seek(position) => {
                        let now = std::time::Instant::now();
                        if now.duration_since(last_seek_time) >= SEEK_DEBOUNCE_DURATION {
                            last_seek_time = now;
                            if let Err(e) = sink.try_seek(position) {
                                tracing::warn!("Failed to seek to position {position:?}: {e}");
                            }
                            if let Some(track_id) = last_track_id.clone() {
                                let _ = logic_tx.send(PTLM::PositionChanged(TrackAndPosition {
                                    track_id,
                                    position,
                                }));
                            }
                        }
                    }
                    LTPM::SeekImmediate(position) => {
                        last_seek_time = std::time::Instant::now();
                        if let Err(e) = sink.try_seek(position) {
                            tracing::warn!("Failed to seek to position {position:?}: {e}");
                        }
                        if let Some(track_id) = last_track_id.clone() {
                            let _ = logic_tx.send(PTLM::PositionChanged(TrackAndPosition {
                                track_id,
                                position,
                            }));
                        }
                    }
                    LTPM::SetVolume(volume) => {
                        sink.set_volume(volume * volume);
                    }
                    LTPM::Shutdown => return,
                }
            }

            // Check for track transitions (gapless playback)
            let current_sink_len = sink.len();
            let expected_len = queued_tracks.len();
            if current_sink_len < expected_len {
                // One or more tracks have finished
                let finished_count = expected_len - current_sink_len;
                for _ in 0..finished_count {
                    queued_tracks.pop_front();
                }

                // Skip invalidated tracks (e.g. from a playback mode change).
                while skip_on_transition > 0 && !queued_tracks.is_empty() {
                    tracing::debug!(
                        "Skipping track {} due to playback mode change",
                        queued_tracks.front().unwrap().0
                    );
                    sink.skip_one();
                    queued_tracks.pop_front();
                    skip_on_transition -= 1;
                }

                // If we still have tracks queued, send TrackStarted for the new current track.
                if let Some(new_current_id) = queued_tracks.front() {
                    last_track_id = Some(new_current_id.clone());
                    let _ = logic_tx.send(PTLM::TrackStarted(TrackAndPosition {
                        track_id: new_current_id.clone(),
                        position: sink.get_pos(),
                    }));
                    tracing::debug!(
                        "Track transition: now playing {} (queue length: {})",
                        new_current_id.0,
                        queued_tracks.len()
                    );
                }
            }

            // Check if we should auto-advance to next track
            if sink.empty() && state == PlaybackState::Playing {
                queued_tracks.clear();
                skip_on_transition = 0;
                update_and_send_state(&logic_tx, &mut state, PlaybackState::Stopped);
                let _ = logic_tx.send(PTLM::TrackEnded);
            }

            // Send position updates every 250ms.
            let current_position = sink.get_pos();
            let now = std::time::Instant::now();
            if now.duration_since(last_position_update) >= Duration::from_millis(250) {
                last_position_update = now;
                if !sink.empty()
                    && !sink.is_paused()
                    && let Some(track_id) = last_track_id.clone()
                {
                    let _ = logic_tx.send(PTLM::PositionChanged(TrackAndPosition {
                        track_id,
                        position: current_position,
                    }));
                }
            }

            // Sleep for 10ms between iterations
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[cfg(not(feature = "audio"))]
    fn run(
        _playback_rx: std::sync::mpsc::Receiver<LogicToPlaybackMessage>,
        _logic_tx: tokio::sync::broadcast::Sender<PlaybackToLogicMessage>,
        _volume: f32,
    ) {
        unimplemented!(
            "Audio playback is disabled - blackbird-core was built without the 'audio' feature"
        )
    }
}
