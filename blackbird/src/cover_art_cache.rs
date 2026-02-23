use std::{borrow::Cow, sync::Arc, time::Duration};

use blackbird_client_shared::cover_art_cache::{self, ClientData};
use blackbird_core::{CoverArt, Logic, blackbird_state::CoverArtId};

pub use cover_art_cache::CachePriority;

const MAX_CACHE_SIZE: usize = 100;
const CACHE_ENTRY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct EguiCoverArt {
    pub image_source: egui::ImageSource<'static>,
}

impl ClientData for EguiCoverArt {
    fn from_image_data(data: &Arc<[u8]>, id: &CoverArtId, is_high_res: bool) -> Self {
        let uri = if is_high_res {
            format!("bytes://{}", id.0)
        } else {
            format!("bytes://low-res/{}", id.0)
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
    pub fn new(
        cover_art_loaded_rx: std::sync::mpsc::Receiver<CoverArt>,
        target_size: Option<usize>,
    ) -> Self {
        Self {
            inner: cover_art_cache::CoverArtCache::new(
                cover_art_loaded_rx,
                target_size,
                MAX_CACHE_SIZE,
                CACHE_ENTRY_TIMEOUT,
            ),
        }
    }

    pub fn update(&mut self, ctx: &egui::Context) {
        let evicted = self.inner.update();
        for cover_art_id in evicted {
            // Forget both low-res and high-res versions
            ctx.forget_image(&format!("bytes://low-res/{}", cover_art_id.0));
            ctx.forget_image(&format!("bytes://{}", cover_art_id.0));
        }
    }

    pub fn get(
        &mut self,
        logic: &Logic,
        cover_art_id: Option<&CoverArtId>,
        priority: CachePriority,
    ) -> egui::ImageSource<'static> {
        match self.inner.get(logic, cover_art_id, priority) {
            Some(egui_data) => egui_data.image_source.clone(),
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
