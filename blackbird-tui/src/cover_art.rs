use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use blackbird_core::{blackbird_state::CoverArtId, CoverArt, Logic};
use ratatui::style::Color;

/// 4 columns × 2 rows of colours extracted from album art.
/// This ratio better matches terminal character aspect ratio (chars are ~2x tall as wide).
#[derive(Debug, Clone, Copy)]
pub struct ArtColors {
    /// Colors arranged as [row][col], where row 0 is top, col 0 is left.
    pub colors: [[Color; 4]; 2],
}

impl Default for ArtColors {
    fn default() -> Self {
        Self {
            colors: [[Color::DarkGray; 4]; 2],
        }
    }
}

// Keep the old name as an alias for compatibility during transition.
pub type QuadrantColors = ArtColors;

pub struct CoverArtCache {
    cover_art_loaded_rx: std::sync::mpsc::Receiver<CoverArt>,
    cache: HashMap<CoverArtId, CacheEntry>,
    cache_dir: PathBuf,
}

impl CoverArtCache {
    pub fn new(cover_art_loaded_rx: std::sync::mpsc::Receiver<CoverArt>) -> Self {
        let cache_dir = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(CACHE_DIR_NAME);

        if let Err(e) = std::fs::create_dir_all(&cache_dir) {
            tracing::warn!("Failed to create cache directory: {e}");
        }

        Self {
            cover_art_loaded_rx,
            cache: HashMap::new(),
            cache_dir,
        }
    }

    pub fn update(&mut self) {
        // Process incoming cover art.
        for incoming in self.cover_art_loaded_rx.try_iter() {
            if let Some(entry) = self.cache.get_mut(&incoming.cover_art_id) {
                let colors = compute_quadrant_colors(&incoming.cover_art);
                entry.state = CacheEntryState::Loaded(colors);
                tracing::debug!("Loaded cover art colours for {}", incoming.cover_art_id);

                // Also save to disk cache if not already present.
                let safe_filename = incoming
                    .cover_art_id
                    .0
                    .replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
                let cache_path = self.cache_dir.join(format!("{safe_filename}.png"));
                if !cache_path.exists() {
                    let cover_art = incoming.cover_art.clone();
                    std::thread::spawn(move || {
                        save_to_disk_cache(&cache_path, &cover_art);
                    });
                }
            }
        }

        // Evict timed-out entries.
        self.cache.retain(|id, entry| {
            let keep = entry.last_requested.elapsed() <= CACHE_ENTRY_TIMEOUT;
            if !keep {
                tracing::debug!("Evicting cover art for {id} from TUI cache");
            }
            keep
        });

        // Evict excess entries (oldest first).
        if self.cache.len() > MAX_CACHE_SIZE {
            let overage = self.cache.len() - MAX_CACHE_SIZE;
            let mut entries: Vec<_> = self.cache.keys().cloned().collect();
            entries.sort_by_key(|id| {
                self.cache
                    .get(id)
                    .map(|e| e.first_requested)
                    .unwrap_or(std::time::Instant::now())
            });
            for id in entries.into_iter().take(overage) {
                self.cache.remove(&id);
            }
        }
    }

    pub fn get(&mut self, logic: &Logic, cover_art_id: Option<&CoverArtId>) -> QuadrantColors {
        let Some(cover_art_id) = cover_art_id else {
            return QuadrantColors::default();
        };

        let entry = self
            .cache
            .entry(cover_art_id.clone())
            .or_insert(CacheEntry {
                first_requested: std::time::Instant::now(),
                last_requested: std::time::Instant::now(),
                state: CacheEntryState::Unloaded,
            });

        entry.last_requested = std::time::Instant::now();

        // Try loading from disk cache first.
        if let CacheEntryState::Unloaded = entry.state {
            if let Some(data) = load_from_disk_cache(&self.cache_dir, cover_art_id) {
                let colors = compute_quadrant_colors(&data);
                entry.state = CacheEntryState::Loaded(colors);
                return colors;
            }
        }

        // Request from network after delay.
        if entry.first_requested.elapsed() > TIME_BEFORE_LOAD_ATTEMPT {
            if let CacheEntryState::Unloaded = entry.state {
                // Request a small size since we only need colours.
                logic.request_cover_art(cover_art_id, Some(64));
                entry.state = CacheEntryState::Loading;
            }
        }

        match &entry.state {
            CacheEntryState::Loaded(colors) => *colors,
            _ => QuadrantColors::default(),
        }
    }
}

const TIME_BEFORE_LOAD_ATTEMPT: Duration = Duration::from_millis(100);
const CACHE_ENTRY_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_CACHE_SIZE: usize = 50;
const CACHE_DIR_NAME: &str = "album-art-cache";

struct CacheEntry {
    first_requested: std::time::Instant,
    last_requested: std::time::Instant,
    state: CacheEntryState,
}

enum CacheEntryState {
    Unloaded,
    Loading,
    Loaded(QuadrantColors),
}

/// Computes the average colour of each region in a 4×2 grid (4 cols, 2 rows).
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

    // 4 columns, 2 rows
    let col_width = w / 4;
    let row_height = h / 2;

    let mut colors = [[Color::DarkGray; 4]; 2];
    for row in 0..2 {
        for col in 0..4 {
            let x0 = col * col_width;
            let y0 = row * row_height;
            let x1 = if col == 3 { w } else { (col + 1) * col_width };
            let y1 = if row == 1 { h } else { (row + 1) * row_height };
            colors[row][col] = average_region(x0, y0, x1.max(x0 + 1), y1.max(y0 + 1));
        }
    }

    ArtColors { colors }
}

fn load_from_disk_cache(cache_dir: &Path, cover_art_id: &CoverArtId) -> Option<Arc<[u8]>> {
    let safe_filename = cover_art_id
        .0
        .replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
    let path = cache_dir.join(format!("{safe_filename}.png"));
    std::fs::read(&path).ok().map(|d| d.into())
}

fn save_to_disk_cache(cache_path: &Path, image_data: &[u8]) {
    let Ok(img) = image::load_from_memory(image_data) else {
        return;
    };

    let resized = img.resize_exact(16, 16, image::imageops::FilterType::Triangle);
    let blurred = image::imageops::fast_blur(&resized.into_rgb8(), 1.0);

    let mut buffer = std::io::Cursor::new(Vec::new());
    if blurred
        .write_to(&mut buffer, image::ImageFormat::Png)
        .is_err()
    {
        return;
    }

    let _ = std::fs::write(cache_path, buffer.into_inner());
}
