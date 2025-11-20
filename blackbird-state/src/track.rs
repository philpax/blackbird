use serde::{Deserialize, Serialize};

use crate::{AlbumId, bs};

/// A track ID
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TrackId(pub String);
impl std::fmt::Display for TrackId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

/// A track, as `blackbird` cares about it
#[derive(Debug, Clone)]
pub struct Track {
    /// The track ID
    pub id: TrackId,
    /// The track title
    pub title: String,
    /// The track artist
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
    /// Whether the track is starred
    pub starred: bool,
    /// The number of times this track has been played
    pub play_count: Option<u64>,
}
impl From<bs::Child> for Track {
    fn from(child: bs::Child) -> Self {
        Track {
            id: TrackId(child.id),
            title: child.title,
            artist: child.artist.filter(|a| a != "[Unknown Artist]"),
            track: child.track,
            year: child.year,
            _genre: child.genre,
            duration: child.duration,
            disc_number: child.disc_number,
            album_id: child.album_id.map(AlbumId),
            starred: child.starred.is_some(),
            play_count: child.play_count,
        }
    }
}
impl PartialEq for Track {
    fn eq(&self, other: &Self) -> bool {
        (self.year, self.disc_number, self.track) == (other.year, other.disc_number, other.track)
    }
}
impl Eq for Track {}
impl PartialOrd for Track {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Track {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.year, self.disc_number, self.track).cmp(&(other.year, other.disc_number, other.track))
    }
}
