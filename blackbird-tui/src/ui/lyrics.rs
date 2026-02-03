use blackbird_core::util::seconds_to_hms_string;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

use crate::app::App;

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
                Style::default()
                    .fg(line_color)
                    .add_modifier(Modifier::BOLD)
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
