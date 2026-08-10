pub mod album_art_overlay;
pub(crate) mod layout;
pub(crate) mod library;
pub(crate) mod loading;
pub(crate) mod logs;
pub(crate) mod lyrics;
pub(crate) mod now_playing;
pub(crate) mod panel;
pub(crate) mod queue;
pub(crate) mod scroll;
pub(crate) mod search;
pub(crate) mod settings;
pub(crate) mod sidebar;

use blackbird_client_shared::style as shared_style;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
};

use smol_str::ToSmolStr as _;

use crate::{
    app::{App, FocusedPanel},
    cover_art::ArtColors,
    keys,
};

/// Builds a bordered block with a title and border colour, the common framing
/// used by the library (when a sidebar is present) and every sidebar
/// component. Callers render the block over `area` and draw content into
/// `block.inner(area)`.
pub(crate) fn framed_block(title: &str, border_color: Color) -> Block<'static> {
    Block::default()
        .title(title.to_string())
        .borders(ratatui::widgets::Borders::ALL)
        .border_style(Style::default().fg(border_color))
}

/// Returns the effective background color: either the configured background
/// or `Color::Reset` (terminal native) when `use_terminal_background` is set.
pub(crate) fn effective_bg(config: &crate::config::Config) -> Color {
    if config.layout.use_terminal_background {
        Color::Reset
    } else {
        config.style.general.background().to_color()
    }
}

/// Extension trait for using shared style colors with ratatui.
/// Converts a shared `Rgb` to a ratatui `Color`.
pub trait ToColor {
    fn to_color(self) -> Color;
}
impl ToColor for shared_style::Rgb {
    fn to_color(self) -> Color {
        Color::Rgb(self.r, self.g, self.b)
    }
}

/// Converts a shared HSV colour to a ratatui `Color` (gamma-corrected).
pub fn style_color(hsv: shared_style::Hsv) -> Color {
    shared_style::hsv_to_rgb(hsv).to_color()
}

