use std::time::Duration;

use blackbird_state::TrackId;

use crate::app_state::TrackAndPosition;

#[cfg(feature = "audio")]
use std::collections::VecDeque;

pub struct PlaybackThread {
    /// Wrapped in `Option` so that `Drop` can close the channel before joining
    /// the thread.
    logic_to_playback_tx: Option<PlaybackThreadSendHandle>,
    /// Wrapped in `Option` so that `Drop` can join the thread.
    playback_thread_handle: Option<std::thread::JoinHandle<()>>,
    playback_to_logic_rx: PlaybackToLogicRx,
}

#[derive(Clone)]
pub struct PlaybackThreadSendHandle(std::sync::mpsc::Sender<LogicToPlaybackMessage>);
impl PlaybackThreadSendHandle {
    pub fn send(&self, message: LogicToPlaybackMessage) {
        self.0.send(message).unwrap();
    }
}
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum LogicToPlaybackMessage {
    PlayTrack(TrackId, Vec<u8>),
    AppendNextTrack(TrackId, Vec<u8>),
    ClearQueuedNextTracks,
    TogglePlayback,
    Play,
    Pause,
    StopPlayback,
    Seek(Duration),
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
        // Join the thread so audio stops before the process exits.
        if let Some(handle) = self.playback_thread_handle.take() {
            let _ = handle.join();
        }
    }
}

impl PlaybackThread {
    pub fn new(volume: f32) -> Self {
        let (logic_to_playback_tx, logic_to_playback_rx) =
            std::sync::mpsc::channel::<LogicToPlaybackMessage>();
        let (playback_to_logic_tx, playback_to_logic_rx) =
            tokio::sync::broadcast::channel::<PlaybackToLogicMessage>(100);

        let playback_thread_handle = std::thread::spawn(move || {
            Self::run(logic_to_playback_rx, playback_to_logic_tx, volume);
        });

        Self {
            logic_to_playback_tx: Some(PlaybackThreadSendHandle(logic_to_playback_tx)),
            playback_thread_handle: Some(playback_thread_handle),
            playback_to_logic_rx,
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

    pub fn subscribe(&self) -> PlaybackToLogicRx {
        self.playback_to_logic_rx.resubscribe()
    }

    #[cfg(feature = "audio")]
    fn run(
        playback_rx: std::sync::mpsc::Receiver<LogicToPlaybackMessage>,
        logic_tx: tokio::sync::broadcast::Sender<PlaybackToLogicMessage>,
        volume: f32,
    ) {
        use LogicToPlaybackMessage as LTPM;
        use PlaybackToLogicMessage as PTLM;

        let mut stream_handle = rodio::OutputStreamBuilder::open_default_stream().unwrap();
        stream_handle.log_on_drop(false);
        let sink = rodio::Sink::connect_new(stream_handle.mixer());
        sink.set_volume(volume * volume);

        const SEEK_DEBOUNCE_DURATION: Duration = Duration::from_millis(250);

        let mut last_track_id = None;
        let mut last_seek_time = std::time::Instant::now();
        let mut last_position_update = std::time::Instant::now();

        let mut state = PlaybackState::Stopped;
        let mut queued_tracks: VecDeque<TrackId> = VecDeque::new();
        // Track which queued track should be skipped (e.g., after playback mode change)
        let mut skip_next_track: Option<TrackId> = None;
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
                    LTPM::PlayTrack(track_id, data) => {
                        let decoder = rodio::decoder::DecoderBuilder::new()
                            .with_byte_len(data.len() as u64)
                            .with_data(std::io::Cursor::new(data))
                            .build();

                        let decoder = match decoder {
                            Ok(decoder) => decoder,
                            Err(err) => {
                                // Send a dummy track-started to ensure core is aware of what the
                                // track was that caused the failure
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

                        // Append new track first, then clear old tracks
                        // This ensures the sink is never completely empty
                        sink.append(decoder);

                        // Skip all the old tracks (everything except the one we just appended)
                        let tracks_to_skip = queued_tracks.len();
                        for _ in 0..tracks_to_skip {
                            sink.skip_one();
                        }

                        sink.play();

                        // Reset queue tracking - only this track is now queued
                        queued_tracks.clear();
                        queued_tracks.push_back(track_id.clone());
                        skip_next_track = None;

                        last_track_id = Some(track_id.clone());
                        let _ = logic_tx.send(PTLM::TrackStarted(TrackAndPosition {
                            track_id,
                            position: Duration::from_secs(0),
                        }));
                        update_and_send_state(&logic_tx, &mut state, PlaybackState::Playing);
                    }
                    LTPM::AppendNextTrack(track_id, data) => {
                        let decoder = rodio::decoder::DecoderBuilder::new()
                            .with_byte_len(data.len() as u64)
                            .with_data(std::io::Cursor::new(data))
                            .build();

                        let decoder = match decoder {
                            Ok(decoder) => decoder,
                            Err(err) => {
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

                        // Append to sink for gapless playback
                        sink.append(decoder);
                        queued_tracks.push_back(track_id.clone());
                        tracing::debug!(
                            "Appended next track {} (queue length: {})",
                            track_id.0,
                            queued_tracks.len()
                        );
                    }
                    LTPM::ClearQueuedNextTracks => {
                        // Mark the queued next track to be skipped when it starts playing
                        if queued_tracks.len() > 1 {
                            skip_next_track = queued_tracks.get(1).cloned();
                            tracing::debug!(
                                "Marked next track {:?} to be skipped on transition",
                                skip_next_track
                            );
                        }
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
                        } else {
                            let _ = logic_tx.send(PTLM::PositionChanged(TrackAndPosition {
                                track_id: last_track_id.clone().unwrap(),
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
                            let _ = logic_tx.send(PTLM::PositionChanged(TrackAndPosition {
                                track_id: last_track_id.clone().unwrap(),
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

                // If we still have tracks queued, send TrackStarted for the new current track
                if let Some(new_current_id) = queued_tracks.front() {
                    // Check if this track should be skipped
                    if skip_next_track.as_ref() == Some(new_current_id) {
                        tracing::debug!(
                            "Skipping track {} due to playback mode change",
                            new_current_id.0
                        );
                        skip_next_track = None;
                        sink.skip_one();
                        queued_tracks.pop_front();
                        // After skipping, check if there's another track
                        if let Some(actual_current_id) = queued_tracks.front() {
                            last_track_id = Some(actual_current_id.clone());
                            let _ = logic_tx.send(PTLM::TrackStarted(TrackAndPosition {
                                track_id: actual_current_id.clone(),
                                position: sink.get_pos(),
                            }));
                            tracing::debug!(
                                "Track transition after skip: now playing {} (queue length: {})",
                                actual_current_id.0,
                                queued_tracks.len()
                            );
                        }
                    } else {
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
            }

            // Check if we should auto-advance to next track
            if sink.empty() && state == PlaybackState::Playing {
                queued_tracks.clear();
                skip_next_track = None;
                update_and_send_state(&logic_tx, &mut state, PlaybackState::Stopped);
                let _ = logic_tx.send(PTLM::TrackEnded);
            }

            // Send position updates every second
            let current_position = sink.get_pos();
            let now = std::time::Instant::now();
            if now.duration_since(last_position_update) >= Duration::from_millis(250) {
                last_position_update = now;
                if !sink.empty() && !sink.is_paused() {
                    let _ = logic_tx.send(PTLM::PositionChanged(TrackAndPosition {
                        track_id: last_track_id.clone().unwrap(),
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
