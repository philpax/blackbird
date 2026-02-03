use std::time::Duration;

use blackbird_core::util::seconds_to_hms_string;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

use crate::app::App;
use crate::keys::Action;

use super::StyleExt;

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    let style = &app.config.style;

    let block = Block::default()
        .title(" Lyrics ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(style.album_color()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.lyrics_loading {
        let loading = Paragraph::new("Loading lyrics...")
            .style(Style::default().fg(style.track_duration_color()));
        frame.render_widget(loading, inner);
        return;
    }

    let Some(lyrics) = &app.lyrics_data else {
        let msg = Paragraph::new("No lyrics available for this track.")
            .style(Style::default().fg(style.track_duration_color()));
        frame.render_widget(msg, inner);
        return;
    };

    if lyrics.line.is_empty() {
        let msg = Paragraph::new("No lyrics available for this track.")
            .style(Style::default().fg(style.track_duration_color()));
        frame.render_widget(msg, inner);
        return;
    }

    let current_line_idx = blackbird_client_shared::lyrics::find_current_lyrics_line(
        lyrics,
        app.logic.get_playing_position(),
    );

    let selected_index = app.lyrics_selected_index;
    let track_name_hovered_color = style.track_name_hovered_color();

    // Pre-compute style colors to avoid borrow conflicts in closure.
    let text_color = style.text_color();
    let track_duration_color = style.track_duration_color();
    let track_name_playing_color = style.track_name_playing_color();

    let items: Vec<ListItem> = lyrics
        .line
        .iter()
        .enumerate()
        .map(|(idx, line)| {
            let is_current = lyrics.synced && idx == current_line_idx;
            let is_past = lyrics.synced && idx < current_line_idx;
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
    if lyrics.synced {
        // If the user has a keyboard selection, center on that; otherwise follow playback.
        let focus_line = selected_index.unwrap_or(current_line_idx);
        list_state.select(Some(focus_line));
        let visible_height = inner.height as usize;
        let offset = focus_line.saturating_sub(visible_height / 2);
        *list_state.offset_mut() = offset;
    } else {
        list_state.select(selected_index);
        *list_state.offset_mut() = app.lyrics_scroll_offset;
    }

    frame.render_stateful_widget(list, inner, &mut list_state);
}

pub fn handle_key(app: &mut App, action: Action) {
    match action {
        Action::Back => app.toggle_lyrics(),
        Action::Quit => app.should_quit = true,
        Action::MoveUp => move_selection(app, -1),
        Action::MoveDown => move_selection(app, 1),
        Action::PageUp => move_selection(app, -(super::layout::PAGE_SCROLL_SIZE as i32)),
        Action::PageDown => move_selection(app, super::layout::PAGE_SCROLL_SIZE as i32),
        Action::Select => seek_to_selected(app),
        Action::SeekForward => app.seek_relative(super::layout::SEEK_STEP_SECS),
        Action::SeekBackward => app.seek_relative(-super::layout::SEEK_STEP_SECS),
        Action::PlayPause => app.logic.toggle_current(),
        Action::Next => app.logic.next(),
        Action::Previous => app.logic.previous(),
        _ => {}
    }
}

/// Handle click in the lyrics area — seek to the clicked line.
pub fn handle_mouse_click(app: &mut App, area: Rect, _x: u16, y: u16) {
    let Some(lyrics) = &app.lyrics_data else {
        return;
    };
    if lyrics.line.is_empty() {
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
        lyrics,
        app.logic.get_playing_position(),
    );
    let scroll_offset = if lyrics.synced {
        if let Some(selected) = app.lyrics_selected_index {
            selected.saturating_sub(inner_height as usize / 2)
        } else {
            current_line_idx.saturating_sub(inner_height as usize / 2)
        }
    } else {
        app.lyrics_scroll_offset
    };

    let clicked_index = scroll_offset + row_in_list;
    if clicked_index < lyrics.line.len() {
        seek_to_line(app, clicked_index);
    }
}

/// Move the lyrics selection cursor by `delta` lines.
/// If no selection exists, starts from the current playing line.
pub fn move_selection(app: &mut App, delta: i32) {
    let line_count = app.lyrics_data.as_ref().map(|l| l.line.len()).unwrap_or(0);
    if line_count == 0 {
        return;
    }

    let current = app.lyrics_selected_index.unwrap_or_else(|| {
        app.lyrics_data
            .as_ref()
            .map(|lyrics| {
                blackbird_client_shared::lyrics::find_current_lyrics_line(
                    lyrics,
                    app.logic.get_playing_position(),
                )
            })
            .unwrap_or(0)
    });

    let new_index = (current as i32 + delta).clamp(0, line_count as i32 - 1) as usize;
    app.lyrics_selected_index = Some(new_index);
}

/// Seek playback to the timestamp of the currently selected lyrics line.
pub fn seek_to_selected(app: &mut App) {
    let Some(selected) = app.lyrics_selected_index else {
        return;
    };
    let Some(lyrics) = &app.lyrics_data else {
        return;
    };
    if let Some(line) = lyrics.line.get(selected)
        && let Some(start_ms) = line.start
    {
        app.logic
            .seek_current(Duration::from_millis(start_ms as u64));
        // Clear selection so the view returns to auto-follow.
        app.lyrics_selected_index = None;
    }
}

/// Seek playback to the timestamp of a lyrics line at the given index.
pub fn seek_to_line(app: &mut App, line_index: usize) {
    let Some(lyrics) = &app.lyrics_data else {
        return;
    };
    if let Some(line) = lyrics.line.get(line_index)
        && let Some(start_ms) = line.start
    {
        app.logic
            .seek_current(Duration::from_millis(start_ms as u64));
        app.lyrics_selected_index = None;
    }
}
