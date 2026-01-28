use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::app::App;

use super::string_to_color;

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    let details = app.logic.get_track_display_details();

    let Some(tdd) = details else {
        draw_idle(frame, app, area);
        return;
    };

    // Layout: [album art] [track info] [controls]
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(9),  // album art (4 cols x 2 rows of half-blocks)
            Constraint::Min(20),    // track info
            Constraint::Length(24), // transport controls
        ])
        .split(area);

    // -- Album art as 4 coloured quadrants --
    draw_album_art(frame, app, chunks[0], tdd.cover_art_id.as_ref());

    // -- Track info --
    let album_starred = app
        .logic
        .get_state()
        .read()
        .unwrap()
        .library
        .albums
        .get(&tdd.album_id)
        .map(|a| a.starred)
        .unwrap_or(false);

    let track_heart = if tdd.starred { "\u{2665} " } else { "" };
    let album_heart = if album_starred { "\u{2665} " } else { "" };

    let artist_display = if let Some(ref track_artist) = tdd.track_artist {
        if track_artist.as_str() != tdd.album_artist.as_str() {
            format!("{} \u{2014} ", track_artist)
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    let artist_color = tdd
        .track_artist
        .as_ref()
        .filter(|a| a.as_str() != tdd.album_artist.as_str())
        .map(|a| string_to_color(a))
        .unwrap_or(Color::White);

    // Line 1: [heart] [track artist -] track title
    let mut track_spans = Vec::new();
    if !track_heart.is_empty() {
        track_spans.push(Span::styled(track_heart, Style::default().fg(Color::Red)));
    }
    if !artist_display.is_empty() {
        track_spans.push(Span::styled(
            artist_display,
            Style::default().fg(artist_color),
        ));
    }
    track_spans.push(Span::styled(
        tdd.track_title.to_string(),
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    ));

    // Line 2: [heart] album by artist
    let mut album_spans = Vec::new();
    if !album_heart.is_empty() {
        album_spans.push(Span::styled(album_heart, Style::default().fg(Color::Red)));
    }
    album_spans.push(Span::styled(
        tdd.album_name.to_string(),
        Style::default().fg(Color::Rgb(100, 180, 255)),
    ));
    album_spans.push(Span::styled(" by ", Style::default().fg(Color::DarkGray)));
    album_spans.push(Span::styled(
        tdd.album_artist.to_string(),
        Style::default().fg(string_to_color(&tdd.album_artist)),
    ));

    let info_lines = vec![
        Line::from(""),
        Line::from(track_spans),
        Line::from(album_spans),
    ];

    let info = Paragraph::new(info_lines);
    frame.render_widget(info, chunks[1]);

    // -- Transport controls --
    draw_transport(frame, app, chunks[2]);
}

fn draw_idle(frame: &mut Frame, app: &App, area: Rect) {
    let track_count = app
        .logic
        .get_state()
        .read()
        .unwrap()
        .library
        .track_ids
        .len();
    let has_loaded = app.logic.has_loaded_all_tracks();

    let status = if has_loaded {
        format!("{track_count} tracks loaded")
    } else if track_count > 0 {
        format!("Loading... ({track_count} tracks)")
    } else {
        "Loading library...".to_string()
    };

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            " blackbird",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!(" {status}"),
            Style::default().fg(Color::DarkGray),
        )),
    ];
    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, area);
}

fn draw_album_art(
    frame: &mut Frame,
    app: &mut App,
    area: Rect,
    cover_art_id: Option<&blackbird_core::blackbird_state::CoverArtId>,
) {
    let colors = app.cover_art_cache.get(&app.logic, cover_art_id);

    if area.height < 2 || area.width < 4 {
        return;
    }

    // Render 2x2 grid of coloured blocks using half-block characters (U+2580 upper half)
    // Each row uses the upper half block with fg = top colour and bg = bottom colour
    // This gives us a 2x2 grid in 2 terminal rows
    let half_w = (area.width - 1) / 2; // -1 for right margin
    let left_x = area.x + 1; // +1 for left margin

    // Center vertically
    let top_y = area.y + (area.height.saturating_sub(2)) / 2;

    // Row 0: top-left / top-right (using upper half block to combine top + bottom)
    let row0 = Rect::new(left_x, top_y, half_w * 2, 1);
    let mut row0_spans = Vec::new();
    // Left half: fg=top_left, bg=bottom_left
    for _ in 0..half_w {
        row0_spans.push(Span::styled(
            "\u{2580}",
            Style::default().fg(colors.top_left).bg(colors.bottom_left),
        ));
    }
    // Right half: fg=top_right, bg=bottom_right
    for _ in 0..half_w {
        row0_spans.push(Span::styled(
            "\u{2580}",
            Style::default()
                .fg(colors.top_right)
                .bg(colors.bottom_right),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(row0_spans)), row0);

    // Row 1: bottom halves (using lower half block)
    let row1 = Rect::new(left_x, top_y + 1, half_w * 2, 1);
    let mut row1_spans = Vec::new();
    for _ in 0..half_w {
        row1_spans.push(Span::styled(
            "\u{2580}",
            Style::default().fg(colors.top_left).bg(colors.bottom_left),
        ));
    }
    for _ in 0..half_w {
        row1_spans.push(Span::styled(
            "\u{2580}",
            Style::default()
                .fg(colors.top_right)
                .bg(colors.bottom_right),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(row1_spans)), row1);
}

fn draw_transport(frame: &mut Frame, app: &App, area: Rect) {
    let is_playing = app.logic.get_playing_position().is_some();
    let mode = app.logic.get_playback_mode();

    let play_icon = if is_playing { "\u{25B6}" } else { "\u{23F8}" };
    let play_color = if is_playing {
        Color::Cyan
    } else {
        Color::Yellow
    };

    // Transport row:  |<  []  >|
    let transport = Line::from(vec![
        Span::styled(" \u{23EE}", Style::default().fg(Color::White)),
        Span::raw("  "),
        Span::styled(
            play_icon,
            Style::default().fg(play_color).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled("\u{23F9}", Style::default().fg(Color::White)),
        Span::raw("  "),
        Span::styled("\u{23ED}", Style::default().fg(Color::White)),
    ]);

    // Mode line
    let mode_line = Line::from(vec![Span::styled(
        format!(" [{mode}]"),
        Style::default().fg(Color::DarkGray),
    )]);

    let lines = vec![Line::from(""), transport, mode_line];

    let widget = Paragraph::new(lines).alignment(Alignment::Left);
    frame.render_widget(widget, area);
}
