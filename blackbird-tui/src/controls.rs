#[cfg(feature = "media-controls")]
use std::sync::{Arc, RwLock};

#[cfg(feature = "media-controls")]
use blackbird_core::{
    AppState, LogicRequestHandle, LogicRequestMessage, PlaybackState, PlaybackToLogicMessage,
    PlaybackToLogicRx, TrackDisplayDetails,
};
#[cfg(feature = "media-controls")]
use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, PlatformConfig, SeekDirection,
};

/// On Windows, retrieve the console window HWND so souvlaki can attach
/// System Media Transport Controls to it.
#[cfg(all(feature = "media-controls", target_os = "windows"))]
fn get_console_hwnd() -> Option<*mut std::ffi::c_void> {
    extern "system" {
        fn GetConsoleWindow() -> *mut std::ffi::c_void;
    }
    // SAFETY: GetConsoleWindow is always safe to call; returns null if there is no console.
    let hwnd = unsafe { GetConsoleWindow() };
    if hwnd.is_null() { None } else { Some(hwnd) }
}

#[cfg(feature = "media-controls")]
pub struct Controls {
    controls: MediaControls,
    playback_to_logic_rx: PlaybackToLogicRx,
    state: Arc<RwLock<AppState>>,
}

#[cfg(feature = "media-controls")]
impl Controls {
    pub fn new(
        playback_to_logic_rx: PlaybackToLogicRx,
        logic_request: LogicRequestHandle,
        state: Arc<RwLock<AppState>>,
    ) -> Result<Self, souvlaki::Error> {
        #[cfg(target_os = "windows")]
        let hwnd = get_console_hwnd();
        #[cfg(not(target_os = "windows"))]
        let hwnd: Option<*mut std::ffi::c_void> = None;

        let mut controls = MediaControls::new(PlatformConfig {
            dbus_name: "blackbird",
            display_name: "Blackbird Music Player",
            hwnd,
        })?;

        controls.attach(move |event: MediaControlEvent| match event {
            MediaControlEvent::Play => {
                logic_request.send(LogicRequestMessage::PlayCurrent);
            }
            MediaControlEvent::Pause => {
                logic_request.send(LogicRequestMessage::PauseCurrent);
            }
            MediaControlEvent::Toggle => {
                logic_request.send(LogicRequestMessage::ToggleCurrent);
            }
            MediaControlEvent::Next => {
                logic_request.send(LogicRequestMessage::Next);
            }
            MediaControlEvent::Previous => {
                logic_request.send(LogicRequestMessage::Previous);
            }
            MediaControlEvent::Stop => {
                logic_request.send(LogicRequestMessage::StopCurrent);
            }
            MediaControlEvent::Seek(direction) => {
                logic_request.send(LogicRequestMessage::SeekBy {
                    seconds: 10 * seek_direction_to_sign(direction),
                });
            }
            MediaControlEvent::SeekBy(direction, duration) => {
                logic_request.send(LogicRequestMessage::SeekBy {
                    seconds: duration.as_secs().cast_signed() * seek_direction_to_sign(direction),
                });
            }
            MediaControlEvent::SetPosition(position) => {
                logic_request.send(LogicRequestMessage::Seek(position.0));
            }
            _ => {
                tracing::debug!("Unhandled media control event: {:?}", event);
            }
        })?;

        Ok(Self {
            controls,
            playback_to_logic_rx,
            state,
        })
    }

    pub fn update(&mut self) {
        while let Ok(event) = self.playback_to_logic_rx.try_recv() {
            let result = match event {
                PlaybackToLogicMessage::TrackStarted(track_and_position) => {
                    let display_details = TrackDisplayDetails::from_track_and_position(
                        &track_and_position,
                        &self.state.read().unwrap(),
                    );
                    if let Some(display_details) = display_details {
                        self.controls.set_metadata(MediaMetadata {
                            title: Some(&display_details.track_title),
                            artist: Some(&display_details.album_artist),
                            album: Some(&display_details.album_name),
                            duration: Some(display_details.track_duration),
                            ..Default::default()
                        })
                    } else {
                        Ok(())
                    }
                }
                PlaybackToLogicMessage::PlaybackStateChanged(state) => {
                    let playback_status = match state {
                        PlaybackState::Playing => MediaPlayback::Playing { progress: None },
                        PlaybackState::Paused => MediaPlayback::Paused { progress: None },
                        PlaybackState::Stopped => {
                            self.controls.set_metadata(MediaMetadata::default()).ok();
                            MediaPlayback::Stopped
                        }
                    };
                    self.controls.set_playback(playback_status)
                }
                PlaybackToLogicMessage::PositionChanged(track_and_position) => {
                    self.controls.set_playback(MediaPlayback::Playing {
                        progress: Some(souvlaki::MediaPosition(track_and_position.position)),
                    })
                }
                PlaybackToLogicMessage::TrackEnded
                | PlaybackToLogicMessage::FailedToPlayTrack(..) => Ok(()),
            };
            if let Err(e) = result {
                tracing::warn!("Failed to update media controls: {:?}", e);
            }
        }
    }
}

#[cfg(feature = "media-controls")]
fn seek_direction_to_sign(direction: SeekDirection) -> i64 {
    if direction == SeekDirection::Forward {
        1
    } else {
        -1
    }
}
