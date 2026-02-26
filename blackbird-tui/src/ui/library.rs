use std::collections::HashMap;

use blackbird_client_shared::library_scroll;
use blackbird_core::{
    self as bc, SortOrder,
    blackbird_state::{CoverArtId, TrackId},
    util::seconds_to_hms_string,
};
use ratatui::{
    Frame,
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, ListState},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    app::App, cover_art::QuadrantColors, keys::Action, ui::album_art_overlay::AlbumArtOverlay,
};

use super::{StyleExt, string_to_color};

/// Returns the width of the scroll indicator based on sort order.
/// Alphabetical uses single letters (1 char), year-based modes use full years (4 chars).
fn scroll_indicator_width(sort_order: SortOrder) -> usize {
    match sort_order {
        SortOrder::Alphabetical => 1,
        SortOrder::NewestFirst | SortOrder::RecentlyAdded => 4,
    }
}

/// A single entry in the flat library list.
#[derive(Debug, Clone)]
pub enum LibraryEntry {
    GroupHeader {
        artist: String,
        album: String,
        year: Option<i32>,
        /// The date the album was added to the library (ISO 8601 format).
        created: Option<String>,
        duration: u32,
        starred: bool,
        album_id: blackbird_core::blackbird_state::AlbumId,
        cover_art_id: Option<blackbird_core::blackbird_state::CoverArtId>,
    },
    Track {
        id: TrackId,
        title: String,
        artist: Option<String>,
        album_artist: String,
        track_number: Option<u32>,
        disc_number: Option<u32>,
        duration: Option<u32>,
        starred: bool,
        play_count: Option<u64>,
    },
}

impl LibraryEntry {
    pub fn height(&self) -> usize {
        match self {
            LibraryEntry::GroupHeader { .. } => 2,
            LibraryEntry::Track { .. } => 1,
        }
    }
}

pub fn total_entry_lines(entries: &[LibraryEntry]) -> usize {
    entries.iter().map(LibraryEntry::height).sum()
}

pub struct LibraryState {
    pub scroll_offset: usize,
    pub selected_index: usize,
    pub needs_scroll_to_playing: bool,
    pub scroll_to_track: Option<TrackId>,

    // Mouse interaction
    pub click_pending: Option<(u16, u16, usize)>,
    pub dragging: bool,
    pub drag_last_y: Option<u16>,
    pub scrollbar_dragging: bool,

    // Private cache
    cached_flat_library: Vec<LibraryEntry>,
    flat_library_dirty: bool,
}

impl LibraryState {
    pub fn new() -> Self {
        Self {
            scroll_offset: 0,
            selected_index: 0,
            needs_scroll_to_playing: true,
            scroll_to_track: None,

            click_pending: None,
            dragging: false,
            drag_last_y: None,
            scrollbar_dragging: false,

            cached_flat_library: Vec::new(),
            flat_library_dirty: true,
        }
    }

    /// Marks the flat library cache as dirty, forcing a rebuild on next access.
    pub fn mark_dirty(&mut self) {
        self.flat_library_dirty = true;
    }

    /// Returns the track ID of the currently selected entry, if it is a track.
    pub fn selected_track_id(&self) -> Option<&TrackId> {
        match self.cached_flat_library.get(self.selected_index)? {
            LibraryEntry::Track { id, .. } => Some(id),
            LibraryEntry::GroupHeader { .. } => None,
        }
    }

    /// Returns the cached flat library, rebuilding if needed.
    pub fn get_flat_library(&mut self, logic: &bc::Logic) -> &[LibraryEntry] {
        if self.flat_library_dirty {
            self.rebuild_flat_library(logic);
            self.flat_library_dirty = false;
        }
        &self.cached_flat_library
    }

    /// Returns the length of the flat library without requiring mutable access.
    pub fn flat_library_len(&self) -> usize {
        self.cached_flat_library.len()
    }

    /// Returns a clone of the entry at the given index, if it exists.
    pub fn get_library_entry(&mut self, logic: &bc::Logic, index: usize) -> Option<LibraryEntry> {
        if self.flat_library_dirty {
            self.rebuild_flat_library(logic);
            self.flat_library_dirty = false;
        }
        self.cached_flat_library.get(index).cloned()
    }

    /// Rebuilds the cached flat library from the current state.
    fn rebuild_flat_library(&mut self, logic: &bc::Logic) {
        let state = logic.get_state();
        let state = state.read().unwrap();

        self.cached_flat_library.clear();
        for group in &state.library.groups {
            let created = state
                .library
                .albums
                .get(&group.album_id)
                .map(|a| a.created.to_string());
            self.cached_flat_library.push(LibraryEntry::GroupHeader {
                artist: group.artist.to_string(),
                album: group.album.to_string(),
                year: group.year,
                created,
                duration: group.duration,
                starred: group.starred,
                album_id: group.album_id.clone(),
                cover_art_id: group.cover_art_id.clone(),
            });

            for track_id in &group.tracks {
                if let Some(track) = state.library.track_map.get(track_id) {
                    self.cached_flat_library.push(LibraryEntry::Track {
                        id: track.id.clone(),
                        title: track.title.to_string(),
                        artist: track.artist.as_ref().map(|a| a.to_string()),
                        album_artist: group.artist.to_string(),
                        track_number: track.track,
                        disc_number: track.disc_number,
                        duration: track.duration,
                        starred: track.starred,
                        play_count: track.play_count,
                    });
                }
            }
        }
    }

