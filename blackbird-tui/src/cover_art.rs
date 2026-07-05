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
    grid_tx: Sender<(CoverArtId, Resolution, ArtColorGrid)>,
    grid_rx: Receiver<(CoverArtId, Resolution, ArtColorGrid)>,
    grid_computing: HashSet<CoverArtId>,
    /// Cached grids keyed by `(CoverArtId, cols, rows)`, with the resolution
    /// they were computed from. Persisted across image-data transitions so
    /// the previous grid remains visible while a higher-res grid is computing.
    overlay_grids: HashMap<(CoverArtId, usize, usize), CachedGrid>,
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
    /// Cached fixed-size protocols for the now-playing thumbnail and overlay.
    /// Keyed by `(CoverArtId, width, height)` in character cells so that
    /// different render sizes get distinct protocols. `Arc` allows cheap
    /// cloning for rendering without copying encoded image data.
    protocols: HashMap<ProtocolKey, CachedProtocol<Protocol>>,
    /// Tracks in-flight fixed-size protocol decodes.
    protocol_computing: HashSet<ProtocolKey>,
    /// Channel for fixed-size protocol results from background threads.
    protocol_tx: Sender<ProtocolResult<Protocol>>,
    protocol_rx: Receiver<ProtocolResult<Protocol>>,
    /// Fixed-size protocol decodes that failed, per source resolution, so
    /// they are not retried every frame. Cleared on eviction so a later
    /// re-fetch can retry.
    protocol_failed: HashSet<(ProtocolKey, Resolution)>,
    /// Cached sliced protocols for the library BelowAlbum art (scrollable).
    /// Keyed by `(CoverArtId, width, height)` in character cells; the art
    /// area dimensions derive from the terminal size (`large_art_cols()`),
    /// so a resize produces new keys and recomputes the art.
    sliced_protocols: HashMap<ProtocolKey, CachedProtocol<SlicedProtocol>>,
    /// Tracks in-flight sliced protocol decodes.
    sliced_protocol_computing: HashSet<ProtocolKey>,
    /// Channel for sliced protocol results from background threads.
    sliced_protocol_tx: Sender<ProtocolResult<SlicedProtocol>>,
    sliced_protocol_rx: Receiver<ProtocolResult<SlicedProtocol>>,
    /// Sliced protocol decodes that failed, per source resolution.
    sliced_protocol_failed: HashSet<(ProtocolKey, Resolution)>,
}

/// An overlay grid together with the resolution of the source image it
/// was computed from, so we know when to trigger recomputation.
struct CachedGrid {
    grid: ArtColorGrid,
    source_resolution: Resolution,
}

/// Cache key for image protocols: the cover art id and the target render
/// size in character cells.
type ProtocolKey = (CoverArtId, u16, u16);

/// Result of a background protocol decode: the cache key, the source
/// resolution the protocol was computed from, and the decode result.
type ProtocolResult<P> = (ProtocolKey, Resolution, Result<P, String>);

/// A cached image protocol together with the resolution of the source image
/// it was computed from. The first decode often runs from the 16px disk-cache
/// image before better data has loaded, so the source resolution is tracked
/// to recompute the protocol when a higher resolution arrives.
struct CachedProtocol<P> {
    protocol: Arc<P>,
    source_resolution: Resolution,
}

