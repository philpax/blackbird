use std::{
    collections::{HashMap, HashSet},
    io::Cursor,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, Sender},
    },
    time::Duration,
};

use blackbird_client_shared::cover_art_cache::{self, CachePriority, ClientData, Resolution};
use blackbird_core::{CoverArt, Logic, blackbird_state::CoverArtId};
use ratatui::layout::Size;
use ratatui::style::Color;
use ratatui_image::{Resize, picker::Picker, protocol::Protocol, sliced::SlicedProtocol};

const POOL_SIZE: usize = 4;

struct ThreadPool {
    tx: Sender<Box<dyn FnOnce() + Send>>,
}

impl ThreadPool {
    fn new(num_threads: usize) -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<Box<dyn FnOnce() + Send>>();
        let rx = Arc::new(Mutex::new(rx));
        for _ in 0..num_threads {
            let rx = rx.clone();
            std::thread::spawn(move || {
                loop {
                    // Take the job while holding the lock, but release it
                    // before running the job — holding it across `job()`
                    // would serialize the workers, and a panicking job would
                    // poison the mutex and kill the whole pool.
                    let job = match rx.lock() {
                        Ok(receiver) => receiver.recv(),
                        Err(_) => break,
                    };
                    let Ok(job) = job else { break };
                    if catch_unwind(AssertUnwindSafe(job)).is_err() {
                        tracing::error!("a cover art worker job panicked");
                    }
                }
            });
        }
        Self { tx }
    }

    fn spawn(&self, f: impl FnOnce() + Send + 'static) {
        let _ = self.tx.send(Box::new(f));
    }
}

/// 4 columns × 4 rows of colours extracted from album art.
/// This allows 2 terminal lines of album art (each half-block shows 2 rows).
#[derive(Debug, Clone, Copy)]
pub struct ArtColors {
    /// Colors arranged as [row][col], where row 0 is top, col 0 is left.
    pub colors: [[Color; 4]; 4],
}

impl Default for ArtColors {
    fn default() -> Self {
        Self {
            colors: [[Color::DarkGray; 4]; 4],
        }
    }
}

// Keep the old name as an alias for compatibility during transition.
pub type QuadrantColors = ArtColors;

/// Variable-size grid of colours for the album art overlay.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ArtColorGrid {
    /// Colors arranged row-major: `colors[row][col]`.
    pub colors: Vec<Vec<Color>>,
    pub cols: usize,
    pub rows: usize,
}

impl ArtColorGrid {
    pub fn empty(cols: usize, rows: usize) -> Self {
        Self {
            colors: vec![vec![Color::DarkGray; cols]; rows],
            cols,
            rows,
        }
    }
}

#[derive(Clone)]
pub struct TuiCoverArt {
    raw_bytes: Arc<[u8]>,
    /// 4x4 color grid for library thumbnails (computed in background).
    pub colors: Option<QuadrantColors>,
    /// Aspect ratio (height / width) of the source image. Computed from the
    /// image header so it is available without a full decode.
    aspect_ratio: Option<f64>,
}

impl ClientData for TuiCoverArt {
    fn from_image_data(data: &Arc<[u8]>, _id: &CoverArtId, resolution: Resolution) -> Self {
        // Low-res images (16×16 disk cache) are trivially cheap to process —
        // compute colors synchronously to avoid a frame of gray.
        let colors = if resolution == Resolution::Low {
            Some(compute_quadrant_colors(data))
        } else {
            None
        };
        let aspect_ratio = image_aspect_ratio(data);
        TuiCoverArt {
            raw_bytes: data.clone(),
            colors,
            aspect_ratio,
        }
    }

    fn carry_over(&mut self, previous: &Self) {
        if self.colors.is_none() {
            self.colors = previous.colors;
        }
        if self.aspect_ratio.is_none() {
            self.aspect_ratio = previous.aspect_ratio;
        }
    }
}

const MAX_CACHE_SIZE: usize = 50;
const CACHE_ENTRY_TIMEOUT: Duration = Duration::from_secs(30);

