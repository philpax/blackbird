use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use blackbird_core::{CoverArt, Logic};

const TIME_BEFORE_LOAD_ATTEMPT: Duration = Duration::from_millis(100);
const CACHE_ENTRY_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_CACHE_SIZE: usize = 100;
const LOW_RES_CACHE_SIZE: u32 = 16;
const CACHE_DIR_NAME: &str = "album_art_cache";

pub struct CoverArtCache {
    cover_art_loaded_rx: std::sync::mpsc::Receiver<CoverArt>,
    cache: HashMap<String, CacheEntry>,
    target_size: Option<usize>,
    cache_dir: PathBuf,
}
struct CacheEntry {
    first_requested: std::time::Instant,
    last_requested: std::time::Instant,
    state: CacheEntryState,
    /// If set, this entry will never be evicted from the cache
    priority: bool,
}
enum CacheEntryState {
    Unloaded,
    /// Loading from network, no image available yet
    Loading,
    /// Low-res version loaded from disk cache, not yet requested from network
    LoadedLowRes(Arc<[u8]>),
    /// Low-res version loaded from disk cache, high-res loading from network
    LoadingWithLowRes(Arc<[u8]>),
    /// High-res version loaded from network
    Loaded(Arc<[u8]>),
}
impl CoverArtCache {
    pub fn new(
        cover_art_loaded_rx: std::sync::mpsc::Receiver<CoverArt>,
        target_size: Option<usize>,
    ) -> Self {
        // Get the cache directory path
        let cache_dir = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(CACHE_DIR_NAME);

        // Create the cache directory if it doesn't exist
        if let Err(e) = std::fs::create_dir_all(&cache_dir) {
            tracing::warn!("Failed to create cache directory: {}", e);
        } else {
            tracing::info!("Album art cache directory: {:?}", cache_dir);
        }

        Self {
            cover_art_loaded_rx,
            cache: HashMap::new(),
            target_size,
            cache_dir,
        }
    }

    pub fn update(&mut self, ctx: &egui::Context) {
        for incoming_cover_art in self.cover_art_loaded_rx.try_iter() {
            if let Some(cache_entry) = self.cache.get_mut(&incoming_cover_art.cover_art_id) {
                // Save the high-res version to memory cache
                cache_entry.state = CacheEntryState::Loaded(incoming_cover_art.cover_art.clone().into());
                tracing::debug!("Loaded cover art for {}", incoming_cover_art.cover_art_id);

                // Save a low-res version to disk cache for future use
                let cache_dir = self.cache_dir.clone();
                let cover_art_id = incoming_cover_art.cover_art_id.clone();
                let cover_art = incoming_cover_art.cover_art.clone();
                std::thread::spawn(move || {
                    save_to_disk_cache(&cache_dir, &cover_art_id, &cover_art);
                });
            } else {
                tracing::debug!(
                    "Cache entry for {} not found when receiving cover art",
                    incoming_cover_art.cover_art_id
                );
            }
        }

        let mut removal_candidates = HashSet::new();

        // Remove entries that have timed out
        for (cover_art_id, _) in self.cache.iter().filter(|(_, cache_entry)| {
            cache_entry.last_requested.elapsed() > CACHE_ENTRY_TIMEOUT && !cache_entry.priority
        }) {
            tracing::debug!("Forgetting cover art for {cover_art_id} from cache due to timeout");
            removal_candidates.insert(cover_art_id.clone());
        }

        // Remove any entries that exceed our cache size limit
        let overage = self
            .cache
            .len()
            .saturating_sub(MAX_CACHE_SIZE)
            .saturating_sub(removal_candidates.len());
        if overage > 0 {
            tracing::debug!("Forgetting {overage} cover arts from cache due to size limit");
        }
        let mut cache_entries_by_oldest = self
            .cache
            .iter()
            .filter(|(cover_art_id, cache_entry)| {
                !(removal_candidates.contains(*cover_art_id) || cache_entry.priority)
            })
            .collect::<Vec<_>>();
        cache_entries_by_oldest.sort_by_key(|(_, cache_entry)| cache_entry.first_requested);
        for (cover_art_id, _) in cache_entries_by_oldest.iter().take(overage) {
            tracing::debug!("Forgetting cover art for {cover_art_id} from cache due to size limit");
            removal_candidates.insert(cover_art_id.to_string());
        }

        self.cache
            .retain(|cover_art_id, _| !removal_candidates.contains(cover_art_id));
        for cover_art_id in removal_candidates {
            // Forget both low-res and high-res versions
            ctx.forget_image(&cover_art_id_to_url(&cover_art_id, true));
            ctx.forget_image(&cover_art_id_to_url(&cover_art_id, false));
        }
    }

