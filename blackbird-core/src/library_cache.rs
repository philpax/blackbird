//! An on-disk cache of the library, so that it can be shown immediately on
//! startup instead of after the full walk of the server's library.
//!
//! The cache holds a [`RawLibrary`] (the data before sorting and grouping) as
//! JSON, one file per server and user. It is only ever a starting point: the
//! library is always re-fetched in the background, and the cache is replaced
//! whenever the fetched data differs from it.
//!
//! The format version is part of the file name, so a build with a different
//! format simply doesn't see files written by another build. Any file that
//! fails to decode is treated as a cache miss.
use std::{
    io::Write as _,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use blackbird_state::RawLibrary;

/// The version of the cache format. Bump this whenever [`RawLibrary`] or any
/// of the types it contains changes in a way that isn't backwards-compatible
/// with serde's defaults (e.g. a renamed field, or a new non-`Option` field).
const FORMAT_VERSION: u32 = 1;

/// The location of the cache for a particular server and user.
#[derive(Debug, Clone)]
pub(crate) struct LibraryCache {
    path: PathBuf,
}

/// A library loaded from the cache.
pub(crate) struct CachedLibrary {
    pub library: RawLibrary,
    /// The encoded form of `library`, for comparison against fresh fetches.
    pub encoded: Vec<u8>,
}

impl LibraryCache {
    /// Returns the cache for the given server and user within `cache_dir`.
    pub fn new(cache_dir: &Path, base_url: &str, username: &str) -> Self {
        Self {
            path: cache_dir.join(file_name(base_url, username)),
        }
    }

    /// Loads the cached library. Returns `Ok(None)` if there is no cache.
    pub fn load(&self) -> Result<Option<CachedLibrary>, LibraryCacheError> {
        let encoded = match std::fs::read(&self.path) {
            Ok(encoded) => encoded,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(LibraryCacheError::Read {
                    path: self.path.clone(),
                    error,
                });
            }
        };
        let library =
            serde_json::from_slice(&encoded).map_err(|error| LibraryCacheError::Decode {
                path: self.path.clone(),
                error,
            })?;
        Ok(Some(CachedLibrary { library, encoded }))
    }

    /// Atomically replaces the cache with `encoded`, which must have been
    /// produced by [`encode`].
    pub fn store(&self, encoded: &[u8]) -> Result<(), LibraryCacheError> {
        let write_error = |error| LibraryCacheError::Write {
            path: self.path.clone(),
            error,
        };

        let dir = self.path.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(dir).map_err(|error| LibraryCacheError::CreateDir {
            path: dir.to_owned(),
            error,
        })?;

        // Write to a temporary file in the same directory and rename it over
        // the cache, so that a crash mid-write never leaves a truncated cache.
        // The temporary file is unique to this write, so that concurrent
        // writers (e.g. two instances of the app) can't interleave their
        // writes; the last rename wins.
        static WRITE_COUNTER: AtomicU64 = AtomicU64::new(0);
        let mut temp_path = self.path.clone().into_os_string();
        temp_path.push(format!(
            ".{}-{}.tmp",
            std::process::id(),
            WRITE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let temp_path = PathBuf::from(temp_path);
        let result = (|| {
            let mut file = std::fs::File::create(&temp_path)?;
            file.write_all(encoded)?;
            file.sync_all()?;
            std::fs::rename(&temp_path, &self.path)?;
            sync_dir(dir)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temp_path);
        }
        result.map_err(write_error)
    }
}

/// Makes a rename within `dir` durable. On Unix, a rename is only durable
/// once the directory itself is synced.
#[cfg(unix)]
fn sync_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::File::open(dir)?.sync_all()
}

/// Makes a rename within `dir` durable. Windows offers no way to sync a
/// directory; `MoveFileEx`-based renames are journaled by NTFS.
#[cfg(windows)]
fn sync_dir(_dir: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Encodes `library` in the cache format. The same library always encodes to
/// the same bytes.
pub(crate) fn encode(library: &RawLibrary) -> Vec<u8> {
    serde_json::to_vec(library).expect("serializing a RawLibrary to JSON cannot fail")
}

/// Returns the cache file name for a server and user.
///
/// The server URL and username are hashed rather than embedded so that the
/// name is always a valid file name. This is not meant to hide which servers
/// have been used; the hash is unsalted, and the contents aren't encrypted.
fn file_name(base_url: &str, username: &str) -> String {
    // FNV-1a, as its output is stable across Rust versions and platforms
    // (unlike `DefaultHasher`), which the file name needs to be.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in base_url
        .as_bytes()
        .iter()
        .chain(&[0])
        .chain(username.as_bytes())
    {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("library-v{FORMAT_VERSION}-{hash:016x}.json")
}

/// An error that occurred while reading or writing the library cache.
#[derive(Debug)]
pub(crate) enum LibraryCacheError {
    /// The directory for the cache file couldn't be created.
    CreateDir {
        path: PathBuf,
        error: std::io::Error,
    },
    /// The cache file couldn't be read.
    Read {
        path: PathBuf,
        error: std::io::Error,
    },
    /// The cache file was read, but its contents couldn't be decoded.
    Decode {
        path: PathBuf,
        error: serde_json::Error,
    },
    /// The cache file couldn't be written.
    Write {
        path: PathBuf,
        error: std::io::Error,
    },
}
impl std::fmt::Display for LibraryCacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LibraryCacheError::CreateDir { path, error } => {
                write!(
                    f,
                    "create library cache directory {}: {error}",
                    path.display()
                )
            }
            LibraryCacheError::Read { path, error } => {
                write!(f, "read library cache {}: {error}", path.display())
            }
            LibraryCacheError::Decode { path, error } => {
                write!(f, "decode library cache {}: {error}", path.display())
            }
            LibraryCacheError::Write { path, error } => {
                write!(f, "write library cache {}: {error}", path.display())
            }
        }
    }
}
impl std::error::Error for LibraryCacheError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LibraryCacheError::CreateDir { error, .. }
            | LibraryCacheError::Read { error, .. }
            | LibraryCacheError::Write { error, .. } => Some(error),
            LibraryCacheError::Decode { error, .. } => Some(error),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use blackbird_state::{Album, AlbumId, ArtistId, Track, TrackId, bs};
    use smol_str::SmolStr;

    use super::*;

    /// A fresh per-test directory, so that tests neither touch the user's real
    /// cache nor observe each other's writes. Removed when dropped.
    pub(crate) struct TestDir(PathBuf);
    impl TestDir {
        pub fn new(prefix: &str, test_name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "blackbird-{prefix}-test-{}-{test_name}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            Self(dir)
        }
    }
    impl std::ops::Deref for TestDir {
        type Target = Path;
        fn deref(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn test_dir(test_name: &str) -> TestDir {
        TestDir::new("library-cache", test_name)
    }

    fn album(id: &str) -> Album {
        Album {
            id: AlbumId(id.into()),
            name: format!("Album {id}").into(),
            artist: "Artist".into(),
            artist_id: Some(ArtistId("artist".into())),
            cover_art_id: None,
            track_count: 1,
            duration: 180,
            year: Some(2001),
            _genre: None,
            starred: false,
            created: "2024-01-01T00:00:00Z".into(),
        }
    }

    fn track(id: &str, album_id: &str) -> Track {
        Track {
            id: TrackId(id.into()),
            title: format!("Track {id}").into(),
            artist: None,
            track: Some(1),
            year: None,
            _genre: None,
            duration: Some(180),
            disc_number: None,
            album_id: Some(AlbumId(album_id.into())),
            starred: true,
            play_count: Some(3),
            // Only some fields set, to cover `skip_serializing_if` fields.
            replay_gain: Some(bs::ReplayGain {
                track_gain: Some(-6.5),
                ..Default::default()
            }),
        }
    }

    fn library(track_ids: &[&str]) -> RawLibrary {
        let mut library = RawLibrary::default();
        library.albums.insert(AlbumId("a1".into()), album("a1"));
        for id in track_ids {
            library
                .tracks
                .insert(TrackId((*id).into()), track(id, "a1"));
        }
        library
            .artist_sort_names
            .insert(ArtistId("artist".into()), SmolStr::new("artist, the"));
        library
    }

    #[test]
    fn stored_library_loads_back_identically() {
        let dir = test_dir("roundtrip");
        let cache = LibraryCache::new(&dir, "http://server", "user");
        let original = library(&["t1", "t2"]);
        let encoded = encode(&original);
        cache.store(&encoded).unwrap();

        let loaded = cache.load().unwrap().expect("the cache was just stored");
        assert_eq!(loaded.encoded, encoded);
        assert_eq!(encode(&loaded.library), encoded);

        let track = &loaded.library.tracks[&TrackId("t1".into())];
        assert_eq!(
            track.replay_gain,
            Some(bs::ReplayGain {
                track_gain: Some(-6.5),
                ..Default::default()
            })
        );
        assert_eq!(track.play_count, Some(3));
        assert!(track.starred);
    }

    #[test]
    fn missing_cache_is_not_an_error() {
        let dir = test_dir("missing");
        let cache = LibraryCache::new(&dir, "http://server", "user");
        assert!(cache.load().unwrap().is_none());
    }

    #[test]
    fn corrupt_cache_is_a_decode_error() {
        let dir = test_dir("corrupt");
        let cache = LibraryCache::new(&dir, "http://server", "user");
        cache.store(br#"{"albums": {"#).unwrap();
        assert!(matches!(
            cache.load(),
            Err(LibraryCacheError::Decode { .. })
        ));
    }

    #[test]
    fn store_replaces_the_cache_without_leaving_a_temporary_file() {
        let dir = test_dir("replace");
        let cache = LibraryCache::new(&dir, "http://server", "user");
        cache.store(&encode(&library(&["t1"]))).unwrap();
        let updated = encode(&library(&["t1", "t2"]));
        cache.store(&updated).unwrap();

        assert_eq!(cache.load().unwrap().unwrap().encoded, updated);
        let files: Vec<_> = std::fs::read_dir(&*dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(files.len(), 1, "unexpected files: {files:?}");
    }

    #[test]
    fn encoding_is_independent_of_insertion_order() {
        let forwards = library(&["t1", "t2", "t3"]);
        let backwards = library(&["t3", "t2", "t1"]);
        assert_eq!(encode(&forwards), encode(&backwards));
    }

    #[test]
    fn file_name_is_stable_and_distinguishes_servers_and_users() {
        // Pinned so that an accidental change to the naming scheme (which
        // would orphan every existing cache) is caught.
        assert_eq!(
            file_name("http://server", "user"),
            "library-v1-697d2fbbe1a2db7b.json"
        );
        assert_ne!(
            file_name("http://server", "user"),
            file_name("http://server", "other")
        );
        assert_ne!(
            file_name("http://server", "user"),
            file_name("http://other", "user")
        );
        // The separator keeps the URL and username from running together.
        assert_ne!(
            file_name("http://server", "user"),
            file_name("http://serveru", "ser")
        );
    }
}
