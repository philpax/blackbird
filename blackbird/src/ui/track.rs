use egui::{Align2, Rect, Sense, TextStyle, Ui, WidgetText, pos2, vec2};

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
    pub track_y: f32,
    pub track_row_height: f32,
}

pub fn ui(
    track: &Track,
    ui: &mut Ui,
    style: &style::Style,
    album_artist: &str,
    params: TrackParams,
) -> TrackResponse {
    // Use shared spacing calculation
    let total_spacing = ui_util::track_spacing(ui);
    let actual_row_height = params.track_row_height + total_spacing;

    // Create a rect for this track with proper spacing
    let track_rect = Rect::from_min_size(
        pos2(ui.min_rect().left(), params.track_y),
        vec2(ui.available_width(), actual_row_height),
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

    // Calculate text baseline position (add some padding from top)
    let text_y = params.track_y + (actual_row_height - params.track_row_height) / 2.0;

    // Draw track number
    let track_x = ui.min_rect().left() + 16.0;
    ui.painter().text(
        pos2(track_x, text_y),
        Align2::RIGHT_TOP,
        &track_str,
        TextStyle::Body.resolve(ui.style()),
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
        TextStyle::Body.resolve(ui.style()),
        title_color,
    );

    // Draw duration (right-aligned)
    let duration_str = track_length_str(track);
    ui.painter().text(
        pos2(ui.max_rect().right(), text_y),
        Align2::RIGHT_TOP,
        &duration_str,
        TextStyle::Body.resolve(ui.style()),
        style.track_length(),
    );

    // Draw artist if different from album artist
    if let Some(artist) = track
        .artist
        .as_ref()
        .filter(|artist| *artist != album_artist)
    {
        // Leave space for duration
        let artist_x = ui.max_rect().right() - params.max_track_length_width - 6.0;
        ui.painter().text(
            pos2(artist_x, text_y),
            Align2::RIGHT_TOP,
            artist,
            TextStyle::Body.resolve(ui.style()),
            style::string_to_colour(artist).into(),
        );
    }

    TrackResponse {
        clicked: track_response.clicked(),
    }
}

fn track_length_str(track: &Track) -> String {
    util::seconds_to_hms_string(track.duration.unwrap_or(0), false)
}
