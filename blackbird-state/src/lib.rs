//! Representations of blackbird's state, as well as a way to retrieve it from an OpenSubsonic server.
//!
//! Separated out to allow for use in other utilities.
#![deny(missing_docs)]

mod album;
mod group;
mod song;

use std::{collections::HashMap, sync::Arc};

pub use album::{Album, AlbumId};
pub use group::Group;
pub use song::{Song, SongId, SongMap};

pub use blackbird_subsonic as bs;

/// The output of [`fetch_all`].
pub struct FetchAllOutput {
    /// The albums that were fetched.
    pub albums: HashMap<AlbumId, Album>,
    /// The songs that were fetched.
    pub songs: HashMap<SongId, Song>,
    /// The groups that were constructed.
    pub groups: Vec<Arc<Group>>,
}

/// Fetches all albums and songs from the server, and constructs groups.
///
/// `on_songs_fetched` is called with the number of songs that were just fetched,
/// as well as the total number of songs fetched so far.
pub async fn fetch_all(
    client: &bs::Client,
    on_songs_fetched: impl Fn(u32, u32),
) -> bs::ClientResult<FetchAllOutput> {
    // Fetch all albums.
    let albums: HashMap<AlbumId, Album> = Album::fetch_all(client)
        .await?
        .into_iter()
        .map(|a| (a.id.clone(), a))
        .collect();

    // Fetch all songs.
    let mut offset = 0;
    let mut songs = SongMap::new();
    loop {
        let response = client
            .search3(&bs::Search3Request {
                query: "".to_string(),
                artist_count: Some(0),
                album_count: Some(0),
                song_count: Some(10000),
                song_offset: Some(offset),
                ..Default::default()
            })
            .await
            .unwrap();

        if response.song.is_empty() {
            break;
        }

        let song_count = response.song.len();
        songs.extend(
            response
                .song
                .into_iter()
                .map(|s| (SongId(s.id.clone()), Song::from(s))),
        );
        offset += song_count as u32;
        on_songs_fetched(song_count as u32, offset);
    }

    // This is all mad ineffcient but cbf doing it better.
    // Sort songs.
    let mut song_ids: Vec<SongId> = songs.keys().cloned().collect();
    {
        let song_data: HashMap<SongId, _> = song_ids
            .iter()
            .map(|id| {
                let song = songs.get(id).unwrap_or_else(|| {
                    panic!("Song not found in song map: {id}");
                });
                let album_id = song.album_id.as_ref().unwrap_or_else(|| {
                    panic!("Album ID not found in song: {song:?}");
                });
                let album = albums.get(album_id).unwrap_or_else(|| {
                    panic!("Album not found in state: {album_id:?}");
                });
                let album_artist = album.artist.to_lowercase();
                let is_various_artists = album_artist == "various artists";
                (
                    id.clone(),
                    (
                        album_artist,
                        album
                            .year
                            .filter(|_| {
                                // HACK: We want to ignore the date for Various Artists albums;
                                // these should be sorted entirely by name, as there's no
                                // connecting tissue between them.
                                !is_various_artists
                            })
                            .unwrap_or_default(),
                        album.name.clone(),
                        song.disc_number.unwrap_or_default(),
                        song.track.unwrap_or_default(),
                        song.title.clone(),
                    ),
                )
            })
            .collect();
        song_ids.sort_by_cached_key(|id| song_data.get(id).unwrap());
    }

    // Build groups.
    let mut groups = vec![];
    {
        let mut current_group: Option<Group> = None;
        for song_id in &song_ids {
            let song = songs.get(song_id).unwrap_or_else(|| {
                panic!("Song not found in song map: {song_id}");
            });
            let album_id = song.album_id.as_ref().unwrap_or_else(|| {
                panic!("Album ID not found in song: {song:?}");
            });
            let album = albums.get(album_id).unwrap_or_else(|| {
                panic!("Album not found in album map: {album_id:?}");
            });

            if !current_group.as_ref().is_some_and(|group| {
                group.artist == album.artist
                    && group.album == album.name
                    && group.year == album.year
            }) {
                if let Some(group) = current_group.take() {
                    groups.push(Arc::new(group));
                }

                current_group = Some(Group {
                    artist: album.artist.clone(),
                    album: album.name.clone(),
                    year: album.year,
                    duration: album.duration,
                    songs: vec![],
                    cover_art_id: album.cover_art_id.clone(),
                });
            }

            current_group.as_mut().unwrap().songs.push(song_id.clone());
        }
        if let Some(group) = current_group.take() {
            groups.push(Arc::new(group));
        }
    }

    Ok(FetchAllOutput {
        albums,
        songs,
        groups,
    })
}
