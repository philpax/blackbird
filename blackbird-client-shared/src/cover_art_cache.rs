use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use blackbird_core::{CoverArt, Logic, blackbird_state::CoverArtId};

const TIME_BEFORE_LOAD_ATTEMPT: Duration = Duration::from_millis(100);
const LOW_RES_CACHE_SIZE: u32 = 16;
const CACHE_DIR_NAME: &str = "album-art-cache";

/// Clients implement this to produce their own data from raw cover art bytes.
/// Called when image data first arrives or transitions to a new resolution.
pub trait ClientData: Clone {
    /// `is_high_res` is false for data loaded from the 16×16 disk cache,
    /// true for data loaded from the network.
    fn from_image_data(data: &Arc<[u8]>, cover_art_id: &CoverArtId, is_high_res: bool) -> Self;
}

pub struct CoverArtCache<T: ClientData> {
    cover_art_loaded_rx: std::sync::mpsc::Receiver<CoverArt>,
    cache: HashMap<CoverArtId, CacheEntry<T>>,
    target_size: Option<usize>,
    cache_dir: PathBuf,
    max_cache_size: usize,
    cache_entry_timeout: Duration,
}

/// Priority levels for cache entries, from highest to lowest.
/// Higher priority entries are protected from size-based eviction.
/// All entries timeout after the configured duration of not being requested, regardless of priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CachePriority {
    /// Transient art loaded while scrolling - evicted first when cache is full
    #[allow(dead_code)]
    Transient = 0,
    /// Art for albums surrounding/including the next track in queue - evicted second when cache is full
    NextTrack = 1,
    /// Currently visible art - protected from size-based eviction, but will timeout if not actively displayed
    Visible = 2,
}

struct CacheEntry<T> {
    first_requested: std::time::Instant,
    last_requested: std::time::Instant,
    state: CacheEntryState<T>,
    priority: CachePriority,
}

enum CacheEntryState<T> {
    Unloaded,
    /// Loading from network, no image available yet
    Loading,
    /// Low-res version loaded from disk cache, not yet requested from network
    LoadedLowRes {
        data: Arc<[u8]>,
        client_data: T,
    },
    /// Low-res version loaded from disk cache, high-res loading from network
    LoadingWithLowRes {
        data: Arc<[u8]>,
        client_data: T,
    },
    /// High-res version loaded from network
    Loaded {
        data: Arc<[u8]>,
        client_data: T,
    },
}

impl<T: ClientData> CoverArtCache<T> {
    pub fn new(
        cover_art_loaded_rx: std::sync::mpsc::Receiver<CoverArt>,
        target_size: Option<usize>,
        max_cache_size: usize,
        cache_entry_timeout: Duration,
    ) -> Self {
        // Get the cache directory path
        let cache_dir = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(CACHE_DIR_NAME);

        // Create the cache directory if it doesn't exist
        if let Err(e) = std::fs::create_dir_all(&cache_dir) {
            panic!("Failed to create cache directory: {e}");
        }

        Self {
            cover_art_loaded_rx,
            cache: HashMap::new(),
            target_size,
            cache_dir,
            max_cache_size,
            cache_entry_timeout,
        }
    }