    /// Finds the flat index for a given track in the library.
    pub fn find_flat_index_for_track(
        &self,
        state: &bc::AppState,
        target_track_id: &TrackId,
    ) -> Option<usize> {
        let mut index = 0;
        for group in &state.library.groups {
            index += 1; // group header
            for track_id in &group.tracks {
                if track_id == target_track_id {
                    return Some(index);
                }
                if state.library.track_map.contains_key(track_id) {
                    index += 1;
                }
            }
        }
        None
    }

    /// Navigates to the first track in the given album.
    pub fn scroll_to_album(
        &mut self,
        logic: &bc::Logic,
        album_id: &blackbird_core::blackbird_state::AlbumId,
    ) {
        if self.flat_library_dirty {
            self.rebuild_flat_library(logic);
            self.flat_library_dirty = false;
        }
        let mut found_header = false;
        for (i, entry) in self.cached_flat_library.iter().enumerate() {
            match entry {
                LibraryEntry::GroupHeader { album_id: aid, .. } => {
                    found_header = aid == album_id;
                }
                LibraryEntry::Track { .. } if found_header => {
                    self.selected_index = i;
                    return;
                }
                _ => {}
            }
        }
    }

    /// Ensures the current selection is on a track, not a group header.
    /// If currently on a header, moves to the first track in the library.
    pub fn ensure_selection_on_track(&mut self, logic: &bc::Logic) {
        if self.flat_library_dirty {
            self.rebuild_flat_library(logic);
            self.flat_library_dirty = false;
        }

        // Check if current selection is already a track.
        if let Some(LibraryEntry::Track { .. }) = self.cached_flat_library.get(self.selected_index)
        {
            return;
        }

        // Find the first track in the library.
        for (i, entry) in self.cached_flat_library.iter().enumerate() {
            if let LibraryEntry::Track { .. } = entry {
                self.selected_index = i;
                return;
            }
        }
    }
}

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    // Extract style colors upfront to avoid borrow conflicts later.
    let background_color = app.config.style.background_color();
    let text_color = app.config.style.text_color();
    let album_color = app.config.style.album_color();
    let album_year_color = app.config.style.album_year_color();
    let album_length_color = app.config.style.album_length_color();
    let track_number_color = app.config.style.track_number_color();
    let track_name_color = app.config.style.track_name_color();
    let track_name_playing_color = app.config.style.track_name_playing_color();
    let track_length_color = app.config.style.track_length_color();
    let track_duration_color = app.config.style.track_duration_color();
    let track_name_hovered_color = app.config.style.track_name_hovered_color();

    // Determine which entry's heart is being hovered (for hover color effect).
    let hovered_heart_index = compute_hovered_heart_index(app, area);
    // Determine which track entry is being hovered anywhere on the row.
    let hovered_entry_index = compute_hovered_entry_index(app, area);

    let has_loaded = app.logic.has_loaded_all_tracks();

    // Use the full area directly (no frame/border)
    let inner = area;

    if !has_loaded {
        let track_count = app
            .logic
            .get_state()
            .read()
            .unwrap()
            .library
            .track_ids
            .len();
        super::loading::draw(frame, app.tick_count, &app.config.style, track_count, inner);
        return;
    }

    // Copy values we need before borrowing entries.
    let selected_index = app.library.selected_index;
    let playing_track_id = app.logic.get_playing_track_id();

    // Clone entries to avoid borrow conflicts when accessing cover_art_cache later.
    let entries: Vec<LibraryEntry> = app.library.get_flat_library(&app.logic).to_vec();

    if entries.is_empty() {
        let empty = ratatui::widgets::Paragraph::new("No tracks found")
            .style(Style::default().fg(track_duration_color));
        frame.render_widget(empty, inner);
        return;
    }

    // Calculate total content height in lines to determine if scrollbar is needed.
    let total_lines = total_entry_lines(&entries);
    let sort_order = app.logic.get_sort_order();
    let indicator_width = scroll_indicator_width(sort_order);
    let visible_height = inner.height as usize;

    let geo = super::layout::library_geometry(inner, total_lines, indicator_width);
    let has_scrollbar = geo.has_scrollbar;
    let list_width = geo.list_width;

    // Calculate the line offset for center-scroll.
    // GroupHeaders take 2 lines, Tracks take 1 line.
    let mut line_offset = 0usize;
    for (i, entry) in entries.iter().enumerate() {
        if i >= selected_index {
            break;
        }
        line_offset += entry.height();
    }

    // Center the selected item in the visible area.
    let half_height = (geo.visible_height / 2).saturating_sub(1);
    let centered_offset = line_offset.saturating_sub(half_height);

    // Convert line offset back to item offset for ListState.
    let mut item_offset = 0usize;
    let mut current_line = 0usize;
    for (i, entry) in entries.iter().enumerate() {
        if current_line >= centered_offset {
            item_offset = i;
            break;
        }
        current_line += entry.height();
        item_offset = i + 1;
    }
    item_offset = item_offset.min(entries.len().saturating_sub(1));

    // Determine the visible item range: walk forward from item_offset until we
    // exceed the visible height (plus a small buffer for partially visible items).
    let buffer_items = 5;
    let mut visible_item_end = item_offset;
    let mut accumulated_height = 0usize;
    let height_limit = visible_height + buffer_items;
    for entry in entries.iter().skip(item_offset) {
        if accumulated_height >= height_limit {
            break;
        }
        accumulated_height += entry.height();
        visible_item_end += 1;
    }
    visible_item_end = visible_item_end.min(entries.len());

    // Pre-compute quadrant colors only for visible group headers.
    let mut art_colors: HashMap<CoverArtId, QuadrantColors> = HashMap::new();
    for entry in &entries[item_offset..visible_item_end] {
        if let LibraryEntry::GroupHeader {
            cover_art_id: Some(id),
            ..
        } = entry
            && !art_colors.contains_key(id)
        {
            let colors = app.cover_art_cache.get(&app.logic, Some(id));
            art_colors.insert(id.clone(), colors);
        }
    }

    // Build ListItems only for the visible range.
    let items: Vec<ListItem> = entries[item_offset..visible_item_end]
        .iter()
        .enumerate()
        .map(|(vi, entry)| {
            let i = item_offset + vi;
            let is_selected = i == selected_index;
            match entry {
                LibraryEntry::GroupHeader {
                    artist,
                    album,
                    year,
                    created,
                    duration,
                    starred,
                    cover_art_id,
                    ..
                } => {
                    let is_heart_hovered =
                        hovered_heart_index == Some(i) || hovered_entry_index == Some(i);
                    let (heart, heart_style) = heart_to_tui(
                        blackbird_client_shared::style::HeartState::from_interaction(
                            *starred,
                            is_heart_hovered,
                        ),
                        track_duration_color,
                    );

                    let colors = cover_art_id
                        .as_ref()
                        .and_then(|id| art_colors.get(id))
                        .copied()
                        .unwrap_or_default();

                    // Format year and added date.
                    let year_str = year.map(|y| format!(" ({y})")).unwrap_or_default();
                    let added_str = created
                        .as_ref()
                        .and_then(|c| c.get(..10)) // Extract "YYYY-MM-DD" from ISO 8601
                        .map(|d| format!(" +{d}"))
                        .unwrap_or_default();
                    let dur_str = seconds_to_hms_string(*duration, false);

                    // Line 1: Album art (rows 0-1) + artist name
                    let art_cols = super::layout::art_cols();
                    let mut line1_spans = vec![Span::raw(" ")];
                    line1_spans.extend(super::art_row_spans(&colors, 0, 1));
                    line1_spans.push(Span::raw(" "));
                    line1_spans.push(Span::styled(
                        artist,
                        Style::default().fg(string_to_color(artist)),
                    ));
                    let line1 = Line::from(line1_spans);

                    // Line 2: Album art (rows 2-3) + album name + year + added + duration + heart (right-aligned)
                    // Calculate padding for right-alignment using unicode width.
                    let left_content_width = super::layout::ART_LEFT_MARGIN as usize
                        + art_cols as usize
                        + 1
                        + album.width()
                        + year_str.width()
                        + added_str.width(); // margin + art + space + album + year + added
                    let right_content = format!(" {dur_str} ");
                    let right_width = right_content.width() + 1; // duration + heart
                    let padding_needed = list_width
                        .saturating_sub(left_content_width + right_width)
                        .saturating_sub(1);

                    let mut line2_spans = vec![Span::raw(" ")];
                    line2_spans.extend(super::art_row_spans(&colors, 2, 3));
                    line2_spans.push(Span::raw(" "));
                    // Content starts here (album name onward) — underline from this point.
                    let content_start = line2_spans.len();
                    line2_spans.push(Span::styled(album, Style::default().fg(album_color)));
                    line2_spans.push(Span::styled(
                        year_str,
                        Style::default().fg(album_year_color),
                    ));
                    line2_spans.push(Span::styled(
                        added_str,
                        Style::default().fg(album_year_color),
                    ));
                    line2_spans.push(Span::raw(" ".repeat(padding_needed)));
                    line2_spans.push(Span::styled(
                        right_content,
                        Style::default().fg(album_length_color),
                    ));
                    line2_spans.push(Span::styled(heart, heart_style));

                    if hovered_entry_index == Some(i) {
                        for span in &mut line2_spans[content_start..] {
                            span.style = span.style.add_modifier(Modifier::UNDERLINED);
                        }
                    }

                    let line2 = Line::from(line2_spans);

                    ListItem::new(vec![line1, line2])
                }
                LibraryEntry::Track {
                    id,
                    title,
                    artist,
                    album_artist,
                    track_number,
                    disc_number,
                    duration,
                    starred,
                    play_count,
                } => {
                    let is_playing = playing_track_id.as_ref() == Some(id);
                    let is_heart_hovered =
                        hovered_heart_index == Some(i) || hovered_entry_index == Some(i);
                    let (heart, heart_style) = heart_to_tui(
                        blackbird_client_shared::style::HeartState::from_interaction(
                            *starred,
                            is_heart_hovered,
                        ),
                        track_duration_color,
                    );

                    let track_str = if let Some(disc) = disc_number {
                        format!("{disc}.{}", track_number.unwrap_or(0))
                    } else {
                        format!("{}", track_number.unwrap_or(0))
                    };

                    let dur_str = duration
                        .map(|d| seconds_to_hms_string(d, false))
                        .unwrap_or_default();

                    let title_style = if is_playing {
                        Style::default()
                            .fg(track_name_playing_color)
                            .add_modifier(Modifier::BOLD)
                    } else if is_selected {
                        Style::default().fg(track_name_hovered_color)
                    } else {
                        Style::default().fg(track_name_color)
                    };

                    // Build left side: indent + track number + play icon + title + playcount
                    let track_num_formatted = format!("{:>5} ", track_str);
                    let mut left_spans = vec![
                        Span::raw(" ".repeat(super::layout::TRACK_INDENT)),
                        Span::styled(
                            track_num_formatted.clone(),
                            Style::default().fg(track_number_color),
                        ),
                    ];

                    let mut left_width = super::layout::TRACK_INDENT + track_num_formatted.width(); // indent + track_str formatted

                    if is_playing {
                        left_spans.push(Span::styled(
                            "\u{25B6} ",
                            Style::default()
                                .fg(track_name_playing_color)
                                .add_modifier(Modifier::BOLD),
                        ));
                        left_width += 2;
                    }

                    left_spans.push(Span::styled(title, title_style));
                    left_width += title.width();

                    // Add playcount immediately after title (uses track_number_color like egui)
                    if let Some(pc) = play_count {
                        let pc_str = format!(" {pc}");
                        left_width += pc_str.width();
                        left_spans.push(Span::styled(
                            pc_str,
                            Style::default().fg(track_number_color),
                        ));
                    }

                    // Build right side: [artist] duration heart
                    let mut right_spans = Vec::new();
                    let mut right_width = 0;

                    // Show artist if different from album artist (no dash)
                    if let Some(track_artist) = artist
                        && track_artist != album_artist
                    {
                        let artist_str = format!("{track_artist} ");
                        right_width += artist_str.width();
                        right_spans.push(Span::styled(
                            artist_str,
                            Style::default().fg(string_to_color(track_artist)),
                        ));
                    }

                    right_width += dur_str.width() + 2; // duration + space + heart
                    right_spans.push(Span::styled(
                        dur_str,
                        Style::default().fg(track_length_color),
                    ));
                    right_spans.push(Span::raw(" "));
                    right_spans.push(Span::styled(heart, heart_style));

                    // Calculate padding for right-alignment using unicode width
                    let padding_needed = list_width
                        .saturating_sub(left_width + right_width)
                        .saturating_sub(1);

                    let mut spans = left_spans;
                    spans.push(Span::raw(" ".repeat(padding_needed)));
                    spans.extend(right_spans);

                    if hovered_entry_index == Some(i) {
                        // Skip the leading indent span.
                        for span in &mut spans[1..] {
                            span.style = span.style.add_modifier(Modifier::UNDERLINED);
                        }
                    }

                    let line = Line::from(spans);

                    ListItem::new(line)
                }
            }
        })
        .collect();

    let list = List::new(items);

    // Use a ListState with offset 0 since we only built visible items.
    let mut state = ListState::default();
    let relative_selection = selected_index.saturating_sub(item_offset);
    state.select(Some(relative_selection));
    *state.offset_mut() = 0;

    frame.render_stateful_widget(list, inner, &mut state);

    // Save the item_offset for use by keyboard navigation and mouse hit-testing.
    app.library.scroll_offset = item_offset;

    // Render combined scrollbar + library indicator on the right column.
    render_scrollbar_with_library_indicator(
        frame,
        &entries,
        inner,
        geo.visible_height,
        geo.total_lines,
        centered_offset,
        has_scrollbar,
        app.logic.get_sort_order(),
        text_color,
        background_color,
    );
}

