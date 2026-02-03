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

pub const NOW_PLAYING_ART_WIDTH: u16 = 6;
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
            Constraint::Length(NOW_PLAYING_ART_WIDTH),
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

pub const ART_COLS: u16 = 4;
pub const ART_TERM_ROWS: u16 = 2;
pub const ART_LEFT_MARGIN: u16 = 1;
pub const ART_END_COL: u16 = 5; // = MARGIN + COLS

// ── Transport buttons ───────────────────────────────────────────────────────

pub const TRANSPORT_BUTTON_GROUP_WIDTH: u16 = 10;
pub const TRANSPORT_BTN_PREV: u16 = 0;
pub const TRANSPORT_BTN_PLAY: u16 = 3;
pub const TRANSPORT_BTN_STOP: u16 = 6;
pub const TRANSPORT_BTN_NEXT: u16 = 9;

// ── Album art overlay ───────────────────────────────────────────────────────

pub const OVERLAY_WIDTH_FRACTION: f32 = 0.9;
pub const OVERLAY_MIN_WIDTH: u16 = 10;
pub const OVERLAY_BORDER_OVERHEAD: u16 = 3;
pub const OVERLAY_X_BUTTON_OFFSET: u16 = 4;

pub fn overlay_rect(size: Rect) -> Rect {
    let overlay_width = ((size.width as f32) * OVERLAY_WIDTH_FRACTION) as u16;
    let overlay_width = overlay_width.max(OVERLAY_MIN_WIDTH).min(size.width);
    let art_cols = (overlay_width - 2) as usize;
    let art_pixel_rows = art_cols;
    let art_term_rows = art_pixel_rows.div_ceil(2);
    let overlay_height = (art_term_rows as u16 + OVERLAY_BORDER_OVERHEAD).min(size.height);
    let overlay_x = (size.width.saturating_sub(overlay_width)) / 2;
    let overlay_y = (size.height.saturating_sub(overlay_height)) / 2;
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

pub fn library_geometry(area: Rect, total_lines: usize) -> LibraryGeometry {
    let visible_height = area.height as usize;
    let has_scrollbar = total_lines > visible_height;
    let list_width = area.width as usize - 1 - if has_scrollbar { 1 } else { 0 };
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
pub const SEEK_STEP_SECS: i64 = 5;
pub const VOLUME_STEP: f32 = 0.05;

// ── Log view ────────────────────────────────────────────────────────────────

pub const LOG_TARGET_WIDTH: usize = 24;
pub const LOG_TARGET_SUFFIX_LEN: usize = 21;
