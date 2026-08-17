use blackbird_client_shared::style as shared_style;
use blackbird_core::{self as bc, blackbird_state::TrackId};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

use crate::keys::Action;

use super::{ToColor, TrackIndicator};

pub enum SearchAction {
    ToggleSearch,
    GotoTrack(TrackId),
}

pub struct SearchState {
    pub query: String,
    pub results: Vec<TrackId>,
    pub selected_index: usize,

    /// Shared scroll/drag/inertia mechanism. Each result is one line, so
    /// `viewport.line` doubles as the index of the first visible result.
    pub viewport: super::scroll::Scroller,

    // Mouse interaction
    /// Pending click at `(x, y, result_index)`. Resolved on mouse-up: if no
    /// drag intervened, the track is played.
    pub click_pending: Option<(u16, u16, usize)>,
}

impl SearchState {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            results: Vec::new(),
            selected_index: 0,
            viewport: super::scroll::Scroller::new(),
            click_pending: None,
        }
    }

    pub fn reset(&mut self) {
        self.query.clear();
        self.results.clear();
        self.selected_index = 0;
        self.viewport = super::scroll::Scroller::new();
        self.click_pending = None;
    }

    pub fn update(&mut self, logic: &bc::Logic) {
        if self.query.len() >= 3 {
            let state = logic.get_state();
            let mut state = state.write().unwrap();
            self.results = state.library.search(&self.query);
        } else {
            self.results.clear();
        }
        self.selected_index = 0;
        self.viewport.line = 0;
        self.viewport.cancel_inertia();
    }

    /// Adjust the viewport so `selected_index` is in the visible window.
    fn ensure_selection_visible(&mut self) {
        let visible_height = self.viewport.visible_height;
        if visible_height == 0 {
            return;
        }
        if self.selected_index < self.viewport.line {
            self.viewport.line = self.selected_index;
        } else if self.selected_index >= self.viewport.line + visible_height {
            self.viewport.line = self.selected_index + 1 - visible_height;
        }
    }

    /// Handle a left-mouse-down inside the search panel. Either starts a
    /// scrollbar drag (click in the rightmost column) or records a pending
    /// click that `handle_mouse_up` will resolve as a track play.
    pub fn handle_mouse_click(&mut self, area: Rect, x: u16, y: u16) {
        let results = results_area(area);
        if y < results.y || y >= results.y + results.height || self.results.is_empty() {
            return;
        }

        // Click on the scrollbar column → start a scrollbar drag (only when
        // the scrollbar is actually visible, so the rightmost result column
        // remains clickable when it isn't).
        if self.viewport.needs_scrollbar(self.results.len())
            && super::scroll::is_in_scrollbar_column(results, x, 1)
        {
            self.viewport
                .apply_scrollbar_drag(y, self.results.len(), results.y, results.height);
            self.click_pending = None;
            return;
        }

        let row_in_list = (y - results.y) as usize;
        let clicked_index = self.viewport.line + row_in_list;
        if clicked_index < self.results.len() {
            self.selected_index = clicked_index;
            self.click_pending = Some((x, y, clicked_index));
            self.viewport.drag_last_y = Some(y);
        }
    }

    /// Handle a left-mouse-drag inside the search panel. Routes scrollbar
    /// drags to `apply_scrollbar_drag` and content drags to
    /// `apply_content_drag`. Promotes a pending click to a drag so mouse-up
    /// won't play a track.
    pub fn handle_mouse_drag(&mut self, area: Rect, x: u16, y: u16) {
        let results = results_area(area);
        let total = self.results.len();

        // Continue an in-progress scrollbar drag regardless of x.
        if self.viewport.scrollbar_dragging && y >= results.y && y < results.y + results.height {
            self.viewport
                .apply_scrollbar_drag(y, total, results.y, results.height);
            self.click_pending = None;
            return;
        }

        // Start a scrollbar drag from the scrollbar column (only when visible).
        if self.viewport.needs_scrollbar(total)
            && super::scroll::is_in_scrollbar_column(results, x, 1)
            && y >= results.y
            && y < results.y + results.height
        {
            self.viewport
                .apply_scrollbar_drag(y, total, results.y, results.height);
            self.click_pending = None;
            return;
        }

        if self.click_pending.is_none() && !self.viewport.dragging {
            return;
        }
        self.click_pending = None;
        self.viewport.apply_content_drag(y, total);
    }

    /// Handle a left-mouse-up inside the search panel. If a click is still
    /// pending (no drag intervened), play the clicked track and close search.
    pub fn handle_mouse_up(&mut self, logic: &bc::Logic) -> Option<SearchAction> {
        let pending = self.click_pending.take();
        let outcome = self.viewport.end_drag();

        if outcome != super::scroll::EndDragOutcome::Idle {
            return None;
        }

        if let Some((_x, _y, index)) = pending
            && let Some(track_id) = self.results.get(index)
        {
            logic.request_play_track(track_id);
            return Some(SearchAction::ToggleSearch);
        }
        None
    }

    /// Handle a mouse-wheel scroll inside the search panel. `direction` is
    /// -1 for up, 1 for down; `steps` is the number of rows to move.
    pub fn handle_scroll(&mut self, direction: i32, steps: usize) {
        self.viewport
            .apply_wheel(direction, steps, self.results.len());
    }

    /// Advance inertia for the search panel by one tick. Returns true if the
    /// viewport moved (caller should redraw).
    pub fn tick_inertia(&mut self) -> bool {
        matches!(
            self.viewport.tick_inertia(self.results.len()),
            super::scroll::InertiaTick::Moved
        )
    }

    pub fn handle_key(&mut self, logic: &bc::Logic, action: Action) -> Option<SearchAction> {
        match action {
            Action::Back => return Some(SearchAction::ToggleSearch),
            Action::Select => {
                if let Some(track_id) = self.results.get(self.selected_index) {
                    logic.request_play_track(track_id);
                    return Some(SearchAction::ToggleSearch);
                }
            }
            Action::GotoSelected => {
                if let Some(track_id) = self.results.get(self.selected_index) {
                    return Some(SearchAction::GotoTrack(track_id.clone()));
                }
            }
            Action::MoveUp if self.selected_index > 0 => {
                self.selected_index -= 1;
                self.ensure_selection_visible();
            }
            Action::MoveDown
                if !self.results.is_empty() && self.selected_index < self.results.len() - 1 =>
            {
                self.selected_index += 1;
                self.ensure_selection_visible();
            }
            Action::DeleteChar => {
                self.query.pop();
                self.update(logic);
            }
            Action::ClearLine => {
                self.query.clear();
                self.update(logic);
            }
            Action::Char(c) => {
                self.query.push(c);
                self.update(logic);
            }
            _ => {}
        }
        None
    }
}