/// Map a [`HeartState`] to a TUI string and style.
/// The `dim_color` is used for the hidden state so that the space character
/// inherits the surrounding dim style.
fn heart_to_tui(
    state: blackbird_client_shared::style::HeartState,
    dim_color: Color,
) -> (&'static str, Style) {
    use blackbird_client_shared::style::HeartState;
    match state {
        HeartState::Hidden => (" ", Style::default().fg(dim_color)),
        HeartState::Preview => ("\u{2661}", Style::default().fg(Color::Red)),
        HeartState::Active => ("\u{2665}", Style::default().fg(Color::Red)),
        HeartState::HoveredActive => ("\u{2665}", Style::default().fg(Color::White)),
    }
}

/// Computes which library entry's heart is being hovered by the mouse, if any.
fn compute_hovered_heart_index(app: &mut App, area: Rect) -> Option<usize> {
    // Suppress hover when the playback mode dropdown is covering the library.
    if app.playback_mode_dropdown {
        return None;
    }

    let (mx, my) = app.mouse_position?;

    // Must be within library area
    if my < area.y || my >= area.y + area.height || mx < area.x || mx >= area.x + area.width {
        return None;
    }

    // Capture scroll_offset before borrowing entries.
    let scroll_offset = app.library.scroll_offset;
    let entries = app.library.get_flat_library(&app.logic);

    // Compute list_width and heart column.
    let total_lines = total_entry_lines(entries);
    let indicator_width = scroll_indicator_width(app.logic.get_sort_order());
    let geo = super::layout::library_geometry(area, total_lines, indicator_width);
    let heart_col = geo.heart_col;

    // Check if mouse is on the heart column
    if (mx as usize) < heart_col || (mx as usize) > heart_col + 1 {
        return None;
    }

    // Determine which entry is at this Y position
    let inner_y = my.saturating_sub(area.y) as usize;

    let mut line = 0usize;
    for (i, entry) in entries.iter().enumerate().skip(scroll_offset) {
        let h = entry.height();

        if inner_y >= line && inner_y < line + h {
            // For group headers, heart is only on line 2 (index 1)
            if let LibraryEntry::GroupHeader { .. } = entry {
                if inner_y - line == 1 {
                    return Some(i);
                }
                return None;
            }
            return Some(i);
        }
        line += h;
    }
    None
}

