use serde::{Deserialize, Serialize};

use crate::{ArtistId, bs};

/// An album ID
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
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
    /// The artist ID
    pub artist_id: Option<ArtistId>,
    /// The album cover art ID
    pub cover_art_id: Option<String>,
    /// The number of tracks in the album
    pub track_count: u32,
    /// The total duration of the album in seconds
    pub duration: u32,
    /// The release year of the album
    pub year: Option<i32>,
    /// The genre of the album
    pub _genre: Option<String>,
    /// Whether the album is starred.
    pub starred: bool,
}
impl From<bs::AlbumID3> for Album {
    fn from(album: bs::AlbumID3) -> Self {
        Album {
            id: AlbumId(album.id),
            name: album.name,
            artist: album.artist.unwrap_or_else(|| "Unknown Artist".to_string()),
            artist_id: album.artist_id.map(ArtistId),
            cover_art_id: album.cover_art,
            track_count: album.song_count,
            duration: album.duration,
            year: album.year,
            _genre: album.genre,
            starred: album.starred.is_some(),
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
    /// Returns all albums; does not include tracks.
    pub async fn fetch_all(client: &bs::Client) -> bs::ClientResult<Vec<Album>> {
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
}
