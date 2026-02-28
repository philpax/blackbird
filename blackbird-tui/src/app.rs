use std::time::{Duration, Instant};

use blackbird_core::{self as bc, PlaybackToLogicMessage};

use crate::{
    config::Config,
    cover_art::CoverArtCache,
    keys,
    log_buffer::LogBuffer,
    ui::{
        album_art_overlay::AlbumArtOverlay, library::LibraryState, logs::LogsState,
        lyrics::LyricsViewState, queue::QueueState, search::SearchState,
    },
};

/// Which panel/mode the UI is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusedPanel {
    Library,
    Search,
    Lyrics,
    Logs,
    Queue,
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
    /// Whether the playback mode dropdown is open.
    pub playback_mode_dropdown: bool,
    /// Clickable regions in the help bar: (x_start, x_end, action).
    pub help_bar_items: Vec<(u16, u16, keys::Action)>,
    /// Monotonically increasing tick counter for animations.
    pub tick_count: u64,

    // Config auto-reload
    last_config_check: Instant,

    // Per-view state (owned by their respective modules)
    pub library: LibraryState,
    pub search: SearchState,
    pub lyrics: LyricsViewState,
    pub logs: LogsState,
    pub queue: QueueState,
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

            last_config_check: Instant::now(),

            focused_panel: FocusedPanel::Library,
            volume_editing: false,
            quit_confirming: false,
            should_quit: false,
            needs_redraw: true,
            mouse_position: None,
            album_art_overlay: None,
            playback_mode_dropdown: false,
            help_bar_items: Vec::new(),
            tick_count: 0,

            library: LibraryState::new(),
            search: SearchState::new(),
            lyrics: LyricsViewState::new(),
            logs: LogsState::new(log_buffer),
            queue: QueueState::new(),
        }
    }

    pub fn tick(&mut self) {
        self.tick_count = self.tick_count.wrapping_add(1);
        self.logic.update();
        self.cover_art_cache.update();
        self.cover_art_cache
            .preload_next_track_surrounding_art(&self.logic);
        self.cover_art_cache.tick_prefetch(&self.logic);

        // Process playback events.
        while let Ok(event) = self.playback_to_logic_rx.try_recv() {
            if let PlaybackToLogicMessage::TrackStarted(tap) = event {
                self.library.scroll_to_track = Some(tap.track_id.clone());
                self.library.needs_scroll_to_playing = false;

                // Request lyrics if inline lyrics are enabled or the panel is open.
                let panel_open = self.focused_panel == FocusedPanel::Lyrics;
                if self.lyrics.shared.on_track_started(
                    &tap.track_id,
                    self.config.shared.show_inline_lyrics,
                    panel_open,
                ) {
                    self.logic.request_lyrics(&tap.track_id);
                }
            }
        }

        // Process lyrics data.
        while let Ok(lyrics_data) = self.lyrics_loaded_rx.try_recv() {
            self.lyrics.shared.on_lyrics_loaded(&lyrics_data);
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

            // Populate the background art prefetch queue with all album cover art IDs.
            let state = self.logic.get_state();
            let state = state.read().unwrap();
            let ids: Vec<_> = state
                .library
                .groups
                .iter()
                .filter_map(|g| g.cover_art_id.clone())
                .collect();
            drop(state);
            self.cover_art_cache.populate_prefetch_queue(ids);
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

        // Reload config from disk if changed (check once per second).
        if self.last_config_check.elapsed() >= Duration::from_secs(1) {
            self.last_config_check = Instant::now();
            let new_config = Config::load();
            if new_config != self.config {
                self.config = new_config;
                self.config.save();
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
            self.lyrics.reset_view();
            // Request lyrics if not already loaded for the current track.
            let playing_id = self.logic.get_playing_track_id();
            if self.lyrics.shared.on_panel_opened(playing_id.as_ref())
                && let Some(track_id) = playing_id
            {
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

    pub fn toggle_queue(&mut self) {
        if self.focused_panel == FocusedPanel::Queue {
            self.focused_panel = FocusedPanel::Library;
        } else {
            self.focused_panel = FocusedPanel::Queue;
            self.queue.reset();
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
