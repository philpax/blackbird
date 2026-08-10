use std::time::Duration;

use blackbird_client_shared::style as shared_style;
use blackbird_core::{self as bc, bs::StructuredLyrics, util::seconds_to_hms_string};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::keys::Action;

use super::ToColor;

pub enum LyricsAction {
    ToggleLyrics,
    Quit,
    SeekRelative(i64),
}

/// TUI-specific lyrics view state wrapping the shared data state.
///
/// Both the full-panel and sidebar views share this state: a single `Scroller`
/// for scroll position, a `selected_index` for keyboard selection, and flags
/// for auto-follow behavior.
pub struct LyricsViewState {
    pub shared: blackbird_client_shared::lyrics::LyricsState,
    /// Keyboard-selected line index for scrubbing. `None` = auto-follow playback.
    pub selected_index: Option<usize>,
    /// Shared scroll state for both the full-panel and sidebar views.
    pub scroller: super::scroll::Scroller,
    /// Whether the user has manually scrolled (disables auto-follow).
    pub user_scrolled: bool,
    /// Total rendered row count, updated each draw. Used by mouse wheel handlers
    /// to compute correct scroll bounds.
    pub total_rows: usize,
}

impl LyricsViewState {
    pub fn new() -> Self {
        Self {
            shared: blackbird_client_shared::lyrics::LyricsState::new(),
            selected_index: None,
            scroller: super::scroll::Scroller::new(),
            user_scrolled: false,
            total_rows: 0,
        }
    }

    /// Resets all view-specific state (scroll, selection, auto-follow).
    pub fn reset_view(&mut self) {
        self.selected_index = None;
        self.scroller = super::scroll::Scroller::new();
        self.user_scrolled = false;
        self.total_rows = 0;
    }
}

/// Draws the lyrics component into its full sub-area (including its own
/// border). Used by the sidebar and panel renderers.
pub fn draw_lyrics_component(
    frame: &mut Frame,
    lyrics: &mut LyricsViewState,
    style: &shared_style::Style,
    playing_position: Option<Duration>,
    area: Rect,
    is_focused: bool,
    mouse: Option<(u16, u16)>,
) {
    draw_lyrics_content(
        frame,
        lyrics,
        style,
        playing_position,
        area,
        is_focused,
        mouse,
    );
}

