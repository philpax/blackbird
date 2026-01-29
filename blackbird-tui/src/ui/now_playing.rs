use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::app::App;

use super::{StyleExt, string_to_color};

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    // Extract style colors upfront to avoid borrow conflicts.
    let text_color = app.config.style.text_color();
    let album_color = app.config.style.album_color();
    let track_name_playing_color = app.config.style.track_name_playing_color();
    let track_duration_color = app.config.style.track_duration_color();

    let details = app.logic.get_track_display_details();

    let Some(tdd) = details else {
        draw_idle(frame, app, area);
        return;
    };

    // Layout: [album art] [track info] [controls]
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(6), // album art (4 cols x 2 rows of half-blocks + 1 margin + 1 space)
            Constraint::Min(20),   // track info
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

    // Determine heart hover state based on mouse position.
    let info_area = chunks[1];
    let mouse_pos = app.mouse_position;
    let track_heart_hovered = mouse_pos.is_some_and(|(mx, my)| mx == info_area.x && my == area.y);
    let album_heart_hovered =
        mouse_pos.is_some_and(|(mx, my)| mx == info_area.x && my == area.y + 1);

    let (track_heart, track_heart_style) = heart_display(tdd.starred, track_heart_hovered);
    let (album_heart, album_heart_style) = heart_display(album_starred, album_heart_hovered);

    let artist_display = if let Some(ref track_artist) = tdd.track_artist {
        if track_artist.as_str() != tdd.album_artist.as_str() {
            format!("{} ", track_artist)
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
        .unwrap_or(text_color);

    // Line 1: heart [track artist -] track title
    let mut track_spans = vec![Span::styled(track_heart, track_heart_style), Span::raw(" ")];
    if !artist_display.is_empty() {
        track_spans.push(Span::styled(
            artist_display,
            Style::default().fg(artist_color),
        ));
    }
    track_spans.push(Span::styled(
        tdd.track_title.to_string(),
        Style::default()
            .fg(track_name_playing_color)
            .add_modifier(Modifier::BOLD),
    ));

    // Line 2: heart album by artist
    let album_spans = vec![
        Span::styled(album_heart, album_heart_style),
        Span::raw(" "),
        Span::styled(tdd.album_name.to_string(), Style::default().fg(album_color)),
        Span::styled(" by ", Style::default().fg(track_duration_color)),
        Span::styled(
            tdd.album_artist.to_string(),
            Style::default().fg(string_to_color(&tdd.album_artist)),
        ),
    ];

    let info_lines = vec![Line::from(track_spans), Line::from(album_spans)];

    let info = Paragraph::new(info_lines);
    frame.render_widget(info, chunks[1]);

    // -- Transport controls --
    draw_transport(frame, app, chunks[2]);
}

fn draw_idle(frame: &mut Frame, app: &App, area: Rect) {
    let style = &app.config.style;
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
        Line::from(Span::styled(
            " blackbird",
            Style::default()
                .fg(style.track_name_playing_color())
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!(" {status}"),
            Style::default().fg(style.track_duration_color()),
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
    let art = app.cover_art_cache.get(&app.logic, cover_art_id);

    if area.height < 1 || area.width < 6 {
        return;
    }

    // Render 4 cols × 4 rows using half-block characters (U+2580 upper half).
    // Each terminal character shows 2 vertical colors: fg = top row, bg = bottom row.
    // This gives us 4 columns × 4 rows in 2 terminal rows, 4 characters wide.

    let art_width = 4u16; // 4 columns of color
    let art_height = 2u16; // 2 terminal rows (each showing 2 color rows via half-block)
    let left_x = area.x + 1; // +1 for left margin

    // Center vertically
    let top_y = area.y + (area.height.saturating_sub(art_height)) / 2;

    // Terminal row 0: color rows 0-1, Terminal row 1: color rows 2-3
    for term_row in 0..art_height.min(area.height) {
        let row_rect = Rect::new(left_x, top_y + term_row, art_width, 1);
        let mut spans = Vec::new();

        let color_row_top = (term_row * 2) as usize;
        let color_row_bot = color_row_top + 1;

        for col in 0..4 {
            spans.push(Span::styled(
                "\u{2580}",
                Style::default()
                    .fg(art.colors[color_row_top][col])
                    .bg(art.colors[color_row_bot][col]),
            ));
        }

        frame.render_widget(Paragraph::new(Line::from(spans)), row_rect);
    }
}

/// Heart display for the now-playing area, matching the library heart behavior.
/// - Unstarred + not hovered: space (invisible, preserves alignment)
/// - Unstarred + hovered: ♥ in Red (preview)
/// - Starred + not hovered: ♥ in Red
/// - Starred + hovered: ♥ in White (indicate "click to unstar")
fn heart_display(starred: bool, hovered: bool) -> (&'static str, Style) {
    match (starred, hovered) {
        (false, false) => (" ", Style::default()),
        (false, true) => ("\u{2665}", Style::default().fg(Color::Red)),
        (true, false) => ("\u{2665}", Style::default().fg(Color::Red)),
        (true, true) => ("\u{2665}", Style::default().fg(Color::White)),
    }
}

fn draw_transport(frame: &mut Frame, app: &App, area: Rect) {
    let style = &app.config.style;
    let is_playing = app.logic.get_playing_position().is_some();
    let mode = app.logic.get_playback_mode();

    let play_icon = if is_playing { "\u{25B6}" } else { "\u{23F8}" };
    let play_color = if is_playing {
        style.track_name_playing_color()
    } else {
        style.track_name_hovered_color()
    };

    // Transport row:  |<  []  >|
    let transport = Line::from(vec![
        Span::styled("\u{23EE}", Style::default().fg(style.text_color())),
        Span::raw("  "),
        Span::styled(
            play_icon,
            Style::default().fg(play_color).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled("\u{23F9}", Style::default().fg(style.text_color())),
        Span::raw("  "),
        Span::styled("\u{23ED}", Style::default().fg(style.text_color())),
    ]);

    // Mode line
    let mode_line = Line::from(vec![Span::styled(
        format!("[{mode}]"),
        Style::default().fg(style.track_duration_color()),
    )]);

    let lines = vec![transport, mode_line];

    let widget = Paragraph::new(lines).alignment(Alignment::Right);
    frame.render_widget(widget, area);
}
