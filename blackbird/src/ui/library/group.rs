use blackbird_client_shared::config::AlbumArtStyle;
use blackbird_core::{AppState, Logic};
use egui::{Align, Align2, Label, Layout, RichText, TextFormat, TextStyle, Ui, pos2, vec2};

use crate::{
    bc::{
        blackbird_state::{Group, TrackId},
        util,
    },
    cover_art_cache::{CachePriority, CoverArtCache},
    ui::{style, style::StyleExt, util as ui_util},
};

use super::track;

pub const GROUP_ARTIST_LINE_COUNT: usize = 1;
pub const GROUP_ALBUM_LINE_COUNT: usize = 1;
pub const GROUP_MARGIN_BOTTOM_ROW_COUNT: usize = 1;

pub const GROUP_ALBUM_ART_SIZE: f32 = 128.0;
// Should be roughly synchronised to GROUP_ALBUM_ART_SIZE
pub const GROUP_ALBUM_ART_LINE_COUNT: usize = 8;

pub struct GroupResponse<'a> {
    pub clicked_track: Option<&'a TrackId>,
    pub clicked_heart: bool,
    /// When set, the user is hovering over album art. Contains the cover art ID
    /// and the screen-space rect of the thumbnail.
    pub hovered_art: Option<(blackbird_core::blackbird_state::CoverArtId, egui::Rect)>,
}

