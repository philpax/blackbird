use std::{collections::HashMap, path::Path};

use crate::common::{Albums, Tracks};

mod library {
    use std::{collections::HashMap, path::Path};

    use serde::{Deserialize, Serialize};

    use crate::common;

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct Track {
        artist: String,
        album: String,
        track: String,
        uri: String,
    }
    impl From<&Track> for common::AlbumId {
        fn from(track: &Track) -> Self {
            common::AlbumId {
                artist: track.artist.clone(),
                album: track.album.clone(),
            }
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct Album {
        artist: String,
        album: String,
        uri: String,
    }
    impl From<&Album> for common::AlbumId {
        fn from(album: &Album) -> Self {
            common::AlbumId {
                artist: album.artist.clone(),
                album: album.album.clone(),
            }
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct Artist {
        name: String,
        uri: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct Library {
        tracks: Vec<Track>,
        albums: Vec<Album>,
        artists: Vec<Artist>,
    }

    pub fn parse_and_collate(
        data_path: &Path,
        albums: &mut HashMap<common::AlbumId, common::Album>,
        tracks: &mut HashMap<common::Uri, common::Track>,
    ) -> anyhow::Result<()> {
        let library: Library = serde_json::from_str(&std::fs::read_to_string(
            data_path.join("YourLibrary.json"),
        )?)?;

        for album in library.albums {
            albums.insert(
                (&album).into(),
                common::Album {
                    album_id: (&album).into(),
                    uri: Some(common::Uri(album.uri)),
                    play_count: 0,
                },
            );
        }

        for track in library.tracks {
            let album_id = common::AlbumId::from(&track);
            if !albums.contains_key(&album_id) {
                albums.insert(
                    album_id.clone(),
                    common::Album {
                        album_id: album_id.clone(),
                        uri: None,
                        play_count: 0,
                    },
                );
            }

            tracks.insert(
                common::Uri(track.uri.clone()),
                common::Track {
                    album_id,
                    track: track.track,
                    uri: common::Uri(track.uri),
                    play_count: 0,
                },
            );
        }

        Ok(())
    }
}

mod history {
    use std::{collections::HashMap, path::Path};

    use serde::{Deserialize, Serialize};

    use crate::common;

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct BaseRecord {
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
    struct TrackRecord {
        #[serde(flatten)]
        base: BaseRecord,
        master_metadata_track_name: String,
        master_metadata_album_artist_name: String,
        master_metadata_album_album_name: String,
        spotify_track_uri: String,
    }
    impl From<&TrackRecord> for common::AlbumId {
        fn from(track: &TrackRecord) -> Self {
            common::AlbumId {
                artist: track.master_metadata_album_artist_name.clone(),
                album: track.master_metadata_album_album_name.clone(),
            }
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct EpisodeRecord {
        #[serde(flatten)]
        base: BaseRecord,
        episode_name: String,
        episode_show_name: String,
        spotify_episode_uri: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct AudiobookRecord {
        #[serde(flatten)]
        base: BaseRecord,
        audiobook_title: String,
        audiobook_uri: String,
        audiobook_chapter_uri: String,
        audiobook_chapter_title: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(untagged)]
    enum Record {
        Track(TrackRecord),
        PodcastEpisode(EpisodeRecord),
        Audiobook(AudiobookRecord),
    }

    type History = Vec<Record>;

    pub fn parse_and_collate(
        data_path: &Path,
        albums: &mut HashMap<common::AlbumId, common::Album>,
        tracks: &mut HashMap<common::Uri, common::Track>,
    ) -> anyhow::Result<()> {
        for file in data_path
            .join("Spotify Extended Streaming History")
            .read_dir()?
        {
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
            let history: History = serde_json::from_str(&std::fs::read_to_string(file)?)?;

            for record in history {
                let Record::Track(track) = record else {
                    continue;
                };

                let album_id = common::AlbumId::from(&track);

                tracks
                    .entry(common::Uri(track.spotify_track_uri.clone()))
                    .or_insert(common::Track {
                        album_id: album_id.clone(),
                        track: track.master_metadata_track_name,
                        uri: common::Uri(track.spotify_track_uri),
                        play_count: 0,
                    })
                    .play_count += 1;

                albums
                    .entry(album_id.clone())
                    .or_insert(common::Album {
                        album_id,
                        uri: None,
                        play_count: 0,
                    })
                    .play_count += 1;
            }
        }

        Ok(())
    }
}

pub fn parse_and_collate_data(data_path: &Path) -> anyhow::Result<(Albums, Tracks)> {
    let mut albums = HashMap::new();
    let mut tracks = HashMap::new();

    library::parse_and_collate(data_path, &mut albums, &mut tracks)?;
    history::parse_and_collate(data_path, &mut albums, &mut tracks)?;

    Ok((Albums(albums), Tracks(tracks)))
}
