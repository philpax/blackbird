use std::time::Duration;

pub struct PlaybackThread {
    logic_to_playback_tx: PlaybackThreadSendHandle,
    _playback_thread_handle: std::thread::JoinHandle<()>,
    playback_to_logic_rx: PlaybackToLogicRx,
}
pub type PlaybackToLogicRx = tokio::sync::broadcast::Receiver<PlaybackToLogicMessage>;
#[derive(Clone)]
pub struct PlaybackThreadSendHandle(std::sync::mpsc::Sender<LogicToPlaybackMessage>);
impl PlaybackThreadSendHandle {
    pub fn send(&self, message: LogicToPlaybackMessage) {
        self.0.send(message).unwrap();
    }
}
#[derive(Debug, Clone)]
pub enum PlaybackToLogicMessage {
    TrackStarted(PlayingInfo),
    PlaybackStateChanged(PlaybackState),
    PositionChanged(Duration),
}
#[derive(Debug, Clone)]
pub enum LogicToPlaybackMessage {
    PlaySong(Vec<u8>, PlayingInfo),
    TogglePlayback,
    Play,
    Pause,
    StopPlayback,
    Seek(Duration),
}

#[derive(Debug, Clone)]
pub enum PlaybackState {
    Playing,
    Paused,
    Stopped,
}

#[derive(Debug, Clone)]
pub struct PlayingInfo {
    pub album_name: String,
    pub album_artist: String,
    pub song_title: String,
    pub song_artist: Option<String>,
    pub song_duration: Duration,
    pub song_position: Duration,
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

        fn build_decoder(data: Vec<u8>) -> rodio::decoder::Decoder<std::io::Cursor<Vec<u8>>> {
            rodio::decoder::DecoderBuilder::new()
                .with_byte_len(data.len() as u64)
                .with_data(std::io::Cursor::new(data))
                .build()
                .unwrap()
        }

        const SEEK_DEBOUNCE_DURATION: Duration = Duration::from_millis(250);

        let mut last_data = None;
        let mut last_seek_time = std::time::Instant::now();
        let mut last_position_update = std::time::Instant::now();

        loop {
            // Process all available messages without blocking
            while let Ok(msg) = playback_rx.try_recv() {
                match msg {
                    LTPM::PlaySong(data, playing_info) => {
                        sink.clear();
                        last_data = Some(data.clone());
                        sink.append(build_decoder(data));
                        sink.play();
                        let _ = logic_tx.send(PTLM::TrackStarted(playing_info));
                        let _ = logic_tx.send(PTLM::PlaybackStateChanged(PlaybackState::Playing));
                    }
                    LTPM::TogglePlayback => {
                        if !sink.is_paused() {
                            sink.pause();
                            let _ =
                                logic_tx.send(PTLM::PlaybackStateChanged(PlaybackState::Paused));
                            continue;
                        }
                        if sink.empty()
                            && let Some(data) = last_data.clone()
                        {
                            sink.append(build_decoder(data));
                        }
                        sink.play();
                        let _ = logic_tx.send(PTLM::PlaybackStateChanged(PlaybackState::Playing));
                    }
                    LTPM::Play => {
                        if sink.empty()
                            && let Some(data) = last_data.clone()
                        {
                            sink.append(build_decoder(data));
                        }
                        sink.play();
                        let _ = logic_tx.send(PTLM::PlaybackStateChanged(PlaybackState::Playing));
                    }
                    LTPM::Pause => {
                        sink.pause();
                        let _ = logic_tx.send(PTLM::PlaybackStateChanged(PlaybackState::Paused));
                    }
                    LTPM::StopPlayback => {
                        sink.clear();
                        let _ = logic_tx.send(PTLM::PlaybackStateChanged(PlaybackState::Stopped));
                    }
                    LTPM::Seek(position) => {
                        let now = std::time::Instant::now();
                        if now.duration_since(last_seek_time) >= SEEK_DEBOUNCE_DURATION {
                            last_seek_time = now;
                            if let Err(e) = sink.try_seek(position) {
                                tracing::warn!("Failed to seek to position {position:?}: {e}");
                            }
                            let _ = logic_tx.send(PTLM::PositionChanged(position));
                        }
                    }
                }
            }

            // Check if we should auto-advance to next track
            if sink.empty()
                && let Some(data) = last_data.clone()
            {
                sink.append(build_decoder(data));
            }

            // Send position updates every second
            let current_position = sink.get_pos();
            let now = std::time::Instant::now();
            if now.duration_since(last_position_update) >= Duration::from_millis(250) {
                last_position_update = now;
                if !sink.empty() && !sink.is_paused() {
                    let _ = logic_tx.send(PTLM::PositionChanged(current_position));
                }
            }

            // Sleep for 10ms between iterations
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}