/// Computes which library entry is being hovered by the mouse, if any.
/// Unlike `compute_hovered_heart_index`, this triggers on any X position within the row,
/// not just the heart column. For group headers, only the second line (album line) counts.
fn compute_hovered_entry_index(app: &mut App, area: Rect) -> Option<usize> {
    // Suppress hover when the playback mode dropdown is covering the library.
    if app.playback_mode_dropdown {
        return None;
    }

    let (mx, my) = app.mouse_position?;

    // Must be within library area.
    if my < area.y || my >= area.y + area.height || mx < area.x || mx >= area.x + area.width {
        return None;
    }

    let scroll_offset = app.library.scroll_offset;
    let entries = app.library.get_flat_library(&app.logic);

    let inner_y = my.saturating_sub(area.y) as usize;

    let mut line = 0usize;
    for (i, entry) in entries.iter().enumerate().skip(scroll_offset) {
        let h = entry.height();

        if inner_y >= line && inner_y < line + h {
            match entry {
                LibraryEntry::Track { .. } => return Some(i),
                // Only the second line (album name) triggers hover.
                LibraryEntry::GroupHeader { .. } if inner_y - line == 1 => return Some(i),
                _ => return None,
            }
        }
        line += h;
    }
    None
}

/// Renders a combined scrollbar + library scroll indicator on the rightmost column.
///
/// The indicator shows different labels based on sort order:
/// - Alphabetical: first letter of artist name (A-Z)
/// - NewestFirst: release year
/// - RecentlyAdded: year from the created date
///
/// Each cell in the column shows one of:
/// - Label on thumb: inverted (background fg, text bg) — shows both thumb and label
/// - Label off thumb: normal
/// - Thumb without label: "█" block
/// - Empty track: "│" line (only when scrollbar visible)
#[allow(clippy::too_many_arguments)]
fn render_scrollbar_with_library_indicator(
    frame: &mut Frame,
    entries: &[LibraryEntry],
    area: Rect,
    visible_height: usize,
    total_lines: usize,
    scroll_offset: usize,
    has_scrollbar: bool,
    sort_order: SortOrder,
    text_color: Color,
    background_color: Color,
) {
    use std::borrow::Cow;

    if visible_height == 0 {
        return;
    }

    // Aggregate entries into groups for the shared logic.
    let mut groups: Vec<(Cow<'_, str>, usize)> = Vec::new();
    for entry in entries {
        match entry {
            LibraryEntry::GroupHeader {
                artist,
                year,
                created,
                ..
            } => {
                let label: Cow<'_, str> = match sort_order {
                    SortOrder::Alphabetical => {
                        Cow::Owned(artist.chars().next().unwrap_or('?').to_string())
                    }
                    SortOrder::NewestFirst => Cow::Owned(
                        year.map(|y| y.to_string())
                            .unwrap_or_else(|| "?".to_string()),
                    ),
                    SortOrder::RecentlyAdded => Cow::Owned(
                        created
                            .as_ref()
                            .map(|c| c.chars().take(4).collect::<String>())
                            .filter(|s| !s.is_empty())
                            .unwrap_or_else(|| "?".to_string()),
                    ),
                };
                groups.push((label, entry.height()));
            }
            LibraryEntry::Track { .. } => {
                if let Some(last) = groups.last_mut() {
                    last.1 += entry.height();
                }
            }
        }
    }

    let cluster_threshold = 1.0 / visible_height as f32;
    let positions = library_scroll::compute_positions(groups.into_iter(), cluster_threshold);

    // Build a map of screen row -> label for quick lookup.
    let mut label_at_row: HashMap<u16, &str> = HashMap::new();
    for (label, fraction) in &positions {
        let row = (fraction * visible_height as f32) as u16;
        if row < area.height {
            label_at_row.insert(row, label.as_str());
        }
    }

    // Calculate thumb row range.
    let thumb_range = if has_scrollbar && total_lines > 0 {
        let vh = visible_height as f32;
        let thumb_start_frac = scroll_offset as f32 / total_lines as f32;
        let thumb_size_frac = visible_height as f32 / total_lines as f32;
        let start = (thumb_start_frac * vh) as u16;
        let end = ((thumb_start_frac + thumb_size_frac) * vh).ceil() as u16;
        Some((start, end))
    } else {
        None
    };

    // Rightmost position for right-aligned labels.
    let right_edge = area.x + area.width;
    let buf = frame.buffer_mut();

    for row in 0..area.height {
        let is_thumb = thumb_range
            .map(|(s, e)| row >= s && row < e)
            .unwrap_or(false);
        let label = label_at_row.get(&row);

        // Write label or scrollbar indicator directly to the buffer.
        if let Some(lbl) = label {
            let label_width = lbl.len() as u16;
            let label_x = right_edge.saturating_sub(label_width);
            let style = if is_thumb {
                Style::default().fg(background_color).bg(text_color)
            } else {
                Style::default().fg(text_color)
            };
            for (ci, ch) in lbl.chars().enumerate() {
                let x = label_x + ci as u16;
                let y = area.y + row;
                let pos = Position::new(x, y);
                if area.contains(pos) {
                    let cell = &mut buf[pos];
                    cell.set_char(ch);
                    cell.set_style(style);
                }
            }
        } else {
            let (ch, should_render) = match (is_thumb, has_scrollbar) {
                (true, _) => ('█', true),
                (false, true) => ('│', true),
                (false, false) => (' ', false),
            };
            if should_render {
                let x = right_edge.saturating_sub(1);
                let y = area.y + row;
                let pos = Position::new(x, y);
                if area.contains(pos) {
                    let cell = &mut buf[pos];
                    cell.set_char(ch);
                    cell.set_style(Style::default().fg(text_color));
                }
            }
        }
    }
}

