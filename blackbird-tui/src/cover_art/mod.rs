//! Cover art caching and derived-artifact management for the TUI.
//!
//! Wraps the shared [`blackbird_client_shared::cover_art_cache`] cache
//! (which owns fetching, resolution tiers, and eviction) with the
//! TUI-specific artifacts derived from the raw image bytes: quantized color
//! grids for half-block rendering ([`quantize`]), and ratatui-image
//! protocols for terminals with a graphics protocol. Each artifact family
//! lives in a [`derived_cache::DerivedCache`] keyed by cover art id and
//! render size, which encodes the shared compute/upgrade/eviction
//! lifecycle; background work runs on the [`pool`] worker threads.

mod derived_cache;
mod pool;
mod quantize;

pub use quantize::{
    ArtColorGrid, ArtColors, QuadrantColors, compute_art_grid, compute_quadrant_colors,
};

use std::{collections::HashSet, sync::Arc, time::Duration};

use blackbird_client_shared::cover_art_cache::{self, CachePriority, ClientData, Resolution};
use blackbird_core::{CoverArt, Logic, blackbird_state::CoverArtId};
use ratatui::layout::Size;
use ratatui_image::{Resize, picker::Picker, protocol::Protocol, sliced::SlicedProtocol};

use derived_cache::DerivedCache;
use pool::ThreadPool;
use quantize::image_aspect_ratio;

const POOL_SIZE: usize = 4;
const MAX_CACHE_SIZE: usize = 50;
const CACHE_ENTRY_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub struct TuiCoverArt {
    raw_bytes: Arc<[u8]>,
    /// Aspect ratio (height / width) of the source image. Computed from the
    /// image header so it is available without a full decode.
    aspect_ratio: Option<f64>,
}

impl ClientData for TuiCoverArt {
    fn from_image_data(data: &Arc<[u8]>, _id: &CoverArtId, _resolution: Resolution) -> Self {
        TuiCoverArt {
            raw_bytes: data.clone(),
            aspect_ratio: image_aspect_ratio(data),
        }
    }

    fn carry_over(&mut self, previous: &Self) {
        if self.aspect_ratio.is_none() {
            self.aspect_ratio = previous.aspect_ratio;
        }
    }
}

/// Cache key for color grids: the cover art id and the grid dimensions in
/// half-block pixels.
type GridKey = (CoverArtId, usize, usize);

/// Cache key for image protocols: the cover art id and the target render
/// size in character cells.
type ProtocolKey = (CoverArtId, u16, u16);

pub struct CoverArtCache {
    inner: cover_art_cache::CoverArtCache<TuiCoverArt>,
    pool: ThreadPool,
    /// 4×4 thumbnail colors for `LeftOfAlbum` mode. Always computed
    /// synchronously from the 16px low-res image, so this cache is only ever
    /// seeded, never spawns background computes.
    colors: DerivedCache<CoverArtId, QuadrantColors>,
    /// Color grids for the library BelowAlbum art and the overlay, keyed by
    /// `(CoverArtId, cols, rows)`.
    grids: DerivedCache<GridKey, ArtColorGrid>,
    /// `(CoverArtId, Resolution)` pairs requested with `Visible` priority
    /// since the last `begin_frame()`. The TUI uses lazy redraw, so entries
    /// in this set are touched every tick via `keep_visible_alive()` to keep
    /// the cache from timing them out while no draws are happening (e.g.
    /// while paused), which would otherwise cause a high-res → low-res →
    /// high-res flicker on the next redraw.
    visible_this_frame: HashSet<(CoverArtId, Resolution)>,

    // ── Image protocol (graphics protocol) caches ──────────────────────────
    /// The terminal graphics protocol picker, set via [`set_picker`].
    /// `None` when `AlbumArtProtocol::Halfblock` is configured, disabling
    /// all protocol-based rendering.
    ///
    /// [`set_picker`]: CoverArtCache::set_picker
    protocol_picker: Option<Picker>,
    /// Fixed-size protocols for the now-playing thumbnail, library
    /// thumbnails, and the overlay, keyed by `(CoverArtId, width, height)`
    /// in character cells so that different render sizes get distinct
    /// protocols.
    protocols: DerivedCache<ProtocolKey, Protocol>,
    /// Sliced protocols for the library BelowAlbum art (scrollable). Keyed
    /// like `protocols`; the art area dimensions derive from the terminal
    /// size (`large_art_cols()`), so a resize produces new keys and
    /// recomputes the art.
    sliced_protocols: DerivedCache<ProtocolKey, SlicedProtocol>,
}