#[allow(clippy::too_many_arguments)]
pub fn ui<'a>(
    group: &'a Group,
    ui: &mut Ui,
    style: &style::Style,
    logic: &mut Logic,
    playing_track: Option<&TrackId>,
    incremental_search_target: Option<&TrackId>,
    cover_art_cache: &mut CoverArtCache,
    album_art_style: AlbumArtStyle,
) -> GroupResponse<'a> {
    let mut clicked_track = None;
    let mut clicked_heart = false;
    let mut hovered_art: Option<(blackbird_core::blackbird_state::CoverArtId, egui::Rect)> = None;

    // Compute the header art size for LeftOfAlbum so it can be reused for
    // track alignment below.
    let left_of_album_art_size = if album_art_style == AlbumArtStyle::LeftOfAlbum {
        let text_height = ui.text_style_height(&TextStyle::Body);
        let item_spacing_y = ui.spacing().item_spacing.y;
        // Match the height of the two text lines (artist + album) including
        // the vertical spacing between them.
        Some(text_height * 2.0 + item_spacing_y)
    } else {
        None
    };

    const LEFT_OF_ALBUM_ART_LEFT_MARGIN: f32 = 4.0;
    const LEFT_OF_ALBUM_ART_RIGHT_MARGIN: f32 = 8.0;

    ui.horizontal(|ui| {
        // In LeftOfAlbum mode, show a small thumbnail beside the header.
        if let Some(art_size) = left_of_album_art_size {
            // Disable horizontal item spacing so only our explicit margins
            // control the gaps — this keeps track titles aligned with the
            // album name, which uses the same margin constants.
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.add_space(LEFT_OF_ALBUM_ART_LEFT_MARGIN);
            let art_rect =
                egui::Rect::from_min_size(ui.cursor().left_top(), vec2(art_size, art_size));
            egui::Image::new(cover_art_cache.get(
                logic,
                group.cover_art_id.as_ref(),
                CachePriority::Visible,
            ))
            .show_loading_spinner(false)
            .paint_at(ui, art_rect);
            // Sense hover on the art area.
            let art_response = ui.allocate_rect(art_rect, egui::Sense::hover());
            if art_response.hovered()
                && let Some(id) = &group.cover_art_id
            {
                hovered_art = Some((id.clone(), art_response.rect));
            }
            ui.add_space(LEFT_OF_ALBUM_ART_RIGHT_MARGIN);
        }

        ui.vertical(|ui| {
            // Artist
            ui.add(
                Label::new(
                    RichText::new(group.artist.as_str())
                        .color(style::string_to_colour(&group.artist)),
                )
                .selectable(false),
            );

            // Album + Year + Added + Duration
            ui.horizontal(|ui| {
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    let mut layout_job = egui::text::LayoutJob::default();
                    layout_job.append(
                        group.album.as_str(),
                        0.0,
                        TextFormat {
                            color: style.album_color32(),
                            ..Default::default()
                        },
                    );
                    if let Some(year) = group.year {
                        layout_job.append(
                            format!(" ({year})").as_str(),
                            0.0,
                            TextFormat {
                                color: style.album_year_color32(),
                                ..Default::default()
                            },
                        );
                    }
                    // Show the date the album was added to the library.
                    let state = logic.get_state();
                    let state = state.read().unwrap();
                    if let Some(album) = state.library.albums.get(&group.album_id) {
                        // Extract "YYYY-MM-DD" from ISO 8601 timestamp.
                        if let Some(date) = album.created.get(..10) {
                            layout_job.append(
                                format!(" +{date}").as_str(),
                                0.0,
                                TextFormat {
                                    color: style.album_year_color32(),
                                    ..Default::default()
                                },
                            );
                        }
                    }
                    ui.add(Label::new(layout_job).selectable(false));
                });

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let (heart_response, _) = ui_util::draw_heart(
                        ui,
                        TextStyle::Body.resolve(ui.style()),
                        ui_util::HeartPlacement::Space,
                        group.starred,
                        false,
                    );

                    if heart_response.clicked() {
                        clicked_heart = true;
                    }

                    ui.add(
                        Label::new(
                            RichText::new(util::seconds_to_hms_string(group.duration, false))
                                .color(style.album_length_color32()),
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

        let state = logic.get_state();
        let track_map = &state.read().unwrap().library.track_map;

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

        match album_art_style {
            AlbumArtStyle::BelowAlbum => {
                let image_size = GROUP_ALBUM_ART_SIZE;
                let image_top_margin = 4.0;
                let image_left_margin = 4.0;
                let image_right_margin = 12.0;
                let image_pos = pos2(
                    ui.min_rect().left() + image_left_margin,
                    ui.min_rect().top() + image_top_margin,
                );
                let art_rect = egui::Rect {
                    min: image_pos,
                    max: image_pos + vec2(image_size, image_size),
                };

                egui::Image::new(cover_art_cache.get(
                    logic,
                    group.cover_art_id.as_ref(),
                    CachePriority::Visible,
                ))
                .show_loading_spinner(false)
                .paint_at(ui, art_rect);
                ui.allocate_rect(art_rect, egui::Sense::hover());

                let track_x = image_pos.x + image_size + image_right_margin;
                ui.scope_builder(
                    egui::UiBuilder::new().max_rect(egui::Rect {
                        min: pos2(track_x, ui.min_rect().top()),
                        max: pos2(ui.max_rect().right(), ui.max_rect().bottom()),
                    }),
                    |ui| {
                        render_tracks(
                            ui,
                            tracks,
                            track_map,
                            style,
                            logic,
                            &group.artist,
                            playing_track,
                            incremental_search_target,
                            max_track_length_width,
                            spaced_row_height,
                            total_spacing,
                            &mut clicked_track,
                        );
                    },
                );
            }
            AlbumArtStyle::LeftOfAlbum => {
                // Align track titles with the album name in the header.
                // track::ui draws the title at `scope_left + 24.0`
                // (16.0 for the track number right-edge + 8.0 gap).
                // We want `scope_left + 24.0 = header_text_x`.
                let art_size = left_of_album_art_size.unwrap_or(0.0);
                let header_text_x = ui.min_rect().left()
                    + LEFT_OF_ALBUM_ART_LEFT_MARGIN
                    + art_size
                    + LEFT_OF_ALBUM_ART_RIGHT_MARGIN;

                let track_title_internal_offset = 24.0;
                let track_x = header_text_x - track_title_internal_offset;

                ui.scope_builder(
                    egui::UiBuilder::new().max_rect(egui::Rect {
                        min: pos2(track_x, ui.min_rect().top()),
                        max: pos2(ui.max_rect().right(), ui.max_rect().bottom()),
                    }),
                    |ui| {
                        render_tracks(
                            ui,
                            tracks,
                            track_map,
                            style,
                            logic,
                            &group.artist,
                            playing_track,
                            incremental_search_target,
                            max_track_length_width,
                            spaced_row_height,
                            total_spacing,
                            &mut clicked_track,
                        );
                    },
                );
            }
        }
    });

    GroupResponse {
        clicked_track,
        clicked_heart,
        hovered_art,
    }
}

#[allow(clippy::too_many_arguments)]
fn render_tracks<'a>(
    ui: &mut Ui,
    tracks: &'a [TrackId],
    track_map: &std::collections::HashMap<TrackId, blackbird_core::blackbird_state::Track>,
    style: &style::Style,
    logic: &mut Logic,
    artist: &str,
    playing_track: Option<&TrackId>,
    incremental_search_target: Option<&TrackId>,
    max_track_length_width: f32,
    spaced_row_height: f32,
    total_spacing: f32,
    clicked_track: &mut Option<&'a TrackId>,
) {
    for (track_index, track_id) in tracks.iter().enumerate() {
        let y_offset = track_index as f32 * spaced_row_height;
        let track_y = ui.min_rect().top() + y_offset;

        let Some(track) = track_map.get(track_id) else {
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
            logic,
            artist,
            track::TrackParams {
                max_track_length_width,
                playing: playing_track == Some(&track.id),
                incremental_search_target: incremental_search_target == Some(&track.id),
                track_y,
                track_row_height: spaced_row_height - total_spacing,
            },
        );

        if r.clicked {
            *clicked_track = Some(track_id);
        }
    }
}

pub fn line_count(group: &Group, album_art_style: AlbumArtStyle) -> usize {
    let track_lines = group.tracks.len();

    let min_track_lines = match album_art_style {
        AlbumArtStyle::LeftOfAlbum => track_lines,
        AlbumArtStyle::BelowAlbum => track_lines.max(GROUP_ALBUM_ART_LINE_COUNT),
    };

    GROUP_ARTIST_LINE_COUNT
        + GROUP_ALBUM_LINE_COUNT
        + min_track_lines
        + GROUP_MARGIN_BOTTOM_ROW_COUNT
}

pub fn line_count_for_group_and_track(group: &Group, track_id: &TrackId) -> usize {
    GROUP_ARTIST_LINE_COUNT
        + GROUP_ALBUM_LINE_COUNT
        + group.tracks.iter().take_while(|id| *id != track_id).count()
}

pub fn target_scroll_height_for_track(
    state: &AppState,
    spaced_row_height: f32,
    track_id: &TrackId,
    album_art_style: AlbumArtStyle,
) -> Option<f32> {
    let track = state.library.track_map.get(track_id)?;
    let album_id = track.album_id.as_ref()?;

    let mut scroll_to_rows = 0;
    for group in &state.library.groups {
        if group.album_id == *album_id {
            scroll_to_rows += line_count_for_group_and_track(group, track_id);
            break;
        }

        scroll_to_rows += line_count(group, album_art_style);
    }

    Some(scroll_to_rows as f32 * spaced_row_height)
}
