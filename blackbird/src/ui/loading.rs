//! Animated blackbird-themed loading screen for the library content area.
//!
//! Renders a flock of small bird glyphs drifting in a wave pattern,
//! with the "blackbird" title and track-count status centered below.

use ratatui::{
    Frame,
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use super::StyleExt;

/// Bird glyph: modifier letter voiceless (U+02EC), a soft bird-like silhouette.
const BIRD: char = '\u{02EC}';

/// Base positions of each bird in the flock, as (x, y) offsets from center.
/// Spread across roughly 20 columns x 5 rows to form a loose V-shape.
const FLOCK: &[(i16, i16)] = &[
    (0, -2),  // lead bird
    (-3, -1), // left wing, row 1
    (3, -1),  // right wing, row 1
    (-6, 0),  // left wing, row 2
    (6, 0),   // right wing, row 2
    (-9, 1),  // left wing, row 3
    (9, 1),   // right wing, row 3
    (-5, 1),  // inner left
    (5, 1),   // inner right
    (-8, 2),  // trailing left
    (8, 2),   // trailing right
];

/// 8-entry sine lookup table for gentle wave motion.
/// Values represent pixel displacement: [0, 1, 1, 2, 2, 1, 1, 0].
const SINE_TABLE: &[i16] = &[0, 1, 1, 2, 2, 1, 1, 0];

/// Height of the flock area in terminal rows.
const FLOCK_HEIGHT: u16 = 5;

/// Total height of the loading display: flock + blank line + title + status.
const TOTAL_HEIGHT: u16 = FLOCK_HEIGHT + 1 + 1 + 1;

/// Draws the animated loading screen centered in `area`.
pub fn draw(
    frame: &mut Frame,
    tick_count: u64,
    style: &blackbird_client_shared::style::Style,
    track_count: usize,
    area: Rect,
) {
    if area.width < 4 || area.height < TOTAL_HEIGHT {
        // Area too small for the animation; fall back to simple text.
        draw_minimal(frame, style, track_count, tick_count, area);
        return;
    }

    let accent = style.track_name_playing_color();
    let dim = style.track_duration_color();

    // Vertical centering: place the block in the middle of the area.
    let top_y = area.y + (area.height.saturating_sub(TOTAL_HEIGHT)) / 2;
    let center_x = area.x + area.width / 2;

    // Draw the flock.
    let flock_area = Rect::new(area.x, top_y, area.width, FLOCK_HEIGHT);
    draw_flock(frame.buffer_mut(), tick_count, accent, center_x, flock_area);

    // "blackbird" title, centered below the flock.
    let title_y = top_y + FLOCK_HEIGHT + 1;
    if title_y < area.y + area.height {
        let title_area = Rect::new(area.x, title_y, area.width, 1);
        let title = Paragraph::new(Line::from(Span::styled(
            "blackbird",
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        )))
        .centered();
        frame.render_widget(title, title_area);
    }

    // Status line, centered below the title.
    let status_y = title_y + 1;
    if status_y < area.y + area.height {
        let status_area = Rect::new(area.x, status_y, area.width, 1);
        let status_text = loading_status_text(track_count, tick_count);
        let status = Paragraph::new(Line::from(Span::styled(
            status_text,
            Style::default().fg(dim),
        )))
        .centered();
        frame.render_widget(status, status_area);
    }
}

/// Renders each bird glyph into the buffer at its animated position.
fn draw_flock(
    buf: &mut Buffer,
    tick_count: u64,
    color: ratatui::style::Color,
    cx: u16,
    area: Rect,
) {
    let tick = tick_count as usize;

    for (i, &(base_x, base_y)) in FLOCK.iter().enumerate() {
        // Per-bird phase offset for varied motion.
        let phase = (tick + i * 3) % SINE_TABLE.len();
        let dx = SINE_TABLE[phase];
        // Vertical wave uses a different phase offset.
        let vy_phase = (tick + i * 5) % SINE_TABLE.len();
        let dy = SINE_TABLE[vy_phase] / 2;

        let x = cx as i16 + base_x + dx;
        let y = (area.y + area.height / 2) as i16 + base_y + dy;

        // Clamp to the flock area bounds.
        if x >= area.x as i16
            && x < (area.x + area.width) as i16
            && y >= area.y as i16
            && y < (area.y + area.height) as i16
        {
            let cell = &mut buf[(x as u16, y as u16)];
            cell.set_char(BIRD);
            cell.set_fg(color);
        }
    }
}

/// Generates the status text with animated dots or a track count.
/// The result is padded to a fixed width so centered text doesn't jitter
/// as the dot count cycles.
fn loading_status_text(track_count: usize, tick_count: u64) -> String {
    let dot_count = (tick_count / 5 % 4 + 1) as usize;
    let dots = ".".repeat(dot_count);
    let pad = " ".repeat(4 - dot_count);
    if track_count > 0 {
        format!("{track_count} tracks loaded, scanning{dots}{pad}")
    } else {
        format!("loading{dots}{pad}")
    }
}

/// Minimal fallback when the area is too small for the full animation.
fn draw_minimal(
    frame: &mut Frame,
    style: &blackbird_client_shared::style::Style,
    track_count: usize,
    tick_count: u64,
    area: Rect,
) {
    let dim = style.track_duration_color();
    let text = loading_status_text(track_count, tick_count);
    let paragraph = Paragraph::new(text).style(Style::default().fg(dim));
    frame.render_widget(paragraph, area);
}
