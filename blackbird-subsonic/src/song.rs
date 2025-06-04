use serde::{Deserialize, Serialize};

/// Represents a child item (file or directory) in the Subsonic API
#[derive(Debug, Clone, Serialize, Deserialize)]
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
