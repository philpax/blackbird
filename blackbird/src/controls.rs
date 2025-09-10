use std::sync::{Arc, RwLock};

use blackbird_core::{
    AppState, LogicRequestHandle, LogicRequestMessage, PlaybackState, PlaybackToLogicMessage,
    PlaybackToLogicRx, TrackDisplayDetails,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, PlatformConfig, SeekDirection,
};

pub struct Controls {
    controls: MediaControls,
    playback_to_logic_rx: PlaybackToLogicRx,
    state: Arc<RwLock<AppState>>,
}
impl Controls {
    pub fn new(
        window_handle: Option<&dyn HasWindowHandle>,
        playback_to_logic_rx: PlaybackToLogicRx,
        logic_request: LogicRequestHandle,
        state: Arc<RwLock<AppState>>,
    ) -> Result<Self, souvlaki::Error> {
        let mut controls = MediaControls::new(PlatformConfig {
            dbus_name: "blackbird",
            display_name: "Blackbird Music Player",
            hwnd: window_handle
                .and_then(|handle| handle.window_handle().ok())
                .and_then(|handle| {
                    if let RawWindowHandle::Win32(win32_handle) = handle.as_raw() {
                        Some(win32_handle.hwnd.get() as *mut std::ffi::c_void)
                    } else {
                        None
                    }
                }),
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
                    let display_details = TrackDisplayDetails::from_track_id(
                        &track_and_position.track_id,
                        track_and_position.position,
                        &self.state,
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
                PlaybackToLogicMessage::TrackEnded => {
                    // PlaybackStateChanged will take care of this
                    Ok(())
                }
            };
            if let Err(e) = result {
                tracing::warn!("Failed to update media controls: {:?}", e);
            }
        }
    }
}

fn seek_direction_to_sign(direction: SeekDirection) -> i64 {
    if direction == SeekDirection::Forward {
        1
    } else {
        -1
    }
}
