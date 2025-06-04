use serde::{Deserialize, Serialize};

/// An artist with ID3 metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistID3 {
    /// The id of the artist.
    pub id: String,
    /// The name of the artist.
    pub name: String,
    /// The cover art of the artist.
    pub cover_art: Option<String>,
    /// The artist image url.
    pub artist_image_url: Option<String>,
    /// The album count of the artist.
    pub album_count: u32,
    /// The date the artist was starred. [ISO 8601]
    pub starred: Option<String>,
    /// The artist MusicBrainzID.
    #[serde(default)]
    pub music_brainz_id: Option<String>,
    /// The artist sort name.
    #[serde(default)]
    pub sort_name: Option<String>,
    /// The list of all roles this artist has in the library.
    #[serde(default)]
    pub roles: Vec<String>,
}