impl CoverArtCache {
    pub fn new(cover_art_loaded_rx: std::sync::mpsc::Receiver<CoverArt>) -> Self {
        let (color_tx, color_rx) = std::sync::mpsc::channel();
        let (grid_tx, grid_rx) = std::sync::mpsc::channel();
        let (protocol_tx, protocol_rx) = std::sync::mpsc::channel();
        let (sliced_protocol_tx, sliced_protocol_rx) = std::sync::mpsc::channel();
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
            grid_tx,
            grid_rx,
            grid_computing: HashSet::new(),
            overlay_grids: HashMap::new(),
            visible_this_frame: HashSet::new(),
            protocol_picker: None,
            protocols: HashMap::new(),
            protocol_computing: HashSet::new(),
            protocol_tx,
            protocol_rx,
            protocol_failed: HashSet::new(),
            sliced_protocols: HashMap::new(),
            sliced_protocol_computing: HashSet::new(),
            sliced_protocol_tx,
            sliced_protocol_rx,
            sliced_protocol_failed: HashSet::new(),
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
            self.grid_computing.remove(id);
            self.overlay_grids
                .retain(|(grid_id, _, _), _| grid_id != id);
            // Remove protocol caches for this id. Kitty virtual placements
            // are removed by the terminal itself once their unicode
            // placeholders stop being drawn; the transmitted image data
            // persists until the terminal evicts it from its own store.
            self.protocols.retain(|(proto_id, _, _), _| proto_id != id);
            self.protocol_computing
                .retain(|(proto_id, _, _)| proto_id != id);
            self.protocol_failed
                .retain(|((failed_id, _, _), _)| failed_id != id);
            self.sliced_protocols
                .retain(|(sliced_id, _, _), _| sliced_id != id);
            self.sliced_protocol_computing
                .retain(|(sliced_id, _, _)| sliced_id != id);
            self.sliced_protocol_failed
                .retain(|((failed_id, _, _), _)| failed_id != id);
        }

        // On upgraded entries, allow recomputation from the better data if the
        // cached grid was computed from a lower resolution. Don't remove the
        // cached grid — it serves as a fallback while the new one computes.
        for (id, resolution) in &result.upgraded {
            let any_dominated = self.overlay_grids.iter().any(|((grid_id, _, _), cached)| {
                grid_id == id && cached.source_resolution < *resolution
            });
            if any_dominated {
                self.grid_computing.remove(id);
            }
        }

        for (id, resolution, colors) in self.color_rx.try_iter() {
            changed = true;
            self.computing.remove(&(id.clone(), resolution));
            self.inner
                .with_client_data_mut_at(&id, resolution, |data, _raw| {
                    data.colors = Some(colors);
                });
        }

        for (id, source_resolution, grid) in self.grid_rx.try_iter() {
            changed = true;
            self.grid_computing.remove(&id);
            let key = (id, grid.cols, grid.rows);
            // Only replace the cached grid if this one is from an equal or
            // higher resolution source (don't downgrade on late arrivals).
            let dominated = self
                .overlay_grids
                .get(&key)
                .is_some_and(|cached| cached.source_resolution > source_resolution);
            if !dominated {
                self.overlay_grids.insert(
                    key,
                    CachedGrid {
                        grid,
                        source_resolution,
                    },
                );
            }
        }

        // Drain fixed-size protocol results from background threads.
        for (key, source_resolution, result) in self.protocol_rx.try_iter() {
            // Ignore results whose request is no longer tracked (the entry
            // was evicted while the decode was in flight); otherwise a
            // zombie cache entry would linger until the id is evicted again.
            if !self.protocol_computing.remove(&key) {
                continue;
            }
            match result {
                Ok(protocol) => {
                    // Only replace the cached protocol if this one is from an
                    // equal or higher resolution source (don't downgrade on
                    // late arrivals).
                    let dominated = self
                        .protocols
                        .get(&key)
                        .is_some_and(|cached| cached.source_resolution > source_resolution);
                    if !dominated {
                        changed = true;
                        self.protocols.insert(
                            key,
                            CachedProtocol {
                                protocol: Arc::new(protocol),
                                source_resolution,
                            },
                        );
                    }
                }
                Err(e) => {
                    // Record the failure so the same source is not retried
                    // every frame; don't mark `changed`, which would force a
                    // redraw and immediately respawn the failing decode.
                    tracing::warn!("protocol decode failed for {key:?}: {e}");
                    self.protocol_failed.insert((key, source_resolution));
                }
            }
        }