    /// Process incoming cover art, evict stale/excess entries.
    /// Returns IDs of evicted entries (for client-side cleanup like `ctx.forget_image()`).
    pub fn update(&mut self) -> Vec<CoverArtId> {
        for incoming_cover_art in self.cover_art_loaded_rx.try_iter() {
            if let Some(cache_entry) = self.cache.get_mut(&incoming_cover_art.cover_art_id) {
                let data: Arc<[u8]> = incoming_cover_art.cover_art.clone().into();
                let client_data = T::from_image_data(&data, &incoming_cover_art.cover_art_id, true);
                // Save the high-res version to memory cache
                cache_entry.state = CacheEntryState::Loaded {
                    data: data.clone(),
                    client_data,
                };
                tracing::debug!("Loaded cover art for {}", incoming_cover_art.cover_art_id);

                // Save a low-res version to disk cache for future use
                // Only if it doesn't already exist
                let safe_filename = incoming_cover_art
                    .cover_art_id
                    .0
                    .replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
                let cache_path = self.cache_dir.join(format!("{}.png", safe_filename));
                if !cache_path.exists() {
                    let cover_art = incoming_cover_art.cover_art.clone();
                    let cache_path = cache_path.clone();
                    std::thread::spawn(move || {
                        save_to_disk_cache(&cache_path, &cover_art);
                    });
                }
            } else {
                tracing::debug!(
                    "Cache entry for {} not found when receiving cover art",
                    incoming_cover_art.cover_art_id
                );
            }
        }

        let mut removal_candidates = HashSet::new();

        // Remove entries that have timed out (not requested recently)
        for (cover_art_id, cache_entry) in self.cache.iter() {
            if cache_entry.last_requested.elapsed() > self.cache_entry_timeout {
                tracing::debug!(
                    "Forgetting cover art for {cover_art_id} from cache due to timeout (priority: {:?})",
                    cache_entry.priority
                );
                removal_candidates.insert(cover_art_id.clone());
            }
        }

        // Remove any entries that exceed our cache size limit
        // Evict in priority order: Transient first, then NextTrack
        // Visible items are protected from size-based eviction (but can timeout if not requested)
        let overage = self
            .cache
            .len()
            .saturating_sub(self.max_cache_size)
            .saturating_sub(removal_candidates.len());
        if overage > 0 {
            tracing::debug!("Cache overage: {overage} entries need to be evicted");

            // Collect all non-removed entries and sort by priority (lowest first), then by age (oldest first)
            let mut cache_entries_by_priority_and_age = self
                .cache
                .iter()
                .filter(|(cover_art_id, cache_entry)| {
                    !removal_candidates.contains(*cover_art_id)
                        && cache_entry.priority != CachePriority::Visible
                })
                .collect::<Vec<_>>();

            // Sort by priority (ascending, so Transient comes first), then by first_requested (oldest first)
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

        removal_candidates.into_iter().collect()
    }

    /// Get client data for a cover art entry, triggering loading if needed.
    /// Returns `None` when no image data is available yet (Unloaded/Loading states).
    pub fn get(
        &mut self,
        logic: &Logic,
        id: Option<&CoverArtId>,
        priority: CachePriority,
    ) -> Option<&T> {
        let cover_art_id = id?;

        let cache_entry = self
            .cache
            .entry(cover_art_id.clone())
            .or_insert(CacheEntry {
                first_requested: std::time::Instant::now(),
                last_requested: std::time::Instant::now(),
                state: CacheEntryState::Unloaded,
                priority,
            });

        cache_entry.last_requested = std::time::Instant::now();
        // Always update priority to match current request
        cache_entry.priority = priority;

        // Check disk cache if we haven't loaded anything yet
        if let CacheEntryState::Unloaded = cache_entry.state
            && let Some(low_res_data) = load_from_disk_cache(&self.cache_dir, cover_art_id)
        {
            let client_data = T::from_image_data(&low_res_data, cover_art_id, false);
            cache_entry.state = CacheEntryState::LoadedLowRes {
                data: low_res_data,
                client_data,
            };
        }

        // Request from network after the initial delay, if we don't have high-res yet
        if cache_entry.first_requested.elapsed() > TIME_BEFORE_LOAD_ATTEMPT {
            match &cache_entry.state {
                CacheEntryState::Unloaded => {
                    logic.request_cover_art(cover_art_id, self.target_size);
                    cache_entry.state = CacheEntryState::Loading;
                    tracing::debug!("Requesting cover art for {cover_art_id}");
                }
                CacheEntryState::LoadedLowRes { data, client_data } => {
                    let data = data.clone();
                    let client_data = client_data.clone();
                    logic.request_cover_art(cover_art_id, self.target_size);
                    cache_entry.state = CacheEntryState::LoadingWithLowRes { data, client_data };
                    tracing::debug!("Requesting cover art for {cover_art_id} (low-res cached)");
                }
                _ => {}
            }
        }

        match &cache_entry.state {
            CacheEntryState::Unloaded | CacheEntryState::Loading => None,
            CacheEntryState::LoadedLowRes { client_data, .. }
            | CacheEntryState::LoadingWithLowRes { client_data, .. }
            | CacheEntryState::Loaded { client_data, .. } => Some(client_data),
        }
    }

    /// Mutable access to the client data and raw bytes of a loaded entry.
    /// Refreshes `last_requested`. Returns `None` if entry has no loaded data.
    pub fn with_client_data_mut<R>(
        &mut self,
        id: &CoverArtId,
        f: impl FnOnce(&mut T, &Arc<[u8]>) -> R,
    ) -> Option<R> {
        let cache_entry = self.cache.get_mut(id)?;
        cache_entry.last_requested = std::time::Instant::now();

        match &mut cache_entry.state {
            CacheEntryState::LoadedLowRes { data, client_data }
            | CacheEntryState::LoadingWithLowRes { data, client_data }
            | CacheEntryState::Loaded { data, client_data } => Some(f(client_data, data)),
            _ => None,
        }
    }

    /// Preload album art for albums surrounding the next track in the queue.
    /// This ensures smooth transitions when moving to the next track.
    pub fn preload_next_track_surrounding_art(&mut self, logic: &Logic) {
        let cover_art_ids = logic.get_next_track_surrounding_cover_art_ids();

        for cover_art_id in &cover_art_ids {
            // Use get with NextTrack priority to trigger loading
            self.get(logic, Some(cover_art_id), CachePriority::NextTrack);
        }
    }
}

fn load_from_disk_cache(cache_dir: &Path, cover_art_id: &CoverArtId) -> Option<Arc<[u8]>> {
    // Sanitize the cover_art_id to make it a valid filename
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
    // Decode the image
    let Ok(img) = image::load_from_memory(image_data) else {
        tracing::warn!("Failed to decode image for {}", cache_path.display());
        return;
    };

    // Resize to low-res size
    let resized = img.resize_exact(
        LOW_RES_CACHE_SIZE,
        LOW_RES_CACHE_SIZE,
        image::imageops::FilterType::Triangle,
    );

    // Apply explicit blur to destroy high-level detail
    let blurred = image::imageops::fast_blur(&resized.into_rgb8(), 1.0);

    // Encode as PNG
    let mut buffer = std::io::Cursor::new(Vec::new());
    if let Err(e) = blurred.write_to(&mut buffer, image::ImageFormat::Png) {
        tracing::warn!(
            "Failed to encode resized image for {}: {}",
            cache_path.display(),
            e
        );
        return;
    }

    // Save to disk
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
