//! Cover art caching and derived-artifact management for the TUI.
//!
//! Wraps the shared [`blackbird_client_shared::cover_art_cache`] cache
//! (which owns fetching, resolution tiers, and eviction) with the
//! TUI-specific artifacts derived from the raw image bytes: quantized color
//! grids for half-block rendering ([`quantize`]), and ratatui-image
//! protocols for terminals with a graphics protocol. Each artifact family
//! lives in a [`derived_cache::DerivedCache`] keyed by cover art id and
//! render size, which encodes the shared compute/upgrade/eviction
//! lifecycle; background work runs on a
//! [`blackbird_client_shared::thread_pool::ThreadPool`].

mod derived_cache;
mod quantize;

pub use quantize::{
    ArtColorGrid, ArtColors, QuadrantColors, compute_art_grid, compute_quadrant_colors,
};

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use blackbird_client_shared::{
    cover_art_cache::{self, CachePriority, ClientData, Resolution},
    thread_pool::ThreadPool,
};
use blackbird_core::{CoverArt, Logic, blackbird_state::CoverArtId};
use image::{
    DynamicImage, Rgba,
    imageops::{self, FilterType},
};
use ratatui::layout::Size;
use ratatui_image::{
    FontSize, Resize,
    picker::{Picker, ProtocolType},
    protocol::{Protocol, kitty::Kitty},
    sliced::SlicedProtocol,
};

use derived_cache::DerivedCache;
use quantize::image_aspect_ratio;

