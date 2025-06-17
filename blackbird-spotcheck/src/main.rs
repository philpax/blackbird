use std::{
    collections::HashMap,
    io::Write as _,
    path::{Path, PathBuf},
};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Album {
    artist: String,
    album: String,
    uri: Option<String>,
    play_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
struct AlbumId {
    artist: String,
    album: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Track {
    album_id: AlbumId,
    track: String,
    uri: String,
    play_count: u32,
}

fn main() -> anyhow::Result<()> {
    let path = PathBuf::from(std::env::args().nth(1).context("No path provided")?);

    let library: Library =
        serde_json::from_str(&std::fs::read_to_string(path.join("YourLibrary.json"))?)?;

    let mut tracks = HashMap::new();
    let mut albums = HashMap::new();
    for album in library.albums {
        albums.insert(
            AlbumId {
                artist: album.artist.clone(),
                album: album.album.clone(),
            },
            Album {
                artist: album.artist,
                album: album.album,
                uri: Some(album.uri),
                play_count: 0,
            },
        );
    }

    for track in library.tracks {
        let album_id = AlbumId {
            artist: track.artist.clone(),
            album: track.album.clone(),
        };
        if !albums.contains_key(&album_id) {
            albums.insert(
                album_id.clone(),
                Album {
                    artist: track.artist.clone(),
                    album: track.album.clone(),
                    uri: None,
                    play_count: 0,
                },
            );
        }

        tracks.insert(
            track.uri.clone(),
            Track {
                album_id,
                track: track.track,
                uri: track.uri,
                play_count: 0,
            },
        );
    }

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

        for record in history {
            match record {
                StreamingHistoryRecord::Track(track) => {
                    let album_id = AlbumId {
                        artist: track.master_metadata_album_artist_name,
                        album: track.master_metadata_album_album_name,
                    };

                    tracks
                        .entry(track.spotify_track_uri.clone())
                        .or_insert(Track {
                            album_id: album_id.clone(),
                            track: track.master_metadata_track_name,
                            uri: track.spotify_track_uri,
                            play_count: 0,
                        })
                        .play_count += 1;

                    albums
                        .entry(album_id.clone())
                        .or_insert(Album {
                            artist: album_id.artist,
                            album: album_id.album,
                            uri: None,
                            play_count: 0,
                        })
                        .play_count += 1;
                }
                _ => {}
            }
        }
    }

    let output_dir = Path::new("spotcheck-output");
    let _ = std::fs::remove_dir_all(output_dir);
    std::fs::create_dir_all(output_dir)?;

    let mut tracks_file = std::fs::File::create(output_dir.join("tracks.ndjson"))?;
    for track in tracks.values() {
        serde_json::to_writer(&mut tracks_file, &track)?;
        tracks_file.write_all(b"\n")?;
    }

    let mut albums_file = std::fs::File::create(output_dir.join("albums.ndjson"))?;
    for album in albums.values() {
        serde_json::to_writer(&mut albums_file, &album)?;
        albums_file.write_all(b"\n")?;
    }

    Ok(())
}
