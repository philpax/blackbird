use serde::{Deserialize, Serialize};

use crate::{Client, ClientResult, song::Child};

/// Represents an album with ID3 metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumID3 {
    /// The album ID
    pub id: String,
    /// The album name
    pub name: String,
    /// The album artist name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    /// The album artist ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artist_id: Option<String>,
    /// The album cover art ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_art: Option<String>,
    /// The number of songs in the album
    pub song_count: u32,
    /// The total duration of the album in seconds
    pub duration: u32,
    /// The number of times the album has been played
    #[serde(skip_serializing_if = "Option::is_none")]
    pub play_count: Option<u64>,
    /// The creation date of the album
    pub created: String,
    /// The date the album was starred by the user
    #[serde(skip_serializing_if = "Option::is_none")]
    pub starred: Option<String>,
    /// The release year of the album
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<i32>,
    /// The genre of the album
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
}

/// Represents an album with ID3 metadata and songs
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumWithSongsID3 {
    /// The album metadata
    #[serde(flatten)]
    pub album: AlbumID3,
    /// The songs in the album
    pub song: Vec<Child>,
}

/// The type of album list to get.
#[derive(Debug, Clone, Copy)]
pub enum AlbumListType {
    /// A random list of albums.
    Random,
    /// The newest albums.
    Newest,
    /// The most frequently played albums.
    Frequent,
    /// The most recently played albums.
    Recent,
    /// The starred albums.
    Starred,
    /// The albums sorted alphabetically by name.
    AlphabeticalByName,
    /// The albums sorted alphabetically by artist.
    AlphabeticalByArtist,
}
impl AlbumListType {
    fn as_str(&self) -> &'static str {
        match self {
            AlbumListType::Random => "random",
            AlbumListType::Newest => "newest",
            AlbumListType::Frequent => "frequent",
            AlbumListType::Recent => "recent",
            AlbumListType::Starred => "starred",
            AlbumListType::AlphabeticalByName => "alphabeticalByName",
            AlbumListType::AlphabeticalByArtist => "alphabeticalByArtist",
        }
    }
}

/// Album-related endpoints.
impl Client {
    /// Get a list of albums, organised by ID3 tags.
    ///
    /// Size has a maximum of 500.
    pub async fn get_album_list_2(
        &self,
        ty: AlbumListType,
        size: Option<usize>,
        offset: Option<usize>,
    ) -> ClientResult<Vec<AlbumID3>> {
        let mut parameters = vec![("type", ty.as_str().to_string())];
        if let Some(size) = size {
            parameters.push(("size", size.to_string()));
        }
        if let Some(offset) = offset {
            parameters.push(("offset", offset.to_string()));
        }

        #[derive(Deserialize)]
        struct AlbumList2 {
            album: Vec<AlbumID3>,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct AlbumList2Response {
            album_list_2: AlbumList2,
        }

        Ok(self
            .request::<AlbumList2Response>("getAlbumList2", &parameters)
            .await?
            .album_list_2
            .album)
    }

    /// Get a specific album with its songs.
    pub async fn get_album_with_songs(
        &self,
        id: impl Into<String>,
    ) -> ClientResult<AlbumWithSongsID3> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct AlbumResponse {
            album: AlbumWithSongsID3,
        }

        Ok(self
            .request::<AlbumResponse>("getAlbum", &[("id", id.into())])
            .await?
            .album)
    }
}
