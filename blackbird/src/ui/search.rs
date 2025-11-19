use std::ops::Range;

use blackbird_core::{
    AppState, TrackDisplayDetails, blackbird_state::TrackId, util::seconds_to_hms_string,
};
use egui::{
    CentralPanel, Color32, Context, Key, Sense, TextEdit, TextFormat, TextStyle, Ui, Vec2, Vec2b,
    ViewportBuilder, ViewportId, vec2,
};

use crate::{bc, ui::style};

/// Main search window UI
pub fn ui(
    logic: &mut bc::Logic,
    ctx: &Context,
    style: &style::Style,
    search_open: &mut bool,
    search_query: &mut String,
) {
    if !*search_open {
        // Close the viewport if it exists
        ctx.send_viewport_cmd_to(search_viewport_id(), egui::ViewportCommand::Close);
        return;
    }

    let mut requested_track_id = None;
    let mut clear = false;

    ctx.show_viewport_immediate(
        search_viewport_id(),
        ViewportBuilder::default()
            .with_title("Blackbird - Search")
            .with_inner_size([800.0, 300.0])
            .with_active(true)
            .with_always_on_top(),
        |ctx, _class| {
            CentralPanel::default().show(ctx, |ui| {
                let response = ui.add_sized(
                    Vec2::new(ui.available_width(), ui.text_style_height(&TextStyle::Body)),
                    TextEdit::singleline(search_query).hint_text("Your search here..."),
                );
                response.request_focus();

                let mut play_first_track = false;
                if response.has_focus() {
                    if ui.input(|i| i.key_pressed(Key::Escape)) {
                        clear = true;
                    } else if ui.input(|i| i.key_pressed(Key::Enter)) {
                        play_first_track = true;
                    }
                }

                egui::Frame::dark_canvas(ui.style()).show(ui, |ui| {
                    ui.set_min_size(ui.available_size());

                    let length = search_query.len();
                    if length == 0 {
                        ui.label("Type something in to search...");
                        return;
                    } else if length < 3 {
                        ui.label("Query too short, please enter at least 3 characters...");
                        return;
                    }

                    let app_state = logic.get_state();
                    let mut app_state = app_state.write().unwrap();
                    let results = app_state.library.search(search_query);
                    if results.is_empty() {
                        ui.label("No results found...");
                        return;
                    }

                    // If Enter was pressed and we have results, select the first item
                    if play_first_track && !results.is_empty() {
                        requested_track_id = Some(results[0].clone());
                    }

                    let response = egui::ScrollArea::new(Vec2b::TRUE)
                        .auto_shrink(Vec2b::FALSE)
                        .show_rows(
                            ui,
                            ui.text_style_height(&TextStyle::Body),
                            results.len(),
                            |ui, row_indices| {
                                render_search_results(ui, row_indices, &results, &app_state, style)
                            },
                        );

                    if requested_track_id.is_none() {
                        requested_track_id = response.inner;
                    }
                });

                // Check if viewport was closed
                if ctx.input(|i| i.viewport().close_requested()) {
                    clear = true;
                }

                if let Some(track_id) = &requested_track_id {
                    logic.request_play_track(track_id);
                    clear = true;
                }

                if clear {
                    *search_open = false;
                    search_query.clear();
                }
            });
        },
    );
}

/// Renders search result rows and returns the clicked track ID if any
fn render_search_results(
    ui: &mut Ui,
    row_indices: Range<usize>,
    results: &[TrackId],
    app_state: &AppState,
    style: &style::Style,
) -> Option<TrackId> {
    let mut requested_track_id = None;
    for id in &results[row_indices] {
        let Some(details) = TrackDisplayDetails::from_track_id(id, app_state) else {
            continue;
        };

        let font_id = TextStyle::Body.resolve(ui.style());

        // Allocate space for this row and sense interaction
        let (rect, response) = ui.allocate_exact_size(
            vec2(ui.available_width(), ui.text_style_height(&TextStyle::Body)),
            Sense::click(),
        );

        let darken = |color: Color32| -> Color32 {
            const DARKEN_FACTOR: f32 = 0.75;
            let [r, g, b, a] = color.to_array();
            Color32::from_rgba_unmultiplied(
                (r as f32 * DARKEN_FACTOR) as u8,
                (g as f32 * DARKEN_FACTOR) as u8,
                (b as f32 * DARKEN_FACTOR) as u8,
                a,
            )
        };

        let is_hovered = response.hovered();
        let artist = details.artist();
        let [artist_color, track_color, length_color] = [
            style::string_to_colour(artist).into(),
            style.track_name(),
            style.track_length(),
        ]
        .map(|color| if is_hovered { color } else { darken(color) });
        let layout_job = {
            let mut layout_job = egui::text::LayoutJob::default();
            layout_job.append(
                artist,
                0.0,
                TextFormat {
                    color: artist_color,
                    font_id: font_id.clone(),
                    ..Default::default()
                },
            );
            layout_job.append(
                " - ",
                0.0,
                TextFormat {
                    font_id: font_id.clone(),
                    ..Default::default()
                },
            );
            layout_job.append(
                &details.track_title,
                0.0,
                TextFormat {
                    color: track_color,
                    font_id: font_id.clone(),
                    ..Default::default()
                },
            );
            layout_job.append(
                &format!(
                    " [{}]",
                    seconds_to_hms_string(details.track_duration.as_secs() as u32, false)
                ),
                0.0,
                TextFormat {
                    color: length_color,
                    font_id: font_id.clone(),
                    ..Default::default()
                },
            );
            layout_job.wrap.max_width = f32::INFINITY;
            layout_job
        };
        let galley = ui.fonts(|fonts| fonts.layout_job(layout_job));
        ui.painter()
            .galley(rect.left_top(), galley, Color32::PLACEHOLDER);

        if response.clicked() {
            requested_track_id = Some(id.clone());
        }
    }
    requested_track_id
}

/// Create search viewport ID dynamically
fn search_viewport_id() -> ViewportId {
    ViewportId::from_hash_of("search_window")
}
