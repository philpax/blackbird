pub mod album_art_overlay;
pub(crate) mod layout;
pub(crate) mod library;
pub(crate) mod logs;
pub(crate) mod lyrics;
pub(crate) mod now_playing;
pub(crate) mod search;

use blackbird_client_shared::style as shared_style;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Gauge, Paragraph},
};

use std::time::Duration;

use smol_str::ToSmolStr as _;

use crate::{
    app::{App, FocusedPanel},
    cover_art::ArtColors,
    keys,
};

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
        hsv_to_color(self.background_hsv)
    }
    fn text_color(&self) -> Color {
        hsv_to_color(self.text_hsv)
    }
    fn album_color(&self) -> Color {
        hsv_to_color(self.album_hsv)
    }
    fn album_length_color(&self) -> Color {
        hsv_to_color(self.album_length_hsv)
    }
    fn album_year_color(&self) -> Color {
        hsv_to_color(self.album_year_hsv)
    }
    fn track_number_color(&self) -> Color {
        hsv_to_color(self.track_number_hsv)
    }
    fn track_length_color(&self) -> Color {
        hsv_to_color(self.track_length_hsv)
    }
    fn track_name_color(&self) -> Color {
        hsv_to_color(self.track_name_hsv)
    }
    fn track_name_hovered_color(&self) -> Color {
        hsv_to_color(self.track_name_hovered_hsv)
    }
    fn track_name_playing_color(&self) -> Color {
        hsv_to_color(self.track_name_playing_hsv)
    }
    fn track_duration_color(&self) -> Color {
        hsv_to_color(self.track_duration_hsv)
    }
}
/// Converts a shared style Rgb color to ratatui's Color.
fn rgb_to_color(rgb: shared_style::Rgb) -> Color {
    Color::Rgb(rgb.r, rgb.g, rgb.b)
}
fn hsv_to_color(hsv: shared_style::Hsv) -> Color {
    // from egui, fusing together hsv conversion and gamma correction
    /// All ranges in 0-1, rgb is linear.
    #[inline]
    pub fn from_hsv([h, s, v]: shared_style::Hsv) -> shared_style::Rgb {
        #![allow(clippy::many_single_char_names)]
        let h = (h.fract() + 1.0).fract(); // wrap
        let s = s.clamp(0.0, 1.0);

        let f = h * 6.0 - (h * 6.0).floor();
        let p = v * (1.0 - s);
        let q = v * (1.0 - f * s);
        let t = v * (1.0 - (1.0 - f) * s);

        let [r, g, b] = match (h * 6.0).floor() as i32 % 6 {
            0 => [v, t, p],
            1 => [q, v, p],
            2 => [p, v, t],
            3 => [p, q, v],
            4 => [t, p, v],
            5 => [v, p, q],
            _ => unreachable!(),
        };

        pub fn gamma_u8_from_linear_f32(l: f32) -> u8 {
            if l <= 0.0 {
                0
            } else if l <= 0.0031308 {
                fast_round(3294.6 * l)
            } else if l <= 1.0 {
                fast_round(269.025 * l.powf(1.0 / 2.4) - 14.025)
            } else {
                255
            }
        }

        fn fast_round(r: f32) -> u8 {
            (r + 0.5) as _ // rust does a saturating cast since 1.45
        }

        shared_style::Rgb::new(
            gamma_u8_from_linear_f32(r),
            gamma_u8_from_linear_f32(g),
            gamma_u8_from_linear_f32(b),
        )
    }
    rgb_to_color(from_hsv(hsv))
}

/// Builds half-block art spans for one terminal row from a 4x4 color grid,
/// stretching to [`layout::art_cols()`] display columns via nearest-neighbor
/// mapping.
fn art_row_spans(colors: &ArtColors, top_row: usize, bot_row: usize) -> Vec<Span<'static>> {
    let cols = layout::art_cols();
    let mut spans = Vec::with_capacity(cols as usize);
    for col in 0..cols {
        let data_col = col as usize * 4 / cols as usize;
        spans.push(Span::styled(
            "\u{2580}",
            Style::default()
                .fg(colors.colors[top_row][data_col])
                .bg(colors.colors[bot_row][data_col]),
        ));
    }
    spans
}

