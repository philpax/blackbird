use blackbird_client_shared::config::SidebarComponent;
use blackbird_core::{
    self as bc, TrackDisplayDetails, blackbird_state::TrackId, util::seconds_to_hms_string,
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use smallvec::SmallVec;

use crate::{app::App, keys::Action};

use super::{ToColor, layout, string_to_color};

/// The sidebar component order/focus wrapper.
///
/// The per-component states (`LyricsViewState`, `SimilarSongsState`) live on
/// `App` (`app.lyrics`, `app.similar_songs`); this struct only holds the
/// configured order and which component has keyboard focus. To add a new
/// component: add a config variant, plus switch arms in `draw_component`,
/// `handle_key`, `handle_mouse_click`, and `handle_scroll`.
#[derive(Debug, Clone)]
pub struct SidebarState {
    /// The configured component order, mapped from the shared config.
    pub order: SmallVec<[SidebarComponentId; 4]>,
    /// Index into `order` of the focused component (when the sidebar has focus).
    pub focused_component: usize,
}

/// Runtime sidebar component IDs, mapped from the shared config enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarComponentId {
    Lyrics,
    SimilarSongs,
}

impl From<SidebarComponent> for SidebarComponentId {
    fn from(c: SidebarComponent) -> Self {
        match c {
            SidebarComponent::Lyrics => SidebarComponentId::Lyrics,
            SidebarComponent::SimilarSongs => SidebarComponentId::SimilarSongs,
        }
    }
}

impl SidebarState {
    /// Builds the runtime order from the shared config.
    pub fn from_config(config: &crate::config::Config) -> Self {
        let order = config
            .layout
            .base
            .sidebar
            .components
            .iter()
            .map(|c| SidebarComponentId::from(*c))
            .collect();
        Self {
            order,
            focused_component: 0,
        }
    }

    /// Rebuilds the order from the shared config, re-mapping the focused
    /// component to the new order so focus doesn't drift. Also rebalances the
    /// proportional heights when the component list changed.
    pub fn update_from_config(&mut self, config: &crate::config::Config) {
        let order: SmallVec<[SidebarComponentId; 4]> = config
            .layout
            .base
            .sidebar
            .components
            .iter()
            .map(|c| SidebarComponentId::from(*c))
            .collect();
        if order.is_empty() {
            self.focused_component = 0;
        } else {
            self.focused_component = self.focused_component.min(order.len() - 1);
        }
        self.order = order;
    }

    /// Whether the sidebar has any components.
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
}

/// State for the similar-songs list, modeled on `SearchState`.
pub struct SimilarSongsState {
    /// Track IDs of the similar songs (filtered to non-directory entries).
    pub results: Vec<TrackId>,
    /// The track this list was fetched for. `None` before the first fetch or
    /// when the state is reset for a new track.
    pub track_id: Option<TrackId>,
    /// Keyboard-selected result index.
    pub selected_index: usize,
    /// Whether a fetch is in flight.
    pub loading: bool,
    /// Shared scroll/drag/inertia mechanism. Each result is one line.
    pub viewport: super::scroll::Scroller,
    /// Pending click at `(x, y, result_index)`. Resolved on mouse-up.
    pub click_pending: Option<(u16, u16, usize)>,
}

impl SimilarSongsState {
    pub fn new() -> Self {
        Self {
            results: Vec::new(),
            track_id: None,
            selected_index: 0,
            loading: false,
            viewport: super::scroll::Scroller::new(),
            click_pending: None,
        }
    }

    /// Resets all state for a new track.
    pub fn reset(&mut self) {
        self.results.clear();
        self.track_id = None;
        self.selected_index = 0;
        self.loading = false;
        self.viewport = super::scroll::Scroller::new();
        self.click_pending = None;
    }

    /// Marks a similar-songs fetch as in flight for `track_id`.
    pub fn on_fetch_started(&mut self, track_id: &TrackId) {
        self.track_id = Some(track_id.clone());
        self.loading = true;
        self.results.clear();
        self.selected_index = 0;
    }

