use egui::{
    Align, Align2, Color32, Context, Label, RichText, ScrollArea, Sense, Vec2, Vec2b, Window,
};

use blackbird_client_shared::next_playback_mode;

use crate::{
    bc,
    ui::{style, style::StyleExt},
};

/// Number of tracks to show before and after the current track in the queue window.
const QUEUE_RADIUS: usize = 50;

pub fn ui(logic: &mut bc::Logic, ctx: &Context, style: &style::Style, queue_open: &mut bool) {
    // Handle mode cycling while the queue window is open.
    ctx.input(|i| {
        for event in &i.events {
            if let egui::Event::Key {
                key: egui::Key::M,
                pressed: true,
                modifiers,
                ..
            } = event
                && !modifiers.command
                && !modifiers.alt
                && !modifiers.ctrl
            {
                let next = next_playback_mode(logic.get_playback_mode());
                logic.set_playback_mode(next);
            }
        }
    });

    // Gather queue data before rendering to avoid holding the state lock during UI rendering.
    let (before, current, after) = logic.get_queue_window(QUEUE_RADIUS);

    struct TrackInfo {
        track_id: bc::blackbird_state::TrackId,
        label: String,
        duration_str: String,
    }

    let all_track_ids: Vec<_> = before
        .iter()
        .chain(current.iter())
        .chain(after.iter())
        .cloned()
        .collect();

    let track_infos: Vec<TrackInfo> = {
        let state = logic.get_state();
        let st = state.read().unwrap();
        all_track_ids
            .iter()
            .map(|track_id| {
                let display = bc::TrackDisplayDetails::from_track_id(track_id, &st);
                TrackInfo {
                    track_id: track_id.clone(),
                    label: match &display {
                        Some(d) => format!("{} - {}", d.artist(), d.track_title),
                        None => track_id.0.clone(),
                    },
                    duration_str: display
                        .as_ref()
                        .map(|d| {
                            format!(
                                " [{}]",
                                bc::util::seconds_to_hms_string(
                                    d.track_duration.as_secs() as u32,
                                    false,
                                )
                            )
                        })
                        .unwrap_or_default(),
                }
            })
            .collect()
    };

    let current_list_index = before.len();
    let mut clicked_track = None;

    let mode = logic.get_playback_mode();
    Window::new(format!("Queue [{}]", mode))
        .open(queue_open)
        .default_pos(ctx.screen_rect().center())
        .default_size(ctx.screen_rect().size() * Vec2::new(0.4, 0.6))
        .pivot(Align2::CENTER_CENTER)
        .collapsible(false)
        .show(ctx, |ui| {
            if current.is_none() {
                ui.vertical_centered(|ui| {
                    ui.add_space(10.0);
                    ui.label("No tracks in the queue.");
                    ui.add_space(10.0);
                });
                return;
            }

            ScrollArea::vertical()
                .auto_shrink(Vec2b::FALSE)
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());

                    for (idx, info) in track_infos.iter().enumerate() {
                        let is_current = idx == current_list_index;
                        let is_past = idx < current_list_index;

                        let text_color = if is_current {
                            style.track_name_playing_color32()
                        } else if is_past {
                            let [r, g, b, a] = style.text_color32().to_array();
                            Color32::from_rgba_unmultiplied(
                                (r as f32 * 0.5) as u8,
                                (g as f32 * 0.5) as u8,
                                (b as f32 * 0.5) as u8,
                                a,
                            )
                        } else {
                            style.text_color32()
                        };

                        let row_text = format!(
                            "{}{}{}",
                            if is_current { "\u{25b6} " } else { "  " },
                            info.label,
                            info.duration_str,
                        );

                        let rich_text = RichText::new(&row_text).color(text_color);
                        let label_widget = if is_current {
                            Label::new(rich_text.strong())
                        } else {
                            Label::new(rich_text)
                        };

                        let response = ui.add(label_widget.selectable(false));

                        if is_current {
                            response.scroll_to_me(Some(Align::Center));
                        }

                        let row_interaction = ui.interact(
                            response.rect,
                            ui.id().with(("queue_track", idx)),
                            Sense::click(),
                        );

                        if row_interaction.clicked() {
                            clicked_track = Some(info.track_id.clone());
                        }

                        if row_interaction.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                    }
                });
        });

    if let Some(track_id) = clicked_track {
        logic.request_play_track(&track_id);
    }
}
