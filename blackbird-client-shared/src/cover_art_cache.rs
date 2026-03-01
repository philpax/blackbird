//! Cover art cache shared between the egui and TUI clients.
//!
//! Each cache entry holds up to three resolution tiers (low, library, full)
//! that load on demand and degrade when no longer actively requested.
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use blackbird_core::{CoverArt, Logic, blackbird_state::CoverArtId};

const TIME_BEFORE_LOAD_ATTEMPT: Duration = Duration::from_millis(100);
const LOW_RES_CACHE_SIZE: u32 = 16;
const CACHE_DIR_NAME: &str = "album-art-cache";

/// How long a higher-resolution slot can go unrequested before being dropped.
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

/// Priority levels for cache entries, from highest to lowest.
/// Higher priority entries are protected from size-based eviction.
/// All entries timeout after the configured duration of not being requested, regardless of priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CachePriority {
    /// Transient art loaded while scrolling — evicted first when cache is full.
    Transient = 0,
    /// Art for albums surrounding/including the next track in queue — evicted second when cache is full.
    NextTrack = 1,
    /// Currently visible art — protected from size-based eviction, but will timeout if not actively displayed.
    Visible = 2,
}

struct ImageData<T> {
    data: Arc<[u8]>,
    client_data: T,
    last_requested: Instant,
}

struct CacheEntry<T> {
    low_res: Option<ImageData<T>>,
    library_res: Option<ImageData<T>>,
    full_res: Option<ImageData<T>>,
    /// Resolutions currently being loaded from the network.
    loading: HashSet<Resolution>,
    first_requested: Instant,
    last_requested: Instant,
    priority: CachePriority,
}

impl<T> CacheEntry<T> {
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
    prefetcher: BackgroundPrefetcher,
}

impl<T: ClientData> CoverArtCache<T> {
    pub fn new(
        cover_art_loaded_rx: std::sync::mpsc::Receiver<CoverArt>,
        max_cache_size: usize,
        cache_entry_timeout: Duration,
    ) -> Self {
        // Get the cache directory path.
        let cache_dir = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(CACHE_DIR_NAME);

        // Create the cache directory if it doesn't exist.
        if let Err(e) = std::fs::create_dir_all(&cache_dir) {
            panic!("Failed to create cache directory: {e}");
        }

        Self {
            cover_art_loaded_rx,
            cache: HashMap::new(),
            cache_dir,
            max_cache_size,
            cache_entry_timeout,
            prefetcher: BackgroundPrefetcher::new(),
        }
    }

    /// Process incoming cover art, evict stale/excess entries, and degrade
    /// unused higher-resolution slots. Returns evicted entry IDs and newly
    /// populated resolution upgrades.
    pub fn update(&mut self) -> UpdateResult {
        let mut upgraded = Vec::new();

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

            let now = Instant::now();
            *entry.slot_mut(resolution) = Some(ImageData {
                data: data.clone(),
                client_data,
                last_requested: now,
            });

            tracing::debug!(
                "Loaded {:?} cover art for {}",
                resolution,
                incoming.cover_art_id
            );

            upgraded.push((incoming.cover_art_id.clone(), resolution));

            // Save a low-res version to disk cache for future use.
            let safe_filename = incoming
                .cover_art_id
                .0
                .replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
            let cache_path = self.cache_dir.join(format!("{}.png", safe_filename));
            if !cache_path.exists() {
                let cover_art = incoming.cover_art.clone();
                let cache_path = cache_path.clone();
                std::thread::spawn(move || {
                    save_to_disk_cache(&cache_path, &cover_art);
                });
            }
        }

