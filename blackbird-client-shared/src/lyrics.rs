use std::time::Duration;

use blackbird_core::{LyricsData, blackbird_state::TrackId, bs::StructuredLyrics};

/// Find the index of the current lyrics line based on playback position.
/// Returns 0 for unsynced lyrics or if no line matches.
pub fn find_current_lyrics_line(
    lyrics: &StructuredLyrics,
    playback_position: Option<Duration>,
) -> usize {
    if !lyrics.synced {
        return 0;
    }
    let current_ms = playback_position.map(|d| d.as_millis() as i64).unwrap_or(0);
    let adjusted_ms = current_ms + lyrics.offset.unwrap_or(0);
    lyrics
        .line
        .iter()
        .enumerate()
        .rev()
        .find(|(_, line)| line.start.unwrap_or(0) <= adjusted_ms)
        .map(|(idx, _)| idx)
        .unwrap_or(0)
}

/// Shared lyrics data state used by both the egui and TUI clients.
///
/// Centralizes lyrics data management and fetch-decision logic so that both
/// clients share the same behavior for when to request lyrics and how to
/// store the result.
#[derive(Default)]
pub struct LyricsState {
    /// The track ID for which lyrics are currently loaded or being loaded.
    pub track_id: Option<TrackId>,
    /// The loaded lyrics data, if available.
    pub data: Option<StructuredLyrics>,
    /// Whether a lyrics fetch is currently in progress.
    pub loading: bool,
}

impl LyricsState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Called when a new track starts playing. Updates internal state and
    /// returns `true` if `Logic::request_lyrics` should be called.
    pub fn on_track_started(
        &mut self,
        track_id: &TrackId,
        show_inline_lyrics: bool,
        panel_open: bool,
    ) -> bool {
        if !show_inline_lyrics && !panel_open {
            return false;
        }
        self.track_id = Some(track_id.clone());
        self.loading = true;
        self.data = None;
        true
    }

    /// Called when lyrics data has been loaded from the server.
    pub fn on_lyrics_loaded(&mut self, loaded: &LyricsData) {
        if self.track_id.as_ref() == Some(&loaded.track_id) {
            self.data = loaded.lyrics.clone();
            self.loading = false;
        }
    }

    /// Called when the lyrics panel is opened. Returns `true` if
    /// `Logic::request_lyrics` should be called (i.e. no data is already
    /// loaded or loading for the current track).
    pub fn on_panel_opened(&mut self, playing_track_id: Option<&TrackId>) -> bool {
        let Some(playing_id) = playing_track_id else {
            return false;
        };
        // Already loaded or loading for this track.
        if self.track_id.as_ref() == Some(playing_id) {
            return false;
        }
        self.track_id = Some(playing_id.clone());
        self.loading = true;
        self.data = None;
        true
    }

    /// Returns `true` if loaded lyrics are synced and non-empty, meaning the
    /// inline lyrics block should be visible.
    pub fn has_synced_lyrics(&self) -> bool {
        self.data
            .as_ref()
            .is_some_and(|l| l.synced && !l.line.is_empty())
    }

    /// Returns the current synced lyric line, or `None` if the current line's
    /// text is empty (instrumental break, etc.). Callers should check
    /// [`has_synced_lyrics`](Self::has_synced_lyrics) first to decide whether
    /// to show the inline lyrics block at all.
    pub fn current_inline_line(
        &self,
        position: Option<Duration>,
    ) -> Option<&blackbird_core::bs::LyricLine> {
        let lyrics = self.data.as_ref()?;
        if !lyrics.synced || lyrics.line.is_empty() {
            return None;
        }
        position?;
        let idx = find_current_lyrics_line(lyrics, position);
        let line = &lyrics.line[idx];
        if line.value.trim().is_empty() {
            return None;
        }
        Some(line)
    }
}
