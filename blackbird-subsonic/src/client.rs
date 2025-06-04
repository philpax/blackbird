#[derive(Debug)]
/// An error that can occur when interacting with the client.
pub enum ClientError {
    /// An error that occurred when making a request.
    ReqwestError(reqwest::Error),
    /// An error that occurred when deserializing a response.
    DeserializationError(serde_json::Error),
    /// The server returned an error.
    SubsonicError {
        /// The error code.
        code: i32,
        /// The error message.
        message: Option<String>,
    },
}
impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::ReqwestError(e) => write!(f, "Reqwest error: {e}"),
            ClientError::DeserializationError(e) => write!(f, "Deserialization error: {e}"),
            ClientError::SubsonicError { code, message } => {
                write!(f, "Subsonic error: {code}")?;
                if let Some(message) = message {
                    write!(f, ": {message}")?;
                }
                Ok(())
            }
        }
    }
}
impl std::error::Error for ClientError {}
impl From<reqwest::Error> for ClientError {
    fn from(e: reqwest::Error) -> Self {
        ClientError::ReqwestError(e)
    }
}
impl From<serde_json::Error> for ClientError {
    fn from(e: serde_json::Error) -> Self {
        ClientError::DeserializationError(e)
    }
}
/// A result type for the client.
pub type ClientResult<T> = Result<T, ClientError>;

/// A client for the Subsonic API.
pub struct Client {
    pub(crate) base_url: String,
    pub(crate) username: String,
    pub(crate) password: String,
    pub(crate) client_id: String,
    pub(crate) client: reqwest::Client,
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
}
