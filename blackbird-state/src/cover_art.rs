use serde::{Deserialize, Serialize};
use smol_str::SmolStr;

/// A cover art ID
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CoverArtId(pub SmolStr);

impl std::fmt::Display for CoverArtId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
