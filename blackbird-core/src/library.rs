use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

use blackbird_state::{Album, AlbumId, Group, Track, TrackId};

use crate::SortOrder;

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
        _track_ids: Vec<TrackId>,
        track_map: HashMap<TrackId, Track>,
        groups: Vec<Arc<Group>>,
        albums: HashMap<AlbumId, Album>,
        sort_order: SortOrder,
    ) {
        self.albums = albums;
        self.track_map = track_map;
        self.groups = groups;

        // Build derived data structures (track_ids, lookup maps, search queries).
        self.resort(sort_order);

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

    /// Resorts the library groups based on the given sort order and rebuilds all lookup structures.
    pub fn resort(&mut self, order: SortOrder) {
        use std::cmp::Ordering;

        /// Compare by artist name (case-insensitive, ascending).
        fn cmp_artist(a: &Group, b: &Group) -> Ordering {
            a.artist.to_lowercase().cmp(&b.artist.to_lowercase())
        }

        /// Compare by year (descending, newest first; None values sort last).
        fn cmp_year_desc(a: &Group, b: &Group) -> Ordering {
            match (a.year, b.year) {
                (Some(y1), Some(y2)) => y2.cmp(&y1),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            }
        }

        /// Compare by year (ascending, oldest first; None values sort last).
        fn cmp_year_asc(a: &Group, b: &Group) -> Ordering {
            match (a.year, b.year) {
                (Some(y1), Some(y2)) => y1.cmp(&y2),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            }
        }

        /// Compare by album name (case-insensitive, ascending).
        fn cmp_album(a: &Group, b: &Group) -> Ordering {
            a.album.to_lowercase().cmp(&b.album.to_lowercase())
        }

        /// Compare by (artist, year asc, album).
        fn cmp_artist_year_album(a: &Group, b: &Group) -> Ordering {
            cmp_artist(a, b)
                .then_with(|| cmp_year_asc(a, b))
                .then_with(|| cmp_album(a, b))
        }

        match order {
            SortOrder::Alphabetical => {
                // Sort by (artist, year desc, album).
                self.groups.sort_by(|a, b| cmp_artist_year_album(a, b));
            }
            SortOrder::NewestFirst => {
                // Sort by (year desc, artist, album).
                self.groups.sort_by(|a, b| {
                    cmp_year_desc(a, b)
                        .then_with(|| cmp_artist(a, b))
                        .then_with(|| cmp_album(a, b))
                });
            }
            SortOrder::RecentlyAdded => {
                // Sort by (added desc, artist, year desc, album).
                let albums = &self.albums;
                self.groups.sort_by(|a, b| {
                    let created_a = albums.get(&a.album_id).map(|album| album.created.as_str());
                    let created_b = albums.get(&b.album_id).map(|album| album.created.as_str());
                    // Reverse comparison for descending order (most recent first).
                    created_b
                        .cmp(&created_a)
                        .then_with(|| cmp_artist_year_album(a, b))
                });
            }
        }

        // Rebuild track_ids from reordered groups.
        self.track_ids.clear();
        for group in &self.groups {
            for track_id in &group.tracks {
                self.track_ids.push(track_id.clone());
            }
        }

        // Rebuild reverse lookup maps.
        self.track_to_group_index.clear();
        self.track_to_group_track_index.clear();
        self.album_to_group_index.clear();
        for (group_idx, group) in self.groups.iter().enumerate() {
            for (track_idx, track_id) in group.tracks.iter().enumerate() {
                self.track_to_group_index
                    .insert(track_id.clone(), group_idx);
                self.track_to_group_track_index
                    .insert(track_id.clone(), track_idx);
            }
            self.album_to_group_index
                .insert(group.album_id.clone(), group_idx);
        }

        // Clear search cache since the order has changed.
        self.search_cache.clear();
        self.search_cache_order.clear();

        // Rebuild track search queries to match new order.
        self.track_search_queries.clear();
        for track_id in &self.track_ids {
            let track = self.track_map.get(track_id).unwrap();
            let album = track.album_id.as_ref().and_then(|id| self.albums.get(id));
            let artist = track
                .artist
                .as_deref()
                .or(album.as_ref().map(|a| a.artist.as_str()));

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
            self.track_search_queries.push(query);
        }
    }
}
