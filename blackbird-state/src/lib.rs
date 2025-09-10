//! Representations of blackbird's state, as well as a way to retrieve it from an OpenSubsonic server.
//!
//! Separated out to allow for use in other utilities.
#![deny(missing_docs)]

mod album;
mod group;
mod track;

use std::{collections::HashMap, sync::Arc};

pub use album::{Album, AlbumId};
pub use group::Group;
pub use track::{Track, TrackId};

pub use blackbird_subsonic as bs;

/// The output of [`fetch_all`].
pub struct FetchAllOutput {
    /// The albums that were fetched.
    pub albums: HashMap<AlbumId, Album>,
    /// The tracks that were fetched.
    pub track_map: HashMap<TrackId, Track>,
    /// The sorted track IDs.
    pub track_ids: Vec<TrackId>,
    /// The groups that were constructed.
    pub groups: Vec<Arc<Group>>,
}

/// Fetches all albums and tracks from the server, and constructs groups.
///
/// `on_tracks_fetched` is called with the number of tracks that were just fetched,
/// as well as the total number of tracks fetched so far.
pub async fn fetch_all(
    client: &bs::Client,
    on_tracks_fetched: impl Fn(u32, u32),
) -> bs::ClientResult<FetchAllOutput> {
    // Fetch all albums.
    let albums: HashMap<AlbumId, Album> = Album::fetch_all(client)
        .await?
        .into_iter()
        .map(|a| (a.id.clone(), a))
        .collect();

    // Fetch all tracks.
    let mut offset = 0;
    let mut tracks = HashMap::new();
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

        let track_count = response.song.len();
        tracks.extend(
            response
                .song
                .into_iter()
                .map(|s| (TrackId(s.id.clone()), Track::from(s))),
        );
        offset += track_count as u32;
        on_tracks_fetched(track_count as u32, offset);
    }

    // This is all mad ineffcient but cbf doing it better.
    // Sort tracks.
    let mut track_ids: Vec<TrackId> = tracks.keys().cloned().collect();
    {
        let track_data: HashMap<TrackId, _> = track_ids
            .iter()
            .map(|id| {
                let track = tracks.get(id).unwrap_or_else(|| {
                    panic!("Track not found in track map: {id}");
                });
                let album_id = track.album_id.as_ref().unwrap_or_else(|| {
                    panic!("Album ID not found in track: {track:?}");
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
                        track.disc_number.unwrap_or_default(),
                        track.track.unwrap_or_default(),
                        track.title.clone(),
                    ),
                )
            })
            .collect();
        track_ids.sort_by_cached_key(|id| track_data.get(id).unwrap());
    }

    // Build groups.
    let mut groups = vec![];
    {
        let mut current_group: Option<Group> = None;
        for track_id in &track_ids {
            let track = tracks.get(track_id).unwrap_or_else(|| {
                panic!("Track not found in track map: {track_id}");
            });
            let album_id = track.album_id.as_ref().unwrap_or_else(|| {
                panic!("Album ID not found in track: {track:?}");
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
                    tracks: vec![],
                    cover_art_id: album.cover_art_id.clone(),
                });
            }

            current_group
                .as_mut()
                .unwrap()
                .tracks
                .push(track_id.clone());
        }
        if let Some(group) = current_group.take() {
            groups.push(Arc::new(group));
        }
    }

    Ok(FetchAllOutput {
        albums,
        track_map: tracks,
        track_ids,
        groups,
    })
}
