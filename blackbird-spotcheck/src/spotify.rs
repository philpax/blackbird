use std::{collections::HashMap, path::Path};

use serde::{Deserialize, Serialize};

use crate::common::{Album, AlbumId, Albums, Track, Tracks, Uri};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryTrack {
    artist: String,
    album: String,
    track: String,
    uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryAlbum {
    artist: String,
    album: String,
    uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryArtist {
    name: String,
    uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Library {
    tracks: Vec<LibraryTrack>,
    albums: Vec<LibraryAlbum>,
    artists: Vec<LibraryArtist>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingHistoryBaseRecord {
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
pub struct StreamingHistoryTrackRecord {
    #[serde(flatten)]
    base: StreamingHistoryBaseRecord,
    master_metadata_track_name: String,
    master_metadata_album_artist_name: String,
    master_metadata_album_album_name: String,
    spotify_track_uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingHistoryPodcastEpisodeRecord {
    #[serde(flatten)]
    base: StreamingHistoryBaseRecord,
    episode_name: String,
    episode_show_name: String,
    spotify_episode_uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingHistoryAudiobookRecord {
    #[serde(flatten)]
    base: StreamingHistoryBaseRecord,
    audiobook_title: String,
    audiobook_uri: String,
    audiobook_chapter_uri: String,
    audiobook_chapter_title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StreamingHistoryRecord {
    Track(StreamingHistoryTrackRecord),
    PodcastEpisode(StreamingHistoryPodcastEpisodeRecord),
    Audiobook(StreamingHistoryAudiobookRecord),
}

pub type StreamingHistory = Vec<StreamingHistoryRecord>;

pub fn parse_and_collate_data(path: &Path) -> anyhow::Result<(Albums, Tracks)> {
    let library: Library =
        serde_json::from_str(&std::fs::read_to_string(path.join("YourLibrary.json"))?)?;

    let mut albums = HashMap::new();
    for album in library.albums {
        albums.insert(
            AlbumId {
                artist: album.artist.clone(),
                album: album.album.clone(),
            },
            Album {
                album_id: AlbumId {
                    artist: album.artist,
                    album: album.album,
                },
                uri: Some(Uri(album.uri)),
                play_count: 0,
            },
        );
    }

    let mut tracks = HashMap::new();
    for track in library.tracks {
        let album_id = AlbumId {
            artist: track.artist.clone(),
            album: track.album.clone(),
        };
        if !albums.contains_key(&album_id) {
            albums.insert(
                album_id.clone(),
                Album {
                    album_id: album_id.clone(),
                    uri: None,
                    play_count: 0,
                },
            );
        }

        tracks.insert(
            Uri(track.uri.clone()),
            Track {
                album_id,
                track: track.track,
                uri: Uri(track.uri),
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
                        .entry(Uri(track.spotify_track_uri.clone()))
                        .or_insert(Track {
                            album_id: album_id.clone(),
                            track: track.master_metadata_track_name,
                            uri: Uri(track.spotify_track_uri),
                            play_count: 0,
                        })
                        .play_count += 1;

                    albums
                        .entry(album_id.clone())
                        .or_insert(Album {
                            album_id,
                            uri: None,
                            play_count: 0,
                        })
                        .play_count += 1;
                }
                _ => {}
            }
        }
    }

    Ok((Albums(albums), Tracks(tracks)))
}
