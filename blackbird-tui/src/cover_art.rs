use std::{
    collections::{HashMap, HashSet},
    io::Cursor,
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, Sender},
    },
    time::Duration,
};

use blackbird_client_shared::cover_art_cache::{self, CachePriority, ClientData, Resolution};
use blackbird_core::{CoverArt, Logic, blackbird_state::CoverArtId};
use ratatui::style::Color;

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
                while let Ok(job) = rx.lock().unwrap().recv() {
                    job();
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
}

/// An overlay grid together with the resolution of the source image it
/// was computed from, so we know when to trigger recomputation.
struct CachedGrid {
    grid: ArtColorGrid,
    source_resolution: Resolution,
}

impl CoverArtCache {
    pub fn new(cover_art_loaded_rx: std::sync::mpsc::Receiver<CoverArt>) -> Self {
        let (color_tx, color_rx) = std::sync::mpsc::channel();
        let (grid_tx, grid_rx) = std::sync::mpsc::channel();
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
        }
    }

    pub fn update(&mut self) {
        let result = self.inner.update();
        for id in &result.evicted {
            // Remove all resolution-keyed entries for this id.
            self.computing.remove(&(id.clone(), Resolution::Low));
            self.computing.remove(&(id.clone(), Resolution::Library));
            self.computing.remove(&(id.clone(), Resolution::Full));
            self.grid_computing.remove(id);
            self.overlay_grids
                .retain(|(grid_id, _, _), _| grid_id != id);
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
            self.computing.remove(&(id.clone(), resolution));
            self.inner
                .with_client_data_mut_at(&id, resolution, |data, _raw| {
                    data.colors = Some(colors);
                });
        }

        for (id, source_resolution, grid) in self.grid_rx.try_iter() {
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
    }

    /// Get quadrant colors for a cover art entry at low resolution.
    /// Used for LeftOfAlbum thumbnail colors.
    pub fn get(&mut self, logic: &Logic, cover_art_id: Option<&CoverArtId>) -> QuadrantColors {
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
fn compute_quadrant_colors(image_data: &[u8]) -> ArtColors {
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
fn compute_art_grid(image_data: &[u8], cols: usize, rows: usize) -> ArtColorGrid {
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

            if count > 0 {
                *cell = Color::Rgb(
                    (r_sum / count) as u8,
                    (g_sum / count) as u8,
                    (b_sum / count) as u8,
                );
            }
        }
    }

    ArtColorGrid {
        colors: grid,
        cols,
        rows,
    }
}
