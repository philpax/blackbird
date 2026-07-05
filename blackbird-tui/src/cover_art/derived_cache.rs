//! The shared lifecycle for artifacts derived from cover art image data.

use std::{
    collections::HashMap,
    fmt::Debug,
    hash::Hash,
    sync::{
        Arc,
        mpsc::{Receiver, Sender},
    },
};

use blackbird_client_shared::cover_art_cache::Resolution;

use super::pool::ThreadPool;

/// A cache of artifacts derived from encoded cover art bytes in background
/// threads, keyed by `K`.
///
/// Encodes the lifecycle shared by every derived artifact (color grids and
/// image protocols): compute once per key, serve the stale value while a
/// higher-resolution recompute is in flight, never downgrade on late
/// arrivals, remember failures per source resolution so they are not retried
/// every frame, and drop results whose entry was evicted mid-flight.
pub(super) struct DerivedCache<K, V> {
    entries: HashMap<K, DerivedEntry<V>>,
    tx: Sender<(K, Resolution, Result<V, String>)>,
    rx: Receiver<(K, Resolution, Result<V, String>)>,
    /// Artifact name for log messages.
    name: &'static str,
}

/// The full lifecycle state of one derived artifact. The first compute often
/// runs from the 16px disk-cache image before better data has loaded, so the
/// source resolution is tracked to recompute when a higher one arrives.
struct DerivedEntry<V> {
    /// The best value computed so far, with the resolution of the source
    /// image it was computed from. `Arc` allows cheap cloning for rendering
    /// without copying the artifact.
    value: Option<(Arc<V>, Resolution)>,
    /// The source resolution of the in-flight compute, if any.
    computing: Option<Resolution>,
    /// The source resolution of the most recent failed compute. A compute
    /// from this exact resolution is not retried; a different resolution
    /// becoming the best available source clears the way for a retry.
    failed: Option<Resolution>,
}

impl<V> Default for DerivedEntry<V> {
    fn default() -> Self {
        Self {
            value: None,
            computing: None,
            failed: None,
        }
    }
}

/// The result of a [`DerivedCache`] lookup.
pub(super) struct Lookup<V> {
    /// The best value computed so far, possibly from a lower resolution than
    /// the best available source. `None` before the first compute completes.
    pub(super) value: Option<Arc<V>>,
    /// `true` while a compute is in flight or a retryable better source is
    /// available than the one the value was computed from.
    pub(super) stale: bool,
}

