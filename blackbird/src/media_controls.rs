use std::sync::Arc;
use std::time::Duration;

use blackbird_core as bc;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, PlatformConfig, SeekDirection};

pub fn setup(
    logic: Arc<bc::Logic>,
    window_handle: Option<&dyn HasWindowHandle>,
) -> Result<(), souvlaki::Error> {
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

    controls.attach({
        let logic = logic.clone();
        move |event: MediaControlEvent| match event {
            MediaControlEvent::Play => {
                logic.play();
            }
            MediaControlEvent::Pause => {
                logic.pause();
            }
            MediaControlEvent::Toggle => {
                logic.toggle_playback();
            }
            MediaControlEvent::Next => {
                todo!()
            }
            MediaControlEvent::Previous => {
                todo!()
            }
            MediaControlEvent::Stop => {
                logic.stop_playback();
            }
            MediaControlEvent::Seek(direction) => {
                if let Some(playing_info) = logic.get_playing_info() {
                    let current_position = playing_info.song_position;
                    let seek_amount = Duration::from_secs(10);

                    let new_position = match direction {
                        SeekDirection::Forward => current_position + seek_amount,
                        SeekDirection::Backward => current_position.saturating_sub(seek_amount),
                    };

                    logic.seek(new_position);
                }
            }
            MediaControlEvent::SeekBy(direction, duration) => {
                if let Some(playing_info) = logic.get_playing_info() {
                    let current_position = playing_info.song_position;

                    let new_position = match direction {
                        SeekDirection::Forward => current_position + duration,
                        SeekDirection::Backward => current_position.saturating_sub(duration),
                    };

                    logic.seek(new_position);
                }
            }
            MediaControlEvent::SetPosition(position) => {
                let duration = position.0;
                logic.seek(duration);
            }
            _ => {
                tracing::debug!("Unhandled media control event: {:?}", event);
            }
        }
    })?;

    logic.spawn({
        let track_change_rx = logic.subscribe_to_track_changes();
        async move {
            let mut track_change_rx = track_change_rx;
            while let Ok(event) = track_change_rx.recv().await {
                let result = match event {
                    bc::PlaybackToLogicMessage::TrackStarted(playing_info) => {
                        controls.set_metadata(MediaMetadata {
                            title: Some(&playing_info.song_title),
                            artist: Some(&playing_info.album_artist),
                            album: Some(&playing_info.album_name),
                            duration: Some(playing_info.song_duration),
                            ..Default::default()
                        })
                    }
                    bc::PlaybackToLogicMessage::PlaybackStateChanged(state) => {
                        use souvlaki::MediaPlayback;
                        let playback_status = match state {
                            bc::PlaybackState::Playing => MediaPlayback::Playing { progress: None },
                            bc::PlaybackState::Paused => MediaPlayback::Paused { progress: None },
                            bc::PlaybackState::Stopped => {
                                // When stopped, clear metadata and set playback status
                                controls.set_metadata(MediaMetadata::default()).ok();
                                MediaPlayback::Stopped
                            }
                        };
                        controls.set_playback(playback_status)
                    }
                    bc::PlaybackToLogicMessage::PositionChanged(position) => {
                        use souvlaki::MediaPlayback;
                        controls.set_playback(MediaPlayback::Playing {
                            progress: Some(souvlaki::MediaPosition(position)),
                        })
                    }
                };
                if let Err(e) = result {
                    tracing::warn!("Failed to update media controls: {:?}", e);
                }
            }
        }
    });

    Ok(())
}
