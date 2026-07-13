//! Cover art cache shared between the egui and TUI clients.
//!
//! The cache is demand-driven. Each draw, the client starts a new demand
//! frame with [`CoverArtCache::begin_frame`] and declares the art it wants
//! via [`CoverArtCache::get`], which is a pure read plus a demand record.
//! Once per tick, [`CoverArtCache::update`] reconciles the cache against
//! the accumulated demand: it fetches demanded art that is missing, keeps
//! demanded art alive, and evicts art whose demand has lapsed. Between
//! draws the last frame's demand stays in effect, so a lazily redrawing
//! client keeps its visible art alive without further bookkeeping.
//!
//! Each cache entry holds up to three resolution tiers (low, library, full)
//! that load on demand and degrade when no longer demanded.
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use blackbird_core::{CoverArt, Logic, blackbird_state::CoverArtId};

use crate::thread_pool::ThreadPool;

/// How long an id must have been demanded before a library-res fetch is
/// issued, debouncing fetch storms while scrolling quickly.
const TIME_BEFORE_LOAD_ATTEMPT: Duration = Duration::from_millis(100);
const LOW_RES_CACHE_SIZE: u32 = 16;
const CACHE_DIR_NAME: &str = "album-art-cache";

/// How long a higher-resolution slot can go undemanded before being dropped.
const RESOLUTION_STALE_TIMEOUT: Duration = Duration::from_secs(30);

/// Resolution requested from the server for library-size art (thumbnails, BelowAlbum grids).
pub const LIBRARY_ART_SIZE: usize = 128;

/// Maximum number of full-resolution images kept in memory (overlays).
pub const FULL_RES_MAX_CACHE_SIZE: usize = 5;

/// Resolution tiers for cover art, ordered from lowest to highest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Resolution {
    /// 16px — disk cache, always kept while entry exists.
    Low = 0,
    /// 128px — on-demand for rendered albums.
    Library = 1,
    /// Full resolution — on-demand for overlay viewing.
    Full = 2,
}

/// Clients implement this to produce their own data from raw cover art bytes.
/// Called when image data first arrives or transitions to a new resolution.
pub trait ClientData: Clone {
    /// Create client data from raw image bytes at a given resolution.
    fn from_image_data(data: &Arc<[u8]>, cover_art_id: &CoverArtId, resolution: Resolution)
    -> Self;

    /// Called during resolution upgrades to carry relevant state from the
    /// previous (lower) resolution's client data into the new one.
    fn carry_over(&mut self, _previous: &Self) {}
}

/// The cache's window onto the application: how to fetch art from the
/// server, and which art the play queue is about to need. Abstracted from
/// [`Logic`] so the cache's reconciliation policy is unit-testable.
pub trait CoverArtSource {
    /// Request cover art from the server; the response arrives on the
    /// channel passed to [`CoverArtCache::new`]. `size` is the requested
    /// edge length in pixels, or `None` for full resolution.
    fn request_cover_art(&self, cover_art_id: &CoverArtId, size: Option<usize>);

    /// The cover art id for the album containing the next queued track.
    /// Demanded at library resolution and `NextTrack` priority every update
    /// so that track transitions don't flash placeholder art.
    fn next_track_cover_art_id(&self) -> Option<CoverArtId>;

    /// Cover art ids for the albums surrounding the next track's album.
    /// Demanded at library resolution and `Nearby` priority every update.
    fn next_track_surrounding_cover_art_ids(&self) -> Vec<CoverArtId>;
}

impl CoverArtSource for Logic {
    fn request_cover_art(&self, cover_art_id: &CoverArtId, size: Option<usize>) {
        Logic::request_cover_art(self, cover_art_id, size);
    }

    fn next_track_cover_art_id(&self) -> Option<CoverArtId> {
        self.get_next_track_cover_art_id()
    }

    fn next_track_surrounding_cover_art_ids(&self) -> Vec<CoverArtId> {
        self.get_next_track_surrounding_cover_art_ids()
    }
}

/// Result of a `get()` call, containing the best available client data
/// and the resolution tier it came from.
pub struct GetResult<'a, T> {
    pub data: &'a T,
    pub resolution: Resolution,
}

/// Result of an `update()` call.
pub struct UpdateResult {
    /// IDs of entries that were fully evicted from the cache.
    pub evicted: Vec<CoverArtId>,
    /// IDs and resolutions of newly populated image data slots.
    pub upgraded: Vec<(CoverArtId, Resolution)>,
}

/// Priority levels for demanded art, from lowest to highest. Undemanded
/// entries rank below every priority for size-based eviction, and entries
/// demanded at `Visible` are never size-evicted. All entries are evicted
/// once their demand has lapsed for the configured timeout, regardless of
/// the priority they were last demanded at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CachePriority {
    /// Art near what is displayed or queued: albums about a page outside
    /// the viewport, or albums around the next queued track's album. Kept
    /// warm so scrolling and track transitions don't flash placeholder art.
    Nearby,
    /// Art for the album containing the next queued track.
    NextTrack,
    /// Art that is currently displayed.
    Visible,
}

/// The set of resolutions demanded for one id within a frame, indexed by
/// [`Resolution`] discriminant.
#[derive(Debug, Default, Clone, Copy)]
struct DemandedResolutions([bool; 3]);

impl DemandedResolutions {
    fn insert(&mut self, resolution: Resolution) {
        self.0[resolution as usize] = true;
    }

