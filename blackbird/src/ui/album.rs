use std::ops::Range;

use crate::{
    album::Album,
    song::{SongId, SongMap},
    style,
    ui::song,
    util,
};

pub struct AlbumResponse<'a> {
    pub clicked_song: Option<&'a SongId>,
    pub hovered_song: Option<&'a SongId>,
}
#[allow(clippy::too_many_arguments)]
pub fn ui<'a>(
    album: &'a Album,
    ui: &mut egui::Ui,
    style: &style::Style,
    row_range: Range<usize>,
    album_art: Option<egui::ImageSource>,
    album_art_enabled: bool,
    song_map: &SongMap,
    playing_song: Option<&SongId>,
    hovered_song_last_frame: Option<&SongId>,
) -> AlbumResponse<'a> {
    ui.horizontal(|ui| {
        let artist_visible = row_range.contains(&0);
        let album_visible = row_range.contains(&1);

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
                        egui::RichText::new(&album.artist)
                            .color(style::string_to_colour(&album.artist)),
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
                            album.name.as_str(),
                            0.0,
                            egui::TextFormat {
                                color: style.album(),
                                ..Default::default()
                            },
                        );
                        if let Some(year) = album.year {
                            layout_job.append(
                                format!(" ({})", year).as_str(),
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
                                egui::RichText::new(util::seconds_to_hms_string(album.duration))
                                    .color(style.album_length()),
                            )
                            .selectable(false),
                        );
                    });
                });
            }
        });
    });

    // The first two rows are for headers, so adjust the song row indices by subtracting 2.
    let song_start = row_range.start.saturating_sub(2);
    let song_end = row_range.end.saturating_sub(2);
    if song_start >= song_end {
        return AlbumResponse {
            clicked_song: None,
            hovered_song: None,
        };
    }

    let mut clicked_song = None;
    let mut hovered_song = None;

    egui::Frame::NONE
        .inner_margin(egui::Margin {
            left: 10,
            ..egui::Margin::ZERO
        })
        .show(ui, |ui| {
            const LOADING_LABEL: &str = "[loading...]";
            let Some(songs) = &album.songs else {
                for _ in song_start..song_end {
                    ui.add(egui::Label::new(LOADING_LABEL).selectable(false));
                }
                return;
            };

            // Clamp the song slice to the actual number of songs.
            let end = song_end.min(songs.len());

            // Do a pre-pass to calculate the maximum track length width.
            let max_track_length_width = songs[song_start..end]
                .iter()
                .filter_map(|song_id| song_map.get(song_id))
                .map(|song| song::track_length_str_width(song, ui))
                .fold(0.0, f32::max);

            for song_id in &songs[song_start..end] {
                let Some(song) = song_map.get(song_id) else {
                    ui.add(egui::Label::new(LOADING_LABEL).selectable(false));
                    continue;
                };
                let r = song::ui(
                    song,
                    ui,
                    style,
                    &album.artist,
                    max_track_length_width,
                    playing_song == Some(&song.id),
                    hovered_song_last_frame == Some(&song.id),
                );

                if r.clicked {
                    clicked_song = Some(song_id);
                }
                if r.was_hovered {
                    hovered_song = Some(song_id);
                }
            }
        });

    AlbumResponse {
        clicked_song,
        hovered_song,
    }
}

pub fn line_count(album: &Album) -> usize {
    let artist_lines = 1;
    let album_lines = 1;
    let songs_lines = album
        .songs
        .as_ref()
        .map_or(album.song_count as usize, |songs| songs.len());

    artist_lines + album_lines + songs_lines
}