        // Degrade stale higher-resolution slots.
        for entry in self.cache.values_mut() {
            if let Some(ref slot) = entry.full_res
                && slot.last_requested.elapsed() > RESOLUTION_STALE_TIMEOUT
            {
                entry.full_res = None;
            }
            if let Some(ref slot) = entry.library_res
                && slot.last_requested.elapsed() > RESOLUTION_STALE_TIMEOUT
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
                    .map(|slot| (id.clone(), slot.last_requested))
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

        // Evict entire entries by timeout.
        let mut removal_candidates = HashSet::new();
        for (cover_art_id, cache_entry) in self.cache.iter() {
            if cache_entry.last_requested.elapsed() > self.cache_entry_timeout {
                tracing::debug!(
                    "Forgetting cover art for {cover_art_id} from cache due to timeout (priority: {:?})",
                    cache_entry.priority
                );
                removal_candidates.insert(cover_art_id.clone());
            }
        }

        // Evict entries that exceed the cache size limit.
        let overage = self
            .cache
            .len()
            .saturating_sub(self.max_cache_size)
            .saturating_sub(removal_candidates.len());
        if overage > 0 {
            tracing::debug!("Cache overage: {overage} entries need to be evicted");

            let mut cache_entries_by_priority_and_age = self
                .cache
                .iter()
                .filter(|(cover_art_id, cache_entry)| {
                    !removal_candidates.contains(*cover_art_id)
                        && cache_entry.priority != CachePriority::Visible
                })
                .collect::<Vec<_>>();

            cache_entries_by_priority_and_age.sort_by_key(|(_, cache_entry)| {
                (cache_entry.priority, cache_entry.first_requested)
            });

            for (cover_art_id, cache_entry) in
                cache_entries_by_priority_and_age.iter().take(overage)
            {
                tracing::debug!(
                    "Forgetting cover art for {cover_art_id} from cache due to size limit (priority: {:?})",
                    cache_entry.priority
                );
                removal_candidates.insert((*cover_art_id).clone());
            }
        }

        self.cache
            .retain(|cover_art_id, _| !removal_candidates.contains(cover_art_id));

        UpdateResult {
            evicted: removal_candidates.into_iter().collect(),
            upgraded,
        }
    }

    /// Get the best available client data for a cover art entry, triggering
    /// loading at the requested resolution if needed.
    /// Returns `None` when no image data is available yet.
    pub fn get(
        &mut self,
        logic: &Logic,
        id: Option<&CoverArtId>,
        resolution: Resolution,
        priority: CachePriority,
    ) -> Option<GetResult<'_, T>> {
        let cover_art_id = id?;

        let now = Instant::now();
        let entry = self
            .cache
            .entry(cover_art_id.clone())
            .or_insert(CacheEntry {
                low_res: None,
                library_res: None,
                full_res: None,
                loading: HashSet::new(),
                first_requested: now,
                last_requested: now,
                priority,
            });

        entry.last_requested = now;
        entry.priority = priority;

        // Load from disk cache into the low_res slot if empty.
        if entry.low_res.is_none()
            && let Some(low_res_data) = load_from_disk_cache(&self.cache_dir, cover_art_id)
        {
            let client_data = T::from_image_data(&low_res_data, cover_art_id, Resolution::Low);
            entry.low_res = Some(ImageData {
                data: low_res_data,
                client_data,
                last_requested: now,
            });
        }

        // Request from network based on the exact resolution requested.
        // Library and Full are independent — requesting Full does NOT also
        // request Library, because the low-res fallback is sufficient while
        // waiting and the extra request would compete with the prefetch queue.
        match resolution {
            Resolution::Low => {}
            Resolution::Library => {
                if entry.library_res.is_none()
                    && !entry.loading.contains(&Resolution::Library)
                    && entry.first_requested.elapsed() > TIME_BEFORE_LOAD_ATTEMPT
                {
                    logic.request_cover_art(cover_art_id, Some(LIBRARY_ART_SIZE));
                    entry.loading.insert(Resolution::Library);
                    tracing::debug!("Requesting library-res cover art for {cover_art_id}");
                }
            }
            Resolution::Full => {
                if entry.full_res.is_none() && !entry.loading.contains(&Resolution::Full) {
                    logic.request_cover_art(cover_art_id, None);
                    entry.loading.insert(Resolution::Full);
                    tracing::debug!("Requesting full-res cover art for {cover_art_id}");
                }
            }
        }

