use std::{
    ops::Range,
    sync::{Arc, RwLock},
};

use blackbird_core::AppState;

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
    ui: &mut egui::Ui,
    style: &style::Style,
    row_range: Range<usize>,
    album_art: Option<egui::ImageSource>,
    album_art_enabled: bool,
    state: Arc<RwLock<AppState>>,
    playing_track: Option<&TrackId>,
) -> GroupResponse<'a> {
    let mut clicked_track = None;

    // Header section (artist and album info)
    let artist_visible = row_range.contains(&0);
    let album_visible = row_range.contains(&1);

    if artist_visible || album_visible {
        ui.horizontal(|ui| {
            if album_art_enabled && (artist_visible || album_visible) {
                let album_art_size = ui.text_style_height(&egui::TextStyle::Body) * 2.0;
                ui.add_sized(
                    [album_art_size, album_art_size],
                    egui::Image::new(album_art.unwrap_or(egui::include_image!(
                        "../../assets/blackbird-female-bird.jpg"
                    ))),
                );
            }

            ui.vertical(|ui| {
                // If the first row is visible, draw the artist.
                if artist_visible {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&group.artist)
                                .color(style::string_to_colour(&group.artist)),
                        )
                        .selectable(false),
                    );
                }
                // If the second row is visible, draw the album title (including release year if available), as well as
                // the total duration.
                if album_visible {
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            let mut layout_job = egui::text::LayoutJob::default();
                            layout_job.append(
                                group.album.as_str(),
                                0.0,
                                egui::TextFormat {
                                    color: style.album(),
                                    ..Default::default()
                                },
                            );
                            if let Some(year) = group.year {
                                layout_job.append(
                                    format!(" ({year})").as_str(),
                                    0.0,
                                    egui::TextFormat {
                                        color: style.album_year(),
                                        ..Default::default()
                                    },
                                );
                            }
                            ui.add(egui::Label::new(layout_job).selectable(false));
                        });

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(util::seconds_to_hms_string(
                                        group.duration,
                                        false,
                                    ))
                                    .color(style.album_length()),
                                )
                                .selectable(false),
                            );
                        });
                    });
                }
            });
        });
    }

    // Tracks section with virtual rendering
    let track_start = row_range.start.saturating_sub(2);
    let track_end = row_range.end.saturating_sub(2);

    if track_start < track_end && !group.tracks.is_empty() {
        egui::Frame::NONE
            .inner_margin(egui::Margin {
                left: 10,
                ..egui::Margin::ZERO
            })
            .show(ui, |ui| {
                let tracks = &group.tracks;
                let track_row_height = ui.text_style_height(&egui::TextStyle::Body);

                // Clamp the track slice to the actual number of tracks
                let end = track_end.min(tracks.len());
                let start = track_start.min(tracks.len());

                if start >= end {
                    return;
                }

                let track_map = &state.read().unwrap().track_map;

                // Do a pre-pass to calculate the maximum track length width for visible tracks
                let max_track_length_width = tracks[start..end]
                    .iter()
                    .filter_map(|id| track_map.get(id))
                    .map(|track| track::track_length_str_width(track, ui))
                    .fold(0.0, f32::max);

                // Use shared spacing calculation
                let total_spacing = ui_util::track_spacing(ui);
                let spaced_row_height = track_row_height + total_spacing;

                // Set up the total height for all tracks in this range (with spacing)
                let total_height = (end - start) as f32 * spaced_row_height;
                ui.allocate_space(egui::vec2(ui.available_width(), total_height));

                // Render only the visible tracks using direct positioning
                for (track_index, track_id) in tracks[start..end].iter().enumerate() {
                    let y_offset = track_index as f32 * spaced_row_height;
                    let track_y = ui.min_rect().top() + y_offset;

                    let Some(track) = track_map.get(track_id) else {
                        // Draw loading text directly with painter
                        ui.painter().text(
                            egui::pos2(ui.min_rect().left(), track_y + total_spacing / 2.0),
                            egui::Align2::LEFT_TOP,
                            "[loading...]",
                            egui::TextStyle::Body.resolve(ui.style()),
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
            });
    }

    GroupResponse { clicked_track }
}

pub const GROUP_ARTIST_LINE_COUNT: usize = 1;
pub const GROUP_ALBUM_LINE_COUNT: usize = 1;
pub const GROUP_MARGIN_BOTTOM_ROW_COUNT: usize = 1;

pub fn line_count(group: &Group) -> usize {
    let track_lines = group.tracks.len();

    GROUP_ARTIST_LINE_COUNT + GROUP_ALBUM_LINE_COUNT + track_lines + GROUP_MARGIN_BOTTOM_ROW_COUNT
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
