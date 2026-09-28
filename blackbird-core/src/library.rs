use std::{
    borrow::Cow,
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet, VecDeque},
    ops::Bound,
    sync::Arc,
};

use blackbird_state::{Album, AlbumId, AlphabeticalOrder, FetchAllOutput, Group, Track, TrackId};
use icu_normalizer::DecomposingNormalizer;
use icu_properties::{CodePointMapData, props::CanonicalCombiningClass};
use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;
use smol_str::SmolStr;

use crate::SortOrder;

const SEARCH_CACHE_SIZE: usize = 50;

#[derive(Default)]
pub struct Library {
    pub track_ids: Vec<TrackId>,
    pub track_map: FxHashMap<TrackId, Track>,
    pub groups: Vec<Arc<Group>>,
    pub albums: FxHashMap<AlbumId, Album>,
    pub has_loaded_all_tracks: bool,

    /// The index into `groups` of each album's group.
    pub album_to_group_index: FxHashMap<AlbumId, usize>,

    // The library's layout when it was last populated. `resort` only reorders
    // the groups, so data keyed by this layout stays valid across re-sorts,
    // and needs only the small `group_positions` to be kept up to date.
    /// The track IDs in populated order; the search index refers to tracks by
    /// their position in this list.
    populated_track_ids: Vec<TrackId>,
    /// The position in `populated_track_ids` of each populated group's first
    /// track, in populated order.
    populated_group_starts: Vec<u32>,
    /// The populated group of each track.
    track_populated_groups: FxHashMap<TrackId, u32>,
    /// The populated group of each entry of `groups`.
    group_populated_indices: Vec<u32>,
    /// The index into `groups` of each populated group.
    group_positions: Vec<u32>,

    /// Inverted search index: normalized word → track positions (into
    /// `populated_track_ids`). Each posting list is sorted and deduplicated.
    word_index: BTreeMap<SmolStr, Vec<u32>>,

    /// Search cache: stores last [`SEARCH_CACHE_SIZE`] queries.
    search_cache: FxHashMap<String, Vec<TrackId>>,
    search_cache_order: VecDeque<String>,

    /// Tracks whose starred status was changed locally since the last library
    /// refresh began. The refresh may have read them before the change, so
    /// [`Self::carry_over_from`] keeps the local values.
    locally_starred_tracks: FxHashSet<TrackId>,
    /// Tracks whose play count was updated locally since the last library
    /// refresh began; see `locally_starred_tracks`.
    locally_played_tracks: FxHashSet<TrackId>,
    /// Albums whose starred status was changed locally since the last library
    /// refresh began; see `locally_starred_tracks`.
    locally_starred_albums: FxHashSet<AlbumId>,
}
impl Library {
    pub fn populate(
        &mut self,
        track_map: FxHashMap<TrackId, Track>,
        groups: Vec<Arc<Group>>,
        albums: FxHashMap<AlbumId, Album>,
        sort_order: SortOrder,
    ) {
        self.albums = albums;
        self.track_map = track_map;
        self.groups = groups;

        self.populated_track_ids.clear();
        self.populated_group_starts.clear();
        self.track_populated_groups.clear();
        self.track_populated_groups.reserve(self.track_map.len());
        for (group_idx, group) in self.groups.iter().enumerate() {
            self.populated_group_starts
                .push(self.populated_track_ids.len() as u32);
            for track_id in &group.tracks {
                self.populated_track_ids.push(track_id.clone());
                self.track_populated_groups
                    .insert(track_id.clone(), group_idx as u32);
            }
        }
        self.group_populated_indices = (0..self.groups.len() as u32).collect();
        self.rebuild_word_index();

        // Build the order-dependent structures.
        self.resort(sort_order);

        self.has_loaded_all_tracks = true;
    }

    /// Returns the index into `groups` of the group containing `track_id`.
    pub fn group_index_of(&self, track_id: &TrackId) -> Option<usize> {
        let populated_group = *self.track_populated_groups.get(track_id)?;
        Some(self.group_positions[populated_group as usize] as usize)
    }

    /// Marks the start of a library refresh, from which point local edits are
    /// tracked so that [`Self::replace`] can carry them over.
    pub fn begin_refresh(&mut self) {
        self.locally_starred_tracks.clear();
        self.locally_played_tracks.clear();
        self.locally_starred_albums.clear();
    }

