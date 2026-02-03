use blackbird_core::{
    self as bc, TrackDisplayDetails, blackbird_state::TrackId, util::seconds_to_hms_string,
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

use crate::{app::App, keys::Action};

use super::{StyleExt, string_to_color};

pub struct SearchState {
    pub query: String,
    pub results: Vec<TrackId>,
    pub selected_index: usize,
}

impl SearchState {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            results: Vec::new(),
            selected_index: 0,
        }
    }

    pub fn reset(&mut self) {
        self.query.clear();
        self.results.clear();
        self.selected_index = 0;
    }

    pub fn update(&mut self, logic: &bc::Logic) {
        if self.query.len() >= 3 {
            let state = logic.get_state();
            let mut state = state.write().unwrap();
            self.results = state.library.search(&self.query);
            self.selected_index = 0;
        } else {
            self.results.clear();
        }
    }
}

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    let style = &app.config.style;

    let block = Block::default()
        .title(" Search ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(style.track_name_playing_color()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(inner);

    // Search input
    let input = Paragraph::new(Line::from(vec![
        Span::styled("> ", Style::default().fg(style.track_name_playing_color())),
        Span::styled(&app.search.query, Style::default().fg(style.text_color())),
        Span::styled(
            "\u{2588}",
            Style::default().fg(style.track_name_playing_color()),
        ),
    ]));
    frame.render_widget(input, chunks[0]);

    // Search results
    if app.search.query.len() < 3 {
        let hint = if app.search.query.is_empty() {
            "Type to search..."
        } else {
            "Enter at least 3 characters..."
        };
        let hint_widget =
            Paragraph::new(hint).style(Style::default().fg(style.track_duration_color()));
        frame.render_widget(hint_widget, chunks[1]);
        return;
    }

    if app.search.results.is_empty() {
        let no_results = Paragraph::new("No results found.")
            .style(Style::default().fg(style.track_duration_color()));
        frame.render_widget(no_results, chunks[1]);
        return;
    }

    let state_arc = app.logic.get_state();
    let app_state = state_arc.read().unwrap();

    // Pre-compute style colors to avoid borrow conflicts in closure.
    let track_name_color = style.track_name_color();
    let track_length_color = style.track_length_color();
    let track_duration_color = style.track_duration_color();
    let track_name_hovered_color = style.track_name_hovered_color();

    let items: Vec<ListItem> = app
        .search
        .results
        .iter()
        .enumerate()
        .map(|(i, track_id)| {
            let is_selected = i == app.search.selected_index;
            let details = TrackDisplayDetails::from_track_id(track_id, &app_state);

            let line = if let Some(d) = details {
                let artist = d.artist();
                let dur_str = seconds_to_hms_string(d.track_duration.as_secs() as u32, false);

                Line::from(vec![
                    Span::styled(
                        artist.to_string(),
                        Style::default().fg(string_to_color(artist)),
                    ),
                    Span::raw(" - "),
                    Span::styled(
                        d.track_title.to_string(),
                        Style::default().fg(track_name_color),
                    ),
                    Span::styled(
                        format!(" [{dur_str}]"),
                        Style::default().fg(track_length_color),
                    ),
                ])
            } else {
                Line::from(Span::styled(
                    format!("[{track_id}]"),
                    Style::default().fg(track_duration_color),
                ))
            };

            let item_style = if is_selected {
                Style::default().bg(track_name_hovered_color)
            } else {
                Style::default()
            };

            ListItem::new(line).style(item_style)
        })
        .collect();

    let list = List::new(items);
    let mut list_state = ListState::default();
    list_state.select(Some(app.search.selected_index));

    frame.render_stateful_widget(list, chunks[1], &mut list_state);
}

pub fn handle_key(app: &mut App, action: Action) {
    match action {
        Action::Back => app.toggle_search(),
        Action::Select => {
            if let Some(track_id) = app.search.results.get(app.search.selected_index) {
                app.logic.request_play_track(track_id);
                app.toggle_search();
            }
        }
        Action::MoveUp => {
            if app.search.selected_index > 0 {
                app.search.selected_index -= 1;
            }
        }
        Action::MoveDown => {
            if !app.search.results.is_empty()
                && app.search.selected_index < app.search.results.len() - 1
            {
                app.search.selected_index += 1;
            }
        }
        Action::DeleteChar => {
            app.search.query.pop();
            app.search.update(&app.logic);
        }
        Action::ClearLine => {
            app.search.query.clear();
            app.search.update(&app.logic);
        }
        Action::Char(c) => {
            app.search.query.push(c);
            app.search.update(&app.logic);
        }
        _ => {}
    }
}
