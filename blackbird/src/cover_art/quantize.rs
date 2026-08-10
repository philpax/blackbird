//! Color quantization of cover art images into terminal-cell color grids,
//! used by the half-block rendering fallback.

use std::io::Cursor;

use ratatui::style::Color;

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

/// Reads the image header to extract the aspect ratio (height / width)
/// without decoding the full pixel data.
pub(super) fn image_aspect_ratio(data: &[u8]) -> Option<f64> {
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
pub fn compute_quadrant_colors(image_data: &[u8]) -> ArtColors {
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
pub fn compute_art_grid(image_data: &[u8], cols: usize, rows: usize) -> ArtColorGrid {
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