    /// Builds a library from a fetch.
    pub fn from_fetched(fetched: FetchAllOutput, sort_order: SortOrder) -> Self {
        let mut library = Library::default();
        library.populate(
            fetched.track_map,
            fetched.groups,
            fetched.albums,
            sort_order,
        );
        library
    }

    /// Takes over the state of `previous` that a fresh fetch doesn't capture,
    /// in preparation for replacing it with this library.
    ///
    /// Local edits made to `previous` since [`Self::begin_refresh`] are
    /// carried over, as the fetch may predate them. `keep_track` (e.g. the
    /// track that is currently playing) is kept in `track_map` even if it is no
    /// longer on the server, so that its details remain available until it
    /// stops playing; it won't appear in the library itself.
    pub fn carry_over_from(
        &mut self,
        previous: &Library,
        sort_order: SortOrder,
        keep_track: Option<&TrackId>,
    ) {
        for track_id in &previous.locally_starred_tracks {
            if let (Some(local), Some(track)) = (
                previous.track_map.get(track_id),
                self.track_map.get_mut(track_id),
            ) {
                track.starred = local.starred;
            }
        }

        let mut play_counts_changed = false;
        for track_id in &previous.locally_played_tracks {
            if let (Some(local), Some(track)) = (
                previous.track_map.get(track_id),
                self.track_map.get_mut(track_id),
            ) && track.play_count != local.play_count
            {
                track.play_count = local.play_count;
                play_counts_changed = true;
            }
        }

        for album_id in &previous.locally_starred_albums {
            if let Some(local) = previous.albums.get(album_id) {
                self.apply_album_starred(album_id, local.starred);
            }
        }

        if let Some(track_id) = keep_track
            && !self.track_map.contains_key(track_id)
            && let Some(track) = previous.track_map.get(track_id)
        {
            if let Some(album_id) = &track.album_id
                && !self.albums.contains_key(album_id)
                && let Some(album) = previous.albums.get(album_id)
            {
                self.albums.insert(album_id.clone(), album.clone());
            }
            self.track_map.insert(track_id.clone(), track.clone());
        }

        if play_counts_changed && sort_order == SortOrder::MostPlayed {
            self.resort(sort_order);
        }
    }

    /// Replaces the library with a freshly fetched one; see
    /// [`Self::carry_over_from`].
    pub fn replace(
        &mut self,
        fetched: FetchAllOutput,
        sort_order: SortOrder,
        keep_track: Option<&TrackId>,
    ) {
        let mut library = Self::from_fetched(fetched, sort_order);
        library.carry_over_from(self, sort_order, keep_track);
        *self = library;
    }

    /// Replaces a single track's data, e.g. after re-fetching it to pick up a
    /// new play count.
    pub fn update_track(&mut self, track: Track) {
        self.locally_played_tracks.insert(track.id.clone());
        self.track_map.insert(track.id.clone(), track);
    }

    pub fn set_track_starred(&mut self, track_id: &TrackId, starred: bool) -> Option<bool> {
        self.locally_starred_tracks.insert(track_id.clone());
        let mut old_starred = None;
        if let Some(track) = self.track_map.get_mut(track_id) {
            old_starred = Some(track.starred);
            track.starred = starred;
        }
        old_starred
    }

    pub fn set_album_starred(&mut self, album_id: &AlbumId, starred: bool) -> Option<bool> {
        self.locally_starred_albums.insert(album_id.clone());
        self.apply_album_starred(album_id, starred)
    }

