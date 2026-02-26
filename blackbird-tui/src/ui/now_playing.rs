use blackbird_core::PlaybackMode;
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
};

use crate::{
    app::{App, FocusedPanel},
    ui::album_art_overlay::AlbumArtOverlay,
};

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
    let np = super::layout::split_now_playing(area);

    // -- Album art as 4 coloured quadrants --
    draw_album_art(frame, app, np.album_art, tdd.cover_art_id.as_ref());

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
    let info_area = np.track_info;
    let mouse_pos = app.mouse_position;
    let track_heart_hovered = mouse_pos.is_some_and(|(mx, my)| mx == info_area.x && my == area.y);
    let album_heart_hovered =
        mouse_pos.is_some_and(|(mx, my)| mx == info_area.x && my == area.y + 1);

    let (track_heart, track_heart_style) = heart_to_tui(
        blackbird_client_shared::style::HeartState::from_interaction(
            tdd.starred,
            track_heart_hovered,
        ),
    );
    let (album_heart, album_heart_style) = heart_to_tui(
        blackbird_client_shared::style::HeartState::from_interaction(
            album_starred,
            album_heart_hovered,
        ),
    );

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
    frame.render_widget(info, np.track_info);

    // -- Transport controls --
    draw_transport(frame, app, np.transport);
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

    let mut lines = vec![Line::from(Span::styled(
        " blackbird",
        Style::default()
            .fg(style.track_name_playing_color())
            .add_modifier(Modifier::BOLD),
    ))];

    // Only show the status line once we have track information.
    if has_loaded {
        lines.push(Line::from(Span::styled(
            format!(" {track_count} tracks loaded"),
            Style::default().fg(style.track_duration_color()),
        )));
    } else if track_count > 0 {
        lines.push(Line::from(Span::styled(
            format!(" {track_count} tracks loaded, scanning..."),
            Style::default().fg(style.track_duration_color()),
        )));
    }
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

    let art_width = super::layout::art_cols();
    let art_height = super::layout::ART_TERM_ROWS;
    let left_x = area.x + super::layout::ART_LEFT_MARGIN;

    // Center vertically.
    let top_y = area.y + (area.height.saturating_sub(art_height)) / 2;

    for term_row in 0..art_height.min(area.height) {
        let top = (term_row * 2) as usize;
        let spans = super::art_row_spans(&art, top, top + 1);
        let row_rect = Rect::new(left_x, top_y + term_row, art_width, 1);
        frame.render_widget(Paragraph::new(Line::from(spans)), row_rect);
    }
}

