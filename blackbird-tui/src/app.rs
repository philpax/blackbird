use std::time::Duration;

use blackbird_core::{self as bc, PlaybackToLogicMessage};

use crate::{
    config::Config,
    cover_art::CoverArtCache,
    log_buffer::LogBuffer,
    ui::{
        album_art_overlay::AlbumArtOverlay, library::LibraryState, logs::LogsState,
        lyrics::LyricsState, search::SearchState,
    },
};

/// Which panel/mode the UI is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusedPanel {
    Library,
    Search,
    Lyrics,
    Logs,
}

pub struct App {
    // Core infrastructure (shared across views)
    pub logic: bc::Logic,
    pub config: Config,
    pub cover_art_cache: CoverArtCache,
    pub playback_to_logic_rx: bc::PlaybackToLogicRx,
    pub lyrics_loaded_rx: std::sync::mpsc::Receiver<bc::LyricsData>,
    pub library_populated_rx: std::sync::mpsc::Receiver<()>,

    // Global UI orchestration
    pub focused_panel: FocusedPanel,
    pub volume_editing: bool,
    pub quit_confirming: bool,
    pub should_quit: bool,
    pub needs_redraw: bool,
    pub mouse_position: Option<(u16, u16)>,
    pub album_art_overlay: Option<AlbumArtOverlay>,

    // Per-view state (owned by their respective modules)
    pub library: LibraryState,
    pub search: SearchState,
    pub lyrics: LyricsState,
    pub logs: LogsState,
}

impl App {
    pub fn new(
        config: Config,
        logic: bc::Logic,
        playback_to_logic_rx: bc::PlaybackToLogicRx,
        cover_art_cache: CoverArtCache,
        lyrics_loaded_rx: std::sync::mpsc::Receiver<bc::LyricsData>,
        library_populated_rx: std::sync::mpsc::Receiver<()>,
        log_buffer: LogBuffer,
    ) -> Self {
        Self {
            logic,
            config,
            cover_art_cache,
            playback_to_logic_rx,
            lyrics_loaded_rx,
            library_populated_rx,

            focused_panel: FocusedPanel::Library,
            volume_editing: false,
            quit_confirming: false,
            should_quit: false,
            needs_redraw: true,
            mouse_position: None,
            album_art_overlay: None,

            library: LibraryState::new(),
            search: SearchState::new(),
            lyrics: LyricsState::new(),
            logs: LogsState::new(log_buffer),
        }
    }

    pub fn tick(&mut self) {
        self.logic.update();
        self.cover_art_cache.update();

        // Process playback events.
        while let Ok(event) = self.playback_to_logic_rx.try_recv() {
            if let PlaybackToLogicMessage::TrackStarted(tap) = event {
                self.library.scroll_to_track = Some(tap.track_id.clone());
                self.library.needs_scroll_to_playing = false;

                // Auto-request lyrics if the lyrics panel is open.
                if self.focused_panel == FocusedPanel::Lyrics {
                    self.lyrics.start_loading(tap.track_id.clone());
                    self.logic.request_lyrics(&tap.track_id);
                }
            }
        }

        // Process lyrics data.
        while let Ok(lyrics_data) = self.lyrics_loaded_rx.try_recv() {
            if Some(&lyrics_data.track_id) == self.lyrics.track_id.as_ref() {
                self.lyrics.data = lyrics_data.lyrics;
                self.lyrics.loading = false;
            }
        }

        // Process library population.
        while let Ok(()) = self.library_populated_rx.try_recv() {
            self.library.mark_dirty();
            if self.library.needs_scroll_to_playing
                && let Some(track_id) = self.logic.get_playing_track_id()
            {
                self.library.scroll_to_track = Some(track_id);
            }
            // Ensure selection is on a track, not a group header.
            self.library.ensure_selection_on_track(&self.logic);
        }

        // Handle scroll-to-track.
        if let Some(track_id) = self.library.scroll_to_track.take() {
            let state = self.logic.get_state();
            let state = state.read().unwrap();
            if let Some(index) = self.library.find_flat_index_for_track(&state, &track_id) {
                self.library.selected_index = index;
            } else {
                // Track not in library yet, re-queue.
                self.library.scroll_to_track = Some(track_id);
            }
        }

        if self.logic.should_shutdown() {
            self.should_quit = true;
        }

        // Tick processes playback updates, scrub bar advancement, etc.
        self.needs_redraw = true;
    }

    pub fn toggle_search(&mut self) {
        if self.focused_panel == FocusedPanel::Search {
            self.focused_panel = FocusedPanel::Library;
        } else {
            self.focused_panel = FocusedPanel::Search;
        }
        self.search.reset();
    }

    pub fn toggle_lyrics(&mut self) {
        if self.focused_panel == FocusedPanel::Lyrics {
            self.focused_panel = FocusedPanel::Library;
        } else {
            self.focused_panel = FocusedPanel::Lyrics;
            self.lyrics.reset();
            // Request lyrics for the currently playing track.
            if let Some(track_id) = self.logic.get_playing_track_id() {
                self.lyrics.start_loading(track_id.clone());
                self.logic.request_lyrics(&track_id);
            }
        }
    }

    pub fn toggle_logs(&mut self) {
        if self.focused_panel == FocusedPanel::Logs {
            self.focused_panel = FocusedPanel::Library;
        } else {
            self.focused_panel = FocusedPanel::Logs;
            self.logs.scroll_to_end();
        }
    }

    pub fn cycle_playback_mode(&mut self) {
        let next = blackbird_client_shared::next_playback_mode(self.logic.get_playback_mode());
        self.logic.set_playback_mode(next);
    }

    pub fn save_state(&self) {
        let mut config = self.config.clone();
        config.general.volume = self.logic.get_volume();
        if let Some(tap) = self.logic.get_playing_track_and_position() {
            config.shared.last_playback.track_id = Some(tap.track_id);
            config.shared.last_playback.track_position_secs = tap.position.as_secs_f64();
        }
        config.shared.last_playback.playback_mode = self.logic.get_playback_mode();
        config.shared.last_playback.sort_order = self.logic.get_sort_order();
        config.save();
    }

    pub fn adjust_volume(&mut self, delta: f32) {
        let vol = (self.logic.get_volume() + delta).clamp(0.0, 1.0);
        self.logic.set_volume(vol);
    }

    pub fn seek_relative(&mut self, seconds: i64) {
        if let Some(details) = self.logic.get_track_display_details() {
            let current = details.track_position;
            let delta = Duration::from_secs(seconds.unsigned_abs());
            let new_pos = if seconds > 0 {
                current + delta
            } else {
                current.saturating_sub(delta)
            };
            self.logic.seek_current(new_pos);
        }
    }
}
