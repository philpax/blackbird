use crate::{
    bc::{state::Song, util},
    ui::style,
};

pub fn track_length_str_width(song: &Song, ui: &egui::Ui) -> f32 {
    egui::WidgetText::from(track_length_str(song))
        .into_galley(ui, None, f32::INFINITY, egui::TextStyle::Body)
        .size()
        .x
}

pub struct SongResponse {
    pub clicked: bool,
}

pub struct SongParams {
    pub max_track_length_width: f32,
    pub playing: bool,
    pub song_y: f32,
    pub song_row_height: f32,
}

pub fn ui(
    song: &Song,
    ui: &mut egui::Ui,
    style: &style::Style,
    album_artist: &str,
    params: SongParams,
) -> SongResponse {
    // Add proper spacing to match egui's default item spacing
    let spacing = ui.spacing().item_spacing.y;
    let actual_row_height = params.song_row_height + spacing;

    // Create a rect for this song with proper spacing
    let song_rect = egui::Rect::from_min_size(
        egui::pos2(ui.min_rect().left(), params.song_y),
        egui::vec2(ui.available_width(), actual_row_height),
    );

    // Check for interactions with this song area
    let song_response = ui.allocate_rect(song_rect, egui::Sense::click_and_drag());

    // Get track information
    let track = song.track.unwrap_or(0);
    let track_str = if let Some(disc_number) = song.disc_number {
        format!("{disc_number}.{track}")
    } else {
        track.to_string()
    };

    // Calculate text baseline position (add some padding from top)
    let text_y = params.song_y + (actual_row_height - params.song_row_height) / 2.0;

    // Draw track number (right-aligned in 32px column)
    let track_x = ui.min_rect().left() + 32.0;
    ui.painter().text(
        egui::pos2(track_x, text_y),
        egui::Align2::RIGHT_TOP,
        &track_str,
        egui::TextStyle::Body.resolve(ui.style()),
        style.track_number(),
    );

    // Draw song title
    let title_x = track_x + 8.0; // Small gap after track number
    let title_color = if song_response.hovered() {
        style.track_name_hovered()
    } else if params.playing {
        style.track_name_playing()
    } else {
        style.track_name()
    };

    ui.painter().text(
        egui::pos2(title_x, text_y),
        egui::Align2::LEFT_TOP,
        &song.title,
        egui::TextStyle::Body.resolve(ui.style()),
        title_color,
    );

    // Draw duration (right-aligned)
    let duration_str = track_length_str(song);
    ui.painter().text(
        egui::pos2(ui.max_rect().right(), text_y),
        egui::Align2::RIGHT_TOP,
        &duration_str,
        egui::TextStyle::Body.resolve(ui.style()),
        style.track_length(),
    );

    // Draw artist if different from album artist
    if let Some(artist) = song
        .artist
        .as_ref()
        .filter(|artist| *artist != album_artist)
    {
        let artist_x = ui.max_rect().right() - params.max_track_length_width - 40.0; // Leave space for duration
        ui.painter().text(
            egui::pos2(artist_x, text_y),
            egui::Align2::RIGHT_TOP,
            artist,
            egui::TextStyle::Body.resolve(ui.style()),
            style::string_to_colour(artist).into(),
        );
    }

    SongResponse {
        clicked: song_response.clicked(),
    }
}

fn track_length_str(song: &Song) -> String {
    util::seconds_to_hms_string(song.duration.unwrap_or(0), false)
}
