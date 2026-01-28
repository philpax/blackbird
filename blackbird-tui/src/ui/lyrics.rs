use blackbird_core::util::seconds_to_hms_string;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::app::App;

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    let block = Block::default()
        .title(" Lyrics ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.lyrics_loading {
        let loading =
            Paragraph::new("Loading lyrics...").style(Style::default().fg(Color::DarkGray));
        frame.render_widget(loading, inner);
        return;
    }

    let Some(lyrics) = &app.lyrics_data else {
        let msg = Paragraph::new("No lyrics available for this track.")
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(msg, inner);
        return;
    };

    if lyrics.line.is_empty() {
        let msg = Paragraph::new("No lyrics available for this track.")
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(msg, inner);
        return;
    }

    let current_position_ms = app
        .logic
        .get_playing_position()
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    let adjusted_position_ms = current_position_ms + lyrics.offset.unwrap_or(0);

    let current_line_idx = if lyrics.synced {
        lyrics
            .line
            .iter()
            .enumerate()
            .rev()
            .find(|(_, line)| line.start.unwrap_or(0) <= adjusted_position_ms)
            .map(|(idx, _)| idx)
            .unwrap_or(0)
    } else {
        0
    };

    let items: Vec<ListItem> = lyrics
        .line
        .iter()
        .enumerate()
        .map(|(idx, line)| {
            let is_current = lyrics.synced && idx == current_line_idx;
            let is_past = lyrics.synced && idx < current_line_idx;

            let text_color = if is_current {
                Color::White
            } else if is_past {
                Color::Rgb(128, 128, 128)
            } else {
                Color::Rgb(180, 180, 180)
            };

            let mut spans = Vec::new();

            if let Some(start_ms) = line.start
                && !line.value.trim().is_empty()
            {
                let timestamp_secs = (start_ms / 1000) as u32;
                let timestamp_str = seconds_to_hms_string(timestamp_secs, false);
                let ts_color = if is_current {
                    Color::Cyan
                } else {
                    Color::Rgb(80, 80, 80)
                };
                spans.push(Span::styled(
                    format!("{timestamp_str:>6} "),
                    Style::default().fg(ts_color),
                ));
            }

            let text_style = if is_current {
                Style::default().fg(text_color).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(text_color)
            };

            spans.push(Span::styled(&line.value, text_style));

            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(items);

    // Auto-scroll to current line for synced lyrics
    let mut list_state = ListState::default();
    if lyrics.synced {
        list_state.select(Some(current_line_idx));
        // Offset so the current line is roughly centered
        let visible_height = inner.height as usize;
        let offset = current_line_idx.saturating_sub(visible_height / 2);
        *list_state.offset_mut() = offset;
    } else {
        list_state.select(None);
        *list_state.offset_mut() = app.lyrics_scroll_offset;
    }

    frame.render_stateful_widget(list, inner, &mut list_state);
}
