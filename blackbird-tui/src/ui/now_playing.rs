use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::app::App;

use super::string_to_color;

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(Color::DarkGray));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let details = app.logic.get_track_display_details();

    let Some(tdd) = details else {
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
            format!("Nothing playing | {track_count} tracks")
        } else {
            format!("Loading tracks... | {track_count} tracks")
        };

        let lines = vec![
            Line::from(Span::styled(status, Style::default().fg(Color::White))),
            Line::from(Span::styled(
                "Select a track to play!",
                Style::default().fg(Color::DarkGray),
            )),
        ];
        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, inner);
        return;
    };

    // Layout: [album art quadrant] [track info] [controls]
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(8), // album art (4x4 blocks = 8 cols x 4 rows)
            Constraint::Min(20),   // track info
            Constraint::Length(30), // controls
        ])
        .split(inner);

    // Draw album art as 4 coloured quadrants
    draw_album_art(frame, app, chunks[0], tdd.cover_art_id.as_ref());

    // Draw track info
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

    let track_heart = if tdd.starred { "\u{2665} " } else { "  " };
    let album_heart = if album_starred { "\u{2665} " } else { "  " };

    let artist_display = if let Some(ref track_artist) = tdd.track_artist {
        if track_artist.as_str() != tdd.album_artist.as_str() {
            format!("{} - ", track_artist)
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

    let mut track_line_spans = vec![
        Span::styled(
            track_heart,
            Style::default().fg(if tdd.starred {
                Color::Red
            } else {
                Color::DarkGray
            }),
        ),
    ];

    if !artist_display.is_empty() {
        track_line_spans.push(Span::styled(
            artist_display,
            Style::default().fg(artist_color),
        ));
    }

    track_line_spans.push(Span::styled(
        tdd.track_title.to_string(),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ));

    let album_line = Line::from(vec![
        Span::styled(
            album_heart,
            Style::default().fg(if album_starred {
                Color::Red
            } else {
                Color::DarkGray
            }),
        ),
        Span::styled(
            tdd.album_name.to_string(),
            Style::default().fg(Color::Rgb(100, 180, 255)),
        ),
        Span::raw(" by "),
        Span::styled(
            tdd.album_artist.to_string(),
            Style::default().fg(string_to_color(&tdd.album_artist)),
        ),
    ]);

    let info_lines = vec![
        Line::from(track_line_spans),
        album_line,
    ];

    let info = Paragraph::new(info_lines);
    frame.render_widget(info, chunks[1]);

    // Draw controls
    if app.logic.is_track_loaded() {
        draw_controls(frame, app, chunks[2]);
    }
}

fn draw_album_art(
    frame: &mut Frame,
    app: &mut App,
    area: Rect,
    cover_art_id: Option<&blackbird_core::blackbird_state::CoverArtId>,
) {
    let colors = app
        .cover_art_cache
        .get(&app.logic, cover_art_id);

    // Render 2x2 quadrants using half-block characters
    // Each "pixel" is a half-block, so 2 rows = 4 colour rows
    // We'll render 2 rows of text, each with upper/lower half blocks
    if area.height < 2 || area.width < 4 {
        return;
    }

    let half_w = area.width / 2;
    let half_h = area.height / 2;

    // Top-left quadrant
    let tl_rect = Rect::new(area.x, area.y, half_w, half_h);
    let tl = Paragraph::new("\u{2588}".repeat(half_w as usize))
        .style(Style::default().fg(colors.top_left));
    frame.render_widget(tl, tl_rect);

    // Top-right quadrant
    let tr_rect = Rect::new(area.x + half_w, area.y, area.width - half_w, half_h);
    let tr = Paragraph::new("\u{2588}".repeat((area.width - half_w) as usize))
        .style(Style::default().fg(colors.top_right));
    frame.render_widget(tr, tr_rect);

    // Bottom-left quadrant
    let bl_rect = Rect::new(area.x, area.y + half_h, half_w, area.height - half_h);
    let bl = Paragraph::new("\u{2588}".repeat(half_w as usize))
        .style(Style::default().fg(colors.bottom_left));
    frame.render_widget(bl, bl_rect);

    // Bottom-right quadrant
    let br_rect = Rect::new(
        area.x + half_w,
        area.y + half_h,
        area.width - half_w,
        area.height - half_h,
    );
    let br = Paragraph::new("\u{2588}".repeat((area.width - half_w) as usize))
        .style(Style::default().fg(colors.bottom_right));
    frame.render_widget(br, br_rect);
}

fn draw_controls(frame: &mut Frame, app: &App, area: Rect) {
    let mode = app.logic.get_playback_mode();
    let mode_str = mode.as_str();

    let is_playing = app
        .logic
        .get_playing_position()
        .is_some();

    let play_indicator = if is_playing { "\u{25B6}" } else { "\u{23F8}" };

    let lines = vec![
        Line::from(vec![
            Span::styled(
                format!(" {play_indicator} "),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" [{mode_str}]"),
                Style::default().fg(Color::Yellow),
            ),
        ]),
        Line::from(Span::styled(
            " \u{23EE} prev | \u{23F9} stop | \u{23ED} next",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let controls = Paragraph::new(lines);
    frame.render_widget(controls, area);
}
