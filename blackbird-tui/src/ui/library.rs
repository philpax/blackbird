use std::collections::HashMap;

use blackbird_core::{blackbird_state::CoverArtId, util::seconds_to_hms_string};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, ListState, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use crate::{
    app::{App, LibraryEntry},
    cover_art::QuadrantColors,
};

use super::{StyleExt, string_to_color};

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    // Extract style colors upfront to avoid borrow conflicts later.
    let album_color = app.config.style.album_color();
    let album_year_color = app.config.style.album_year_color();
    let album_length_color = app.config.style.album_length_color();
    let track_number_color = app.config.style.track_number_color();
    let track_name_color = app.config.style.track_name_color();
    let track_name_playing_color = app.config.style.track_name_playing_color();
    let track_length_color = app.config.style.track_length_color();
    let track_duration_color = app.config.style.track_duration_color();
    let track_name_hovered_color = app.config.style.track_name_hovered_color();

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
    // Subtract 1 from width if scrollbar is shown to prevent overlap
    let list_width = inner.width as usize - if has_scrollbar { 1 } else { 0 };

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
                    let heart = if *starred { "\u{2665}" } else { " " };
                    let heart_style = if *starred {
                        Style::default().fg(Color::Red)
                    } else {
                        Style::default().fg(track_duration_color)
                    };

                    let colors = cover_art_id
                        .as_ref()
                        .and_then(|id| art_colors.get(id))
                        .copied()
                        .unwrap_or_default();

                    let year_str = year.map(|y| format!(" ({y})")).unwrap_or_default();
                    let dur_str = seconds_to_hms_string(*duration, false);

                    // Line 1: Album art (rows 0-1) + artist name
                    let line1 = Line::from(vec![
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
                    // Calculate padding for right-alignment
                    let left_content_len = 4 + 1 + album.len() + year_str.len(); // art + space + album + year
                    let right_content = format!(" {dur_str} ");
                    let right_len = right_content.len() + 1; // duration + heart
                    let padding_needed = list_width
                        .saturating_sub(left_content_len + right_len)
                        .saturating_sub(1);

                    let line2 = Line::from(vec![
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

                    let style = if is_selected {
                        Style::default().bg(track_name_hovered_color)
                    } else {
                        Style::default()
                    };

                    ListItem::new(vec![line1, line2]).style(style)
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
                    let heart = if *starred { "\u{2665}" } else { " " };
                    let heart_style = if *starred {
                        Style::default().fg(Color::Red)
                    } else {
                        Style::default().fg(track_duration_color)
                    };

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
                    } else {
                        Style::default().fg(track_name_color)
                    };

                    // Build left side: indent + track number + play icon + title + playcount
                    let mut left_spans = vec![
                        Span::raw("     "),
                        Span::styled(
                            format!("{:>5} ", track_str),
                            Style::default().fg(track_number_color),
                        ),
                    ];

                    let mut left_len = 5 + 6; // indent + track_str formatted

                    if is_playing {
                        left_spans.push(Span::styled(
                            "\u{25B6} ",
                            Style::default()
                                .fg(track_name_playing_color)
                                .add_modifier(Modifier::BOLD),
                        ));
                        left_len += 2;
                    }

                    left_spans.push(Span::styled(title, title_style));
                    left_len += title.len();

                    // Add playcount immediately after title (uses track_number_color like egui)
                    if let Some(pc) = play_count {
                        let pc_str = format!(" {pc}");
                        left_len += pc_str.len();
                        left_spans.push(Span::styled(
                            pc_str,
                            Style::default().fg(track_number_color),
                        ));
                    }

                    // Build right side: [artist] duration heart
                    let mut right_spans = Vec::new();
                    let mut right_len = 0;

                    // Show artist if different from album artist (no dash)
                    if let Some(track_artist) = artist
                        && track_artist != album_artist
                    {
                        let artist_str = format!("{track_artist} ");
                        right_len += artist_str.len();
                        right_spans.push(Span::styled(
                            artist_str,
                            Style::default().fg(string_to_color(track_artist)),
                        ));
                    }

                    right_len += dur_str.len() + 2; // duration + space + heart
                    right_spans.push(Span::styled(
                        dur_str,
                        Style::default().fg(track_length_color),
                    ));
                    right_spans.push(Span::raw(" "));
                    right_spans.push(Span::styled(heart, heart_style));

                    // Calculate padding for right-alignment
                    let padding_needed = list_width
                        .saturating_sub(left_len + right_len)
                        .saturating_sub(1);

                    let mut spans = left_spans;
                    spans.push(Span::raw(" ".repeat(padding_needed)));
                    spans.extend(right_spans);

                    let line = Line::from(spans);

                    let style = if is_selected {
                        Style::default().bg(track_name_hovered_color)
                    } else {
                        Style::default()
                    };

                    ListItem::new(line).style(style)
                }
            }
        })
        .collect();

    let list = List::new(items).highlight_style(
        Style::default()
            .bg(track_name_hovered_color)
            .add_modifier(Modifier::BOLD),
    );

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

    // Render scrollbar on the right edge if needed.
    if has_scrollbar {
        let mut scrollbar_state = ScrollbarState::new(total_lines).position(centered_offset);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some("│"))
            .thumb_symbol("█");
        frame.render_stateful_widget(scrollbar, inner, &mut scrollbar_state);
    }
}