    pub fn get(
        &mut self,
        logic: &Logic,
        cover_art_id: Option<&str>,
        priority: bool,
    ) -> egui::ImageSource<'static> {
        let loading_image = egui::include_image!("../assets/no-album-art.png");
        let missing_art_image = egui::include_image!("../assets/no-album-art.png");

        let Some(cover_art_id) = cover_art_id else {
            return missing_art_image;
        };

        let cache_entry = self
            .cache
            .entry(cover_art_id.to_string())
            .or_insert(CacheEntry {
                first_requested: std::time::Instant::now(),
                last_requested: std::time::Instant::now(),
                state: CacheEntryState::Unloaded,
                priority,
            });

        cache_entry.last_requested = std::time::Instant::now();
        cache_entry.priority = priority;

        // Check disk cache if we haven't loaded anything yet
        if let CacheEntryState::Unloaded = cache_entry.state
            && let Some(low_res_data) = load_from_disk_cache(&self.cache_dir, cover_art_id)
        {
            cache_entry.state = CacheEntryState::LoadedLowRes(low_res_data);
        }

        // Request from network after the initial delay, if we don't have high-res yet
        if cache_entry.first_requested.elapsed() > TIME_BEFORE_LOAD_ATTEMPT {
            match &cache_entry.state {
                CacheEntryState::Unloaded => {
                    logic.request_cover_art(cover_art_id, self.target_size);
                    cache_entry.state = CacheEntryState::Loading;
                    tracing::debug!("Requesting cover art for {cover_art_id}");
                }
                CacheEntryState::LoadedLowRes(data) => {
                    logic.request_cover_art(cover_art_id, self.target_size);
                    cache_entry.state = CacheEntryState::LoadingWithLowRes(data.clone());
                    tracing::debug!("Requesting cover art for {cover_art_id} (low-res cached)");
                }
                _ => {}
            }
        }

        match &cache_entry.state {
            CacheEntryState::Unloaded | CacheEntryState::Loading => loading_image,
            CacheEntryState::LoadedLowRes(cover_art)
            | CacheEntryState::LoadingWithLowRes(cover_art) => egui::ImageSource::Bytes {
                uri: Cow::Owned(cover_art_id_to_url(cover_art_id, true)),
                bytes: cover_art.clone().into(),
            },
            CacheEntryState::Loaded(cover_art) => egui::ImageSource::Bytes {
                uri: Cow::Owned(cover_art_id_to_url(cover_art_id, false)),
                bytes: cover_art.clone().into(),
            },
        }
    }
}

fn cover_art_id_to_url(cover_art_id: &str, is_low_res: bool) -> String {
    if is_low_res {
        format!("bytes://low-res/{cover_art_id}")
    } else {
        format!("bytes://{cover_art_id}")
    }
}

fn get_cache_path(cache_dir: &Path, cover_art_id: &str) -> PathBuf {
    // Sanitize the cover_art_id to make it a valid filename
    let safe_filename = cover_art_id.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
    cache_dir.join(format!("{}.png", safe_filename))
}

fn load_from_disk_cache(cache_dir: &Path, cover_art_id: &str) -> Option<Arc<[u8]>> {
    let path = get_cache_path(cache_dir, cover_art_id);
    match std::fs::read(&path) {
        Ok(data) => {
            tracing::debug!("Loaded low-res cover art for {} from disk cache", cover_art_id);
            Some(data.into())
        }
        Err(_) => None,
    }
}

fn save_to_disk_cache(cache_dir: &Path, cover_art_id: &str, image_data: &[u8]) {
    // Decode the image
    let Ok(img) = image::load_from_memory(image_data) else {
        tracing::warn!("Failed to decode image for {}", cover_art_id);
        return;
    };

    // Resize to low-res size
    let resized = img.resize_exact(
        LOW_RES_CACHE_SIZE,
        LOW_RES_CACHE_SIZE,
        image::imageops::FilterType::Lanczos3,
    );

    // Encode as PNG
    let mut buffer = std::io::Cursor::new(Vec::new());
    if let Err(e) = resized.write_to(&mut buffer, image::ImageFormat::Png) {
        tracing::warn!("Failed to encode resized image for {}: {}", cover_art_id, e);
        return;
    }

    // Save to disk
    let path = get_cache_path(cache_dir, cover_art_id);
    if let Err(e) = std::fs::write(&path, buffer.into_inner()) {
        tracing::warn!("Failed to save low-res cover art for {} to disk: {}", cover_art_id, e);
    } else {
        tracing::debug!("Saved low-res cover art for {} to disk cache", cover_art_id);
    }
}
