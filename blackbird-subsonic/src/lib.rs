#![deny(missing_docs)]
//! A barebones client for the Subsonic API.
use rand::seq::IndexedRandom;
use serde::{Deserialize, Serialize};

#[derive(Debug)]
/// An error that can occur when interacting with the client.
pub enum ClientError {
    /// An error that occurred when making a request.
    ReqwestError(reqwest::Error),
    /// An error that occurred when deserializing a response.
    DeserializationError(serde_json::Error),
    /// The server returned an error.
    SubsonicError {
        /// The error code.
        code: i32,
        /// The error message.
        message: Option<String>,
    },
}
impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::ReqwestError(e) => write!(f, "Reqwest error: {e}"),
            ClientError::DeserializationError(e) => write!(f, "Deserialization error: {e}"),
            ClientError::SubsonicError { code, message } => {
                write!(f, "Subsonic error: {code}")?;
                if let Some(message) = message {
                    write!(f, ": {message}")?;
                }
                Ok(())
            }
        }
    }
}
impl std::error::Error for ClientError {}
impl From<reqwest::Error> for ClientError {
    fn from(e: reqwest::Error) -> Self {
        ClientError::ReqwestError(e)
    }
}
impl From<serde_json::Error> for ClientError {
    fn from(e: serde_json::Error) -> Self {
        ClientError::DeserializationError(e)
    }
}
/// A result type for the client.
pub type ClientResult<T> = Result<T, ClientError>;

/// A client for the Subsonic API.
pub struct Client {
    base_url: String,
    username: String,
    password: String,
    client_id: String,
    client: reqwest::Client,
}
#[derive(Debug, Clone, Copy)]
/// The type of album list to get.
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
impl Client {
    /// The API version of the client.
    pub const API_VERSION: &str = "1.16.1";

    /// Create a new client.
    pub fn new(
        base_url: impl Into<String>,
        username: impl Into<String>,
        password: impl Into<String>,
        client_id: impl Into<String>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            username: username.into(),
            password: password.into(),
            client_id: client_id.into(),
            client: reqwest::Client::new(),
        }
    }

    /// Ping the server and verify the connection.
    pub async fn ping(&self) -> ClientResult<()> {
        self.request("ping", &[]).await?;
        Ok(())
    }

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
        Ok(self
            .request("getAlbumList2", &parameters)
            .await?
            .subsonic_response
            .album_list_2
            .unwrap()
            .album)
    }

    /// Get a specific album with its songs.
    pub async fn get_album_with_songs(
        &self,
        id: impl Into<String>,
    ) -> ClientResult<AlbumWithSongsID3> {
        Ok(self
            .request("getAlbum", &[("id", id.into())])
            .await?
            .subsonic_response
            .album
            .unwrap())
    }

    /// Get cover art for a given ID.
    pub async fn get_cover_art(&self, id: impl Into<String>) -> ClientResult<Vec<u8>> {
        let response = self
            .request_raw("getCoverArt", &[("id", id.into())])
            .await?;

        if let Err(err @ ClientError::SubsonicError { .. }) = Self::parse_response(&response) {
            return Err(err);
        }

        Ok(response)
    }
}
impl Client {
    async fn request(
        &self,
        endpoint: &str,
        parameters: &[(&str, String)],
    ) -> ClientResult<Response> {
        let bytes = self.request_raw(endpoint, parameters).await?;
        Self::parse_response(&bytes)
    }

    async fn request_raw(
        &self,
        endpoint: &str,
        parameters: &[(&str, String)],
    ) -> ClientResult<Vec<u8>> {
        let (salt, token) = self.generate_salt_and_token();
        let request = self
            .client
            .get(format!("{}/rest/{endpoint}", self.base_url))
            .query(&[
                ("u", self.username.clone()),
                ("v", Self::API_VERSION.to_string()),
                ("c", self.client_id.clone()),
                ("f", "json".to_string()),
                ("t", token),
                ("s", salt),
            ])
            .query(parameters);

        Ok(request.send().await?.bytes().await?.into())
    }

    fn parse_response(bytes: &[u8]) -> ClientResult<Response> {
        let response: Response = serde_json::from_slice(&bytes)?;

        if response.subsonic_response.status == ResponseStatus::Failed {
            let error = response.subsonic_response.error.unwrap();
            return Err(ClientError::SubsonicError {
                code: error.code,
                message: error.message,
            });
        }

        Ok(response)
    }

    fn generate_salt_and_token(&self) -> (String, String) {
        let mut rng = rand::rng();

        let password = &self.password;
        const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let salt = String::from_iter(CHARSET.choose_multiple(&mut rng, 16).map(|c| *c as char));

        let token = format!("{password}{salt}");
        let token = md5::compute(token).0;
        let token = data_encoding::HEXLOWER.encode(&token);

        (salt, token)
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct Response {
    subsonic_response: SubsonicResponse,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubsonicResponse {
    status: ResponseStatus,
    version: String,
    error: Option<ResponseError>,

    // Response body
    album_list_2: Option<AlbumList2>,
    album: Option<AlbumWithSongsID3>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
/// The status of a response.
enum ResponseStatus {
    /// The request was successful.
    Ok,
    /// The request failed.
    Failed,
}

#[derive(Debug, Serialize, Deserialize)]
/// An error that occurred when making a request.
struct ResponseError {
    /// The error code.
    code: i32,
    /// The error message.
    message: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AlbumList2 {
    album: Vec<AlbumID3>,
}

/// Represents an album with ID3 metadata
#[derive(Debug, Serialize, Deserialize)]
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
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumWithSongsID3 {
    /// The album metadata
    #[serde(flatten)]
    pub album: AlbumID3,
    /// The songs in the album
    pub song: Vec<Child>,
}

/// Represents a child item (file or directory) in the Subsonic API
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Child {
    /// The unique identifier
    pub id: String,
    /// The ID of the parent directory
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// Whether this is a directory
    pub is_dir: bool,
    /// The title of the item
    pub title: String,
    /// The album name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    /// The artist name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    /// The track number
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track: Option<u32>,
    /// The release year
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<i32>,
    /// The genre
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    /// The cover art ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_art: Option<String>,
    /// The file size in bytes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// The content type (MIME)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    /// The file suffix (extension)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suffix: Option<String>,
    /// The transcoded content type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcoded_content_type: Option<String>,
    /// The transcoded suffix
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcoded_suffix: Option<String>,
    /// The duration in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<u32>,
    /// The bitrate in kbps
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bit_rate: Option<u32>,
    /// The path of the file
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Whether this is a video
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_video: Option<bool>,
    /// The user's rating (1-5)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_rating: Option<u32>,
    /// The average rating (1-5)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub average_rating: Option<f32>,
    /// The number of times the item has been played
    #[serde(skip_serializing_if = "Option::is_none")]
    pub play_count: Option<u64>,
    /// The disc number
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disc_number: Option<u32>,
    /// The creation date
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    /// When the item was starred by the user
    #[serde(skip_serializing_if = "Option::is_none")]
    pub starred: Option<String>,
    /// The album ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album_id: Option<String>,
    /// The artist ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artist_id: Option<String>,
    /// The media type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
    /// The bookmark position in milliseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bookmark_position: Option<i64>,
    /// The original width of the media
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_width: Option<u32>,
    /// The original height of the media
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_height: Option<u32>,
}