pub fn draw(
    frame: &mut Frame,
    search: &mut SearchState,
    style: &shared_style::Style,
    logic: &bc::Logic,
    hovered_index: Option<usize>,
    area: Rect,
) {
    let block = Block::default()
        .title(" Search ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(style.panels.border().to_color()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(inner);

    search.viewport.visible_height = chunks[1].height as usize;
    search.viewport.clamp(search.results.len());

    // Search input
    let input = Paragraph::new(Line::from(vec![
        Span::styled("> ", Style::default().fg(style.panels.border().to_color())),
        Span::styled(
            &search.query,
            Style::default().fg(style.general.text().to_color()),
        ),
        Span::styled(
            "\u{2588}",
            Style::default().fg(style.panels.border().to_color()),
        ),
    ]));
    frame.render_widget(input, chunks[0]);

    // Search results
    if search.query.len() < 3 {
        let hint = if search.query.is_empty() {
            "Type to search..."
        } else {
            "Enter at least 3 characters..."
        };
        let hint_widget = Paragraph::new(hint)
            .style(Style::default().fg(style.library.track_duration().to_color()));
        frame.render_widget(hint_widget, chunks[1]);
        return;
    }

    if search.results.is_empty() {
        let no_results = Paragraph::new("No results found.")
            .style(Style::default().fg(style.library.track_duration().to_color()));
        frame.render_widget(no_results, chunks[1]);
        return;
    }

    let state_arc = logic.get_state();
    let app_state = state_arc.read().unwrap();

    let items: Vec<ListItem> = search
        .results
        .iter()
        .enumerate()
        .map(|(i, track_id)| {
            let is_selected = i == search.selected_index;
            let is_hovered = hovered_index == Some(i);
            let line = super::render_track_line(
                track_id,
                &app_state,
                style,
                is_selected,
                if is_selected {
                    TrackIndicator::Selected
                } else {
                    TrackIndicator::None
                },
                false,
                chunks[1].width.saturating_sub(1), // -1 for the scrollbar column
            );

            // Underline the hovered row (like the library), except when it is
            // the keyboard-selected row.
            let line = if is_hovered && !is_selected {
                let spans: Vec<Span> = line
                    .spans
                    .into_iter()
                    .map(|s| {
                        let mut s = s;
                        s.style = s.style.add_modifier(Modifier::UNDERLINED);
                        s
                    })
                    .collect();
                Line::from(spans)
            } else {
                line
            };

            ListItem::new(line)
        })
        .collect();

    let list = List::new(items);
    let mut list_state = ListState::default();
    // Drive the viewport manually so wheel/drag scrolling pans independently
    // of the keyboard selection. Leaving `select` as `None` prevents ratatui's
    // auto-scroll from clobbering our offset when the selection is off-screen.
    *list_state.offset_mut() = search.viewport.line;

    frame.render_stateful_widget(list, chunks[1], &mut list_state);

    search.viewport.render_scrollbar(
        frame,
        chunks[1],
        search.results.len(),
        style.library.track_duration().to_color(),
        style.library.track_name_playing().to_color(),
    );
}

/// `(start_y, end_y)` row range covered by the results list, mirroring the
/// layout in `draw` (border + 1-row input + content + border).
fn results_y_range(area: Rect) -> (u16, u16) {
    let start = area.y + 2;
    let end = area.y + area.height.saturating_sub(1);
    (start, end)
}

/// `Rect` covering the results list, matching the layout in `draw`.
fn results_area(area: Rect) -> Rect {
    let (start, end) = results_y_range(area);
    Rect {
        x: area.x + 1,
        y: start,
        width: area.width.saturating_sub(2),
        height: end.saturating_sub(start),
    }
}

/// Computes which search result row the mouse is hovering over, if any.
/// `scroll_line` is the viewport's first visible row (`SearchState.viewport.line`).
pub fn hovered_result_index(
    mouse: Option<(u16, u16)>,
    area: Rect,
    scroll_line: usize,
) -> Option<usize> {
    super::panel::hovered_row(mouse, results_area(area), scroll_line)
}
