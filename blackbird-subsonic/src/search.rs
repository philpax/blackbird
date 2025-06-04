use serde::{Deserialize, Serialize};

use crate::{AlbumID3, ArtistID3, Child, Client, ClientResult};

/// A request to the `search3` endpoint.
#[derive(Debug, Clone, Default)]
pub struct Search3Request {
    /// The search query.
    pub query: String,
    /// The maximum number of artists to return.
    pub artist_count: Option<u32>,
    /// The search result offset for artists. Used for paging.
    pub artist_offset: Option<u32>,
    /// The maximum number of albums to return.
    pub album_count: Option<u32>,
    /// The search result offset for albums. Used for paging.
    pub album_offset: Option<u32>,
    /// The maximum number of songs to return.
    pub song_count: Option<u32>,
    /// The search result offset for songs. Used for paging.
    pub song_offset: Option<u32>,
    /// The ID of the music folder to return results from.
    pub music_folder_id: Option<u32>,
}

/// A response from the `search3` endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Search3Response {
    /// The artists found.
    #[serde(default)]
    pub artist: Vec<ArtistID3>,
    /// The albums found.
    #[serde(default)]
    pub album: Vec<AlbumID3>,
    /// The songs found.
    #[serde(default)]
    pub song: Vec<Child>,
}

/// Search-related functionality.
impl Client {
    /// Search for an album.
    pub async fn search3(&self, request: &Search3Request) -> ClientResult<Search3Response> {
        let mut parameters = vec![("query", request.query.clone())];
        if let Some(artist_count) = request.artist_count {
            parameters.push(("artistCount", artist_count.to_string()));
        }
        if let Some(artist_offset) = request.artist_offset {
            parameters.push(("artistOffset", artist_offset.to_string()));
        }
        if let Some(album_count) = request.album_count {
            parameters.push(("albumCount", album_count.to_string()));
        }
        if let Some(album_offset) = request.album_offset {
            parameters.push(("albumOffset", album_offset.to_string()));
        }
        if let Some(song_count) = request.song_count {
            parameters.push(("songCount", song_count.to_string()));
        }
        if let Some(song_offset) = request.song_offset {
            parameters.push(("songOffset", song_offset.to_string()));
        }
        if let Some(music_folder_id) = request.music_folder_id {
            parameters.push(("musicFolderId", music_folder_id.to_string()));
        }

        #[derive(Debug, Clone, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Search3ApiResponse {
            search_result_3: Search3Response,
        }

        Ok(self
            .request::<Search3ApiResponse>("search3", &parameters)
            .await?
            .search_result_3)
    }
}
