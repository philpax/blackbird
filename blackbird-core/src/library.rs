use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    ops::Bound,
    sync::Arc,
};

use blackbird_state::{Album, AlbumId, Group, Track, TrackId};
use smallvec::SmallVec;
use smol_str::SmolStr;

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

    /// Inverted search index: normalized word → track indices (into `track_ids`).
    /// Each posting list is sorted and deduplicated.
    word_index: BTreeMap<SmolStr, Vec<u32>>,

    /// Search cache: stores last [`SEARCH_CACHE_SIZE`] queries.
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
        let cache_key = query.to_lowercase();

        if let Some(cached_result) = self.search_cache.get(&cache_key) {
            return cached_result.clone();
        }

        let results = self.run_search(query);

        self.search_cache.insert(cache_key.clone(), results.clone());
        self.search_cache_order.push_back(cache_key);

        if self.search_cache_order.len() > SEARCH_CACHE_SIZE
            && let Some(oldest_query) = self.search_cache_order.pop_front()
        {
            self.search_cache.remove(&oldest_query);
        }

        results
    }

    fn run_search(&self, query: &str) -> Vec<TrackId> {
        let variants = normalize_variants(query);

        // Union of matches across all query variants.
        let mut matching_indices: BTreeSet<u32> = BTreeSet::new();

        for variant in &variants {
            let tokens: SmallVec<[&str; 4]> = variant.split_whitespace().collect();
            if tokens.is_empty() {
                continue;
            }

            // Intersect per-token match sets within a single variant: a track
            // matches the variant only if every token has a prefix match on at
            // least one of the track's indexed words.
            let mut variant_matches: Option<BTreeSet<u32>> = None;
            for token in &tokens {
                let token_matches = self.indices_with_word_prefix(token);
                variant_matches = Some(match variant_matches {
                    None => token_matches,
                    Some(existing) => existing
                        .intersection(&token_matches)
                        .copied()
                        .collect::<BTreeSet<_>>(),
                });
                if variant_matches.as_ref().is_some_and(|m| m.is_empty()) {
                    break;
                }
            }

            if let Some(vm) = variant_matches {
                matching_indices.extend(vm);
            }
        }

        matching_indices
            .into_iter()
            .map(|idx| self.track_ids[idx as usize].clone())
            .collect()
    }

    /// Returns the set of track indices for any indexed word that starts with
    /// `prefix`, discovered via a BTreeMap range scan over the index.
    fn indices_with_word_prefix(&self, prefix: &str) -> BTreeSet<u32> {
        let mut matches = BTreeSet::new();
        for (word, indices) in self
            .word_index
            .range::<str, _>((Bound::Included(prefix), Bound::Unbounded))
        {
            if !word.starts_with(prefix) {
                break;
            }
            matches.extend(indices.iter().copied());
        }
        matches
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
            SortOrder::MostPlayed => {
                // Sort by average playcount per listened track (descending).
                // Groups with no listened tracks sort last.
                let track_map = &self.track_map;
                self.groups.sort_by(|a, b| {
                    let avg_playcount = |group: &Group| -> Option<f64> {
                        let mut total: u64 = 0;
                        let mut count: u64 = 0;
                        for track_id in &group.tracks {
                            if let Some(track) = track_map.get(track_id)
                                && let Some(pc) = track.play_count
                                && pc > 0
                            {
                                total += pc;
                                count += 1;
                            }
                        }
                        if count > 0 {
                            Some(total as f64 / count as f64)
                        } else {
                            None
                        }
                    };
                    let avg_a = avg_playcount(a);
                    let avg_b = avg_playcount(b);
                    match (avg_a, avg_b) {
                        (Some(a_val), Some(b_val)) => b_val
                            .partial_cmp(&a_val)
                            .unwrap_or(Ordering::Equal)
                            .then_with(|| cmp_artist_year_album(a, b)),
                        (Some(_), None) => Ordering::Less,
                        (None, Some(_)) => Ordering::Greater,
                        (None, None) => cmp_artist_year_album(a, b),
                    }
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

        // Rebuild the inverted word index to match the new track order.
        self.word_index.clear();
        for (idx, track_id) in self.track_ids.iter().enumerate() {
            let idx = idx as u32;
            let track = self.track_map.get(track_id).unwrap();
            let album = track.album_id.as_ref().and_then(|id| self.albums.get(id));
            let artist = track
                .artist
                .as_deref()
                .or(album.as_ref().map(|a| a.artist.as_str()));

            let mut raw = String::new();
            if let Some(artist) = artist {
                raw.push_str(artist);
                raw.push(' ');
            }
            if let Some(album) = album {
                raw.push_str(&album.name);
                raw.push(' ');
            }
            raw.push_str(&track.title);

            for variant in normalize_variants(&raw) {
                for word in variant.split_whitespace() {
                    // Tracks are iterated in ascending order, so the posting
                    // list for a word grows monotonically. Checking `last()` is
                    // enough to avoid duplicates without a post-pass.
                    let postings = self.word_index.entry(SmolStr::new(word)).or_default();
                    if postings.last() != Some(&idx) {
                        postings.push(idx);
                    }
                }
            }
        }
    }
}

/// Returns deduplicated normalized variants of `s` for indexing or querying.
///
/// This currently emits up to two forms:
/// - `stripped`: lowercase, with ASCII punctuation removed outright.
/// - `spaced`: lowercase, with ASCII punctuation replaced by spaces and runs of
///   whitespace collapsed.
///
/// The two forms coincide when `s` contains no punctuation (or when punctuation
/// is already adjacent to whitespace), so in the common case only one variant
/// is returned. Indexing and querying both apply this function, so e.g. a
/// query of `"ac dc"` finds a track titled `"AC/DC"` via the spaced variant,
/// while `"acdc"` finds it via the stripped variant.
fn normalize_variants(s: &str) -> SmallVec<[SmolStr; 2]> {
    let stripped: String = s
        .chars()
        .filter(|c| !c.is_ascii_punctuation())
        .flat_map(|c| c.to_lowercase())
        .collect();

    let spaced_raw: String = s
        .chars()
        .map(|c| if c.is_ascii_punctuation() { ' ' } else { c })
        .flat_map(|c| c.to_lowercase())
        .collect();
    let mut spaced = String::with_capacity(spaced_raw.len());
    for word in spaced_raw.split_whitespace() {
        if !spaced.is_empty() {
            spaced.push(' ');
        }
        spaced.push_str(word);
    }

    let mut variants: SmallVec<[SmolStr; 2]> = SmallVec::new();
    variants.push(SmolStr::new(&stripped));
    if spaced != stripped {
        variants.push(SmolStr::new(&spaced));
    }
    variants
}

#[cfg(test)]
mod tests {
    use super::*;

    fn variants(s: &str) -> Vec<String> {
        normalize_variants(s)
            .into_iter()
            .map(|v| v.to_string())
            .collect()
    }

    #[test]
    fn normalize_variants_collapses_when_equal() {
        // No punctuation: one variant.
        assert_eq!(variants("Hello World"), vec!["hello world"]);
        // Punctuation adjacent to whitespace yields identical forms.
        assert_eq!(variants("Mr. Invisible"), vec!["mr invisible"]);
    }

    #[test]
    fn normalize_variants_emits_both_for_intra_word_punctuation() {
        assert_eq!(variants("AC/DC"), vec!["acdc", "ac dc"]);
        assert_eq!(variants("Sci-Fi"), vec!["scifi", "sci fi"]);
        assert_eq!(variants("John's"), vec!["johns", "john s"]);
    }

    #[test]
    fn normalize_variants_handles_runs_of_punctuation() {
        // Multiple punctuation chars collapse to a single space in the spaced
        // form, matching how a user would type the query.
        assert_eq!(
            variants("J.R.R. Tolkien"),
            vec!["jrr tolkien", "j r r tolkien"]
        );
    }

    /// A single track specification for test fixtures: `(track_id, title, artist, album_id, album_name)`.
    type TrackSpec = (
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        &'static str,
    );

    fn build_library(specs: &[TrackSpec]) -> Library {
        let mut track_map: HashMap<TrackId, Track> = HashMap::new();
        let mut albums: HashMap<AlbumId, Album> = HashMap::new();
        let mut group_tracks: HashMap<AlbumId, Vec<TrackId>> = HashMap::new();

        for (tid, title, artist, aid, aname) in specs {
            let track_id = TrackId((*tid).into());
            let album_id = AlbumId((*aid).into());

            track_map.insert(
                track_id.clone(),
                Track {
                    id: track_id.clone(),
                    title: (*title).into(),
                    artist: Some((*artist).into()),
                    track: None,
                    year: None,
                    _genre: None,
                    duration: None,
                    disc_number: None,
                    album_id: Some(album_id.clone()),
                    starred: false,
                    play_count: None,
                },
            );
            albums.entry(album_id.clone()).or_insert_with(|| Album {
                id: album_id.clone(),
                name: (*aname).into(),
                artist: (*artist).into(),
                artist_id: None,
                cover_art_id: None,
                track_count: 0,
                duration: 0,
                year: None,
                _genre: None,
                starred: false,
                created: "".into(),
            });
            group_tracks.entry(album_id).or_default().push(track_id);
        }

        let groups: Vec<Arc<Group>> = group_tracks
            .into_iter()
            .map(|(album_id, tracks)| {
                let album = &albums[&album_id];
                Arc::new(Group {
                    artist: album.artist.clone(),
                    sort_artist: album.artist.clone(),
                    album: album.name.clone(),
                    year: None,
                    duration: 0,
                    tracks,
                    cover_art_id: None,
                    album_id,
                    starred: false,
                })
            })
            .collect();

        let mut library = Library::default();
        library.populate(vec![], track_map, groups, albums, SortOrder::Alphabetical);
        library
    }

    fn search_ids(library: &mut Library, query: &str) -> Vec<String> {
        library.search(query).into_iter().map(|id| id.0).collect()
    }

    #[test]
    fn search_finds_track_with_punctuation_in_title() {
        let mut lib = build_library(&[
            ("t1", "Mr. Invisible", "Some Artist", "a1", "Album One"),
            ("t2", "Something Else", "Other Artist", "a2", "Album Two"),
        ]);

        // The original motivating case.
        assert_eq!(search_ids(&mut lib, "mr invisible"), vec!["t1"]);
        // Punctuation in the query itself is also normalized.
        assert_eq!(search_ids(&mut lib, "Mr. Invisible"), vec!["t1"]);
    }

    #[test]
    fn search_matches_both_intra_word_forms() {
        let mut lib = build_library(&[
            ("t1", "Thunderstruck", "AC/DC", "a1", "The Razors Edge"),
            ("t2", "Starlight", "Muse", "a2", "Black Holes"),
        ]);

        // Collapsed form.
        assert_eq!(search_ids(&mut lib, "acdc"), vec!["t1"]);
        // Spaced form.
        assert_eq!(search_ids(&mut lib, "ac dc"), vec!["t1"]);
        // Original form.
        assert_eq!(search_ids(&mut lib, "AC/DC"), vec!["t1"]);
    }

    #[test]
    fn search_intersects_tokens_regardless_of_order() {
        let mut lib = build_library(&[
            ("t1", "Invisible Touch", "Genesis", "a1", "Invisible Touch"),
            ("t2", "Mr. Invisible", "Genesis", "a1", "Invisible Touch"),
            (
                "t3",
                "Land of Confusion",
                "Genesis",
                "a1",
                "Invisible Touch",
            ),
        ]);

        // All three tracks are on the "Invisible Touch" album, so all three
        // index the word "invisible". Adding "mr" narrows to t2 only, and
        // order of the query tokens should not matter.
        let forward = search_ids(&mut lib, "mr invisible");
        let reverse = search_ids(&mut lib, "invisible mr");
        assert_eq!(forward, vec!["t2"]);
        assert_eq!(forward, reverse);
    }

    #[test]
    fn search_uses_prefix_matching_per_token() {
        let mut lib = build_library(&[
            ("t1", "Invisible Touch", "Genesis", "a1", "Invisible Touch"),
            ("t2", "Mr. Invisible", "Genesis", "a1", "Invisible Touch"),
        ]);

        // Prefix of a word matches.
        let mut got = search_ids(&mut lib, "invis");
        got.sort();
        assert_eq!(got, vec!["t1", "t2"]);
        // A non-prefix substring does not match (this is the intentional
        // semantic change from the previous substring-on-full-haystack
        // behavior).
        assert!(search_ids(&mut lib, "sible").is_empty());
    }

    #[test]
    fn search_returns_empty_for_no_match() {
        let mut lib = build_library(&[("t1", "Hello World", "Artist", "a1", "Album")]);
        assert!(search_ids(&mut lib, "xyz").is_empty());
    }
}
