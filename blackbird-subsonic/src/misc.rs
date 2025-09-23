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
}