pub struct CoverArtCache {
    inner: cover_art_cache::CoverArtCache<TuiCoverArt>,
    pool: ThreadPool,
    color_tx: Sender<(CoverArtId, Resolution, QuadrantColors)>,
    color_rx: Receiver<(CoverArtId, Resolution, QuadrantColors)>,
    /// Tracks which (id, resolution) pairs are currently computing colors.
    computing: HashSet<(CoverArtId, Resolution)>,
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

/// Cache key for color grids: the cover art id and the grid dimensions in
/// half-block pixels.
type GridKey = (CoverArtId, usize, usize);

/// Cache key for image protocols: the cover art id and the target render
/// size in character cells.
type ProtocolKey = (CoverArtId, u16, u16);

/// A cache of artifacts derived from encoded cover art bytes in background
/// threads, keyed by `K`.
///
/// Encodes the lifecycle shared by every derived artifact (color grids and
/// image protocols): compute once per key, serve the stale value while a
/// higher-resolution recompute is in flight, never downgrade on late
/// arrivals, remember failures per source resolution so they are not retried
/// every frame, and drop results whose entry was evicted mid-flight.
struct DerivedCache<K, V> {
    entries: HashMap<K, DerivedEntry<V>>,
    tx: Sender<(K, Resolution, Result<V, String>)>,
    rx: Receiver<(K, Resolution, Result<V, String>)>,
    /// Artifact name for log messages.
    name: &'static str,
}

/// The full lifecycle state of one derived artifact. The first compute often
/// runs from the 16px disk-cache image before better data has loaded, so the
/// source resolution is tracked to recompute when a higher one arrives.
struct DerivedEntry<V> {
    /// The best value computed so far, with the resolution of the source
    /// image it was computed from. `Arc` allows cheap cloning for rendering
    /// without copying the artifact.
    value: Option<(Arc<V>, Resolution)>,
    /// The source resolution of the in-flight compute, if any.
    computing: Option<Resolution>,
    /// The source resolution of the most recent failed compute. A compute
    /// from this exact resolution is not retried; a different resolution
    /// becoming the best available source clears the way for a retry.
    failed: Option<Resolution>,
}

impl<V> Default for DerivedEntry<V> {
    fn default() -> Self {
        Self {
            value: None,
            computing: None,
            failed: None,
        }
    }
}

/// The result of a [`DerivedCache`] lookup.
struct Lookup<V> {
    /// The best value computed so far, possibly from a lower resolution than
    /// the best available source. `None` before the first compute completes.
    value: Option<Arc<V>>,
    /// `true` while a compute is in flight or a retryable better source is
    /// available than the one the value was computed from.
    stale: bool,
}

impl<K, V> DerivedCache<K, V>
where
    K: Clone + Eq + std::hash::Hash + std::fmt::Debug + Send + 'static,
    V: Send + 'static,
{
    fn new(name: &'static str) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Self {
            entries: HashMap::new(),
            tx,
            rx,
            name,
        }
    }

    /// Looks up the artifact for `key`, spawning `compute` on `pool` when no
    /// value has been computed yet or `source` is better than the one the
    /// cached value was computed from. The stale value keeps being served
    /// while the recompute runs, so the art doesn't flicker back to the
    /// caller's fallback rendering.
    fn get_or_compute(
        &mut self,
        pool: &ThreadPool,
        key: &K,
        source: Option<(Resolution, Arc<[u8]>)>,
        compute: impl FnOnce(Arc<[u8]>) -> Result<V, String> + Send + 'static,
    ) -> Lookup<V> {
        let entry = self.entries.entry(key.clone()).or_default();

        let cached_resolution = entry.value.as_ref().map(|(_, resolution)| *resolution);
        let wants_compute = source.as_ref().is_some_and(|(source_resolution, _)| {
            let better = cached_resolution.is_none_or(|cached| *source_resolution > cached);
            better && entry.failed != Some(*source_resolution)
        });

        if wants_compute
            && entry.computing.is_none()
            && let Some((source_resolution, bytes)) = source
        {
            entry.computing = Some(source_resolution);
            let key = key.clone();
            let tx = self.tx.clone();
            pool.spawn(move || {
                let _ = tx.send((key, source_resolution, compute(bytes)));
            });
        }

        Lookup {
            value: entry.value.as_ref().map(|(value, _)| value.clone()),
            stale: entry.computing.is_some() || wants_compute,
        }
    }

