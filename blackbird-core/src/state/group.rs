use crate::state::SongId;

/// An grouping of tracks.
#[derive(Debug, Clone)]
pub struct Group {
    /// The heading of the group.
    pub artist: String,
    /// The subheading of the group.
    pub album: String,
    /// The year of the group.
    pub year: Option<i32>,
    /// The total duration of the group in seconds.
    pub duration: u32,
    /// The songs in the group.
    pub songs: Vec<SongId>,
    /// The album cover art ID
    pub cover_art_id: Option<String>,
}