pub fn handle_key(app: &mut App, action: Action) {
    let entries_len = app.library.flat_library_len();

    match action {
        Action::Quit => app.quit_confirming = true,
        Action::PlayPause => app.logic.toggle_current(),
        Action::Next => app.logic.next(),
        Action::Previous => app.logic.previous(),
        Action::NextGroup => app.logic.next_group(),
        Action::PreviousGroup => app.logic.previous_group(),
        Action::Stop => app.logic.stop_current(),
        Action::CyclePlaybackMode => app.cycle_playback_mode(),
        Action::ToggleSortOrder => {
            let scroll_target = app.library.selected_track_id().cloned();
            let next = blackbird_client_shared::toggle_sort_order(app.logic.get_sort_order());
            app.logic.set_sort_order(next);
            app.library.mark_dirty();
            app.library.scroll_to_track = scroll_target;
        }
        Action::Search => app.toggle_search(),
        Action::Lyrics => app.toggle_lyrics(),
        Action::Logs => app.toggle_logs(),
        Action::Queue => app.toggle_queue(),
        Action::VolumeMode => app.volume_editing = true,
        Action::GotoPlaying => {
            if let Some(track_id) = app.logic.get_playing_track_id() {
                app.library.scroll_to_track = Some(track_id);
            }
        }
        Action::SeekBackward => app.seek_relative(-super::layout::SEEK_STEP_SECS),
        Action::SeekForward => app.seek_relative(super::layout::SEEK_STEP_SECS),
        Action::Star => {
            let selected = app.library.selected_index;
            if let Some(entry) = app.library.get_library_entry(&app.logic, selected) {
                match entry {
                    LibraryEntry::Track { id, starred, .. } => {
                        app.logic.set_track_starred(&id, !starred);
                        app.library.mark_dirty();
                    }
                    LibraryEntry::GroupHeader {
                        album_id, starred, ..
                    } => {
                        app.logic.set_album_starred(&album_id, !starred);
                        app.library.mark_dirty();
                    }
                }
            }
        }
        Action::MoveUp => {
            let mut new_index = app.library.selected_index;
            while new_index > 0 {
                new_index -= 1;
                if let Some(LibraryEntry::Track { .. }) =
                    app.library.get_library_entry(&app.logic, new_index)
                {
                    break;
                }
            }
            if let Some(LibraryEntry::Track { .. }) =
                app.library.get_library_entry(&app.logic, new_index)
            {
                app.library.selected_index = new_index;
            }
        }
        Action::MoveDown => {
            let mut new_index = app.library.selected_index;
            while new_index < entries_len.saturating_sub(1) {
                new_index += 1;
                if let Some(LibraryEntry::Track { .. }) =
                    app.library.get_library_entry(&app.logic, new_index)
                {
                    break;
                }
            }
            if let Some(LibraryEntry::Track { .. }) =
                app.library.get_library_entry(&app.logic, new_index)
            {
                app.library.selected_index = new_index;
            }
        }
        Action::PageUp => {
            let target = app
                .library
                .selected_index
                .saturating_sub(super::layout::PAGE_SCROLL_SIZE);
            let mut new_index = target;
            while new_index < entries_len {
                if let Some(LibraryEntry::Track { .. }) =
                    app.library.get_library_entry(&app.logic, new_index)
                {
                    break;
                }
                new_index += 1;
            }
            if new_index < entries_len {
                app.library.selected_index = new_index;
            }
        }
        Action::PageDown => {
            if entries_len > 0 {
                let target = (app.library.selected_index + super::layout::PAGE_SCROLL_SIZE)
                    .min(entries_len - 1);
                let mut new_index = target;
                loop {
                    if let Some(LibraryEntry::Track { .. }) =
                        app.library.get_library_entry(&app.logic, new_index)
                    {
                        break;
                    }
                    if new_index == 0 {
                        break;
                    }
                    new_index -= 1;
                }
                if let Some(LibraryEntry::Track { .. }) =
                    app.library.get_library_entry(&app.logic, new_index)
                {
                    app.library.selected_index = new_index;
                }
            }
        }
        Action::GotoTop => {
            for i in 0..entries_len {
                if let Some(LibraryEntry::Track { .. }) =
                    app.library.get_library_entry(&app.logic, i)
                {
                    app.library.selected_index = i;
                    break;
                }
            }
        }
        Action::GotoBottom => {
            if entries_len > 0 {
                for i in (0..entries_len).rev() {
                    if let Some(LibraryEntry::Track { .. }) =
                        app.library.get_library_entry(&app.logic, i)
                    {
                        app.library.selected_index = i;
                        break;
                    }
                }
            }
        }
        Action::Select => {
            let selected = app.library.selected_index;
            if let Some(LibraryEntry::Track { id, .. }) =
                app.library.get_library_entry(&app.logic, selected)
            {
                app.logic.request_play_track(&id);
            }
        }
        _ => {}
    }
}

