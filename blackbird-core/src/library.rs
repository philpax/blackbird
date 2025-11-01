use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

use blackbird_state::{Album, AlbumId, Group, Track, TrackId};

const SEARCH_CACHE_SIZE: usize = 50;

#[derive(Default)]
pub struct Library {
    pub track_ids: Vec<TrackId>,
    pub track_map: HashMap<TrackId, Track>,
    pub groups: Vec<Arc<Group>>,
    pub albums: HashMap<AlbumId, Album>,
    pub has_loaded_all_tracks: bool,

    // Reverse lookup maps
    pub album_to_group_index: HashMap<AlbumId, usize>,
    pub track_to_group_index: HashMap<TrackId, usize>,
    pub track_to_group_track_index: HashMap<TrackId, usize>,

    track_search_queries: Vec<String>,

    /// Search cache: stores last [`SEARCH_CACHE_SIZE`] queries
    search_cache: HashMap<String, Vec<TrackId>>,
    search_cache_order: VecDeque<String>,
}
impl Library {
    pub fn populate(
        &mut self,
        track_ids: Vec<TrackId>,
        track_map: HashMap<TrackId, Track>,
        groups: Vec<Arc<Group>>,
        albums: HashMap<AlbumId, Album>,
    ) {
        self.albums = albums;
        self.track_map = track_map;
        self.track_ids = track_ids;

        // Clear search cache since library data is changing
        self.search_cache.clear();
        self.search_cache_order.clear();

        // Populate reverse lookup maps for efficient group shuffle navigation
        self.track_to_group_index.clear();
        self.track_to_group_track_index.clear();
        for (group_idx, group) in groups.iter().enumerate() {
            for (track_idx, track_id) in group.tracks.iter().enumerate() {
                self.track_to_group_index
                    .insert(track_id.clone(), group_idx);
                self.track_to_group_track_index
                    .insert(track_id.clone(), track_idx);
            }
            self.album_to_group_index
                .insert(group.album_id.clone(), group_idx);
        }

        fn populate_track_search_queries(
            track_ids: &[TrackId],
            track_map: &HashMap<TrackId, Track>,
            albums: &HashMap<AlbumId, Album>,
        ) -> Vec<String> {
            let mut queries = vec![];
            for track_id in track_ids {
                let track = track_map.get(track_id).unwrap();
                let album = track.album_id.as_ref().and_then(|id| albums.get(id));
                let artist = album
                    .as_ref()
                    .map(|a| a.artist.as_str())
                    .or(track.artist.as_deref());

                let mut query = String::new();
                if let Some(artist) = artist {
                    query.push_str(&artist.to_lowercase());
                    query.push(' ');
                }
                if let Some(album) = album {
                    query.push_str(&album.name.to_lowercase());
                    query.push(' ');
                }
                query.push_str(&track.title.to_lowercase());
                queries.push(query);
            }

            queries
        }
        self.track_search_queries =
            populate_track_search_queries(&self.track_ids, &self.track_map, &self.albums);

        self.groups = groups;
        self.has_loaded_all_tracks = true;
    }

    pub fn set_track_starred(&mut self, track_id: &TrackId, starred: bool) -> Option<bool> {
        let mut old_starred = None;
        if let Some(track) = self.track_map.get_mut(track_id) {
            old_starred = Some(track.starred);
            track.starred = starred;
        }
        old_starred
    }

    pub fn set_album_starred(&mut self, album_id: &AlbumId, starred: bool) -> Option<bool> {
        let mut old_starred = None;

        if let Some(album) = self.albums.get_mut(album_id) {
            old_starred = Some(album.starred);
            album.starred = starred;
        }
        if let Some(group_idx) = self.album_to_group_index.get(album_id)
            && let Some(group) = self.groups.get(*group_idx)
        {
            let group = Group {
                starred,
                ..(**group).clone()
            };
            self.groups[*group_idx] = Arc::new(group);
        }

        old_starred
    }

    pub fn search(&mut self, query: &str) -> Vec<TrackId> {
        let query = query.to_lowercase();

        // Check if the query is in the cache
        if let Some(cached_result) = self.search_cache.get(&query) {
            return cached_result.clone();
        }

        // Perform the search
        let results = self
            .track_search_queries
            .iter()
            .enumerate()
            .filter(|(_, q)| q.contains(&query))
            .map(|(idx, _)| self.track_ids[idx].clone())
            .collect::<Vec<_>>();

        // Add to cache
        self.search_cache.insert(query.clone(), results.clone());
        self.search_cache_order.push_back(query.clone());

        // Maintain cache size limit
        if self.search_cache_order.len() > SEARCH_CACHE_SIZE
            && let Some(oldest_query) = self.search_cache_order.pop_front()
        {
            self.search_cache.remove(&oldest_query);
        }

        results
    }
}
