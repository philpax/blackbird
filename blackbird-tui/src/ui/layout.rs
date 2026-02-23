use ratatui::layout::{Constraint, Direction, Layout, Rect};

// ── Main vertical layout ────────────────────────────────────────────────────

pub const NOW_PLAYING_HEIGHT: u16 = 2;
pub const SCRUB_BAR_HEIGHT: u16 = 1;
pub const CONTENT_MIN_HEIGHT: u16 = 3;
pub const HELP_BAR_HEIGHT: u16 = 1;

pub struct MainLayout {
    pub now_playing: Rect,
    pub scrub_bar: Rect,
    pub content: Rect,
    pub help_bar: Rect,
}

pub fn split_main(area: Rect) -> MainLayout {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(NOW_PLAYING_HEIGHT),
            Constraint::Length(SCRUB_BAR_HEIGHT),
            Constraint::Min(CONTENT_MIN_HEIGHT),
            Constraint::Length(HELP_BAR_HEIGHT),
        ])
        .split(area);
    MainLayout {
        now_playing: chunks[0],
        scrub_bar: chunks[1],
        content: chunks[2],
        help_bar: chunks[3],
    }
}

// ── Now-playing horizontal layout ───────────────────────────────────────────

pub const TRACK_INFO_MIN_WIDTH: u16 = 20;
pub const TRANSPORT_WIDTH: u16 = 24;

pub struct NowPlayingLayout {
    pub album_art: Rect,
    pub track_info: Rect,
    pub transport: Rect,
}

pub fn split_now_playing(area: Rect) -> NowPlayingLayout {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(art_cols() + 2),
            Constraint::Min(TRACK_INFO_MIN_WIDTH),
            Constraint::Length(TRANSPORT_WIDTH),
        ])
        .split(area);
    NowPlayingLayout {
        album_art: chunks[0],
        track_info: chunks[1],
        transport: chunks[2],
    }
}

// ── Scrub bar + volume horizontal layout ────────────────────────────────────

pub const SCRUB_BAR_MIN_WIDTH: u16 = 20;
pub const VOLUME_SLIDER_WIDTH: u16 = 16;
pub const VOLUME_ICON_WIDTH: u16 = 2;
pub const VOLUME_BAR_PADDING: u16 = 7; // = ICON (2) + LABEL (5)

pub struct ScrubVolumeLayout {
    pub scrub_bar: Rect,
    pub volume: Rect,
}

pub fn split_scrub_volume(area: Rect) -> ScrubVolumeLayout {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(SCRUB_BAR_MIN_WIDTH),
            Constraint::Length(VOLUME_SLIDER_WIDTH),
        ])
        .split(area);
    ScrubVolumeLayout {
        scrub_bar: chunks[0],
        volume: chunks[1],
    }
}

// ── Album art (now-playing & library) ───────────────────────────────────────

const ART_PIXEL_ROWS: u16 = 4;
pub const ART_TERM_ROWS: u16 = 2;
pub const ART_LEFT_MARGIN: u16 = 1;

/// Returns the number of display columns for the 4x4 art grid, corrected for
/// non-square terminal character cells via nearest-neighbor stretching.
pub(crate) fn art_cols() -> u16 {
    (ART_PIXEL_ROWS as f64 / half_block_correction())
        .round()
        .clamp(ART_PIXEL_ROWS as f64, 8.0) as u16
}

/// Returns the first column past the album art (margin + display columns).
pub(crate) fn art_end_col() -> u16 {
    ART_LEFT_MARGIN + art_cols()
}

// ── Transport buttons ───────────────────────────────────────────────────────

pub const TRANSPORT_BUTTON_GROUP_WIDTH: u16 = 10;
pub const TRANSPORT_BTN_PREV: u16 = 0;
pub const TRANSPORT_BTN_PLAY: u16 = 3;
pub const TRANSPORT_BTN_STOP: u16 = 6;
pub const TRANSPORT_BTN_NEXT: u16 = 9;

// ── Album art overlay ───────────────────────────────────────────────────────

pub const OVERLAY_WIDTH_FRACTION: f32 = 0.9;
pub const OVERLAY_MIN_WIDTH: u16 = 10;
pub const OVERLAY_BORDER_OVERHEAD: u16 = 2;
pub const OVERLAY_X_BUTTON_OFFSET: u16 = 4;

/// Default half-block pixel aspect ratio (width / height) used when the
/// terminal does not report pixel dimensions. Empirically tuned for Windows
/// Terminal with Iosevka.
const DEFAULT_HALF_BLOCK_RATIO: f64 = 10.0 / 13.0;

