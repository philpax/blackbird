use std::collections::HashMap;

use blackbird_client_shared::alphabet_scroll;
use blackbird_core::{blackbird_state::CoverArtId, util::seconds_to_hms_string};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, ListState, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    app::{AlbumArtOverlay, App, LibraryEntry},
    cover_art::QuadrantColors,
    keys::Action,
};

use super::{StyleExt, string_to_color};

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

    let has_loaded = app.logic.has_loaded_all_tracks();

    // Use the full area directly (no frame/border)
    let inner = area;

    if !has_loaded {
        let loading = ratatui::widgets::Paragraph::new("Loading library...")
            .style(Style::default().fg(track_duration_color));
        frame.render_widget(loading, inner);
        return;
    }

    // Copy values we need before borrowing entries.
    let scroll_offset = app.library_scroll_offset;
    let selected_index = app.library_selected_index;
    let playing_track_id = app.logic.get_playing_track_id();

    // Clone entries to avoid borrow conflicts when accessing cover_art_cache later.
    let entries: Vec<LibraryEntry> = app.get_flat_library().to_vec();

    if entries.is_empty() {
        let empty = ratatui::widgets::Paragraph::new("No tracks found")
            .style(Style::default().fg(track_duration_color));
        frame.render_widget(empty, inner);
        return;
    }

    // Calculate visible range to only pre-compute colors for visible group headers.
    let visible_height = inner.height as usize;
    let visible_start = scroll_offset;
    let visible_end = (scroll_offset + visible_height + 5).min(entries.len()); // +5 for buffer

    // Pre-compute quadrant colors only for visible group headers.
    let mut art_colors: HashMap<CoverArtId, QuadrantColors> = HashMap::new();
    for entry in entries
        .iter()
        .skip(visible_start)
        .take(visible_end - visible_start)
    {
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

    // Calculate total content height in lines to determine if scrollbar is needed.
    let total_lines: usize = entries
        .iter()
        .map(|e| match e {
            LibraryEntry::GroupHeader { .. } => 2,
            LibraryEntry::Track { .. } => 1,
        })
        .sum();

    let has_scrollbar = total_lines > visible_height;
    // Subtract 1 for alphabet column (to the right of scrollbar) + 1 for scrollbar if shown
    let list_width = inner.width as usize - 1 - if has_scrollbar { 1 } else { 0 };

    let items: Vec<ListItem> = entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let is_selected = i == selected_index;
            match entry {
                LibraryEntry::GroupHeader {
                    artist,
                    album,
                    year,
                    duration,
                    starred,
                    cover_art_id,
                    ..
                } => {
                    let is_heart_hovered = hovered_heart_index == Some(i);
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

                    let year_str = year.map(|y| format!(" ({y})")).unwrap_or_default();
                    let dur_str = seconds_to_hms_string(*duration, false);

                    // Line 1: Album art (rows 0-1) + artist name
                    let line1 = Line::from(vec![
                        Span::raw(" "), // left margin, same as now playing
                        Span::styled(
                            "\u{2580}",
                            Style::default()
                                .fg(colors.colors[0][0])
                                .bg(colors.colors[1][0]),
                        ),
                        Span::styled(
                            "\u{2580}",
                            Style::default()
                                .fg(colors.colors[0][1])
                                .bg(colors.colors[1][1]),
                        ),
                        Span::styled(
                            "\u{2580}",
                            Style::default()
                                .fg(colors.colors[0][2])
                                .bg(colors.colors[1][2]),
                        ),
                        Span::styled(
                            "\u{2580}",
                            Style::default()
                                .fg(colors.colors[0][3])
                                .bg(colors.colors[1][3]),
                        ),
                        Span::raw(" "),
                        Span::styled(artist, Style::default().fg(string_to_color(artist))),
                    ]);

                    // Line 2: Album art (rows 2-3) + album name + year + duration + heart (right-aligned)
                    // Calculate padding for right-alignment using unicode width
                    let left_content_width = 1 + 4 + 1 + album.width() + year_str.width(); // margin + art + space + album + year
                    let right_content = format!(" {dur_str} ");
                    let right_width = right_content.width() + 1; // duration + heart
                    let padding_needed = list_width
                        .saturating_sub(left_content_width + right_width)
                        .saturating_sub(1);

                    let line2 = Line::from(vec![
                        Span::raw(" "), // left margin, same as now playing
                        Span::styled(
                            "\u{2580}",
                            Style::default()
                                .fg(colors.colors[2][0])
                                .bg(colors.colors[3][0]),
                        ),
                        Span::styled(
                            "\u{2580}",
                            Style::default()
                                .fg(colors.colors[2][1])
                                .bg(colors.colors[3][1]),
                        ),
                        Span::styled(
                            "\u{2580}",
                            Style::default()
                                .fg(colors.colors[2][2])
                                .bg(colors.colors[3][2]),
                        ),
                        Span::styled(
                            "\u{2580}",
                            Style::default()
                                .fg(colors.colors[2][3])
                                .bg(colors.colors[3][3]),
                        ),
                        Span::raw(" "),
                        Span::styled(album, Style::default().fg(album_color)),
                        Span::styled(year_str, Style::default().fg(album_year_color)),
                        Span::raw(" ".repeat(padding_needed)),
                        Span::styled(right_content, Style::default().fg(album_length_color)),
                        Span::styled(heart, heart_style),
                    ]);

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
                    let is_heart_hovered = hovered_heart_index == Some(i);
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
                        Span::raw("     "),
                        Span::styled(
                            track_num_formatted.clone(),
                            Style::default().fg(track_number_color),
                        ),
                    ];

                    let mut left_width = 5 + track_num_formatted.width(); // indent + track_str formatted

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

                    let line = Line::from(spans);

                    ListItem::new(line)
                }
            }
        })
        .collect();

    let list = List::new(items);

    // Use a ListState to manage selection/scrolling with center-scroll behavior.
    let mut state = ListState::default();
    state.select(Some(selected_index));

    // Calculate the line offset for center-scroll.
    // GroupHeaders take 2 lines, Tracks take 1 line.
    let mut line_offset = 0usize;
    for (i, entry) in entries.iter().enumerate() {
        if i >= selected_index {
            break;
        }
        match entry {
            LibraryEntry::GroupHeader { .. } => line_offset += 2,
            LibraryEntry::Track { .. } => line_offset += 1,
        }
    }

    // Center the selected item in the visible area.
    let half_height = (visible_height / 2).saturating_sub(1);
    let centered_offset = line_offset.saturating_sub(half_height);

    // Convert line offset back to item offset for ListState.
    // We need to find which item index corresponds to this line offset.
    let mut item_offset = 0usize;
    let mut current_line = 0usize;
    for (i, entry) in entries.iter().enumerate() {
        if current_line >= centered_offset {
            item_offset = i;
            break;
        }
        match entry {
            LibraryEntry::GroupHeader { .. } => current_line += 2,
            LibraryEntry::Track { .. } => current_line += 1,
        }
        item_offset = i + 1;
    }

    *state.offset_mut() = item_offset.min(entries.len().saturating_sub(1));

    frame.render_stateful_widget(list, inner, &mut state);

    // Save the offset for use by keyboard navigation.
    app.library_scroll_offset = state.offset();

    // Render combined scrollbar + alphabet indicator on the right column.
    render_scrollbar_with_alphabet(
        frame,
        &entries,
        inner,
        visible_height,
        total_lines,
        centered_offset,
        has_scrollbar,
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
        HeartState::Preview | HeartState::Active => ("\u{2665}", Style::default().fg(Color::Red)),
        HeartState::HoveredActive => ("\u{2665}", Style::default().fg(Color::White)),
    }
}

