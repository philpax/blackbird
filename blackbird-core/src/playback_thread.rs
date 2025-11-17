use std::{collections::VecDeque, time::Duration};

use blackbird_state::TrackId;

use crate::app_state::TrackAndPosition;

pub struct PlaybackThread {
    logic_to_playback_tx: PlaybackThreadSendHandle,
    _playback_thread_handle: std::thread::JoinHandle<()>,
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
    TogglePlayback,
    Play,
    Pause,
    StopPlayback,
    Seek(Duration),
    SetVolume(f32),
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
            logic_to_playback_tx: PlaybackThreadSendHandle(logic_to_playback_tx),
            _playback_thread_handle: playback_thread_handle,
            playback_to_logic_rx,
        }
    }

    pub fn send(&self, message: LogicToPlaybackMessage) {
        self.logic_to_playback_tx.send(message);
    }

    pub fn send_handle(&self) -> PlaybackThreadSendHandle {
        self.logic_to_playback_tx.clone()
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

        let stream_handle = rodio::OutputStreamBuilder::open_default_stream().unwrap();
        let sink = rodio::Sink::connect_new(stream_handle.mixer());
        sink.set_volume(volume * volume);

        const SEEK_DEBOUNCE_DURATION: Duration = Duration::from_millis(250);

        let mut last_track_id = None;
        let mut last_seek_time = std::time::Instant::now();
        let mut last_position_update = std::time::Instant::now();

        let mut state = PlaybackState::Stopped;
        let mut queued_tracks: VecDeque<TrackId> = VecDeque::new();
        fn update_and_send_state(
            logic_tx: &tokio::sync::broadcast::Sender<PlaybackToLogicMessage>,
            state: &mut PlaybackState,
            new_state: PlaybackState,
        ) {
            *state = new_state;
            let _ = logic_tx.send(PTLM::PlaybackStateChanged(*state));
        }

        loop {
            // Process all available messages without blocking
            while let Ok(msg) = playback_rx.try_recv() {
                match msg {
                    LTPM::PlayTrack(track_id, data) => {
                        // Clear all tracks from the sink for a fresh start
                        while !sink.empty() {
                            sink.skip_one();
                        }

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

                        sink.append(decoder);
                        sink.play();

                        // Reset queue tracking - only this track is now queued
                        queued_tracks.clear();
                        queued_tracks.push_back(track_id.clone());

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
        unimplemented!("Audio playback is disabled - blackbird-core was built without the 'audio' feature")
    }
}