impl CoverArtCache {
    pub fn new(cover_art_loaded_rx: std::sync::mpsc::Receiver<CoverArt>) -> Self {
        Self {
            inner: cover_art_cache::CoverArtCache::new(
                cover_art_loaded_rx,
                MAX_CACHE_SIZE,
                CACHE_ENTRY_TIMEOUT,
            ),
            pool: ThreadPool::new(POOL_SIZE),
            colors: DerivedCache::new("thumbnail colors"),
            grids: DerivedCache::new("color grid"),
            visible_this_frame: HashSet::new(),
            protocol_picker: None,
            protocols: DerivedCache::new("image protocol"),
            sliced_protocols: DerivedCache::new("sliced image protocol"),
        }
    }

    /// Sets the graphics protocol picker, enabling image-protocol rendering.
    /// Called after terminal setup in `main.rs`. Pass `None` to disable
    /// protocol-based rendering (used when `AlbumArtProtocol::Halfblock`).
    pub fn set_picker(&mut self, picker: Option<Picker>) {
        self.protocol_picker = picker;
    }

    /// Returns `true` if a graphics-protocol picker is available.
    pub fn has_picker(&self) -> bool {
        self.protocol_picker.is_some()
    }

    /// Reset the per-frame visibility set. Call once at the start of each
    /// draw; the `get*` methods then re-populate it as visible art is
    /// requested.
    pub fn begin_frame(&mut self) {
        self.visible_this_frame.clear();
    }

    /// Refresh `last_requested` on every entry that was requested with
    /// `Visible` priority during the most recent draw. The TUI only redraws
    /// when something changes, so without this the cache would time out
    /// visible art after `CACHE_ENTRY_TIMEOUT` whenever the UI is idle.
    pub fn keep_visible_alive(&mut self) {
        for (id, resolution) in &self.visible_this_frame {
            self.inner.touch_for_keepalive(id, *resolution);
        }
    }

    /// Processes incoming cover art data and color/grid computations.
    /// Returns `true` if any visual state changed.
    pub fn update(&mut self) -> bool {
        let mut changed = false;

        let result = self.inner.update();
        if !result.evicted.is_empty() || !result.upgraded.is_empty() {
            changed = true;
        }
        for id in &result.evicted {
            self.colors.evict_matching(|color_id| color_id == id);
            self.grids.evict_matching(|(grid_id, _, _)| grid_id == id);
            // Remove protocol caches for this id. Kitty virtual placements
            // are removed by the terminal itself once their unicode
            // placeholders stop being drawn; the transmitted image data
            // persists until the terminal evicts it from its own store.
            self.protocols
                .evict_matching(|(proto_id, _, _)| proto_id == id);
            self.sliced_protocols
                .evict_matching(|(sliced_id, _, _)| sliced_id == id);
        }

        // Upgraded entries need no explicit invalidation: `changed` above
        // forces a redraw, and the `get*` methods compare the cached source
        // resolution against the best available bytes on every call.

        changed |= self.colors.drain();
        changed |= self.grids.drain();
        changed |= self.protocols.drain();
        changed |= self.sliced_protocols.drain();

        changed
    }

    /// Get quadrant colors for a cover art entry at low resolution.
    /// Used for LeftOfAlbum thumbnail colors.
    pub fn get(&mut self, logic: &Logic, cover_art_id: Option<&CoverArtId>) -> QuadrantColors {
        let Some(id) = cover_art_id else {
            return QuadrantColors::default();
        };

        self.visible_this_frame
            .insert((id.clone(), Resolution::Low));
        let _ = self
            .inner
            .get(logic, Some(id), Resolution::Low, CachePriority::Visible);

        // The colors always come from the 16px low-res image, which is
        // trivially cheap to process — compute synchronously so the first
        // frame shows colors instead of gray.
        if !self.colors.has_value(id)
            && let Some(low_res) = self
                .inner
                .get_resolution(id, Resolution::Low)
                .map(|data| data.raw_bytes.clone())
        {
            self.colors.insert(
                id.clone(),
                Resolution::Low,
                Arc::new(compute_quadrant_colors(&low_res)),
            );
        }

        self.colors
            .get(id)
            .map(|colors| *colors)
            .unwrap_or_default()
    }

    /// Returns a variable-size color grid for library BelowAlbum display.
    /// Requests library-resolution data and computes a grid in a background
    /// thread; returns a fallback grid while computing.
    pub fn get_art_grid(
        &mut self,
        logic: &Logic,
        cover_art_id: Option<&CoverArtId>,
        cols: usize,
        rows: usize,
    ) -> (Arc<ArtColorGrid>, bool) {
        self.art_grid_at(logic, cover_art_id, cols, rows, Resolution::Library)
    }

