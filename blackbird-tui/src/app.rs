use std::time::Duration;

use blackbird_core as bc;
use blackbird_core::{PlaybackMode, PlaybackToLogicMessage, blackbird_state::TrackId};

use crate::config::Config;
use crate::cover_art::CoverArtCache;
use crate::log_buffer::LogBuffer;

/// Which panel/mode the UI is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusedPanel {
    Library,
    Search,
    Lyrics,
    Logs,
}

/// A single entry in the flat library list.
#[derive(Debug, Clone)]
pub enum LibraryEntry {
    GroupHeader {
        artist: String,
        album: String,
        year: Option<i32>,
        duration: u32,
        starred: bool,
        album_id: blackbird_core::blackbird_state::AlbumId,
        cover_art_id: Option<blackbird_core::blackbird_state::CoverArtId>,
    },
    Track {
        id: TrackId,
        title: String,
        artist: Option<String>,
        album_artist: String,
        track_number: Option<u32>,
        disc_number: Option<u32>,
        duration: Option<u32>,
        starred: bool,
        play_count: Option<u64>,
    },
}

pub struct App {
    pub logic: bc::Logic,
    pub config: Config,
    pub cover_art_cache: CoverArtCache,
    pub playback_to_logic_rx: bc::PlaybackToLogicRx,
    pub lyrics_loaded_rx: std::sync::mpsc::Receiver<bc::LyricsData>,
    pub library_populated_rx: std::sync::mpsc::Receiver<()>,

    // UI state
    pub focused_panel: FocusedPanel,
    pub library_scroll_offset: usize,
    pub library_selected_index: usize,
    pub library_needs_scroll_to_playing: bool,

    // Cached flat library for performance.
    cached_flat_library: Vec<LibraryEntry>,
    flat_library_dirty: bool,

    // Search state
    pub search_query: String,
    pub search_results: Vec<TrackId>,
    pub search_selected_index: usize,

    // Lyrics state
    pub lyrics_track_id: Option<TrackId>,
    pub lyrics_data: Option<bc::bs::StructuredLyrics>,
    pub lyrics_loading: bool,
    pub lyrics_scroll_offset: usize,

    // Volume adjustment mode
    pub volume_editing: bool,

    // Track that should be scrolled into view.
    pub scroll_to_track: Option<TrackId>,

    // Logs state
    pub log_buffer: LogBuffer,
    pub logs_scroll_offset: usize,

    pub should_quit: bool,
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
            library_scroll_offset: 0,
            library_selected_index: 0,
            library_needs_scroll_to_playing: true,

            cached_flat_library: Vec::new(),
            flat_library_dirty: true,

            search_query: String::new(),
            search_results: Vec::new(),
            search_selected_index: 0,

            lyrics_track_id: None,
            lyrics_data: None,
            lyrics_loading: false,
            lyrics_scroll_offset: 0,

            volume_editing: false,

            scroll_to_track: None,

            log_buffer,
            logs_scroll_offset: 0,

