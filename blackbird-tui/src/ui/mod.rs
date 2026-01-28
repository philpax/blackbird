mod library;
mod logs;
mod lyrics;
mod now_playing;
mod search;

use blackbird_client_shared::style as shared_style;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Gauge, Paragraph},
};

use crate::{
    app::{App, FocusedPanel},
    keys,
};

/// Converts a shared style Rgb color to ratatui's Color.
fn rgb_to_color(rgb: shared_style::Rgb) -> Color {
    Color::Rgb(rgb.r, rgb.g, rgb.b)
}

/// Converts HSV to gamma-corrected ratatui Color to match egui's rendering.
fn hsv_to_color_gamma(hsv: shared_style::Hsv) -> Color {
    rgb_to_color(shared_style::hsv_to_rgb_gamma(hsv))
}

/// Extension trait for using shared style colors with ratatui.
/// Uses gamma-corrected colors to match egui's appearance.
pub trait StyleExt {
    fn background_color(&self) -> Color;
    fn text_color(&self) -> Color;
    fn album_color(&self) -> Color;
    fn album_length_color(&self) -> Color;
    fn album_year_color(&self) -> Color;
    fn track_number_color(&self) -> Color;
    fn track_length_color(&self) -> Color;
    fn track_name_color(&self) -> Color;
    fn track_name_hovered_color(&self) -> Color;
    fn track_name_playing_color(&self) -> Color;
    fn track_duration_color(&self) -> Color;
}

impl StyleExt for shared_style::Style {
    fn background_color(&self) -> Color {
        hsv_to_color_gamma(self.background_hsv)
    }
    fn text_color(&self) -> Color {
        hsv_to_color_gamma(self.text_hsv)
    }
    fn album_color(&self) -> Color {
        hsv_to_color_gamma(self.album_hsv)
    }
    fn album_length_color(&self) -> Color {
        hsv_to_color_gamma(self.album_length_hsv)
    }
    fn album_year_color(&self) -> Color {
        hsv_to_color_gamma(self.album_year_hsv)
    }
    fn track_number_color(&self) -> Color {
        hsv_to_color_gamma(self.track_number_hsv)
    }
    fn track_length_color(&self) -> Color {
        hsv_to_color_gamma(self.track_length_hsv)
    }
    fn track_name_color(&self) -> Color {
        hsv_to_color_gamma(self.track_name_hsv)
    }
    fn track_name_hovered_color(&self) -> Color {
        hsv_to_color_gamma(self.track_name_hovered_hsv)
    }
    fn track_name_playing_color(&self) -> Color {
        hsv_to_color_gamma(self.track_name_playing_hsv)
    }
    fn track_duration_color(&self) -> Color {
        hsv_to_color_gamma(self.track_duration_hsv)
    }
}

pub fn draw(frame: &mut Frame, app: &mut App) {
    let size = frame.area();

    // Fill entire terminal with background color.
    let bg = Block::default().style(Style::default().bg(app.config.style.background_color()));
    frame.render_widget(bg, size);

    // Main layout matches egui: [NowPlaying] | [Scrub+Volume] | [Library/Search/Lyrics] | [Help].
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // now playing + controls (2 lines, no margin)
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
        FocusedPanel::Logs => logs::draw(frame, app, main_chunks[2]),
    }

    draw_help_bar(frame, app, main_chunks[3]);
}

/// Hashes a string to produce a pleasing colour (uses shared implementation).
/// Uses gamma-corrected version to match egui's color rendering.
pub fn string_to_color(s: &str) -> Color {
    rgb_to_color(shared_style::string_to_rgb_gamma(s))
}

fn draw_scrub_bar(frame: &mut Frame, app: &mut App, area: Rect) {
    let style = &app.config.style;
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
        .gauge_style(
            Style::default()
                .fg(style.track_name_playing_color())
                .bg(style.background_color()),
        )
        .ratio(ratio as f64)
        .label(label);
    frame.render_widget(gauge, chunks[0]);

    let vol_style = if app.volume_editing {
        Style::default()
            .fg(style.track_name_playing_color())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(style.track_duration_color())
    };
    let vol_widget = Paragraph::new(Span::styled(format!(" {vol_str}"), vol_style));
    frame.render_widget(vol_widget, chunks[1]);
}

fn draw_help_bar(frame: &mut Frame, app: &App, area: Rect) {
    let style = &app.config.style;
    let mode = app.logic.get_playback_mode();

    let help_actions: &[keys::Action] = match app.focused_panel {
        FocusedPanel::Library => keys::LIBRARY_HELP,
        FocusedPanel::Search => keys::SEARCH_HELP,
        FocusedPanel::Lyrics => keys::LYRICS_HELP,
        FocusedPanel::Logs => keys::LOGS_HELP,
    };

    let mut spans: Vec<Span> = Vec::new();
    spans.push(Span::raw(" "));

    for action in help_actions {
        if let Some((key, label)) = action.help_label() {
            spans.push(Span::styled(
                key,
                Style::default().fg(style.track_name_playing_color()),
            ));
            spans.push(Span::raw(format!(":{label} ")));
        }
    }

    // Append playback mode for library view.
    if app.focused_panel == FocusedPanel::Library {
        spans.push(Span::styled(
            "m",
            Style::default().fg(style.track_name_playing_color()),
        ));
        spans.push(Span::raw(format!(":mode({mode}) ")));
    }

    let help_line = Line::from(spans);
    let help = Paragraph::new(help_line).style(Style::default().bg(style.background_color()));
    frame.render_widget(help, area);
}