    fn contains(&self, resolution: Resolution) -> bool {
        self.0[resolution as usize]
    }

    fn max(&self) -> Option<Resolution> {
        [Resolution::Full, Resolution::Library, Resolution::Low]
            .into_iter()
            .find(|resolution| self.contains(*resolution))
    }
}

/// One id's accumulated demand: which resolutions are wanted, at what
/// priority.
#[derive(Debug, Clone, Copy)]
struct Demand {
    resolutions: DemandedResolutions,
    priority: CachePriority,
}

struct ImageData<T> {
    data: Arc<[u8]>,
    client_data: T,
    /// The most recent update at which this slot was covered by a demand.
    /// A demand at resolution R covers every loaded slot at or below R,
    /// since the lower slots serve as fallbacks while R loads.
    last_demanded: Instant,
}

struct CacheEntry<T> {
    low_res: Option<ImageData<T>>,
    library_res: Option<ImageData<T>>,
    full_res: Option<ImageData<T>>,
    /// Resolutions currently being loaded from the network.
    loading: HashSet<Resolution>,
    /// When this id first entered the demand set (or was created for the
    /// prefetcher). Library-res fetches are debounced against this so that
    /// scrolling quickly past an album doesn't fetch its art.
    first_demanded: Instant,
    /// The most recent update at which this id was demanded. Entries are
    /// evicted once this lapses beyond the configured timeout.
    last_demanded: Instant,
}

impl<T> CacheEntry<T> {
    fn new(now: Instant) -> Self {
        Self {
            low_res: None,
            library_res: None,
            full_res: None,
            loading: HashSet::new(),
            first_demanded: now,
            last_demanded: now,
        }
    }

    /// Returns the best available client data up to (and including) the
    /// requested resolution, along with its tier.
    fn best_up_to(&self, resolution: Resolution) -> Option<(&T, Resolution)> {
        if resolution >= Resolution::Full
            && let Some(ref slot) = self.full_res
        {
            return Some((&slot.client_data, Resolution::Full));
        }
        if resolution >= Resolution::Library
            && let Some(ref slot) = self.library_res
        {
            return Some((&slot.client_data, Resolution::Library));
        }
        if let Some(ref slot) = self.low_res {
            return Some((&slot.client_data, Resolution::Low));
        }
        None
    }

    /// Returns a mutable reference to the slot for a given resolution.
    fn slot_mut(&mut self, resolution: Resolution) -> &mut Option<ImageData<T>> {
        match resolution {
            Resolution::Low => &mut self.low_res,
            Resolution::Library => &mut self.library_res,
            Resolution::Full => &mut self.full_res,
        }
    }

    /// Returns a reference to the slot for a given resolution.
    fn slot(&self, resolution: Resolution) -> &Option<ImageData<T>> {
        match resolution {
            Resolution::Low => &self.low_res,
            Resolution::Library => &self.library_res,
            Resolution::Full => &self.full_res,
        }
    }
}

pub struct CoverArtCache<T: ClientData> {
    cover_art_loaded_rx: std::sync::mpsc::Receiver<CoverArt>,
    cache: HashMap<CoverArtId, CacheEntry<T>>,
    cache_dir: PathBuf,
    max_cache_size: usize,
    cache_entry_timeout: Duration,
    /// How long a library/full slot can go undemanded before being dropped.
    resolution_stale_timeout: Duration,
    /// How long an id must be demanded before a library-res fetch is issued.
    load_debounce: Duration,
    /// The art demanded by the client's most recent frame, rebuilt by `get`
    /// calls after each `begin_frame`. Persists between frames, so a lazily
    /// redrawing client's visible art stays demanded while it isn't drawing.
    frame_demand: HashMap<CoverArtId, Demand>,
    prefetcher: BackgroundPrefetcher,
    /// A single worker for disk-cache writes, so bursts of incoming art
    /// don't spawn a thread per write.
    disk_write_pool: ThreadPool,
}

impl<T: ClientData> CoverArtCache<T> {
    pub fn new(
        cover_art_loaded_rx: std::sync::mpsc::Receiver<CoverArt>,
        max_cache_size: usize,
        cache_entry_timeout: Duration,
    ) -> Self {
        let cache_dir = blackbird_shared::paths::cache_dir().join(CACHE_DIR_NAME);
        Self::with_cache_dir(
            cover_art_loaded_rx,
            max_cache_size,
            cache_entry_timeout,
            cache_dir,
        )
    }

    fn with_cache_dir(
        cover_art_loaded_rx: std::sync::mpsc::Receiver<CoverArt>,
        max_cache_size: usize,
        cache_entry_timeout: Duration,
        cache_dir: PathBuf,
    ) -> Self {
        if let Err(e) = std::fs::create_dir_all(&cache_dir) {
            panic!("Failed to create cache directory: {e}");
        }

        Self {
            cover_art_loaded_rx,
            cache: HashMap::new(),
            cache_dir,
            max_cache_size,
            cache_entry_timeout,
            resolution_stale_timeout: RESOLUTION_STALE_TIMEOUT,
            load_debounce: TIME_BEFORE_LOAD_ATTEMPT,
            frame_demand: HashMap::new(),
            prefetcher: BackgroundPrefetcher::new(),
            disk_write_pool: ThreadPool::new(1),
        }
    }

