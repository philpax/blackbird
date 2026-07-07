use std::{borrow::Cow, sync::Arc, time::Duration};

use blackbird_client_shared::cover_art_cache::{self, ClientData, Resolution};
use blackbird_core::{CoverArt, Logic, blackbird_state::CoverArtId};

pub use cover_art_cache::CachePriority;

/// Sized for the demand set: up to three pages of albums (the viewport plus
/// a `Nearby` page either side) and the next-track neighbourhood, with
/// headroom for recently offscreen art.
const MAX_CACHE_SIZE: usize = 150;
const CACHE_ENTRY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct EguiCoverArt {
    pub image_source: egui::ImageSource<'static>,
}

impl ClientData for EguiCoverArt {
    fn from_image_data(data: &Arc<[u8]>, id: &CoverArtId, resolution: Resolution) -> Self {
        let uri = match resolution {
            Resolution::Low => format!("bytes://low-res/{}", id.0),
            Resolution::Library => format!("bytes://library/{}", id.0),
            Resolution::Full => format!("bytes://full/{}", id.0),
        };
        EguiCoverArt {
            image_source: egui::ImageSource::Bytes {
                uri: Cow::Owned(uri),
                bytes: data.clone().into(),
            },
        }
    }
}

pub struct CoverArtCache {
    inner: cover_art_cache::CoverArtCache<EguiCoverArt>,
}

impl CoverArtCache {
    pub fn new(cover_art_loaded_rx: std::sync::mpsc::Receiver<CoverArt>) -> Self {
        Self {
            inner: cover_art_cache::CoverArtCache::new(
                cover_art_loaded_rx,
                MAX_CACHE_SIZE,
                CACHE_ENTRY_TIMEOUT,
            ),
        }
    }

    /// Start a new demand frame. Call at the start of each egui frame,
    /// before any drawing; the draw's `get` calls then rebuild the demand
    /// set from what is actually displayed.
    pub fn begin_frame(&mut self) {
        self.inner.begin_frame();
    }

    /// Reconcile the cache against the demand accumulated by the previous
    /// frame's draw: fetch missing art, evict undemanded art, and advance
    /// the background prefetcher.
    pub fn update(&mut self, ctx: &egui::Context, logic: &Logic) {
        let result = self.inner.update(logic);
        for cover_art_id in result.evicted {
            // Forget all three URI variants.
            ctx.forget_image(&format!("bytes://low-res/{}", cover_art_id.0));
            ctx.forget_image(&format!("bytes://library/{}", cover_art_id.0));
            ctx.forget_image(&format!("bytes://full/{}", cover_art_id.0));
        }
    }

    /// Record a `Nearby` demand for library-resolution art: albums just
    /// outside the viewport, kept warm so scrolling doesn't flash
    /// placeholder art.
    pub fn demand_nearby(&mut self, cover_art_id: Option<&CoverArtId>) {
        self.inner
            .demand(cover_art_id, Resolution::Library, CachePriority::Nearby);
    }

    /// Record demand for library-resolution art and return the best
    /// already-loaded image source. Fetching happens in [`update`](Self::update).
    pub fn get(
        &mut self,
        cover_art_id: Option<&CoverArtId>,
        priority: CachePriority,
    ) -> egui::ImageSource<'static> {
        match self.inner.get(cover_art_id, Resolution::Library, priority) {
            Some(result) => result.data.image_source.clone(),
            None => egui::include_image!("../assets/no-album-art.png"),
        }
    }

    /// Get the full-resolution version of cover art for overlays.
    /// Falls back to lower resolutions while full-res is loading.
    pub fn get_full_res(
        &mut self,
        cover_art_id: Option<&CoverArtId>,
    ) -> egui::ImageSource<'static> {
        match self
            .inner
            .get(cover_art_id, Resolution::Full, CachePriority::Visible)
        {
            Some(result) => result.data.image_source.clone(),
            None => egui::include_image!("../assets/no-album-art.png"),
        }
    }

    pub fn populate_prefetch_queue(&mut self, cover_art_ids: Vec<CoverArtId>) {
        self.inner.populate_prefetch_queue(cover_art_ids);
    }
}