/// Handle click in the library area.
pub fn handle_mouse_click(app: &mut App, library_area: Rect, x: u16, y: u16) {
    let entries = app.library.get_flat_library(&app.logic).to_vec();

    // Determine the scroll indicator area (rightmost columns based on sort order).
    let indicator_width = scroll_indicator_width(app.logic.get_sort_order()) as u16;
    let scroll_area_start = library_area.x + library_area.width.saturating_sub(indicator_width);

    // Click on scroll indicator area.
    if x >= scroll_area_start {
        scroll_to_y(app, &entries, library_area, y);
        app.library.scrollbar_dragging = true;
        return;
    }

    // Calculate which entry was clicked
    let inner_y = y.saturating_sub(library_area.y);
    let scroll_offset = app.library.scroll_offset;

    let mut line = 0usize;
    let mut clicked_index = None;
    let mut click_line_in_entry = 0usize;

    for (i, entry) in entries.iter().enumerate().skip(scroll_offset) {
        let h = entry.height();

        if inner_y as usize >= line && (inner_y as usize) < line + h {
            clicked_index = Some(i);
            click_line_in_entry = inner_y as usize - line;
            break;
        }
        line += h;
    }

    let Some(index) = clicked_index else {
        return;
    };
    let Some(entry) = entries.get(index).cloned() else {
        return;
    };

    // Check if clicking on the heart (last content character before scrollbar).
    let total_lines = total_entry_lines(&entries);
    let indicator_width = scroll_indicator_width(app.logic.get_sort_order());
    let geo = super::layout::library_geometry(library_area, total_lines, indicator_width);
    let heart_col = geo.heart_col;
    let is_heart_click = x as usize >= heart_col && x as usize <= heart_col + 1;

    match &entry {
        LibraryEntry::Track { id, starred, .. } => {
            if is_heart_click {
                app.logic.set_track_starred(id, !starred);
                app.library.mark_dirty();
            } else {
                app.library.click_pending = Some((x, y, index));
                app.library.dragging = false;
                app.library.drag_last_y = Some(y);
            }
        }
        LibraryEntry::GroupHeader {
            artist,
            album,
            album_id,
            starred,
            cover_art_id,
            ..
        } => {
            let art_end_col = library_area.x + super::layout::art_end_col();
            if x < art_end_col {
                if let Some(id) = cover_art_id {
                    app.album_art_overlay = Some(AlbumArtOverlay {
                        cover_art_id: id.clone(),
                        title: format!("{artist} \u{2013} {album}"),
                    });
                }
            } else if is_heart_click && click_line_in_entry == 1 {
                app.logic.set_album_starred(album_id, !starred);
                app.library.mark_dirty();
            } else {
                app.library.click_pending = Some((x, y, index));
                app.library.dragging = false;
                app.library.drag_last_y = Some(y);
            }
        }
    }
}