    /// Returns `true` if a value has been computed for `key`.
    fn has_value(&self, key: &K) -> bool {
        self.entries
            .get(key)
            .is_some_and(|entry| entry.value.is_some())
    }

    /// Inserts an externally computed value, unless a value from a higher
    /// source resolution is already cached. Used to seed the cache with a
    /// synchronously computed low-resolution artifact.
    fn insert(&mut self, key: K, source_resolution: Resolution, value: Arc<V>) {
        let entry = self.entries.entry(key).or_default();
        let dominated = entry
            .value
            .as_ref()
            .is_some_and(|(_, cached)| *cached > source_resolution);
        if !dominated {
            entry.value = Some((value, source_resolution));
        }
    }

    /// Drains completed computes. Returns `true` if any cached value
    /// changed (failures don't change visual state, so they don't count).
    fn drain(&mut self) -> bool {
        let mut changed = false;
        for (key, source_resolution, result) in self.rx.try_iter() {
            // Drop results whose entry was evicted while the compute was in
            // flight (or superseded after an evict-and-recreate); accepting
            // them would resurrect a zombie entry.
            let Some(entry) = self.entries.get_mut(&key) else {
                continue;
            };
            if entry.computing != Some(source_resolution) {
                continue;
            }
            entry.computing = None;
            match result {
                Ok(value) => {
                    let dominated = entry
                        .value
                        .as_ref()
                        .is_some_and(|(_, cached)| *cached > source_resolution);
                    if !dominated {
                        changed = true;
                        entry.value = Some((Arc::new(value), source_resolution));
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "{} compute failed for {key:?} from {source_resolution:?}: {e}",
                        self.name
                    );
                    entry.failed = Some(source_resolution);
                }
            }
        }
        changed
    }

    /// Removes every entry whose key matches the predicate.
    fn evict_matching(&mut self, matches: impl Fn(&K) -> bool) {
        self.entries.retain(|key, _| !matches(key));
    }
}

