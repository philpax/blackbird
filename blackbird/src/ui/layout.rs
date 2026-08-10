use ratatui::layout::{Constraint, Direction, Layout, Rect, Size};

use blackbird_client_shared::config::SidebarPosition;

// ── Main vertical layout ────────────────────────────────────────────────────

pub const NOW_PLAYING_HEIGHT: u16 = 2;
pub const SCRUB_BAR_HEIGHT: u16 = 1;
pub const INLINE_LYRICS_HEIGHT: u16 = 3;
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

/// Computes the overlay rect for inline lyrics, anchored to the bottom of the
/// content area just above the help bar. Returns `None` if the content area is
/// too small.
pub fn inline_lyrics_overlay(content: Rect) -> Option<Rect> {
    if content.height < INLINE_LYRICS_HEIGHT {
        return None;
    }
    Some(Rect::new(
        content.x,
        content.y + content.height - INLINE_LYRICS_HEIGHT,
        content.width,
        INLINE_LYRICS_HEIGHT,
    ))
}

/// Splits `content` vertically into `(main, inline_lyrics)`. When inline
/// lyrics are shown they render as a panel at the bottom that cuts into the
/// main content's height (not an overlay). Returns `None` for the lyrics area
/// when the content is too small.
pub fn split_inline_lyrics(content: Rect) -> (Rect, Option<Rect>) {
    let Some(lyrics) = inline_lyrics_overlay(content) else {
        return (content, None);
    };
    let main = Rect::new(content.x, content.y, content.width, lyrics.y - content.y);
    (main, Some(lyrics))
}

// ── Lyrics sidebar horizontal layout ───────────────────────────────────────

/// Minimum width of the lyrics sidebar in terminal columns.
pub const LYRICS_SIDEBAR_MIN_WIDTH: u16 = 10;
/// Width of the separator border between the sidebar and main content.
pub const LYRICS_SIDEBAR_BORDER_WIDTH: u16 = 1;
/// Extra columns on each side of the border that count as drag handles, so
/// the 1-column border is easier to grab between the scrollbar and content.
pub const LYRICS_SIDEBAR_DRAG_TOLERANCE: u16 = 2;

/// The result of splitting the content area with an optional lyrics sidebar.
pub struct ContentLayout {
    /// The main content area (library, search, queue, etc.).
    pub main: Rect,
    /// The lyrics sidebar rect, if a sidebar is shown.
    pub lyrics_sidebar: Option<Rect>,
    /// The 1-column border between sidebar and main content, for drag hit-testing.
    pub lyrics_border: Option<Rect>,
}

