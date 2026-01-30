use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
};

use crate::app::App;

use super::StyleExt;

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    let style = &app.config.style;
    let entries = app.log_buffer.get_entries();

    let block = Block::default()
        .title(format!(" Logs ({}) ", entries.len()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(style.album_color()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if entries.is_empty() {
        let empty = ratatui::widgets::Paragraph::new("No log entries")
            .style(Style::default().fg(style.track_duration_color()));
        frame.render_widget(empty, inner);
        return;
    }

    // Pre-compute style colors.
    let text_color = style.text_color();
    let track_duration_color = style.track_duration_color();
    let track_name_hovered_color = style.track_name_hovered_color();

    let items: Vec<ListItem> = entries
        .iter()
        .map(|entry| {
            // Keep semantic colors for log levels.
            let level_color = match entry.level {
                tracing::Level::ERROR => Color::Red,
                tracing::Level::WARN => Color::Yellow,
                tracing::Level::INFO => Color::Cyan,
                tracing::Level::DEBUG => Color::Green,
                tracing::Level::TRACE => track_duration_color,
            };

            let level_str = match entry.level {
                tracing::Level::ERROR => "ERR",
                tracing::Level::WARN => "WRN",
                tracing::Level::INFO => "INF",
                tracing::Level::DEBUG => "DBG",
                tracing::Level::TRACE => "TRC",
            };

            // Truncate target if too long.
            let target = if entry.target.len() > 24 {
                format!("...{}", &entry.target[entry.target.len() - 21..])
            } else {
                entry.target.clone()
            };

            let line = Line::from(vec![
                Span::styled(
                    level_str,
                    Style::default()
                        .fg(level_color)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled(
                    format!("{target:24}"),
                    Style::default().fg(track_duration_color),
                ),
                Span::raw(" "),
                Span::styled(&entry.message, Style::default().fg(text_color)),
            ]);

            ListItem::new(line)
        })
        .collect();

    let list = List::new(items).highlight_style(
        Style::default()
            .bg(track_name_hovered_color)
            .add_modifier(Modifier::BOLD),
    );

    // Use a ListState to manage scrolling.
    let mut state = ListState::default();

    // Clamp scroll offset to valid range.
    let max_offset = entries.len().saturating_sub(1);
    let offset = app.logs_scroll_offset.min(max_offset);
    app.logs_scroll_offset = offset;

    state.select(Some(offset));

    frame.render_stateful_widget(list, inner, &mut state);
}