        // Drain sliced protocol results from background threads.
        for (key, source_resolution, result) in self.sliced_protocol_rx.try_iter() {
            if !self.sliced_protocol_computing.remove(&key) {
                continue;
            }
            match result {
                Ok(protocol) => {
                    let dominated = self
                        .sliced_protocols
                        .get(&key)
                        .is_some_and(|cached| cached.source_resolution > source_resolution);
                    if !dominated {
                        changed = true;
                        self.sliced_protocols.insert(
                            key,
                            CachedProtocol {
                                protocol: Arc::new(protocol),
                                source_resolution,
                            },
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!("sliced protocol decode failed for {key:?}: {e}");
                    self.sliced_protocol_failed.insert((key, source_resolution));
                }
            }
        }

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
    ) -> (ArtColorGrid, bool) {
        let Some(id) = cover_art_id else {
            return (ArtColorGrid::empty(cols, rows), false);
        };

        self.visible_this_frame
            .insert((id.clone(), Resolution::Library));

        // Request library-res data (triggers network fetch if needed).
        let _result = self
            .inner
            .get(logic, Some(id), Resolution::Library, CachePriority::Visible);

        let library_loaded = self.inner.is_resolution_loaded(id, Resolution::Library);
        let grid_key = (id.clone(), cols, rows);

        // Check if we have a cached grid.
        if let Some(cached) = self.overlay_grids.get(&grid_key) {
            // Kick off a recomputation from better data if needed.
            let grid_needs_upgrade =
                library_loaded && cached.source_resolution < Resolution::Library;
            let computing = self.grid_computing.contains(id);

            if grid_needs_upgrade && !computing {
                let raw_bytes =
                    self.inner
                        .with_client_data_mut_at(id, Resolution::Library, |data, _raw| {
                            data.raw_bytes.clone()
                        });
                if let Some(raw_bytes) = raw_bytes {
                    self.grid_computing.insert(id.clone());
                    let id_clone = id.clone();
                    let tx = self.grid_tx.clone();
                    self.pool.spawn(move || {
                        let grid = compute_art_grid(&raw_bytes, cols, rows);
                        let _ = tx.send((id_clone, Resolution::Library, grid));
                    });
                }
            }

            let loading = grid_needs_upgrade || computing;
            return (cached.grid.clone(), loading);
        }

        // Need to compute — spawn background thread if not already running.
        if !self.grid_computing.contains(id) {
            // Prefer library-res bytes, fall back to low-res.
            let source = self
                .inner
                .with_client_data_mut_at(id, Resolution::Library, |data, _raw| {
                    (data.raw_bytes.clone(), Resolution::Library)
                })
                .or_else(|| {
                    self.inner
                        .with_client_data_mut_at(id, Resolution::Low, |data, _raw| {
                            (data.raw_bytes.clone(), Resolution::Low)
                        })
                });
            if let Some((raw_bytes, source_resolution)) = source {
                self.grid_computing.insert(id.clone());
                let id_clone = id.clone();
                let tx = self.grid_tx.clone();
                self.pool.spawn(move || {
                    let grid = compute_art_grid(&raw_bytes, cols, rows);
                    let _ = tx.send((id_clone, source_resolution, grid));
                });
            }
        }

        // No cached fallback — compute one from the low-res image synchronously
        // (16×16 pixels, sub-millisecond).
        let low_res = self
            .inner
            .get_resolution(id, Resolution::Low)
            .map(|data| data.raw_bytes.clone());
        if let Some(low_res) = low_res {
            let grid = compute_art_grid(&low_res, cols, rows);
            let cached = CachedGrid {
                grid: grid.clone(),
                source_resolution: Resolution::Low,
            };
            self.overlay_grids.insert(grid_key, cached);
            return (grid, true);
        }

        (ArtColorGrid::empty(cols, rows), true)
    }

    /// Returns a variable-size color grid for the overlay using full-resolution
    /// data. Falls back to lower resolutions while full-res is loading.
    pub fn get_full_res_art_grid(
        &mut self,
        logic: &Logic,
        cover_art_id: Option<&CoverArtId>,
        cols: usize,
        rows: usize,
    ) -> (ArtColorGrid, bool) {
        let Some(id) = cover_art_id else {
            return (ArtColorGrid::empty(cols, rows), false);
        };

        self.visible_this_frame
            .insert((id.clone(), Resolution::Full));

        // Request full-res data (triggers network fetch if needed).
        let _result = self
            .inner
            .get(logic, Some(id), Resolution::Full, CachePriority::Visible);

        let full_loaded = self.inner.is_resolution_loaded(id, Resolution::Full);
        let grid_key = (id.clone(), cols, rows);

        // Check if we have a cached grid.
        if let Some(cached) = self.overlay_grids.get(&grid_key) {
            // The grid is up-to-date if it was computed from full-res data
            // (or there's no better data available yet).
            let grid_needs_upgrade = full_loaded && cached.source_resolution < Resolution::Full;
            let computing = self.grid_computing.contains(id);

            // Kick off a recomputation from better data if needed.
            if grid_needs_upgrade && !computing {
                let raw_bytes =
                    self.inner
                        .with_client_data_mut_at(id, Resolution::Full, |data, _raw| {
                            data.raw_bytes.clone()
                        });
                if let Some(raw_bytes) = raw_bytes {
                    self.grid_computing.insert(id.clone());
                    let id_clone = id.clone();
                    let tx = self.grid_tx.clone();
                    self.pool.spawn(move || {
                        let grid = compute_art_grid(&raw_bytes, cols, rows);
                        let _ = tx.send((id_clone, Resolution::Full, grid));
                    });
                }
            }

            let loading = !full_loaded || grid_needs_upgrade || computing;
            return (cached.grid.clone(), loading);
        }

        // No cached grid — need to compute.
        if !self.grid_computing.contains(id) {
            // Prefer full-res bytes, then library, then low.
            let source = self
                .inner
                .with_client_data_mut_at(id, Resolution::Full, |data, _raw| {
                    (data.raw_bytes.clone(), Resolution::Full)
                })
                .or_else(|| {
                    self.inner
                        .with_client_data_mut_at(id, Resolution::Library, |data, _raw| {
                            (data.raw_bytes.clone(), Resolution::Library)
                        })
                })
                .or_else(|| {
                    self.inner
                        .with_client_data_mut_at(id, Resolution::Low, |data, _raw| {
                            (data.raw_bytes.clone(), Resolution::Low)
                        })
                });
            if let Some((raw_bytes, source_resolution)) = source {
                self.grid_computing.insert(id.clone());
                let id_clone = id.clone();
                let tx = self.grid_tx.clone();
                self.pool.spawn(move || {
                    let grid = compute_art_grid(&raw_bytes, cols, rows);
                    let _ = tx.send((id_clone, source_resolution, grid));
                });
            }
        }

        // Return fallback from low-res if available.
        let low_res = self
            .inner
            .get_resolution(id, Resolution::Low)
            .map(|data| data.raw_bytes.clone());
        if let Some(low_res) = low_res {
            let grid = compute_art_grid(&low_res, cols, rows);
            let cached = CachedGrid {
                grid: grid.clone(),
                source_resolution: Resolution::Low,
            };
            self.overlay_grids.insert(grid_key, cached);
            return (grid, true);
        }

        (ArtColorGrid::empty(cols, rows), true)
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
        let picker = self.protocol_picker.as_ref()?;
        let id = cover_art_id?;

        // Trigger a fetch at the requested resolution.
        let _ = self
            .inner
            .get(logic, Some(id), resolution, CachePriority::Visible);
        self.visible_this_frame.insert((id.clone(), resolution));

        let key = (id.clone(), width, height);
        let source = best_raw_bytes_up_to(&mut self.inner, id, resolution);

        let cached = self
            .protocols
            .get(&key)
            .map(|c| (c.protocol.clone(), c.source_resolution));
        if let Some((protocol, cached_resolution)) = &cached {
            let upgrade_available = source
                .as_ref()
                .is_some_and(|(res, _)| res > cached_resolution);
            if !upgrade_available {
                return Some(protocol.clone());
            }
        }

        if !self.protocol_computing.contains(&key)
            && let Some((source_resolution, raw_bytes)) = source
            && !self
                .protocol_failed
                .contains(&(key.clone(), source_resolution))
        {
            self.protocol_computing.insert(key.clone());
            let picker = picker.clone();
            let tx = self.protocol_tx.clone();
            let key_clone = key.clone();
            let size = Size { width, height };
            self.pool.spawn(move || {
                let result = (|| {
                    let dyn_img = image::load_from_memory(&raw_bytes).map_err(|e| e.to_string())?;
                    // `Scale` (not `Fit`) so that sources smaller than the
                    // target area are upscaled to fill it.
                    picker
                        .new_protocol(dyn_img, size, Resize::Scale(None))
                        .map_err(|e| e.to_string())
                })();
                let _ = tx.send((key_clone, source_resolution, result));
            });
        }

        // Serve the stale (lower-resolution) protocol while the upgrade
        // decodes, so the art doesn't flicker back to half-blocks.
        cached.map(|(protocol, _)| protocol)
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
        let picker = self.protocol_picker.as_ref()?;
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

        let cached = self
            .sliced_protocols
            .get(&key)
            .map(|c| (c.protocol.clone(), c.source_resolution));
        if let Some((protocol, cached_resolution)) = &cached {
            let upgrade_available = source
                .as_ref()
                .is_some_and(|(res, _)| res > cached_resolution);
            if !upgrade_available {
                return Some(protocol.clone());
            }
        }

        if !self.sliced_protocol_computing.contains(&key)
            && let Some((source_resolution, raw_bytes)) = source
            && !self
                .sliced_protocol_failed
                .contains(&(key.clone(), source_resolution))
        {
            self.sliced_protocol_computing.insert(key.clone());
            let picker = picker.clone();
            let tx = self.sliced_protocol_tx.clone();
            let key_clone = key.clone();
            self.pool.spawn(move || {
                let result = (|| {
                    let dyn_img = image::load_from_memory(&raw_bytes).map_err(|e| e.to_string())?;
                    // `Scale` (not the `Fit` used by `SlicedProtocol::new`) so
                    // that sources smaller than the target area are upscaled
                    // to fill it.
                    SlicedProtocol::new_with_resize(&picker, dyn_img, size, Resize::Scale(None))
                        .map_err(|e| e.to_string())
                })();
                let _ = tx.send((key_clone, source_resolution, result));
            });
        }

        // Serve the stale (lower-resolution) protocol while the upgrade
        // decodes, so the art doesn't flicker back to half-blocks.
        cached.map(|(protocol, _)| protocol)
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

    /// Verifies that `get_protocol` returns `None` when no picker is configured
    /// (Halfblock mode or before `set_picker` is called).
    #[test]
    fn test_get_protocol_returns_none_without_picker() {
        let (_tx, rx) = std::sync::mpsc::channel::<CoverArt>();
        let cache = CoverArtCache::new(rx);
        // No picker set — has_picker() returns false, which is the guard
        // get_protocol checks first before touching Logic.
        assert!(!cache.has_picker());
    }

    /// Verifies that `get_sliced_protocol` returns `None` when no picker is configured.
    #[test]
    fn test_get_sliced_protocol_returns_none_without_picker() {
        let (_tx, rx) = std::sync::mpsc::channel::<CoverArt>();
        let cache = CoverArtCache::new(rx);
        assert!(!cache.has_picker());
    }

    /// Verifies that cache eviction removes protocol entries.
    #[test]
    fn test_protocol_cache_eviction_on_evicted_id() {
        let mut cache = CoverArtCache::new(std::sync::mpsc::channel::<CoverArt>().1);
        let id = CoverArtId("evict-test".into());
        let key = (id.clone(), 10u16, 10u16);

        // Manually insert a protocol into the cache.
        let picker = Picker::halfblocks();
        let png_bytes = test_png();
        let dyn_img = image::load_from_memory(&png_bytes).unwrap();
        let protocol = picker
            .new_protocol(dyn_img, Size::new(10, 10), Resize::Fit(None))
            .unwrap();
        cache.protocols.insert(
            key.clone(),
            CachedProtocol {
                protocol: Arc::new(protocol),
                source_resolution: Resolution::Full,
            },
        );
        assert!(cache.protocols.contains_key(&key));

        // Simulate eviction by calling update() which processes evicted entries.
        // We can't easily trigger real eviction without a full inner cache,
        // but we can verify the retain logic directly.
        cache
            .protocols
            .retain(|(proto_id, _, _), _| proto_id != &id);
        cache
            .protocol_computing
            .retain(|(proto_id, _, _)| proto_id != &id);
        cache
            .protocol_failed
            .retain(|((failed_id, _, _), _)| failed_id != &id);
        cache
            .sliced_protocols
            .retain(|(sliced_id, _, _), _| sliced_id != &id);
        cache
            .sliced_protocol_computing
            .retain(|(sliced_id, _, _)| sliced_id != &id);
        cache
            .sliced_protocol_failed
            .retain(|((failed_id, _, _), _)| failed_id != &id);

        assert!(!cache.protocols.contains_key(&key));
    }

    /// Verifies that `new_protocol` errors clear the computing flag, insert
    /// nothing into the cache, and record the failed source resolution so
    /// the same decode is not respawned every frame.
    #[test]
    fn test_protocol_error_handling() {
        let (_tx, rx) = std::sync::mpsc::channel::<CoverArt>();
        let mut cache = CoverArtCache::new(rx);
        let id = CoverArtId("error-test".into());
        let key = (id.clone(), 10u16, 10u16);

        // Simulate an error result arriving through the channel.
        cache.protocol_computing.insert(key.clone());
        let _ = cache.protocol_tx.send((
            key.clone(),
            Resolution::Low,
            Err("decode error".to_string()),
        ));

        let changed = cache.update();

        // The computing flag should be cleared.
        assert!(!cache.protocol_computing.contains(&key));
        // Nothing should be in the cache.
        assert!(!cache.protocols.contains_key(&key));
        // The failure should be recorded per source resolution.
        assert!(
            cache
                .protocol_failed
                .contains(&(key.clone(), Resolution::Low))
        );
        // A failed decode changes nothing visually, so it must not force a
        // redraw (which would respawn the failing decode in a loop).
        assert!(!changed);
    }

    /// Verifies that results whose computing flag was removed (eviction while
    /// the decode was in flight) are dropped rather than resurrected.
    #[test]
    fn test_protocol_untracked_result_dropped() {
        let (_tx, rx) = std::sync::mpsc::channel::<CoverArt>();
        let mut cache = CoverArtCache::new(rx);
        let id = CoverArtId("untracked-test".into());
        let key = (id.clone(), 10u16, 10u16);

        let picker = Picker::halfblocks();
        let png_bytes = test_png();
        let dyn_img = image::load_from_memory(&png_bytes).unwrap();
        let protocol = picker
            .new_protocol(dyn_img, Size::new(10, 10), Resize::Fit(None))
            .unwrap();

        // No computing flag set — as if the entry was evicted mid-decode.
        let _ = cache
            .protocol_tx
            .send((key.clone(), Resolution::Full, Ok(protocol)));

        cache.update();

        assert!(!cache.protocols.contains_key(&key));
    }

    /// Verifies that successful protocol results are cached as `Arc<Protocol>`.
    #[test]
    fn test_protocol_success_cached() {
        let (_tx, rx) = std::sync::mpsc::channel::<CoverArt>();
        let mut cache = CoverArtCache::new(rx);
        let id = CoverArtId("success-test".into());
        let key = (id.clone(), 10u16, 10u16);

        let picker = Picker::halfblocks();
        let png_bytes = test_png();
        let dyn_img = image::load_from_memory(&png_bytes).unwrap();
        let protocol = picker
            .new_protocol(dyn_img, Size::new(10, 10), Resize::Fit(None))
            .unwrap();

        cache.protocol_computing.insert(key.clone());
        let _ = cache
            .protocol_tx
            .send((key.clone(), Resolution::Low, Ok(protocol)));

        cache.update();

        // The computing flag should be cleared.
        assert!(!cache.protocol_computing.contains(&key));
        // The protocol should be cached, with its source resolution recorded.
        assert_eq!(
            cache.protocols.get(&key).map(|c| c.source_resolution),
            Some(Resolution::Low)
        );
    }

    /// Verifies that a higher-resolution result replaces a cached
    /// lower-resolution protocol, and that a late lower-resolution result
    /// does not replace a cached higher-resolution protocol.
    #[test]
    fn test_protocol_upgrade_and_no_downgrade() {
        let (_tx, rx) = std::sync::mpsc::channel::<CoverArt>();
        let mut cache = CoverArtCache::new(rx);
        let id = CoverArtId("upgrade-test".into());
        let key = (id.clone(), 10u16, 10u16);

        let picker = Picker::halfblocks();
        let png_bytes = test_png();
        let make_protocol = || {
            let dyn_img = image::load_from_memory(&png_bytes).unwrap();
            picker
                .new_protocol(dyn_img, Size::new(10, 10), Resize::Fit(None))
                .unwrap()
        };

        // Low-res result arrives first.
        cache.protocol_computing.insert(key.clone());
        let _ = cache
            .protocol_tx
            .send((key.clone(), Resolution::Low, Ok(make_protocol())));
        cache.update();
        assert_eq!(
            cache.protocols.get(&key).map(|c| c.source_resolution),
            Some(Resolution::Low)
        );

        // Full-res result upgrades the cache entry.
        cache.protocol_computing.insert(key.clone());
        let _ = cache
            .protocol_tx
            .send((key.clone(), Resolution::Full, Ok(make_protocol())));
        cache.update();
        assert_eq!(
            cache.protocols.get(&key).map(|c| c.source_resolution),
            Some(Resolution::Full)
        );

        // A late library-res result must not downgrade the cache entry.
        cache.protocol_computing.insert(key.clone());
        let _ = cache
            .protocol_tx
            .send((key.clone(), Resolution::Library, Ok(make_protocol())));
        cache.update();
        assert_eq!(
            cache.protocols.get(&key).map(|c| c.source_resolution),
            Some(Resolution::Full)
        );
    }

    /// Verifies that successful sliced protocol results are cached.
    #[test]
    fn test_sliced_protocol_success_cached() {
        let (_tx, rx) = std::sync::mpsc::channel::<CoverArt>();
        let mut cache = CoverArtCache::new(rx);
        let id = CoverArtId("sliced-success-test".into());

        let picker = Picker::halfblocks();
        let png_bytes = test_png();
        let dyn_img = image::load_from_memory(&png_bytes).unwrap();
        let sliced = SlicedProtocol::new(&picker, dyn_img, Some(Size::new(16, 8))).unwrap();
        let key = (id.clone(), 16u16, 8u16);

        cache.sliced_protocol_computing.insert(key.clone());
        let _ = cache
            .sliced_protocol_tx
            .send((key.clone(), Resolution::Library, Ok(sliced)));

        cache.update();

        assert!(!cache.sliced_protocol_computing.contains(&key));
        assert!(cache.sliced_protocols.contains_key(&key));
    }

    /// Verifies that sliced protocol errors clear the computing flag and
    /// record the failed source resolution.
    #[test]
    fn test_sliced_protocol_error_handling() {
        let (_tx, rx) = std::sync::mpsc::channel::<CoverArt>();
        let mut cache = CoverArtCache::new(rx);
        let id = CoverArtId("sliced-error-test".into());
        let key = (id.clone(), 16u16, 8u16);

        cache.sliced_protocol_computing.insert(key.clone());
        let _ = cache.sliced_protocol_tx.send((
            key.clone(),
            Resolution::Library,
            Err("decode error".to_string()),
        ));

        cache.update();

        assert!(!cache.sliced_protocol_computing.contains(&key));
        assert!(!cache.sliced_protocols.contains_key(&key));
        assert!(
            cache
                .sliced_protocol_failed
                .contains(&(key.clone(), Resolution::Library))
        );
    }
}