    fn apply_album_starred(&mut self, album_id: &AlbumId, starred: bool) -> Option<bool> {
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

        // Order the matches as they appear in the library. Positions within a
        // populated group are in track order, and whole groups are ordered by
        // their current position.
        let mut matches: Vec<(u32, u32)> = matching_indices
            .into_iter()
            .map(|idx| {
                let populated_group = self
                    .populated_group_starts
                    .partition_point(|&start| start <= idx)
                    - 1;
                (self.group_positions[populated_group], idx)
            })
            .collect();
        matches.sort_unstable();
        matches
            .into_iter()
            .map(|(_, idx)| self.populated_track_ids[idx as usize].clone())
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

    /// Resorts the library groups based on the given sort order and rebuilds
    /// the structures that depend on the order.
    pub fn resort(&mut self, order: SortOrder) {
        /// Compare by year (descending, newest first; None values sort last).
        fn cmp_year_desc(a: &Group, b: &Group) -> Ordering {
            match (a.year, b.year) {
                (Some(y1), Some(y2)) => y2.cmp(&y1),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            }
        }

        // Sort a permutation of the groups rather than the groups themselves,
        // so that the comparators can use per-group data computed once up
        // front instead of on every comparison.
        let groups = &self.groups;
        let alphabetical = AlphabeticalOrder::new();
        let cmp_alphabetical = |a: usize, b: usize| alphabetical.compare(&groups[a], &groups[b]);

        let mut permutation: Vec<usize> = (0..groups.len()).collect();
        match order {
            SortOrder::Alphabetical => {
                permutation.sort_by(|&a, &b| cmp_alphabetical(a, b));
            }
            SortOrder::NewestFirst => {
                // Sort by year descending, then alphabetically.
                permutation.sort_by(|&a, &b| {
                    cmp_year_desc(&groups[a], &groups[b]).then_with(|| cmp_alphabetical(a, b))
                });
            }
            SortOrder::RecentlyAdded => {
                // Sort by added descending, then alphabetically.
                let created: Vec<Option<&str>> = groups
                    .iter()
                    .map(|g| {
                        self.albums
                            .get(&g.album_id)
                            .map(|album| album.created.as_str())
                    })
                    .collect();
                permutation.sort_by(|&a, &b| {
                    // Reverse comparison for descending order (most recent first).
                    created[b]
                        .cmp(&created[a])
                        .then_with(|| cmp_alphabetical(a, b))
                });
            }
            SortOrder::MostPlayed => {
                // Sort by average playcount per listened track (descending).
                // Groups with no listened tracks sort last.
                let avg_playcounts: Vec<Option<f64>> = groups
                    .iter()
                    .map(|group| {
                        let mut total: u64 = 0;
                        let mut count: u64 = 0;
                        for track_id in &group.tracks {
                            if let Some(track) = self.track_map.get(track_id)
                                && let Some(pc) = track.play_count
                                && pc > 0
                            {
                                total += pc;
                                count += 1;
                            }
                        }
                        (count > 0).then(|| total as f64 / count as f64)
                    })
                    .collect();
                permutation.sort_by(|&a, &b| match (avg_playcounts[a], avg_playcounts[b]) {
                    (Some(a_val), Some(b_val)) => b_val
                        .partial_cmp(&a_val)
                        .unwrap_or(Ordering::Equal)
                        .then_with(|| cmp_alphabetical(a, b)),
                    (Some(_), None) => Ordering::Less,
                    (None, Some(_)) => Ordering::Greater,
                    (None, None) => cmp_alphabetical(a, b),
                });
            }
        }
        self.groups = permutation
            .iter()
            .map(|&i| self.groups[i].clone())
            .collect();
        self.group_populated_indices = permutation
            .iter()
            .map(|&i| self.group_populated_indices[i])
            .collect();

        // Rebuild the structures that depend on the order.
        self.group_positions = vec![0; self.groups.len()];
        for (position, &populated_group) in self.group_populated_indices.iter().enumerate() {
            self.group_positions[populated_group as usize] = position as u32;
        }

        self.track_ids.clear();
        self.track_ids.extend(
            self.groups
                .iter()
                .flat_map(|group| group.tracks.iter().cloned()),
        );

        self.album_to_group_index.clear();
        for (group_idx, group) in self.groups.iter().enumerate() {
            self.album_to_group_index
                .insert(group.album_id.clone(), group_idx);
        }

        // Clear search cache since the order has changed.
        self.search_cache.clear();
        self.search_cache_order.clear();
    }

    /// Rebuilds the inverted word index over `populated_track_ids`.
    ///
    /// A track is indexed under the words of its artist, album name, and
    /// title. Normalizing each part separately yields the same words as
    /// normalizing them joined by spaces (normalization works per character,
    /// and the parts are only ever split on whitespace), which lets the artist
    /// and album words be computed once rather than for every track.
    fn rebuild_word_index(&mut self) {
        // Collect postings in a hash map, then build the ordered map from it in
        // one go, which is cheaper than inserting each word into it in turn.
        let mut postings_by_word: FxHashMap<SmolStr, Vec<u32>> = FxHashMap::default();
        let mut artist_words: FxHashMap<&str, SmallVec<[SmolStr; 4]>> = FxHashMap::default();
        let mut album_words: FxHashMap<&AlbumId, SmallVec<[SmolStr; 4]>> = FxHashMap::default();
        for (idx, track_id) in self.populated_track_ids.iter().enumerate() {
            let idx = idx as u32;
            let track = self.track_map.get(track_id).unwrap();
            let album = track.album_id.as_ref().and_then(|id| self.albums.get(id));
            let artist = track
                .artist
                .as_deref()
                .or(album.as_ref().map(|a| a.artist.as_str()));

            let mut add = |words: &[SmolStr]| {
                for word in words {
                    // Tracks are iterated in ascending order, so the posting
                    // list for a word grows monotonically. Checking `last()` is
                    // enough to avoid duplicates without a post-pass.
                    let postings = match postings_by_word.get_mut(word) {
                        Some(postings) => postings,
                        None => postings_by_word.entry(word.clone()).or_default(),
                    };
                    if postings.last() != Some(&idx) {
                        postings.push(idx);
                    }
                }
            };
            if let Some(artist) = artist {
                add(artist_words
                    .entry(artist)
                    .or_insert_with(|| index_words(artist)));
            }
            if let Some(album) = album {
                add(album_words
                    .entry(&album.id)
                    .or_insert_with(|| index_words(&album.name)));
            }
            add(&index_words(&track.title));
        }

        self.word_index = postings_by_word.into_iter().collect();
    }
}

/// Returns the distinct words that `s` is indexed under: the words of each of
/// its [normalized variants](normalize_variants).
fn index_words(s: &str) -> SmallVec<[SmolStr; 4]> {
    let mut words: SmallVec<[SmolStr; 4]> = SmallVec::new();
    for variant in normalize_variants(s) {
        for word in variant.split_whitespace() {
            if !words.iter().any(|w| w == word) {
                words.push(SmolStr::new(word));
            }
        }
    }
    words
}

/// Maps typographic Unicode characters to their ASCII equivalents.
///
/// These characters — curly quotes, en/em dashes, ellipsis, non-breaking and
/// other narrow spaces, full-width Latin letters — have no NFKD compatibility
/// decomposition, so they survive [`fold_diacritics`] unchanged and break
/// matching. Mapping them to ASCII before the NFKD fold lets the downstream
/// punctuation handling treat them uniformly.
fn fold_lookalikes(s: &str) -> String {
    s.chars()
        .map(|c| {
            // Single curly quotes: ' ' ‚ ‛ -> '
            // Double curly quotes: " " „ ‟ -> "
            // Dashes: – — ― -> -
            // Ellipsis: … -> . (collapsed to ASCII punctuation, then stripped/spaced)
            // Various Unicode spaces -> ASCII space
            match c {
                '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' => '\'',
                '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' => '"',
                '\u{2013}' | '\u{2014}' | '\u{2015}' => '-',
                '\u{2026}' => '.',
                // Non-breaking space + general punctuation spaces (U+2000–U+200A)
                // + narrow no-break space (U+202F) + medium mathematical space (U+205F).
                '\u{00A0}' | '\u{2000}'..='\u{200A}' | '\u{202F}' | '\u{205F}' => ' ',
                // Full-width Latin letters and ASCII punctuation (U+FF01–U+FF5E)
                // map to their ASCII counterparts by subtracting the full-width offset.
                '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
                _ => c,
            }
        })
        .collect()
}

/// Folds diacritics so that e.g. `"Röyksopp"` becomes `"Royksopp"`.
///
/// Uses NFKD decomposition (breaking precomposed accented characters into a
/// base plus combining marks) and then strips anything with a non-zero
/// canonical combining class. Non-Latin scripts without combining marks
/// (Cyrillic, CJK, etc.) pass through unchanged.
///
/// This mirrors the "primary strength" collation that `blackbird-state` uses
/// for sorting — the goal is to treat `"é"` and `"e"` as the same character
/// for search, just as they compare equal for sort ordering.
fn fold_diacritics(s: &str) -> String {
    let nfkd = DecomposingNormalizer::new_nfkd();
    let ccc = CodePointMapData::<CanonicalCombiningClass>::new();
    nfkd.normalize_iter(s.chars())
        .filter(|c| ccc.get(*c) == CanonicalCombiningClass::NotReordered)
        .collect()
}

/// Returns deduplicated normalized variants of `s` for indexing or querying.
///
/// The input is first passed through [`fold_lookalikes`] (mapping typographic
/// characters to ASCII), then folded via [`fold_diacritics`], then up to two
/// further forms are emitted:
/// - `stripped`: lowercase, with ASCII punctuation removed outright.
/// - `spaced`: lowercase, with ASCII punctuation replaced by spaces and runs
///   of whitespace collapsed.
///
/// The two forms coincide when the folded input contains no punctuation (or
/// when punctuation is already adjacent to whitespace), so in the common case
/// only one variant is returned. Indexing and querying both apply this
/// function, so e.g. a query of `"ac dc"` finds a track titled `"AC/DC"` via
/// the spaced variant, while `"acdc"` finds it via the stripped variant, and
/// `"royksopp"` finds `"Röyksopp"` via the fold, and `"i'm"` finds `"i'm"`
/// (U+2019 right single quotation mark) via the lookalike fold.
fn normalize_variants(s: &str) -> SmallVec<[SmolStr; 2]> {
    // Both folds only change non-ASCII characters, so ASCII input (the common
    // case) can skip them.
    let folded = if s.is_ascii() {
        Cow::Borrowed(s)
    } else {
        Cow::Owned(fold_diacritics(&fold_lookalikes(s)))
    };

    let stripped: String = folded
        .chars()
        .filter(|c| !c.is_ascii_punctuation())
        .flat_map(|c| c.to_lowercase())
        .collect();

    let spaced_raw: String = folded
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

    fn track(id: &str, album_id: &str) -> Track {
        Track {
            id: TrackId(id.into()),
            title: SmolStr::new(format!("Track {id}")),
            artist: None,
            track: None,
            year: None,
            _genre: None,
            duration: Some(180),
            disc_number: None,
            album_id: Some(AlbumId(album_id.into())),
            starred: false,
            play_count: Some(1),
            replay_gain: None,
        }
    }

    fn album(id: &str) -> Album {
        Album {
            id: AlbumId(id.into()),
            name: SmolStr::new(format!("Album {id}")),
            artist: "Artist".into(),
            artist_id: None,
            cover_art_id: None,
            track_count: 1,
            duration: 180,
            year: None,
            _genre: None,
            starred: false,
            created: SmolStr::default(),
        }
    }

    /// Builds the output of a fetch containing the given `(track, album)` pairs.
    fn fetched(tracks: &[(&str, &str)]) -> FetchAllOutput {
        let mut raw = blackbird_state::RawLibrary::default();
        for (track_id, album_id) in tracks {
            raw.albums
                .insert(AlbumId((*album_id).into()), album(album_id));
            raw.tracks
                .insert(TrackId((*track_id).into()), track(track_id, album_id));
        }
        raw.build()
    }

    fn tid(id: &str) -> TrackId {
        TrackId(id.into())
    }

    /// `rebuild_word_index` normalizes a track's artist, album, and title
    /// separately; this checks that it yields the same words as normalizing
    /// them joined together.
    #[test]
    fn index_words_of_parts_match_index_words_of_whole() {
        let cases = [
            ("AC/DC", "Back in Black", "Hells Bells"),
            ("Röyksopp", "Melody A.M.", "Eple"),
            ("Sigur Rós", "( )", "Untitled #1"),
            ("Mötley Crüe", "Dr. Feelgood", "Kickstart My Heart"),
            (
                "The Beatles",
                "Sgt. Pepper's Lonely Hearts Club Band",
                "A Day in the Life",
            ),
            (
                "Guns N\u{2019} Roses",
                "Use Your Illusion I",
                "Don\u{2019}t Cry",
            ),
            (
                "\u{FF21}\u{FF23}/\u{FF24}\u{FF23}",
                "rock\u{2013}n\u{2013}roll",
                "one\u{2026}two",
            ),
            ("Café\u{00A0}Tacvba", "Re", "La Ingrata"),
            ("坂本龍一", "音楽図鑑", "Tibetan Dance"),
            ("  spaced   out  ", "-dash-", "...dots..."),
        ];
        for (artist, album, title) in cases {
            let whole: BTreeSet<SmolStr> = index_words(&format!("{artist} {album} {title}"))
                .into_iter()
                .collect();
            let parts: BTreeSet<SmolStr> = [artist, album, title]
                .into_iter()
                .flat_map(index_words)
                .collect();
            assert_eq!(parts, whole, "for {artist:?} / {album:?} / {title:?}");
        }
    }

    #[test]
    fn search_finds_tracks_by_artist_album_and_title() {
        let mut library = Library::default();
        let mut output = fetched(&[("t1", "a1"), ("t2", "a2")]);
        output.track_map.get_mut(&tid("t2")).unwrap().artist = Some("Röyksopp".into());
        library.replace(output, SortOrder::Alphabetical, None);

        assert_eq!(library.search("royksopp"), vec![tid("t2")]);
        assert_eq!(library.search("album a1"), vec![tid("t1")]);
        assert_eq!(library.search("track t2"), vec![tid("t2")]);
        // Album artist, used for tracks without their own artist.
        assert_eq!(library.search("artist"), vec![tid("t1")]);
    }

    #[test]
    fn resort_orders_groups_for_each_sort_order() {
        // (album id, artist, year, created, play count of its one track)
        let specs = [
            ("g1", "beta", Some(2001), "2024-03", Some(10)),
            ("g2", "Alpha", Some(1999), "2024-01", Some(20)),
            ("g3", "alpha", None, "2024-02", None),
            ("g4", "Alpha", Some(1999), "2024-04", Some(5)),
        ];
        let mut track_map = FxHashMap::default();
        let mut albums = FxHashMap::default();
        let mut groups = vec![];
        for (id, artist, year, created, play_count) in specs {
            let album_id = AlbumId(id.into());
            let track_id = tid(&format!("t-{id}"));
            track_map.insert(
                track_id.clone(),
                Track {
                    play_count,
                    ..track(&track_id.0, id)
                },
            );
            albums.insert(
                album_id.clone(),
                Album {
                    created: created.into(),
                    ..album(id)
                },
            );
            groups.push(Arc::new(Group {
                album_id,
                // Album names order g4 before g2 within the same artist and year.
                album: match id {
                    "g2" => "y",
                    "g4" => "b",
                    _ => "a",
                }
                .into(),
                artist: artist.into(),
                sort_artist: artist.to_lowercase().into(),
                year,
                duration: 0,
                tracks: vec![track_id],
                cover_art_id: None,
                starred: false,
            }));
        }

        let mut library = Library::default();
        library.populate(track_map, groups, albums, SortOrder::Alphabetical);
        let order = |library: &Library| -> Vec<String> {
            library
                .groups
                .iter()
                .map(|g| g.album_id.to_string())
                .collect()
        };

        // Artist (case-insensitive), then year ascending with no year last, then album.
        assert_eq!(order(&library), ["g4", "g2", "g3", "g1"]);
        // Year descending with no year last, then artist, then album.
        library.resort(SortOrder::NewestFirst);
        assert_eq!(order(&library), ["g1", "g4", "g2", "g3"]);
        // Most recently created first.
        library.resort(SortOrder::RecentlyAdded);
        assert_eq!(order(&library), ["g4", "g1", "g3", "g2"]);
        // Highest average play count first, unplayed last.
        library.resort(SortOrder::MostPlayed);
        assert_eq!(order(&library), ["g2", "g1", "g4", "g3"]);

        // The derived structures follow the new order.
        let expected_track_ids: Vec<TrackId> = ["g2", "g1", "g4", "g3"]
            .map(|id| tid(&format!("t-{id}")))
            .into();
        assert_eq!(library.track_ids, expected_track_ids);
        assert_eq!(library.group_index_of(&tid("t-g4")).unwrap(), 2);
        assert_eq!(library.album_to_group_index[&AlbumId("g1".into())], 1);
    }

    #[test]
    fn search_results_follow_the_current_sort_order() {
        let mut output = fetched(&[("t1", "a1"), ("t2", "a2"), ("t3", "a3")]);
        let mut albums = std::mem::take(&mut output.albums);
        for (id, created) in [("a1", "2024-01"), ("a2", "2024-03"), ("a3", "2024-02")] {
            albums.get_mut(&AlbumId(id.into())).unwrap().created = created.into();
        }
        output.albums = albums;
        let mut library = Library::default();
        library.replace(output, SortOrder::Alphabetical, None);
        assert_eq!(
            library.search("track"),
            vec![tid("t1"), tid("t2"), tid("t3")]
        );

        library.resort(SortOrder::RecentlyAdded);
        assert_eq!(
            library.search("track"),
            vec![tid("t2"), tid("t3"), tid("t1")]
        );
        assert_eq!(library.group_index_of(&tid("t1")), Some(2));
        assert_eq!(library.track_ids, vec![tid("t2"), tid("t3"), tid("t1")]);
    }

    #[test]
    fn replace_keeps_local_edits_made_during_the_refresh() {
        let mut library = Library::default();
        library.replace(
            fetched(&[("t1", "a1"), ("t2", "a2")]),
            SortOrder::Alphabetical,
            None,
        );

        library.begin_refresh();
        library.set_track_starred(&tid("t1"), true);
        library.update_track(Track {
            play_count: Some(5),
            ..track("t2", "a2")
        });
        library.set_album_starred(&AlbumId("a2".into()), true);

        // The refresh read the server before the edits landed.
        library.replace(
            fetched(&[("t1", "a1"), ("t2", "a2")]),
            SortOrder::Alphabetical,
            None,
        );

        assert!(library.track_map[&tid("t1")].starred);
        assert_eq!(library.track_map[&tid("t2")].play_count, Some(5));
        assert!(library.albums[&AlbumId("a2".into())].starred);
        let group = &library.groups[library.album_to_group_index[&AlbumId("a2".into())]];
        assert!(group.starred);
    }

    #[test]
    fn replace_takes_fresh_play_counts_for_tracks_only_starred_during_the_refresh() {
        let mut library = Library::default();
        library.replace(fetched(&[("t1", "a1")]), SortOrder::Alphabetical, None);

        library.begin_refresh();
        library.set_track_starred(&tid("t1"), true);
        let mut refreshed = fetched(&[("t1", "a1")]);
        refreshed.track_map.get_mut(&tid("t1")).unwrap().play_count = Some(42);
        library.replace(refreshed, SortOrder::Alphabetical, None);

        let track = &library.track_map[&tid("t1")];
        assert!(track.starred);
        assert_eq!(track.play_count, Some(42));
    }

    #[test]
    fn replace_takes_server_values_for_edits_before_the_refresh() {
        let mut library = Library::default();
        library.replace(fetched(&[("t1", "a1")]), SortOrder::Alphabetical, None);
        library.set_track_starred(&tid("t1"), true);

        // The edit predates this refresh, so the server's value wins (e.g. the
        // track was unstarred from another client since).
        library.begin_refresh();
        library.replace(fetched(&[("t1", "a1")]), SortOrder::Alphabetical, None);

        assert!(!library.track_map[&tid("t1")].starred);
    }

    #[test]
    fn replace_keeps_the_playing_track_available_after_removal() {
        let mut library = Library::default();
        library.replace(
            fetched(&[("t1", "a1"), ("t2", "a2")]),
            SortOrder::Alphabetical,
            None,
        );

        library.begin_refresh();
        library.replace(
            fetched(&[("t1", "a1")]),
            SortOrder::Alphabetical,
            Some(&tid("t2")),
        );

        // Still resolvable for the now-playing display...
        assert!(library.track_map.contains_key(&tid("t2")));
        assert!(library.albums.contains_key(&AlbumId("a2".into())));
        // ...but no longer part of the library.
        assert_eq!(library.track_ids, vec![tid("t1")]);
        assert!(library.group_index_of(&tid("t2")).is_none());
    }

    #[test]
    fn replace_drops_removed_tracks_that_are_not_playing() {
        let mut library = Library::default();
        library.replace(
            fetched(&[("t1", "a1"), ("t2", "a2")]),
            SortOrder::Alphabetical,
            None,
        );
        assert_eq!(library.search("track"), vec![tid("t1"), tid("t2")]);

        library.begin_refresh();
        library.replace(
            fetched(&[("t1", "a1")]),
            SortOrder::Alphabetical,
            Some(&tid("t1")),
        );

        assert!(!library.track_map.contains_key(&tid("t2")));
        // Cached search results from the old library are discarded.
        assert_eq!(library.search("track"), vec![tid("t1")]);
    }

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

    #[test]
    fn normalize_variants_folds_diacritics() {
        assert_eq!(variants("Röyksopp"), vec!["royksopp"]);
        assert_eq!(variants("Sigur Rós"), vec!["sigur ros"]);
        assert_eq!(variants("Café del Mar"), vec!["cafe del mar"]);
        assert_eq!(variants("Mötley Crüe"), vec!["motley crue"]);
    }

    #[test]
    fn normalize_variants_folds_lookalikes() {
        // Curly apostrophe (U+2019) becomes ASCII apostrophe.
        assert_eq!(
            variants("i'm the president"),
            vec!["im the president", "i m the president"]
        );
        // Curly double quotes (U+201C/U+201D).
        assert_eq!(variants("\u{201C}hello\u{201D}"), vec!["hello"]);
        // En dash (U+2013) and em dash (U+2014) become ASCII hyphen.
        assert_eq!(
            variants("rock\u{2013}n\u{2013}roll"),
            vec!["rocknroll", "rock n roll"]
        );
        assert_eq!(variants("AC\u{2014}DC"), vec!["acdc", "ac dc"]);
        // Ellipsis (U+2026) becomes ASCII period, then is stripped/spaced.
        assert_eq!(variants("one\u{2026}two"), vec!["onetwo", "one two"]);
        // Non-breaking space (U+00A0) becomes ASCII space.
        assert_eq!(variants("hello\u{00A0}world"), vec!["hello world"]);
        // Full-width Latin (U+FF21 'Ａ' -> 'A').
        assert_eq!(
            variants("\u{FF21}\u{FF23}/\u{FF24}\u{FF23}"),
            vec!["acdc", "ac dc"]
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
        let mut track_map: FxHashMap<TrackId, Track> = FxHashMap::default();
        let mut albums: FxHashMap<AlbumId, Album> = FxHashMap::default();
        let mut group_tracks: FxHashMap<AlbumId, Vec<TrackId>> = FxHashMap::default();

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
                    replay_gain: None,
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
        library.populate(track_map, groups, albums, SortOrder::Alphabetical);
        library
    }

    fn search_ids(library: &mut Library, query: &str) -> Vec<String> {
        library
            .search(query)
            .into_iter()
            .map(|id| id.0.to_string())
            .collect()
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
    fn search_folds_diacritics_both_sides() {
        let mut lib = build_library(&[
            ("t1", "Eple", "Röyksopp", "a1", "Melody A.M."),
            ("t2", "Starálfur", "Sigur Rós", "a2", "Takk..."),
        ]);

        // Indexed with diacritics, queried without: the motivating case.
        assert_eq!(search_ids(&mut lib, "royksopp"), vec!["t1"]);
        // Indexed with diacritics, queried with diacritics: also works.
        assert_eq!(search_ids(&mut lib, "Röyksopp"), vec!["t1"]);
        // Works across tokens: "Sigur Rós" is indexed with the fold applied.
        assert_eq!(search_ids(&mut lib, "sigur ros"), vec!["t2"]);
        // And so is the track title ("Starálfur" -> "staralfur").
        assert_eq!(search_ids(&mut lib, "staralfur"), vec!["t2"]);
    }

    #[test]
    fn search_folds_unicode_lookalikes() {
        let mut lib = build_library(&[
            ("t1", "i\u{2019}m the president", "Artist", "a1", "Album"),
            ("t2", "rock\u{2014}n\u{2014}roll", "Band", "a2", "Album Two"),
        ]);

        // Curly apostrophe in the title, ASCII apostrophe in the query.
        assert_eq!(search_ids(&mut lib, "i'm the president"), vec!["t1"]);
        // And the reverse: ASCII in title is not present here, but curly
        // apostrophe in the query should also find the curly-apostrophe title.
        assert_eq!(search_ids(&mut lib, "i\u{2019}m the president"), vec!["t1"]);
        // Em dash (U+2014) in the title, ASCII hyphen in the query.
        assert_eq!(search_ids(&mut lib, "rock-n-roll"), vec!["t2"]);
        // Em dash in the query as well.
        assert_eq!(
            search_ids(&mut lib, "rock\u{2014}n\u{2014}roll"),
            vec!["t2"]
        );
    }

    #[test]
    fn search_returns_empty_for_no_match() {
        let mut lib = build_library(&[("t1", "Hello World", "Artist", "a1", "Album")]);
        assert!(search_ids(&mut lib, "xyz").is_empty());
    }
}
