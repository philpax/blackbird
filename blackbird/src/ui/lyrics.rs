use std::time::Duration;

use blackbird_core::util::seconds_to_hms_string;
use egui::{
    Align, Align2, Button, Color32, Context, Label, RichText, ScrollArea, Sense, Spinner, Vec2,
    Vec2b, Window,
};

use crate::{bc, ui::style};

const INFO_PADDING: f32 = 10.0;

pub fn ui(
    logic: &mut bc::Logic,
    ctx: &Context,
    style: &style::Style,
    lyrics_open: &mut bool,
    lyrics_data: &mut Option<bc::bs::StructuredLyrics>,
    lyrics_loading: &mut bool,
    lyrics_auto_scroll: &mut bool,
) {
    Window::new("Lyrics")
        .open(lyrics_open)
        .default_pos(ctx.screen_rect().center())
        .default_size(ctx.screen_rect().size() * Vec2::new(0.5, 0.6))
        .pivot(Align2::CENTER_CENTER)
        .collapsible(false)
        .show(ctx, |ui| {
            // Auto-scroll toggle button at the top
            let button_text = if *lyrics_auto_scroll {
                "Auto-scroll: on"
            } else {
                "Auto-scroll: off"
            };
            if ui
                .add_sized([ui.available_width(), 32.0], Button::new(button_text))
                .clicked()
            {
                *lyrics_auto_scroll = !*lyrics_auto_scroll;
            }
            ui.separator();

            if *lyrics_loading {
                ui.vertical_centered(|ui| {
                    ui.add_space(INFO_PADDING);
                    ui.add(Spinner::new());
                    ui.add_space(INFO_PADDING);
                    ui.label("Loading lyrics...");
                });
                return;
            }

            let Some(lyrics) = lyrics_data else {
                ui.vertical_centered(|ui| {
                    ui.add_space(INFO_PADDING);
                    ui.label("No lyrics available for this track.");
                    ui.add_space(INFO_PADDING);
                });
                return;
            };

            if lyrics.line.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(INFO_PADDING);
                    ui.label("No lyrics available for this track.");
                    ui.add_space(INFO_PADDING);
                });
                return;
            }

            // Get current playback position in milliseconds
            let current_position_ms = logic
                .get_playing_position()
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);

            // Apply offset if present
            let adjusted_position_ms = current_position_ms + lyrics.offset.unwrap_or(0);

            // Find the current line index based on playback position
            let current_line_idx = if lyrics.synced {
                lyrics
                    .line
                    .iter()
                    .enumerate()
                    .rev()
                    .find(|(_, line)| line.start.unwrap_or(0) <= adjusted_position_ms)
                    .map(|(idx, _)| idx)
                    .unwrap_or(0)
            } else {
                0 // For unsynced lyrics, don't highlight any line
            };

            ScrollArea::vertical()
                .auto_shrink(Vec2b::FALSE)
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());

                    for (idx, line) in lyrics.line.iter().enumerate() {
                        let is_current = lyrics.synced && idx == current_line_idx;
                        let is_past = lyrics.synced && idx < current_line_idx;

                        let text_color = if is_current {
                            style.text()
                        } else if is_past {
                            // Dim past lyrics
                            let [r, g, b, a] = style.text().to_array();
                            Color32::from_rgba_unmultiplied(
                                (r as f32 * 0.5) as u8,
                                (g as f32 * 0.5) as u8,
                                (b as f32 * 0.5) as u8,
                                a,
                            )
                        } else {
                            // Dim future lyrics
                            let [r, g, b, a] = style.text().to_array();
                            Color32::from_rgba_unmultiplied(
                                (r as f32 * 0.7) as u8,
                                (g as f32 * 0.7) as u8,
                                (b as f32 * 0.7) as u8,
                                a,
                            )
                        };

                        let row_response = ui.horizontal(|ui| {
                            // Show timestamp if available (for synced lyrics) and line is not empty
                            if let Some(start_ms) = line.start
                                && !line.value.trim().is_empty()
                            {
                                let timestamp_secs = (start_ms / 1000) as u32;
                                let timestamp_str = seconds_to_hms_string(timestamp_secs, false);

                                let timestamp_color = if is_current {
                                    style.text()
                                } else {
                                    // Dim timestamps for non-current lines
                                    let [r, g, b, a] = style.text().to_array();
                                    Color32::from_rgba_unmultiplied(
                                        (r as f32 * 0.4) as u8,
                                        (g as f32 * 0.4) as u8,
                                        (b as f32 * 0.4) as u8,
                                        a,
                                    )
                                };

                                ui.add(Label::new(
                                    RichText::new(&timestamp_str)
                                        .color(timestamp_color)
                                        .monospace(),
                                ));

                                ui.add_space(4.0);
                            }

                            let rich_text = RichText::new(&line.value).color(text_color);

                            let label_response = if is_current {
                                ui.label(rich_text.strong())
                            } else {
                                ui.label(rich_text)
                            };

                            // Scroll to keep the current line visible (only if auto-scroll is enabled)
                            if is_current && *lyrics_auto_scroll {
                                label_response.scroll_to_me(Some(Align::Center));
                            }

                            line.start
                        });

                        // Make the entire row clickable if it has a timestamp
                        if let Some(start_ms) = row_response.inner {
                            let row_rect = row_response.response.rect;
                            let row_interaction =
                                ui.interact(row_rect, ui.id().with(idx), Sense::click());

                            if row_interaction.clicked() {
                                logic.seek_current(Duration::from_millis(start_ms as u64));
                            }

                            if row_interaction.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                        }
                    }
                });
        });
}
