mod library;
mod lyrics;
mod now_playing;
mod search;

use std::hash::{Hash, Hasher};

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Gauge, Paragraph},
};

use crate::{
    app::{App, FocusedPanel},
    keys,
};

pub fn draw(frame: &mut Frame, app: &mut App) {
    let size = frame.area();

    // Main layout matches egui: [NowPlaying] | [Scrub+Volume] | [Library/Search/Lyrics] | [Help].
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4), // now playing + controls
            Constraint::Length(1), // scrub bar + volume
            Constraint::Min(3),    // library / search / lyrics
            Constraint::Length(1), // help bar
        ])
        .split(size);

    now_playing::draw(frame, app, main_chunks[0]);
    draw_scrub_bar(frame, app, main_chunks[1]);

    match app.focused_panel {
        FocusedPanel::Library => library::draw(frame, app, main_chunks[2]),
        FocusedPanel::Search => search::draw(frame, app, main_chunks[2]),
        FocusedPanel::Lyrics => lyrics::draw(frame, app, main_chunks[2]),
    }

    draw_help_bar(frame, app, main_chunks[3]);
}

/// Hashes a string to produce a pleasing colour (matches the egui client behaviour).
pub fn string_to_color(s: &str) -> Color {
    const DISTINCT_COLOURS: u64 = 36_000;

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut hasher);
    let hash = hasher.finish();
    let hue = (hash % DISTINCT_COLOURS) as f32 / DISTINCT_COLOURS as f32;

    hsv_to_rgb(hue, 0.75, 0.75)
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> Color {
    let c = v * s;
    let h_prime = h * 6.0;
    let x = c * (1.0 - (h_prime % 2.0 - 1.0).abs());
    let m = v - c;

    let (r, g, b) = if h_prime < 1.0 {
        (c, x, 0.0)
    } else if h_prime < 2.0 {
        (x, c, 0.0)
    } else if h_prime < 3.0 {
        (0.0, c, x)
    } else if h_prime < 4.0 {
        (0.0, x, c)
    } else if h_prime < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    Color::Rgb(
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}

fn draw_scrub_bar(frame: &mut Frame, app: &mut App, area: Rect) {
    let details = app.logic.get_track_display_details();

    let (position_secs, duration_secs) = details
        .as_ref()
        .map(|d| {
            (
                d.track_position.as_secs_f32(),
                d.track_duration.as_secs_f32(),
            )
        })
        .unwrap_or((0.0, 0.0));

    let position_str = blackbird_core::util::seconds_to_hms_string(position_secs as u32, true);
    let duration_str = blackbird_core::util::seconds_to_hms_string(duration_secs as u32, true);
    let volume = app.logic.get_volume();
    let vol_str = format!("Vol:{:3.0}%", volume * 100.0);

    let label = format!(" {position_str} / {duration_str} ");

    let ratio = if duration_secs > 0.0 {
        (position_secs / duration_secs).clamp(0.0, 1.0)
    } else {
        0.0
    };

    // Split area: scrub bar | volume.
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(20),
            Constraint::Length(vol_str.len() as u16 + 2),
        ])
        .split(area);

    let gauge = Gauge::default()
        .gauge_style(Style::default().fg(Color::Cyan).bg(Color::Rgb(30, 30, 40)))
        .ratio(ratio as f64)
        .label(label);
    frame.render_widget(gauge, chunks[0]);

    let vol_style = if app.volume_editing {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let vol_widget = Paragraph::new(Span::styled(format!(" {vol_str}"), vol_style));
    frame.render_widget(vol_widget, chunks[1]);
}

fn draw_help_bar(frame: &mut Frame, app: &App, area: Rect) {
    let mode = app.logic.get_playback_mode();

    let help_actions: &[keys::Action] = match app.focused_panel {
        FocusedPanel::Library => keys::LIBRARY_HELP,
        FocusedPanel::Search => keys::SEARCH_HELP,
        FocusedPanel::Lyrics => keys::LYRICS_HELP,
    };

    let mut spans: Vec<Span> = Vec::new();
    spans.push(Span::raw(" "));

    for action in help_actions {
        if let Some((key, label)) = action.help_label() {
            spans.push(Span::styled(key, Style::default().fg(Color::Yellow)));
            spans.push(Span::raw(format!(":{label} ")));
        }
    }

    // Append playback mode for library view.
    if app.focused_panel == FocusedPanel::Library {
        spans.push(Span::styled("m", Style::default().fg(Color::Yellow)));
        spans.push(Span::raw(format!(":mode({mode}) ")));
    }

    let help_line = Line::from(spans);
    let help = Paragraph::new(help_line).style(Style::default().bg(Color::Rgb(30, 30, 40)));
    frame.render_widget(help, area);
}
