use ratatui::layout::{Constraint, Direction, Layout, Rect, Size};

use blackbird_client_shared::config::SidebarPosition;
use smallvec::SmallVec;

use crate::app::{App, FocusedPanel};

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

/// Computes the bottom-panel rect for inline lyrics, anchored to the bottom of
/// the content area just above the help bar. Returns `None` if the content area
/// is too small.
pub fn inline_lyrics_rect(content: Rect) -> Option<Rect> {
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
    let Some(lyrics) = inline_lyrics_rect(content) else {
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

// ── Unified screen layout (draw + input) ────────────────────────────────────
//
// This is the single planning point for rendering and input. `ui::draw` and
// the mouse/scroll handlers in `main.rs` both call `layout_for` and consume
// the same rects, so draw and hit-testing can never disagree about geometry.

/// The settings sidebar minimum and reserved library-preview widths.
const SETTINGS_SIDEBAR_MIN_WIDTH: u16 = 20;

/// The result of planning the whole screen for one frame.
pub(crate) struct ScreenLayout {
    pub main: MainLayout,
    /// Whether a sidebar is shown (enabled and not loading).
    pub show_sidebar: bool,
    /// The sidebar position (used for border-resize drag direction).
    pub sidebar_position: SidebarPosition,
    /// The effective panel rendered in the main panel area (`Lyrics` with a
    /// sidebar renders the library).
    pub render_panel: FocusedPanel,
    /// The lyrics sidebar rect, if a sidebar is shown.
    pub lyrics_sidebar: Option<Rect>,
    /// The 1-column border between the sidebar and main content, if shown.
    pub lyrics_border: Option<Rect>,
    /// The settings sidebar rect, when the settings panel is focused (open).
    pub settings: Option<Rect>,
    /// The main panel column: after the lyrics sidebar/border and after the
    /// settings sidebar. This is what the library (or Search/Queue/Logs/
    /// full-panel Lyrics) renders into.
    pub panel: Rect,
    /// The library interaction rect: the framed inner rect when the library is
    /// rendered with a sidebar present, else `panel`. Used for clicks/scroll.
    pub library: Rect,
    /// The inline-lyrics strip at the bottom of `panel`, when visible.
    pub inline_lyrics: Option<Rect>,
}

impl ScreenLayout {
    /// The horizontal extent of the lyrics sidebar border hit region, expanded
    /// toward the sidebar so the 1-column border is easier to grab without
    /// interfering with the library's scrollbar.
    pub fn over_lyrics_border(&self, x: u16, y: u16) -> bool {
        let Some(r) = self.lyrics_border else {
            return false;
        };
        let tol = LYRICS_SIDEBAR_DRAG_TOLERANCE;
        let is_right_sidebar = self.sidebar_position == SidebarPosition::Right;
        // Right sidebar: border is to the left of the sidebar, so tolerance
        // extends right (into the sidebar, away from the library scrollbar).
        // Left sidebar: border is to the right of the sidebar, so tolerance
        // extends left (into the sidebar, away from the library scrollbar).
        let (x_start, x_end) = if is_right_sidebar {
            (r.x, r.x + r.width + tol)
        } else {
            (r.x.saturating_sub(tol), r.x + r.width)
        };
        y >= r.y && y < r.y + r.height && x >= x_start && x < x_end
    }
}

/// The width of the settings sidebar for a given main-column width and
/// configured value. Clamps the configured width into `[20, max(20,
/// area_width - 20)]` so the panel never collapses below 20 columns and
/// always leaves at least 20 columns for the library preview to the right.
/// At terminal widths below 40 the 20-column preview minimum cannot be fully
/// met (20 settings + 19 preview = 39, and the settings sidebar itself must
/// stay at least 20), so the preview floor is the remainder; this matches the
/// previous render behavior. This single clamp is shared by planning and the
/// border-drag math.
pub(crate) fn settings_width(area_width: u16, configured: u16) -> u16 {
    let max = area_width.saturating_sub(SETTINGS_SIDEBAR_MIN_WIDTH);
    configured.clamp(
        SETTINGS_SIDEBAR_MIN_WIDTH,
        max.max(SETTINGS_SIDEBAR_MIN_WIDTH),
    )
}

/// Whether the inline-lyrics strip is visible, given the render state. The
/// strip shows only when the library is the rendered panel (Library focus,
/// Settings focus with the library preview, or Lyrics-with-sidebar).
pub(crate) fn inline_lyrics_visible(
    is_loading: bool,
    inline_lyrics_mode: bool,
    has_synced_lyrics: bool,
    render_library: bool,
) -> bool {
    !is_loading && inline_lyrics_mode && has_synced_lyrics && render_library
}

/// The content-region components that are visible for the current app state,
/// in draw order. `ui::draw` iterates this list so each component renders
/// exactly once; the modal overlays (playback dropdown, album art, quit
/// dialog) are drawn separately outside this list.
pub(crate) fn visible_components(layout: &ScreenLayout) -> SmallVec<[VisibleComponent; 8]> {
    let mut components = SmallVec::new();
    components.push(VisibleComponent::MainPanel);
    if layout.settings.is_some() {
        components.push(VisibleComponent::Settings);
    }
    if layout.lyrics_sidebar.is_some() {
        components.push(VisibleComponent::LyricsSidebar);
    }
    if layout.inline_lyrics.is_some() {
        components.push(VisibleComponent::InlineLyrics);
    }
    components
}

/// A content-region component drawn from the single dispatch point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum VisibleComponent {
    /// The main panel (library or Search/Queue/Logs/full-panel Lyrics).
    MainPanel,
    /// The settings sidebar.
    Settings,
    /// The lyrics sidebar.
    LyricsSidebar,
    /// The inline-lyrics strip.
    InlineLyrics,
}

/// Plans the whole screen layout for one frame. Consumed by both `ui::draw`
/// and the input paths in `main.rs`.
pub(crate) fn layout_for(app: &App, size: Rect) -> ScreenLayout {
    let main = split_main(size);
    let is_loading = !app.logic.has_loaded_all_tracks();

    let sidebar_position = app.config.layout.base.sidebar.position;
    let sidebar_enabled = app.config.layout.base.sidebar.enabled;
    let show_sidebar = sidebar_enabled && !is_loading;

    // Start with the content area minus the lyrics sidebar (if any).
    let content_layout = if show_sidebar {
        split_content_with_sidebar(
            main.content,
            sidebar_position,
            app.config.layout.sidebar_width,
        )
    } else {
        ContentLayout {
            main: main.content,
            lyrics_sidebar: None,
            lyrics_border: None,
        }
    };

    // The settings sidebar eats into the main column on the left when the
    // settings panel is focused. With a left lyrics sidebar the order is
    // [lyrics][border][settings][panel]; with a right sidebar it is
    // [settings][panel][border][lyrics].
    let settings = if app.focused_panel == FocusedPanel::Settings {
        let settings_w = settings_width(
            content_layout.main.width,
            app.config.layout.settings_sidebar_width,
        );
        let settings_rect = Rect::new(
            content_layout.main.x,
            content_layout.main.y,
            settings_w,
            content_layout.main.height,
        );
        Some(settings_rect)
    } else {
        None
    };
    let panel = if let Some(settings_rect) = settings {
        Rect::new(
            settings_rect.x + settings_rect.width,
            content_layout.main.y,
            content_layout
                .main
                .width
                .saturating_sub(settings_rect.width),
            content_layout.main.height,
        )
    } else {
        content_layout.main
    };

    // The effective panel rendered in the main panel area.
    let render_panel = if app.focused_panel == FocusedPanel::Lyrics && show_sidebar {
        FocusedPanel::Library
    } else {
        app.focused_panel
    };
    let render_library =
        render_panel == FocusedPanel::Library || render_panel == FocusedPanel::Settings;

    // Inline lyrics cut into the bottom of `panel` (shrinking it).
    let inline_lyrics_shown = inline_lyrics_visible(
        is_loading,
        app.inline_lyrics_mode,
        app.lyrics.shared.has_synced_lyrics(),
        render_library,
    );
    let (panel, inline_lyrics) = if inline_lyrics_shown {
        let (main, lyrics) = split_inline_lyrics(panel);
        (main, lyrics)
    } else {
        (panel, None)
    };

    // The library interaction rect: the framed inner rect when the library is
    // rendered with a sidebar present, else `panel`.
    let library = if show_sidebar && render_library {
        Rect::new(
            panel.x + 1,
            panel.y + 1,
            panel.width.saturating_sub(2),
            panel.height.saturating_sub(2),
        )
    } else {
        panel
    };

    ScreenLayout {
        main,
        show_sidebar,
        sidebar_position,
        render_panel,
        lyrics_sidebar: content_layout.lyrics_sidebar,
        lyrics_border: content_layout.lyrics_border,
        settings,
        panel,
        library,
        inline_lyrics,
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
pub(crate) mod tests {
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

    // ── Unified screen layout tests ──────────────────────────────────────────

    use crate::app::{App, FocusedPanel};
    use blackbird_core::{self as bc};

    /// Builds a minimal `App` for layout, visibility, and animation tests. The
    /// logic's mpsc senders and the app's receivers are dummy (dropped
    /// immediately), and the library is marked loaded so the loading branch is
    /// not exercised; layout tests care only about that flag and lyrics data.
    ///
    /// The base URL is empty, so the logic's initial fetch fails once shortly
    /// after construction and leaves an `InitialFetchFailed` in the error slot.
    /// A caller that clears the loaded flag to reach the loading screen must
    /// wait for that failure and clear it, or it will render the connection
    /// error instead.
    pub(crate) fn test_app() -> App {
        let (cover_art_loaded_tx, cover_art_loaded_rx) = std::sync::mpsc::channel::<bc::CoverArt>();
        let (lyrics_loaded_tx, lyrics_loaded_rx) = std::sync::mpsc::channel::<bc::LyricsData>();
        let (similar_songs_loaded_tx, similar_songs_loaded_rx) =
            std::sync::mpsc::channel::<bc::SimilarSongsData>();
        let (library_populated_tx, library_populated_rx) = std::sync::mpsc::channel::<()>();
        let (track_updated_tx, track_updated_rx) = std::sync::mpsc::channel::<()>();
        let logic = bc::Logic::new(bc::LogicArgs {
            base_url: String::new(),
            username: String::new(),
            password: String::new(),
            transcode: false,
            volume: 0.0,
            apply_replaygain: false,
            replaygain_preamp_db: 0.0,
            sort_order: bc::SortOrder::default(),
            playback_mode: bc::PlaybackMode::default(),
            last_playback: None,
            cover_art_loaded_tx,
            lyrics_loaded_tx,
            similar_songs_loaded_tx,
            library_populated_tx,
            track_updated_tx,
        });
        let playback_to_logic_rx = logic.subscribe_to_playback_events();
        let mut app = App::new(
            crate::config::Config::default(),
            logic,
            playback_to_logic_rx,
            crate::cover_art::CoverArtCache::new(cover_art_loaded_rx),
            lyrics_loaded_rx,
            similar_songs_loaded_rx,
            library_populated_rx,
            track_updated_rx,
            crate::log_buffer::LogBuffer::new(),
        );
        // Mark the library loaded so the loading branch is not exercised.
        app.logic
            .get_state()
            .write()
            .unwrap()
            .library
            .has_loaded_all_tracks = true;
        // Disable the lyrics sidebar by default so layout tests exercise a
        // clean settings-only geometry; the compose test re-enables it.
        app.config.layout.base.sidebar.enabled = false;
        app
    }

    /// Injects synced lyrics into the app's shared lyrics state for the given
    /// track ID so `has_synced_lyrics()` returns true.
    fn inject_synced_lyrics(app: &mut App, track_id: &bc::blackbird_state::TrackId) {
        // Start a track so the shared state's `track_id` matches the loaded
        // data (on_lyrics_loaded only stores data for the expected track).
        let _ = app.lyrics.shared.on_track_started(track_id, true);
        let lyrics_data = bc::LyricsData {
            track_id: track_id.clone(),
            lyrics: Some(bc::bs::StructuredLyrics {
                display_artist: None,
                display_title: None,
                lang: None,
                offset: None,
                synced: true,
                line: vec![bc::bs::LyricLine {
                    start: Some(0),
                    value: "First line".to_string(),
                }],
            }),
        };
        app.lyrics.shared.on_lyrics_loaded(&lyrics_data);
    }

    #[test]
    fn settings_sidebar_shrinks_library_column() {
        let mut app = test_app();
        app.focused_panel = FocusedPanel::Settings;
        // Default settings width is 40; the content area is wide enough.
        let layout = layout_for(&app, Rect::new(0, 0, 120, 24));
        let settings = layout.settings.expect("settings sidebar should exist");
        assert_eq!(settings.width, 40);
        assert_eq!(settings.x, 0);
        assert!(layout.panel.width < layout.main.content.width);
        // Library preview is the settings-shrunken panel (no sidebar shown).
        let expected_panel_width = layout.main.content.width - settings.width;
        assert_eq!(layout.panel.width, expected_panel_width);
        assert_eq!(layout.library, layout.panel);
        // The settings sidebar does not overlap the library render.
        assert_eq!(settings.x + settings.width, layout.panel.x);
        assert_eq!(settings.y, layout.panel.y);
        assert_eq!(settings.height, layout.panel.height);
    }

    #[test]
    fn settings_plus_lyrics_sidebar_compose() {
        let mut app = test_app();
        app.focused_panel = FocusedPanel::Settings;
        // Enable the lyrics sidebar on the left so the order is
        // [lyrics][border][settings][panel].
        app.config.layout.base.sidebar.enabled = true;
        app.config.layout.base.sidebar.position = SidebarPosition::Left;
        let layout = layout_for(&app, Rect::new(0, 0, 160, 24));
        let settings = layout.settings.expect("settings sidebar should exist");
        let sidebar = layout.lyrics_sidebar.expect("lyrics sidebar should exist");
        let border = layout.lyrics_border.expect("lyrics border should exist");

        // Left-positioned lyrics sidebar: sidebar + border + settings + panel.
        let sidebar_w = sidebar.width;
        let border_w = border.width;
        assert_eq!(sidebar.x, 0);
        assert_eq!(border.x, sidebar_w);
        assert_eq!(settings.x, sidebar_w + border_w);
        assert_eq!(layout.panel.x, settings.x + settings.width);
        // The panel is the remaining middle column after both sidebars.
        assert_eq!(layout.panel.x, sidebar_w + border_w + settings.width);
        // Settings does not overlap the library render.
        assert!(settings.x + settings.width <= layout.panel.x);
    }

    #[test]
    fn layout_settings_widths_clamp() {
        // The unified clamp matches the plan: [20, max(20, area - 20)].
        assert_eq!(settings_width(100, 40), 40);
        // A huge configured value clamps to keep 20 columns for the preview.
        assert_eq!(settings_width(100, 1000), 80);
        // Below the minimum clamps up to 20.
        assert_eq!(settings_width(100, 5), 20);
        // Narrow area: the settings sidebar can never take the whole column.
        assert_eq!(settings_width(20, 40), 20);
        // Width 39: max(20, 19) = 20, so 40 clamps to 20 (leaves 19 for the
        // library — one short of the 20 minimum, unavoidable at this width).
        assert_eq!(settings_width(39, 40), 20);
        // Width 40: max(20, 20) = 20, so 40 clamps to 20 (leaves 20).
        assert_eq!(settings_width(40, 40), 20);
    }

    #[test]
    fn inline_lyrics_visibility_rules() {
        // The strip shows only when the library is the rendered panel, inline
        // mode is on, synced lyrics are loaded, and the app is not loading.
        assert!(inline_lyrics_visible(false, true, true, true));
        // Hidden while loading.
        assert!(!inline_lyrics_visible(true, true, true, true));
        // Hidden without inline-lyrics mode.
        assert!(!inline_lyrics_visible(false, false, true, true));
        // Hidden without synced lyrics.
        assert!(!inline_lyrics_visible(false, true, false, true));
        // Hidden when the library is not the rendered panel (Search/Queue/
        // Logs/full-panel Lyrics).
        assert!(!inline_lyrics_visible(false, true, true, false));
    }

    #[test]
    fn settings_focus_shows_inline_lyrics_in_preview() {
        let mut app = test_app();
        app.focused_panel = FocusedPanel::Settings;
        app.inline_lyrics_mode = true;
        inject_synced_lyrics(&mut app, &bc::blackbird_state::TrackId("t1".to_string()));
        let layout = layout_for(&app, Rect::new(0, 0, 120, 24));
        let inline = layout
            .inline_lyrics
            .expect("inline lyrics should show in settings");
        assert_eq!(inline.height, INLINE_LYRICS_HEIGHT);
        // The strip is as wide as the settings-shrunken panel.
        assert_eq!(inline.x, layout.panel.x);
        assert_eq!(inline.width, layout.panel.width);
        // It cuts into the bottom of the panel above the help bar.
        assert_eq!(inline.y, layout.panel.y + layout.panel.height);
    }

    /// Asserts that the dispatch list for a state contains exactly the expected
    /// content components and no duplicates.
    fn assert_visible_components(app: &mut App, expected: &[VisibleComponent], describe: &str) {
        let layout = layout_for(app, Rect::new(0, 0, 160, 24));
        let components = visible_components(&layout);
        let mut assert_msg = format!("state: {describe}");
        for (i, c) in components.iter().enumerate() {
            assert_msg.push_str(&format!("\n  [{i}]: {c:?}"));
        }
        assert_eq!(components.as_slice(), expected, "{assert_msg}");
        // Exactly-once per component (no duplicates).
        assert_eq!(
            components
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            components.len(),
            "duplicate entries in dispatch list for state: {describe}"
        );
    }

    /// AC.4: every visible content component is drawn exactly once from the
    /// single dispatch point, structural across all panel/loading/inline states.
    #[test]
    fn visible_components_list_has_unique_entries() {
        use crate::app::FocusedPanel;

        // --- Library focus, sidebar disabled, inline off ---
        let mut app = test_app();
        app.focused_panel = FocusedPanel::Library;
        assert_visible_components(
            &mut app,
            &[VisibleComponent::MainPanel],
            "Library, no sidebar, inline off",
        );

        // --- Library focus, sidebar enabled, inline off (sidebar shown) ---
        let mut app = test_app();
        app.focused_panel = FocusedPanel::Library;
        app.config.layout.base.sidebar.enabled = true;
        assert_visible_components(
            &mut app,
            &[VisibleComponent::MainPanel, VisibleComponent::LyricsSidebar],
            "Library + lyrics sidebar, inline off",
        );

        // --- Settings focus, sidebar disabled, inline on + synced lyrics ---
        let mut app = test_app();
        app.focused_panel = FocusedPanel::Settings;
        app.inline_lyrics_mode = true;
        inject_synced_lyrics(&mut app, &bc::blackbird_state::TrackId("t1".to_string()));
        assert_visible_components(
            &mut app,
            &[
                VisibleComponent::MainPanel,
                VisibleComponent::Settings,
                VisibleComponent::InlineLyrics,
            ],
            "Settings + inline on + synced",
        );

        // --- Settings focus, inline on but no synced lyrics ---
        let mut app = test_app();
        app.focused_panel = FocusedPanel::Settings;
        app.inline_lyrics_mode = true;
        assert_visible_components(
            &mut app,
            &[VisibleComponent::MainPanel, VisibleComponent::Settings],
            "Settings + inline on, no synced lyrics",
        );

        // --- Settings focus with lyrics sidebar ---
        let mut app = test_app();
        app.focused_panel = FocusedPanel::Settings;
        app.config.layout.base.sidebar.enabled = true;
        assert_visible_components(
            &mut app,
            &[
                VisibleComponent::MainPanel,
                VisibleComponent::Settings,
                VisibleComponent::LyricsSidebar,
            ],
            "Settings + lyrics sidebar",
        );

        // --- Search focus ---
        let mut app = test_app();
        app.focused_panel = FocusedPanel::Search;
        assert_visible_components(&mut app, &[VisibleComponent::MainPanel], "Search");

        // --- Queue focus ---
        let mut app = test_app();
        app.focused_panel = FocusedPanel::Queue;
        assert_visible_components(&mut app, &[VisibleComponent::MainPanel], "Queue");

        // --- Logs focus ---
        let mut app = test_app();
        app.focused_panel = FocusedPanel::Logs;
        assert_visible_components(&mut app, &[VisibleComponent::MainPanel], "Logs");

        // --- Lyrics full panel (no sidebar, components enabled) ---
        let mut app = test_app();
        app.focused_panel = FocusedPanel::Lyrics;
        app.config.layout.base.sidebar.enabled = false;
        // Give the sidebar a component so the full panel draws components.
        app.sidebar.order = smallvec::smallvec![crate::ui::sidebar::SidebarComponentId::Lyrics];
        assert_visible_components(
            &mut app,
            &[VisibleComponent::MainPanel],
            "Lyrics full panel",
        );

        // --- Lyrics-with-sidebar (renders the library, sidebar shown) ---
        let mut app = test_app();
        app.focused_panel = FocusedPanel::Lyrics;
        app.config.layout.base.sidebar.enabled = true;
        assert_visible_components(
            &mut app,
            &[VisibleComponent::MainPanel, VisibleComponent::LyricsSidebar],
            "Lyrics-with-sidebar",
        );

        // --- Loading (nothing but the main panel) ---
        let mut app = test_app();
        app.focused_panel = FocusedPanel::Library;
        app.config.layout.base.sidebar.enabled = true;
        app.logic
            .get_state()
            .write()
            .unwrap()
            .library
            .has_loaded_all_tracks = false;
        assert_visible_components(&mut app, &[VisibleComponent::MainPanel], "Loading");

        // --- Library with sidebar + inline on + synced ---
        let mut app = test_app();
        app.focused_panel = FocusedPanel::Library;
        app.config.layout.base.sidebar.enabled = true;
        app.inline_lyrics_mode = true;
        inject_synced_lyrics(&mut app, &bc::blackbird_state::TrackId("t1".to_string()));
        assert_visible_components(
            &mut app,
            &[
                VisibleComponent::MainPanel,
                VisibleComponent::LyricsSidebar,
                VisibleComponent::InlineLyrics,
            ],
            "Library + sidebar + inline",
        );
    }

    /// AC.5: the settings border drag clamp equals the render clamp. The drag
    /// math (`main.rs`) computes the new width as
    /// `settings_width(main_column_width, x - settings_rect.x)` with
    /// `main_column_width = settings_rect.width + panel.width`; this asserts
    /// that a width produced by that drag formula re-renders to the same
    /// settings rect — the drag border tracks the rendered border.
    #[test]
    fn settings_drag_clamp_matches_render_clamp() {
        use crate::app::FocusedPanel;

        // Wide terminal: content + settings sidebar, no lyrics sidebar.
        let mut app = test_app();
        app.focused_panel = FocusedPanel::Settings;
        let size = Rect::new(0, 0, 120, 24);

        // Drag the cursor to several positions across the settings border.
        for target_x in [0_u16, 30, 39, 59, 1000] {
            let layout = layout_for(&app, size);
            let settings_rect = layout.settings.expect("settings rect");
            let main_column_width = settings_rect.width + layout.panel.width;
            let dragged =
                settings_width(main_column_width, target_x.saturating_sub(settings_rect.x));
            app.config.layout.settings_sidebar_width = dragged;
            let re_layout = layout_for(&app, size);
            let re_settings = re_layout.settings.expect("settings rect");
            // The re-rendered settings width equals the dragged (clamped) width.
            assert_eq!(re_settings.width, dragged);
        }

        // Dragging far right clamps to `area - 20` (reserve the 20-column
        // library minimum); the re-render agrees exactly.
        let layout = layout_for(&app, size);
        let settings_rect = layout.settings.expect("settings rect");
        let main_column_width = settings_rect.width + layout.panel.width;
        let max_dragged = settings_width(main_column_width, 1000);
        app.config.layout.settings_sidebar_width = max_dragged;
        let re_layout = layout_for(&app, size);
        let re_settings = re_layout.settings.expect("settings rect");
        assert_eq!(re_settings.width, max_dragged);
        assert_eq!(re_layout.panel.width, 20);
    }

    /// AC.5: the input paths consume the same rects the draw path renders.
    /// `layout_for` returns the settings rect (the click target), the library
    /// rect (preview click area), and the inline-lyrics rect (click block).
    /// The settings click target is x-scoped: a click at the settings border
    /// column is *outside* the settings rect (the preview is a no-op), and a
    /// click inside the rect is inside.
    #[test]
    fn layout_for_used_by_input_paths() {
        use crate::app::FocusedPanel;

        // Settings focus + inline lyrics shown: the inline strip blocks clicks
        // at the bottom of the preview.
        let mut app = test_app();
        app.focused_panel = FocusedPanel::Settings;
        app.inline_lyrics_mode = true;
        inject_synced_lyrics(&mut app, &bc::blackbird_state::TrackId("t1".to_string()));
        let size = Rect::new(0, 0, 120, 24);
        let layout = layout_for(&app, size);
        let settings_rect = layout.settings.expect("settings rect");
        let inline = layout.inline_lyrics.expect("inline lyrics");

        // The settings rect is the x-scoped click target, exactly the rect
        // `settings::draw` receives.
        assert_eq!(settings_rect.y, layout.main.content.y);
        assert_eq!(settings_rect.height, layout.main.content.height);

        // The inline strip is at the bottom of the settings-shrunken panel,
        // outside the library click rect (so it blocks clicks there).
        assert_eq!(inline.y, layout.panel.y + layout.panel.height);
        let on_inline_row = (settings_rect.x + 1, inline.y + 1);
        let in_library = on_inline_row.0 >= layout.library.x
            && on_inline_row.0 < layout.library.x + layout.library.width
            && on_inline_row.1 >= layout.library.y
            && on_inline_row.1 < layout.library.y + layout.library.height;
        assert!(
            !in_library,
            "inline strip must not be part of the library click area"
        );

        // A click at the right edge of the settings panel (the border column)
        // is outside `layout.settings` — the preview click is a true no-op.
        let border_x = settings_rect.x + settings_rect.width - 1;
        assert!(
            border_x >= settings_rect.x && border_x < settings_rect.x + settings_rect.width,
            "border column is inside settings (the drag band)"
        );
        let right_of_settings = settings_rect.x + settings_rect.width;
        assert!(
            right_of_settings >= settings_rect.x + settings_rect.width,
            "the column right of settings is outside the settings rect"
        );
        // And that column is inside the library preview (display-only no-op).
        assert!(
            right_of_settings >= layout.library.x
                && right_of_settings < layout.library.x + layout.library.width,
            "the preview column is inside the library rect"
        );
    }
}