    /// Returns a variable-size color grid for the overlay using full-resolution
    /// data. Falls back to lower resolutions while full-res is loading.
    pub fn get_full_res_art_grid(
        &mut self,
        logic: &Logic,
        cover_art_id: Option<&CoverArtId>,
        cols: usize,
        rows: usize,
    ) -> (Arc<ArtColorGrid>, bool) {
        self.art_grid_at(logic, cover_art_id, cols, rows, Resolution::Full)
    }

    /// Returns a color grid computed from the best available data at or
    /// below `resolution`, requesting that resolution from the server. The
    /// boolean is `true` while better data is loading or a recompute is in
    /// flight.
    fn art_grid_at(
        &mut self,
        logic: &Logic,
        cover_art_id: Option<&CoverArtId>,
        cols: usize,
        rows: usize,
        resolution: Resolution,
    ) -> (Arc<ArtColorGrid>, bool) {
        let Some(id) = cover_art_id else {
            return (Arc::new(ArtColorGrid::empty(cols, rows)), false);
        };

        self.visible_this_frame.insert((id.clone(), resolution));

        // Request data at the target resolution (triggers a network fetch if
        // needed).
        let _ = self
            .inner
            .get(logic, Some(id), resolution, CachePriority::Visible);

        let key = (id.clone(), cols, rows);

        // Seed the cache synchronously from the low-res image (16×16 pixels,
        // sub-millisecond) so the first frame shows colors instead of gray.
        if !self.grids.has_value(&key)
            && let Some(low_res) = self
                .inner
                .get_resolution(id, Resolution::Low)
                .map(|data| data.raw_bytes.clone())
        {
            self.grids.insert(
                key.clone(),
                Resolution::Low,
                Arc::new(compute_art_grid(&low_res, cols, rows)),
            );
        }

        let source = best_raw_bytes_up_to(&mut self.inner, id, resolution);
        let lookup = self
            .grids
            .get_or_compute(&self.pool, &key, source, move |bytes| {
                Ok(compute_art_grid(&bytes, cols, rows))
            });

        let loading = lookup.stale || !self.inner.is_resolution_loaded(id, resolution);
        let grid = lookup
            .value
            .unwrap_or_else(|| Arc::new(ArtColorGrid::empty(cols, rows)));
        (grid, loading)
    }

    /// Preload album art for albums surrounding the next track in the queue.
    pub fn preload_next_track_surrounding_art(&mut self, logic: &Logic) {
        self.inner.preload_next_track_surrounding_art(logic);
    }

    /// Populate the background prefetch queue with cover art IDs.
    pub fn populate_prefetch_queue(&mut self, cover_art_ids: Vec<CoverArtId>) {
        self.inner.populate_prefetch_queue(cover_art_ids);
    }

    /// Advance the background prefetcher by one tick.
    pub fn tick_prefetch(&mut self, logic: &Logic) {
        self.inner.tick_prefetch(logic);
    }

    /// Returns the aspect ratio (height / width) of the source image, or 1.0
    /// if the image is not in the cache or the dimensions are unknown.
    pub fn get_aspect_ratio(&mut self, cover_art_id: Option<&CoverArtId>) -> f64 {
        let Some(id) = cover_art_id else {
            return 1.0;
        };
        self.inner
            .with_client_data_mut(id, |data, _raw| data.aspect_ratio)
            .flatten()
            .unwrap_or(1.0)
    }

    /// Returns a fixed-size image protocol for rendering via `ratatui_image::Image`.
    ///
    /// Used by the now-playing thumbnail, library thumbnails, and the album
    /// art overlay. `resolution` is the source resolution to request from the
    /// server (`Library` for thumbnails, `Full` for the overlay). The protocol
    /// is decoded in a background thread and cached by
    /// `(CoverArtId, width, height)`; while better source data is loading, a
    /// protocol from the best currently-available resolution is served and
    /// replaced once the better decode completes. Returns `None` before the
    /// first decode completes or when no picker is configured, so the caller
    /// can fall back to the existing half-block rendering.
    pub fn get_protocol(
        &mut self,
        logic: &Logic,
        cover_art_id: Option<&CoverArtId>,
        resolution: Resolution,
        width: u16,
        height: u16,
    ) -> Option<Arc<Protocol>> {
        let picker = self.protocol_picker.clone()?;
        let id = cover_art_id?;

        // Trigger a fetch at the requested resolution.
        let _ = self
            .inner
            .get(logic, Some(id), resolution, CachePriority::Visible);
        self.visible_this_frame.insert((id.clone(), resolution));

        let key = (id.clone(), width, height);
        let source = best_raw_bytes_up_to(&mut self.inner, id, resolution);
        let size = Size { width, height };

        self.protocols
            .get_or_compute(&self.pool, &key, source, move |bytes| {
                let dyn_img = image::load_from_memory(&bytes).map_err(|e| e.to_string())?;
                // `Scale` (not `Fit`) so that sources smaller than the target
                // area are upscaled to fill it.
                picker
                    .new_protocol(dyn_img, size, Resize::Scale(None))
                    .map_err(|e| e.to_string())
            })
            .value
    }

