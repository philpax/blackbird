use std::time::Duration;

use blackbird_client_shared::style as shared_style;
use blackbird_core::{self as bc, bs::StructuredLyrics, util::seconds_to_hms_string};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::keys::Action;

use super::StyleExt;

pub enum LyricsAction {
    ToggleLyrics,
    Quit,
    SeekRelative(i64),
}

/// TUI-specific lyrics view state wrapping the shared data state.
pub struct LyricsViewState {
    pub shared: blackbird_client_shared::lyrics::LyricsState,
    pub scroll_offset: usize,
    /// Keyboard-selected line index for scrubbing. `None` = auto-follow playback.
    pub selected_index: Option<usize>,
    /// Sidebar scroll state, separate from the full-panel scroll.
    pub sidebar_scroller: super::scroll::Scroller,
    /// Whether the user has manually scrolled the sidebar (disables auto-follow).
    pub sidebar_user_scrolled: bool,
    /// Total rendered row count in the sidebar, updated each draw. Used by
    /// mouse wheel handlers to compute correct scroll bounds.
    pub sidebar_total_rows: usize,
}

impl LyricsViewState {
    pub fn new() -> Self {
        Self {
            shared: blackbird_client_shared::lyrics::LyricsState::new(),
            scroll_offset: 0,
            selected_index: None,
            sidebar_scroller: super::scroll::Scroller::new(),
            sidebar_user_scrolled: false,
            sidebar_total_rows: 0,
        }
    }

    /// Resets the view-specific state (scroll and selection).
    pub fn reset_view(&mut self) {
        self.scroll_offset = 0;
        self.selected_index = None;
        self.reset_sidebar_view();
    }

    /// Resets the sidebar scroll state (e.g. on track change).
    pub fn reset_sidebar_view(&mut self) {
        self.sidebar_scroller = super::scroll::Scroller::new();
        self.sidebar_user_scrolled = false;
        self.sidebar_total_rows = 0;
    }
}

