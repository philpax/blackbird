use std::time::Duration;

use blackbird_core::bs::StructuredLyrics;

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