    /// Start a new demand frame, clearing the previous frame's demand. Call
    /// at the start of each draw; the draw's `get` calls then rebuild the
    /// demand set from what is actually displayed.
    pub fn begin_frame(&mut self) {
        self.frame_demand.clear();
    }

    /// Record demand for an id at a resolution and priority without
    /// reading. Used for art that should be kept warm but is not rendered
    /// this frame, such as albums just outside the viewport. Entry
    /// creation, fetching, and eviction all happen in
    /// [`update`](Self::update), reconciled against the demand accumulated
    /// here.
    pub fn demand(
        &mut self,
        id: Option<&CoverArtId>,
        resolution: Resolution,
        priority: CachePriority,
    ) {
        let Some(cover_art_id) = id else {
            return;
        };
        let demand = self
            .frame_demand
            .entry(cover_art_id.clone())
            .or_insert(Demand {
                resolutions: DemandedResolutions::default(),
                priority,
            });
        demand.resolutions.insert(resolution);
        demand.priority = demand.priority.max(priority);
    }

    /// Record demand for an id at a resolution and priority, and return the
    /// best already-loaded data at or below that resolution. A pure read
    /// plus a demand record — see [`demand`](Self::demand). Returns `None`
    /// when no image data is loaded yet.
    pub fn get(
        &mut self,
        id: Option<&CoverArtId>,
        resolution: Resolution,
        priority: CachePriority,
    ) -> Option<GetResult<'_, T>> {
        let cover_art_id = id?;
        self.demand(id, resolution, priority);

