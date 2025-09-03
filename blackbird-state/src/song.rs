use std::collections::HashMap;

use crate::{AlbumId, bs};

/// A map of song IDs to songs
pub type SongMap = HashMap<SongId, Song>;

/// A song ID
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SongId(pub String);
impl std::fmt::Display for SongId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A song, as `blackbird` cares about it
#[derive(Debug, Clone)]
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