const POOL_SIZE: usize = 4;
/// Sized for the demand set: up to three pages of library entries (the
/// viewport plus a `Nearby` page either side) and the next-track
/// neighbourhood, with headroom for recently offscreen art.
const MAX_CACHE_SIZE: usize = 150;
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
    /// Protocol keys drawn since the last [`begin_frame`]. An image protocol
    /// transmits its image to the terminal once, then only emits lightweight
    /// placeholder cells that reference it by id; the transmitted image
    /// persists in the terminal's own store. [`update`] evicts protocols not
    /// drawn in the most recent frame, and deletes their terminal images (see
    /// `protocol_ids`), so the terminal's store stays bounded to what is on
    /// screen. Tracked separately per cache because a fixed-size and a sliced
    /// protocol can share a key.
    ///
    /// [`begin_frame`]: CoverArtCache::begin_frame
    /// [`update`]: CoverArtCache::update
    visible_protocols: HashSet<ProtocolKey>,
    visible_sliced_protocols: HashSet<ProtocolKey>,
    /// The kitty image id assigned to each live protocol, so its terminal
    /// image can be deleted on eviction and overwritten (rather than
    /// duplicated) when the same key is rebuilt at a higher resolution.
    /// Without this bookkeeping the terminal accumulates an image per
    /// distinct (art, size) ever shown until it hits its own storage limit
    /// and evicts images that are still on screen, blanking them. Separate
    /// maps mirror the two protocol caches; both draw ids from
    /// `next_protocol_id`.
    protocol_ids: HashMap<ProtocolKey, u32>,
    sliced_protocol_ids: HashMap<ProtocolKey, u32>,
    /// Monotonic source of kitty image ids. Starts at 0 and pre-increments,
    /// so ids begin at 1 (id 0 is avoided) and never repeat within a session.
    next_protocol_id: u32,
    /// Kitty image-deletion escape sequences accumulated during [`update`],
    /// drained by the render loop via [`take_pending_deletes`] and written to
    /// the terminal. Deferred rather than written inline because the cache
    /// has no terminal handle.
    ///
    /// [`update`]: CoverArtCache::update
    /// [`take_pending_deletes`]: CoverArtCache::take_pending_deletes
    pending_deletes: String,
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
            protocol_picker: None,
            protocols: DerivedCache::new("image protocol"),
            sliced_protocols: DerivedCache::new("sliced image protocol"),
            visible_protocols: HashSet::new(),
            visible_sliced_protocols: HashSet::new(),
            protocol_ids: HashMap::new(),
            sliced_protocol_ids: HashMap::new(),
            next_protocol_id: 0,
            pending_deletes: String::new(),
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

    /// Start a new demand frame in the underlying cache, and reset the set
    /// of protocols considered visible. Call once at the start of each draw;
    /// the `get*` methods then rebuild both the demand set and the visible-
    /// protocol set from the art that is actually rendered. Between draws
    /// the last frame's demand and visibility stay in effect, keeping
    /// visible art alive while the UI is idle (e.g. while paused).
    pub fn begin_frame(&mut self) {
        self.inner.begin_frame();
        self.visible_protocols.clear();
        self.visible_sliced_protocols.clear();
    }

    /// Reconciles the underlying cache against the current demand (fetches,
    /// eviction, prefetch) and drains completed color/grid computations.
    /// Returns `true` if any visual state changed.
    pub fn update(&mut self, logic: &Logic) -> bool {
        let mut changed = false;

        let result = self.inner.update(logic);
        if !result.evicted.is_empty() || !result.upgraded.is_empty() {
            changed = true;
        }
        let is_tmux = self.protocol_picker.as_ref().is_some_and(Picker::is_tmux);
        for id in &result.evicted {
            self.colors.evict_matching(|color_id| color_id == id);
            self.grids.evict_matching(|(grid_id, _, _)| grid_id == id);
            let evicted = self
                .protocols
                .evict_matching(|(proto_id, _, _)| proto_id == id);
            forget_protocol_images(
                &mut self.pending_deletes,
                &mut self.protocol_ids,
                is_tmux,
                &evicted,
            );
            let evicted = self
                .sliced_protocols
                .evict_matching(|(sliced_id, _, _)| sliced_id == id);
            forget_protocol_images(
                &mut self.pending_deletes,
                &mut self.sliced_protocol_ids,
                is_tmux,
                &evicted,
            );
        }

        // Evict image protocols whose art was not drawn in the most recent
        // frame, so re-entering the viewport rebuilds and re-transmits them,
        // and delete their terminal images so the terminal's store stays
        // bounded to what is on screen (see the `protocol_ids` field).
        // Undrawn protocols aren't visible, so dropping them changes nothing
        // on screen — hence no `changed` update here. Color grids have no
        // terminal-side state and are left tied to the byte cache above.
        let evicted = {
            let visible = &self.visible_protocols;
            self.protocols.evict_matching(|key| !visible.contains(key))
        };
        forget_protocol_images(
            &mut self.pending_deletes,
            &mut self.protocol_ids,
            is_tmux,
            &evicted,
        );
        let evicted = {
            let visible = &self.visible_sliced_protocols;
            self.sliced_protocols
                .evict_matching(|key| !visible.contains(key))
        };
        forget_protocol_images(
            &mut self.pending_deletes,
            &mut self.sliced_protocol_ids,
            is_tmux,
            &evicted,
        );

        // Upgraded entries need no explicit invalidation: `changed` above
        // forces a redraw, and the `get*` methods compare the cached source
        // resolution against the best available bytes on every call.

        changed |= self.colors.drain();
        changed |= self.grids.drain();
        changed |= self.protocols.drain();
        changed |= self.sliced_protocols.drain();

        changed
    }

    /// Takes the kitty image-deletion escape sequences accumulated since the
    /// last call, for the render loop to write to the terminal. Returns
    /// `None` when there is nothing to delete. Deleting an image the terminal
    /// no longer holds is a harmless no-op, so the exact write timing does
    /// not matter as long as it follows the frame that stopped drawing the
    /// art (which the render loop guarantees by draining after `update`).
    pub fn take_pending_deletes(&mut self) -> Option<String> {
        (!self.pending_deletes.is_empty()).then(|| std::mem::take(&mut self.pending_deletes))
    }

    /// Record a `Nearby` demand for library-resolution art: albums just
    /// outside the viewport, kept warm so scrolling doesn't flash
    /// placeholder art.
    pub fn demand_nearby(&mut self, cover_art_id: Option<&CoverArtId>) {
        self.inner
            .demand(cover_art_id, Resolution::Library, CachePriority::Nearby);
    }

    /// Get quadrant colors for a cover art entry at low resolution.
    /// Used for LeftOfAlbum thumbnail colors.
    pub fn get(&mut self, cover_art_id: Option<&CoverArtId>) -> QuadrantColors {
        let Some(id) = cover_art_id else {
            return QuadrantColors::default();
        };

        let _ = self
            .inner
            .get(Some(id), Resolution::Low, CachePriority::Visible);

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
    /// Demands library-resolution data and computes a grid in a background
    /// thread; returns a fallback grid while computing.
    pub fn get_art_grid(
        &mut self,
        cover_art_id: Option<&CoverArtId>,
        cols: usize,
        rows: usize,
    ) -> (Arc<ArtColorGrid>, bool) {
        self.art_grid_at(cover_art_id, cols, rows, Resolution::Library)
    }

    /// Returns a variable-size color grid for the overlay using full-resolution
    /// data. Falls back to lower resolutions while full-res is loading.
    pub fn get_full_res_art_grid(
        &mut self,
        cover_art_id: Option<&CoverArtId>,
        cols: usize,
        rows: usize,
    ) -> (Arc<ArtColorGrid>, bool) {
        self.art_grid_at(cover_art_id, cols, rows, Resolution::Full)
    }

    /// Returns a color grid computed from the best available data at or
    /// below `resolution`, demanding that resolution from the cache (which
    /// fetches it on the next `update`). The boolean is `true` while better
    /// data is loading or a recompute is in flight.
    fn art_grid_at(
        &mut self,
        cover_art_id: Option<&CoverArtId>,
        cols: usize,
        rows: usize,
        resolution: Resolution,
    ) -> (Arc<ArtColorGrid>, bool) {
        let Some(id) = cover_art_id else {
            return (Arc::new(ArtColorGrid::empty(cols, rows)), false);
        };

        // Record demand at the target resolution; `update()` fetches it.
        let _ = self.inner.get(Some(id), resolution, CachePriority::Visible);

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

    /// Populate the background prefetch queue with cover art IDs.
    pub fn populate_prefetch_queue(&mut self, cover_art_ids: Vec<CoverArtId>) {
        self.inner.populate_prefetch_queue(cover_art_ids);
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
        cover_art_id: Option<&CoverArtId>,
        resolution: Resolution,
        width: u16,
        height: u16,
    ) -> Option<Arc<Protocol>> {
        let picker = self.protocol_picker.clone()?;
        let id = cover_art_id?;

        // Record demand at the requested resolution; `update()` fetches it.
        let _ = self.inner.get(Some(id), resolution, CachePriority::Visible);

        let key = (id.clone(), width, height);
        self.visible_protocols.insert(key.clone());
        let image_id = alloc_id(&mut self.protocol_ids, &mut self.next_protocol_id, &key);
        let source = best_raw_bytes_up_to(&mut self.inner, id, resolution);
        let size = Size { width, height };

        self.protocols
            .get_or_compute(&self.pool, &key, source, move |bytes| {
                let dyn_img = decode_centered(&bytes, picker.font_size(), size)?;
                build_protocol(&picker, dyn_img, size, image_id)
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
        cover_art_id: Option<&CoverArtId>,
        size: Size,
    ) -> Option<Arc<SlicedProtocol>> {
        let picker = self.protocol_picker.clone()?;
        let id = cover_art_id?;

        // Record a library-res demand; `update()` fetches it.
        let _ = self
            .inner
            .get(Some(id), Resolution::Library, CachePriority::Visible);

        let key = (id.clone(), size.width, size.height);
        self.visible_sliced_protocols.insert(key.clone());
        let image_id = alloc_id(
            &mut self.sliced_protocol_ids,
            &mut self.next_protocol_id,
            &key,
        );
        let source = best_raw_bytes_up_to(&mut self.inner, id, Resolution::Library);

        self.sliced_protocols
            .get_or_compute(&self.pool, &key, source, move |bytes| {
                let dyn_img = decode_centered(&bytes, picker.font_size(), size)?;
                build_sliced_protocol(&picker, dyn_img, size, image_id)
            })
            .value
    }
}

/// Builds a fixed-size protocol for the picker's terminal graphics protocol.
///
/// For kitty, the protocol is constructed directly with `image_id` (a
/// blackbird-managed id) so its terminal image can be reused across rebuilds
/// and deleted on eviction — the only protocol with a persistent terminal-side
/// image store to manage. `decode_centered` already sizes the image to the
/// exact target pixel area, so no further resizing is needed; other protocols
/// go through the picker, which embeds their image data inline per render and
/// has no id to manage.
fn build_protocol(
    picker: &Picker,
    image: DynamicImage,
    size: Size,
    image_id: u32,
) -> Result<Protocol, String> {
    match picker.protocol_type() {
        ProtocolType::Kitty => Kitty::new(image, size, image_id, picker.is_tmux())
            .map(Protocol::Kitty)
            .map_err(|e| e.to_string()),
        _ => picker
            .new_protocol(image, size, Resize::Fit(None))
            .map_err(|e| e.to_string()),
    }
}

/// The [`build_protocol`] equivalent for scrollable [`SlicedProtocol`] art.
fn build_sliced_protocol(
    picker: &Picker,
    image: DynamicImage,
    size: Size,
    image_id: u32,
) -> Result<SlicedProtocol, String> {
    match picker.protocol_type() {
        ProtocolType::Kitty => Kitty::new(image, size, image_id, picker.is_tmux())
            .map(SlicedProtocol::Kitty)
            .map_err(|e| e.to_string()),
        _ => SlicedProtocol::new_with_resize(picker, image, size, Resize::Fit(None))
            .map_err(|e| e.to_string()),
    }
}

/// Returns the stable kitty image id for `key`, allocating the next id from
/// `next` on first use. Ids start at 1 (0 is avoided) and never repeat within
/// a session, so a rebuilt protocol reuses its id — the terminal overwrites
/// its image rather than storing a duplicate.
fn alloc_id(ids: &mut HashMap<ProtocolKey, u32>, next: &mut u32, key: &ProtocolKey) -> u32 {
    *ids.entry(key.clone()).or_insert_with(|| {
        *next = next.wrapping_add(1);
        *next
    })
}

/// Queues deletions of the terminal images for `evicted_keys`, removing their
/// ids from `ids`. A key with no assigned id (never rendered) is skipped;
/// deleting an id the terminal never received is a harmless no-op.
fn forget_protocol_images(
    pending: &mut String,
    ids: &mut HashMap<ProtocolKey, u32>,
    is_tmux: bool,
    evicted_keys: &[ProtocolKey],
) {
    for key in evicted_keys {
        if let Some(image_id) = ids.remove(key) {
            pending.push_str(&ratatui_image::protocol::kitty::delete_image_sequence(
                image_id, is_tmux,
            ));
        }
    }
}

/// Decodes encoded image bytes and letterboxes them into the exact pixel
/// area of `size` character cells: scaled to fit (upscaling smaller
/// sources), centered on both axes, and padded with transparency. The
/// result matches the target area exactly, so the protocol performs no
/// further resizing and non-square art renders centered within its slot
/// rather than anchored to the top-left corner (which is where
/// ratatui-image's own resizing pads to).
fn decode_centered(bytes: &[u8], font_size: FontSize, size: Size) -> Result<DynamicImage, String> {
    let target_width = u32::from(size.width) * u32::from(font_size.width);
    let target_height = u32::from(size.height) * u32::from(font_size.height);
    if target_width == 0 || target_height == 0 {
        return Err("the target area is empty".to_string());
    }

    let image = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let scaled = image.resize(target_width, target_height, FilterType::Triangle);

    let mut canvas = image::RgbaImage::from_pixel(target_width, target_height, Rgba([0, 0, 0, 0]));
    let x = i64::from((target_width - scaled.width()) / 2);
    let y = i64::from((target_height - scaled.height()) / 2);
    imageops::overlay(&mut canvas, &scaled, x, y);
    Ok(DynamicImage::ImageRgba8(canvas))
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

    fn key(name: &str) -> ProtocolKey {
        (CoverArtId(name.into()), 4, 4)
    }

    /// Ids are allocated once per key, are stable across repeated lookups,
    /// start at 1 (never 0), and increase monotonically for new keys.
    #[test]
    fn test_alloc_id_stable_and_monotonic() {
        let mut ids = HashMap::new();
        let mut next = 0;
        let (a, b) = (key("a"), key("b"));

        assert_eq!(alloc_id(&mut ids, &mut next, &a), 1);
        assert_eq!(alloc_id(&mut ids, &mut next, &b), 2);
        // The same key keeps its id and allocates nothing new.
        assert_eq!(alloc_id(&mut ids, &mut next, &a), 1);
        assert_eq!(next, 2);
    }

    /// Forgetting a protocol queues a delete for its image id, removes the
    /// id from the map, and skips keys that were never assigned an id.
    #[test]
    fn test_forget_protocol_images() {
        let mut ids = HashMap::new();
        let mut next = 0;
        let k = key("a");
        let image_id = alloc_id(&mut ids, &mut next, &k);

        let mut pending = String::new();
        forget_protocol_images(&mut pending, &mut ids, false, std::slice::from_ref(&k));
        assert!(pending.contains(&format!("i={image_id}")));
        assert!(pending.contains("a=d,d=I"));
        assert!(!ids.contains_key(&k), "the id is released on delete");

        // A key with no assigned id produces no delete sequence.
        let mut pending = String::new();
        forget_protocol_images(&mut pending, &mut ids, false, &[key("never-rendered")]);
        assert!(pending.is_empty());
    }

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

    /// Encodes a solid red PNG of the given dimensions.
    fn red_png(width: u32, height: u32) -> Vec<u8> {
        use image::{ImageBuffer, ImageEncoder, codecs::png::PngEncoder};
        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(width, height, Rgba([255, 0, 0, 255]));
        let mut buf = Vec::new();
        PngEncoder::new(&mut buf)
            .write_image(img.as_raw(), width, height, image::ExtendedColorType::Rgba8)
            .unwrap();
        buf
    }

    /// Verifies that non-square art is scaled to fit the slot and centered
    /// on both axes, with transparent letterbox padding.
    #[test]
    fn test_decode_centered_letterboxes_non_square() {
        // 4×2 cells at a 10×20 font is a 40×40 pixel target.
        let font_size = FontSize::new(10, 20);
        let size = Size::new(4, 2);

        // A wide 100×50 image scales to 40×20 and centers vertically.
        let wide = decode_centered(&red_png(100, 50), font_size, size).unwrap();
        let wide = wide.to_rgba8();
        assert_eq!((wide.width(), wide.height()), (40, 40));
        assert_eq!(wide.get_pixel(20, 20)[3], 255, "center is opaque");
        assert_eq!(wide.get_pixel(20, 20)[0], 255, "center is red");
        assert_eq!(wide.get_pixel(0, 20)[3], 255, "fills the full width");
        assert_eq!(wide.get_pixel(20, 4)[3], 0, "top letterbox is transparent");
        assert_eq!(
            wide.get_pixel(20, 36)[3],
            0,
            "bottom letterbox is transparent"
        );

        // A tall 50×100 image scales to 20×40 and centers horizontally.
        let tall = decode_centered(&red_png(50, 100), font_size, size).unwrap();
        let tall = tall.to_rgba8();
        assert_eq!((tall.width(), tall.height()), (40, 40));
        assert_eq!(tall.get_pixel(20, 20)[3], 255, "center is opaque");
        assert_eq!(tall.get_pixel(20, 0)[3], 255, "fills the full height");
        assert_eq!(tall.get_pixel(4, 20)[3], 0, "left letterbox is transparent");
        assert_eq!(
            tall.get_pixel(36, 20)[3],
            0,
            "right letterbox is transparent"
        );

        // A small square image is upscaled to fill the slot exactly.
        let small = decode_centered(&red_png(16, 16), font_size, size).unwrap();
        let small = small.to_rgba8();
        assert_eq!((small.width(), small.height()), (40, 40));
        assert_eq!(small.get_pixel(0, 0)[3], 255, "small sources are upscaled");
    }
}