/// Computes which library entry's heart is being hovered by the mouse, if any.
fn compute_hovered_heart_index(app: &mut App, area: Rect) -> Option<usize> {
    let (mx, my) = app.mouse_position?;

    // Must be within library area
    if my < area.y || my >= area.y + area.height || mx < area.x || mx >= area.x + area.width {
        return None;
    }

    // Capture scroll_offset before borrowing entries.
    let scroll_offset = app.library_scroll_offset;
    let entries = app.get_flat_library();

    // Compute list_width and heart column
    let total_lines: usize = entries
        .iter()
        .map(|e| match e {
            LibraryEntry::GroupHeader { .. } => 2,
            LibraryEntry::Track { .. } => 1,
        })
        .sum();
    let visible_height = area.height as usize;
    let has_scrollbar = total_lines > visible_height;
    let list_width = area.width as usize - 1 - if has_scrollbar { 1 } else { 0 };
    let heart_col = area.x as usize + list_width.saturating_sub(2);

    // Check if mouse is on the heart column
    if (mx as usize) < heart_col || (mx as usize) > heart_col + 1 {
        return None;
    }

    // Determine which entry is at this Y position
    let inner_y = my.saturating_sub(area.y) as usize;

    let mut line = 0usize;
    for (i, entry) in entries.iter().enumerate().skip(scroll_offset) {
        let entry_height = match entry {
            LibraryEntry::GroupHeader { .. } => 2,
            LibraryEntry::Track { .. } => 1,
        };

        if inner_y >= line && inner_y < line + entry_height {
            // For group headers, heart is only on line 2 (index 1)
            if let LibraryEntry::GroupHeader { .. } = entry {
                if inner_y - line == 1 {
                    return Some(i);
                }
                return None;
            }
            return Some(i);
        }
        line += entry_height;
    }
    None
}