    /// Called when similar-songs data arrives. The caller only forwards data
    /// for the *current* track, so stale responses are already filtered.
    pub fn on_loaded(&mut self, data: &bc::SimilarSongsData) {
        self.track_id = Some(data.track_id.clone());
        self.loading = false;
        // Some servers may include directories in similar-songs results;
        // filter them out so rows always represent playable tracks.
        self.results = data
            .similar
            .iter()
            .filter(|child| !child.is_dir)
            .map(|child| TrackId(child.id.clone()))
            .collect();
        if self.selected_index >= self.results.len() {
            self.selected_index = 0;
        }
        self.viewport.cancel_inertia();
        self.viewport.clamp(self.results.len());
    }

    /// Ensure the selected index is within the visible window.
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

    /// Handle a left-mouse-down inside the similar-songs results area.
    pub fn handle_mouse_click(&mut self, area: Rect, x: u16, y: u16) {
        let results = results_area(area);
        if y < results.y || y >= results.y + results.height || self.results.is_empty() {
            return;
        }

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

    /// Handle a left-mouse-drag inside the similar-songs results area.
    pub fn handle_mouse_drag(&mut self, area: Rect, x: u16, y: u16) {
        let results = results_area(area);
        let total = self.results.len();

        if self.viewport.scrollbar_dragging && y >= results.y && y < results.y + results.height {
            self.viewport
                .apply_scrollbar_drag(y, total, results.y, results.height);
            self.click_pending = None;
            return;
        }

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

    /// Handle a left-mouse-up; returns the clicked track if a click is resolved.
    pub fn handle_mouse_up(&mut self) -> Option<TrackId> {
        let pending = self.click_pending.take();
        let outcome = self.viewport.end_drag();
        if outcome != super::scroll::EndDragOutcome::Idle {
            return None;
        }
        pending
            .and_then(|(_x, _y, index)| self.results.get(index))
            .cloned()
    }

    /// Handle a mouse-wheel scroll.
    pub fn handle_scroll(&mut self, direction: i32, steps: usize) {
        self.viewport
            .apply_wheel(direction, steps, self.results.len());
    }

    /// Advance inertia; returns true if the viewport moved.
    pub fn tick_inertia(&mut self) -> bool {
        matches!(
            self.viewport.tick_inertia(self.results.len()),
            super::scroll::InertiaTick::Moved
        )
    }

    /// Handle a key; plays the selected track on `Select`.
    pub fn handle_key(&mut self, logic: &bc::Logic, action: Action) {
        match action {
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
            Action::PageUp => move_selection_by(self, -(layout::PAGE_SCROLL_SIZE as i32)),
            Action::PageDown => move_selection_by(self, layout::PAGE_SCROLL_SIZE as i32),
            Action::Select => {
                if let Some(track_id) = self.results.get(self.selected_index) {
                    logic.request_play_track(track_id);
                }
            }
            Action::PlayPause => logic.toggle_current(),
            Action::Next => logic.next(),
            Action::Previous => logic.previous(),
            Action::NextGroup => logic.next_group(),
            Action::PreviousGroup => logic.previous_group(),
            _ => {}
        }
    }
}

fn move_selection_by(state: &mut SimilarSongsState, delta: i32) {
    if state.results.is_empty() {
        return;
    }
    let new_index = (state.selected_index as i32 + delta).clamp(0, state.results.len() as i32 - 1);
    state.selected_index = new_index as usize;
    state.ensure_selection_visible();
}

/// Draws the full sidebar: the component sub-areas stacked vertically, each
/// drawing its own bordered block. `area` is the sidebar region from the
/// content layout (no outer border).
pub fn draw_sidebar(frame: &mut Frame, app: &mut App, area: Rect, is_focused: bool) {
    if app.sidebar.is_empty() {
        let msg = Paragraph::new("No sidebar components enabled.")
            .style(Style::default().fg(app.config.style.library.track_duration().to_color()));
        let inner = Rect::new(
            area.x + 1,
            area.y + 1,
            area.width.saturating_sub(2),
            area.height.saturating_sub(2),
        );
        frame.render_widget(msg, inner);
        return;
    }

    let component_rects = sidebar_layout(area, app);
    // Clone the order so `draw_component` can borrow `app` mutably.
    let order: SmallVec<[SidebarComponentId; 4]> = app.sidebar.order.clone();
    for (idx, component) in order.iter().enumerate() {
        let Some(rect) = component_rects.get(idx).copied() else {
            continue;
        };
        let focused = is_focused && idx == app.sidebar.focused_component;
        draw_component(frame, app, *component, rect, focused);
    }
}

/// Draws the full-panel view of the sidebar components (used when no sidebar is
/// visible), filling the content area.
pub fn draw_panel(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.sidebar.is_empty() {
        let msg = Paragraph::new("No sidebar components enabled.")
            .style(Style::default().fg(app.config.style.library.track_duration().to_color()));
        frame.render_widget(msg, area);
        return;
    }

    let component_rects = sidebar_layout(area, app);
    // Clone the order so `draw_component` can borrow `app` mutably.
    let order: SmallVec<[SidebarComponentId; 4]> = app.sidebar.order.clone();
    for (idx, component) in order.iter().enumerate() {
        let Some(rect) = component_rects.get(idx).copied() else {
            continue;
        };
        draw_component(
            frame,
            app,
            *component,
            rect,
            idx == app.sidebar.focused_component,
        );
    }
}

/// Draws a single component into `area`.
fn draw_component(
    frame: &mut Frame,
    app: &mut App,
    component: SidebarComponentId,
    area: Rect,
    focused: bool,
) {
    match component {
        SidebarComponentId::Lyrics => {
            super::lyrics::draw_lyrics_component(
                frame,
                &mut app.lyrics,
                &app.config.style,
                app.logic.get_playing_position(),
                area,
                focused,
                app.mouse_position,
            );
        }
        SidebarComponentId::SimilarSongs => {
            draw_similar_songs(frame, app, area, focused);
        }
    }
}

/// Public wrapper over [`sidebar_layout`] so other modules (e.g. mouse
/// hit-testing in `ui::panel`) can compute component rects.
pub fn layout_for(app: &App, area: Rect) -> SmallVec<[Rect; 4]> {
    sidebar_layout(area, app)
}

/// Splits `area` vertically among the sidebar components by proportional
/// height. The fractions come from `config.layout.base.sidebar.heights`; when
/// they're missing or mismatched, falls back to equal shares.
fn sidebar_layout(area: Rect, app: &App) -> SmallVec<[Rect; 4]> {
    let order = &app.sidebar.order;
    let mut rects = SmallVec::new();
    if order.is_empty() || area.height == 0 {
        return rects;
    }
    let heights = &app.config.layout.base.sidebar.heights;
    let fractions: SmallVec<[f32; 4]> = if heights.len() == order.len() {
        let sum: f32 = heights.iter().sum();
        if sum <= 0.0 {
            SmallVec::from_elem(1.0 / order.len() as f32, order.len())
        } else {
            heights.iter().map(|h| h / sum).collect()
        }
    } else {
        SmallVec::from_elem(1.0 / order.len() as f32, order.len())
    };

    let total = area.height as f32;
    let mut y = area.y;
    for (i, fraction) in fractions.iter().enumerate() {
        let height = if i == fractions.len() - 1 {
            // Last component absorbs any rounding remainder.
            area.y + area.height - y
        } else {
            ((total * fraction).round() as u16).max(1)
        };
        rects.push(Rect::new(area.x, y, area.width, height));
        y += height;
    }
    rects
}

/// Draws the similar-songs component (bordered block + result list).
pub fn draw_similar_songs(frame: &mut Frame, app: &mut App, area: Rect, focused: bool) {
    let style = &app.config.style;
    let block = Block::default()
        .title(" Similar songs ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(style.sidebar.similar_border().to_color()));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let similar = &mut app.similar_songs;

    if similar.loading {
        let loading = Paragraph::new("Loading similar songs...")
            .style(Style::default().fg(style.library.track_duration().to_color()));
        frame.render_widget(loading, inner);
        return;
    }

    if similar.results.is_empty() {
        let msg = if similar.track_id.is_some() {
            "No similar songs available for this track."
        } else {
            "Not supported by server."
        };
        let hint = Paragraph::new(msg)
            .style(Style::default().fg(style.library.track_duration().to_color()));
        frame.render_widget(hint, inner);
        return;
    }

    similar.viewport.visible_height = inner.height as usize;
    similar.viewport.clamp(similar.results.len());

    // Compute which result row the mouse is hovering over (for the underline).
    // The list renders into `inner` (the block's inner area), matching the
    // click handler's `results_area(area) == inner`.
    let hovered_index = if similar.results.is_empty() {
        None
    } else {
        match super::panel::hovered_row(app.mouse_position, inner, similar.viewport.line) {
            Some(idx) if idx < similar.results.len() => Some(idx),
            _ => None,
        }
    };

    let state_arc = app.logic.get_state();
    let app_state = state_arc.read().unwrap();

    let track_name_color = style.library.track_name().to_color();
    let track_length_color = style.library.track_length().to_color();
    let track_name_hovered_color = style.library.track_name_hovered().to_color();
    let track_duration_color = style.library.track_duration().to_color();

    let items: Vec<ListItem> = similar
        .results
        .iter()
        .enumerate()
        .map(|(i, track_id)| {
            let is_selected = focused && i == similar.selected_index;
            let is_hovered = hovered_index == Some(i);
            let details = TrackDisplayDetails::from_track_id(track_id, &app_state);
            let line = if let Some(d) = details {
                let artist = d.artist();
                let dur_str = seconds_to_hms_string(d.track_duration.as_secs() as u32, false);
                // Match the library: the selected row's track title gets the
                // hovered colour (the per-span colours would otherwise override
                // the row style).
                let title_color = if is_selected {
                    track_name_hovered_color
                } else {
                    track_name_color
                };
                Line::from(vec![
                    Span::styled(
                        artist.to_string(),
                        Style::default().fg(string_to_color(artist)),
                    ),
                    Span::raw(" - "),
                    Span::styled(
                        d.track_title.to_string(),
                        Style::default()
                            .fg(title_color)
                            .add_modifier(if is_selected {
                                Modifier::BOLD
                            } else {
                                Modifier::empty()
                            }),
                    ),
                    Span::styled(
                        format!(" [{dur_str}]"),
                        Style::default().fg(if is_selected {
                            track_name_hovered_color
                        } else {
                            track_length_color
                        }),
                    ),
                ])
            } else {
                Line::from(Span::styled(
                    format!("[{track_id}]"),
                    Style::default().fg(track_duration_color),
                ))
            };
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
    *list_state.offset_mut() = similar.viewport.line;

    frame.render_stateful_widget(list, inner, &mut list_state);
    similar.viewport.render_scrollbar(
        frame,
        inner,
        similar.results.len(),
        style.library.track_duration().to_color(),
        style.library.track_name_playing().to_color(),
    );
}

/// Handle a key in the sidebar, dispatching to the focused component.
pub fn handle_key(app: &mut App, action: Action) -> Option<SidebarKeyAction> {
    let component = app
        .sidebar
        .order
        .get(app.sidebar.focused_component)
        .copied()?;
    match component {
        SidebarComponentId::Lyrics => {
            let la = super::lyrics::handle_key(&mut app.lyrics, &app.logic, action);
            match la {
                Some(super::lyrics::LyricsAction::ToggleLyrics) => {
                    Some(SidebarKeyAction::TogglePanel)
                }
                Some(super::lyrics::LyricsAction::Quit) => Some(SidebarKeyAction::Quit),
                Some(super::lyrics::LyricsAction::SeekRelative(secs)) => {
                    Some(SidebarKeyAction::SeekRelative(secs))
                }
                None => None,
            }
        }
        SidebarComponentId::SimilarSongs => {
            // Transport keys work while similar songs has focus too. Seek is
            // returned as an action for main.rs to apply (like lyrics).
            match action {
                Action::Back => Some(SidebarKeyAction::TogglePanel),
                Action::SeekForward => Some(SidebarKeyAction::SeekRelative(
                    super::layout::SEEK_STEP_SECS,
                )),
                Action::SeekBackward => Some(SidebarKeyAction::SeekRelative(
                    -super::layout::SEEK_STEP_SECS,
                )),
                _ => {
                    app.similar_songs.handle_key(&app.logic, action);
                    None
                }
            }
        }
    }
}

/// Actions the sidebar key handler returns to `main.rs`.
pub enum SidebarKeyAction {
    TogglePanel,
    Quit,
    SeekRelative(i64),
}

/// Handle a coalesced scroll in the sidebar, dispatching to the focused
/// component.
pub fn handle_scroll(app: &mut App, direction: i32, steps: usize) {
    let Some(component) = app
        .sidebar
        .order
        .get(app.sidebar.focused_component)
        .copied()
    else {
        return;
    };
    match component {
        SidebarComponentId::Lyrics => {
            if app.lyrics.total_rows > 0 {
                app.lyrics.user_scrolled = true;
                app.lyrics
                    .scroller
                    .apply_wheel(direction, steps, app.lyrics.total_rows);
            }
        }
        SidebarComponentId::SimilarSongs => app.similar_songs.handle_scroll(direction, steps),
    }
}

/// Handle a mouse drag inside the sidebar content, dispatching to the
/// similar-songs component under the cursor. The lyrics component has no
/// content-drag behavior, so drags over it are no-ops.
pub fn handle_mouse_drag(app: &mut App, sidebar_area: Rect, x: u16, y: u16) {
    if app.sidebar.is_empty() {
        return;
    }
    let component_rects = sidebar_layout(sidebar_area, app);
    let order: SmallVec<[SidebarComponentId; 4]> = app.sidebar.order.clone();
    for (idx, component) in order.iter().enumerate() {
        let Some(rect) = component_rects.get(idx).copied() else {
            continue;
        };
        if y >= rect.y && y < rect.y + rect.height {
            if *component == SidebarComponentId::SimilarSongs {
                app.similar_songs.handle_mouse_drag(rect, x, y);
            }
            return;
        }
    }
}

/// Returns the sidebar component boundary index whose dividing row is under
/// `y` (with a small vertical tolerance), or `None`. The boundary between
/// component `i` and `i+1` sits at the top row of component `i+1`.
pub fn boundary_at_y(app: &App, sidebar_area: Rect, y: u16) -> Option<usize> {
    if app.sidebar.is_empty() {
        return None;
    }
    let component_rects = sidebar_layout(sidebar_area, app);
    // Boundaries are the top row of each component after the first, with a
    // 1-row tolerance so the drag handle is grabbable.
    for i in 1..component_rects.len() {
        let top = component_rects[i].y;
        if y >= top.saturating_sub(1) && y <= top {
            return Some(i - 1);
        }
    }
    None
}

/// Adjusts the proportional heights of components `boundary` and
/// `boundary + 1` so the boundary row tracks the drag `y`. The change is
/// applied to `config.layout.base.sidebar.heights`.
pub fn adjust_component_heights(app: &mut App, sidebar_area: Rect, boundary: usize, y: u16) {
    let heights = &mut app.config.layout.base.sidebar.heights;
    if boundary + 1 >= heights.len() {
        return;
    }
    // The boundary's fractional position within the sidebar.
    let inner_top = sidebar_area.y;
    let inner_height = sidebar_area.height.saturating_sub(2).max(1) as f32;
    let frac = ((y.saturating_sub(inner_top)) as f32 / inner_height).clamp(0.0, 1.0);
    // The current boundary fraction (cumulative up to and including component
    // `boundary`).
    let current_frac: f32 = heights[..=boundary].iter().sum();
    let delta = frac - current_frac;
    // Move the boundary by `delta`: component `boundary` grows/shrinks by
    // `delta`, component `boundary + 1` absorbs the opposite.
    heights[boundary] = (heights[boundary] + delta).clamp(0.05, 0.95);
    heights[boundary + 1] = (heights[boundary + 1] - delta).clamp(0.05, 0.95);
    // Normalize so the sum stays 1.
    let sum: f32 = heights.iter().sum();
    if sum > 0.0 {
        for h in heights.iter_mut() {
            *h /= sum;
        }
    }
}

/// Handle a mouse click inside the sidebar content, hit-testing the component
/// sub-areas. `area` is the sidebar region from the content layout (no border);
/// each component has its own border.
pub fn handle_mouse_click(app: &mut App, sidebar_area: Rect, x: u16, y: u16) {
    if app.sidebar.is_empty() {
        return;
    }
    let component_rects = sidebar_layout(sidebar_area, app);
    let order: SmallVec<[SidebarComponentId; 4]> = app.sidebar.order.clone();
    let mut clicked_idx = None;
    for (idx, component) in order.iter().enumerate() {
        let Some(rect) = component_rects.get(idx).copied() else {
            continue;
        };
        // Include the component's own border row in the hit test.
        if y >= rect.y && y < rect.y + rect.height {
            clicked_idx = Some((idx, *component, rect));
            break;
        }
    }
    let Some((idx, component, rect)) = clicked_idx else {
        return;
    };
    app.sidebar.focused_component = idx;
    match component {
        SidebarComponentId::Lyrics => {
            super::lyrics::handle_mouse_click(
                &mut app.lyrics,
                &app.logic,
                &app.config.style,
                rect,
                x,
                y,
            );
        }
        SidebarComponentId::SimilarSongs => {
            app.similar_songs.handle_mouse_click(rect, x, y);
        }
    }
}

/// `Rect` covering the similar-songs results list, matching `draw_similar_songs`
/// (border offset of 1).
fn results_area(area: Rect) -> Rect {
    Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn child(id: &str, is_dir: bool) -> bc::bs::Child {
        bc::bs::Child {
            id: id.to_string(),
            parent: None,
            is_dir,
            title: format!("Track {id}"),
            album: None,
            artist: None,
            track: None,
            year: None,
            genre: None,
            cover_art: None,
            size: None,
            content_type: None,
            suffix: None,
            transcoded_content_type: None,
            transcoded_suffix: None,
            duration: None,
            bit_rate: None,
            path: None,
            is_video: None,
            user_rating: None,
            average_rating: None,
            play_count: None,
            disc_number: None,
            created: None,
            starred: None,
            album_id: None,
            artist_id: None,
            type_: None,
            bookmark_position: None,
            original_width: None,
            original_height: None,
            replay_gain: None,
        }
    }

    fn data(track_id: &str, children: Vec<bc::bs::Child>) -> bc::SimilarSongsData {
        bc::SimilarSongsData {
            track_id: TrackId(track_id.to_string()),
            similar: children,
        }
    }

    /// The state resets when the track changes (AC.3 track keying).
    #[test]
    fn similar_songs_state_track_keying() {
        let mut state = SimilarSongsState::new();
        state.on_fetch_started(&TrackId("current".to_string()));
        state.on_loaded(&data("current", vec![child("a", false)]));
        assert_eq!(
            state.track_id.as_ref().map(|t| t.0.as_str()),
            Some("current")
        );
        assert_eq!(state.results.len(), 1);

        // A new track resets and re-fetches.
        state.reset();
        assert_eq!(state.track_id, None);
        assert!(state.results.is_empty());
    }

    /// Directories are filtered out of similar-songs results (AC.7).
    #[test]
    fn similar_songs_state_filters_directories() {
        let mut state = SimilarSongsState::new();
        state.on_fetch_started(&TrackId("t".to_string()));
        state.on_loaded(&data("t", vec![child("song", false), child("dir", true)]));
        assert_eq!(state.results.len(), 1);
        assert_eq!(state.results[0].0, "song");
    }

    /// The component order is rebuilt from the config (AC.4).
    #[test]
    fn sidebar_component_order_from_config() {
        let mut config = crate::config::Config::default();
        config.layout.base.sidebar.components =
            vec![SidebarComponent::SimilarSongs, SidebarComponent::Lyrics];
        let state = SidebarState::from_config(&config);
        assert_eq!(
            state.order.as_slice(),
            &[SidebarComponentId::SimilarSongs, SidebarComponentId::Lyrics]
        );
    }
}
