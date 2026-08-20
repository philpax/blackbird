use blackbird_client_shared::{self, style as shared_style};
use blackbird_core::{self as bc, PlayPick, blackbird_state::TrackId};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    widgets::{Block, Borders, List, ListItem, ListState},
};

use crate::keys::Action;

use super::ToColor;

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
pub(crate) const QUEUE_RADIUS: usize = 50;

/// Resolve a visible index in the current queue window to its track id.
///
/// The window is `[before... | current | after...]` as returned by
/// [`bc::Logic::get_queue_window`]. Returns `None` if `index` is out of range
/// or the queue is empty.
pub fn queue_window_track_at(logic: &bc::Logic, index: usize) -> Option<TrackId> {
    let (before, current, after) = logic.get_queue_window(QUEUE_RADIUS);
    let total = before.len() + usize::from(current.is_some()) + after.len();
    if index >= total {
        return None;
    }
    let all: Vec<TrackId> = before.into_iter().chain(current).chain(after).collect();
    all.into_iter().nth(index)
}

/// Navigate within the existing queue to the track at `index`, preserving the
/// current ordering so next/previous still reach the surrounding tracks.
/// No-op if `index` is out of range.
pub fn play_queue_index(logic: &bc::Logic, index: usize) {
    if let Some(track_id) = queue_window_track_at(logic, index) {
        logic.request_play_track(&track_id, PlayPick::Navigate);
    }
}

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
        .border_style(Style::default().fg(style.panels.border().to_color()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let (before, current, after) = logic.get_queue_window(QUEUE_RADIUS);

    if current.is_none() {
        let msg = ratatui::widgets::Paragraph::new("No tracks in the queue.")
            .style(Style::default().fg(style.library.track_duration().to_color()));
        frame.render_widget(msg, inner);
        return;
    }

    let state = logic.get_state();
    let st = state.read().unwrap();

    // Build list: [before... | current | after...]
    let total_items = before.len() + 1 + after.len();
    let current_list_index = before.len();
    let selected_index = queue_state.selected_index;

    let all_tracks: Vec<&TrackId> = before
        .iter()
        .chain(current.iter())
        .chain(after.iter())
        .collect();

    let mut items: Vec<ListItem> = Vec::with_capacity(total_items);

    for (idx, track_id) in all_tracks.iter().enumerate() {
        let is_current = idx == current_list_index;
        let is_selected = selected_index == Some(idx);

        let indicator = if is_current {
            super::TrackIndicator::Playing
        } else if is_selected {
            super::TrackIndicator::Selected
        } else {
            super::TrackIndicator::None
        };

        let dimmed = idx < current_list_index;

        let line = super::render_track_line(
            track_id,
            &st,
            style,
            is_selected,
            indicator,
            dimmed,
            inner.width, // no scrollbar in fullscreen queue
        );

        items.push(ListItem::new(line));
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
        Action::NextGroup => logic.next_group(),
        Action::PreviousGroup => logic.previous_group(),
        Action::CyclePlaybackMode(dir) => {
            let next = blackbird_client_shared::cycle(
                &bc::PlaybackMode::ALL,
                logic.get_playback_mode(),
                dir,
            );
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
        play_queue_index(logic, clicked_index);
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
    play_queue_index(logic, selected);
    queue_state.selected_index = None;
}

/// Move selection by `delta` (for scroll events).
pub fn scroll_selection(queue_state: &mut QueueState, logic: &bc::Logic, delta: i32) {
    move_selection(queue_state, logic, delta);
}
