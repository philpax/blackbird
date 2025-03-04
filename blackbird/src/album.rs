use std::ops::Range;

use crate::{
    bs,
    song::{SongId, SongMap},
    style, util,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AlbumId(pub String);
impl std::fmt::Display for AlbumId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// An album, as `blackbird` cares about it
#[derive(Debug, Clone)]
pub struct Album {
    /// The album ID
    pub id: AlbumId,
    /// The album name
    pub name: String,
    /// The album artist name
    pub artist: String,
    /// The album cover art ID
    pub cover_art_id: Option<String>,
    /// The number of songs in the album
    pub song_count: u32,
    /// The songs in the album
    pub songs: Option<Vec<SongId>>,
    /// The total duration of the album in seconds
    pub duration: u32,
    /// The release year of the album
    pub year: Option<i32>,
    /// The genre of the album
    pub _genre: Option<String>,
}
impl From<bs::AlbumID3> for Album {
    fn from(album: bs::AlbumID3) -> Self {
        Album {
            id: AlbumId(album.id),
            name: album.name,
            artist: album.artist.unwrap_or_else(|| "Unknown Artist".to_string()),
            cover_art_id: album.cover_art,
            song_count: album.song_count,
            songs: None,
            duration: album.duration,
            year: album.year,
            _genre: album.genre,
        }
    }
}
impl PartialEq for Album {
    fn eq(&self, other: &Self) -> bool {
        (self.artist.as_str(), self.year, &self.name)
            == (other.artist.as_str(), other.year, &other.name)
    }
}
impl Eq for Album {}
impl PartialOrd for Album {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Album {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (
            self.artist.to_lowercase(),
            self.year,
            self.name.to_lowercase(),
        )
            .cmp(&(
                other.artist.to_lowercase(),
                other.year,
                other.name.to_lowercase(),
            ))
    }
}
impl Album {
    /// Returns all albums; does not include songs.
    pub async fn fetch_all(client: &bs::Client) -> anyhow::Result<Vec<Album>> {
        let mut all_albums = vec![];
        let mut offset = 0;
        loop {
            let albums = client
                .get_album_list_2(
                    bs::AlbumListType::AlphabeticalByArtist,
                    Some(500),
                    Some(offset),
                )
                .await?;
            let album_count = albums.len();

            offset += album_count;
            all_albums.extend(albums.into_iter().map(|a| a.into()));
            if album_count < 500 {
                break;
            }
        }
        Ok(all_albums)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn ui(
        &self,
        ui: &mut egui::Ui,
        style: &style::Style,
        row_range: Range<usize>,
        album_art: Option<egui::ImageSource>,
        album_art_enabled: bool,
        song_map: &SongMap,
        playing_song: Option<&SongId>,
    ) -> Option<&SongId> {
        ui.horizontal(|ui| {
            let artist_visible = row_range.contains(&0);
            let album_visible = row_range.contains(&1);

            if album_art_enabled && (artist_visible || album_visible) {
                let album_art_size = ui.text_style_height(&egui::TextStyle::Body) * 2.0;
                ui.add_sized(
                    [album_art_size, album_art_size],
                    egui::Image::new(
                        album_art
                            .unwrap_or(egui::include_image!("../assets/blackbird-female-bird.jpg")),
                    ),
                );
            }

            ui.vertical(|ui| {
                // If the first row is visible, draw the artist.
                if artist_visible {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&self.artist)
                                .color(style::string_to_colour(&self.artist)),
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
                                self.name.as_str(),
                                0.0,
                                egui::TextFormat {
                                    color: style.album(),
                                    ..Default::default()
                                },
                            );
                            if let Some(year) = self.year {
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
                                    egui::RichText::new(util::seconds_to_hms_string(self.duration))
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
            return None;
        }

        let mut output = None;

        egui::Frame::NONE
            .inner_margin(egui::Margin {
                left: 10,
                ..egui::Margin::ZERO
            })
            .show(ui, |ui| {
                const LOADING_LABEL: &str = "[loading...]";
                let Some(songs) = &self.songs else {
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
                    .map(|song| song.track_length_str_width(ui))
                    .fold(0.0, f32::max);

                for song_id in &songs[song_start..end] {
                    let Some(song) = song_map.get(song_id) else {
                        ui.add(egui::Label::new(LOADING_LABEL).selectable(false));
                        continue;
                    };
                    if song.ui(
                        ui,
                        style,
                        &self.artist,
                        max_track_length_width,
                        playing_song == Some(&song.id),
                    ) {
                        output = Some(song_id);
                    }
                }
            });

        output
    }

    pub fn line_count(&self) -> usize {
        let artist = 1;
        let album = 1;
        let songs = self
            .songs
            .as_ref()
            .map_or(self.song_count as usize, |songs| songs.len());

        artist + album + songs
    }
}
