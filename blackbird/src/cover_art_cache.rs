use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use blackbird_core::{CoverArt, Logic};

const TIME_BEFORE_LOAD_ATTEMPT: Duration = Duration::from_millis(100);
const CACHE_ENTRY_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_CACHE_SIZE: usize = 100;

pub struct CoverArtCache {
    cover_art_loaded_rx: std::sync::mpsc::Receiver<CoverArt>,
    cache: HashMap<String, CacheEntry>,
    target_size: Option<usize>,
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
    pub fn new(
        cover_art_loaded_rx: std::sync::mpsc::Receiver<CoverArt>,
        target_size: Option<usize>,
    ) -> Self {
        Self {
            cover_art_loaded_rx,
            cache: HashMap::new(),
            target_size,
        }
    }

    pub fn update(&mut self, ctx: &egui::Context) {
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

        let mut removal_candidates = HashSet::new();

        // Remove entries that have timed out
        for (cover_art_id, _) in self
            .cache
            .iter()
            .filter(|(_, cache_entry)| cache_entry.last_requested.elapsed() > CACHE_ENTRY_TIMEOUT)
        {
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
            .map(|(cover_art_id, cache_entry)| (cover_art_id, cache_entry.first_requested))
            .filter(|(cover_art_id, _)| !removal_candidates.contains(*cover_art_id))
            .collect::<Vec<_>>();
        cache_entries_by_oldest.sort_by_key(|(_, first_requested)| *first_requested);
        for (cover_art_id, _) in cache_entries_by_oldest.iter().take(overage) {
            tracing::debug!("Forgetting cover art for {cover_art_id} from cache due to size limit");
            removal_candidates.insert(cover_art_id.to_string());
        }

        self.cache
            .retain(|cover_art_id, _| !removal_candidates.contains(cover_art_id));
        for cover_art_id in removal_candidates {
            ctx.forget_image(&cover_art_id_to_url(&cover_art_id));
        }
    }

    pub fn get(&mut self, logic: &Logic, cover_art_id: Option<&str>) -> egui::ImageSource<'static> {
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
            });

        cache_entry.last_requested = std::time::Instant::now();

        if cache_entry.first_requested.elapsed() > TIME_BEFORE_LOAD_ATTEMPT
            && let CacheEntryState::Unloaded = cache_entry.state
        {
            logic.request_cover_art(cover_art_id, self.target_size);
            cache_entry.state = CacheEntryState::Loading;
            tracing::debug!("Requesting cover art for {cover_art_id}");
        }

        match &cache_entry.state {
            CacheEntryState::Unloaded => loading_image,
            CacheEntryState::Loading => loading_image,
            CacheEntryState::Loaded(cover_art) => egui::ImageSource::Bytes {
                uri: Cow::Owned(cover_art_id_to_url(cover_art_id)),
                bytes: cover_art.clone().into(),
            },
        }
    }
}

fn cover_art_id_to_url(cover_art_id: &str) -> String {
    format!("bytes://{cover_art_id}")
}
