use std::ops::Range;

use crate::{
    bc::{
        state::{Group, SongId, SongMap},
        util,
    },
    ui::{song, style, util as ui_util},
};

pub struct GroupResponse<'a> {
    pub clicked_song: Option<&'a SongId>,
}

#[allow(clippy::too_many_arguments)]
pub fn ui<'a>(
    group: &'a Group,
    ui: &mut egui::Ui,
    style: &style::Style,
    row_range: Range<usize>,
    album_art: Option<egui::ImageSource>,
    album_art_enabled: bool,
    song_map: &SongMap,
    playing_song: Option<&SongId>,
) -> GroupResponse<'a> {
    let mut clicked_song = None;

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

    // Songs section with virtual rendering
    let song_start = row_range.start.saturating_sub(2);
    let song_end = row_range.end.saturating_sub(2);

    if song_start < song_end && !group.songs.is_empty() {
        egui::Frame::NONE
            .inner_margin(egui::Margin {
                left: 10,
                ..egui::Margin::ZERO
            })
            .show(ui, |ui| {
                let songs = &group.songs;
                let song_row_height = ui.text_style_height(&egui::TextStyle::Body);

                // Clamp the song slice to the actual number of songs
                let end = song_end.min(songs.len());
                let start = song_start.min(songs.len());

                if start < end {
                    // Do a pre-pass to calculate the maximum track length width for visible songs
                    let max_track_length_width = songs[start..end]
                        .iter()
                        .filter_map(|song_id| song_map.get(song_id))
                        .map(|song| song::track_length_str_width(song, ui))
                        .fold(0.0, f32::max);

                    // Use shared spacing calculation
                    let total_spacing = ui_util::track_spacing(ui);
                    let spaced_row_height = song_row_height + total_spacing;

                    // Set up the total height for all songs in this range (with spacing)
                    let total_height = (end - start) as f32 * spaced_row_height;
                    ui.allocate_space(egui::vec2(ui.available_width(), total_height));

                    // Render only the visible songs using direct positioning
                    for (song_index, song_id) in songs[start..end].iter().enumerate() {
                        let y_offset = song_index as f32 * spaced_row_height;
                        let song_y = ui.min_rect().top() + y_offset;

                        let Some(song) = song_map.get(song_id) else {
                            // Draw loading text directly with painter
                            ui.painter().text(
                                egui::pos2(ui.min_rect().left(), song_y + total_spacing / 2.0),
                                egui::Align2::LEFT_TOP,
                                "[loading...]",
                                egui::TextStyle::Body.resolve(ui.style()),
                                ui.visuals().text_color(),
                            );
                            continue;
                        };

                        // Use the ui function from song.rs
                        let r = song::ui(
                            song,
                            ui,
                            style,
                            &group.artist,
                            song::SongParams {
                                max_track_length_width,
                                playing: playing_song == Some(&song.id),
                                song_y,
                                song_row_height,
                            },
                        );

                        if r.clicked {
                            clicked_song = Some(song_id);
                        }
                    }
                }
            });
    }

    GroupResponse { clicked_song }
}

pub fn line_count(group: &Group) -> usize {
    let artist_lines = 1;
    let album_lines = 1;
    let songs_lines = group.songs.len();

    artist_lines + album_lines + songs_lines
}
