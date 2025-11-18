use blackbird_core::Logic;
use egui::{Align2, Rect, Sense, TextStyle, Ui, WidgetText, epaint::PathStroke, pos2, vec2};

use crate::{
    bc::{blackbird_state::Track, util},
    ui::{style, util as ui_util},
};

pub fn track_length_str_width(track: &Track, ui: &Ui) -> f32 {
    WidgetText::from(track_length_str(track))
        .into_galley(ui, None, f32::INFINITY, TextStyle::Body)
        .size()
        .x
}

pub struct TrackResponse {
    pub clicked: bool,
}

pub struct TrackParams {
    pub max_track_length_width: f32,
    pub playing: bool,
    pub incremental_search_target: bool,
    pub track_y: f32,
    pub track_row_height: f32,
}

pub fn ui(
    track: &Track,
    ui: &mut Ui,
    style: &style::Style,
    logic: &mut Logic,
    album_artist: &str,
    params: TrackParams,
) -> TrackResponse {
    // Use shared spacing calculation
    let total_spacing = ui_util::track_spacing(ui);
    let actual_row_height = params.track_row_height + total_spacing;
    let default_font = TextStyle::Body.resolve(ui.style());

    let mut right_x = ui.max_rect().right();
    // Calculate text baseline position (add some padding from top)
    let text_y = params.track_y + (actual_row_height - params.track_row_height) / 2.0;

    // Draw heart
    let (heart_response, heart_size) = ui_util::draw_heart(
        ui,
        default_font.clone(),
        ui_util::HeartPlacement::Position {
            pos: pos2(right_x, text_y),
            right_aligned: true,
        },
        track.starred,
        false,
    );
    right_x -= heart_size;
    if heart_response.clicked() {
        logic.set_track_starred(&track.id, !track.starred);
    }

    let row_width = ui.available_width();

    // Create a rect for this track with proper spacing
    let track_rect = Rect::from_min_size(
        pos2(ui.min_rect().left(), params.track_y),
        vec2(row_width - heart_size, actual_row_height),
    );

    // Check for interactions with this track area
    let track_response = ui.allocate_rect(track_rect, Sense::click());

    // Get track information
    let track_number = track.track.unwrap_or(0);
    let track_str = if let Some(disc_number) = track.disc_number {
        format!("{disc_number}.{track_number}")
    } else {
        track_number.to_string()
    };

    // Draw track number
    let track_x = ui.min_rect().left() + 16.0;
    ui.painter().text(
        pos2(track_x, text_y),
        Align2::RIGHT_TOP,
        &track_str,
        default_font.clone(),
        style.track_number(),
    );

    // Draw track title
    let title_x = track_x + 8.0; // Small gap after track number
    let title_color = if track_response.hovered() {
        style.track_name_hovered()
    } else if params.playing {
        style.track_name_playing()
    } else {
        style.track_name()
    };

    ui.painter().text(
        pos2(title_x, text_y),
        Align2::LEFT_TOP,
        &track.title,
        default_font.clone(),
        title_color,
    );

    // Draw duration (right-aligned)
    right_x -= 6.0;
    let duration_str = track_length_str(track);
    ui.painter().text(
        pos2(right_x, text_y),
        Align2::RIGHT_TOP,
        &duration_str,
        default_font.clone(),
        style.track_length(),
    );
    right_x -= params.max_track_length_width + 6.0;

    // Draw artist if different from album artist
    if let Some(artist) = track
        .artist
        .as_ref()
        .filter(|artist| *artist != album_artist)
    {
        // Leave space for duration
        ui.painter().text(
            pos2(right_x, text_y),
            Align2::RIGHT_TOP,
            artist,
            default_font,
            style::string_to_colour(artist).into(),
        );
    }

    // If the heart is hovered, draw a line underneath the track to make it
    // easier to line them up.
    if heart_response.hovered() {
        let line_start = track_rect.left_top() + vec2(0.0, track_rect.height());
        let line_end = line_start + vec2(row_width, 0.0);

        ui.painter().line(
            vec![line_start, line_end],
            PathStroke::new(1.0, style.track_name_hovered()),
        );
    }

    // If this is the incremental search target, draw a line underneath
    if params.incremental_search_target {
        let line_start = track_rect.left_top() + vec2(0.0, track_rect.height());
        let line_end = line_start + vec2(row_width, 0.0);

        ui.painter().line(
            vec![line_start, line_end],
            PathStroke::new(1.0, style.track_name_hovered()),
        );
    }

    TrackResponse {
        clicked: track_response.clicked(),
    }
}

fn track_length_str(track: &Track) -> String {
    util::seconds_to_hms_string(track.duration.unwrap_or(0), false)
}