/// Handle mouse drag in the library area. Returns `true` if the drag was handled.
pub fn handle_mouse_drag(app: &mut App, library_area: Rect, x: u16, y: u16) -> bool {
    // Scrollbar drag — once started, continues regardless of x position
    if app.library.scrollbar_dragging
        && y >= library_area.y
        && y < library_area.y + library_area.height
    {
        let entries = app.library.get_flat_library(&app.logic).to_vec();
        scroll_to_y(app, &entries, library_area, y);
        app.library.click_pending = None;
        app.library.dragging = true;
        return true;
    }

    // Check if starting a drag in the scroll indicator area.
    let indicator_width = scroll_indicator_width(app.logic.get_sort_order()) as u16;
    let scroll_area_start = library_area.x + library_area.width.saturating_sub(indicator_width);

    if x >= scroll_area_start && y >= library_area.y && y < library_area.y + library_area.height {
        let entries = app.library.get_flat_library(&app.logic).to_vec();
        scroll_to_y(app, &entries, library_area, y);
        app.library.click_pending = None;
        app.library.dragging = true;
        app.library.scrollbar_dragging = true;
        return true;
    }

    // Content drag → pan library
    if app.library.click_pending.is_some() || app.library.dragging {
        app.library.click_pending = None;
        app.library.dragging = true;

        if let Some(last_y) = app.library.drag_last_y {
            let delta = y as i32 - last_y as i32;
            if delta != 0 {
                let entries_len = app.library.flat_library_len();
                let steps = delta.unsigned_abs() as usize;
                for _ in 0..steps {
                    let mut new_index = app.library.selected_index;
                    if delta > 0 {
                        while new_index > 0 {
                            new_index -= 1;
                            if let Some(LibraryEntry::Track { .. }) =
                                app.library.get_library_entry(&app.logic, new_index)
                            {
                                break;
                            }
                        }
                    } else {
                        while new_index < entries_len.saturating_sub(1) {
                            new_index += 1;
                            if let Some(LibraryEntry::Track { .. }) =
                                app.library.get_library_entry(&app.logic, new_index)
                            {
                                break;
                            }
                        }
                    }
                    if let Some(LibraryEntry::Track { .. }) =
                        app.library.get_library_entry(&app.logic, new_index)
                    {
                        app.library.selected_index = new_index;
                    }
                }
            }
        }
        app.library.drag_last_y = Some(y);
        return true;
    }

    false
}