            should_quit: false,
        }
    }

    pub fn tick(&mut self) {
        self.logic.update();
        self.cover_art_cache.update();

        // Process playback events.
        while let Ok(event) = self.playback_to_logic_rx.try_recv() {
            if let PlaybackToLogicMessage::TrackStarted(tap) = event {
                self.scroll_to_track = Some(tap.track_id.clone());
                self.library_needs_scroll_to_playing = false;

                // Auto-request lyrics if the lyrics panel is open.
                if self.focused_panel == FocusedPanel::Lyrics {
                    self.lyrics_track_id = Some(tap.track_id.clone());
                    self.lyrics_loading = true;
                    self.lyrics_data = None;
                    self.logic.request_lyrics(&tap.track_id);
                }
            }
        }

        // Process lyrics data.
        while let Ok(lyrics_data) = self.lyrics_loaded_rx.try_recv() {
            if Some(&lyrics_data.track_id) == self.lyrics_track_id.as_ref() {
                self.lyrics_data = lyrics_data.lyrics;
                self.lyrics_loading = false;
            }
        }

        // Process library population.
        while let Ok(()) = self.library_populated_rx.try_recv() {
            self.flat_library_dirty = true;
            if self.library_needs_scroll_to_playing
                && let Some(track_id) = self.logic.get_playing_track_id()
            {
                self.scroll_to_track = Some(track_id);
            }
            // Ensure selection is on a track, not a group header.
            self.ensure_selection_on_track();
        }

        // Handle scroll-to-track.
        if let Some(track_id) = self.scroll_to_track.take() {
            let state = self.logic.get_state();
            let state = state.read().unwrap();
            if let Some(index) = self.find_flat_index_for_track(&state, &track_id) {
                self.library_selected_index = index;
            } else {
                // Track not in library yet, re-queue.
                self.scroll_to_track = Some(track_id);
            }
        }

        if self.logic.should_shutdown() {
            self.should_quit = true;
        }
    }

    /// Returns the cached flat library, rebuilding if needed.
    pub fn get_flat_library(&mut self) -> &[LibraryEntry] {
        if self.flat_library_dirty {
            self.rebuild_flat_library();
            self.flat_library_dirty = false;
        }
        &self.cached_flat_library
    }

    /// Returns the length of the flat library without requiring mutable access.
    pub fn flat_library_len(&self) -> usize {
        self.cached_flat_library.len()
    }

    /// Returns a clone of the entry at the given index, if it exists.
    pub fn get_library_entry(&mut self, index: usize) -> Option<LibraryEntry> {
        if self.flat_library_dirty {
            self.rebuild_flat_library();
            self.flat_library_dirty = false;
        }
        self.cached_flat_library.get(index).cloned()
    }

    /// Rebuilds the cached flat library from the current state.
    fn rebuild_flat_library(&mut self) {
        let state = self.logic.get_state();
        let state = state.read().unwrap();

        self.cached_flat_library.clear();
        for group in &state.library.groups {
            self.cached_flat_library.push(LibraryEntry::GroupHeader {
                artist: group.artist.to_string(),
                album: group.album.to_string(),
                year: group.year,
                duration: group.duration,
                starred: group.starred,
                album_id: group.album_id.clone(),
                cover_art_id: group.cover_art_id.clone(),
            });

            for track_id in &group.tracks {
                if let Some(track) = state.library.track_map.get(track_id) {
                    self.cached_flat_library.push(LibraryEntry::Track {
                        id: track.id.clone(),
                        title: track.title.to_string(),
                        artist: track.artist.as_ref().map(|a| a.to_string()),
                        album_artist: group.artist.to_string(),
                        track_number: track.track,
                        disc_number: track.disc_number,
                        duration: track.duration,
                        starred: track.starred,
                        play_count: track.play_count,
                    });
                }
            }
        }
    }

    pub fn toggle_search(&mut self) {
        if self.focused_panel == FocusedPanel::Search {
            self.focused_panel = FocusedPanel::Library;
            self.search_query.clear();
            self.search_results.clear();
        } else {
            self.focused_panel = FocusedPanel::Search;
            self.search_query.clear();
            self.search_results.clear();
            self.search_selected_index = 0;
        }
    }

    pub fn toggle_lyrics(&mut self) {
        if self.focused_panel == FocusedPanel::Lyrics {
            self.focused_panel = FocusedPanel::Library;
        } else {
            self.focused_panel = FocusedPanel::Lyrics;
            self.lyrics_scroll_offset = 0;
            // Request lyrics for the currently playing track.
            if let Some(track_id) = self.logic.get_playing_track_id() {
                self.lyrics_track_id = Some(track_id.clone());
                self.lyrics_loading = true;
                self.lyrics_data = None;
                self.logic.request_lyrics(&track_id);
            }
        }
    }

    pub fn toggle_logs(&mut self) {
        if self.focused_panel == FocusedPanel::Logs {
            self.focused_panel = FocusedPanel::Library;
        } else {
            self.focused_panel = FocusedPanel::Logs;
            // Scroll to the end of the log buffer.
            let len = self.log_buffer.len();
            self.logs_scroll_offset = len.saturating_sub(1);
        }
    }

    pub fn update_search(&mut self) {
        if self.search_query.len() >= 3 {
            let state = self.logic.get_state();
            let mut state = state.write().unwrap();
            self.search_results = state.library.search(&self.search_query);
            self.search_selected_index = 0;
        } else {
            self.search_results.clear();
        }
    }

    pub fn cycle_playback_mode(&mut self) {
        let current = self.logic.get_playback_mode();
        let next = match current {
            PlaybackMode::Sequential => PlaybackMode::RepeatOne,
            PlaybackMode::RepeatOne => PlaybackMode::GroupRepeat,
            PlaybackMode::GroupRepeat => PlaybackMode::Shuffle,
            PlaybackMode::Shuffle => PlaybackMode::LikedShuffle,
            PlaybackMode::LikedShuffle => PlaybackMode::GroupShuffle,
            PlaybackMode::GroupShuffle => PlaybackMode::LikedGroupShuffle,
            PlaybackMode::LikedGroupShuffle => PlaybackMode::Sequential,
        };
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

    /// Finds the flat index for a given track in the library.
    fn find_flat_index_for_track(
        &self,
        state: &bc::AppState,
        target_track_id: &TrackId,
    ) -> Option<usize> {
        let mut index = 0;
        for group in &state.library.groups {
            index += 1; // group header
            for track_id in &group.tracks {
                if track_id == target_track_id {
                    return Some(index);
                }
                if state.library.track_map.contains_key(track_id) {
                    index += 1;
                }
            }
        }
        None
    }

    /// Ensures the current selection is on a track, not a group header.
    /// If currently on a header, moves to the first track in the library.
    pub fn ensure_selection_on_track(&mut self) {
        if self.flat_library_dirty {
            self.rebuild_flat_library();
            self.flat_library_dirty = false;
        }

        // Check if current selection is already a track.
        if let Some(LibraryEntry::Track { .. }) =
            self.cached_flat_library.get(self.library_selected_index)
        {
            return;
        }

        // Find the first track in the library.
        for (i, entry) in self.cached_flat_library.iter().enumerate() {
            if let LibraryEntry::Track { .. } = entry {
                self.library_selected_index = i;
                return;
            }
        }
    }
}