/// Splits the content area into a main region and an optional sidebar.
///
/// For `Left`/`Right`, carves out the sidebar and a 1-column separator border.
/// The sidebar width is clamped to `[LYRICS_SIDEBAR_MIN_WIDTH, content.width / 2]`.
pub fn split_content_with_sidebar(
    content: Rect,
    position: SidebarPosition,
    sidebar_width: u16,
) -> ContentLayout {
    // Position is always Left/Right (existence is `sidebar.enabled`, handled
    // by the caller), so the sidebar is always carved out here.
    // Clamp the sidebar width to valid bounds.
    let max_width = (content.width / 2).max(LYRICS_SIDEBAR_MIN_WIDTH);
    let sidebar_w = sidebar_width
        .clamp(LYRICS_SIDEBAR_MIN_WIDTH, max_width)
        // Ensure sidebar + border doesn't exceed content width.
        .min(content.width.saturating_sub(LYRICS_SIDEBAR_BORDER_WIDTH));

    // If there isn't enough room for sidebar + border + at least 1 column of
    // main content, skip the sidebar.
    let total_sidebar = sidebar_w + LYRICS_SIDEBAR_BORDER_WIDTH;
    if total_sidebar >= content.width {
        return ContentLayout {
            main: content,
            lyrics_sidebar: None,
            lyrics_border: None,
        };
    }

    match position {
        SidebarPosition::Left => {
            let sidebar = Rect::new(content.x, content.y, sidebar_w, content.height);
            let border = Rect::new(
                content.x + sidebar_w,
                content.y,
                LYRICS_SIDEBAR_BORDER_WIDTH,
                content.height,
            );
            let main = Rect::new(
                content.x + total_sidebar,
                content.y,
                content.width - total_sidebar,
                content.height,
            );
            ContentLayout {
                main,
                lyrics_sidebar: Some(sidebar),
                lyrics_border: Some(border),
            }
        }
        // Left/Right are the only positions.
        SidebarPosition::Right => {
            let main_width = content.width - total_sidebar;
            let main = Rect::new(content.x, content.y, main_width, content.height);
            let border = Rect::new(
                content.x + main_width,
                content.y,
                LYRICS_SIDEBAR_BORDER_WIDTH,
                content.height,
            );
            let sidebar = Rect::new(
                content.x + main_width + LYRICS_SIDEBAR_BORDER_WIDTH,
                content.y,
                sidebar_w,
                content.height,
            );
            ContentLayout {
                main,
                lyrics_sidebar: Some(sidebar),
                lyrics_border: Some(border),
            }
        }
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

pub const OVERLAY_WIDTH_FRACTION: f32 = blackbird_client_shared::OVERLAY_WIDTH_FRACTION;
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

/// Returns the pixel dimensions of a single terminal cell, derived from the
/// terminal's reported window size. Returns `None` when the terminal does not
/// report pixel dimensions.
pub(crate) fn cell_pixel_size() -> Option<(u16, u16)> {
    let ws = crossterm::terminal::window_size().ok()?;
    if ws.width == 0 || ws.height == 0 || ws.columns == 0 || ws.rows == 0 {
        return None;
    }
    let cell_width = ws.width / ws.columns;
    let cell_height = ws.height / ws.rows;
    if cell_width == 0 || cell_height == 0 {
        return None;
    }
    Some((cell_width, cell_height))
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

// ── Large album art (BelowAlbum mode) ────────────────────────────────────────

/// Number of terminal rows for the large art displayed beside tracks in
/// `BelowAlbum` mode. Each terminal row encodes 2 pixel rows via half-blocks.
pub const LARGE_ART_TERM_ROWS: usize = 8;

/// Number of display columns for the large art grid, corrected for
/// non-square terminal character cells (same approach as [`art_cols`]).
pub(crate) fn large_art_cols() -> u16 {
    let pixel_rows = (LARGE_ART_TERM_ROWS * 2) as u16;
    (pixel_rows as f64 / half_block_correction())
        .round()
        .clamp(pixel_rows as f64, (pixel_rows * 2) as f64) as u16
}

/// Left margin before the large art grid in `BelowAlbum` mode.
pub const LARGE_ART_LEFT_MARGIN: usize = 1;

/// Gap between the large art and the track text.
pub const LARGE_ART_RIGHT_MARGIN: usize = 1;

// ── Art column geometry ─────────────────────────────────────────────────────

/// Horizontal and vertical geometry of an art column: a left margin, the art
/// cells themselves, a right margin, and the art height in terminal rows.
///
/// Construct one per draw and use it for every cell the art touches — the
/// blank reservation spans inside list rows, the `Rect` an image widget is
/// placed over, and mouse hit-testing — so those call sites can never
/// disagree about where the art is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtColumn {
    /// Columns of padding before the art.
    pub left_margin: u16,
    /// Columns the art itself occupies.
    pub cols: u16,
    /// Columns of padding between the art and the following text.
    pub right_margin: u16,
    /// Terminal rows the art occupies.
    pub rows: u16,
}

impl ArtColumn {
    /// The thumbnail art column, used by the now-playing bar and
    /// `LeftOfAlbum` group headers.
    pub fn thumbnail() -> Self {
        Self {
            left_margin: ART_LEFT_MARGIN,
            cols: art_cols(),
            right_margin: 1,
            rows: ART_TERM_ROWS,
        }
    }

    /// The large art column beside tracks in `BelowAlbum` mode.
    pub fn large() -> Self {
        Self {
            left_margin: LARGE_ART_LEFT_MARGIN as u16,
            cols: large_art_cols(),
            right_margin: LARGE_ART_RIGHT_MARGIN as u16,
            rows: LARGE_ART_TERM_ROWS as u16,
        }
    }

    /// The total width of the column, margins included.
    pub fn total_width(&self) -> u16 {
        self.left_margin + self.cols + self.right_margin
    }

    /// The art dimensions in character cells, for sizing image protocols.
    pub fn size(&self) -> Size {
        Size::new(self.cols, self.rows)
    }

    /// The `Rect` the art cells occupy when the art's top row is at `y`
    /// inside `area`, clipped to `area`.
    pub fn rect(&self, area: Rect, y: u16) -> Rect {
        Rect::new(area.x + self.left_margin, y, self.cols, self.rows).intersection(area)
    }
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

// Drag inertia scrolling parameters.
pub const INERTIA_FRICTION: f64 = 0.973;
pub const INERTIA_STOP_THRESHOLD: f64 = 0.1;
/// Multiplier applied to the drag velocity when seeding inertia on release.
pub const INERTIA_INITIAL_BOOST: f64 = 0.7;
/// Exponential smoothing factor for drag velocity (0 = no smoothing, 1 = ignore new samples).
pub const DRAG_VELOCITY_SMOOTHING: f64 = 0.3;
pub use blackbird_client_shared::{SEEK_STEP_SECS, VOLUME_STEP};

// ── Log view ────────────────────────────────────────────────────────────────

pub const LOG_TARGET_WIDTH: usize = 24;
pub const LOG_TARGET_SUFFIX_LEN: usize = 21;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_right_carves_sidebar_from_right() {
        let content = Rect::new(0, 0, 80, 24);
        let layout = split_content_with_sidebar(content, SidebarPosition::Right, 30);
        let sidebar = layout.lyrics_sidebar.expect("sidebar should exist");
        let border = layout.lyrics_border.expect("border should exist");
        // Sidebar is on the right: total = sidebar(30) + border(1) = 31.
        // main_width = 80 - 31 = 49. Border at x=49. Sidebar at x=50.
        assert_eq!(sidebar.x, 50);
        assert_eq!(sidebar.width, 30);
        assert_eq!(sidebar.height, 24);
        // Border is 1 column to the left of the sidebar.
        assert_eq!(border.x, 49);
        assert_eq!(border.width, 1);
        // Main content is narrower.
        assert_eq!(layout.main.x, 0);
        assert_eq!(layout.main.width, 49);
        assert!(layout.main.width < content.width);
    }

    #[test]
    fn split_left_carves_sidebar_from_left() {
        let content = Rect::new(0, 0, 80, 24);
        let layout = split_content_with_sidebar(content, SidebarPosition::Left, 20);
        let sidebar = layout.lyrics_sidebar.expect("sidebar should exist");
        let border = layout.lyrics_border.expect("border should exist");
        // Sidebar is on the left: x = 0, width = 20.
        assert_eq!(sidebar.x, 0);
        assert_eq!(sidebar.width, 20);
        // Border is 1 column to the right of the sidebar.
        assert_eq!(border.x, 20);
        assert_eq!(border.width, 1);
        // Main content starts after sidebar + border.
        assert_eq!(layout.main.x, 21);
        assert_eq!(layout.main.width, 59);
    }

    #[test]
    fn split_clamps_sidebar_width() {
        let content = Rect::new(0, 0, 80, 24);
        // Request way more than half — should clamp to content.width / 2 = 40.
        let layout = split_content_with_sidebar(content, SidebarPosition::Right, 100);
        let sidebar = layout.lyrics_sidebar.expect("sidebar should exist");
        assert_eq!(sidebar.width, 40);
        // Request below minimum — should clamp to 10.
        let layout = split_content_with_sidebar(content, SidebarPosition::Right, 3);
        let sidebar = layout.lyrics_sidebar.expect("sidebar should exist");
        assert_eq!(sidebar.width, LYRICS_SIDEBAR_MIN_WIDTH);
    }

    #[test]
    fn split_skips_sidebar_when_too_narrow() {
        let content = Rect::new(0, 0, 10, 24);
        let layout = split_content_with_sidebar(content, SidebarPosition::Right, 30);
        // Not enough room for sidebar + border + at least 1 column of main.
        assert!(layout.lyrics_sidebar.is_none());
        assert!(layout.lyrics_border.is_none());
        assert_eq!(layout.main, content);
    }
}
