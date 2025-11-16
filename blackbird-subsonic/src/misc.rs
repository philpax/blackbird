use crate::{Client, ClientResult};

/// Miscellaneous endpoints.
impl Client {
    /// Ping the server and verify the connection.
    pub async fn ping(&self) -> ClientResult<()> {
        self.request::<()>("ping", &[]).await?;
        Ok(())
    }

    /// Get cover art for a given ID.
    pub async fn get_cover_art(
        &self,
        id: impl Into<String>,
        size: Option<usize>,
    ) -> ClientResult<Vec<u8>> {
        let mut parameters = vec![("id", id.into())];
        if let Some(size) = size {
            parameters.push(("size", size.to_string()));
        }

        Self::check_for_subsonic_error_in_bytes(self.request_raw("getCoverArt", &parameters).await?)
    }

    /// Star items.
    pub async fn star(
        &self,
        track_ids: impl IntoIterator<Item = String>,
        album_ids: impl IntoIterator<Item = String>,
        artist_ids: impl IntoIterator<Item = String>,
    ) -> ClientResult<()> {
        let mut parameters = vec![];
        for track_id in track_ids.into_iter() {
            parameters.push(("id", track_id));
        }
        for album_id in album_ids.into_iter() {
            parameters.push(("albumId", album_id));
        }
        for artist_id in artist_ids.into_iter() {
            parameters.push(("artistId", artist_id));
        }

        self.request::<()>("star", &parameters).await
    }

    /// Unstar items.
    pub async fn unstar(
        &self,
        track_ids: impl IntoIterator<Item = String>,
        album_ids: impl IntoIterator<Item = String>,
        artist_ids: impl IntoIterator<Item = String>,
    ) -> ClientResult<()> {
        let mut parameters = vec![];
        for track_id in track_ids.into_iter() {
            parameters.push(("id", track_id));
        }
        for album_id in album_ids.into_iter() {
            parameters.push(("albumId", album_id));
        }
        for artist_id in artist_ids.into_iter() {
            parameters.push(("artistId", artist_id));
        }

        self.request::<()>("unstar", &parameters).await
    }

    /// Scrobble a track to register local playback.
    ///
    /// This endpoint:
    /// - Scrobbles to last.fm if user credentials are configured
    /// - Updates play count and last played timestamp
    /// - Updates the "Now playing" page
    ///
    /// # Arguments
    ///
    /// * `id` - The track ID to scrobble
    /// * `time` - Optional timestamp (in milliseconds since 1 Jan 1970) when the track was played
    /// * `submission` - If true (default), registers a scrobble. If false, only updates "now playing"
    pub async fn scrobble(
        &self,
        id: impl Into<String>,
        time: Option<u64>,
        submission: Option<bool>,
    ) -> ClientResult<()> {
        let mut parameters = vec![("id", id.into())];

        if let Some(time) = time {
            parameters.push(("time", time.to_string()));
        }

        if let Some(submission) = submission {
            parameters.push(("submission", submission.to_string()));
        }

        self.request::<()>("scrobble", &parameters).await
    }
}