/// Handle mouse button release in the library — confirm pending click or reset drag state.
pub fn handle_mouse_up(app: &mut App) {
    if let Some((_cx, _cy, index)) = app.library.click_pending.take()
        && !app.library.dragging
        && let Some(LibraryEntry::Track { id, .. }) =
            app.library.get_library_entry(&app.logic, index)
    {
        app.library.selected_index = index;
        app.logic.request_play_track(&id);
    }
    app.library.dragging = false;
    app.library.drag_last_y = None;
    app.library.scrollbar_dragging = false;
}

/// Handle scroll wheel in the library. `direction` is -1 for up, 1 for down.
pub fn handle_scroll(app: &mut App, direction: i32, steps: usize) {
    let entries_len = app.library.flat_library_len();
    for _ in 0..steps {
        let mut new_index = app.library.selected_index;
        if direction < 0 {
            while new_index > 0 {
                new_index -= 1;
                if let Some(LibraryEntry::Track { .. }) =
                    app.library.get_library_entry(&app.logic, new_index)
                {
                    break;
                }
            }
        } else {
            while new_index < entries_len.saturating_sub(1) {
                new_index += 1;
                if let Some(LibraryEntry::Track { .. }) =
                    app.library.get_library_entry(&app.logic, new_index)
                {
                    break;
                }
            }
        }
        if let Some(LibraryEntry::Track { .. }) =
            app.library.get_library_entry(&app.logic, new_index)
        {
            app.library.selected_index = new_index;
        }
    }
}

/// Scroll library to a position based on Y coordinate (for scrollbar dragging).
pub fn scroll_to_y(app: &mut App, entries: &[LibraryEntry], library_area: Rect, y: u16) {
    let visible_height = library_area.height as usize;
    let inner_y = y.saturating_sub(library_area.y);
    let ratio = inner_y as f32 / visible_height as f32;

    let total_lines = total_entry_lines(entries);

    let target_line = ((total_lines as f32) * ratio) as usize;

    let mut current_line = 0usize;
    for (i, entry) in entries.iter().enumerate() {
        let h = entry.height();

        if current_line + h > target_line {
            let mut track_index = i;
            while track_index < entries.len() {
                if let LibraryEntry::Track { .. } = &entries[track_index] {
                    break;
                }
                track_index += 1;
            }
            if track_index < entries.len() {
                app.library.selected_index = track_index;
            }
            return;
        }
        current_line += h;
    }
}
