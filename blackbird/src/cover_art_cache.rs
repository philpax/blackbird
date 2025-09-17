use std::{borrow::Cow, collections::HashMap, sync::Arc, time::Duration};

use blackbird_core::{CoverArt, Logic};

const TIME_BEFORE_LOAD_ATTEMPT: Duration = Duration::from_millis(150);
const CACHE_ENTRY_TIMEOUT: Duration = Duration::from_secs(10);

pub struct CoverArtCache {
    cover_art_loaded_rx: std::sync::mpsc::Receiver<CoverArt>,
    cache: HashMap<String, CacheEntry>,
}
struct CacheEntry {
    first_requested: std::time::Instant,
    last_requested: std::time::Instant,
    state: CacheEntryState,
}
enum CacheEntryState {
    Unloaded,
    Loading,
    Loaded(Arc<[u8]>),
}
impl CoverArtCache {
    pub fn new(cover_art_loaded_rx: std::sync::mpsc::Receiver<CoverArt>) -> Self {
        Self {
            cover_art_loaded_rx,
            cache: HashMap::new(),
        }
    }

    pub fn update(&mut self) {
        for incoming_cover_art in self.cover_art_loaded_rx.try_iter() {
            if let Some(cache_entry) = self.cache.get_mut(&incoming_cover_art.cover_art_id) {
                cache_entry.state = CacheEntryState::Loaded(incoming_cover_art.cover_art.into());
                tracing::debug!("Loaded cover art for {}", incoming_cover_art.cover_art_id);
            } else {
                tracing::debug!(
                    "Cache entry for {} not found when receiving cover art",
                    incoming_cover_art.cover_art_id
                );
            }
        }

        let now = std::time::Instant::now();
        self.cache.retain(|cover_art_id, cache_entry| {
            let is_active = now.duration_since(cache_entry.last_requested) <= CACHE_ENTRY_TIMEOUT;
            if !is_active {
                tracing::debug!("Cache entry for {cover_art_id} timed out");
            }
            is_active
        });
    }

    pub fn get(&mut self, logic: &Logic, cover_art_id: Option<&str>) -> egui::ImageSource<'static> {
        let loading_image = egui::include_image!("../assets/no-album-art.png");
        let missing_art_image = egui::include_image!("../assets/no-album-art.png");

        let Some(cover_art_id) = cover_art_id else {
            return missing_art_image.clone();
        };

        let cache_entry = self
            .cache
            .entry(cover_art_id.to_string())
            .or_insert(CacheEntry {
                first_requested: std::time::Instant::now(),
                last_requested: std::time::Instant::now(),
                state: CacheEntryState::Unloaded,
            });

        cache_entry.last_requested = std::time::Instant::now();

        if cache_entry.first_requested.elapsed() > TIME_BEFORE_LOAD_ATTEMPT
            && let CacheEntryState::Unloaded = cache_entry.state
        {
            logic.request_cover_art(cover_art_id);
            cache_entry.state = CacheEntryState::Loading;
            tracing::debug!("Requesting cover art for {cover_art_id}");
        }

        match &cache_entry.state {
            CacheEntryState::Unloaded => loading_image.clone(),
            CacheEntryState::Loading => loading_image.clone(),
            CacheEntryState::Loaded(cover_art) => egui::ImageSource::Bytes {
                uri: Cow::Owned(format!("bytes://{cover_art_id}")),
                bytes: cover_art.clone().into(),
            },
        }
    }
}