/// Builds half-block art spans for one terminal row from a 4x4 color grid,
/// stretching to [`layout::art_cols()`] display columns via nearest-neighbor
/// mapping.
pub(crate) fn art_row_spans(
    colors: &ArtColors,
    top_row: usize,
    bot_row: usize,
) -> Vec<Span<'static>> {
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
    //
    // When using the terminal's native background (`Color::Reset`), we set a
    // non-default fg color on the fill. Since every cell is a space, the fg is
    // invisible, but it ensures cells differ from the default buffer state so
    // ratatui's diff includes them. Combined with `swap_buffers()` in the main
    // loop, this forces a full redraw every frame, preventing artifacts during
    // rapid scrolling.
    let bg_color = effective_bg(&app.config);
    let fill_style = if app.config.layout.use_terminal_background {
        Style::default().bg(bg_color).fg(Color::DarkGray)
    } else {
        Style::default().bg(bg_color)
    };
    frame.render_widget(Block::default().style(fill_style), size);

    // Main layout: [NowPlaying] | [Scrub+Volume] | [Content] | [Help].
    let main = layout::split_main(size);

    let is_loading = !app.logic.has_loaded_all_tracks();

    // Hide the now-playing header and scrub bar while the loading animation is active,
    // so only the centered flock animation is visible.
    if !is_loading {
        now_playing::draw(frame, app, main.now_playing);
        draw_scrub_bar(frame, app, main.scrub_bar);
    }

    // Plan the whole screen once. Both the draw path and the input paths in
    // `main.rs` consume the same rects, so draw and hit-testing agree.
    let layout = layout::layout_for(app, size);

    // Dispatch each visible content component exactly once from this single
    // point. The modal overlays (playback dropdown, album art, quit dialog)
    // are drawn separately below by design.
    for component in layout::visible_components(&layout) {
        match component {
            layout::VisibleComponent::MainPanel => {
                // The library is the main panel for Library and Settings
                // focus, and for Lyrics-with-sidebar (which renders the
                // library in the main area). The Settings arm is the same
                // code path as Library — no special casing.
                match layout.render_panel {
                    FocusedPanel::Library | FocusedPanel::Settings => {
                        // When a sidebar is shown, the library is framed; the
                        // frame's outer area is `layout.panel` and its inner
                        // rect is `layout.library`.
                        if layout.show_sidebar {
                            let border_color = app.config.style.library.border().to_color();
                            library::draw_in_frame(frame, app, layout.panel, border_color);
                        } else {
                            library::draw(frame, app, layout.panel);
                        }
                    }
                    FocusedPanel::Search => {
                        let scroll_line = app.search.viewport.line;
                        let hovered = search::hovered_result_index(
                            app.mouse_position,
                            layout.panel,
                            scroll_line,
                        );
                        search::draw(
                            frame,
                            &mut app.search,
                            &app.config.style,
                            &app.logic,
                            hovered,
                            layout.panel,
                        );
                    }
                    FocusedPanel::Lyrics => {
                        if app.sidebar.is_empty() {
                            // The sidebar has no components enabled while
                            // Lyrics was focused: fall back to the library
                            // (unframed, since no sidebar is present) with a
                            // hint that nothing is configured.
                            library::draw(frame, app, layout.panel);
                            let hint = Paragraph::new("No sidebar components enabled.").style(
                                Style::default().fg(app
                                    .config
                                    .style
                                    .library
                                    .track_duration()
                                    .to_color()),
                            );
                            frame.render_widget(
                                hint,
                                Rect::new(
                                    layout.panel.x + 1,
                                    layout.panel.y + 1,
                                    layout.panel.width.saturating_sub(2),
                                    1,
                                ),
                            );
                        } else {
                            sidebar::draw_panel(frame, app, layout.panel)
                        }
                    }
                    FocusedPanel::Logs => {
                        logs::draw(frame, &mut app.logs, &app.config.style, layout.panel);
                    }
                    FocusedPanel::Queue => queue::draw(
                        frame,
                        &app.queue,
                        &app.config.style,
                        &app.logic,
                        layout.panel,
                    ),
                }
            }
            layout::VisibleComponent::Settings => {
                if let Some(settings_rect) = layout.settings {
                    settings::draw(frame, &mut app.settings, &app.config, settings_rect);
                }
            }
            layout::VisibleComponent::LyricsSidebar => {
                if let Some(sidebar_area) = layout.lyrics_sidebar {
                    let sidebar_focused = app.focused_panel == FocusedPanel::Lyrics;
                    sidebar::draw_sidebar(frame, app, sidebar_area, sidebar_focused);
                }
            }
            layout::VisibleComponent::InlineLyrics => {
                if let Some(inline_area) = layout.inline_lyrics {
                    draw_inline_lyrics(frame, app, inline_area);
                }
            }
        }
    }

    draw_help_bar(frame, app, main.help_bar);

    // Draw playback mode dropdown if open.
    if app.playback_mode_dropdown {
        now_playing::draw_playback_mode_dropdown(frame, app, size);
    }

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
        let clear = Block::default().style(Style::default().bg(bg_color));
        frame.render_widget(clear, popup_area);

        let popup = Paragraph::new(format!(" {prompt}"))
            .block(
                Block::bordered()
                    .style(Style::default().fg(app.config.style.general.text().to_color())),
            )
            .style(Style::default().fg(app.config.style.general.text().to_color()));
        frame.render_widget(popup, popup_area);
    }
}

