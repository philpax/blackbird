use std::collections::HashMap;

use blackbird_core::{blackbird_state::CoverArtId, util::seconds_to_hms_string};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

use crate::{
    app::{App, LibraryEntry},
    cover_art::QuadrantColors,
};

use super::string_to_color;

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    let has_loaded = app.logic.has_loaded_all_tracks();

    let block = Block::default()
        .title(if has_loaded {
            " Library "
        } else {
            " Library (loading...) "
        })
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::White));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if !has_loaded {
        let loading = ratatui::widgets::Paragraph::new("Loading library...")
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(loading, inner);
        return;
    }

    let entries = app.get_flat_library();

    if entries.is_empty() {
        let empty = ratatui::widgets::Paragraph::new("No tracks found")
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(empty, inner);
        return;
    }

    // Calculate visible range to only pre-compute colors for visible group headers.
    let visible_height = inner.height as usize;
    let scroll_offset = app.library_scroll_offset;
    let visible_start = scroll_offset;
    let visible_end = (scroll_offset + visible_height + 5).min(entries.len()); // +5 for buffer

    // Pre-compute quadrant colors only for visible group headers.
    let mut art_colors: HashMap<CoverArtId, QuadrantColors> = HashMap::new();
    for entry in entries
        .iter()
        .skip(visible_start)
        .take(visible_end - visible_start)
    {
        if let LibraryEntry::GroupHeader { cover_art_id, .. } = entry {
            if let Some(id) = cover_art_id {
                if !art_colors.contains_key(id) {
                    let colors = app.cover_art_cache.get(&app.logic, Some(id));
                    art_colors.insert(id.clone(), colors);
                }
            }
        }
    }

    let playing_track_id = app.logic.get_playing_track_id();

    let items: Vec<ListItem> = entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let is_selected = i == app.library_selected_index;
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
                        Style::default().fg(Color::DarkGray)
                    };

                    let colors = cover_art_id
                        .as_ref()
                        .and_then(|id| art_colors.get(id))
                        .copied()
                        .unwrap_or_default();

                    let year_str = year.map(|y| format!(" ({y})")).unwrap_or_default();
                    let dur_str = seconds_to_hms_string(*duration, false);

                    let line = Line::from(vec![
                        // Album art indicator: 4 half-blocks showing 4×2 color grid.
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
                        Span::styled(heart, heart_style),
                        Span::raw(" "),
                        Span::styled(artist, Style::default().fg(string_to_color(artist))),
                        Span::raw(" \u{2014} "),
                        Span::styled(album, Style::default().fg(Color::Rgb(100, 180, 255))),
                        Span::styled(year_str, Style::default().fg(Color::DarkGray)),
                        Span::raw(" "),
                        Span::styled(dur_str, Style::default().fg(Color::DarkGray)),
                    ]);

                    let style = if is_selected {
                        Style::default().bg(Color::Rgb(40, 40, 60))
                    } else {
                        Style::default()
                    };

                    ListItem::new(line).style(style)
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
                        Style::default().fg(Color::DarkGray)
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
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };

                    let mut spans = vec![
                        Span::raw("      "),
                        Span::styled(heart, heart_style),
                        Span::raw(" "),
                        Span::styled(
                            format!("{:>5} ", track_str),
                            Style::default().fg(Color::Rgb(100, 130, 200)),
                        ),
                    ];

                    if is_playing {
                        spans.push(Span::styled(
                            "\u{25B6} ",
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        ));
                    }

                    spans.push(Span::styled(title, title_style));

                    // Show artist if different from album artist.
                    if let Some(track_artist) = artist {
                        if track_artist != album_artist {
                            spans.push(Span::raw(" \u{2014} "));
                            spans.push(Span::styled(
                                track_artist,
                                Style::default().fg(string_to_color(track_artist)),
                            ));
                        }
                    }

                    if let Some(pc) = play_count {
                        spans.push(Span::styled(
                            format!(" ({pc})"),
                            Style::default().fg(Color::DarkGray),
                        ));
                    }

                    spans.push(Span::styled(
                        format!("  {dur_str}"),
                        Style::default().fg(Color::Rgb(100, 180, 150)),
                    ));

                    let line = Line::from(spans);

                    let style = if is_selected {
                        Style::default().bg(Color::Rgb(40, 40, 60))
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
            .bg(Color::Rgb(50, 50, 80))
            .add_modifier(Modifier::BOLD),
    );

    // Use a ListState to manage selection/scrolling.
    let mut state = ListState::default();
    state.select(Some(app.library_selected_index));

    frame.render_stateful_widget(list, inner, &mut state);

    // Save the offset for use by keyboard navigation.
    app.library_scroll_offset = state.offset();
}
