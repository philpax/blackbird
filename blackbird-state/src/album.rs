use serde::{Deserialize, Serialize};
use smol_str::SmolStr;

use crate::{ArtistId, CoverArtId, bs};

/// An album ID
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AlbumId(pub SmolStr);
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
    pub name: SmolStr,
    /// The album artist name
    pub artist: SmolStr,
    /// The artist ID
    pub artist_id: Option<ArtistId>,
    /// The album cover art ID
    pub cover_art_id: Option<CoverArtId>,
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
    /// The date the album was added to the library (ISO 8601 format).
    pub created: SmolStr,
}
impl From<bs::AlbumID3> for Album {
    fn from(album: bs::AlbumID3) -> Self {
        Album {
            id: AlbumId(album.id.into()),
            name: album.name.into(),
            artist: album
                .artist
                .unwrap_or_else(|| "Unknown Artist".to_string())
                .into(),
            artist_id: album.artist_id.map(|id| ArtistId(id.into())),
            cover_art_id: album.cover_art.map(|id| CoverArtId(id.into())),
            track_count: album.song_count,
            duration: album.duration,
            year: album.year,
            _genre: album.genre,
            starred: album.starred.is_some(),
            created: album.created.into(),
        }
    }
}
impl PartialEq for Album {
    fn eq(&self, other: &Self) -> bool {
        (self.artist.as_str(), self.year, self.name.as_str())
            == (other.artist.as_str(), other.year, other.name.as_str())
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