        let (data, res) = self.cache.get(cover_art_id)?.best_up_to(resolution)?;
        Some(GetResult {
            data,
            resolution: res,
        })
    }

    /// Reconcile the cache against the current demand: process incoming
    /// cover art, create entries and issue fetches for demanded art that is
    /// missing, degrade resolution slots whose demand has lapsed, evict
    /// entries that are undemanded or over budget, and advance the
    /// background prefetcher. Returns evicted entry IDs and newly populated
    /// resolution slots.
    pub fn update(&mut self, source: &impl CoverArtSource) -> UpdateResult {
        let now = Instant::now();
        let mut upgraded = Vec::new();

        // Process incoming cover art into resolution slots.
        for incoming in self.cover_art_loaded_rx.try_iter() {
            let Some(entry) = self.cache.get_mut(&incoming.cover_art_id) else {
                tracing::debug!(
                    "Cache entry for {} not found when receiving cover art",
                    incoming.cover_art_id
                );
                continue;
            };

            // Determine which slot this response belongs to.
            let resolution = match incoming.requested_size {
                None => Resolution::Full,
                Some(s) if s == LIBRARY_ART_SIZE => Resolution::Library,
                Some(_) => Resolution::Library, // Fallback for other sizes.
            };

            entry.loading.remove(&resolution);

            let data: Arc<[u8]> = incoming.cover_art.clone().into();
            let mut client_data = T::from_image_data(&data, &incoming.cover_art_id, resolution);

            // Carry over state from the next lower resolution if available.
            let lower = match resolution {
                Resolution::Full => entry.library_res.as_ref().or(entry.low_res.as_ref()),
                Resolution::Library => entry.low_res.as_ref(),
                Resolution::Low => None,
            };
            if let Some(lower_slot) = lower {
                client_data.carry_over(&lower_slot.client_data);
            }

            *entry.slot_mut(resolution) = Some(ImageData {
                data,
                client_data,
                last_demanded: now,
            });

            tracing::debug!(
                "Loaded {:?} cover art for {}",
                resolution,
                incoming.cover_art_id
            );

            upgraded.push((incoming.cover_art_id.clone(), resolution));

            // Save a low-res version to disk cache for future use.
            let cache_path = disk_cache_path(&self.cache_dir, &incoming.cover_art_id);
            if !cache_path.exists() {
                let cover_art = incoming.cover_art.clone();
                self.disk_write_pool.spawn(move || {
                    save_to_disk_cache(&cache_path, &cover_art);
                });
            }
        }

        // Merge the frame demand with the queue demand: the next queued
        // track's album is demanded at `NextTrack` priority and its
        // surrounding albums at `Nearby`, all at library resolution, so
        // that track transitions don't flash placeholder art.
        let mut demand: HashMap<CoverArtId, Demand> = self
            .frame_demand
            .iter()
            .map(|(id, demand)| (id.clone(), *demand))
            .collect();
        {
            let mut queue_demand = |id: CoverArtId, priority: CachePriority| {
                let entry = demand.entry(id).or_insert(Demand {
                    resolutions: DemandedResolutions::default(),
                    priority,
                });
                entry.resolutions.insert(Resolution::Library);
                entry.priority = entry.priority.max(priority);
            };
            if let Some(id) = source.next_track_cover_art_id() {
                queue_demand(id, CachePriority::NextTrack);
            }
            for id in source.next_track_surrounding_cover_art_ids() {
                queue_demand(id, CachePriority::Nearby);
            }
        }

        // Reconcile each demanded id: ensure an entry exists, refresh its
        // liveness, and issue fetches for demanded-but-missing resolutions.
        for (id, demand) in &demand {
            let entry = self
                .cache
                .entry(id.clone())
                .or_insert_with(|| CacheEntry::new(now));
            entry.last_demanded = now;

            // Load the disk-cached low-res thumbnail if the slot is empty.
            if entry.low_res.is_none()
                && let Some(low_res_data) = load_from_disk_cache(&self.cache_dir, id)
            {
                let client_data = T::from_image_data(&low_res_data, id, Resolution::Low);
                entry.low_res = Some(ImageData {
                    data: low_res_data,
                    client_data,
                    last_demanded: now,
                });
                upgraded.push((id.clone(), Resolution::Low));
            }

            // A demand at resolution R covers every loaded slot at or below
            // R: the lower slots serve as fallbacks while R loads, and are
            // typically still displayed elsewhere.
            if let Some(max_resolution) = demand.resolutions.max() {
                for resolution in [Resolution::Library, Resolution::Full] {
                    if resolution <= max_resolution
                        && let Some(slot) = entry.slot_mut(resolution).as_mut()
                    {
                        slot.last_demanded = now;
                    }
                }
            }

            // Issue fetches for demanded-but-missing resolutions. Low is
            // disk-only. Library and full are independent — a full-res
            // demand does not imply a library fetch, because the low-res
            // fallback is sufficient while waiting and the extra request
            // would compete with other fetches.
            if demand.resolutions.contains(Resolution::Library)
                && entry.library_res.is_none()
                && !entry.loading.contains(&Resolution::Library)
                && entry.first_demanded.elapsed() > self.load_debounce
            {
                source.request_cover_art(id, Some(LIBRARY_ART_SIZE));
                entry.loading.insert(Resolution::Library);
                tracing::debug!("Requesting library-res cover art for {id}");
            }
            if demand.resolutions.contains(Resolution::Full)
                && entry.full_res.is_none()
                && !entry.loading.contains(&Resolution::Full)
            {
                source.request_cover_art(id, None);
                entry.loading.insert(Resolution::Full);
                tracing::debug!("Requesting full-res cover art for {id}");
            }
        }

        // Degrade higher-resolution slots whose demand has lapsed.
        for entry in self.cache.values_mut() {
            if let Some(ref slot) = entry.full_res
                && slot.last_demanded.elapsed() > self.resolution_stale_timeout
            {
                entry.full_res = None;
            }
            if let Some(ref slot) = entry.library_res
                && slot.last_demanded.elapsed() > self.resolution_stale_timeout
            {
                entry.library_res = None;
            }
        }

        // Limit total full_res slots to FULL_RES_MAX_CACHE_SIZE via LRU eviction.
        let mut full_res_entries: Vec<(CoverArtId, Instant)> = self
            .cache
            .iter()
            .filter_map(|(id, entry)| {
                entry
                    .full_res
                    .as_ref()
                    .map(|slot| (id.clone(), slot.last_demanded))
            })
            .collect();
        if full_res_entries.len() > FULL_RES_MAX_CACHE_SIZE {
            full_res_entries.sort_by_key(|(_, t)| *t);
            let to_evict = full_res_entries.len() - FULL_RES_MAX_CACHE_SIZE;
            for (id, _) in full_res_entries.into_iter().take(to_evict) {
                if let Some(entry) = self.cache.get_mut(&id) {
                    entry.full_res = None;
                    tracing::debug!("Evicted full-res cover art for {id}");
                }
            }
        }

        // Evict entries whose demand lapsed longer than the timeout ago.
        // Demanded entries were refreshed above, so this only ever removes
        // undemanded ones; the grace period keeps recently offscreen art
        // warm for scrolling back.
        let mut removal_candidates = HashSet::new();
        for (cover_art_id, cache_entry) in self.cache.iter() {
            if cache_entry.last_demanded.elapsed() > self.cache_entry_timeout {
                tracing::debug!(
                    "Forgetting cover art for {cover_art_id} from cache due to timeout"
                );
                removal_candidates.insert(cover_art_id.clone());
            }
        }

        // Evict entries beyond the size budget. Entries demanded at
        // `Visible` are never size-evicted; everything else goes
        // least-recently-demanded first, with undemanded entries before
        // demanded ones, so recently offscreen art outlives older art.
        let overage = self
            .cache
            .len()
            .saturating_sub(self.max_cache_size)
            .saturating_sub(removal_candidates.len());
        if overage > 0 {
            tracing::debug!("Cache overage: {overage} entries need to be evicted");

            let mut evictable = self
                .cache
                .iter()
                .filter_map(|(cover_art_id, cache_entry)| {
                    if removal_candidates.contains(cover_art_id) {
                        return None;
                    }
                    let priority = demand.get(cover_art_id).map(|demand| demand.priority);
                    (priority != Some(CachePriority::Visible)).then_some((
                        cover_art_id.clone(),
                        priority,
                        cache_entry.last_demanded,
                    ))
                })
                .collect::<Vec<_>>();

            // `None` (undemanded) orders before every demanded priority.
            evictable.sort_by_key(|(_, priority, last_demanded)| (*priority, *last_demanded));

            for (cover_art_id, priority, _) in evictable.into_iter().take(overage) {
                tracing::debug!(
                    "Forgetting cover art for {cover_art_id} from cache due to size limit (demand: {priority:?})"
                );
                removal_candidates.insert(cover_art_id);
            }
        }

        self.cache
            .retain(|cover_art_id, _| !removal_candidates.contains(cover_art_id));

        // Advance the background prefetcher: create an entry to receive the
        // response (so it can be disk-cached), bypassing the load debounce.
        if let Some(id) = self.prefetcher.tick() {
            let entry = self
                .cache
                .entry(id.clone())
                .or_insert_with(|| CacheEntry::new(now));
            if entry.library_res.is_none() && !entry.loading.contains(&Resolution::Library) {
                source.request_cover_art(&id, Some(LIBRARY_ART_SIZE));
                entry.loading.insert(Resolution::Library);
            }
        }

        UpdateResult {
            evicted: removal_candidates.into_iter().collect(),
            upgraded,
        }
    }

    /// Read-only access to the client data at a specific resolution tier.
    /// No demand is recorded.
    pub fn get_resolution(&self, id: &CoverArtId, resolution: Resolution) -> Option<&T> {
        let entry = self.cache.get(id)?;
        entry
            .slot(resolution)
            .as_ref()
            .map(|slot| &slot.client_data)
    }

    /// Returns true if the given resolution tier is fully loaded for the entry.
    pub fn is_resolution_loaded(&self, id: &CoverArtId, resolution: Resolution) -> bool {
        self.cache
            .get(id)
            .is_some_and(|entry| entry.slot(resolution).is_some())
    }

    /// Mutable access to the client data and raw bytes at the best available
    /// resolution. No demand is recorded. Returns `None` if the entry has
    /// no loaded data.
    pub fn with_client_data_mut<R>(
        &mut self,
        id: &CoverArtId,
        f: impl FnOnce(&mut T, &Arc<[u8]>) -> R,
    ) -> Option<R> {
        let entry = self.cache.get_mut(id)?;

        // Try full > library > low.
        for res in [Resolution::Full, Resolution::Library, Resolution::Low] {
            if let Some(slot) = entry.slot_mut(res) {
                return Some(f(&mut slot.client_data, &slot.data));
            }
        }
        None
    }

    /// Mutable access to the client data and raw bytes at a specific
    /// resolution tier. No demand is recorded. Returns `None` if the slot
    /// is not populated.
    pub fn with_client_data_mut_at<R>(
        &mut self,
        id: &CoverArtId,
        resolution: Resolution,
        f: impl FnOnce(&mut T, &Arc<[u8]>) -> R,
    ) -> Option<R> {
        let entry = self.cache.get_mut(id)?;
        if let Some(slot) = entry.slot_mut(resolution) {
            Some(f(&mut slot.client_data, &slot.data))
        } else {
            None
        }
    }

    /// Populate the background prefetch queue with cover art IDs, filtering
    /// out any that already exist in the on-disk cache or in-memory cache.
    pub fn populate_prefetch_queue(&mut self, cover_art_ids: Vec<CoverArtId>) {
        let ids: Vec<CoverArtId> = cover_art_ids
            .into_iter()
            .filter(|id| {
                // Skip IDs already in the in-memory cache (already requested or loaded).
                if self.cache.contains_key(id) {
                    return false;
                }
                // Skip IDs already in the on-disk cache.
                !disk_cache_path(&self.cache_dir, id).exists()
            })
            .collect();
        self.prefetcher.populate(ids);
    }
}

