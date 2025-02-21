#![deny(missing_docs)]
//! A barebones client for the Subsonic API.

use rand::seq::IndexedRandom;

#[derive(Debug)]
/// An error that can occur when interacting with the client.
pub enum ClientError {
    /// An error that occurred when making a request.
    ReqwestError(reqwest::Error),
}
impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::ReqwestError(e) => write!(f, "Reqwest error: {e}"),
        }
    }
}
impl std::error::Error for ClientError {}
impl From<reqwest::Error> for ClientError {
    fn from(e: reqwest::Error) -> Self {
        ClientError::ReqwestError(e)
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
    pub async fn ping(&self) -> ClientResult<String> {
        self.request("ping", &[]).await
    }
}
impl Client {
    async fn request(&self, endpoint: &str, parameters: &[(&str, &str)]) -> ClientResult<String> {
        let (salt, token) = self.generate_salt_and_token();
        let request = self
            .client
            .get(format!("{}/rest/{endpoint}", self.base_url))
            .query(&[
                ("u", self.username.clone()),
                ("v", Self::API_VERSION.to_string()),
                ("c", self.client_id.clone()),
                ("t", token),
                ("s", salt),
            ])
            .query(parameters);

        Ok(request.send().await?.text().await?)
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
