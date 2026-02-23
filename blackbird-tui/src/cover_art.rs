use std::{
    collections::{HashMap, HashSet},
    io::Cursor,
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, Sender},
    },
    time::Duration,
};

use blackbird_client_shared::cover_art_cache::{self, CachePriority, ClientData};
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
    /// Kept across low→high-res transitions so the overlay can show a quick
    /// fallback grid while the high-res grid is computing.
    low_res_bytes: Option<Arc<[u8]>>,
    /// 4x4 color grid for library thumbnails (computed in background).
    pub colors: Option<QuadrantColors>,
    /// Variable-size grid for the overlay (lazily computed via with_client_data_mut).
    /// Reset to None when from_image_data is called (new image data arrived).
    pub overlay_grid: Option<ArtColorGrid>,
    /// Aspect ratio (height / width) of the source image. Computed from the
    /// image header so it is available without a full decode.
    aspect_ratio: Option<f64>,
}

impl ClientData for TuiCoverArt {
    fn from_image_data(data: &Arc<[u8]>, _id: &CoverArtId, is_high_res: bool) -> Self {
        // Low-res images (16×16 disk cache) are trivially cheap to process —
        // compute colors synchronously to avoid a frame of gray.
        let (colors, low_res_bytes) = if is_high_res {
            (None, None)
        } else {
            (Some(compute_quadrant_colors(data)), Some(data.clone()))
        };
        let aspect_ratio = image_aspect_ratio(data);
        TuiCoverArt {
            raw_bytes: data.clone(),
            low_res_bytes,
            colors,
            overlay_grid: None,
            aspect_ratio,
        }
    }

    fn carry_over(&mut self, previous: &Self) {
        if self.colors.is_none() {
            self.colors = previous.colors;
        }
        if self.low_res_bytes.is_none() {
            self.low_res_bytes = previous.low_res_bytes.clone();
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
    color_tx: Sender<(CoverArtId, QuadrantColors)>,
    color_rx: Receiver<(CoverArtId, QuadrantColors)>,
    computing: HashSet<CoverArtId>,
    grid_tx: Sender<(CoverArtId, ArtColorGrid)>,
    grid_rx: Receiver<(CoverArtId, ArtColorGrid)>,
    grid_computing: HashSet<CoverArtId>,
    /// Fallback overlay grids, persisted across image-data transitions so that
    /// the low-res grid remains visible while the high-res grid is computing.
    overlay_grids: HashMap<CoverArtId, ArtColorGrid>,
}

impl CoverArtCache {
    pub fn new(cover_art_loaded_rx: std::sync::mpsc::Receiver<CoverArt>) -> Self {
        let (color_tx, color_rx) = std::sync::mpsc::channel();
        let (grid_tx, grid_rx) = std::sync::mpsc::channel();
        Self {
            inner: cover_art_cache::CoverArtCache::new(
                cover_art_loaded_rx,
                None, // full resolution — overlay needs high-res data
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
        let evicted = self.inner.update();
        for id in &evicted {
            self.computing.remove(id);
            self.grid_computing.remove(id);
            self.overlay_grids.remove(id);
        }

        for (id, colors) in self.color_rx.try_iter() {
            self.computing.remove(&id);
            self.inner.with_client_data_mut(&id, |data, _raw| {
                data.colors = Some(colors);
            });
        }

        for (id, grid) in self.grid_rx.try_iter() {
            self.grid_computing.remove(&id);
            self.overlay_grids.insert(id.clone(), grid.clone());
            self.inner.with_client_data_mut(&id, |data, _raw| {
                data.overlay_grid = Some(grid);
            });
        }
    }

    pub fn get(&mut self, logic: &Logic, cover_art_id: Option<&CoverArtId>) -> QuadrantColors {
        let Some(tui_data) = self.inner.get(logic, cover_art_id, CachePriority::Visible) else {
            return QuadrantColors::default();
        };

        if let Some(colors) = tui_data.colors {
            return colors;
        }

        // Colors not computed yet — spawn background thread if not already running
        let id = cover_art_id.unwrap(); // safe: inner.get returned Some
        if !self.computing.contains(id) {
            self.computing.insert(id.clone());
            let raw = tui_data.raw_bytes.clone();
            let id_clone = id.clone();
            let tx = self.color_tx.clone();
            self.pool.spawn(move || {
                let colors = compute_quadrant_colors(&raw);
                let _ = tx.send((id_clone, colors));
            });
        }

        QuadrantColors::default()
    }

    /// Returns a variable-size color grid for the overlay display and whether
    /// a higher-resolution version is still being computed.
    /// Computes the grid in a background thread; returns the previous grid
    /// (or empty) while the new one is being computed.
    pub fn get_art_grid(
        &mut self,
        cover_art_id: Option<&CoverArtId>,
        cols: usize,
        rows: usize,
    ) -> (ArtColorGrid, bool) {
        let Some(id) = cover_art_id else {
            return (ArtColorGrid::empty(cols, rows), false);
        };

        // Check if client data already has a grid with matching dimensions.
        let up_to_date = self
            .inner
            .with_client_data_mut(id, |data, _raw| {
                data.overlay_grid
                    .as_ref()
                    .is_some_and(|g| g.cols == cols && g.rows == rows)
            })
            .unwrap_or(false);

        if up_to_date {
            let grid = self
                .inner
                .with_client_data_mut(id, |data, _raw| data.overlay_grid.clone().unwrap())
                .unwrap();
            self.overlay_grids.insert(id.clone(), grid.clone());
            return (grid, false);
        }

        // Need to compute — spawn background thread if not already running.
        if !self.grid_computing.contains(id) {
            let raw_bytes = self
                .inner
                .with_client_data_mut(id, |data, _raw| data.raw_bytes.clone());
            if let Some(raw_bytes) = raw_bytes {
                self.grid_computing.insert(id.clone());
                let id_clone = id.clone();
                let tx = self.grid_tx.clone();
                self.pool.spawn(move || {
                    let grid = compute_art_grid(&raw_bytes, cols, rows);
                    let _ = tx.send((id_clone, grid));
                });
            }
        }

        // Return the previous grid as fallback while computing.
        if let Some(grid) = self
            .overlay_grids
            .get(id)
            .filter(|g| g.cols == cols && g.rows == rows)
        {
            return (grid.clone(), true);
        }

        // No cached fallback — compute one from the low-res image synchronously
        // (16×16 pixels, sub-millisecond).
        let low_res = self
            .inner
            .with_client_data_mut(id, |data, _raw| data.low_res_bytes.clone())
            .flatten();
        if let Some(low_res) = low_res {
            let grid = compute_art_grid(&low_res, cols, rows);
            self.overlay_grids.insert(id.clone(), grid.clone());
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