    /// Returns a sliced image protocol for scrollable rendering via
    /// `ratatui_image::sliced::SlicedImage`.
    ///
    /// Used by the library BelowAlbum art. `size` is the art area in
    /// character cells; the protocol is decoded in a background thread and
    /// cached by `(CoverArtId, width, height)`, so a terminal resize (which
    /// changes the art area) produces a new key and recomputes the art.
    /// While better source data is loading, a protocol from the best
    /// currently-available resolution is served and replaced once the better
    /// decode completes. Returns `None` before the first decode completes or
    /// when no picker is configured.
    pub fn get_sliced_protocol(
        &mut self,
        logic: &Logic,
        cover_art_id: Option<&CoverArtId>,
        size: Size,
    ) -> Option<Arc<SlicedProtocol>> {
        let picker = self.protocol_picker.clone()?;
        let id = cover_art_id?;

        // Trigger a library-res fetch.
        let _ = self
            .inner
            .get(logic, Some(id), Resolution::Library, CachePriority::Visible);
        self.visible_this_frame
            .insert((id.clone(), Resolution::Library));

        let key = (id.clone(), size.width, size.height);
        let source = best_raw_bytes_up_to(&mut self.inner, id, Resolution::Library);

        self.sliced_protocols
            .get_or_compute(&self.pool, &key, source, move |bytes| {
                let dyn_img = image::load_from_memory(&bytes).map_err(|e| e.to_string())?;
                // `Scale` (not the `Fit` used by `SlicedProtocol::new`) so
                // that sources smaller than the target area are upscaled to
                // fill it.
                SlicedProtocol::new_with_resize(&picker, dyn_img, size, Resize::Scale(None))
                    .map_err(|e| e.to_string())
            })
            .value
    }
}

/// Returns the raw encoded bytes of the best cached image at or below
/// `resolution`, together with the resolution they came from. A free function
/// (rather than a method) so callers can hold a borrow of another
/// `CoverArtCache` field across the call.
fn best_raw_bytes_up_to(
    inner: &mut cover_art_cache::CoverArtCache<TuiCoverArt>,
    id: &CoverArtId,
    resolution: Resolution,
) -> Option<(Resolution, Arc<[u8]>)> {
    [Resolution::Full, Resolution::Library, Resolution::Low]
        .into_iter()
        .filter(|res| *res <= resolution)
        .find_map(|res| {
            inner
                .with_client_data_mut_at(id, res, |data, _raw| data.raw_bytes.clone())
                .map(|bytes| (res, bytes))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates a small 2×2 PNG test image (solid red).
    fn test_png() -> Vec<u8> {
        use image::{ImageBuffer, ImageEncoder, Rgba, codecs::png::PngEncoder};
        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(2, 2, Rgba([255, 0, 0, 255]));
        let mut buf = Vec::new();
        PngEncoder::new(&mut buf)
            .write_image(img.as_raw(), 2, 2, image::ExtendedColorType::Rgba8)
            .unwrap();
        buf
    }

    /// Verifies that the `ratatui-image` dependency can decode images with the
    /// workspace's enabled image formats (jpeg, png, webp, gif) — feature
    /// unification must provide these formats since `image-defaults` is disabled.
    #[test]
    fn test_dependency_image_format_unification() {
        let picker = Picker::halfblocks();
        let png_bytes = test_png();
        let dyn_img = image::load_from_memory(&png_bytes).expect("failed to decode test PNG");
        let size = Size::new(2, 2);
        let protocol = picker
            .new_protocol(dyn_img, size, Resize::Fit(None))
            .expect("failed to create protocol from test image");
        // Just verify it was created — the variant depends on the picker's protocol type.
        let _ = protocol.size();
    }

    /// Verifies that protocol getters are disabled when no picker is
    /// configured (Halfblock mode or before `set_picker` is called).
    #[test]
    fn test_no_picker_disables_protocols() {
        let (_tx, rx) = std::sync::mpsc::channel::<CoverArt>();
        let cache = CoverArtCache::new(rx);
        // No picker set — has_picker() returns false, which is the guard
        // the protocol getters check first before touching Logic.
        assert!(!cache.has_picker());
    }
}