impl<K, V> DerivedCache<K, V>
where
    K: Clone + Eq + Hash + Debug + Send + 'static,
    V: Send + 'static,
{
    pub(super) fn new(name: &'static str) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Self {
            entries: HashMap::new(),
            tx,
            rx,
            name,
        }
    }

    /// Looks up the artifact for `key`, spawning `compute` on `pool` when no
    /// value has been computed yet or `source` is better than the one the
    /// cached value was computed from. The stale value keeps being served
    /// while the recompute runs, so the art doesn't flicker back to the
    /// caller's fallback rendering.
    pub(super) fn get_or_compute(
        &mut self,
        pool: &ThreadPool,
        key: &K,
        source: Option<(Resolution, Arc<[u8]>)>,
        compute: impl FnOnce(Arc<[u8]>) -> Result<V, String> + Send + 'static,
    ) -> Lookup<V> {
        let entry = self.entries.entry(key.clone()).or_default();

        let cached_resolution = entry.value.as_ref().map(|(_, resolution)| *resolution);
        let wants_compute = source.as_ref().is_some_and(|(source_resolution, _)| {
            let better = cached_resolution.is_none_or(|cached| *source_resolution > cached);
            better && entry.failed != Some(*source_resolution)
        });

        if wants_compute
            && entry.computing.is_none()
            && let Some((source_resolution, bytes)) = source
        {
            entry.computing = Some(source_resolution);
            let key = key.clone();
            let tx = self.tx.clone();
            pool.spawn(move || {
                let _ = tx.send((key, source_resolution, compute(bytes)));
            });
        }

        Lookup {
            value: entry.value.as_ref().map(|(value, _)| value.clone()),
            stale: entry.computing.is_some() || wants_compute,
        }
    }

    /// Returns `true` if a value has been computed for `key`.
    pub(super) fn has_value(&self, key: &K) -> bool {
        self.entries
            .get(key)
            .is_some_and(|entry| entry.value.is_some())
    }

    /// Inserts an externally computed value, unless a value from a higher
    /// source resolution is already cached. Used to seed the cache with a
    /// synchronously computed low-resolution artifact.
    pub(super) fn insert(&mut self, key: K, source_resolution: Resolution, value: Arc<V>) {
        let entry = self.entries.entry(key).or_default();
        let dominated = entry
            .value
            .as_ref()
            .is_some_and(|(_, cached)| *cached > source_resolution);
        if !dominated {
            entry.value = Some((value, source_resolution));
        }
    }

    /// Drains completed computes. Returns `true` if any cached value
    /// changed (failures don't change visual state, so they don't count).
    pub(super) fn drain(&mut self) -> bool {
        let mut changed = false;
        for (key, source_resolution, result) in self.rx.try_iter() {
            // Drop results whose entry was evicted while the compute was in
            // flight (or superseded after an evict-and-recreate); accepting
            // them would resurrect a zombie entry.
            let Some(entry) = self.entries.get_mut(&key) else {
                continue;
            };
            if entry.computing != Some(source_resolution) {
                continue;
            }
            entry.computing = None;
            match result {
                Ok(value) => {
                    let dominated = entry
                        .value
                        .as_ref()
                        .is_some_and(|(_, cached)| *cached > source_resolution);
                    if !dominated {
                        changed = true;
                        entry.value = Some((Arc::new(value), source_resolution));
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "{} compute failed for {key:?} from {source_resolution:?}: {e}",
                        self.name
                    );
                    entry.failed = Some(source_resolution);
                }
            }
        }
        changed
    }

    /// Removes every entry whose key matches the predicate.
    pub(super) fn evict_matching(&mut self, matches: impl Fn(&K) -> bool) {
        self.entries.retain(|key, _| !matches(key));
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use blackbird_core::blackbird_state::CoverArtId;

    use super::*;

    type TestKey = (CoverArtId, u16, u16);

    fn test_key(name: &str) -> TestKey {
        (CoverArtId(name.into()), 10, 10)
    }

    fn test_bytes() -> Arc<[u8]> {
        Arc::from(&[0u8; 4][..])
    }

    /// Polls `drain` until it reports a change or the timeout elapses.
    /// Returns `true` if a change was observed.
    fn drain_within(cache: &mut DerivedCache<TestKey, u32>, timeout: Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline {
            if cache.drain() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    /// A compute spawned through the pool completes, is cached, and is
    /// served on subsequent lookups without recomputing.
    #[test]
    fn test_derived_cache_computes_and_caches() {
        let pool = ThreadPool::new(1);
        let mut cache: DerivedCache<TestKey, u32> = DerivedCache::new("test");
        let key = test_key("compute");

        let lookup = cache.get_or_compute(
            &pool,
            &key,
            Some((Resolution::Library, test_bytes())),
            |_bytes| Ok(42),
        );
        assert!(lookup.value.is_none());
        assert!(lookup.stale);

        assert!(drain_within(&mut cache, Duration::from_secs(5)));

        // The cached value is now served, and no further compute is wanted.
        let lookup = cache.get_or_compute(
            &pool,
            &key,
            Some((Resolution::Library, test_bytes())),
            |_bytes| Ok(1),
        );
        assert_eq!(lookup.value.as_deref(), Some(&42));
        assert!(!lookup.stale);
    }

    /// A cached lower-resolution value keeps being served while a
    /// higher-resolution recompute runs, and is replaced when it lands.
    #[test]
    fn test_derived_cache_serves_stale_while_upgrading() {
        let pool = ThreadPool::new(1);
        let mut cache: DerivedCache<TestKey, u32> = DerivedCache::new("test");
        let key = test_key("upgrade");

        cache.insert(key.clone(), Resolution::Low, Arc::new(1));

        let lookup = cache.get_or_compute(
            &pool,
            &key,
            Some((Resolution::Full, test_bytes())),
            |_bytes| Ok(2),
        );
        // The stale low-res value is served while the upgrade decodes.
        assert_eq!(lookup.value.as_deref(), Some(&1));
        assert!(lookup.stale);

        assert!(drain_within(&mut cache, Duration::from_secs(5)));

        let lookup = cache.get_or_compute(
            &pool,
            &key,
            Some((Resolution::Full, test_bytes())),
            |_bytes| Ok(3),
        );
        assert_eq!(lookup.value.as_deref(), Some(&2));
        assert!(!lookup.stale);
    }

    /// A late lower-resolution result must not replace a cached
    /// higher-resolution value.
    #[test]
    fn test_derived_cache_no_downgrade() {
        let mut cache: DerivedCache<TestKey, u32> = DerivedCache::new("test");
        let key = test_key("downgrade");

        cache.entries.insert(
            key.clone(),
            DerivedEntry {
                value: Some((Arc::new(5), Resolution::Full)),
                computing: Some(Resolution::Library),
                failed: None,
            },
        );
        let _ = cache.tx.send((key.clone(), Resolution::Library, Ok(3)));

        assert!(!cache.drain());
        let entry = cache.entries.get(&key).unwrap();
        assert_eq!(
            entry.value.as_ref().map(|(v, r)| (**v, *r)),
            Some((5, Resolution::Full))
        );
        assert_eq!(entry.computing, None);
    }

    /// A failed compute records the failed source resolution, does not
    /// report a change (which would force a redraw and respawn the failing
    /// compute in a loop), and is not retried from the same source.
    #[test]
    fn test_derived_cache_failure_not_retried() {
        let pool = ThreadPool::new(1);
        let mut cache: DerivedCache<TestKey, u32> = DerivedCache::new("test");
        let key = test_key("failure");

        cache.entries.insert(
            key.clone(),
            DerivedEntry {
                value: None,
                computing: Some(Resolution::Low),
                failed: None,
            },
        );
        let _ = cache.tx.send((
            key.clone(),
            Resolution::Low,
            Err("decode error".to_string()),
        ));

        assert!(!cache.drain());
        assert_eq!(
            cache.entries.get(&key).unwrap().failed,
            Some(Resolution::Low)
        );

        // The same source must not be retried…
        let lookup = cache.get_or_compute(
            &pool,
            &key,
            Some((Resolution::Low, test_bytes())),
            |_bytes| Ok(1),
        );
        assert!(!lookup.stale);
        assert_eq!(cache.entries.get(&key).unwrap().computing, None);

        // …but a better source clears the way for a retry.
        let lookup = cache.get_or_compute(
            &pool,
            &key,
            Some((Resolution::Library, test_bytes())),
            |_bytes| Ok(1),
        );
        assert!(lookup.stale);
        assert!(drain_within(&mut cache, Duration::from_secs(5)));
    }

    /// Results whose entry was evicted while the compute was in flight are
    /// dropped rather than resurrected as zombie entries.
    #[test]
    fn test_derived_cache_untracked_result_dropped() {
        let mut cache: DerivedCache<TestKey, u32> = DerivedCache::new("test");
        let key = test_key("untracked");

        // No entry exists — as if it was evicted mid-decode.
        let _ = cache.tx.send((key.clone(), Resolution::Full, Ok(9)));

        assert!(!cache.drain());
        assert!(!cache.entries.contains_key(&key));
    }

    /// Eviction removes exactly the entries whose keys match.
    #[test]
    fn test_derived_cache_evict_matching() {
        let mut cache: DerivedCache<TestKey, u32> = DerivedCache::new("test");
        let keep = test_key("keep");
        let evict = test_key("evict");
        cache.insert(keep.clone(), Resolution::Low, Arc::new(1));
        cache.insert(evict.clone(), Resolution::Low, Arc::new(2));

        let (evict_id, _, _) = evict.clone();
        cache.evict_matching(|(id, _, _)| *id == evict_id);

        assert!(cache.has_value(&keep));
        assert!(!cache.entries.contains_key(&evict));
    }

    /// `insert` seeds a value but never overwrites a higher-resolution one.
    #[test]
    fn test_derived_cache_insert_respects_dominance() {
        let mut cache: DerivedCache<TestKey, u32> = DerivedCache::new("test");
        let key = test_key("insert");

        cache.insert(key.clone(), Resolution::Library, Arc::new(1));
        cache.insert(key.clone(), Resolution::Low, Arc::new(2));

        let entry = cache.entries.get(&key).unwrap();
        assert_eq!(
            entry.value.as_ref().map(|(v, r)| (**v, *r)),
            Some((1, Resolution::Library))
        );
    }
}
