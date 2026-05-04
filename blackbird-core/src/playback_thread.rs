use std::time::Duration;

use blackbird_state::TrackId;

use crate::app_state::TrackAndPosition;

#[cfg(feature = "audio")]
use crate::playback_source::PlaybackController;

/// How a track should be loaded into the playback thread.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub enum TrackLoadMode {
    /// Start playing immediately from the beginning.
    Play,
    /// Load paused and seek to the given position (session restore).
    Paused(Duration),
}

/// The ReplayGain-derived coefficients for a single track. The audio
/// pipeline combines `factor` with a live preamp and clamps the product
/// to `inv_peak` to prevent clipping.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct ReplayGainTrackInfo {
    /// Base linear factor computed from the track's `trackGain`/`albumGain`
    /// plus `baseGain`. Does not include the user-configurable preamp.
    pub factor: f32,
    /// `1 / peak` — the maximum linear multiplier that keeps the loudest
    /// sample at or below 1.0. `f32::INFINITY` if no peak is available.
    pub inv_peak: f32,
}

/// A track's decoded-audio payload as sent to the playback thread.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TrackPlayback {
    pub track_id: TrackId,
    pub data: Vec<u8>,
    /// ReplayGain coefficients. `None` means the track has no metadata
    /// and will be played back untouched (no preamp or clipping clamp
    /// applied).
    pub replaygain: Option<ReplayGainTrackInfo>,
}

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

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum LogicToPlaybackMessage {
    /// Load a track with the specified mode (play or paused at position).
    LoadTrack {
        track: TrackPlayback,
        mode: TrackLoadMode,
    },
    /// Append a track to the gapless next slot.
    AppendNextTrack(TrackPlayback),
    /// Drop the staged gapless next track. Sent when the playback mode
    /// changes and the previously selected next track is no longer valid.
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
    /// Enables or disables ReplayGain application for the currently
    /// playing source and any future ones.
    SetApplyReplayGain(bool),
    /// Adjusts the ReplayGain preamp (in dB) for the currently playing
    /// source and any future ones.
    SetReplayGainPreamp(f32),
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
        // Don't join — the underlying audio device cleanup can block while
        // buffers drain, which stalls the main thread and keeps audio
        // playing. The process exit will kill the thread and close audio
        // devices immediately.
    }
}

impl PlaybackThread {
    /// Creates a new playback thread with the given volume, ReplayGain
    /// settings, and broadcast sender. The broadcast sender is used to send
    /// playback events back to the logic layer.
    pub fn new(
        volume: f32,
        apply_replaygain: bool,
        replaygain_preamp_db: f32,
        playback_to_logic_tx: tokio::sync::broadcast::Sender<PlaybackToLogicMessage>,
    ) -> Self {
        let (logic_to_playback_tx, logic_to_playback_rx) =
            std::sync::mpsc::channel::<LogicToPlaybackMessage>();

        let playback_thread_handle = std::thread::spawn(move || {
            Self::run(
                logic_to_playback_rx,
                playback_to_logic_tx,
                volume,
                apply_replaygain,
                replaygain_preamp_db,
            );
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
        apply_replaygain: bool,
        replaygain_preamp_db: f32,
    ) {
        use LogicToPlaybackMessage as LTPM;
        use PlaybackToLogicMessage as PTLM;
        use rodio::cpal::traits::HostTrait as _;

        fn error_callback(err: rodio::cpal::Error) {
            tracing::warn!("audio stream error: {err}");
        }

        // Use a fixed buffer size to avoid underruns on machines where the
        // default ALSA buffer is too small for real-time resampling.
        let buffer_size = rodio::cpal::BufferSize::Fixed(2048);

        let mut stream_handle = rodio::DeviceSinkBuilder::from_default_device()
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
                    if let Ok(builder) = rodio::DeviceSinkBuilder::from_device(device)
                        && let Ok(handle) = builder
                            .with_buffer_size(buffer_size)
                            .with_error_callback(error_callback as fn(_))
                            .open_stream()
                    {
                        return Ok(handle);
                    }
                }
                Err(rodio::DeviceSinkError::NoDevice)
            })
            .unwrap();
        stream_handle.log_on_drop(false);

