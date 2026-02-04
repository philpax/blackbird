use blackbird_client_shared::style as shared_style;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
};

use crate::{keys::Action, log_buffer::LogBuffer};

use super::StyleExt;

pub enum LogsAction {
    ToggleLogs,
    Quit,
}

pub struct LogsState {
    pub log_buffer: LogBuffer,
    pub scroll_offset: usize,
}

impl LogsState {
    pub fn new(log_buffer: LogBuffer) -> Self {
        Self {
            log_buffer,
            scroll_offset: 0,
        }
    }

    pub fn scroll_to_end(&mut self) {
        let len = self.log_buffer.len();
        self.scroll_offset = len.saturating_sub(1);
    }
}

pub fn draw(frame: &mut Frame, logs: &mut LogsState, style: &shared_style::Style, area: Rect) {
    let entries = logs.log_buffer.get_entries();

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
            let target = if entry.target.len() > super::layout::LOG_TARGET_WIDTH {
                format!(
                    "...{}",
                    &entry.target[entry.target.len() - super::layout::LOG_TARGET_SUFFIX_LEN..]
                )
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
                    format!("{target:width$}", width = super::layout::LOG_TARGET_WIDTH),
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
    let offset = logs.scroll_offset.min(max_offset);
    logs.scroll_offset = offset;

    state.select(Some(offset));

    frame.render_stateful_widget(list, inner, &mut state);
}

pub fn handle_key(logs: &mut LogsState, action: Action) -> Option<LogsAction> {
    let log_len = logs.log_buffer.len();

    match action {
        Action::Back => return Some(LogsAction::ToggleLogs),
        Action::Quit => return Some(LogsAction::Quit),
        Action::MoveUp => {
            logs.scroll_offset = logs.scroll_offset.saturating_sub(1);
        }
        Action::MoveDown => {
            if log_len > 0 {
                logs.scroll_offset = (logs.scroll_offset + 1).min(log_len - 1);
            }
        }
        Action::PageUp => {
            logs.scroll_offset = logs
                .scroll_offset
                .saturating_sub(super::layout::PAGE_SCROLL_SIZE);
        }
        Action::PageDown => {
            if log_len > 0 {
                logs.scroll_offset =
                    (logs.scroll_offset + super::layout::PAGE_SCROLL_SIZE).min(log_len - 1);
            }
        }
        Action::GotoTop => {
            logs.scroll_offset = 0;
        }
        Action::GotoBottom => {
            if log_len > 0 {
                logs.scroll_offset = log_len - 1;
            }
        }
        _ => {}
    }
    None
}
