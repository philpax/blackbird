use serde::{Deserialize, Serialize};

use crate::{Client, ClientResult};

/// A server-advertised OpenSubsonic extension, as returned by the
/// `getOpenSubsonicExtensions` endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OpenSubsonicExtension {
    /// The extension name, e.g. `sonicSimilarity`.
    pub name: String,
    /// The extension version, e.g. `1.0.0`.
    pub version: String,
}

/// OpenSubsonic extension discovery.
impl Client {
    /// Query the server for the OpenSubsonic extensions it supports.
    ///
    /// This is an OpenSubsonic extension endpoint. Servers that do not support
    /// it either return a successful response with an empty extension list or
    /// an error; callers should treat both as "no extensions".
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response is not valid.
    #[cfg(feature = "opensubsonic")]
    pub async fn get_open_subsonic_extensions(&self) -> ClientResult<Vec<OpenSubsonicExtension>> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct OpenSubsonicExtensionsApiResponse {
            #[serde(rename = "openSubsonicExtensions", default)]
            open_subsonic_extensions: ExtensionsList,
        }

        #[derive(Deserialize, Default)]
        #[serde(rename_all = "camelCase")]
        struct ExtensionsList {
            #[serde(default)]
            extension: Vec<OpenSubsonicExtension>,
        }

        Ok(self
            .request::<OpenSubsonicExtensionsApiResponse>("getOpenSubsonicExtensions", &[])
            .await?
            .open_subsonic_extensions
            .extension)
    }
}
