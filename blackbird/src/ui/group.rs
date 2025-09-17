use std::sync::{Arc, RwLock};

use blackbird_core::AppState;
use egui::{Align, Align2, Label, Layout, RichText, TextFormat, TextStyle, Ui, pos2, vec2};

use crate::{
    bc::{
        blackbird_state::{Group, TrackId},
        util,
    },
    ui::{style, track, util as ui_util},
};

pub struct GroupResponse<'a> {
    pub clicked_track: Option<&'a TrackId>,
}

#[allow(clippy::too_many_arguments)]
pub fn ui<'a>(
    group: &'a Group,
    ui: &mut Ui,
    style: &style::Style,
    state: Arc<RwLock<AppState>>,
    playing_track: Option<&TrackId>,
) -> GroupResponse<'a> {
    let mut clicked_track = None;

    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            // If the first row is visible, draw the artist.
            ui.add(
                Label::new(
                    RichText::new(&group.artist).color(style::string_to_colour(&group.artist)),
                )
                .selectable(false),
            );

            // If the second row is visible, draw the album title (including release year if available), as well as
            // the total duration.
            ui.horizontal(|ui| {
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    let mut layout_job = egui::text::LayoutJob::default();
                    layout_job.append(
                        group.album.as_str(),
                        0.0,
                        TextFormat {
                            color: style.album(),
                            ..Default::default()
                        },
                    );
                    if let Some(year) = group.year {
                        layout_job.append(
                            format!(" ({year})").as_str(),
                            0.0,
                            TextFormat {
                                color: style.album_year(),
                                ..Default::default()
                            },
                        );
                    }
                    ui.add(Label::new(layout_job).selectable(false));
                });

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add(
                        Label::new(
                            RichText::new(util::seconds_to_hms_string(group.duration, false))
                                .color(style.album_length()),
                        )
                        .selectable(false),
                    );
                });
            });
        });
    });

    ui.scope(|ui| {
        let tracks = &group.tracks;
        let track_row_height = ui.text_style_height(&TextStyle::Body);

        let track_map = &state.read().unwrap().track_map;

        // Do a pre-pass to calculate the maximum track length width for visible tracks
        let max_track_length_width = tracks
            .iter()
            .filter_map(|id| track_map.get(id))
            .map(|track| track::track_length_str_width(track, ui))
            .fold(0.0, f32::max);

        // Use shared spacing calculation
        let total_spacing = ui_util::track_spacing(ui);
        let spaced_row_height = track_row_height + total_spacing;

        // Set up the total height for all tracks in this range (with spacing)
        let total_height = tracks.len() as f32 * spaced_row_height;
        ui.allocate_space(vec2(ui.available_width(), total_height));

        let image_size = spaced_row_height * GROUP_ALBUM_ART_LINE_COUNT as f32;
        let image_top_margin = 4.0;
        let image_pos = pos2(ui.min_rect().left(), ui.min_rect().top() + image_top_margin);
        egui::Image::new(egui::include_image!(
            "../../assets/blackbird-female-bird.jpg"
        ))
        .paint_at(
            ui,
            egui::Rect {
                min: image_pos,
                max: image_pos + vec2(image_size, image_size),
            },
        );

        let track_x = image_pos.x + image_size + 16.0;

        ui.scope_builder(
            egui::UiBuilder::new().max_rect(egui::Rect {
                min: pos2(track_x, ui.min_rect().top()),
                max: pos2(ui.max_rect().right(), ui.max_rect().bottom()),
            }),
            |ui| {
                // Render only the visible tracks using direct positioning
                for (track_index, track_id) in tracks.iter().enumerate() {
                    let y_offset = track_index as f32 * spaced_row_height;
                    let track_y = ui.min_rect().top() + y_offset;

                    let Some(track) = track_map.get(track_id) else {
                        // Draw loading text directly with painter
                        ui.painter().text(
                            pos2(ui.min_rect().left(), track_y + total_spacing / 2.0),
                            Align2::LEFT_TOP,
                            "[loading...]",
                            TextStyle::Body.resolve(ui.style()),
                            ui.visuals().text_color(),
                        );
                        continue;
                    };

                    let r = track::ui(
                        track,
                        ui,
                        style,
                        &group.artist,
                        track::TrackParams {
                            max_track_length_width,
                            playing: playing_track == Some(&track.id),
                            track_y,
                            track_row_height,
                        },
                    );

                    if r.clicked {
                        clicked_track = Some(track_id);
                    }
                }
            },
        );
    });

    GroupResponse { clicked_track }
}

pub const GROUP_ARTIST_LINE_COUNT: usize = 1;
pub const GROUP_ALBUM_LINE_COUNT: usize = 1;
pub const GROUP_MARGIN_BOTTOM_ROW_COUNT: usize = 1;
pub const GROUP_ALBUM_ART_LINE_COUNT: usize = 5;

pub fn line_count(group: &Group) -> usize {
    let track_lines = group.tracks.len();

    GROUP_ARTIST_LINE_COUNT
        + GROUP_ALBUM_LINE_COUNT
        + track_lines.max(GROUP_ALBUM_ART_LINE_COUNT)
        + GROUP_MARGIN_BOTTOM_ROW_COUNT
}

pub fn target_scroll_height_for_track(
    state: &AppState,
    spaced_row_height: f32,
    track_id: &TrackId,
) -> Option<f32> {
    let track = state.track_map.get(track_id)?;
    let album_id = track.album_id.as_ref()?;

    let mut scroll_to_rows = 0;
    for group in &state.groups {
        if group.album_id == *album_id {
            scroll_to_rows += GROUP_ARTIST_LINE_COUNT;
            scroll_to_rows += GROUP_ALBUM_LINE_COUNT;
            scroll_to_rows += group.tracks.iter().take_while(|id| *id != track_id).count();
            break;
        }

        scroll_to_rows += line_count(group);
    }

    Some(scroll_to_rows as f32 * spaced_row_height)
}
