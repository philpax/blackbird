//! Representations of blackbird's state, as well as a way to retrieve it from an OpenSubsonic server.
//!
//! Separated out to allow for use in other utilities.
#![deny(missing_docs)]

use std::{collections::BTreeMap, sync::Arc};

pub use blackbird_subsonic as bs;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use smol_str::{SmolStr, format_smolstr};

mod album;
pub use album::{Album, AlbumId};

mod artist;
pub use artist::ArtistId;

mod cover_art;
pub use cover_art::CoverArtId;

mod group;
pub use group::Group;

mod track;
pub use track::{Track, TrackId};

/// Creates a collator configured for sorting artist and album names.
///
/// The collator is configured with:
/// - Primary strength (ignores case and diacritics)
/// - Numeric ordering enabled
/// - Case level off
///
/// This means "E" and "È" will compare as equal, and "Track 2" will sort before "Track 10".
pub fn create_collator() -> icu_collator::CollatorBorrowed<'static> {
    let mut collator_preferences = icu_collator::CollatorPreferences::default();
    collator_preferences.numeric_ordering =
        Some(icu_collator::preferences::CollationNumericOrdering::True);

    let mut collator_options = icu_collator::options::CollatorOptions::default();
    collator_options.strength = Some(icu_collator::options::Strength::Primary);
    collator_options.case_level = Some(icu_collator::options::CaseLevel::Off);

    icu_collator::Collator::try_new(collator_preferences, collator_options).unwrap()
}

/// The alphabetical order of groups: by artist sort name, then year (oldest
/// first, with groups without a year last), then album name.
///
/// Names are compared with the collator from [`create_collator`], so case and
/// diacritics are ignored and numbers are ordered numerically. Various Artists
/// groups ignore the year and are ordered by album name alone, as there's no
/// connecting tissue between them.
pub struct AlphabeticalOrder {
    collator: icu_collator::CollatorBorrowed<'static>,
}
impl Default for AlphabeticalOrder {
    fn default() -> Self {
        Self::new()
    }
}
impl AlphabeticalOrder {
    /// Creates the ordering.
    pub fn new() -> Self {
        Self {
            collator: create_collator(),
        }
    }

    /// Compares two groups. Groups that are otherwise equal are ordered by
    /// album ID, so that the order is deterministic.
    pub fn compare(&self, a: &Group, b: &Group) -> std::cmp::Ordering {
        self.collator
            .compare(&a.sort_artist, &b.sort_artist)
            .then_with(|| {
                // Both or neither are Various Artists, as their artists
                // collate equally, and Various Artists is detected by
                // collation too.
                if self.is_various_artists(a) {
                    std::cmp::Ordering::Equal
                } else {
                    compare_years(a.year, b.year)
                }
            })
            .then_with(|| self.collator.compare(&a.album, &b.album))
            .then_with(|| a.album_id.cmp(&b.album_id))
    }

    /// Whether the group is by Various Artists. This must use the collator,
    /// as a plain string comparison could disagree about artists that collate
    /// equally (e.g. differing only in accents), which would make
    /// [`Self::compare`] inconsistent.
    fn is_various_artists(&self, group: &Group) -> bool {
        self.collator.compare(&group.sort_artist, "various artists") == std::cmp::Ordering::Equal
    }
}

