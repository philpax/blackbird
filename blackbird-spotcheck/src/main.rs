use std::path::PathBuf;

use anyhow::Context as _;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LibraryTrack {
    artist: String,
    album: String,
    track: String,
    uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LibraryAlbum {
    artist: String,
    album: String,
    uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LibraryArtist {
    name: String,
    uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Library {
    tracks: Vec<LibraryTrack>,
    albums: Vec<LibraryAlbum>,
    artists: Vec<LibraryArtist>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StreamingHistoryBaseRecord {
    ts: chrono::DateTime<chrono::Utc>,
    platform: String,
    ms_played: u64,
    conn_country: String,
    ip_addr: String,
    audiobook_chapter_title: Option<String>,
    reason_start: String,
    reason_end: String,
    shuffle: bool,
    skipped: bool,
    offline: bool,
    offline_timestamp: Option<u64>,
    incognito_mode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StreamingHistoryTrackRecord {
    #[serde(flatten)]
    base: StreamingHistoryBaseRecord,
    master_metadata_track_name: String,
    master_metadata_album_artist_name: String,
    master_metadata_album_album_name: String,
    spotify_track_uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StreamingHistoryPodcastEpisodeRecord {
    #[serde(flatten)]
    base: StreamingHistoryBaseRecord,
    episode_name: String,
    episode_show_name: String,
    spotify_episode_uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StreamingHistoryAudiobookRecord {
    #[serde(flatten)]
    base: StreamingHistoryBaseRecord,
    audiobook_title: String,
    audiobook_uri: String,
    audiobook_chapter_uri: String,
    audiobook_chapter_title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum StreamingHistoryRecord {
    Track(StreamingHistoryTrackRecord),
    PodcastEpisode(StreamingHistoryPodcastEpisodeRecord),
    Audiobook(StreamingHistoryAudiobookRecord),
}

type StreamingHistory = Vec<StreamingHistoryRecord>;

fn main() -> anyhow::Result<()> {
    let path = PathBuf::from(std::env::args().nth(1).context("No path provided")?);

    let library: Library =
        serde_json::from_str(&std::fs::read_to_string(path.join("YourLibrary.json"))?)?;

    println!("{library:?}");

    for file in path.join("Spotify Extended Streaming History").read_dir()? {
        let file = file?;
        let file = file.path();
        if !file
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("Streaming_History_Audio")
        {
            continue;
        }

        println!("Processing {}", file.display());
        let history: StreamingHistory = serde_json::from_str(&std::fs::read_to_string(file)?)?;
        println!("{history:?}");
    }

    Ok(())
}
