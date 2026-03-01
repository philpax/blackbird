use std::{borrow::Cow, sync::Arc, time::Duration};

use blackbird_client_shared::cover_art_cache::{self, ClientData, Resolution};
use blackbird_core::{CoverArt, Logic, blackbird_state::CoverArtId};

pub use cover_art_cache::CachePriority;

const MAX_CACHE_SIZE: usize = 100;
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

    pub fn update(&mut self, ctx: &egui::Context) {
        let result = self.inner.update();
        for cover_art_id in result.evicted {
            // Forget all three URI variants.
            ctx.forget_image(&format!("bytes://low-res/{}", cover_art_id.0));
            ctx.forget_image(&format!("bytes://library/{}", cover_art_id.0));
            ctx.forget_image(&format!("bytes://full/{}", cover_art_id.0));
        }
    }

    pub fn get(
        &mut self,
        logic: &Logic,
        cover_art_id: Option<&CoverArtId>,
        priority: CachePriority,
    ) -> egui::ImageSource<'static> {
        match self
            .inner
            .get(logic, cover_art_id, Resolution::Library, priority)
        {
            Some(result) => result.data.image_source.clone(),
            None => egui::include_image!("../assets/no-album-art.png"),
        }
    }

    /// Get the full-resolution version of cover art for overlays.
    /// Falls back to lower resolutions while full-res is loading.
    pub fn get_full_res(
        &mut self,
        logic: &Logic,
        cover_art_id: Option<&CoverArtId>,
    ) -> egui::ImageSource<'static> {
        match self.inner.get(
            logic,
            cover_art_id,
            Resolution::Full,
            CachePriority::Visible,
        ) {
            Some(result) => result.data.image_source.clone(),
            None => egui::include_image!("../assets/no-album-art.png"),
        }
    }

    pub fn preload_next_track_surrounding_art(&mut self, logic: &Logic) {
        self.inner.preload_next_track_surrounding_art(logic);
    }

    pub fn populate_prefetch_queue(&mut self, cover_art_ids: Vec<CoverArtId>) {
        self.inner.populate_prefetch_queue(cover_art_ids);
    }

    pub fn tick_prefetch(&mut self, logic: &Logic) {
        self.inner.tick_prefetch(logic);
    }
}