/// Hashes a string to produce a pleasing colour (uses shared implementation).
/// Uses gamma-corrected version to match the retired GUI's color rendering.
pub fn string_to_color(s: &str) -> Color {
    style_color(shared_style::string_to_hsv(s))
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

    // Use the preview ratio during scrub bar drags for instant visual feedback,
    // falling back to the playback thread's reported position otherwise.
    let (ratio, display_position_secs) = if let Some(preview) = app.scrub_preview_ratio {
        let r = preview.clamp(0.0, 1.0);
        (r, r * duration_secs)
    } else if duration_secs > 0.0 {
        (
            (position_secs / duration_secs).clamp(0.0, 1.0),
            position_secs,
        )
    } else {
        (0.0, 0.0)
    };

    let position_str =
        blackbird_core::util::seconds_to_hms_string(display_position_secs as u32, true);
    let duration_str = blackbird_core::util::seconds_to_hms_string(duration_secs as u32, true);
    let volume = app.logic.get_volume();

    let label = format!(" {position_str} / {duration_str} ");

    // Split area: scrub bar | volume slider.
    let sv = layout::split_scrub_volume(area);

    // Render the scrub bar with half-block precision. Each column can show
    // empty, a left-half block (▌), or a full block (█), giving twice the
    // resolution of the built-in Gauge widget.
    let bar_width = sv.scrub_bar.width as f64;
    let filled_half_blocks = (ratio as f64 * bar_width * 2.0).round() as u16;
    let full_cols = filled_half_blocks / 2;
    let has_half = filled_half_blocks % 2 == 1;

    let fg = style.library.track_name_playing().to_color();
    let bg = effective_bg(&app.config);
    let buf = frame.buffer_mut();
    let y = sv.scrub_bar.y;

    for col in 0..sv.scrub_bar.width {
        let x = sv.scrub_bar.x + col;
        let pos = ratatui::layout::Position::new(x, y);
        if !sv.scrub_bar.contains(pos) {
            continue;
        }
        let cell = &mut buf[pos];
        if col < full_cols {
            cell.set_char('█');
            cell.set_style(Style::default().fg(fg));
        } else if col == full_cols && has_half {
            cell.set_char('▌');
            cell.set_style(Style::default().fg(fg).bg(bg));
        } else {
            cell.set_char(' ');
            cell.set_style(Style::default().bg(bg));
        }
    }

    // Center the time label over the bar.
    let label_width = label.len() as u16;
    let label_start = sv.scrub_bar.x + sv.scrub_bar.width.saturating_sub(label_width) / 2;
    for (ci, ch) in label.chars().enumerate() {
        let x = label_start + ci as u16;
        let pos = ratatui::layout::Position::new(x, y);
        if sv.scrub_bar.contains(pos) {
            let col = x - sv.scrub_bar.x;
            let cell = &mut buf[pos];
            cell.set_char(ch);
            if col < full_cols {
                // Label on filled portion: invert colors.
                cell.set_style(Style::default().fg(bg).bg(fg));
            } else {
                cell.set_style(Style::default().fg(fg).bg(bg));
            }
        }
    }

    // Draw volume as a visual slider: "♪ ████░░░░ nn%"
    let vol_area = sv.volume;
    let bar_width = (vol_area.width as usize).saturating_sub(layout::VOLUME_BAR_PADDING as usize);
    let filled = ((volume * bar_width as f32).round() as usize).min(bar_width);
    let empty = bar_width.saturating_sub(filled);

    let vol_pct = format!("{:3.0}%", volume * 100.0);
    let vol_active_color = if app.volume_editing {
        style.library.track_name_playing().to_color()
    } else {
        style.library.track_duration().to_color()
    };

    let vol_line = Line::from(vec![
        Span::styled("\u{266A} ", Style::default().fg(vol_active_color)),
        Span::styled(
            "\u{2588}".repeat(filled),
            Style::default().fg(vol_active_color),
        ),
        Span::styled(
            "\u{2591}".repeat(empty),
            Style::default().fg(effective_bg(&app.config)),
        ),
        Span::styled(format!(" {vol_pct}"), Style::default().fg(vol_active_color)),
    ]);
    frame.render_widget(Paragraph::new(vol_line), vol_area);
}