/// Compares years in ascending order, with a missing year last.
fn compare_years(a: Option<i32>, b: Option<i32>) -> std::cmp::Ordering {
    match (a, b) {
        (Some(a), Some(b)) => a.cmp(&b),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

/// The library data as fetched from the server, before any derived structures
/// (sorting and grouping) are built from it.
///
/// This is the unit that is persisted to the library cache. The maps are
/// ordered so that serializing the same data always produces the same bytes,
/// which lets a fresh fetch be compared against the cache cheaply.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RawLibrary {
    /// The albums that were fetched.
    pub albums: BTreeMap<AlbumId, Album>,
    /// The tracks that were fetched.
    pub tracks: BTreeMap<TrackId, Track>,
    /// The sort names of the artists that have one; these are the only artist
    /// data used when building the library.
    pub artist_sort_names: BTreeMap<ArtistId, SmolStr>,
}

/// The output of [`fetch_all`] and [`RawLibrary::build`].
pub struct FetchAllOutput {
    /// The albums that were fetched.
    pub albums: FxHashMap<AlbumId, Album>,
    /// The tracks that were fetched.
    pub track_map: FxHashMap<TrackId, Track>,
    /// The sorted track IDs.
    pub track_ids: Vec<TrackId>,
    /// The groups that were constructed.
    pub groups: Vec<Arc<Group>>,
    /// Tracks that were dropped because they have no album, or their album
    /// wasn't fetched (e.g. because a server scan ran during the fetch).
    pub orphaned_track_ids: Vec<TrackId>,
}

/// Fetches all albums and tracks from the server, and constructs groups.
///
/// `on_tracks_fetched` is called with the number of tracks that were just fetched,
/// as well as the total number of tracks fetched so far.
pub async fn fetch_all(
    client: &bs::Client,
    on_tracks_fetched: impl Fn(u32, u32),
) -> bs::ClientResult<FetchAllOutput> {
    Ok(fetch_raw(client, on_tracks_fetched).await?.build())
}

/// Fetches all albums, tracks, and artists from the server, without building
/// any derived structures.
///
/// `on_tracks_fetched` is called with the number of tracks that were just fetched,
/// as well as the total number of tracks fetched so far.
pub async fn fetch_raw(
    client: &bs::Client,
    on_tracks_fetched: impl Fn(u32, u32),
) -> bs::ClientResult<RawLibrary> {
    // The three walks are independent, so run them concurrently; the total
    // time is then bounded by the slowest walk rather than their sum.
    let (albums, tracks, artist_sort_names) = futures::try_join!(
        Album::fetch_all(client),
        fetch_all_tracks(client, on_tracks_fetched),
        fetch_all_artist_sort_names(client),
    )?;
    Ok(RawLibrary {
        albums: albums.into_iter().map(|a| (a.id.clone(), a)).collect(),
        tracks,
        artist_sort_names,
    })
}

impl RawLibrary {
    /// Sorts the tracks and groups them into albums.
    pub fn build(self) -> FetchAllOutput {
        let RawLibrary {
            albums,
            tracks,
            artist_sort_names,
        } = self;
        let albums: FxHashMap<AlbumId, Album> = albums.into_iter().collect();

        let mut orphaned_track_ids = vec![];
        let tracks: FxHashMap<TrackId, Track> = tracks
            .into_iter()
            .filter(|(id, track)| {
                let has_album = track
                    .album_id
                    .as_ref()
                    .is_some_and(|album_id| albums.contains_key(album_id));
                if !has_album {
                    orphaned_track_ids.push(id.clone());
                }
                has_album
            })
            .collect();

        build_library(albums, tracks, &artist_sort_names, orphaned_track_ids)
    }
}

/// Groups `tracks` into albums and sorts them alphabetically (see
/// [`AlphabeticalOrder`]). Every track must have an album in `albums`.
fn build_library(
    albums: FxHashMap<AlbumId, Album>,
    tracks: FxHashMap<TrackId, Track>,
    artists: &BTreeMap<ArtistId, SmolStr>,
    orphaned_track_ids: Vec<TrackId>,
) -> FetchAllOutput {
    // The artist sort name belongs to the album, but is needed for every
    // track, so compute it once per album.
    let album_sort_artists: FxHashMap<&AlbumId, SmolStr> = albums
        .iter()
        .map(|(id, album)| (id, normalized_artist_sort_name(album, artists)))
        .collect();
    let album_of = |track: &Track| {
        let album_id = track
            .album_id
            .as_ref()
            .unwrap_or_else(|| panic!("Album ID not found in track: {track:?}"));
        let album = albums
            .get(album_id)
            .unwrap_or_else(|| panic!("Album not found in album map: {album_id:?}"));
        (album, &album_sort_artists[album_id])
    };

    // Group tracks by (sort artist, album name, year). Albums that share
    // these (e.g. a release split across several album entries on the server)
    // form a single group.
    let mut group_indices: FxHashMap<(&str, &str, Option<i32>), usize> = FxHashMap::default();
    let mut group_members: Vec<Vec<&TrackId>> = vec![];
    for (track_id, track) in &tracks {
        let (album, sort_artist) = album_of(track);
        let index = *group_indices
            .entry((sort_artist.as_str(), album.name.as_str(), album.year))
            .or_insert_with(|| {
                group_members.push(vec![]);
                group_members.len() - 1
            });
        group_members[index].push(track_id);
    }

    let order = AlphabeticalOrder::new();
    let mut groups: Vec<Arc<Group>> = group_members
        .into_iter()
        .map(|mut members| {
            // Tracks that are otherwise equal are ordered by ID, so that the
            // order doesn't depend on `HashMap` iteration order.
            members.sort_unstable_by(|a, b| {
                let (track_a, track_b) = (&tracks[*a], &tracks[*b]);
                (track_a.disc_number.unwrap_or_default())
                    .cmp(&track_b.disc_number.unwrap_or_default())
                    .then_with(|| {
                        (track_a.track.unwrap_or_default()).cmp(&track_b.track.unwrap_or_default())
                    })
                    .then_with(|| order.collator.compare(&track_a.title, &track_b.title))
                    .then_with(|| a.cmp(b))
            });

            // The group takes its details from the album of its first track.
            let (album, sort_artist) = album_of(&tracks[members[0]]);
            Arc::new(Group {
                artist: album.artist.clone(),
                sort_artist: sort_artist.clone(),
                album: album.name.clone(),
                year: album.year,
                duration: album.duration,
                tracks: members.into_iter().cloned().collect(),
                cover_art_id: album.cover_art_id.clone(),
                album_id: album.id.clone(),
                starred: album.starred,
            })
        })
        .collect();
    groups.sort_by(|a, b| order.compare(a, b));

    let track_ids = groups
        .iter()
        .flat_map(|group| group.tracks.iter().cloned())
        .collect();

    FetchAllOutput {
        albums,
        track_map: tracks,
        track_ids,
        groups,
        orphaned_track_ids,
    }
}

fn normalized_artist_sort_name(album: &Album, artists: &BTreeMap<ArtistId, SmolStr>) -> SmolStr {
    let album_artist = album.artist.to_lowercase();
    album
        .artist_id
        .as_ref()
        .and_then(|id| {
            let raw_artist_sort_name = artists.get(id)?;
            Some(if album_artist.starts_with("the ") {
                format_smolstr!("the {raw_artist_sort_name}")
            } else if album_artist.starts_with("an ") {
                format_smolstr!("an {raw_artist_sort_name}")
            } else if album_artist.starts_with("a ") {
                format_smolstr!("a {raw_artist_sort_name}")
            } else if album_artist.starts_with("el ") {
                format_smolstr!("el {raw_artist_sort_name}")
            } else if album_artist.starts_with("los ") {
                format_smolstr!("los {raw_artist_sort_name}")
            } else if album_artist.starts_with("las ") {
                format_smolstr!("las {raw_artist_sort_name}")
            } else if album_artist.starts_with("les ") {
                format_smolstr!("les {raw_artist_sort_name}")
            } else {
                raw_artist_sort_name.clone()
            })
        })
        .unwrap_or_else(|| album_artist.into())
}

/// Fetches all tracks from the server by paging through an empty `search3` query.
async fn fetch_all_tracks(
    client: &bs::Client,
    on_tracks_fetched: impl Fn(u32, u32),
) -> bs::ClientResult<BTreeMap<TrackId, Track>> {
    let mut offset = 0;
    let mut tracks = BTreeMap::new();
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
            .await?;

        if response.song.is_empty() {
            break;
        }

        let track_count = response.song.len();
        tracks.extend(
            response
                .song
                .into_iter()
                .map(|s| (TrackId(s.id.as_str().into()), Track::from(s))),
        );
        offset += track_count as u32;
        on_tracks_fetched(track_count as u32, offset);
    }
    Ok(tracks)
}

