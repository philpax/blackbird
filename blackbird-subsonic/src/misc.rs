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
}