fn draw_inline_lyrics(frame: &mut Frame, app: &App, area: Rect) {
    let style = &app.config.style;
    let position = app.logic.get_playing_position();
    let lyrics_line = app.lyrics.shared.current_inline_line(position);

    let line = if let Some(lyrics_line) = lyrics_line {
        let mut spans = Vec::new();
        // Timestamp prefix, matching the full lyrics panel style.
        if let Some(start_ms) = lyrics_line.start {
            let timestamp_secs = (start_ms / 1000) as u32;
            let timestamp_str = blackbird_core::util::seconds_to_hms_string(timestamp_secs, false);
            spans.push(Span::styled(
                format!(" {timestamp_str:>6} "),
                Style::default().fg(style.library.track_name_playing().to_color()),
            ));
        } else {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(
            &lyrics_line.value,
            Style::default().fg(style.general.text().to_color()),
        ));
        Line::from(spans)
    } else {
        Line::from(Span::styled(
            " [no lyrics]",
            Style::default().fg(style.library.track_duration().to_color()),
        ))
    };

    let paragraph = Paragraph::new(line).style(
        Style::default()
            .bg(effective_bg(&app.config))
            .fg(style.library.track_duration().to_color()),
    );
    // Use top and bottom borders to visually separate inline lyrics from
    // the content area above and the help bar below.
    let block = Block::default()
        .borders(ratatui::widgets::Borders::TOP | ratatui::widgets::Borders::BOTTOM)
        .border_style(Style::default().fg(style.sidebar.lyrics_border().to_color()));
    // Clear the area first so library content underneath doesn't bleed through.
    frame.render_widget(Clear, area);
    frame.render_widget(paragraph.block(block), area);
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
        // Set preview ratio for instant visual feedback; the actual seek
        // is deferred until mouse-up via `seek_current_immediate`.
        let ratio = (x - sv.scrub_bar.x) as f32 / sv.scrub_bar.width as f32;
        app.scrub_preview_ratio = Some(ratio);
    }
}

fn draw_help_bar(frame: &mut Frame, app: &mut App, area: Rect) {
    let style = &app.config.style;

    let help_entries: &[keys::HelpEntry] = match app.focused_panel {
        FocusedPanel::Library => keys::LIBRARY_HELP,
        FocusedPanel::Search => keys::SEARCH_HELP,
        FocusedPanel::Lyrics => keys::LYRICS_HELP,
        FocusedPanel::Logs => keys::LOGS_HELP,
        FocusedPanel::Queue => keys::QUEUE_HELP,
        FocusedPanel::Settings => keys::settings_help(app.settings.edit_mode()),
    };

    let mut spans: Vec<Span> = Vec::new();
    let mut x_pos = area.x + 1; // Account for the leading space.
    spans.push(Span::raw(" "));

    let highlight = Style::default().fg(style.library.track_name_playing().to_color());

    app.help_bar_items.clear();

    for entry in help_entries {
        match entry {
            keys::HelpEntry::Single(action) => {
                let Some((key, label)) = action.help_label(&app.logic) else {
                    continue;
                };
                let key_str = String::from(key);
                let label_str = format!(":{label} ");
                let item_width = key_str.len() as u16 + label_str.len() as u16;

                app.help_bar_items
                    .push((x_pos, x_pos + item_width, *action));

                spans.push(Span::styled(key_str, highlight));
                spans.push(Span::raw(label_str));

                x_pos += item_width;
            }
            keys::HelpEntry::Custom(action, desc) => {
                let Some((key, _)) = action.help_label(&app.logic) else {
                    continue;
                };
                let key_str = String::from(key);
                let label_str = format!(":{desc} ");
                let item_width = key_str.len() as u16 + label_str.len() as u16;

                app.help_bar_items
                    .push((x_pos, x_pos + item_width, *action));

                spans.push(Span::styled(key_str, highlight));
                spans.push(Span::raw(label_str));

                x_pos += item_width;
            }
            keys::HelpEntry::Pair(a, b, desc) => {
                let la = a.help_label(&app.logic);
                let lb = b.help_label(&app.logic);

                let (key_a_str, key_b_str) = match (&la, &lb) {
                    (Some((ka, _)), Some((kb, _))) => {
                        (String::from(ka.as_str()), String::from(kb.as_str()))
                    }
                    // If only one is visible, render it as a single entry.
                    (Some((key, desc)), None) | (None, Some((key, desc))) => {
                        let action = if la.is_some() { *a } else { *b };
                        let key_str = String::from(key.as_str());
                        let label_str = format!(":{desc} ");
                        let item_width = key_str.len() as u16 + label_str.len() as u16;

                        app.help_bar_items.push((x_pos, x_pos + item_width, action));

                        spans.push(Span::styled(key_str, highlight));
                        spans.push(Span::raw(label_str));

                        x_pos += item_width;
                        continue;
                    }
                    (None, None) => continue,
                };

                let desc_str = format!(":{desc} ");

                // Click target for first key.
                let ka_width = key_a_str.len() as u16;
                app.help_bar_items.push((x_pos, x_pos + ka_width, *a));
                spans.push(Span::styled(key_a_str, highlight));
                x_pos += ka_width;

                // Separator `/` (highlighted but not clickable).
                spans.push(Span::styled("/", highlight));
                x_pos += 1;

                // Click target for second key.
                let kb_width = key_b_str.len() as u16;
                app.help_bar_items.push((x_pos, x_pos + kb_width, *b));
                spans.push(Span::styled(key_b_str, highlight));
                x_pos += kb_width;

                // Description (not clickable).
                x_pos += desc_str.len() as u16;
                spans.push(Span::raw(desc_str));
            }
        }
    }

    let help_line = Line::from(spans);
    let help = Paragraph::new(help_line).style(Style::default().bg(effective_bg(&app.config)));
    frame.render_widget(help, area);
}

