use rand::seq::IndexedRandom as _;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::{Client, ClientError, ClientResult};

/// Making requests to the Subsonic API.
impl Client {
    /// Make a request to the Subsonic API. `T` must contain a field corresponding to
    /// the actual value you want from the endpoint: that is, for `getAlbum`,
    /// `T` should be `{ album: AlbumWithSongsID3 }`.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response is not valid.
    pub async fn request<T: Serialize + DeserializeOwned>(
        &self,
        endpoint: &str,
        parameters: &[(&str, String)],
    ) -> ClientResult<T> {
        let bytes = self.request_raw(endpoint, parameters).await?;
        Self::parse_response::<T>(&bytes)
    }

    pub(crate) async fn request_raw(
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

    /// Check if the response contains a Subsonic error. Used for
    /// binary data endpoints.
    ///
    /// # Errors
    ///
    /// Returns an error if the response contains a Subsonic error.
    pub fn check_for_subsonic_error_in_bytes(bytes: Vec<u8>) -> Result<Vec<u8>, ClientError> {
        match Self::parse_response::<()>(&bytes) {
            Err(err @ ClientError::SubsonicError { .. }) => Err(err),
            _ => Ok(bytes),
        }
    }

    fn parse_response<T: Serialize + DeserializeOwned>(bytes: &[u8]) -> ClientResult<T> {
        let response: Response<T> = serde_json::from_slice(bytes)?;

        if response.subsonic_response.status == ResponseStatus::Failed {
            let error = response.subsonic_response.error.unwrap();
            return Err(ClientError::SubsonicError {
                code: error.code,
                message: error.message,
            });
        }

        Ok(response.subsonic_response.body)
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct Response<T> {
    pub subsonic_response: SubsonicResponse<T>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubsonicResponse<T> {
    status: ResponseStatus,
    version: String,
    error: Option<ResponseError>,

    // Response body
    #[serde(flatten)]
    body: T,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
/// The status of a response.
enum ResponseStatus {
    /// The request was successful.
    Ok,
    /// The request failed.
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// An error that occurred when making a request.
struct ResponseError {
    /// The error code.
    code: i32,
    /// The error message.
    message: Option<String>,
}