        let target_channels = stream_handle.config().channel_count();
        let target_sample_rate = stream_handle.config().sample_rate();
        let (controller, source) = PlaybackController::new(
            target_channels,
            target_sample_rate,
            volume * volume,
            apply_replaygain,
            replaygain_preamp_db,
            logic_tx.clone(),
        );
        stream_handle.mixer().add(source);

        const SEEK_DEBOUNCE_DURATION: Duration = Duration::from_millis(250);

        let mut last_seek_time = std::time::Instant::now();
        let mut last_position_update = std::time::Instant::now();

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
                        let track_id = track.track_id.clone();
                        if let Err(e) = controller.load_track(track, mode) {
                            // Send a dummy track-started so the core knows
                            // which track failed.
                            let _ = logic_tx.send(PTLM::TrackStarted(TrackAndPosition {
                                track_id: track_id.clone(),
                                position: Duration::ZERO,
                            }));
                            let _ =
                                logic_tx.send(PTLM::PlaybackStateChanged(PlaybackState::Stopped));
                            let _ = logic_tx
                                .send(PTLM::FailedToPlayTrack(track_id, e.error.to_string()));
                            controller.stop();
                        }
                    }
                    LTPM::AppendNextTrack(track) => {
                        let track_id = track.track_id.clone();
                        match controller.append_next(track) {
                            Ok(()) => {
                                tracing::debug!("Appended next track {}", track_id.0);
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "Failed to decode next track {}: {}",
                                    track_id.0,
                                    e.error
                                );
                                let _ = logic_tx
                                    .send(PTLM::FailedToPlayTrack(track_id, e.error.to_string()));
                            }
                        }
                    }
                    LTPM::ClearQueuedNextTracks => {
                        controller.clear_next();
                    }
                    LTPM::TogglePlayback => controller.toggle(),
                    LTPM::Play => controller.play(),
                    LTPM::Pause => controller.pause(),
                    LTPM::StopPlayback => controller.stop(),
                    LTPM::Seek(position) => {
                        let now = std::time::Instant::now();
                        if now.duration_since(last_seek_time) >= SEEK_DEBOUNCE_DURATION {
                            last_seek_time = now;
                            controller.seek(position);
                            if let Some(snapshot) = controller.current_position() {
                                let _ = logic_tx.send(PTLM::PositionChanged(TrackAndPosition {
                                    track_id: snapshot.track_id,
                                    position,
                                }));
                            }
                        }
                    }
                    LTPM::SeekImmediate(position) => {
                        last_seek_time = std::time::Instant::now();
                        controller.seek(position);
                        if let Some(snapshot) = controller.current_position() {
                            let _ = logic_tx.send(PTLM::PositionChanged(TrackAndPosition {
                                track_id: snapshot.track_id,
                                position,
                            }));
                        }
                    }
                    LTPM::SetVolume(volume) => {
                        controller.set_volume(volume * volume);
                    }
                    LTPM::SetApplyReplayGain(enabled) => {
                        controller.set_replaygain_enabled(enabled);
                    }
                    LTPM::SetReplayGainPreamp(preamp_db) => {
                        controller.set_replaygain_preamp_db(preamp_db);
                    }
                    LTPM::Shutdown => return,
                }
            }

            // Send position updates every 250ms.
            let now = std::time::Instant::now();
            if now.duration_since(last_position_update) >= Duration::from_millis(250) {
                last_position_update = now;
                if controller.current_state() == PlaybackState::Playing
                    && let Some(snapshot) = controller.current_position()
                {
                    let _ = logic_tx.send(PTLM::PositionChanged(snapshot));
                }
            }

            // Sleep for 10ms between iterations.
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[cfg(not(feature = "audio"))]
    fn run(
        _playback_rx: std::sync::mpsc::Receiver<LogicToPlaybackMessage>,
        _logic_tx: tokio::sync::broadcast::Sender<PlaybackToLogicMessage>,
        _volume: f32,
        _apply_replaygain: bool,
        _replaygain_preamp_db: f32,
    ) {
        unimplemented!(
            "Audio playback is disabled - blackbird-core was built without the 'audio' feature"
        )
    }
}