/// Fetches the sort names of all artists from the server by paging through an
/// empty `search3` query. Artists without a sort name are omitted.
async fn fetch_all_artist_sort_names(
    client: &bs::Client,
) -> bs::ClientResult<BTreeMap<ArtistId, SmolStr>> {
    let mut offset = 0;
    let mut artists = BTreeMap::new();
    loop {
        let response = client
            .search3(&bs::Search3Request {
                query: "".to_string(),
                artist_count: Some(10000),
                artist_offset: Some(offset),
                ..Default::default()
            })
            .await?;

        if response.artist.is_empty() {
            break;
        }

        let artist_count = response.artist.len();
        artists.extend(response.artist.into_iter().filter_map(|a| {
            let sort_name = a.sort_name?;
            Some((ArtistId(a.id.into()), SmolStr::from(sort_name)))
        }));

        offset += artist_count as u32;
    }
    Ok(artists)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn album(id: &str, name: &str) -> Album {
        Album {
            id: AlbumId(id.into()),
            name: name.into(),
            artist: "Artist".into(),
            artist_id: None,
            cover_art_id: None,
            track_count: 1,
            duration: 180,
            year: Some(2001),
            _genre: None,
            starred: false,
            created: "2024-01-01T00:00:00Z".into(),
        }
    }

    fn track(id: &str, album_id: Option<&str>, number: u32) -> Track {
        Track {
            id: TrackId(id.into()),
            title: format!("Track {id}").into(),
            artist: None,
            track: Some(number),
            year: None,
            _genre: None,
            duration: Some(180),
            disc_number: None,
            album_id: album_id.map(|id| AlbumId(id.into())),
            starred: false,
            play_count: None,
            replay_gain: None,
        }
    }

    fn group(album_id: &str, sort_artist: &str, album: &str, year: Option<i32>) -> Group {
        Group {
            artist: sort_artist.into(),
            sort_artist: sort_artist.into(),
            album: album.into(),
            year,
            duration: 0,
            tracks: vec![],
            cover_art_id: None,
            album_id: AlbumId(album_id.into()),
            starred: false,
        }
    }

    fn alphabetical(groups: &[Group]) -> Vec<&str> {
        let order = AlphabeticalOrder::new();
        let mut sorted: Vec<&Group> = groups.iter().collect();
        sorted.sort_by(|a, b| order.compare(a, b));
        sorted.iter().map(|g| g.album_id.0.as_str()).collect()
    }

    #[test]
    fn alphabetical_order_collates_sort_names() {
        let groups = [
            group("zappa", "zappa", "Hot Rats", Some(1969)),
            // Accents and case are ignored.
            group("emilie", "Émilie Simon", "Émilie Simon", Some(2003)),
            group("eno", "brian eno", "Another Green World", Some(1975)),
            // A collaboration sorts with its primary artist's sort name.
            group(
                "eno-collab",
                "brian eno",
                "Drums Between the Bells",
                Some(2011),
            ),
            group("abba", "ABBA", "Arrival", Some(1976)),
        ];
        assert_eq!(
            alphabetical(&groups),
            ["abba", "eno", "eno-collab", "emilie", "zappa"]
        );
    }

    #[test]
    fn alphabetical_order_sorts_years_ascending_with_missing_years_last() {
        let groups = [
            group("undated", "artist", "A", None),
            group("new", "artist", "B", Some(2020)),
            group("old", "artist", "C", Some(1990)),
        ];
        assert_eq!(alphabetical(&groups), ["old", "new", "undated"]);
    }

    #[test]
    fn alphabetical_order_orders_numbers_numerically() {
        let groups = [
            group("vol10", "artist", "Vol. 10", Some(2000)),
            group("vol2", "artist", "Vol. 2", Some(2000)),
            group("vol1", "artist", "Vol. 1", Some(2000)),
        ];
        assert_eq!(alphabetical(&groups), ["vol1", "vol2", "vol10"]);
    }

    #[test]
    fn alphabetical_order_ignores_years_for_various_artists() {
        let groups = [
            group("z-2001", "various artists", "Zeta", Some(2001)),
            group("a-2020", "Various Artists", "Alpha", Some(2020)),
            group("m-1990", "various artists", "Mu", Some(1990)),
        ];
        assert_eq!(alphabetical(&groups), ["a-2020", "m-1990", "z-2001"]);
    }

    #[test]
    fn alphabetical_order_detects_various_artists_by_collation() {
        let groups = [
            group("z-2001", "V\u{00E1}rious Artists", "Zeta", Some(2001)),
            group("a-2020", "\u{FF56}arious artists", "Alpha", Some(2020)),
            group("m-1990", "various artists", "Mu", Some(1990)),
        ];
        assert_eq!(alphabetical(&groups), ["a-2020", "m-1990", "z-2001"]);
    }

    proptest! {
        /// Sorting requires a total order; an inconsistent comparator makes
        /// the standard library's sort panic.
        #[test]
        fn alphabetical_order_is_a_total_order(
            groups in proptest::collection::vec(arbitrary_group(), 0..40)
        ) {
            let order = AlphabeticalOrder::new();
            for a in &groups {
                prop_assert_eq!(order.compare(a, a), std::cmp::Ordering::Equal);
                for b in &groups {
                    let ab = order.compare(a, b);
                    prop_assert_eq!(ab, order.compare(b, a).reverse());
                    for c in &groups {
                        if ab != std::cmp::Ordering::Greater
                            && order.compare(b, c) != std::cmp::Ordering::Greater
                        {
                            prop_assert_ne!(order.compare(a, c), std::cmp::Ordering::Greater);
                        }
                    }
                }
            }
            let mut sorted = groups.clone();
            sorted.sort_by(|a, b| order.compare(a, b));
        }
    }

    /// Generates groups from a small pool of names, so that collisions (and
    /// collation-equal variants of Various Artists) are common.
    fn arbitrary_group() -> impl Strategy<Value = Group> {
        let artists = prop::sample::select(vec![
            "various artists",
            "Various Artists",
            "V\u{00E1}rious Artists",
            "\u{FF56}arious artists",
            "abba",
            "ABBA",
            "\u{00C9}milie",
            "emilie",
        ]);
        let albums =
            prop::sample::select(vec!["Alpha", "alpha", "Vol. 2", "Vol. 10", "\u{00C1}lpha"]);
        let years = prop::option::of(1990..1993i32);
        let ids = prop::sample::select(vec!["a", "b", "c"]);
        (artists, albums, years, ids)
            .prop_map(|(artist, album, year, id)| group(id, artist, album, year))
    }

    #[test]
    fn build_merges_albums_that_share_a_group_and_orders_their_tracks() {
        let mut raw = RawLibrary::default();
        // The same release, split across two album entries on the server.
        raw.albums
            .insert(AlbumId("a1".into()), album("a1", "Album"));
        raw.albums
            .insert(AlbumId("a2".into()), album("a2", "Album"));
        raw.albums.insert(AlbumId("b".into()), album("b", "Before"));
        let mut disc_two = track("d2t1", Some("a2"), 1);
        disc_two.disc_number = Some(2);
        let mut disc_one = track("d1t2", Some("a1"), 2);
        disc_one.disc_number = Some(1);
        let mut disc_one_first = track("d1t1", Some("a1"), 1);
        disc_one_first.disc_number = Some(1);
        for track in [
            disc_two,
            disc_one,
            disc_one_first,
            track("b1", Some("b"), 1),
        ] {
            raw.tracks.insert(track.id.clone(), track);
        }

        let output = raw.build();
        let groups: Vec<Vec<&str>> = output
            .groups
            .iter()
            .map(|g| g.tracks.iter().map(|t| t.0.as_str()).collect())
            .collect();
        assert_eq!(groups, [vec!["d1t1", "d1t2", "d2t1"], vec!["b1"]]);
        let flat: Vec<&str> = output.track_ids.iter().map(|t| t.0.as_str()).collect();
        assert_eq!(flat, ["d1t1", "d1t2", "d2t1", "b1"]);
    }

    #[test]
    fn build_drops_tracks_without_a_known_album() {
        let mut raw = RawLibrary::default();
        raw.albums
            .insert(AlbumId("a1".into()), album("a1", "Album"));
        for track in [
            track("t1", Some("a1"), 2),
            track("t2", Some("a1"), 1),
            // The album was added by a scan that ran mid-fetch.
            track("t3", Some("a2"), 1),
            track("t4", None, 1),
        ] {
            raw.tracks.insert(track.id.clone(), track);
        }

        let output = raw.build();
        assert_eq!(
            output.track_ids,
            vec![TrackId("t2".into()), TrackId("t1".into())]
        );
        assert_eq!(output.track_map.len(), 2);
        assert_eq!(
            output.orphaned_track_ids,
            vec![TrackId("t3".into()), TrackId("t4".into())]
        );
        assert_eq!(output.groups.len(), 1);
        assert_eq!(output.groups[0].tracks, output.track_ids);
    }
}
