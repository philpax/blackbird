use std::time::Duration;

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
pub enum LogicToPlaybackMessage {
    PlayTrack(TrackId, Vec<u8>),
    TogglePlayback,
    Play,
    Pause,
    StopPlayback,
    Seek(Duration),
}

pub type PlaybackToLogicRx = tokio::sync::broadcast::Receiver<PlaybackToLogicMessage>;
#[derive(Debug, Clone)]
pub enum PlaybackToLogicMessage {
    TrackStarted(TrackAndPosition),
    PlaybackStateChanged(PlaybackState),
    PositionChanged(TrackAndPosition),
    TrackEnded,
    FailedToPlayTrack(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Playing,
    Paused,
    Stopped,
}

impl PlaybackThread {
    pub fn new() -> Self {
        let (logic_to_playback_tx, logic_to_playback_rx) =
            std::sync::mpsc::channel::<LogicToPlaybackMessage>();
        let (playback_to_logic_tx, playback_to_logic_rx) =
            tokio::sync::broadcast::channel::<PlaybackToLogicMessage>(100);

        let playback_thread_handle = std::thread::spawn(move || {
            Self::run(logic_to_playback_rx, playback_to_logic_tx);
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

    fn run(
        playback_rx: std::sync::mpsc::Receiver<LogicToPlaybackMessage>,
        logic_tx: tokio::sync::broadcast::Sender<PlaybackToLogicMessage>,
    ) {
        use LogicToPlaybackMessage as LTPM;
        use PlaybackToLogicMessage as PTLM;

        let stream_handle = rodio::OutputStreamBuilder::open_default_stream().unwrap();
        let sink = rodio::Sink::connect_new(stream_handle.mixer());
        sink.set_volume(1.0);

        const SEEK_DEBOUNCE_DURATION: Duration = Duration::from_millis(250);

        let mut last_track_id = None;
        let mut last_seek_time = std::time::Instant::now();
        let mut last_position_update = std::time::Instant::now();

        let mut state = PlaybackState::Stopped;
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
                        let need_to_skip = !sink.empty();

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
                                    track_id,
                                    position: Duration::from_secs(0),
                                }));
                                update_and_send_state(
                                    &logic_tx,
                                    &mut state,
                                    PlaybackState::Stopped,
                                );
                                let _ = logic_tx.send(PTLM::FailedToPlayTrack(err.to_string()));
                                continue;
                            }
                        };

                        sink.append(decoder);
                        if need_to_skip {
                            sink.skip_one();
                        }
                        sink.play();
                        last_track_id = Some(track_id.clone());
                        let _ = logic_tx.send(PTLM::TrackStarted(TrackAndPosition {
                            track_id,
                            position: Duration::from_secs(0),
                        }));
                        update_and_send_state(&logic_tx, &mut state, PlaybackState::Playing);
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
                }
            }

            // Check if we should auto-advance to next track
            if sink.empty() && state == PlaybackState::Playing {
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
}