/// Map a [`HeartState`] to a TUI string and style.
fn heart_to_tui(state: blackbird_client_shared::style::HeartState) -> (&'static str, Style) {
    use blackbird_client_shared::style::HeartState;
    match state {
        HeartState::Hidden => (" ", Style::default()),
        HeartState::Preview | HeartState::Active => ("\u{2665}", Style::default().fg(Color::Red)),
        HeartState::HoveredActive => ("\u{2665}", Style::default().fg(Color::White)),
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

/// Handle click in the now-playing area (track info, album info, transport, playback mode).
pub fn handle_mouse_click(app: &mut App, area: Rect, x: u16, y: u16) {
    // Recompute the now-playing horizontal layout.
    let np = super::layout::split_now_playing(area);

    let art_area = np.album_art;
    let info_area = np.track_info;
    let transport_area = np.transport;

    let row = y.saturating_sub(area.y);

    // Click on album art → open overlay
    if x >= art_area.x
        && x < art_area.x + art_area.width
        && let Some(details) = app.logic.get_track_display_details()
        && let Some(cover_art_id) = details.cover_art_id
    {
        app.album_art_overlay = Some(AlbumArtOverlay {
            cover_art_id,
            title: format!("{} \u{2013} {}", details.album_artist, details.album_name),
        });
        return;
    }

    // Click on track info area
    if x >= info_area.x && x < info_area.x + info_area.width {
        if x == info_area.x {
            // Click on heart column → toggle star
            if row == 0 {
                if let Some(track_id) = app.logic.get_playing_track_id() {
                    let starred = app
                        .logic
                        .get_state()
                        .read()
                        .unwrap()
                        .library
                        .track_map
                        .get(&track_id)
                        .map(|t| t.starred)
                        .unwrap_or(false);
                    app.logic.set_track_starred(&track_id, !starred);
                    app.library.mark_dirty();
                }
            } else if row == 1
                && let Some(details) = app.logic.get_track_display_details()
            {
                let starred = app
                    .logic
                    .get_state()
                    .read()
                    .unwrap()
                    .library
                    .albums
                    .get(&details.album_id)
                    .map(|a| a.starred)
                    .unwrap_or(false);
                app.logic.set_album_starred(&details.album_id, !starred);
                app.library.mark_dirty();
            }
        } else {
            // Click on text → navigate to playing track/album
            if row == 0 {
                if let Some(track_id) = app.logic.get_playing_track_id() {
                    app.library.scroll_to_track = Some(track_id);
                    app.focused_panel = FocusedPanel::Library;
                }
            } else if row == 1
                && let Some(details) = app.logic.get_track_display_details()
            {
                app.library.scroll_to_album(&app.logic, &details.album_id);
                app.focused_panel = FocusedPanel::Library;
            }
        }
        return;
    }

    // Click on transport area
    if x >= transport_area.x && x < transport_area.x + transport_area.width {
        if row == 0 {
            // Transport buttons row: "⏮  ▶  ⏹  ⏭" right-aligned in 24 chars
            let rel_x = x.saturating_sub(transport_area.x);
            let btn_start = transport_area
                .width
                .saturating_sub(super::layout::TRANSPORT_BUTTON_GROUP_WIDTH);
            if rel_x >= btn_start {
                let btn_x = rel_x - btn_start;
                match btn_x {
                    super::layout::TRANSPORT_BTN_PREV => app.logic.previous(),
                    super::layout::TRANSPORT_BTN_PLAY => app.logic.toggle_current(),
                    super::layout::TRANSPORT_BTN_STOP => app.logic.stop_current(),
                    super::layout::TRANSPORT_BTN_NEXT => app.logic.next(),
                    _ => {}
                }
            }
        } else if row == 1 {
            // Mode text row: "[mode]" right-aligned → open playback mode dropdown.
            app.playback_mode_dropdown = !app.playback_mode_dropdown;
        }
    }
}

/// The marker prefix for the current mode in the dropdown (" > ").
const DROPDOWN_MARKER_CURRENT: &str = " > ";
/// The marker prefix for non-current modes in the dropdown ("   ").
const DROPDOWN_MARKER_OTHER: &str = "   ";

/// Computes the dropdown rect for the playback mode selector, anchored below
/// the mode text in the transport area and right-aligned to the terminal.
pub fn playback_mode_dropdown_rect(size: Rect) -> Rect {
    let main = super::layout::split_main(size);
    let np = super::layout::split_now_playing(main.now_playing);

    let marker_width = DROPDOWN_MARKER_CURRENT.len() as u16;

    // Width: marker + widest mode label + 1 trailing space + border (2).
    let max_label_width = PlaybackMode::ALL
        .iter()
        .map(|m| m.as_str().len())
        .max()
        .unwrap_or(0) as u16;
    let width = marker_width + max_label_width + 1 + 2;

    // Height: one row per mode + border (2).
    let height = PlaybackMode::ALL.len() as u16 + 2;

    // Anchor: right-aligned to transport area, just below the now-playing row.
    let right_edge = np.transport.x + np.transport.width;
    let x = right_edge.saturating_sub(width);
    let y = main.now_playing.y + main.now_playing.height;

    Rect::new(x, y, width, height)
}

/// Draws the playback mode dropdown overlay.
pub fn draw_playback_mode_dropdown(frame: &mut Frame, app: &App, size: Rect) {
    let style = &app.config.style;
    let rect = playback_mode_dropdown_rect(size);
    let current_mode = app.logic.get_playback_mode();

    frame.render_widget(Clear, rect);

    let block = Block::bordered().style(
        Style::default()
            .fg(style.text_color())
            .bg(style.background_color()),
    );
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    for (i, mode) in PlaybackMode::ALL.iter().enumerate() {
        if i as u16 >= inner.height {
            break;
        }
        let is_current = *mode == current_mode;
        let marker = if is_current {
            DROPDOWN_MARKER_CURRENT
        } else {
            DROPDOWN_MARKER_OTHER
        };
        let label = format!("{marker}{}", mode.as_str());

        // Determine row color: current mode is highlighted, hovered rows
        // turn white (text color), others use the subdued duration color.
        let row_rect = Rect::new(inner.x, inner.y + i as u16, inner.width, 1);
        let hovered = app.mouse_position.is_some_and(|(mx, my)| {
            mx >= row_rect.x && mx < row_rect.x + row_rect.width && my == row_rect.y
        });

        let fg = if is_current {
            style.track_name_playing_color()
        } else if hovered {
            style.text_color()
        } else {
            style.track_duration_color()
        };

        frame.render_widget(
            Paragraph::new(label).style(Style::default().fg(fg).bg(style.background_color())),
            row_rect,
        );
    }
}
