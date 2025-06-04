use crate::{Client, ClientResult};

/// Miscellaneous endpoints.
impl Client {
    /// Ping the server and verify the connection.
    pub async fn ping(&self) -> ClientResult<()> {
        self.request::<()>("ping", &[]).await?;
        Ok(())
    }

    /// Get cover art for a given ID.
    pub async fn get_cover_art(&self, id: impl Into<String>) -> ClientResult<Vec<u8>> {
        Self::check_for_subsonic_error_in_bytes(
            self.request_raw("getCoverArt", &[("id", id.into())])
                .await?,
        )
    }

    /// Download a file from the server.
    pub async fn download(&self, id: impl Into<String>) -> ClientResult<Vec<u8>> {
        Self::check_for_subsonic_error_in_bytes(
            self.request_raw("download", &[("id", id.into())]).await?,
        )
    }
}
