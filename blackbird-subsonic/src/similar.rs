use serde::Deserialize;

use crate::{Child, Client, ClientResult};

/// Similar-songs functionality.
impl Client {
    /// Get a list of songs similar to the given song.
    ///
    /// This is an OpenSubsonic extension endpoint backed by the `getSimilarSongs2`
    /// call. The server decides how similarity is computed (metadata-based, or
    /// AudioMuse-AI-style analysis when that plugin is installed).
    ///
    /// # Arguments
    ///
    /// * `id` - The song ID to find similar songs for
    /// * `count` - The maximum number of songs to return. `None` applies the
    ///   server default; `Some(0)` is sent verbatim and most servers interpret
    ///   0 as "no limit" (the settings UI clamps this to a minimum of 1).
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response is not valid.
    #[cfg(feature = "opensubsonic")]
    pub async fn get_similar_songs2(
        &self,
        id: impl Into<String>,
        count: Option<usize>,
    ) -> ClientResult<Vec<Child>> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct SimilarSongs2ApiResponse {
            similar_songs2: SimilarSongs2Response,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct SimilarSongs2Response {
            #[serde(default)]
            song: Vec<Child>,
        }

        Ok(self
            .request::<SimilarSongs2ApiResponse>(
                "getSimilarSongs2",
                &similar_songs_parameters(&id.into(), count),
            )
            .await?
            .similar_songs2
            .song)
    }

    /// Get a list of songs similar to the given song, computed by the server's
    /// sonic-similarity analysis.
    ///
    /// This is the OpenSubsonic `sonicSimilarity` extension endpoint
    /// (`getSonicSimilarTracks`) and should only be called when the server
    /// advertises that extension. Unlike [`Self::get_similar_songs2`], the
    /// similarity is derived from audio analysis rather than metadata.
    ///
    /// # Arguments
    ///
    /// * `id` - The song ID to find similar songs for
    /// * `count` - The maximum number of tracks to return. `None` applies the
    ///   server default.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response is not valid.
    #[cfg(feature = "opensubsonic")]
    pub async fn get_sonic_similar_tracks(
        &self,
        id: impl Into<String>,
        count: Option<usize>,
    ) -> ClientResult<Vec<Child>> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct SonicSimilarTracksApiResponse {
            sonic_similar_tracks: SonicSimilarTracksResponse,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct SonicSimilarTracksResponse {
            #[serde(default)]
            track: Vec<Child>,
        }

        Ok(self
            .request::<SonicSimilarTracksApiResponse>(
                "getSonicSimilarTracks",
                &similar_songs_parameters(&id.into(), count),
            )
            .await?
            .sonic_similar_tracks
            .track)
    }
}

/// Builds the shared `id`/`count` parameter list for the similar-songs endpoints.
fn similar_songs_parameters(id: &str, count: Option<usize>) -> Vec<(&str, String)> {
    let mut parameters = vec![("id", id.to_string())];
    if let Some(count) = count {
        parameters.push(("count", count.to_string()));
    }
    parameters
}