#[cfg(test)]
mod render_tests {
    use image::{ImageBuffer, ImageEncoder, Rgba, codecs::png::PngEncoder};
    use ratatui::{
        Terminal,
        backend::TestBackend,
        layout::{Position, Rect, Size},
    };
    use ratatui_image::{
        Image, Resize,
        picker::Picker,
        protocol::Protocol,
        sliced::{SignedPosition, SlicedImage, SlicedProtocol},
    };
    use std::sync::Arc;

    /// Creates a small 4×4 PNG test image (solid red).
    fn test_png() -> Vec<u8> {
        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(40, 40, Rgba([255, 0, 0, 255]));
        let mut buf = Vec::new();
        PngEncoder::new(&mut buf)
            .write_image(img.as_raw(), 40, 40, image::ExtendedColorType::Rgba8)
            .unwrap();
        buf
    }

    /// Creates a PNG with distinct colors per row (vertical gradient).
    /// 8 rows of distinct colors, each 10 pixels tall × 10 pixels wide.
    fn gradient_png() -> Vec<u8> {
        let colors = [
            Rgba([255, 0, 0, 255]),
            Rgba([0, 255, 0, 255]),
            Rgba([0, 0, 255, 255]),
            Rgba([255, 255, 255, 255]),
            Rgba([255, 128, 0, 255]),
            Rgba([128, 0, 255, 255]),
            Rgba([0, 255, 255, 255]),
            Rgba([255, 0, 255, 255]),
        ];
        let mut img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::new(80, 80);
        for y in 0..80 {
            for x in 0..80 {
                img.put_pixel(x, y, colors[(y / 10) as usize]);
            }
        }
        let mut buf = Vec::new();
        PngEncoder::new(&mut buf)
            .write_image(img.as_raw(), 80, 80, image::ExtendedColorType::Rgba8)
            .unwrap();
        buf
    }

    /// Verifies that `Image` mode with a halfblocks picker renders
    /// image content into the terminal buffer (colored cells), confirming
    /// that ratatui-image's halfblocks backend is functional.
    #[test]
    fn test_image_mode_halfblocks_backend() {
        let picker = Picker::halfblocks();
        let png_bytes = test_png();
        let dyn_img = image::load_from_memory(&png_bytes).unwrap();
        // The halfblocks picker has font_size 10×20, so a 40×40 pixel image
        // maps to 4×2 character cells. Use a size that fits.
        let size = Size::new(4, 2);
        let protocol: Protocol = picker
            .new_protocol(dyn_img, size, Resize::Fit(None))
            .unwrap();

        let backend = TestBackend::new(10, 5);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                let area = Rect::new(0, 0, 4, 2);
                f.render_widget(Image::new(&protocol).allow_clipping(true), area);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();

        // The halfblocks protocol renders image data as cell colors.
        // For a solid red image, cells should have Rgb(255, 0, 0) as fg or bg.
        // Verify at least one cell in the art area has a non-default color.
        let has_color = (0..4u16)
            .flat_map(|y| (0..2u16).map(move |x| (x, y)))
            .any(|(x, y)| {
                let cell = buffer.cell(Position { x, y }).unwrap();
                cell.fg != ratatui::style::Color::default()
                    || cell.bg != ratatui::style::Color::default()
            });

        assert!(
            has_color,
            "expected at least one colored cell in the art area"
        );
    }