/// Aspect ratio of a single half-block pixel (width / height) for the current
/// terminal. A half-block pixel occupies one column and half a character cell
/// row, so its dimensions are `char_width × (char_height / 2)`. This ratio is
/// used to convert between column counts and half-block row counts so that art
/// appears with correct proportions regardless of the terminal font.
pub(crate) fn half_block_correction() -> f64 {
    let Ok(ws) = crossterm::terminal::window_size() else {
        return DEFAULT_HALF_BLOCK_RATIO;
    };
    if ws.width == 0 || ws.height == 0 || ws.columns == 0 || ws.rows == 0 {
        return DEFAULT_HALF_BLOCK_RATIO;
    }
    let char_width = ws.width as f64 / ws.columns as f64;
    let char_height = ws.height as f64 / ws.rows as f64;
    // Each half-block pixel is char_width wide and char_height/2 tall.
    2.0 * char_width / char_height
}

/// Computes the overlay rectangle, preserving the source image's aspect ratio
/// and ensuring the overlay never covers the now-playing bar or scrub bar.
///
/// `aspect_ratio` is the source image's height / width (1.0 for square).
pub fn overlay_rect(size: Rect, aspect_ratio: f64) -> Rect {
    // The overlay must sit below the now-playing bar and scrub bar.
    let min_y = NOW_PLAYING_HEIGHT + SCRUB_BAR_HEIGHT;
    let max_height = size.height.saturating_sub(min_y);

    if max_height < OVERLAY_BORDER_OVERHEAD + 1 || size.width < OVERLAY_MIN_WIDTH {
        return Rect::new(0, min_y, OVERLAY_MIN_WIDTH.min(size.width), 0);
    }

    // Combine image aspect ratio with the half-block correction so the art
    // appears with correct proportions regardless of the terminal font.
    let corrected_ratio = aspect_ratio * half_block_correction();

    // Start with the width-based sizing.
    let mut overlay_width = ((size.width as f32) * OVERLAY_WIDTH_FRACTION) as u16;
    overlay_width = overlay_width.max(OVERLAY_MIN_WIDTH).min(size.width);
    let art_cols = (overlay_width.saturating_sub(2)) as usize;

    // Derive art height from the corrected aspect ratio.
    let art_pixel_rows = ((art_cols as f64) * corrected_ratio).ceil() as usize;
    let art_term_rows = art_pixel_rows.div_ceil(2);
    let mut overlay_height = art_term_rows as u16 + OVERLAY_BORDER_OVERHEAD;

    // If too tall for the available space, constrain by height and shrink
    // the width so the aspect ratio is still correct.
    if overlay_height > max_height {
        overlay_height = max_height;
        let art_term_rows = overlay_height.saturating_sub(OVERLAY_BORDER_OVERHEAD) as usize;
        let art_pixel_rows = art_term_rows * 2;
        let art_cols = ((art_pixel_rows as f64) / corrected_ratio).floor() as usize;
        overlay_width = (art_cols as u16 + 2).max(OVERLAY_MIN_WIDTH).min(size.width);
    }

    let overlay_x = (size.width.saturating_sub(overlay_width)) / 2;
    let overlay_y = min_y + (max_height.saturating_sub(overlay_height)) / 2;
    Rect::new(overlay_x, overlay_y, overlay_width, overlay_height)
}

// ── Library geometry ────────────────────────────────────────────────────────

pub const TRACK_INDENT: usize = 5;
pub const HEART_COL_OFFSET: usize = 2;

pub struct LibraryGeometry {
    pub total_lines: usize,
    pub visible_height: usize,
    pub has_scrollbar: bool,
    pub list_width: usize,
    pub heart_col: usize,
}

/// Computes library geometry with the given scroll indicator width.
///
/// The `scroll_indicator_width` is the number of columns reserved for scroll
/// indicator labels (1 for single letters, 4 for full years).
pub fn library_geometry(
    area: Rect,
    total_lines: usize,
    scroll_indicator_width: usize,
) -> LibraryGeometry {
    let visible_height = area.height as usize;
    let has_scrollbar = total_lines > visible_height;
    // Reserve space for scroll indicator labels plus scrollbar track.
    let reserved = scroll_indicator_width + if has_scrollbar { 1 } else { 0 };
    let list_width = (area.width as usize).saturating_sub(reserved);
    let heart_col = area.x as usize + list_width.saturating_sub(HEART_COL_OFFSET);
    LibraryGeometry {
        total_lines,
        visible_height,
        has_scrollbar,
        list_width,
        heart_col,
    }
}

// ── Interaction constants ───────────────────────────────────────────────────

pub const PAGE_SCROLL_SIZE: usize = 20;
pub const SCROLL_WHEEL_STEPS: usize = 6;
pub use blackbird_client_shared::{SEEK_STEP_SECS, VOLUME_STEP};

// ── Log view ────────────────────────────────────────────────────────────────

pub const LOG_TARGET_WIDTH: usize = 24;
pub const LOG_TARGET_SUFFIX_LEN: usize = 21;