const PREFETCH_INTERVAL: Duration = Duration::from_millis(100);

struct BackgroundPrefetcher {
    queue: VecDeque<CoverArtId>,
    total: usize,
    last_request: Instant,
    next_milestone: usize,
    active: bool,
}

impl BackgroundPrefetcher {
    fn new() -> Self {
        Self {
            queue: VecDeque::new(),
            total: 0,
            last_request: Instant::now(),
            next_milestone: 10,
            active: false,
        }
    }

    fn populate(&mut self, ids: Vec<CoverArtId>) {
        if ids.is_empty() {
            return;
        }
        tracing::info!("Background art prefetch: starting ({} albums)", ids.len());
        self.queue = ids.into();
        self.total = self.queue.len();
        self.next_milestone = 10;
        self.active = true;
    }

    /// Returns the next ID to fetch if enough time has passed, or `None`.
    fn tick(&mut self) -> Option<CoverArtId> {
        if !self.active {
            return None;
        }

        if self.last_request.elapsed() < PREFETCH_INTERVAL {
            return None;
        }

        let id = match self.queue.pop_front() {
            Some(id) => id,
            None => {
                tracing::info!("Background art prefetch: complete ({} albums)", self.total);
                self.active = false;
                return None;
            }
        };

        self.last_request = Instant::now();

        // Log at 10% milestones.
        let done = self.total - self.queue.len();
        let pct = done * 100 / self.total;
        if pct >= self.next_milestone {
            tracing::info!("Background art prefetch: {done}/{} ({pct}%)", self.total);
            self.next_milestone = pct / 10 * 10 + 10;
        }

        Some(id)
    }
}

/// The on-disk path for an id's low-res thumbnail, with the id sanitized
/// into a valid filename.
fn disk_cache_path(cache_dir: &Path, cover_art_id: &CoverArtId) -> PathBuf {
    let safe_filename = cover_art_id
        .0
        .replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
    cache_dir.join(format!("{safe_filename}.png"))
}

fn load_from_disk_cache(cache_dir: &Path, cover_art_id: &CoverArtId) -> Option<Arc<[u8]>> {
    match std::fs::read(disk_cache_path(cache_dir, cover_art_id)) {
        Ok(data) => {
            tracing::debug!(
                "Loaded low-res cover art for {} from disk cache",
                cover_art_id
            );
            Some(data.into())
        }
        Err(_) => None,
    }
}

