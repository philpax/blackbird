use crate::{album::AlbumId, bs, style, util};

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SongId(pub String);
impl std::fmt::Display for SongId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug)]
/// A song, as `blackbird` cares about it
pub struct Song {
    /// The song ID
    pub _id: SongId,
    /// The song title
    pub title: String,
    /// The song artist
    pub artist: Option<String>,
    /// The track number
    pub track: Option<u32>,
    /// The release year
    pub year: Option<i32>,
    /// The genre
    pub _genre: Option<String>,
    /// The duration in seconds
    pub duration: Option<u32>,
    /// The disc number
    pub disc_number: Option<u32>,
    /// The album ID
    pub _album_id: Option<AlbumId>,
}
impl From<bs::Child> for Song {
    fn from(child: bs::Child) -> Self {
        Song {
            _id: SongId(child.id),
            title: child.title,
            artist: child.artist,
            track: child.track,
            year: child.year,
            _genre: child.genre,
            duration: child.duration,
            disc_number: child.disc_number,
            _album_id: child.album_id.map(AlbumId),
        }
    }
}
impl PartialEq for Song {
    fn eq(&self, other: &Self) -> bool {
        (self.year, self.disc_number, self.track) == (other.year, other.disc_number, other.track)
    }
}
impl Eq for Song {}
impl PartialOrd for Song {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Song {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.year, self.disc_number, self.track).cmp(&(other.year, other.disc_number, other.track))
    }
}
impl Song {
    pub fn ui(&self, ui: &mut egui::Ui, style: &style::Style, album_artist: &str) {
        let track = self.track.unwrap_or(0);
        let track_str = if let Some(disc_number) = self.disc_number {
            format!("{disc_number}.{track}")
        } else {
            track.to_string()
        };
        ui.horizontal(|ui| {
            // column 1 left aligned
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                let text_height = ui.text_style_height(&egui::TextStyle::Body);
                ui.add_sized(
                    egui::vec2(32.0, text_height),
                    util::RightAlignedWidget(egui::Label::new(
                        egui::RichText::new(track_str).color(style.track_number()),
                    )),
                );
                ui.add(egui::Label::new(&self.title).selectable(false));
            });

            // column 2 right-aligned
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(util::seconds_to_hms_string(
                            self.duration.unwrap_or(0),
                        ))
                        .color(style.track_length()),
                    )
                    .selectable(false),
                );
                if let Some(artist) = self
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
        });
    }
}