impl CoverArtCache {
    pub fn new(cover_art_loaded_rx: std::sync::mpsc::Receiver<CoverArt>) -> Self {
        let (color_tx, color_rx) = std::sync::mpsc::channel();
        Self {
            inner: cover_art_cache::CoverArtCache::new(
                cover_art_loaded_rx,
                MAX_CACHE_SIZE,
                CACHE_ENTRY_TIMEOUT,
            ),
            pool: ThreadPool::new(POOL_SIZE),
            color_tx,
            color_rx,
            computing: HashSet::new(),
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
            // Remove all resolution-keyed entries for this id.
            self.computing.remove(&(id.clone(), Resolution::Low));
            self.computing.remove(&(id.clone(), Resolution::Library));
            self.computing.remove(&(id.clone(), Resolution::Full));
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

        for (id, resolution, colors) in self.color_rx.try_iter() {
            changed = true;
            self.computing.remove(&(id.clone(), resolution));
            self.inner
                .with_client_data_mut_at(&id, resolution, |data, _raw| {
                    data.colors = Some(colors);
                });
        }

        changed |= self.grids.drain();
        changed |= self.protocols.drain();

        changed |= self.sliced_protocols.drain();

        changed
    }

    /// Get quadrant colors for a cover art entry at low resolution.
    /// Used for LeftOfAlbum thumbnail colors.
    pub fn get(&mut self, logic: &Logic, cover_art_id: Option<&CoverArtId>) -> QuadrantColors {
        if let Some(id) = cover_art_id {
            self.visible_this_frame
                .insert((id.clone(), Resolution::Low));
        }
        let Some(result) =
            self.inner
                .get(logic, cover_art_id, Resolution::Low, CachePriority::Visible)
        else {
            return QuadrantColors::default();
        };

        if let Some(colors) = result.data.colors {
            return colors;
        }

        // Colors not computed yet — spawn background thread if not already running.
        let id = cover_art_id.unwrap(); // Safe: inner.get returned Some.
        let key = (id.clone(), result.resolution);
        if !self.computing.contains(&key) {
            self.computing.insert(key);
            let raw = result.data.raw_bytes.clone();
            let id_clone = id.clone();
            let resolution = result.resolution;
            let tx = self.color_tx.clone();
            self.pool.spawn(move || {
                let colors = compute_quadrant_colors(&raw);
                let _ = tx.send((id_clone, resolution, colors));
            });
        }

        QuadrantColors::default()
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
    /// Used by the library BelowAlbum art. The protocol is decoded in a
    /// background thread and cached by `(CoverArtId, width, height)`, where
    /// the dimensions are the current `large_art_cols() ×
    /// LARGE_ART_TERM_ROWS` art area (a terminal resize changes the key and
    /// recomputes the art). While better source data is loading, a protocol
    /// from the best currently-available resolution is served and replaced
    /// once the better decode completes. Returns `None` before the first
    /// decode completes or when no picker is configured.
    pub fn get_sliced_protocol(
        &mut self,
        logic: &Logic,
        cover_art_id: Option<&CoverArtId>,
    ) -> Option<Arc<SlicedProtocol>> {
        let picker = self.protocol_picker.clone()?;
        let id = cover_art_id?;

        // Trigger a library-res fetch.
        let _ = self
            .inner
            .get(logic, Some(id), Resolution::Library, CachePriority::Visible);
        self.visible_this_frame
            .insert((id.clone(), Resolution::Library));

        let size = Size {
            width: crate::ui::layout::large_art_cols(),
            height: crate::ui::layout::LARGE_ART_TERM_ROWS as u16,
        };
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

/// Reads the image header to extract the aspect ratio (height / width)
/// without decoding the full pixel data.
fn image_aspect_ratio(data: &[u8]) -> Option<f64> {
    let reader = image::ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .ok()?;
    let (w, h) = reader.into_dimensions().ok()?;
    if w == 0 {
        return None;
    }
    Some(h as f64 / w as f64)
}

/// Computes the average colour of each region in a 4×4 grid (4 cols, 4 rows).
pub(crate) fn compute_quadrant_colors(image_data: &[u8]) -> ArtColors {
    let Ok(img) = image::load_from_memory(image_data) else {
        return ArtColors::default();
    };

    let rgb = img.to_rgb8();
    let (w, h) = (rgb.width() as usize, rgb.height() as usize);

    if w == 0 || h == 0 {
        return ArtColors::default();
    }

    let average_region = |x0: usize, y0: usize, x1: usize, y1: usize| -> Color {
        let mut r_sum: u64 = 0;
        let mut g_sum: u64 = 0;
        let mut b_sum: u64 = 0;
        let mut count: u64 = 0;

        for y in y0..y1 {
            for x in x0..x1 {
                let pixel = rgb.get_pixel(x as u32, y as u32);
                r_sum += pixel[0] as u64;
                g_sum += pixel[1] as u64;
                b_sum += pixel[2] as u64;
                count += 1;
            }
        }

        if count == 0 {
            return Color::DarkGray;
        }

        Color::Rgb(
            (r_sum / count) as u8,
            (g_sum / count) as u8,
            (b_sum / count) as u8,
        )
    };

    // 4 columns, 4 rows
    let col_width = w / 4;
    let row_height = h / 4;

    let mut colors = [[Color::DarkGray; 4]; 4];
    for (row, row_colors) in colors.iter_mut().enumerate() {
        for (col, color) in row_colors.iter_mut().enumerate() {
            let x0 = col * col_width;
            let y0 = row * row_height;
            let x1 = if col == 3 { w } else { (col + 1) * col_width };
            let y1 = if row == 3 { h } else { (row + 1) * row_height };
            *color = average_region(x0, y0, x1.max(x0 + 1), y1.max(y0 + 1));
        }
    }

    ArtColors { colors }
}

/// Computes a variable-size grid of averaged colours from image data.
pub(crate) fn compute_art_grid(image_data: &[u8], cols: usize, rows: usize) -> ArtColorGrid {
    if cols == 0 || rows == 0 {
        return ArtColorGrid::empty(cols, rows);
    }

    let Ok(img) = image::load_from_memory(image_data) else {
        return ArtColorGrid::empty(cols, rows);
    };

    let rgb = img.to_rgb8();
    let (w, h) = (rgb.width() as usize, rgb.height() as usize);

    if w == 0 || h == 0 {
        return ArtColorGrid::empty(cols, rows);
    }

    let mut grid = vec![vec![Color::DarkGray; cols]; rows];

    for (row, row_colors) in grid.iter_mut().enumerate().take(rows) {
        for (col, cell) in row_colors.iter_mut().enumerate().take(cols) {
            let x0 = col * w / cols;
            let y0 = row * h / rows;
            let x1 = ((col + 1) * w / cols).max(x0 + 1);
            let y1 = ((row + 1) * h / rows).max(y0 + 1);

            let mut r_sum: u64 = 0;
            let mut g_sum: u64 = 0;
            let mut b_sum: u64 = 0;
            let mut count: u64 = 0;

            for y in y0..y1 {
                for x in x0..x1 {
                    let pixel = rgb.get_pixel(x as u32, y as u32);
                    r_sum += pixel[0] as u64;
                    g_sum += pixel[1] as u64;
                    b_sum += pixel[2] as u64;
                    count += 1;
                }
            }

            // The bounds above guarantee x1 > x0 and y1 > y0 (both clamped via
            // `.max(+1)`), so the inner loops always iterate at least once and
            // `count` is strictly positive.
            *cell = Color::Rgb(
                (r_sum / count) as u8,
                (g_sum / count) as u8,
                (b_sum / count) as u8,
            );
        }
    }

    ArtColorGrid {
        colors: grid,
        cols,
        rows,
    }
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

    // ── DerivedCache lifecycle ──────────────────────────────────────────────

    type TestKey = (CoverArtId, u16, u16);

    fn test_key(name: &str) -> TestKey {
        (CoverArtId(name.into()), 10, 10)
    }

    fn test_bytes() -> Arc<[u8]> {
        Arc::from(&[0u8; 4][..])
    }

    /// Polls `drain` until it reports a change or the timeout elapses.
    /// Returns `true` if a change was observed.
    fn drain_within(cache: &mut DerivedCache<TestKey, u32>, timeout: Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline {
            if cache.drain() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    /// A compute spawned through the pool completes, is cached, and is
    /// served on subsequent lookups without recomputing.
    #[test]
    fn test_derived_cache_computes_and_caches() {
        let pool = ThreadPool::new(1);
        let mut cache: DerivedCache<TestKey, u32> = DerivedCache::new("test");
        let key = test_key("compute");

        let lookup = cache.get_or_compute(
            &pool,
            &key,
            Some((Resolution::Library, test_bytes())),
            |_bytes| Ok(42),
        );
        assert!(lookup.value.is_none());
        assert!(lookup.stale);

        assert!(drain_within(&mut cache, Duration::from_secs(5)));

        // The cached value is now served, and no further compute is wanted.
        let lookup = cache.get_or_compute(
            &pool,
            &key,
            Some((Resolution::Library, test_bytes())),
            |_bytes| Ok(1),
        );
        assert_eq!(lookup.value.as_deref(), Some(&42));
        assert!(!lookup.stale);
    }

    /// A cached lower-resolution value keeps being served while a
    /// higher-resolution recompute runs, and is replaced when it lands.
    #[test]
    fn test_derived_cache_serves_stale_while_upgrading() {
        let pool = ThreadPool::new(1);
        let mut cache: DerivedCache<TestKey, u32> = DerivedCache::new("test");
        let key = test_key("upgrade");

        cache.insert(key.clone(), Resolution::Low, Arc::new(1));

        let lookup = cache.get_or_compute(
            &pool,
            &key,
            Some((Resolution::Full, test_bytes())),
            |_bytes| Ok(2),
        );
        // The stale low-res value is served while the upgrade decodes.
        assert_eq!(lookup.value.as_deref(), Some(&1));
        assert!(lookup.stale);

        assert!(drain_within(&mut cache, Duration::from_secs(5)));

        let lookup = cache.get_or_compute(
            &pool,
            &key,
            Some((Resolution::Full, test_bytes())),
            |_bytes| Ok(3),
        );
        assert_eq!(lookup.value.as_deref(), Some(&2));
        assert!(!lookup.stale);
    }

    /// A late lower-resolution result must not replace a cached
    /// higher-resolution value.
    #[test]
    fn test_derived_cache_no_downgrade() {
        let mut cache: DerivedCache<TestKey, u32> = DerivedCache::new("test");
        let key = test_key("downgrade");

        cache.entries.insert(
            key.clone(),
            DerivedEntry {
                value: Some((Arc::new(5), Resolution::Full)),
                computing: Some(Resolution::Library),
                failed: None,
            },
        );
        let _ = cache.tx.send((key.clone(), Resolution::Library, Ok(3)));

        assert!(!cache.drain());
        let entry = cache.entries.get(&key).unwrap();
        assert_eq!(
            entry.value.as_ref().map(|(v, r)| (**v, *r)),
            Some((5, Resolution::Full))
        );
        assert_eq!(entry.computing, None);
    }

    /// A failed compute records the failed source resolution, does not
    /// report a change (which would force a redraw and respawn the failing
    /// compute in a loop), and is not retried from the same source.
    #[test]
    fn test_derived_cache_failure_not_retried() {
        let pool = ThreadPool::new(1);
        let mut cache: DerivedCache<TestKey, u32> = DerivedCache::new("test");
        let key = test_key("failure");

        cache.entries.insert(
            key.clone(),
            DerivedEntry {
                value: None,
                computing: Some(Resolution::Low),
                failed: None,
            },
        );
        let _ = cache.tx.send((
            key.clone(),
            Resolution::Low,
            Err("decode error".to_string()),
        ));

        assert!(!cache.drain());
        assert_eq!(
            cache.entries.get(&key).unwrap().failed,
            Some(Resolution::Low)
        );

        // The same source must not be retried…
        let lookup = cache.get_or_compute(
            &pool,
            &key,
            Some((Resolution::Low, test_bytes())),
            |_bytes| Ok(1),
        );
        assert!(!lookup.stale);
        assert_eq!(cache.entries.get(&key).unwrap().computing, None);

        // …but a better source clears the way for a retry.
        let lookup = cache.get_or_compute(
            &pool,
            &key,
            Some((Resolution::Library, test_bytes())),
            |_bytes| Ok(1),
        );
        assert!(lookup.stale);
        assert!(drain_within(&mut cache, Duration::from_secs(5)));
    }

    /// Results whose entry was evicted while the compute was in flight are
    /// dropped rather than resurrected as zombie entries.
    #[test]
    fn test_derived_cache_untracked_result_dropped() {
        let mut cache: DerivedCache<TestKey, u32> = DerivedCache::new("test");
        let key = test_key("untracked");

        // No entry exists — as if it was evicted mid-decode.
        let _ = cache.tx.send((key.clone(), Resolution::Full, Ok(9)));

        assert!(!cache.drain());
        assert!(!cache.entries.contains_key(&key));
    }

    /// Eviction removes exactly the entries whose keys match.
    #[test]
    fn test_derived_cache_evict_matching() {
        let mut cache: DerivedCache<TestKey, u32> = DerivedCache::new("test");
        let keep = test_key("keep");
        let evict = test_key("evict");
        cache.insert(keep.clone(), Resolution::Low, Arc::new(1));
        cache.insert(evict.clone(), Resolution::Low, Arc::new(2));

        let (evict_id, _, _) = evict.clone();
        cache.evict_matching(|(id, _, _)| *id == evict_id);

        assert!(cache.has_value(&keep));
        assert!(!cache.entries.contains_key(&evict));
    }

    /// `insert` seeds a value but never overwrites a higher-resolution one.
    #[test]
    fn test_derived_cache_insert_respects_dominance() {
        let mut cache: DerivedCache<TestKey, u32> = DerivedCache::new("test");
        let key = test_key("insert");

        cache.insert(key.clone(), Resolution::Library, Arc::new(1));
        cache.insert(key.clone(), Resolution::Low, Arc::new(2));

        let entry = cache.entries.get(&key).unwrap();
        assert_eq!(
            entry.value.as_ref().map(|(v, r)| (**v, *r)),
            Some((1, Resolution::Library))
        );
    }
}