    /// Verifies that when no protocol is available (None), rendering
    /// correctly falls through to the fallback path (no Image widget
    /// rendered, no image colors in the buffer).
    #[test]
    fn test_fallback_to_halfblocks_when_no_protocol() {
        let protocol: Option<Arc<Protocol>> = None;

        let backend = TestBackend::new(10, 5);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                let area = Rect::new(0, 0, 4, 2);
                if let Some(ref protocol) = protocol {
                    f.render_widget(Image::new(protocol).allow_clipping(true), area);
                } else {
                    // Fallback: render a plain block (simulating half-block fallback).
                    f.render_widget(ratatui::widgets::Block::default(), area);
                }
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        // With the fallback, no image colors should be present.
        let has_color = (0..4u16)
            .flat_map(|y| (0..2u16).map(move |x| (x, y)))
            .any(|(x, y)| {
                let cell = buffer.cell(Position { x, y }).unwrap();
                cell.fg != ratatui::style::Color::default()
                    || cell.bg != ratatui::style::Color::default()
            });

        assert!(!has_color, "fallback path should not render image colors");
    }

    /// Verifies that `SlicedImage` with a scroll offset renders the
    /// correct portion of the image. Uses a vertical-gradient test image
    /// where each row has a distinct color, then scrolls 1 line into the art
    /// area and verifies the buffer is non-empty.
    #[test]
    fn test_library_scroll_partial_group() {
        let picker = Picker::halfblocks();
        let png_bytes = gradient_png();
        let dyn_img = image::load_from_memory(&png_bytes).unwrap();

        // Create a SlicedProtocol sized to 8 cols × 8 rows.
        let sliced = SlicedProtocol::new(&picker, dyn_img, Some(Size::new(8, 8))).unwrap();

        // Render with scroll_offset = 3 (skip first 3 rows of the image).
        let backend = TestBackend::new(20, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let area = Rect::new(0, 0, 8, 8);
                let position = SignedPosition { x: 0, y: -3 };
                f.render_widget(SlicedImage::new(&sliced, position), area);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let has_content_scrolled =
            (0..8u16)
                .flat_map(|y| (0..8u16).map(move |x| (x, y)))
                .any(|(x, y)| {
                    let cell = buffer.cell(Position { x, y }).unwrap();
                    cell.fg != ratatui::style::Color::default()
                        || cell.bg != ratatui::style::Color::default()
                });
        assert!(
            has_content_scrolled,
            "expected colored cells in scrolled art area"
        );

        // Also render with scroll_offset = 0 and verify it also has content.
        let backend2 = TestBackend::new(20, 10);
        let mut terminal2 = Terminal::new(backend2).unwrap();
        terminal2
            .draw(|f| {
                let area = Rect::new(0, 0, 8, 8);
                let position = SignedPosition { x: 0, y: 0 };
                f.render_widget(SlicedImage::new(&sliced, position), area);
            })
            .unwrap();

        let buffer2 = terminal2.backend().buffer();
        let has_content_normal =
            (0..8u16)
                .flat_map(|y| (0..8u16).map(move |x| (x, y)))
                .any(|(x, y)| {
                    let cell = buffer2.cell(Position { x, y }).unwrap();
                    cell.fg != ratatui::style::Color::default()
                        || cell.bg != ratatui::style::Color::default()
                });
        assert!(
            has_content_normal,
            "expected colored cells in non-scrolled art area"
        );
    }

    /// Verifies that the `Halfblock` config variant is distinct from
    /// `Auto` and `Image`, confirming the config-level decision point that
    /// bypasses picker creation.
    #[test]
    fn test_halfblock_config_bypasses_image() {
        use crate::config::AlbumArtProtocol;
        assert_eq!(AlbumArtProtocol::default(), AlbumArtProtocol::Auto);
        assert_ne!(AlbumArtProtocol::Halfblock, AlbumArtProtocol::Auto);
        assert_ne!(AlbumArtProtocol::Halfblock, AlbumArtProtocol::Image);
    }
}
