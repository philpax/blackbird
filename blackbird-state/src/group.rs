use smol_str::SmolStr;

use crate::{AlbumId, CoverArtId, TrackId};

/// An grouping of tracks.
#[derive(Debug, Clone)]
pub struct Group {
    /// The heading of the group.
    pub artist: SmolStr,
    /// The sort artist of the group.
    pub sort_artist: SmolStr,
    /// The subheading of the group.
    pub album: SmolStr,
    /// The year of the group.
    pub year: Option<i32>,
    /// The total duration of the group in seconds.
    pub duration: u32,
    /// The tracks in the group.
    pub tracks: Vec<TrackId>,
    /// The album cover art ID
    pub cover_art_id: Option<CoverArtId>,
    /// The associated album's ID
    pub album_id: AlbumId,
    /// Whether the group is starred.
    pub starred: bool,
}