pub fn draw(
    frame: &mut Frame,
    lyrics: &LyricsViewState,
    style: &shared_style::Style,
    playing_position: Option<Duration>,
    area: Rect,
) {
    let block = Block::default()
        .title(" Lyrics ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(style.album_color()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if lyrics.shared.loading {
        let loading = Paragraph::new("Loading lyrics...")
            .style(Style::default().fg(style.track_duration_color()));
        frame.render_widget(loading, inner);
        return;
    }

    let Some(lyrics_data) = &lyrics.shared.data else {
        let msg = Paragraph::new("No lyrics available for this track.")
            .style(Style::default().fg(style.track_duration_color()));
        frame.render_widget(msg, inner);
        return;
    };

    if lyrics_data.line.is_empty() {
        let msg = Paragraph::new("No lyrics available for this track.")
            .style(Style::default().fg(style.track_duration_color()));
        frame.render_widget(msg, inner);
        return;
    }

    let current_line_idx =
        blackbird_client_shared::lyrics::find_current_lyrics_line(lyrics_data, playing_position);

    let selected_index = lyrics.selected_index;
    let track_name_hovered_color = style.track_name_hovered_color();

    // Pre-compute style colors to avoid borrow conflicts in closure.
    let text_color = style.text_color();
    let track_duration_color = style.track_duration_color();
    let track_name_playing_color = style.track_name_playing_color();

    let items: Vec<ListItem> = lyrics_data
        .line
        .iter()
        .enumerate()
        .map(|(idx, line)| {
            let is_current = lyrics_data.synced && idx == current_line_idx;
            let is_past = lyrics_data.synced && idx < current_line_idx;
            let is_selected = selected_index == Some(idx);

            let line_color = if is_selected {
                track_name_hovered_color
            } else if is_current {
                text_color
            } else if is_past {
                Color::Rgb(128, 128, 128)
            } else {
                Color::Rgb(180, 180, 180)
            };

            let mut spans = Vec::new();

            // Selection indicator
            if is_selected {
                spans.push(Span::styled(
                    "> ",
                    Style::default()
                        .fg(track_name_hovered_color)
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                spans.push(Span::raw("  "));
            }

            if let Some(start_ms) = line.start
                && !line.value.trim().is_empty()
            {
                let timestamp_secs = (start_ms / 1000) as u32;
                let timestamp_str = seconds_to_hms_string(timestamp_secs, false);
                let ts_color = if is_selected {
                    track_name_hovered_color
                } else if is_current {
                    track_name_playing_color
                } else {
                    track_duration_color
                };
                spans.push(Span::styled(
                    format!("{timestamp_str:>6} "),
                    Style::default().fg(ts_color),
                ));
            }

            let text_style = if is_selected || is_current {
                Style::default().fg(line_color).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(line_color)
            };

            spans.push(Span::styled(&line.value, text_style));

            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(items);

    let mut list_state = ListState::default();
    if lyrics_data.synced {
        // If the user has a keyboard selection, center on that; otherwise follow playback.
        let focus_line = selected_index.unwrap_or(current_line_idx);
        list_state.select(Some(focus_line));
        let visible_height = inner.height as usize;
        let offset = focus_line.saturating_sub(visible_height / 2);
        *list_state.offset_mut() = offset;
    } else {
        list_state.select(selected_index);
        *list_state.offset_mut() = lyrics.scroll_offset;
    }

    frame.render_stateful_widget(list, inner, &mut list_state);
}

pub fn handle_key(
    lyrics: &mut LyricsViewState,
    logic: &bc::Logic,
    action: Action,
) -> Option<LyricsAction> {
    match action {
        Action::Back => return Some(LyricsAction::ToggleLyrics),
        Action::Quit => return Some(LyricsAction::Quit),
        Action::MoveUp => move_selection(lyrics, logic.get_playing_position(), -1),
        Action::MoveDown => move_selection(lyrics, logic.get_playing_position(), 1),
        Action::PageUp => move_selection(
            lyrics,
            logic.get_playing_position(),
            -(super::layout::PAGE_SCROLL_SIZE as i32),
        ),
        Action::PageDown => move_selection(
            lyrics,
            logic.get_playing_position(),
            super::layout::PAGE_SCROLL_SIZE as i32,
        ),
        Action::Select => seek_to_selected(lyrics, logic),
        Action::SeekForward => {
            return Some(LyricsAction::SeekRelative(super::layout::SEEK_STEP_SECS));
        }
        Action::SeekBackward => {
            return Some(LyricsAction::SeekRelative(-super::layout::SEEK_STEP_SECS));
        }
        Action::PlayPause => logic.toggle_current(),
        Action::Next => logic.next(),
        Action::Previous => logic.previous(),
        Action::NextGroup => logic.next_group(),
        Action::PreviousGroup => logic.previous_group(),
        _ => {}
    }
    None
}

/// Handle click in the lyrics area — seek to the clicked line.
pub fn handle_mouse_click(
    lyrics: &mut LyricsViewState,
    logic: &bc::Logic,
    area: Rect,
    _x: u16,
    y: u16,
) {
    let Some(lyrics_data) = &lyrics.shared.data else {
        return;
    };
    if lyrics_data.line.is_empty() {
        return;
    }

    // The lyrics area has a border; the inner area starts 1 row below.
    let inner_y = area.y + 1;
    let inner_height = area.height.saturating_sub(2); // top + bottom border
    if y < inner_y || y >= inner_y + inner_height {
        return;
    }

    let row_in_list = (y - inner_y) as usize;

    // Determine the scroll offset that was used during rendering.
    let current_line_idx = blackbird_client_shared::lyrics::find_current_lyrics_line(
        lyrics_data,
        logic.get_playing_position(),
    );
    let scroll_offset = if lyrics_data.synced {
        if let Some(selected) = lyrics.selected_index {
            selected.saturating_sub(inner_height as usize / 2)
        } else {
            current_line_idx.saturating_sub(inner_height as usize / 2)
        }
    } else {
        lyrics.scroll_offset
    };

    let clicked_index = scroll_offset + row_in_list;
    if clicked_index < lyrics_data.line.len() {
        seek_to_line(lyrics, logic, clicked_index);
    }
}

/// Move the lyrics selection cursor by `delta` lines.
/// If no selection exists, starts from the current playing line.
pub fn move_selection(
    lyrics: &mut LyricsViewState,
    playing_position: Option<Duration>,
    delta: i32,
) {
    let line_count = lyrics
        .shared
        .data
        .as_ref()
        .map(|l| l.line.len())
        .unwrap_or(0);
    if line_count == 0 {
        return;
    }

    let current = lyrics.selected_index.unwrap_or_else(|| {
        lyrics
            .shared
            .data
            .as_ref()
            .map(|lyrics_data| {
                blackbird_client_shared::lyrics::find_current_lyrics_line(
                    lyrics_data,
                    playing_position,
                )
            })
            .unwrap_or(0)
    });

    let new_index = (current as i32 + delta).clamp(0, line_count as i32 - 1) as usize;
    lyrics.selected_index = Some(new_index);
}

/// Seek playback to the timestamp of the currently selected lyrics line.
pub fn seek_to_selected(lyrics: &mut LyricsViewState, logic: &bc::Logic) {
    let Some(selected) = lyrics.selected_index else {
        return;
    };
    let Some(lyrics_data) = &lyrics.shared.data else {
        return;
    };
    if let Some(line) = lyrics_data.line.get(selected)
        && let Some(start_ms) = line.start
    {
        logic.seek_current(Duration::from_millis(start_ms as u64));
        // Clear selection so the view returns to auto-follow.
        lyrics.selected_index = None;
    }
}

/// Seek playback to the timestamp of a lyrics line at the given index.
pub fn seek_to_line(lyrics: &mut LyricsViewState, logic: &bc::Logic, line_index: usize) {
    let Some(lyrics_data) = &lyrics.shared.data else {
        return;
    };
    if let Some(line) = lyrics_data.line.get(line_index)
        && let Some(start_ms) = line.start
    {
        logic.seek_current(Duration::from_millis(start_ms as u64));
        lyrics.selected_index = None;
    }
}

// ── Sidebar rendering and interaction ──────────────────────────────────────

/// Wraps a single lyric line's text into rows that fit within `max_width`
/// display columns. Returns a `Vec` of `String` rows.
///
/// Uses `unicode_width` to measure display width so wide characters (CJK, etc.)
/// are handled correctly.
pub fn wrap_lyric_line(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }
    let full_width = text.width();
    if full_width <= max_width {
        return vec![text.to_string()];
    }

    let mut rows = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;

    for word in text.split(' ') {
        let word_width = word.width();
        if current.is_empty() {
            // First word on the line.
            if word_width <= max_width {
                current = word.to_string();
                current_width = word_width;
            } else {
                // Word itself is wider than max_width — hard-break it.
                let mut remaining = word;
                while remaining.width() > max_width {
                    let mut taken = 0usize;
                    let mut chunk = String::new();
                    for ch in remaining.chars() {
                        let cw = ch.width().unwrap_or(0);
                        if taken + cw > max_width {
                            break;
                        }
                        chunk.push(ch);
                        taken += cw;
                    }
                    rows.push(chunk.clone());
                    remaining = &remaining[chunk.len()..];
                }
                if !remaining.is_empty() {
                    current = remaining.to_string();
                    current_width = remaining.width();
                }
            }
        } else if current_width + 1 + word_width <= max_width {
            current.push(' ');
            current.push_str(word);
            current_width += 1 + word_width;
        } else {
            rows.push(std::mem::take(&mut current));
            current = word.to_string();
            current_width = word_width;
        }
    }
    if !current.is_empty() {
        rows.push(current);
    }

    if rows.is_empty() {
        vec![text.to_string()]
    } else {
        rows
    }
}

/// Pre-wraps all lyric lines into a flat list of rendered rows, with a parallel
/// back-mapping from rendered row index to logical line index.
///
/// Each rendered row is a `Line` ready for `Paragraph`. The back-mapping array
/// lets click-to-seek convert a clicked Y coordinate (rendered row) back to the
/// logical lyric line it belongs to.
/// Width of the timestamp prefix (e.g. "1:23 ") in the sidebar.
const SIDEBAR_TIMESTAMP_WIDTH: usize = 7;

pub fn build_wrapped_lyrics(
    lyrics_data: &StructuredLyrics,
    current_line_idx: usize,
    max_width: usize,
    style: &shared_style::Style,
) -> (Vec<Line<'static>>, Vec<usize>) {
    let text_color = style.text_color();
    let track_duration_color = style.track_duration_color();
    let track_name_playing_color = style.track_name_playing_color();

    let mut lines = Vec::new();
    let mut back_mapping = Vec::new();

    for (idx, lyric_line) in lyrics_data.line.iter().enumerate() {
        let is_current = lyrics_data.synced && idx == current_line_idx;
        let is_past = lyrics_data.synced && idx < current_line_idx;

        let line_color = if is_current {
            text_color
        } else if is_past {
            Color::Rgb(128, 128, 128)
        } else {
            Color::Rgb(180, 180, 180)
        };

        let text_style = if is_current {
            Style::default().fg(line_color).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(line_color)
        };

        let display_text = if lyric_line.value.trim().is_empty() {
            "♪".to_string()
        } else {
            lyric_line.value.clone()
        };

        // Build the timestamp span for synced lines with a start time.
        let timestamp_span = if lyrics_data.synced {
            if let Some(start_ms) = lyric_line.start {
                let timestamp_secs = (start_ms / 1000) as u32;
                let timestamp_str = seconds_to_hms_string(timestamp_secs, false);
                let ts_color = if is_current {
                    track_name_playing_color
                } else {
                    track_duration_color
                };
                Some(Span::styled(
                    format!("{timestamp_str:>6} "),
                    Style::default().fg(ts_color),
                ))
            } else {
                // Synced line without a timestamp — reserve the space for alignment.
                Some(Span::raw("       "))
            }
        } else {
            None
        };

        // The first wrapped row gets the timestamp prefix, so it has less
        // width available for text.
        let first_row_width = max_width.saturating_sub(SIDEBAR_TIMESTAMP_WIDTH);
        let text_width = first_row_width.max(1);
        let wrapped_rows = wrap_lyric_line(&display_text, text_width);

        for (row_i, row_text) in wrapped_rows.iter().enumerate() {
            back_mapping.push(idx);
            if row_i == 0
                && let Some(ts) = &timestamp_span
            {
                lines.push(Line::from(vec![
                    ts.clone(),
                    Span::styled(row_text.clone(), text_style),
                ]));
            } else {
                // Continuation rows are indented to align with the text after
                // the timestamp.
                let indent = " ".repeat(SIDEBAR_TIMESTAMP_WIDTH);
                lines.push(Line::from(vec![
                    Span::raw(indent),
                    Span::styled(row_text.clone(), text_style),
                ]));
            }
        }
    }

    (lines, back_mapping)
}

/// Draws the lyrics sidebar into the given area.
///
/// The sidebar renders a bordered block with title " Lyrics ", then pre-wraps
/// each lyric line into rows that fit the inner width. The current line is
/// highlighted. A scrollbar is rendered if content overflows.
pub fn draw_sidebar(
    frame: &mut Frame,
    lyrics: &mut LyricsViewState,
    style: &shared_style::Style,
    playing_position: Option<Duration>,
    area: Rect,
) {
    let block = Block::default()
        .title(" Lyrics ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(style.album_color()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    if lyrics.shared.loading {
        let loading = Paragraph::new("Loading lyrics...")
            .style(Style::default().fg(style.track_duration_color()));
        frame.render_widget(loading, inner);
        return;
    }

    let Some(lyrics_data) = &lyrics.shared.data else {
        let msg = Paragraph::new("No lyrics available.")
            .style(Style::default().fg(style.track_duration_color()));
        frame.render_widget(msg, inner);
        return;
    };

    if lyrics_data.line.is_empty() {
        let msg = Paragraph::new("No lyrics available.")
            .style(Style::default().fg(style.track_duration_color()));
        frame.render_widget(msg, inner);
        return;
    }

    let current_line_idx =
        blackbird_client_shared::lyrics::find_current_lyrics_line(lyrics_data, playing_position);

    // Reserve 1 column for the scrollbar when content will overflow. This
    // prevents the scrollbar from overwriting the last character of lyric
    // lines that reach the full inner width.
    let max_width = (inner.width as usize).saturating_sub(1);

    let (wrapped_lines, back_mapping) =
        build_wrapped_lyrics(lyrics_data, current_line_idx, max_width, style);

    let total_rows = wrapped_lines.len();

    // Store the total rendered row count for mouse wheel scroll bounds.
    lyrics.sidebar_total_rows = total_rows;

    // Update the scroller's visible height for correct bounds computation.
    lyrics.sidebar_scroller.visible_height = inner.height as usize;

    // Auto-follow: scroll to keep the current line visible, unless the user
    // has manually scrolled.
    if lyrics_data.synced && !lyrics.sidebar_user_scrolled {
        // Find the first rendered row of the current logical line.
        let current_row = back_mapping
            .iter()
            .position(|&line_idx| line_idx == current_line_idx)
            .unwrap_or(0);
        // Center the current line in the viewport.
        let target = current_row.saturating_sub(lyrics.sidebar_scroller.visible_height / 2);
        lyrics.sidebar_scroller.line = target;
    }

    lyrics.sidebar_scroller.clamp(total_rows);

    let scroll_offset = lyrics.sidebar_scroller.line as u16;

    let paragraph = Paragraph::new(wrapped_lines).scroll((scroll_offset, 0));

    frame.render_widget(paragraph, inner);

    // Render scrollbar if content overflows.
    if lyrics.sidebar_scroller.needs_scrollbar(total_rows) {
        lyrics.sidebar_scroller.render_scrollbar(
            frame,
            inner,
            total_rows,
            style.track_duration_color(),
            style.track_name_playing_color(),
        );
    }
}

/// Pure function: maps a click Y coordinate to a logical lyric line index.
///
/// `y` is the absolute terminal row. `inner_area` is the inner rect of the
/// sidebar (after border). `scroll_offset` is the current scroll position in
/// rendered rows. `back_mapping` maps rendered row index → logical line index.
pub fn sidebar_click_to_line_index(
    y: u16,
    inner_area: Rect,
    scroll_offset: usize,
    back_mapping: &[usize],
) -> Option<usize> {
    if y < inner_area.y || y >= inner_area.y + inner_area.height {
        return None;
    }
    let row_in_view = (y - inner_area.y) as usize;
    let rendered_row = row_in_view + scroll_offset;
    back_mapping.get(rendered_row).copied()
}

/// Pure function: returns the seek duration for a given logical line index.
pub fn line_index_to_duration(
    lyrics_data: &StructuredLyrics,
    line_index: usize,
) -> Option<Duration> {
    let line = lyrics_data.line.get(line_index)?;
    let start_ms = line.start?;
    Some(Duration::from_millis(start_ms as u64))
}

/// Pure function: returns the `LyricsDisplay` to switch to when the inline
/// overlay is clicked. Clicking the inline overlay switches to sidebar mode
/// (Right).
pub fn inline_overlay_click_switches_to_sidebar() -> blackbird_client_shared::config::LyricsDisplay
{
    blackbird_client_shared::config::LyricsDisplay::Right
}

/// Pure function: computes the new sidebar width from a drag position.
///
/// For a left sidebar, `sidebar_outer_x` is the sidebar's left edge X coordinate,
/// and the width grows as the mouse moves right (`mouse_x - sidebar_outer_x`).
/// For a right sidebar, `sidebar_outer_x` is the sidebar's right edge X coordinate,
/// and the width grows as the mouse moves left (`sidebar_outer_x - mouse_x`).
/// The result is clamped to `[min, max]`.
pub fn compute_sidebar_width_from_drag(
    mouse_x: u16,
    sidebar_outer_x: u16,
    is_left: bool,
    min: u16,
    max: u16,
) -> u16 {
    let raw_width = if is_left {
        mouse_x.saturating_sub(sidebar_outer_x)
    } else {
        sidebar_outer_x.saturating_sub(mouse_x)
    };
    raw_width.clamp(min, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_lyrics(synced: bool, lines: &[(Option<i64>, &str)]) -> StructuredLyrics {
        StructuredLyrics {
            display_artist: None,
            display_title: None,
            lang: None,
            offset: None,
            synced,
            line: lines
                .iter()
                .map(|(start, value)| blackbird_core::bs::LyricLine {
                    start: *start,
                    value: value.to_string(),
                })
                .collect(),
        }
    }

    #[test]
    fn wrap_short_line_returns_single_row() {
        let rows = wrap_lyric_line("hello", 20);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0], "hello");
    }

    #[test]
    fn wrap_long_line_splits_at_word_boundaries() {
        let rows = wrap_lyric_line("one two three four five six", 10);
        assert!(rows.len() > 1);
        // Each row should fit within the max width.
        for row in &rows {
            assert!(row.width() <= 10, "row '{row}' exceeds max width");
        }
    }

    #[test]
    fn wrap_empty_text_returns_single_row() {
        let rows = wrap_lyric_line("", 10);
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn wrap_very_long_word_hard_breaks() {
        let rows = wrap_lyric_line("abcdefghijklmnopqrstuvwxyz", 5);
        assert!(rows.len() > 1);
        for row in &rows {
            assert!(row.width() <= 5, "row '{row}' exceeds max width");
        }
    }

    #[test]
    fn sidebar_click_maps_to_correct_line() {
        // 3 logical lines, each 1 row. Back-mapping: [0, 1, 2].
        let back_mapping = vec![0, 1, 2];
        let inner = Rect::new(0, 1, 20, 10);
        // Click on row 0 (y=1) with no scroll → line 0.
        assert_eq!(
            sidebar_click_to_line_index(1, inner, 0, &back_mapping),
            Some(0)
        );
        // Click on row 1 (y=2) → line 1.
        assert_eq!(
            sidebar_click_to_line_index(2, inner, 0, &back_mapping),
            Some(1)
        );
        // Click on row 2 (y=3) → line 2.
        assert_eq!(
            sidebar_click_to_line_index(3, inner, 0, &back_mapping),
            Some(2)
        );
    }

    #[test]
    fn sidebar_click_with_scroll_offset() {
        // 5 logical lines, each 1 row. Back-mapping: [0, 1, 2, 3, 4].
        let back_mapping = vec![0, 1, 2, 3, 4];
        let inner = Rect::new(0, 1, 20, 3);
        // Scroll offset = 2, so visible rows are 2, 3, 4.
        // Click on y=1 (first visible row) → rendered row 2 → line 2.
        assert_eq!(
            sidebar_click_to_line_index(1, inner, 2, &back_mapping),
            Some(2)
        );
        // Click on y=3 (third visible row) → rendered row 4 → line 4.
        assert_eq!(
            sidebar_click_to_line_index(3, inner, 2, &back_mapping),
            Some(4)
        );
    }

    #[test]
    fn sidebar_click_with_wrapped_lines() {
        // Line 0 wraps to 2 rows, line 1 is 1 row.
        // Back-mapping: [0, 0, 1].
        let back_mapping = vec![0, 0, 1];
        let inner = Rect::new(0, 1, 20, 10);
        // Click on y=1 → rendered row 0 → line 0.
        assert_eq!(
            sidebar_click_to_line_index(1, inner, 0, &back_mapping),
            Some(0)
        );
        // Click on y=2 → rendered row 1 → line 0 (second wrapped row).
        assert_eq!(
            sidebar_click_to_line_index(2, inner, 0, &back_mapping),
            Some(0)
        );
        // Click on y=3 → rendered row 2 → line 1.
        assert_eq!(
            sidebar_click_to_line_index(3, inner, 0, &back_mapping),
            Some(1)
        );
    }

    #[test]
    fn sidebar_click_outside_area_returns_none() {
        let back_mapping = vec![0, 1];
        let inner = Rect::new(0, 1, 20, 3);
        // Click above the inner area.
        assert_eq!(
            sidebar_click_to_line_index(0, inner, 0, &back_mapping),
            None
        );
        // Click below the inner area.
        assert_eq!(
            sidebar_click_to_line_index(4, inner, 0, &back_mapping),
            None
        );
    }

    #[test]
    fn line_index_to_duration_returns_correct_time() {
        let lyrics = make_lyrics(true, &[(Some(5000), "first"), (Some(10000), "second")]);
        assert_eq!(
            line_index_to_duration(&lyrics, 0),
            Some(Duration::from_millis(5000))
        );
        assert_eq!(
            line_index_to_duration(&lyrics, 1),
            Some(Duration::from_millis(10000))
        );
        // Out of bounds.
        assert_eq!(line_index_to_duration(&lyrics, 5), None);
    }

    #[test]
    fn line_index_to_duration_returns_none_for_unsynced() {
        let lyrics = make_lyrics(true, &[(None, "no timestamp")]);
        assert_eq!(line_index_to_duration(&lyrics, 0), None);
    }

    #[test]
    fn inline_click_switches_to_right() {
        use blackbird_client_shared::config::LyricsDisplay;
        assert_eq!(
            inline_overlay_click_switches_to_sidebar(),
            LyricsDisplay::Right
        );
    }

    #[test]
    fn compute_width_from_drag_right_sidebar() {
        // Right sidebar: width = right_edge - mouse_x.
        // right_edge = 80, mouse_x = 50 → width = 30.
        assert_eq!(compute_sidebar_width_from_drag(50, 80, false, 10, 40), 30);
        // Clamp to min.
        assert_eq!(compute_sidebar_width_from_drag(75, 80, false, 10, 40), 10);
        // Clamp to max.
        assert_eq!(compute_sidebar_width_from_drag(10, 80, false, 10, 40), 40);
    }

    #[test]
    fn compute_width_from_drag_left_sidebar() {
        // Left sidebar: width = mouse_x - left_edge.
        // left_edge = 0, mouse_x = 30 → width = 30.
        assert_eq!(compute_sidebar_width_from_drag(30, 0, true, 10, 40), 30);
        // Clamp to min.
        assert_eq!(compute_sidebar_width_from_drag(5, 0, true, 10, 40), 10);
        // Clamp to max.
        assert_eq!(compute_sidebar_width_from_drag(50, 0, true, 10, 40), 40);
    }
}
