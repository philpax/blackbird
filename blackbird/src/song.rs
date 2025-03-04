use std::{
    collections::HashMap,
    sync::{atomic::AtomicBool, Arc},
};

use crate::{album::AlbumId, bs, style, util};

pub type SongMap = HashMap<SongId, Song>;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SongId(pub String);
impl std::fmt::Display for SongId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone)]
/// A song, as `blackbird` cares about it
pub struct Song {
    /// The song ID
    pub id: SongId,
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
    pub album_id: Option<AlbumId>,

    was_hovered: Arc<AtomicBool>,
}
impl From<bs::Child> for Song {
    fn from(child: bs::Child) -> Self {
        Song {
            id: SongId(child.id),
            title: child.title,
            artist: child.artist.filter(|a| a != "[Unknown Artist]"),
            track: child.track,
            year: child.year,
            _genre: child.genre,
            duration: child.duration,
            disc_number: child.disc_number,
            album_id: child.album_id.map(AlbumId),
            was_hovered: Arc::new(AtomicBool::new(false)),
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
    pub fn track_length_str_width(&self, ui: &egui::Ui) -> f32 {
        egui::WidgetText::from(self.track_length_str())
            .into_galley(ui, None, f32::INFINITY, egui::TextStyle::Body)
            .size()
            .x
    }

    pub fn ui(
        &self,
        ui: &mut egui::Ui,
        style: &style::Style,
        album_artist: &str,
        max_track_length_width: f32,
        playing: bool,
    ) -> bool {
        let r = ui
            .horizontal(|ui| {
                let track = self.track.unwrap_or(0);
                let track_str = if let Some(disc_number) = self.disc_number {
                    format!("{disc_number}.{track}")
                } else {
                    track.to_string()
                };
                let text_height = ui.text_style_height(&egui::TextStyle::Body);

                // column 1 left aligned
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add_sized(
                        egui::vec2(32.0, text_height),
                        util::RightAlignedWidget(
                            egui::Label::new(
                                egui::RichText::new(track_str).color(style.track_number()),
                            )
                            .selectable(false),
                        ),
                    );
                    ui.add(
                        egui::Label::new(egui::RichText::new(&self.title).color(
                            // This adds a one-frame delay to hovering, but I can't be bothered
                            // figuring out how to do this properly in egui.
                            //
                            // Interactive labels can have hover colours, but this requires giving
                            // the label a sense, which breaks propagation of sense upwards.
                            if self.was_hovered.load(std::sync::atomic::Ordering::Relaxed) {
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
                        util::RightAlignedWidget(
                            egui::Label::new(
                                egui::RichText::new(self.track_length_str())
                                    .color(style.track_length()),
                            )
                            .truncate()
                            .selectable(false),
                        ),
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
            })
            .response
            .interact(egui::Sense::click());

        self.was_hovered
            .store(r.hovered(), std::sync::atomic::Ordering::Relaxed);

        r.double_clicked()
    }
}
impl Song {
    fn track_length_str(&self) -> String {
        util::seconds_to_hms_string(self.duration.unwrap_or(0))
    }
}
