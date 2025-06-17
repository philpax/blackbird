use std::{
    collections::HashMap,
    hash::Hash,
    io::{BufRead as _, Write as _},
    path::Path,
};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
#[serde(transparent)]
pub struct Uri(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub struct AlbumId {
    pub artist: String,
    pub album: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Album {
    pub album_id: AlbumId,
    pub uri: Option<Uri>,
    pub play_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub album_id: AlbumId,
    pub track: String,
    pub uri: Uri,
    pub play_count: u32,
}

pub trait Ndjson:
    Serialize
    + DeserializeOwned
    + Sized
    + From<HashMap<Self::Id, Self::Value>>
    + AsRef<HashMap<Self::Id, Self::Value>>
{
    type Id: Serialize + DeserializeOwned + Hash + Eq + PartialEq;
    type Value: Serialize + DeserializeOwned;

    fn get_id_for_value(value: &Self::Value) -> Self::Id;

    fn load(path: &Path) -> anyhow::Result<Self> {
        let file = std::fs::File::open(path)?;
        let reader = std::io::BufReader::new(file);
        let mut items = HashMap::new();
        for line in reader.lines() {
            let item: Self::Value = serde_json::from_str(&line?)?;
            items.insert(Self::get_id_for_value(&item), item);
        }
        Ok(Self::from(items))
    }

    fn save(&self, path: &Path) -> anyhow::Result<()> {
        let file = std::fs::File::create(path)?;
        let mut writer = std::io::BufWriter::new(file);
        for item in self.as_ref().values() {
            serde_json::to_writer(&mut writer, &item)?;
            writer.write_all(b"\n")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Albums(pub HashMap<AlbumId, Album>);
impl From<HashMap<AlbumId, Album>> for Albums {
    fn from(value: HashMap<AlbumId, Album>) -> Self {
        Albums(value)
    }
}
impl AsRef<HashMap<AlbumId, Album>> for Albums {
    fn as_ref(&self) -> &HashMap<AlbumId, Album> {
        &self.0
    }
}
impl Ndjson for Albums {
    type Id = AlbumId;
    type Value = Album;

    fn get_id_for_value(value: &Self::Value) -> Self::Id {
        value.album_id.clone()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Tracks(pub HashMap<Uri, Track>);
impl From<HashMap<Uri, Track>> for Tracks {
    fn from(value: HashMap<Uri, Track>) -> Self {
        Tracks(value)
    }
}
impl AsRef<HashMap<Uri, Track>> for Tracks {
    fn as_ref(&self) -> &HashMap<Uri, Track> {
        &self.0
    }
}
impl Ndjson for Tracks {
    type Id = Uri;
    type Value = Track;

    fn get_id_for_value(value: &Self::Value) -> Self::Id {
        value.uri.clone()
    }
}