        // Touch last_requested on the returned slot.
        if let Some((_, res)) = entry.best_up_to(resolution)
            && let Some(slot) = entry.slot_mut(res)
        {
            slot.last_requested = now;
        }

        let (data, res) = entry.best_up_to(resolution)?;
        Some(GetResult {
            data,
            resolution: res,
        })
    }

    /// Read-only access to the client data at a specific resolution tier.
    /// No side effects — does not trigger loading or touch timestamps.
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
    /// resolution. Refreshes `last_requested`. Returns `None` if entry has
    /// no loaded data.
    pub fn with_client_data_mut<R>(
        &mut self,
        id: &CoverArtId,
        f: impl FnOnce(&mut T, &Arc<[u8]>) -> R,
    ) -> Option<R> {
        let entry = self.cache.get_mut(id)?;
        entry.last_requested = Instant::now();

        // Try full > library > low.
        for res in [Resolution::Full, Resolution::Library, Resolution::Low] {
            if let Some(slot) = entry.slot_mut(res) {
                return Some(f(&mut slot.client_data, &slot.data));
            }
        }
        None
    }

    /// Mutable access to the client data and raw bytes at a specific resolution
    /// tier. Returns `None` if the slot is not populated.
    pub fn with_client_data_mut_at<R>(
        &mut self,
        id: &CoverArtId,
        resolution: Resolution,
        f: impl FnOnce(&mut T, &Arc<[u8]>) -> R,
    ) -> Option<R> {
        let entry = self.cache.get_mut(id)?;
        entry.last_requested = Instant::now();
        if let Some(slot) = entry.slot_mut(resolution) {
            Some(f(&mut slot.client_data, &slot.data))
        } else {
            None
        }
    }

    /// Preload album art for albums surrounding the next track in the queue.
    /// This ensures smooth transitions when moving to the next track.
    pub fn preload_next_track_surrounding_art(&mut self, logic: &Logic) {
        let cover_art_ids = logic.get_next_track_surrounding_cover_art_ids();

        for cover_art_id in &cover_art_ids {
            // Use get with NextTrack priority to trigger loading at library resolution.
            self.get(
                logic,
                Some(cover_art_id),
                Resolution::Library,
                CachePriority::NextTrack,
            );
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
                let safe_filename =
                    id.0.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
                let cache_path = self.cache_dir.join(format!("{safe_filename}.png"));
                !cache_path.exists()
            })
            .collect();
        self.prefetcher.populate(ids);
    }

    /// Advance the background prefetcher by one tick, fetching at most one
    /// cover art ID if enough time has elapsed since the last request.
    pub fn tick_prefetch(&mut self, logic: &Logic) {
        let Some(id) = self.prefetcher.tick() else {
            return;
        };

        // Create a cache entry so that `update()` can match the incoming
        // response and save it to the disk cache. Request immediately,
        // bypassing the normal `TIME_BEFORE_LOAD_ATTEMPT` delay.
        let now = Instant::now();
        let entry = self.cache.entry(id.clone()).or_insert(CacheEntry {
            low_res: None,
            library_res: None,
            full_res: None,
            loading: HashSet::new(),
            first_requested: now,
            last_requested: now,
            priority: CachePriority::Transient,
        });

        if entry.library_res.is_none() && !entry.loading.contains(&Resolution::Library) {
            logic.request_cover_art(&id, Some(LIBRARY_ART_SIZE));
            entry.loading.insert(Resolution::Library);
        }
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

fn load_from_disk_cache(cache_dir: &Path, cover_art_id: &CoverArtId) -> Option<Arc<[u8]>> {
    // Sanitize the cover_art_id to make it a valid filename.
    let safe_filename = cover_art_id
        .0
        .replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
    let path = cache_dir.join(format!("{}.png", safe_filename));
    match std::fs::read(&path) {
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