/// Renders a combined scrollbar + alphabet indicator on the rightmost column.
///
/// Each cell in the column shows one of:
/// - Letter on thumb: inverted letter (background fg, text bg) — shows both thumb and letter
/// - Letter off thumb: normal letter
/// - Thumb without letter: "█" block
/// - Empty track: "│" line (only when scrollbar visible)
#[allow(clippy::too_many_arguments)]
fn render_scrollbar_with_alphabet(
    frame: &mut Frame,
    entries: &[LibraryEntry],
    area: Rect,
    visible_height: usize,
    total_lines: usize,
    scroll_offset: usize,
    has_scrollbar: bool,
    text_color: Color,
    background_color: Color,
) {
    if visible_height == 0 {
        return;
    }

    // Aggregate entries into groups for the shared logic.
    let mut groups: Vec<(char, usize)> = Vec::new();
    for entry in entries {
        match entry {
            LibraryEntry::GroupHeader { artist, .. } => {
                let first_char = artist.chars().next().unwrap_or('?');
                groups.push((first_char, 2));
            }
            LibraryEntry::Track { .. } => {
                if let Some(last) = groups.last_mut() {
                    last.1 += 1;
                }
            }
        }
    }

    let cluster_threshold = 1.0 / visible_height as f32;
    let positions = alphabet_scroll::compute_positions(groups.into_iter(), cluster_threshold);

    // Build a map of screen row -> letter for quick lookup.
    let mut letter_at_row: HashMap<u16, char> = HashMap::new();
    for (letter, fraction) in &positions {
        let row = (fraction * visible_height as f32) as u16;
        if row < area.height {
            letter_at_row.insert(row, *letter);
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

    let col_x = area.x + area.width.saturating_sub(1);

    for row in 0..area.height {
        let is_thumb = thumb_range
            .map(|(s, e)| row >= s && row < e)
            .unwrap_or(false);
        let letter = letter_at_row.get(&row);

        let (content, style) = match (letter, is_thumb, has_scrollbar) {
            // Letter on thumb: inverted
            (Some(ch), true, _) => (
                ch.to_string(),
                Style::default().fg(background_color).bg(text_color),
            ),
            // Letter off thumb
            (Some(ch), false, _) => (ch.to_string(), Style::default().fg(text_color)),
            // Thumb without letter
            (None, true, _) => ("█".to_string(), Style::default().fg(text_color)),
            // Scrollbar track (no thumb, no letter)
            (None, false, true) => ("│".to_string(), Style::default().fg(text_color)),
            // No scrollbar, no letter: skip
            (None, false, false) => continue,
        };

        let span = Span::styled(content, style);
        let rect = Rect::new(col_x, area.y + row, 1, 1);
        frame.render_widget(Paragraph::new(Line::from(span)), rect);
    }
}

pub fn handle_key(app: &mut App, action: Action) {
    let entries_len = app.flat_library_len();

    match action {
        Action::Quit => app.should_quit = true,
        Action::PlayPause => app.logic.toggle_current(),
        Action::Next => app.logic.next(),
        Action::Previous => app.logic.previous(),
        Action::Stop => app.logic.stop_current(),
        Action::CyclePlaybackMode => app.cycle_playback_mode(),
        Action::Search => app.toggle_search(),
        Action::Lyrics => app.toggle_lyrics(),
        Action::Logs => app.toggle_logs(),
        Action::VolumeMode => app.volume_editing = true,
        Action::GotoPlaying => {
            if let Some(track_id) = app.logic.get_playing_track_id() {
                app.scroll_to_track = Some(track_id);
            }
        }
        Action::SeekBackward => app.seek_relative(-5),
        Action::SeekForward => app.seek_relative(5),
        Action::Star => {
            if let Some(entry) = app.get_library_entry(app.library_selected_index) {
                match entry {
                    LibraryEntry::Track { id, starred, .. } => {
                        app.logic.set_track_starred(&id, !starred);
                        app.mark_library_dirty();
                    }
                    LibraryEntry::GroupHeader {
                        album_id, starred, ..
                    } => {
                        app.logic.set_album_starred(&album_id, !starred);
                        app.mark_library_dirty();
                    }
                }
            }
        }
        Action::MoveUp => {
            let mut new_index = app.library_selected_index;
            while new_index > 0 {
                new_index -= 1;
                if let Some(LibraryEntry::Track { .. }) = app.get_library_entry(new_index) {
                    break;
                }
            }
            if let Some(LibraryEntry::Track { .. }) = app.get_library_entry(new_index) {
                app.library_selected_index = new_index;
            }
        }
        Action::MoveDown => {
            let mut new_index = app.library_selected_index;
            while new_index < entries_len.saturating_sub(1) {
                new_index += 1;
                if let Some(LibraryEntry::Track { .. }) = app.get_library_entry(new_index) {
                    break;
                }
            }
            if let Some(LibraryEntry::Track { .. }) = app.get_library_entry(new_index) {
                app.library_selected_index = new_index;
            }
        }
        Action::PageUp => {
            let target = app.library_selected_index.saturating_sub(20);
            let mut new_index = target;
            while new_index < entries_len {
                if let Some(LibraryEntry::Track { .. }) = app.get_library_entry(new_index) {
                    break;
                }
                new_index += 1;
            }
            if new_index < entries_len {
                app.library_selected_index = new_index;
            }
        }
        Action::PageDown => {
            if entries_len > 0 {
                let target = (app.library_selected_index + 20).min(entries_len - 1);
                let mut new_index = target;
                loop {
                    if let Some(LibraryEntry::Track { .. }) = app.get_library_entry(new_index) {
                        break;
                    }
                    if new_index == 0 {
                        break;
                    }
                    new_index -= 1;
                }
                if let Some(LibraryEntry::Track { .. }) = app.get_library_entry(new_index) {
                    app.library_selected_index = new_index;
                }
            }
        }
        Action::Home => {
            for i in 0..entries_len {
                if let Some(LibraryEntry::Track { .. }) = app.get_library_entry(i) {
                    app.library_selected_index = i;
                    break;
                }
            }
        }
        Action::End => {
            if entries_len > 0 {
                for i in (0..entries_len).rev() {
                    if let Some(LibraryEntry::Track { .. }) = app.get_library_entry(i) {
                        app.library_selected_index = i;
                        break;
                    }
                }
            }
        }
        Action::Select => {
            if let Some(LibraryEntry::Track { id, .. }) =
                app.get_library_entry(app.library_selected_index)
            {
                app.logic.request_play_track(&id);
            }
        }
        _ => {}
    }
}

/// Handle click in the library area.
pub fn handle_mouse_click(app: &mut App, library_area: Rect, x: u16, y: u16) {
    let entries = app.get_flat_library().to_vec();
    let scrollbar_x = library_area.x + library_area.width - 1;

    // Click on scrollbar (rightmost column)
    if x == scrollbar_x {
        scroll_to_y(app, &entries, library_area, y);
        app.scrollbar_dragging = true;
        return;
    }

    // Calculate which entry was clicked
    let inner_y = y.saturating_sub(library_area.y);
    let scroll_offset = app.library_scroll_offset;

    let mut line = 0usize;
    let mut clicked_index = None;
    let mut click_line_in_entry = 0usize;

    for (i, entry) in entries.iter().enumerate().skip(scroll_offset) {
        let entry_height = match entry {
            LibraryEntry::GroupHeader { .. } => 2,
            LibraryEntry::Track { .. } => 1,
        };

        if inner_y as usize >= line && (inner_y as usize) < line + entry_height {
            clicked_index = Some(i);
            click_line_in_entry = inner_y as usize - line;
            break;
        }
        line += entry_height;
    }

    let Some(index) = clicked_index else {
        return;
    };
    let Some(entry) = entries.get(index).cloned() else {
        return;
    };

    // Check if clicking on the heart (last content character before scrollbar).
    let total_lines: usize = entries
        .iter()
        .map(|e| match e {
            LibraryEntry::GroupHeader { .. } => 2,
            LibraryEntry::Track { .. } => 1,
        })
        .sum();
    let has_scrollbar = total_lines > library_area.height as usize;
    let list_width = library_area.width as usize - 1 - if has_scrollbar { 1 } else { 0 };
    let heart_col = library_area.x as usize + list_width.saturating_sub(2);
    let is_heart_click = x as usize >= heart_col && x as usize <= heart_col + 1;

    match &entry {
        LibraryEntry::Track { id, starred, .. } => {
            if is_heart_click {
                app.logic.set_track_starred(id, !starred);
                app.mark_library_dirty();
            } else {
                app.library_click_pending = Some((x, y, index));
                app.library_dragging = false;
                app.library_drag_last_y = Some(y);
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
            let art_end_col = library_area.x + 5;
            if x < art_end_col {
                if let Some(id) = cover_art_id {
                    app.album_art_overlay = Some(AlbumArtOverlay {
                        cover_art_id: id.clone(),
                        title: format!("{artist} \u{2013} {album}"),
                    });
                }
            } else if is_heart_click && click_line_in_entry == 1 {
                app.logic.set_album_starred(album_id, !starred);
                app.mark_library_dirty();
            } else {
                app.library_click_pending = Some((x, y, index));
                app.library_dragging = false;
                app.library_drag_last_y = Some(y);
            }
        }
    }
}

/// Handle mouse drag in the library area. Returns `true` if the drag was handled.
pub fn handle_mouse_drag(app: &mut App, library_area: Rect, x: u16, y: u16) -> bool {
    // Scrollbar drag — once started, continues regardless of x position
    if app.scrollbar_dragging
        && y >= library_area.y
        && y < library_area.y + library_area.height
    {
        let entries = app.get_flat_library().to_vec();
        scroll_to_y(app, &entries, library_area, y);
        app.library_click_pending = None;
        app.library_dragging = true;
        return true;
    }

    let scrollbar_x = library_area.x + library_area.width - 1;

    if x == scrollbar_x
        && y >= library_area.y
        && y < library_area.y + library_area.height
    {
        let entries = app.get_flat_library().to_vec();
        scroll_to_y(app, &entries, library_area, y);
        app.library_click_pending = None;
        app.library_dragging = true;
        app.scrollbar_dragging = true;
        return true;
    }

    // Content drag → pan library
    if app.library_click_pending.is_some() || app.library_dragging {
        app.library_click_pending = None;
        app.library_dragging = true;

        if let Some(last_y) = app.library_drag_last_y {
            let delta = y as i32 - last_y as i32;
            if delta != 0 {
                let entries_len = app.flat_library_len();
                let steps = delta.unsigned_abs() as usize;
                for _ in 0..steps {
                    let mut new_index = app.library_selected_index;
                    if delta > 0 {
                        while new_index > 0 {
                            new_index -= 1;
                            if let Some(LibraryEntry::Track { .. }) =
                                app.get_library_entry(new_index)
                            {
                                break;
                            }
                        }
                    } else {
                        while new_index < entries_len.saturating_sub(1) {
                            new_index += 1;
                            if let Some(LibraryEntry::Track { .. }) =
                                app.get_library_entry(new_index)
                            {
                                break;
                            }
                        }
                    }
                    if let Some(LibraryEntry::Track { .. }) =
                        app.get_library_entry(new_index)
                    {
                        app.library_selected_index = new_index;
                    }
                }
            }
        }
        app.library_drag_last_y = Some(y);
        return true;
    }

    false
}

/// Handle mouse button release in the library — confirm pending click or reset drag state.
pub fn handle_mouse_up(app: &mut App) {
    if let Some((_cx, _cy, index)) = app.library_click_pending.take()
        && !app.library_dragging
        && let Some(LibraryEntry::Track { id, .. }) = app.get_library_entry(index)
    {
        app.library_selected_index = index;
        app.logic.request_play_track(&id);
    }
    app.library_dragging = false;
    app.library_drag_last_y = None;
    app.scrollbar_dragging = false;
}

/// Handle scroll wheel in the library. `direction` is -1 for up, 1 for down.
pub fn handle_scroll(app: &mut App, direction: i32, steps: usize) {
    let entries_len = app.flat_library_len();
    for _ in 0..steps {
        let mut new_index = app.library_selected_index;
        if direction < 0 {
            while new_index > 0 {
                new_index -= 1;
                if let Some(LibraryEntry::Track { .. }) = app.get_library_entry(new_index) {
                    break;
                }
            }
        } else {
            while new_index < entries_len.saturating_sub(1) {
                new_index += 1;
                if let Some(LibraryEntry::Track { .. }) = app.get_library_entry(new_index) {
                    break;
                }
            }
        }
        if let Some(LibraryEntry::Track { .. }) = app.get_library_entry(new_index) {
            app.library_selected_index = new_index;
        }
    }
}

/// Scroll library to a position based on Y coordinate (for scrollbar dragging).
pub fn scroll_to_y(app: &mut App, entries: &[LibraryEntry], library_area: Rect, y: u16) {
    let visible_height = library_area.height as usize;
    let inner_y = y.saturating_sub(library_area.y);
    let ratio = inner_y as f32 / visible_height as f32;

    let total_lines: usize = entries
        .iter()
        .map(|e| match e {
            LibraryEntry::GroupHeader { .. } => 2,
            LibraryEntry::Track { .. } => 1,
        })
        .sum();

    let target_line = ((total_lines as f32) * ratio) as usize;

    let mut current_line = 0usize;
    for (i, entry) in entries.iter().enumerate() {
        let entry_height = match entry {
            LibraryEntry::GroupHeader { .. } => 2,
            LibraryEntry::Track { .. } => 1,
        };

        if current_line + entry_height > target_line {
            let mut track_index = i;
            while track_index < entries.len() {
                if let LibraryEntry::Track { .. } = &entries[track_index] {
                    break;
                }
                track_index += 1;
            }
            if track_index < entries.len() {
                app.library_selected_index = track_index;
            }
            return;
        }
        current_line += entry_height;
    }
}
