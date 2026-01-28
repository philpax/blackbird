use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

use crate::app::App;

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    let entries = app.log_buffer.get_entries();

    let block = Block::default()
        .title(format!(" Logs ({}) ", entries.len()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if entries.is_empty() {
        let empty = ratatui::widgets::Paragraph::new("No log entries")
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(empty, inner);
        return;
    }

    let items: Vec<ListItem> = entries
        .iter()
        .map(|entry| {
            let level_color = match entry.level {
                tracing::Level::ERROR => Color::Red,
                tracing::Level::WARN => Color::Yellow,
                tracing::Level::INFO => Color::Cyan,
                tracing::Level::DEBUG => Color::Green,
                tracing::Level::TRACE => Color::DarkGray,
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
                    format!("{level_str}"),
                    Style::default()
                        .fg(level_color)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled(format!("{target:24}"), Style::default().fg(Color::DarkGray)),
                Span::raw(" "),
                Span::styled(&entry.message, Style::default().fg(Color::White)),
            ]);

            ListItem::new(line)
        })
        .collect();

    let list = List::new(items).highlight_style(
        Style::default()
            .bg(Color::Rgb(50, 50, 80))
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