fn save_to_disk_cache(cache_path: &Path, image_data: &[u8]) {
    // Decode the image.
    let Ok(img) = image::load_from_memory(image_data) else {
        tracing::warn!("Failed to decode image for {}", cache_path.display());
        return;
    };

    // Resize to low-res size.
    let resized = img.resize_exact(
        LOW_RES_CACHE_SIZE,
        LOW_RES_CACHE_SIZE,
        image::imageops::FilterType::Triangle,
    );

    // Apply explicit blur to destroy high-level detail.
    let blurred = image::imageops::fast_blur(&resized.into_rgb8(), 1.0);

    // Encode as PNG.
    let mut buffer = std::io::Cursor::new(Vec::new());
    if let Err(e) = blurred.write_to(&mut buffer, image::ImageFormat::Png) {
        tracing::warn!(
            "Failed to encode resized image for {}: {}",
            cache_path.display(),
            e
        );
        return;
    }

    // Save to disk.
    if let Err(e) = std::fs::write(cache_path, buffer.into_inner()) {
        tracing::warn!(
            "Failed to save low-res cover art for {} to disk: {}",
            cache_path.display(),
            e
        );
    } else {
        tracing::debug!(
            "Saved low-res cover art for {} to disk cache",
            cache_path.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, sync::mpsc};

    use super::*;

    #[derive(Clone)]
    struct TestData(#[allow(dead_code)] Arc<[u8]>);

    impl ClientData for TestData {
        fn from_image_data(data: &Arc<[u8]>, _id: &CoverArtId, _resolution: Resolution) -> Self {
            TestData(data.clone())
        }
    }

    #[derive(Default)]
    struct MockSource {
        requests: RefCell<Vec<(CoverArtId, Option<usize>)>>,
        next_track_id: Option<CoverArtId>,
        next_track_surrounding_ids: Vec<CoverArtId>,
    }

    impl CoverArtSource for MockSource {
        fn request_cover_art(&self, cover_art_id: &CoverArtId, size: Option<usize>) {
            self.requests
                .borrow_mut()
                .push((cover_art_id.clone(), size));
        }

        fn next_track_cover_art_id(&self) -> Option<CoverArtId> {
            self.next_track_id.clone()
        }

        fn next_track_surrounding_cover_art_ids(&self) -> Vec<CoverArtId> {
            self.next_track_surrounding_ids.clone()
        }
    }

    fn id(name: &str) -> CoverArtId {
        CoverArtId(name.into())
    }

    fn response(cover_art_id: &CoverArtId, requested_size: Option<usize>) -> CoverArt {
        CoverArt {
            cover_art_id: cover_art_id.clone(),
            cover_art: vec![1, 2, 3, 4],
            requested_size,
        }
    }

    /// Creates a cache with a fresh per-test disk-cache directory (so tests
    /// neither touch the user's real cache nor observe each other's writes)
    /// and a zero load debounce.
    fn test_cache(
        test_name: &str,
        max_cache_size: usize,
        cache_entry_timeout: Duration,
    ) -> (CoverArtCache<TestData>, mpsc::Sender<CoverArt>) {
        let (tx, rx) = mpsc::channel();
        let dir = std::env::temp_dir().join(format!(
            "blackbird-cover-art-cache-test-{}-{test_name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let mut cache = CoverArtCache::with_cache_dir(rx, max_cache_size, cache_entry_timeout, dir);
        cache.load_debounce = Duration::ZERO;
        (cache, tx)
    }

    const LONG: Duration = Duration::from_secs(3600);

    /// Demanding an id issues a library-res fetch on the next update, and
    /// the response becomes readable at that resolution.
    #[test]
    fn test_demand_fetches_and_loads() {
        let (mut cache, tx) = test_cache("fetch", 10, LONG);
        let source = MockSource::default();
        let a = id("a");

        cache.begin_frame();
        assert!(
            cache
                .get(Some(&a), Resolution::Library, CachePriority::Visible)
                .is_none()
        );
        cache.update(&source);
        assert_eq!(
            source.requests.borrow().as_slice(),
            &[(a.clone(), Some(LIBRARY_ART_SIZE))]
        );

        // The fetch is not re-issued while in flight.
        cache.update(&source);
        assert_eq!(source.requests.borrow().len(), 1);

        tx.send(response(&a, Some(LIBRARY_ART_SIZE))).unwrap();
        let result = cache.update(&source);
        assert_eq!(result.upgraded, vec![(a.clone(), Resolution::Library)]);

        let got = cache
            .get(Some(&a), Resolution::Library, CachePriority::Visible)
            .expect("art should be loaded");
        assert_eq!(got.resolution, Resolution::Library);
    }

    /// `get` is a pure read plus demand record: it never creates cache
    /// entries or issues fetches by itself.
    #[test]
    fn test_get_never_fetches() {
        let (mut cache, _tx) = test_cache("pure-get", 10, LONG);
        let source = MockSource::default();

        cache.begin_frame();
        cache.get(Some(&id("a")), Resolution::Library, CachePriority::Visible);
        assert!(cache.cache.is_empty());
        assert!(source.requests.borrow().is_empty());
    }

    /// The library-res fetch is debounced: an id must have been demanded
    /// for longer than the debounce before the request is issued.
    #[test]
    fn test_library_fetch_debounced() {
        let (mut cache, _tx) = test_cache("debounce", 10, LONG);
        cache.load_debounce = Duration::from_millis(30);
        let source = MockSource::default();
        let a = id("a");

        cache.begin_frame();
        cache.get(Some(&a), Resolution::Library, CachePriority::Visible);
        cache.update(&source);
        assert!(
            source.requests.borrow().is_empty(),
            "the fetch should be debounced"
        );

        std::thread::sleep(Duration::from_millis(40));
        cache.update(&source);
        assert_eq!(source.requests.borrow().len(), 1);
    }

    /// Full-res demands are fetched immediately (no debounce) and do not
    /// imply a library-res fetch.
    #[test]
    fn test_full_res_fetch_immediate_and_independent() {
        let (mut cache, _tx) = test_cache("full-res", 10, LONG);
        cache.load_debounce = Duration::from_secs(3600);
        let source = MockSource::default();
        let a = id("a");

        cache.begin_frame();
        cache.get(Some(&a), Resolution::Full, CachePriority::Visible);
        cache.update(&source);
        assert_eq!(source.requests.borrow().as_slice(), &[(a.clone(), None)]);
    }

    /// Entries are evicted once their demand has lapsed beyond the timeout,
    /// but stay warm within the grace period (recently offscreen art).
    #[test]
    fn test_undemanded_entry_evicted_after_grace() {
        let (mut cache, _tx) = test_cache("grace", 10, Duration::from_millis(30));
        let source = MockSource::default();
        let a = id("a");

        cache.begin_frame();
        cache.get(Some(&a), Resolution::Library, CachePriority::Visible);
        cache.update(&source);
        assert!(cache.cache.contains_key(&a));

        // A new frame without the id: demand lapses, but the entry stays
        // within the grace period.
        cache.begin_frame();
        let result = cache.update(&source);
        assert!(result.evicted.is_empty());
        assert!(cache.cache.contains_key(&a));

        std::thread::sleep(Duration::from_millis(40));
        let result = cache.update(&source);
        assert_eq!(result.evicted, vec![a.clone()]);
        assert!(!cache.cache.contains_key(&a));
    }

    /// A continuously demanded entry survives past the timeout.
    #[test]
    fn test_demanded_entry_stays_alive() {
        let (mut cache, _tx) = test_cache("keepalive", 10, Duration::from_millis(30));
        let source = MockSource::default();
        let a = id("a");

        cache.begin_frame();
        cache.get(Some(&a), Resolution::Library, CachePriority::Visible);
        cache.update(&source);

        // No new frame — the previous frame's demand stays in effect, as it
        // does for a lazily redrawing client.
        std::thread::sleep(Duration::from_millis(40));
        let result = cache.update(&source);
        assert!(result.evicted.is_empty());
        assert!(cache.cache.contains_key(&a));
    }

    /// Under size pressure, undemanded entries are evicted
    /// least-recently-demanded first, while `Visible` demand is protected.
    #[test]
    fn test_size_eviction_prefers_undemanded_lru() {
        let (mut cache, _tx) = test_cache("size-lru", 2, LONG);
        let source = MockSource::default();
        let (a, b, c) = (id("a"), id("b"), id("c"));

        // Frame 1: `a` is visible.
        cache.begin_frame();
        cache.get(Some(&a), Resolution::Library, CachePriority::Visible);
        cache.update(&source);

        std::thread::sleep(Duration::from_millis(5));

        // Frame 2: `b` and `c` are visible; `a` scrolls offscreen.
        cache.begin_frame();
        cache.get(Some(&b), Resolution::Library, CachePriority::Visible);
        cache.get(Some(&c), Resolution::Library, CachePriority::Visible);
        let result = cache.update(&source);

        // Three entries, budget of two: the undemanded `a` goes; the
        // visible `b` and `c` are protected.
        assert_eq!(result.evicted, vec![a.clone()]);
        assert!(cache.cache.contains_key(&b));
        assert!(cache.cache.contains_key(&c));
    }

    /// The next-track album and its surrounding albums are demanded
    /// automatically at library resolution, and outrank undemanded entries
    /// for size eviction.
    #[test]
    fn test_next_track_demand() {
        let (mut cache, _tx) = test_cache("next-track", 2, LONG);
        let n = id("next");
        let s = id("surrounding");
        let o = id("old");
        let source = MockSource {
            next_track_id: Some(n.clone()),
            next_track_surrounding_ids: vec![s.clone()],
            ..Default::default()
        };

        // Seed an entry that will become undemanded.
        cache.begin_frame();
        cache.get(Some(&o), Resolution::Library, CachePriority::Visible);
        cache.update(&MockSource::default());

        // With no frame demand, the queue demand fetches both the
        // next-track art and its surroundings, and protects them over the
        // undemanded entry.
        cache.begin_frame();
        let result = cache.update(&source);
        let requests = source.requests.borrow();
        assert!(requests.contains(&(n.clone(), Some(LIBRARY_ART_SIZE))));
        assert!(requests.contains(&(s.clone(), Some(LIBRARY_ART_SIZE))));
        assert_eq!(result.evicted, vec![o.clone()]);
        assert!(cache.cache.contains_key(&n));
        assert!(cache.cache.contains_key(&s));
    }

    /// Under size pressure, `Nearby` demand is evicted before `NextTrack`
    /// demand.
    #[test]
    fn test_nearby_evicts_before_next_track() {
        let (mut cache, _tx) = test_cache("nearby-order", 1, LONG);
        let n = id("next");
        let s = id("surrounding");
        let source = MockSource {
            next_track_id: Some(n.clone()),
            next_track_surrounding_ids: vec![s.clone()],
            ..Default::default()
        };

        cache.begin_frame();
        let result = cache.update(&source);
        assert_eq!(result.evicted, vec![s.clone()]);
        assert!(cache.cache.contains_key(&n));
    }

    /// `demand()` records demand without reading: the art is fetched on the
    /// next update just as if it had been drawn.
    #[test]
    fn test_demand_only_fetches() {
        let (mut cache, _tx) = test_cache("demand-only", 10, LONG);
        let source = MockSource::default();
        let a = id("a");

        cache.begin_frame();
        cache.demand(Some(&a), Resolution::Library, CachePriority::Nearby);
        cache.update(&source);
        assert_eq!(
            source.requests.borrow().as_slice(),
            &[(a.clone(), Some(LIBRARY_ART_SIZE))]
        );
    }

    /// An id demanded both `Visible` (frame) and `NextTrack` (queue) keeps
    /// the higher priority: it is never size-evicted.
    #[test]
    fn test_priorities_merge_to_max() {
        let (mut cache, _tx) = test_cache("priority-merge", 0, LONG);
        let a = id("a");
        let source = MockSource {
            next_track_id: Some(a.clone()),
            ..Default::default()
        };

        cache.begin_frame();
        cache.get(Some(&a), Resolution::Library, CachePriority::Visible);
        // A zero-size budget forces maximum pressure; the visible entry
        // must still survive.
        let result = cache.update(&source);
        assert!(result.evicted.is_empty());
        assert!(cache.cache.contains_key(&a));
    }

    /// A full-res slot is dropped once full resolution is no longer
    /// demanded, while the still-demanded entry survives.
    #[test]
    fn test_resolution_degrades_when_undemanded() {
        let (mut cache, tx) = test_cache("degrade", 10, LONG);
        cache.resolution_stale_timeout = Duration::from_millis(30);
        let source = MockSource::default();
        let a = id("a");

        cache.begin_frame();
        cache.get(Some(&a), Resolution::Full, CachePriority::Visible);
        cache.update(&source);
        tx.send(response(&a, None)).unwrap();
        cache.update(&source);
        assert!(cache.is_resolution_loaded(&a, Resolution::Full));

        // A new frame demands only library resolution; the full slot goes
        // stale and is dropped.
        cache.begin_frame();
        cache.get(Some(&a), Resolution::Library, CachePriority::Visible);
        std::thread::sleep(Duration::from_millis(40));
        cache.update(&source);
        assert!(!cache.is_resolution_loaded(&a, Resolution::Full));
        assert!(cache.cache.contains_key(&a));
    }

    /// A demand at full resolution keeps the loaded library slot alive as
    /// its fallback.
    #[test]
    fn test_higher_demand_covers_lower_slots() {
        let (mut cache, tx) = test_cache("covers-lower", 10, LONG);
        cache.resolution_stale_timeout = Duration::from_millis(30);
        let source = MockSource::default();
        let a = id("a");

        cache.begin_frame();
        cache.get(Some(&a), Resolution::Library, CachePriority::Visible);
        cache.update(&source);
        tx.send(response(&a, Some(LIBRARY_ART_SIZE))).unwrap();
        cache.update(&source);

        // Switch to a full-res-only demand (e.g. the overlay): the library
        // slot must be kept alive as the fallback.
        cache.begin_frame();
        cache.get(Some(&a), Resolution::Full, CachePriority::Visible);
        std::thread::sleep(Duration::from_millis(40));
        cache.update(&source);
        assert!(cache.is_resolution_loaded(&a, Resolution::Library));
    }

    /// At most `FULL_RES_MAX_CACHE_SIZE` full-res slots are kept.
    #[test]
    fn test_full_res_slot_cap() {
        let (mut cache, tx) = test_cache("full-cap", 20, LONG);
        let source = MockSource::default();
        let ids: Vec<CoverArtId> = (0..FULL_RES_MAX_CACHE_SIZE + 1)
            .map(|i| id(&format!("full-{i}")))
            .collect();

        cache.begin_frame();
        for i in &ids {
            cache.get(Some(i), Resolution::Full, CachePriority::Visible);
        }
        cache.update(&source);
        for i in &ids {
            tx.send(response(i, None)).unwrap();
        }
        cache.update(&source);

        let loaded = ids
            .iter()
            .filter(|i| cache.is_resolution_loaded(i, Resolution::Full))
            .count();
        assert_eq!(loaded, FULL_RES_MAX_CACHE_SIZE);
    }

    /// A demanded id with a disk-cached thumbnail loads it into the low-res
    /// slot on the next update and reports it as an upgrade.
    #[test]
    fn test_disk_cache_seeds_low_res() {
        let (mut cache, _tx) = test_cache("disk-seed", 10, LONG);
        let source = MockSource::default();
        let a = id("a");
        std::fs::write(disk_cache_path(&cache.cache_dir, &a), [9, 9, 9]).unwrap();

        cache.begin_frame();
        assert!(
            cache
                .get(Some(&a), Resolution::Library, CachePriority::Visible)
                .is_none()
        );
        let result = cache.update(&source);
        assert!(result.upgraded.contains(&(a.clone(), Resolution::Low)));

        let got = cache
            .get(Some(&a), Resolution::Library, CachePriority::Visible)
            .expect("the disk thumbnail should be loaded");
        assert_eq!(got.resolution, Resolution::Low);
    }
}