/// Unified rendering pipeline for both the full-panel and sidebar lyrics views.
///
/// Renders a bordered block with title " Lyrics ", handles loading/empty states,
/// pre-wraps lyric lines via [`build_wrapped_lyrics`], manages scroll state
/// via a shared [`Scroller`], supports keyboard selection (`selected_index`),
/// auto-follows playback unless the user has manually scrolled or selected,
/// and renders a scrollbar when content overflows.
fn draw_lyrics_content(
    frame: &mut Frame,
    lyrics: &mut LyricsViewState,
    style: &shared_style::Style,
    playing_position: Option<Duration>,
    area: Rect,
    is_focused: bool,
    mouse: Option<(u16, u16)>,
) {
    let block = Block::default()
        .title(" Lyrics ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(style.sidebar.lyrics_border().to_color()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    if lyrics.shared.loading {
        let loading = Paragraph::new("Loading lyrics...")
            .style(Style::default().fg(style.library.track_duration().to_color()));
        frame.render_widget(loading, inner);
        return;
    }

    let Some(lyrics_data) = &lyrics.shared.data else {
        let msg = Paragraph::new("No lyrics available for this track.")
            .style(Style::default().fg(style.library.track_duration().to_color()));
        frame.render_widget(msg, inner);
        return;
    };

    if lyrics_data.line.is_empty() {
        let msg = Paragraph::new("No lyrics available for this track.")
            .style(Style::default().fg(style.library.track_duration().to_color()));
        frame.render_widget(msg, inner);
        return;
    }

    let current_line_idx =
        blackbird_client_shared::lyrics::find_current_lyrics_line(lyrics_data, playing_position);

    // Reserve 1 column for the scrollbar when content will overflow. This
    // prevents the scrollbar from overwriting the last character of lyric
    // lines that reach the full inner width.
    let max_width = (inner.width as usize).saturating_sub(1);

    let (mut wrapped_lines, back_mapping) = build_wrapped_lyrics(
        lyrics_data,
        current_line_idx,
        max_width,
        style,
        lyrics.selected_index,
        is_focused,
        None,
    );

    // Determine which lyric line the mouse is hovering over, for the hover
    // underline. The mouse row maps to a rendered row (scroll offset + row in
    // the inner area), then back-maps to a logical line. The hovered line's
    // rendered rows are then re-styled with the underline modifier.
    let hovered_line = mouse.and_then(|(mx, my)| {
        if mx < inner.x || mx >= inner.x + inner.width {
            return None;
        }
        if my < inner.y || my >= inner.y + inner.height {
            return None;
        }
        let rendered_row = lyrics.scroller.line + (my - inner.y) as usize;
        back_mapping.get(rendered_row).copied()
    });
    if let Some(hovered_line) = hovered_line {
        for (row, &line_idx) in back_mapping.iter().enumerate() {
            if line_idx == hovered_line
                && lyrics.selected_index != Some(hovered_line)
                && let Some(line) = wrapped_lines.get_mut(row)
            {
                for span in &mut line.spans {
                    span.style = span.style.add_modifier(Modifier::UNDERLINED);
                }
            }
        }
    }

    let total_rows = wrapped_lines.len();

    // Store the total rendered row count for mouse wheel scroll bounds.
    lyrics.total_rows = total_rows;

    // Update the scroller's visible height for correct bounds computation.
    lyrics.scroller.visible_height = inner.height as usize;

    // Auto-follow: scroll to keep the current line visible, unless the user
    // has manually scrolled or has an active selection.
    let auto_follow = lyrics_data.synced && !lyrics.user_scrolled;
    if auto_follow {
        // When the user has a keyboard selection, center on that; otherwise
        // follow playback.
        let focus_line_idx = lyrics.selected_index.unwrap_or(current_line_idx);
        // Find the first rendered row of the focus line.
        let current_row = back_mapping
            .iter()
            .position(|&line_idx| line_idx == focus_line_idx)
            .unwrap_or(0);
        // Center the focus line in the viewport.
        let target = current_row.saturating_sub(lyrics.scroller.visible_height / 2);
        lyrics.scroller.line = target;
    }

    lyrics.scroller.clamp(total_rows);

    let scroll_offset = lyrics.scroller.line as u16;

    let paragraph = Paragraph::new(wrapped_lines).scroll((scroll_offset, 0));

    frame.render_widget(paragraph, inner);

    // Render scrollbar if content overflows.
    if lyrics.scroller.needs_scrollbar(total_rows) {
        lyrics.scroller.render_scrollbar(
            frame,
            inner,
            total_rows,
            style.library.track_duration().to_color(),
            style.library.track_name_playing().to_color(),
        );
    }
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

/// Unified click handler for both the full-panel and sidebar lyrics views.
///
/// Uses the back-mapping approach: computes the inner area, gets the scroll
/// offset from the shared `Scroller`, rebuilds the back-mapping, and converts
/// the clicked row to a logical line index → seek duration.
///
/// The `area` parameter is the full rect (including border) of the lyrics
/// view, and `y` is the absolute terminal row of the click.
pub fn handle_mouse_click(
    lyrics: &mut LyricsViewState,
    logic: &bc::Logic,
    style: &shared_style::Style,
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

    // The lyrics area has a border; the inner area starts 1 row below and
    // 1 column in.
    let inner = Rect::new(
        area.x + 1,
        area.y + 1,
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    );
    if inner.height == 0 || inner.width == 0 {
        return;
    }

    if y < inner.y || y >= inner.y + inner.height {
        return;
    }

    let scroll_offset = lyrics.scroller.line;

    let current_line_idx = blackbird_client_shared::lyrics::find_current_lyrics_line(
        lyrics_data,
        logic.get_playing_position(),
    );

    // Rebuild the back-mapping with the same width used during rendering
    // (including the scrollbar column reservation: inner.width - 1).
    let max_width = (inner.width as usize).saturating_sub(1);
    let (_, back_mapping) = build_wrapped_lyrics(
        lyrics_data,
        current_line_idx,
        max_width,
        style,
        lyrics.selected_index,
        // The selection indicator state doesn't affect the number of wrapped
        // rows — it only affects the text width available for wrapping, and
        // we always reserve the 2-char indicator space.
        true,
        None,
    );

    if let Some(line_index) = sidebar_click_to_line_index(y, inner, scroll_offset, &back_mapping) {
        seek_to_line(lyrics, logic, line_index);
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
    if let Some(duration) = line_index_to_duration(lyrics_data, selected) {
        logic.seek_current(duration);
        // Clear selection so the view returns to auto-follow.
        lyrics.selected_index = None;
        lyrics.user_scrolled = false;
    }
}

/// Seek playback to the timestamp of a lyrics line at the given index.
pub fn seek_to_line(lyrics: &mut LyricsViewState, logic: &bc::Logic, line_index: usize) {
    let Some(lyrics_data) = &lyrics.shared.data else {
        return;
    };
    if let Some(duration) = line_index_to_duration(lyrics_data, line_index) {
        logic.seek_current(duration);
        lyrics.selected_index = None;
        lyrics.user_scrolled = false;
    }
}

// ── Wrapped lyrics rendering ───────────────────────────────────────────────

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

/// Width of the timestamp prefix (e.g. "1:23 ") in the sidebar.
const SIDEBAR_TIMESTAMP_WIDTH: usize = 7;
/// Width of the selection indicator prefix ("> " or "  ").
const SELECTION_INDICATOR_WIDTH: usize = 2;

/// Pre-wraps all lyric lines into a flat list of rendered rows, with a parallel
/// back-mapping from rendered row index to logical line index.
///
/// Each rendered row is a `Line` ready for `Paragraph`. The back-mapping array
/// lets click-to-seek convert a clicked Y coordinate (rendered row) back to the
/// logical lyric line it belongs to.
///
/// When `show_selection_indicator` is true, each line's first row gets a 2-char
/// prefix: `> ` if selected, `  ` otherwise. To prevent reflow when focus
/// changes, the 2-char indicator space is always reserved (even when not
/// focused, the space is used for leading spaces), so the first row's text
/// width is always `max_width - 2 - SIDEBAR_TIMESTAMP_WIDTH`.
///
/// When `show_selection_indicator` is false, no prefix is added and the first
/// row's text width is `max_width - SIDEBAR_TIMESTAMP_WIDTH` (current behavior).
pub fn build_wrapped_lyrics(
    lyrics_data: &StructuredLyrics,
    current_line_idx: usize,
    max_width: usize,
    style: &shared_style::Style,
    selected_index: Option<usize>,
    show_selection_indicator: bool,
    hovered_line: Option<usize>,
) -> (Vec<Line<'static>>, Vec<usize>) {
    let text_color = style.general.text().to_color();
    let track_duration_color = style.library.track_duration().to_color();
    let track_name_playing_color = style.library.track_name_playing().to_color();
    let track_name_hovered_color = style.library.track_name_hovered().to_color();

    let mut lines = Vec::new();
    let mut back_mapping = Vec::new();

    for (idx, lyric_line) in lyrics_data.line.iter().enumerate() {
        let is_current = lyrics_data.synced && idx == current_line_idx;
        let is_past = lyrics_data.synced && idx < current_line_idx;
        let is_selected = selected_index == Some(idx);
        // Hover underline (like the library and similar-songs lists), except
        // when the row is keyboard-selected.
        let is_hovered = hovered_line == Some(idx) && !is_selected;

        let line_color = if is_selected {
            track_name_hovered_color
        } else if is_current {
            text_color
        } else if is_past {
            Color::Rgb(128, 128, 128)
        } else {
            Color::Rgb(180, 180, 180)
        };

        let underline = if is_hovered {
            Modifier::UNDERLINED
        } else {
            Modifier::empty()
        };
        let text_style = if is_selected || is_current {
            Style::default()
                .fg(line_color)
                .add_modifier(Modifier::BOLD)
                .add_modifier(underline)
        } else {
            Style::default().fg(line_color).add_modifier(underline)
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
                let ts_color = if is_selected {
                    track_name_hovered_color
                } else if is_current {
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

        // The selection indicator prefix: "> " for selected, "  " otherwise.
        // The 2-char indicator space is always reserved (even when not focused)
        // to prevent reflow when focus changes. When show_selection_indicator
        // is false, the indicator is rendered as spaces — the wrapping width
        // stays constant, so back-mapping is consistent between rendering and
        // click handling regardless of focus state.
        let indicator = if is_selected && show_selection_indicator {
            "> "
        } else {
            "  "
        };
        let indicator_style = if is_selected && show_selection_indicator {
            Style::default()
                .fg(track_name_hovered_color)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let indicator_span = Span::styled(indicator, indicator_style);

        // The first wrapped row gets the indicator and timestamp prefix, so
        // it has less width available for text. The indicator space is always
        // reserved to keep the wrapping width constant across focus changes.
        let indicator_width = SELECTION_INDICATOR_WIDTH;
        let first_row_width = max_width
            .saturating_sub(indicator_width)
            .saturating_sub(SIDEBAR_TIMESTAMP_WIDTH);
        let text_width = first_row_width.max(1);
        let wrapped_rows = wrap_lyric_line(&display_text, text_width);

        for (row_i, row_text) in wrapped_rows.iter().enumerate() {
            back_mapping.push(idx);
            if row_i == 0 {
                let mut spans = Vec::new();
                spans.push(indicator_span.clone());
                if let Some(ts) = &timestamp_span {
                    spans.push(ts.clone());
                }
                spans.push(Span::styled(row_text.clone(), text_style));
                lines.push(Line::from(spans));
            } else {
                // Continuation rows are indented to align with the text after
                // the indicator and timestamp.
                let indent = " ".repeat(indicator_width + SIDEBAR_TIMESTAMP_WIDTH);
                lines.push(Line::from(vec![
                    Span::raw(indent),
                    Span::styled(row_text.clone(), text_style),
                ]));
            }
        }
    }

    (lines, back_mapping)
}

/// Pure function: maps a click Y coordinate to a logical lyric line index.
///
/// `y` is the absolute terminal row. `inner_area` is the inner rect of the
/// lyrics view (after border). `scroll_offset` is the current scroll position in
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

    fn make_style() -> shared_style::Style {
        shared_style::Style::default()
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

    // ── Selection indicator tests ──────────────────────────────────────────

    #[test]
    fn selection_indicator_appears_on_selected_line_when_shown() {
        let lyrics_data = make_lyrics(true, &[(Some(0), "first"), (Some(5000), "second")]);
        let style = make_style();
        let (lines, _) = build_wrapped_lyrics(&lyrics_data, 0, 80, &style, Some(1), true, None);
        // Line 0 should start with "  " (not selected).
        // Line 1 should start with "> " (selected).
        let line0_str = line_to_string(&lines[0]);
        assert!(
            line0_str.starts_with("  "),
            "line 0 should start with '  ', got: {line0_str:?}"
        );
        let line1_str = line_to_string(&lines[1]);
        assert!(
            line1_str.starts_with("> "),
            "line 1 should start with '> ', got: {line1_str:?}"
        );
    }

    #[test]
    fn selection_indicator_absent_when_not_shown() {
        let lyrics_data = make_lyrics(true, &[(Some(0), "first"), (Some(5000), "second")]);
        let style = make_style();
        let (lines, _) = build_wrapped_lyrics(&lyrics_data, 0, 80, &style, Some(1), false, None);
        // No ">" indicator should be present on any line.
        for (i, line) in lines.iter().enumerate() {
            let s = line_to_string(line);
            assert!(
                !s.starts_with("> "),
                "line {i} should not have '> ' indicator, got: {s:?}"
            );
        }
    }

    #[test]
    fn selected_line_uses_hovered_color() {
        let lyrics_data = make_lyrics(true, &[(Some(0), "first"), (Some(5000), "second")]);
        let style = make_style();
        let hovered_color = style.library.track_name_hovered().to_color();
        let (lines, _) = build_wrapped_lyrics(&lyrics_data, 0, 80, &style, Some(1), true, None);
        // Line 1 is selected; its text span should use the hovered color.
        // The line has 3 spans: indicator, timestamp, text.
        let line1 = &lines[1];
        assert!(line1.spans.len() >= 3, "expected at least 3 spans");
        let text_span = &line1.spans[2];
        assert_eq!(
            text_span.style.fg,
            Some(hovered_color),
            "selected line text should use hovered color"
        );
    }

    #[test]
    fn both_views_produce_identical_output_when_focused() {
        let lyrics_data = make_lyrics(true, &[(Some(0), "first"), (Some(5000), "second")]);
        let style = make_style();
        // Full panel: is_focused=true
        let (full_lines, full_back) =
            build_wrapped_lyrics(&lyrics_data, 0, 80, &style, None, true, None);
        // Sidebar: is_focused=true
        let (sidebar_lines, sidebar_back) =
            build_wrapped_lyrics(&lyrics_data, 0, 80, &style, None, true, None);
        // Back-mappings should be identical.
        assert_eq!(full_back, sidebar_back);
        // Lines should have the same count.
        assert_eq!(full_lines.len(), sidebar_lines.len());
    }

    #[test]
    fn no_reflow_when_focus_changes() {
        // The 2-char indicator space is always reserved, so the wrapping width
        // is constant regardless of focus state. This means the number of
        // wrapped rows (and thus the back-mapping) is identical whether or not
        // the selection indicator is shown.
        let lyrics_data = make_lyrics(
            true,
            &[(Some(0), "first line"), (Some(5000), "second line here")],
        );
        let style = make_style();
        // Use a width where 2 extra/less chars could cause a different wrap.
        let (focused_lines, focused_back) =
            build_wrapped_lyrics(&lyrics_data, 0, 20, &style, Some(1), true, None);
        let (unfocused_lines, unfocused_back) =
            build_wrapped_lyrics(&lyrics_data, 0, 20, &style, Some(1), false, None);
        // Same number of wrapped rows → no reflow.
        assert_eq!(
            focused_lines.len(),
            unfocused_lines.len(),
            "row count should not change with focus state"
        );
        // Same back-mapping → click-to-seek is consistent.
        assert_eq!(
            focused_back, unfocused_back,
            "back-mapping should not change with focus state"
        );
    }

    /// Helper to convert a `Line` to a plain string for assertion.
    fn line_to_string(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    // ── move_selection tests ──────────────────────────────────────────────

    #[test]
    fn move_selection_down_from_current_line() {
        let mut state = LyricsViewState::new();
        state.shared.data = Some(make_lyrics(
            true,
            &[(Some(0), "a"), (Some(1000), "b"), (Some(2000), "c")],
        ));
        // No selection yet; move down from current line (index 0).
        move_selection(&mut state, None, 1);
        assert_eq!(state.selected_index, Some(1));
    }

    #[test]
    fn move_selection_up_clamps_at_zero() {
        let mut state = LyricsViewState::new();
        state.shared.data = Some(make_lyrics(true, &[(Some(0), "a"), (Some(1000), "b")]));
        state.selected_index = Some(0);
        move_selection(&mut state, None, -1);
        assert_eq!(state.selected_index, Some(0));
    }

    #[test]
    fn move_selection_down_clamps_at_last() {
        let mut state = LyricsViewState::new();
        state.shared.data = Some(make_lyrics(true, &[(Some(0), "a"), (Some(1000), "b")]));
        state.selected_index = Some(1);
        move_selection(&mut state, None, 1);
        assert_eq!(state.selected_index, Some(1));
    }
}
