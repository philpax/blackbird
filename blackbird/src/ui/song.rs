use crate::{
    state::Song,
    ui::{style, util::RightAlignedWidget},
    util,
};

pub fn track_length_str_width(song: &Song, ui: &egui::Ui) -> f32 {
    egui::WidgetText::from(track_length_str(song))
        .into_galley(ui, None, f32::INFINITY, egui::TextStyle::Body)
        .size()
        .x
}

pub struct SongResponse {
    pub was_hovered: bool,
    pub clicked: bool,
}
pub fn ui(
    song: &Song,
    ui: &mut egui::Ui,
    style: &style::Style,
    album_artist: &str,
    max_track_length_width: f32,
    playing: bool,
    was_hovered_last_frame: bool,
) -> SongResponse {
    let r = ui
        .horizontal(|ui| {
            let track = song.track.unwrap_or(0);
            let track_str = if let Some(disc_number) = song.disc_number {
                format!("{disc_number}.{track}")
            } else {
                track.to_string()
            };
            let text_height = ui.text_style_height(&egui::TextStyle::Body);

            // column 1 left aligned
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add_sized(
                    egui::vec2(32.0, text_height),
                    RightAlignedWidget(
                        egui::Label::new(
                            egui::RichText::new(track_str).color(style.track_number()),
                        )
                        .selectable(false),
                    ),
                );
                ui.add(
                    egui::Label::new(egui::RichText::new(&song.title).color(
                        // This adds a one-frame delay to hovering, but I can't be bothered
                        // figuring out how to do this properly in egui.
                        //
                        // Interactive labels can have hover colours, but this requires giving
                        // the label a sense, which breaks propagation of sense upwards.
                        if was_hovered_last_frame {
                            style.track_name_hovered()
                        } else if playing {
                            style.track_name_playing()
                        } else {
                            style.track_name()
                        },
                    ))
                    .selectable(false),
                );
            });

            // column 2 right-aligned
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_sized(
                    egui::vec2(
                        // fudge number that includes margin.
                        // for some reason, the text width we get back is not enough to clear the label entirely
                        max_track_length_width + 32.0,
                        text_height,
                    ),
                    RightAlignedWidget(
                        egui::Label::new(
                            egui::RichText::new(track_length_str(song)).color(style.track_length()),
                        )
                        .truncate()
                        .selectable(false),
                    ),
                );
                if let Some(artist) = song
                    .artist
                    .as_ref()
                    .filter(|artist| *artist != album_artist)
                {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(artist).color(style::string_to_colour(artist)),
                        )
                        .selectable(false),
                    );
                }
            });
        })
        .response
        .interact(egui::Sense::click());

    SongResponse {
        was_hovered: r.hovered(),
        clicked: r.clicked(),
    }
}

fn track_length_str(song: &Song) -> String {
    util::seconds_to_hms_string(song.duration.unwrap_or(0))
}
