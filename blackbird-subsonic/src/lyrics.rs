use serde::{Deserialize, Serialize};

use crate::{Client, ClientResult};

/// A single line of lyrics with timing information.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricLine {
    /// The timestamp in milliseconds when this line should be displayed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<i64>,
    /// The lyric text for this line.
    pub value: String,
}

/// Structured lyrics with timing information.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuredLyrics {
    /// The display name for the lyrics source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_artist: Option<String>,
    /// The display title for the lyrics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_title: Option<String>,
    /// The language of the lyrics (ISO 639 code).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    /// The offset in milliseconds to apply to all timestamps.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<i64>,
    /// Whether the lyrics are synced (have timing information).
    pub synced: bool,
    /// The individual lyric lines.
    #[serde(default)]
    pub line: Vec<LyricLine>,
}

/// Response from the getLyricsBySongId endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsList {
    /// The list of structured lyrics for the song.
    #[serde(default)]
    pub structured_lyrics: Vec<StructuredLyrics>,
}

/// Lyrics-related functionality.
impl Client {
    /// Get lyrics for a song by ID.
    ///
    /// This is an OpenSubsonic extension endpoint that returns structured lyrics
    /// with timing information if available.
    ///
    /// # Arguments
    ///
    /// * `id` - The song ID to get lyrics for
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response is not valid.
    #[cfg(feature = "opensubsonic")]
    pub async fn get_lyrics_by_song_id(
        &self,
        id: impl Into<String>,
    ) -> ClientResult<LyricsList> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct LyricsApiResponse {
            lyrics_list: LyricsList,
        }

        Ok(self
            .request::<LyricsApiResponse>("getLyricsBySongId", &[("id", id.into())])
            .await?
            .lyrics_list)
    }
}