pub fn draw(frame: &mut Frame, app: &mut App) {
    let size = frame.area();

    // Fill entire terminal with background color.
    let bg = Block::default().style(Style::default().bg(app.config.style.background_color()));
    frame.render_widget(bg, size);

    // Main layout matches egui: [NowPlaying] | [Scrub+Volume] | [Library/Search/Lyrics] | [Help].
    let main = layout::split_main(size);

    now_playing::draw(frame, app, main.now_playing);
    draw_scrub_bar(frame, app, main.scrub_bar);

    match app.focused_panel {
        FocusedPanel::Library => library::draw(frame, app, main.content),
        FocusedPanel::Search => search::draw(
            frame,
            &app.search,
            &app.config.style,
            &app.logic,
            main.content,
        ),
        FocusedPanel::Lyrics => lyrics::draw(
            frame,
            &app.lyrics,
            &app.config.style,
            app.logic.get_playing_position(),
            main.content,
        ),
        FocusedPanel::Logs => logs::draw(frame, &mut app.logs, &app.config.style, main.content),
    }

    draw_help_bar(frame, app, main.help_bar);

    // Draw album art overlay on top of everything if active.
    if app.album_art_overlay.is_some() {
        album_art_overlay::draw(frame, app, size);
    }

    // Draw quit confirmation dialog on top of everything.
    if app.quit_confirming {
        let yes = keys::KEY_CONFIRM_YES.to_smolstr();
        let no = keys::KEY_CONFIRM_NO.to_smolstr();
        let prompt = format!("Quit? {yes}/{no}");
        let popup_width = prompt.len() as u16 + 4; // border (2) + padding (2)
        let popup_height = 3_u16;
        let x = size.x + (size.width.saturating_sub(popup_width)) / 2;
        let y = size.y + (size.height.saturating_sub(popup_height)) / 2;
        let popup_area = Rect::new(x, y, popup_width, popup_height);

        // Clear the area behind the popup.
        let clear =
            Block::default().style(Style::default().bg(app.config.style.background_color()));
        frame.render_widget(clear, popup_area);

        let popup = Paragraph::new(format!(" {prompt}"))
            .block(Block::bordered().style(Style::default().fg(app.config.style.text_color())))
            .style(Style::default().fg(app.config.style.text_color()));
        frame.render_widget(popup, popup_area);
    }
}

/// Hashes a string to produce a pleasing colour (uses shared implementation).
/// Uses gamma-corrected version to match egui's color rendering.
pub fn string_to_color(s: &str) -> Color {
    hsv_to_color(shared_style::string_to_hsv(s))
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

    let label = format!(" {position_str} / {duration_str} ");

    let ratio = if duration_secs > 0.0 {
        (position_secs / duration_secs).clamp(0.0, 1.0)
    } else {
        0.0
    };

    // Split area: scrub bar | volume slider.
    let sv = layout::split_scrub_volume(area);

    let gauge = Gauge::default()
        .gauge_style(
            Style::default()
                .fg(style.track_name_playing_color())
                .bg(style.background_color()),
        )
        .ratio(ratio as f64)
        .label(label);
    frame.render_widget(gauge, sv.scrub_bar);

    // Draw volume as a visual slider: "♪ ████░░░░ nn%"
    let vol_area = sv.volume;
    let bar_width = (vol_area.width as usize).saturating_sub(layout::VOLUME_BAR_PADDING as usize);
    let filled = ((volume * bar_width as f32).round() as usize).min(bar_width);
    let empty = bar_width.saturating_sub(filled);

    let vol_pct = format!("{:3.0}%", volume * 100.0);
    let vol_active_color = if app.volume_editing {
        style.track_name_playing_color()
    } else {
        style.track_duration_color()
    };

    let vol_line = Line::from(vec![
        Span::styled("\u{266A} ", Style::default().fg(vol_active_color)),
        Span::styled(
            "\u{2588}".repeat(filled),
            Style::default().fg(vol_active_color),
        ),
        Span::styled(
            "\u{2591}".repeat(empty),
            Style::default().fg(style.background_color()),
        ),
        Span::styled(format!(" {vol_pct}"), Style::default().fg(vol_active_color)),
    ]);
    frame.render_widget(Paragraph::new(vol_line), vol_area);
}

/// Handle click on scrub bar or volume slider area.
pub fn handle_scrub_volume_click(app: &mut App, scrub_area: Rect, x: u16) {
    // Recompute the scrub bar layout matching draw_scrub_bar.
    let sv = layout::split_scrub_volume(scrub_area);

    if x >= sv.volume.x && x < sv.volume.x + sv.volume.width {
        // Click on volume slider: "♪ ████░░░░ nnn%"
        // The slider bar starts at offset VOLUME_ICON_WIDTH ("♪ ") and ends VOLUME_LABEL_WIDTH before the end (" nnn%")
        let bar_start = sv.volume.x + layout::VOLUME_ICON_WIDTH;
        let bar_width = sv.volume.width.saturating_sub(layout::VOLUME_BAR_PADDING);
        if bar_width > 1 && x >= bar_start && x < bar_start + bar_width {
            let ratio = (x - bar_start) as f32 / (bar_width - 1) as f32;
            app.logic.set_volume(ratio.clamp(0.0, 1.0));
        }
    } else if x >= sv.scrub_bar.x && x < sv.scrub_bar.x + sv.scrub_bar.width {
        // Click on scrub bar → seek
        let ratio = (x - sv.scrub_bar.x) as f32 / sv.scrub_bar.width as f32;
        if let Some(details) = app.logic.get_track_display_details() {
            let seek_pos = Duration::from_secs_f32(details.track_duration.as_secs_f32() * ratio);
            app.logic.seek_current(seek_pos);
        }
    }
}

fn draw_help_bar(frame: &mut Frame, app: &App, area: Rect) {
    let style = &app.config.style;

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
                String::from(key),
                Style::default().fg(style.track_name_playing_color()),
            ));
            spans.push(Span::raw(format!(":{label} ")));
        }
    }

    let help_line = Line::from(spans);
    let help = Paragraph::new(help_line).style(Style::default().bg(style.background_color()));
    frame.render_widget(help, area);
}
