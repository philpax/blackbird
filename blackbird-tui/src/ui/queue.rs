use blackbird_client_shared::{self, style as shared_style};
use blackbird_core::{self as bc, TrackDisplayDetails, blackbird_state::TrackId};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
};

use crate::keys::Action;

use super::StyleExt;

pub enum QueueAction {
    ToggleQueue,
    Quit,
}

pub struct QueueState {
    /// Keyboard-selected line index. `None` = auto-follow current track.
    pub selected_index: Option<usize>,
    pub scroll_offset: usize,
}

impl QueueState {
    pub fn new() -> Self {
        Self {
            selected_index: None,
            scroll_offset: 0,
        }
    }

    pub fn reset(&mut self) {
        self.selected_index = None;
        self.scroll_offset = 0;
    }
}

/// Number of tracks to show before and after the current track in the queue window.
const QUEUE_RADIUS: usize = 50;

pub fn draw(
    frame: &mut Frame,
    queue_state: &QueueState,
    style: &shared_style::Style,
    logic: &bc::Logic,
    area: Rect,
) {
    let mode = logic.get_playback_mode();
    let block = Block::default()
        .title(format!(" Queue [{}] ", mode))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(style.album_color()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let (before, current, after) = logic.get_queue_window(QUEUE_RADIUS);

    if current.is_none() {
        let msg = ratatui::widgets::Paragraph::new("No tracks in the queue.")
            .style(Style::default().fg(style.track_duration_color()));
        frame.render_widget(msg, inner);
        return;
    }

    let state = logic.get_state();
    let st = state.read().unwrap();

    // Pre-compute style colors.
    let text_color = style.text_color();
    let track_duration_color = style.track_duration_color();
    let track_name_playing_color = style.track_name_playing_color();
    let track_name_hovered_color = style.track_name_hovered_color();

    // Build list: [before... | current | after...]
    let total_items = before.len() + 1 + after.len();
    let current_list_index = before.len();
    let selected_index = queue_state.selected_index;

    let mut items: Vec<ListItem> = Vec::with_capacity(total_items);

    let all_tracks: Vec<&TrackId> = before
        .iter()
        .chain(current.iter())
        .chain(after.iter())
        .collect();

    for (idx, track_id) in all_tracks.iter().enumerate() {
        let is_current = idx == current_list_index;
        let is_selected = selected_index == Some(idx);

        let display = TrackDisplayDetails::from_track_id(track_id, &st);
        let label = match &display {
            Some(d) => format!("{} - {}", d.artist(), d.track_title),
            None => format!("{}", track_id.0),
        };

        let duration_str = display
            .as_ref()
            .map(|d| {
                format!(
                    " [{}]",
                    bc::util::seconds_to_hms_string(d.track_duration.as_secs() as u32, false)
                )
            })
            .unwrap_or_default();

        let line_color = if is_selected {
            track_name_hovered_color
        } else if is_current {
            track_name_playing_color
        } else if idx < current_list_index {
            // Previous tracks are dimmed.
            ratatui::style::Color::Rgb(128, 128, 128)
        } else {
            text_color
        };

        let mut spans = Vec::new();

        // Selection indicator.
        if is_selected {
            spans.push(Span::styled(
                "> ",
                Style::default()
                    .fg(track_name_hovered_color)
                    .add_modifier(Modifier::BOLD),
            ));
        } else if is_current {
            spans.push(Span::styled(
                "▶ ",
                Style::default()
                    .fg(track_name_playing_color)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::raw("  "));
        }

        let text_style = if is_selected || is_current {
            Style::default().fg(line_color).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(line_color)
        };

        spans.push(Span::styled(label, text_style));
        spans.push(Span::styled(
            duration_str,
            Style::default().fg(track_duration_color),
        ));

        items.push(ListItem::new(Line::from(spans)));
    }

    let list = List::new(items);

    let mut list_state = ListState::default();
    let focus_line = selected_index.unwrap_or(current_list_index);
    list_state.select(Some(focus_line));
    let visible_height = inner.height as usize;
    let offset = focus_line.saturating_sub(visible_height / 2);
    *list_state.offset_mut() = offset;

    frame.render_stateful_widget(list, inner, &mut list_state);
}

pub fn handle_key(
    queue_state: &mut QueueState,
    logic: &bc::Logic,
    action: Action,
) -> Option<QueueAction> {
    match action {
        Action::Back => return Some(QueueAction::ToggleQueue),
        Action::Quit => return Some(QueueAction::Quit),
        Action::MoveUp => move_selection(queue_state, logic, -1),
        Action::MoveDown => move_selection(queue_state, logic, 1),
        Action::PageUp => {
            move_selection(
                queue_state,
                logic,
                -(super::layout::PAGE_SCROLL_SIZE as i32),
            );
        }
        Action::PageDown => {
            move_selection(queue_state, logic, super::layout::PAGE_SCROLL_SIZE as i32);
        }
        Action::Select => play_selected(queue_state, logic),
        Action::PlayPause => logic.toggle_current(),
        Action::Next => logic.next(),
        Action::Previous => logic.previous(),
        Action::CyclePlaybackMode => {
            let next = blackbird_client_shared::next_playback_mode(logic.get_playback_mode());
            logic.set_playback_mode(next);
        }
        _ => {}
    }
    None
}

/// Handle a mouse click in the queue area — play the clicked track.
pub fn handle_mouse_click(
    queue_state: &mut QueueState,
    logic: &bc::Logic,
    area: Rect,
    _x: u16,
    y: u16,
) {
    let inner_y = area.y + 1;
    let inner_height = area.height.saturating_sub(2);
    if y < inner_y || y >= inner_y + inner_height {
        return;
    }

    let (before, current, after) = logic.get_queue_window(QUEUE_RADIUS);
    if current.is_none() {
        return;
    }
    let total_items = before.len() + 1 + after.len();

    let current_list_index = before.len();
    let visible_height = inner_height as usize;
    let focus_line = queue_state.selected_index.unwrap_or(current_list_index);
    let scroll_offset = focus_line.saturating_sub(visible_height / 2);

    let row_in_list = (y - inner_y) as usize;
    let clicked_index = scroll_offset + row_in_list;

    if clicked_index < total_items {
        let all_tracks: Vec<TrackId> = before.into_iter().chain(current).chain(after).collect();
        logic.request_play_track(&all_tracks[clicked_index]);
        queue_state.selected_index = None;
    }
}

fn move_selection(queue_state: &mut QueueState, logic: &bc::Logic, delta: i32) {
    let (before, current, after) = logic.get_queue_window(QUEUE_RADIUS);
    if current.is_none() {
        return;
    }
    let total_items = before.len() + 1 + after.len();
    if total_items == 0 {
        return;
    }

    let current_list_index = before.len();
    let current_sel = queue_state.selected_index.unwrap_or(current_list_index);
    let new_index = (current_sel as i32 + delta).clamp(0, total_items as i32 - 1) as usize;
    queue_state.selected_index = Some(new_index);
}

fn play_selected(queue_state: &mut QueueState, logic: &bc::Logic) {
    let Some(selected) = queue_state.selected_index else {
        return;
    };

    let (before, current, after) = logic.get_queue_window(QUEUE_RADIUS);
    if current.is_none() {
        return;
    }

    let all_tracks: Vec<TrackId> = before.into_iter().chain(current).chain(after).collect();

    if let Some(track_id) = all_tracks.get(selected) {
        logic.request_play_track(track_id);
        queue_state.selected_index = None;
    }
}

/// Move selection by `delta` (for scroll events).
pub fn scroll_selection(queue_state: &mut QueueState, logic: &bc::Logic, delta: i32) {
    move_selection(queue_state, logic, delta);
}
