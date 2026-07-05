use std::collections::HashMap;

use blackbird_client_shared::{config::AlbumArtStyle, cover_art_cache::Resolution, library_scroll};
use blackbird_core::{
    self as bc, SortOrder,
    blackbird_state::{CoverArtId, TrackId},
    util::seconds_to_hms_string,
};
use blackbird_shared::config::ConfigFile as _;
use ratatui::{
    Frame,
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{List, ListItem, ListState, Paragraph},
};
use ratatui_image::{
    protocol::Protocol,
    sliced::{SignedPosition, SlicedImage, SlicedProtocol},
};
use std::sync::Arc;
use unicode_width::UnicodeWidthStr;

use crate::{
    app::App,
    cover_art::{ArtColorGrid, QuadrantColors},
    keys::Action,
    ui::album_art_overlay::AlbumArtOverlay,
};

use super::{StyleExt, string_to_color};

/// Context for rendering a single `LibraryEntry` into a `ListItem`.
///
/// Extracted so that both the main library view and the settings preview
/// can share the same rendering logic.
pub(crate) struct EntryRenderContext<'a> {
    pub album_art_style: AlbumArtStyle,
    pub list_width: usize,
    /// Geometry of the large BelowAlbum art column, shared by the blank
    /// reservation spans, the half-block rendering, and the image overlay.
    pub large_art: super::layout::ArtColumn,
    pub background_color: Color,
    pub album_color: Color,
    pub album_year_color: Color,
    pub album_length_color: Color,
    pub track_number_color: Color,
    pub track_name_color: Color,
    pub track_name_playing_color: Color,
    pub track_name_hovered_color: Color,
    pub track_length_color: Color,
    pub track_duration_color: Color,
    pub playing_track_id: Option<&'a TrackId>,
    pub selected_index: usize,
    pub underline_index: Option<usize>,
    pub hovered_heart_index: Option<usize>,
    pub hovered_entry_index: Option<usize>,
    pub art_colors: &'a HashMap<CoverArtId, QuadrantColors>,
    pub large_art_grids: &'a HashMap<CoverArtId, Arc<ArtColorGrid>>,
    /// When `true`, image protocols are available for visible groups —
    /// `render_library_entry` renders blank spaces in the art columns
    /// instead of half-block art, reserving space for `SlicedImage`
    /// widgets rendered on top afterward.
    pub has_image_protocol: bool,
}

/// Renders a single `LibraryEntry` at the given absolute index into a `ListItem`.
pub(crate) fn render_library_entry<'a>(
    entry: &'a LibraryEntry,
    i: usize,
    ctx: &EntryRenderContext<'a>,
) -> ListItem<'a> {
    let is_selected = i == ctx.selected_index;
    match entry {
        LibraryEntry::GroupHeader {
            artist,
            album,
            year,
            created,
            duration,
            starred,
            cover_art_id,
            ..
        } => {
            let is_heart_hovered =
                ctx.hovered_heart_index == Some(i) || ctx.hovered_entry_index == Some(i);
            let (heart, heart_style) = heart_to_tui(
                blackbird_client_shared::style::HeartState::from_interaction(
                    *starred,
                    is_heart_hovered,
                ),
                ctx.track_duration_color,
            );

            // Format year and added date.
            let year_str = year.map(|y| format!(" ({y})")).unwrap_or_default();
            let added_str = created
                .as_ref()
                .and_then(|c| c.get(..10))
                .map(|d| format!(" +{d}"))
                .unwrap_or_default();
            let dur_str = seconds_to_hms_string(*duration, false);

            match ctx.album_art_style {
                AlbumArtStyle::LeftOfAlbum => {
                    let colors = cover_art_id
                        .as_ref()
                        .and_then(|id| ctx.art_colors.get(id))
                        .copied()
                        .unwrap_or_default();

                    let thumbnail = super::layout::ArtColumn::thumbnail();
                    let mut line1_spans =
                        vec![Span::raw(" ".repeat(thumbnail.left_margin as usize))];
                    if ctx.has_image_protocol {
                        // Reserve space for the Image widget overlay.
                        line1_spans.push(Span::raw(" ".repeat(thumbnail.cols as usize)));
                    } else {
                        line1_spans.extend(super::art_row_spans(&colors, 0, 1));
                    }
                    line1_spans.push(Span::raw(" ".repeat(thumbnail.right_margin as usize)));
                    line1_spans.push(Span::styled(
                        artist,
                        Style::default().fg(string_to_color(artist)),
                    ));
                    let line1 = Line::from(line1_spans);

                    let left_content_width = thumbnail.total_width() as usize
                        + album.width()
                        + year_str.width()
                        + added_str.width();
                    let right_content = format!(" {dur_str} ");
                    let right_width = right_content.width() + 1;
                    let padding_needed = ctx
                        .list_width
                        .saturating_sub(left_content_width + right_width)
                        .saturating_sub(1);

                    let mut line2_spans =
                        vec![Span::raw(" ".repeat(thumbnail.left_margin as usize))];
                    if ctx.has_image_protocol {
                        line2_spans.push(Span::raw(" ".repeat(thumbnail.cols as usize)));
                    } else {
                        line2_spans.extend(super::art_row_spans(&colors, 2, 3));
                    }
                    line2_spans.push(Span::raw(" ".repeat(thumbnail.right_margin as usize)));
                    let content_start = line2_spans.len();
                    line2_spans.push(Span::styled(album, Style::default().fg(ctx.album_color)));
                    line2_spans.push(Span::styled(
                        year_str,
                        Style::default().fg(ctx.album_year_color),
                    ));
                    line2_spans.push(Span::styled(
                        added_str,
                        Style::default().fg(ctx.album_year_color),
                    ));
                    line2_spans.push(Span::raw(" ".repeat(padding_needed)));
                    line2_spans.push(Span::styled(
                        right_content,
                        Style::default().fg(ctx.album_length_color),
                    ));
                    line2_spans.push(Span::styled(heart, heart_style));

                    if ctx.underline_index == Some(i) {
                        for span in &mut line2_spans[content_start..] {
                            span.style = span.style.add_modifier(Modifier::UNDERLINED);
                        }
                    }

                    ListItem::new(vec![line1, Line::from(line2_spans)])
                }
                AlbumArtStyle::BelowAlbum => {
                    let line1 = Line::from(vec![
                        Span::raw(" "),
                        Span::styled(artist, Style::default().fg(string_to_color(artist))),
                    ]);

                    let left_content_width =
                        1 + album.width() + year_str.width() + added_str.width();
                    let right_content = format!(" {dur_str} ");
                    let right_width = right_content.width() + 1;
                    let padding_needed = ctx
                        .list_width
                        .saturating_sub(left_content_width + right_width)
                        .saturating_sub(1);

                    let mut line2_spans = vec![Span::raw(" ")];
                    let content_start = line2_spans.len();
                    line2_spans.push(Span::styled(album, Style::default().fg(ctx.album_color)));
                    line2_spans.push(Span::styled(
                        year_str,
                        Style::default().fg(ctx.album_year_color),
                    ));
                    line2_spans.push(Span::styled(
                        added_str,
                        Style::default().fg(ctx.album_year_color),
                    ));
                    line2_spans.push(Span::raw(" ".repeat(padding_needed)));
                    line2_spans.push(Span::styled(
                        right_content,
                        Style::default().fg(ctx.album_length_color),
                    ));
                    line2_spans.push(Span::styled(heart, heart_style));

                    if ctx.underline_index == Some(i) {
                        for span in &mut line2_spans[content_start..] {
                            span.style = span.style.add_modifier(Modifier::UNDERLINED);
                        }
                    }

                    ListItem::new(vec![line1, Line::from(line2_spans)])
                }
            }
        }
        LibraryEntry::Track {
            id,
            title,
            artist,
            album_artist,
            track_number,
            disc_number,
            duration,
            starred,
            play_count,
            cover_art_id,
            track_index_in_group,
        } => {
            let is_playing = ctx.playing_track_id == Some(id);
            let is_heart_hovered =
                ctx.hovered_heart_index == Some(i) || ctx.hovered_entry_index == Some(i);
            let (heart, heart_style) = heart_to_tui(
                blackbird_client_shared::style::HeartState::from_interaction(
                    *starred,
                    is_heart_hovered,
                ),
                ctx.track_duration_color,
            );

            let track_str = if let Some(disc) = disc_number {
                format!("{disc}.{}", track_number.unwrap_or(0))
            } else {
                format!("{}", track_number.unwrap_or(0))
            };

            let dur_str = duration
                .map(|d| seconds_to_hms_string(d, false))
                .unwrap_or_default();

            let title_style = if is_playing {
                Style::default()
                    .fg(ctx.track_name_playing_color)
                    .add_modifier(Modifier::BOLD)
            } else if is_selected {
                Style::default().fg(ctx.track_name_hovered_color)
            } else {
                Style::default().fg(ctx.track_name_color)
            };

            let mut left_spans: Vec<Span<'_>> = Vec::new();
            let mut left_width: usize;
            let underline_start: usize;

            match ctx.album_art_style {
                AlbumArtStyle::LeftOfAlbum => {
                    left_spans.push(Span::raw(" ".repeat(super::layout::TRACK_INDENT)));
                    left_width = super::layout::TRACK_INDENT;
                    underline_start = 1;
                }
                AlbumArtStyle::BelowAlbum => {
                    let indent_width = ctx.large_art.total_width() as usize;

                    if *track_index_in_group < ctx.large_art.rows as usize {
                        large_art_row_spans(
                            &mut left_spans,
                            ctx,
                            cover_art_id.as_ref(),
                            *track_index_in_group,
                        );
                    } else {
                        left_spans.push(Span::raw(" ".repeat(indent_width)));
                    }
                    left_width = indent_width;
                    underline_start = left_spans.len();
                }
            }

            let track_num_formatted = format!("{:>5} ", track_str);
            left_spans.push(Span::styled(
                track_num_formatted.clone(),
                Style::default().fg(ctx.track_number_color),
            ));
            left_width += track_num_formatted.width();

            if is_playing {
                left_spans.push(Span::styled(
                    "\u{25B6} ",
                    Style::default()
                        .fg(ctx.track_name_playing_color)
                        .add_modifier(Modifier::BOLD),
                ));
                left_width += 2;
            }

            left_spans.push(Span::styled(title, title_style));
            left_width += title.width();

            if let Some(pc) = play_count {
                let pc_str = format!(" {pc}");
                left_width += pc_str.width();
                left_spans.push(Span::styled(
                    pc_str,
                    Style::default().fg(ctx.track_number_color),
                ));
            }

            let mut right_spans = Vec::new();
            let mut right_width = 0;

            if let Some(track_artist) = artist
                && track_artist != album_artist
            {
                let artist_str = format!("{track_artist} ");
                right_width += artist_str.width();
                right_spans.push(Span::styled(
                    artist_str,
                    Style::default().fg(string_to_color(track_artist)),
                ));
            }

            right_width += dur_str.width() + 2;
            right_spans.push(Span::styled(
                dur_str,
                Style::default().fg(ctx.track_length_color),
            ));
            right_spans.push(Span::raw(" "));
            right_spans.push(Span::styled(heart, heart_style));

            let padding_needed = ctx
                .list_width
                .saturating_sub(left_width + right_width)
                .saturating_sub(1);

            let mut spans = left_spans;
            spans.push(Span::raw(" ".repeat(padding_needed)));
            spans.extend(right_spans);

            if ctx.underline_index == Some(i) {
                for span in &mut spans[underline_start..] {
                    span.style = span.style.add_modifier(Modifier::UNDERLINED);
                }
            }

            ListItem::new(Line::from(spans))
        }
        LibraryEntry::GroupSpacer {
            cover_art_id,
            art_row_index,
            ..
        } => {
            let mut spans: Vec<Span<'_>> = Vec::new();

            if *art_row_index < ctx.large_art.rows as usize {
                large_art_row_spans(&mut spans, ctx, cover_art_id.as_ref(), *art_row_index);
            } else {
                spans.push(Span::raw(" ".repeat(ctx.large_art.total_width() as usize)));
            }

            ListItem::new(Line::from(spans))
        }
        LibraryEntry::AlbumGap => ListItem::new(Line::from("")),
    }
}

/// The visible-entry window and target area shared by the image overlay
/// render passes drawn on top of the library `List`.
struct OverlayWindow<'a> {
    entries: &'a [LibraryEntry],
    /// Index of the first visible entry.
    item_offset: usize,
    /// One past the index of the last visible entry.
    item_end: usize,
    /// The list area the overlays are clipped to.
    inner: Rect,
}

impl OverlayWindow<'_> {
    /// The absolute line offset rendered at the top row of `inner`.
    ///
    /// This is the line offset of the entry at `item_offset` — not the
    /// requested scroll offset, which may point one line into a partially
    /// visible 2-line group header. `compute_item_offset` includes such a
    /// header in the visible range and the `List` renders it in full, so
    /// overlays positioned from the scroll offset would land one row above
    /// the text they must line up with.
    fn display_origin(&self) -> i32 {
        self.entries[..self.item_offset]
            .iter()
            .map(|entry| entry.height() as i32)
            .sum()
    }
}

/// Pushes one terminal row of the large BelowAlbum art column (margins
/// included) onto `spans`.
///
/// When an image protocol is available, the art cells are blank — reserving
/// the exact cells the `SlicedImage` widget is drawn over afterwards (both
/// derive their geometry from [`EntryRenderContext::large_art`]). Otherwise
/// the row is rendered as half-block characters from the color grid.
fn large_art_row_spans<'a>(
    spans: &mut Vec<Span<'a>>,
    ctx: &EntryRenderContext<'a>,
    cover_art_id: Option<&CoverArtId>,
    term_row: usize,
) {
    let art = &ctx.large_art;
    spans.push(Span::raw(" ".repeat(art.left_margin as usize)));

    if ctx.has_image_protocol {
        // Reserve blank cells for the SlicedImage overlay.
        spans.push(Span::raw(" ".repeat(art.cols as usize)));
    } else {
        let grid = cover_art_id.and_then(|id| ctx.large_art_grids.get(id));
        if let Some(grid) = grid {
            let color_row_top = term_row * 2;
            let color_row_bot = color_row_top + 1;
            for col in 0..art.cols as usize {
                let fg = if color_row_top < grid.rows {
                    grid.colors[color_row_top][col]
                } else {
                    ctx.background_color
                };
                let bg = if color_row_bot < grid.rows {
                    grid.colors[color_row_bot][col]
                } else {
                    ctx.background_color
                };
                spans.push(Span::styled("\u{2580}", Style::default().fg(fg).bg(bg)));
            }
        } else {
            spans.push(Span::raw(" ".repeat(art.cols as usize)));
        }
    }

    spans.push(Span::raw(" ".repeat(art.right_margin as usize)));
}

/// Converts a line offset to an item offset. Uses `>` so that an item whose
/// span *contains* the offset is included (important for 2-line group headers
/// whose first line may be above the viewport).
fn compute_item_offset(entries: &[LibraryEntry], line_offset: usize) -> usize {
    let mut item_offset = 0usize;
    let mut current_line = 0usize;
    for (i, entry) in entries.iter().enumerate() {
        let next_line = current_line + entry.height();
        if next_line > line_offset {
            item_offset = i;
            return item_offset.min(entries.len().saturating_sub(1));
        }
        current_line = next_line;
        item_offset = i + 1;
    }
    item_offset.min(entries.len().saturating_sub(1))
}

/// Returns the width of the scroll indicator based on sort order.
/// Alphabetical uses single letters (1 char), year-based modes use full years (4 chars).
/// Modes without labels still need 1 column for the scrollbar track.
fn scroll_indicator_width(sort_order: SortOrder) -> usize {
    match sort_order {
        SortOrder::Alphabetical | SortOrder::MostPlayed => 1,
        SortOrder::NewestFirst | SortOrder::RecentlyAdded => 4,
    }
}

/// A single entry in the flat library list.
#[derive(Debug, Clone)]
pub enum LibraryEntry {
    GroupHeader {
        artist: String,
        album: String,
        year: Option<i32>,
        /// The date the album was added to the library (ISO 8601 format).
        created: Option<String>,
        duration: u32,
        starred: bool,
        album_id: blackbird_core::blackbird_state::AlbumId,
        cover_art_id: Option<blackbird_core::blackbird_state::CoverArtId>,
    },
    Track {
        id: TrackId,
        title: String,
        artist: Option<String>,
        album_artist: String,
        track_number: Option<u32>,
        disc_number: Option<u32>,
        duration: Option<u32>,
        starred: bool,
        play_count: Option<u64>,
        /// The group's cover art ID (used in `BelowAlbum` mode).
        cover_art_id: Option<CoverArtId>,
        /// 0-based index of this track within its group (used in `BelowAlbum` mode).
        track_index_in_group: usize,
    },
    /// Padding entry added after the last track in a `BelowAlbum` group when
    /// the group has fewer tracks than the art height, so the art is fully visible.
    GroupSpacer {
        /// The group's cover art ID (used to render art continuation rows).
        cover_art_id: Option<CoverArtId>,
        /// The track index within the group this spacer row corresponds to
        /// (i.e., `track_count + spacer_index`), used for art row calculation.
        art_row_index: usize,
    },
    /// Blank row between albums for visual spacing.
    AlbumGap,
}

impl LibraryEntry {
    pub fn height(&self) -> usize {
        match self {
            LibraryEntry::GroupHeader { .. } => 2,
            LibraryEntry::Track { .. }
            | LibraryEntry::GroupSpacer { .. }
            | LibraryEntry::AlbumGap => 1,
        }
    }
}

/// Assembles a flat list of library entries from `(header, tracks)` group pairs,
/// inserting spacer and gap entries according to the layout configuration.
///
/// This is the single source of truth for the structural layout of the flat
/// library. Both the real library and the settings preview use this function.
pub(crate) fn assemble_flat_library(
    groups: impl IntoIterator<Item = (LibraryEntry, Vec<LibraryEntry>)>,
    album_art_style: AlbumArtStyle,
    album_spacing: usize,
) -> Vec<LibraryEntry> {
    let groups: Vec<_> = groups.into_iter().collect();
    let group_count = groups.len();
    let mut result = Vec::new();

    for (group_index, (header, tracks)) in groups.into_iter().enumerate() {
        let cover_art_id = match &header {
            LibraryEntry::GroupHeader { cover_art_id, .. } => cover_art_id.clone(),
            _ => None,
        };
        let track_count = tracks.len();

        result.push(header);
        result.extend(tracks);

        // In BelowAlbum mode, pad short groups so the art is fully visible.
        if album_art_style == AlbumArtStyle::BelowAlbum
            && track_count < super::layout::LARGE_ART_TERM_ROWS
        {
            for si in 0..(super::layout::LARGE_ART_TERM_ROWS - track_count) {
                result.push(LibraryEntry::GroupSpacer {
                    cover_art_id: cover_art_id.clone(),
                    art_row_index: track_count + si,
                });
            }
        }

        // Add blank gap rows between albums (not after the last group).
        if group_index + 1 < group_count {
            for _ in 0..album_spacing {
                result.push(LibraryEntry::AlbumGap);
            }
        }
    }

    result
}

pub fn total_entry_lines(entries: &[LibraryEntry]) -> usize {
    entries.iter().map(LibraryEntry::height).sum()
}

/// Returns the entry index whose line span contains `target_line`, if any.
fn entry_at_line(entries: &[LibraryEntry], target_line: usize) -> Option<usize> {
    let mut current_line = 0usize;
    for (i, entry) in entries.iter().enumerate() {
        let h = entry.height();
        if target_line < current_line + h {
            return Some(i);
        }
        current_line += h;
    }
    None
}

pub struct LibraryState {
    pub scroll_offset: usize,
    pub selected_index: usize,
    pub needs_scroll_to_playing: bool,
    pub scroll_to_track: Option<TrackId>,

    /// Shared scroll/drag/inertia mechanism.
    pub viewport: super::scroll::Scroller,

    // Mouse interaction
    pub click_pending: Option<(u16, u16, usize)>,
    pub drag_selected_index: Option<usize>,

    // Private cache
    cached_flat_library: Vec<LibraryEntry>,
    flat_library_dirty: bool,
    album_art_style: AlbumArtStyle,
    album_spacing: usize,
}

impl LibraryState {
    pub fn new() -> Self {
        Self {
            scroll_offset: 0,
            selected_index: 0,
            needs_scroll_to_playing: true,
            scroll_to_track: None,

            viewport: super::scroll::Scroller::new(),

            click_pending: None,
            drag_selected_index: None,

            cached_flat_library: Vec::new(),
            flat_library_dirty: true,
            album_art_style: AlbumArtStyle::default(),
            album_spacing: 1,
        }
    }

    /// Update the album art style used for spacer entry generation.
    pub fn set_album_art_style(&mut self, style: AlbumArtStyle) {
        if self.album_art_style != style {
            self.album_art_style = style;
            self.flat_library_dirty = true;
        }
    }

    /// Update the album spacing (blank rows between albums).
    pub fn set_album_spacing(&mut self, spacing: usize) {
        if self.album_spacing != spacing {
            self.album_spacing = spacing;
            self.flat_library_dirty = true;
        }
    }

    /// Marks the flat library cache as dirty, forcing a rebuild on next access.
    pub fn mark_dirty(&mut self) {
        self.flat_library_dirty = true;
    }

    /// Returns the track ID of the currently selected entry, if it is a track.
    pub fn selected_track_id(&self) -> Option<&TrackId> {
        match self.cached_flat_library.get(self.selected_index)? {
            LibraryEntry::Track { id, .. } => Some(id),
            LibraryEntry::GroupHeader { .. }
            | LibraryEntry::GroupSpacer { .. }
            | LibraryEntry::AlbumGap => None,
        }
    }

    /// Returns the cached flat library, rebuilding if needed.
    pub fn get_flat_library(&mut self, logic: &bc::Logic) -> &[LibraryEntry] {
        self.ensure_flat_library(logic);
        &self.cached_flat_library
    }

    /// Ensures the flat library cache is up to date, rebuilding if dirty.
    /// Call this before using [`flat_library`] to avoid stale data.
    pub fn ensure_flat_library(&mut self, logic: &bc::Logic) {
        if self.flat_library_dirty {
            self.rebuild_flat_library(logic);
            self.flat_library_dirty = false;
        }
    }

    /// Returns a shared reference to the cached flat library.
    /// The caller must ensure the cache is fresh by calling
    /// [`ensure_flat_library`] first.
    pub fn flat_library(&self) -> &[LibraryEntry] {
        &self.cached_flat_library
    }

    /// Returns the length of the flat library without requiring mutable access.
    pub fn flat_library_len(&self) -> usize {
        self.cached_flat_library.len()
    }

    /// Returns a clone of the entry at the given index, if it exists.
    pub fn get_library_entry(&mut self, logic: &bc::Logic, index: usize) -> Option<LibraryEntry> {
        self.ensure_flat_library(logic);
        self.cached_flat_library.get(index).cloned()
    }

    /// Rebuilds the cached flat library from the current state.
    fn rebuild_flat_library(&mut self, logic: &bc::Logic) {
        let state = logic.get_state();
        let state = state.read().unwrap();

        let groups = state.library.groups.iter().map(|group| {
            let created = state
                .library
                .albums
                .get(&group.album_id)
                .map(|a| a.created.to_string());

            let header = LibraryEntry::GroupHeader {
                artist: group.artist.to_string(),
                album: group.album.to_string(),
                year: group.year,
                created,
                duration: group.duration,
                starred: group.starred,
                album_id: group.album_id.clone(),
                cover_art_id: group.cover_art_id.clone(),
            };

            let tracks: Vec<_> = group
                .tracks
                .iter()
                .enumerate()
                .filter_map(|(track_index, track_id)| {
                    let track = state.library.track_map.get(track_id)?;
                    Some(LibraryEntry::Track {
                        id: track.id.clone(),
                        title: track.title.to_string(),
                        artist: track.artist.as_ref().map(|a| a.to_string()),
                        album_artist: group.artist.to_string(),
                        track_number: track.track,
                        disc_number: track.disc_number,
                        duration: track.duration,
                        starred: track.starred,
                        play_count: track.play_count,
                        cover_art_id: group.cover_art_id.clone(),
                        track_index_in_group: track_index,
                    })
                })
                .collect();

            (header, tracks)
        });

        self.cached_flat_library =
            assemble_flat_library(groups, self.album_art_style, self.album_spacing);
    }

    /// Finds the flat index for a given track in the library.
    pub fn find_flat_index_for_track(
        &self,
        _state: &bc::AppState,
        target_track_id: &TrackId,
    ) -> Option<usize> {
        self.cached_flat_library.iter().position(
            |entry| matches!(entry, LibraryEntry::Track { id, .. } if id == target_track_id),
        )
    }

    /// Sets `viewport.line` to center `selected_index` in the visible area.
    pub fn center_viewport_on_selection(&mut self) {
        let mut line_offset = 0usize;
        for entry in self.cached_flat_library.iter().take(self.selected_index) {
            line_offset += entry.height();
        }
        let half_height = (self.viewport.visible_height / 2).saturating_sub(1);
        let total_lines = total_entry_lines(&self.cached_flat_library);
        let max_viewport = self.viewport.max_line(total_lines);
        self.viewport.line = line_offset.saturating_sub(half_height).min(max_viewport);
    }

    /// Centers the viewport on `selected_index` only if it is outside the
    /// visible area. Leaves the viewport untouched when the cursor is already
    /// on-screen, avoiding jarring snaps during keyboard navigation.
    pub fn ensure_viewport_shows_selection(&mut self) {
        let mut line_offset = 0usize;
        for entry in self.cached_flat_library.iter().take(self.selected_index) {
            line_offset += entry.height();
        }
        let entry_height = self
            .cached_flat_library
            .get(self.selected_index)
            .map(LibraryEntry::height)
            .unwrap_or(1);

        let above = line_offset < self.viewport.line;
        let below = line_offset + entry_height > self.viewport.line + self.viewport.visible_height;

        // Only scroll if the selected entry is completely outside the viewport.
        if above || below {
            self.center_viewport_on_selection();
        }
    }

    /// Returns whether the entry at the given flat index is visible in the
    /// current viewport.
    pub fn is_index_visible(&self, index: usize) -> bool {
        let mut line_offset = 0usize;
        for entry in self.cached_flat_library.iter().take(index) {
            line_offset += entry.height();
        }
        let entry_height = self
            .cached_flat_library
            .get(index)
            .map(LibraryEntry::height)
            .unwrap_or(0);
        let viewport_end = self.viewport.line + self.viewport.visible_height;
        line_offset + entry_height > self.viewport.line && line_offset < viewport_end
    }

    /// Sets `selected_index` to the nearest track at the viewport center.
    pub fn snap_cursor_to_viewport_center(&mut self) {
        let total_lines = total_entry_lines(&self.cached_flat_library);
        let max_viewport = self.viewport.max_line(total_lines);
        let center_line = self.viewport.line.min(max_viewport) + self.viewport.visible_height / 2;
        if let Some(idx) = entry_at_line(&self.cached_flat_library, center_line) {
            let target = match self.cached_flat_library.get(idx) {
                Some(LibraryEntry::Track { .. }) => Some(idx),
                Some(LibraryEntry::GroupHeader { .. }) => {
                    match self.cached_flat_library.get(idx + 1) {
                        Some(LibraryEntry::Track { .. }) => Some(idx + 1),
                        _ => None,
                    }
                }
                _ => None,
            };
            if let Some(t) = target {
                self.selected_index = t;
            }
        }
    }

    /// Navigates to the first track in the given album.
    pub fn scroll_to_album(
        &mut self,
        logic: &bc::Logic,
        album_id: &blackbird_core::blackbird_state::AlbumId,
    ) {
        if self.flat_library_dirty {
            self.rebuild_flat_library(logic);
            self.flat_library_dirty = false;
        }
        let mut found_header = false;
        for (i, entry) in self.cached_flat_library.iter().enumerate() {
            match entry {
                LibraryEntry::GroupHeader { album_id: aid, .. } => {
                    found_header = aid == album_id;
                }
                LibraryEntry::Track { .. } if found_header => {
                    self.selected_index = i;
                    self.center_viewport_on_selection();
                    return;
                }
                _ => {}
            }
        }
    }

    /// Applies inertia-based drag scrolling. Returns `true` if the view moved.
    ///
    /// This continues the drag viewport animation after the user releases the
    /// mouse button, decelerating smoothly until the velocity drops below the
    /// stop threshold, at which point it snaps `selected_index` to the viewport
    /// center.
    pub fn tick_inertia(&mut self, logic: &bc::Logic) -> bool {
        if self.flat_library_dirty {
            self.rebuild_flat_library(logic);
            self.flat_library_dirty = false;
        }
        let total_lines = total_entry_lines(&self.cached_flat_library);
        match self.viewport.tick_inertia(total_lines) {
            super::scroll::InertiaTick::Moved => true,
            super::scroll::InertiaTick::Stopped => {
                self.snap_cursor_to_viewport_center();
                false
            }
            super::scroll::InertiaTick::Tickless | super::scroll::InertiaTick::Inactive => false,
        }
    }

    /// Cancels any active inertia scrolling, snapping the selection to the
    /// current viewport center.
    pub fn cancel_inertia(&mut self, logic: &bc::Logic) {
        if self.viewport.cancel_inertia() {
            if self.flat_library_dirty {
                self.rebuild_flat_library(logic);
                self.flat_library_dirty = false;
            }
            self.snap_cursor_to_viewport_center();
        }
    }

    /// Ensures the current selection is on a track, not a group header.
    /// If currently on a header, moves to the first track in the library.
    pub fn ensure_selection_on_track(&mut self, logic: &bc::Logic) {
        if self.flat_library_dirty {
            self.rebuild_flat_library(logic);
            self.flat_library_dirty = false;
        }

        // Check if current selection is already a track.
        if let Some(LibraryEntry::Track { .. }) = self.cached_flat_library.get(self.selected_index)
        {
            return;
        }

        // Find the first track in the library.
        for (i, entry) in self.cached_flat_library.iter().enumerate() {
            if let LibraryEntry::Track { .. } = entry {
                self.selected_index = i;
                self.center_viewport_on_selection();
                return;
            }
        }
    }
}

/// Draws a centered error message when the server connection fails,
/// directing the user to the settings panel or config file.
fn draw_connection_error(
    frame: &mut Frame,
    style: &blackbird_client_shared::style::Style,
    error: &str,
    area: Rect,
) {
    let accent = style.track_name_playing_color();
    let dim = style.track_duration_color();
    let text_color = style.text_color();

    let config_path = crate::config::Config::path();
    let config_path_str = config_path.display().to_string();

    let lines = vec![
        Line::from(Span::styled(
            "connection failed",
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            error.to_string(),
            Style::default().fg(text_color),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("Press ", Style::default().fg(dim)),
            Span::styled(
                "i",
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " to open settings and check your server configuration.",
                Style::default().fg(dim),
            ),
        ]),
        Line::from(Span::styled(
            format!("Config file: {config_path_str}"),
            Style::default().fg(dim),
        )),
    ];

    let text = Text::from(lines);
    let paragraph = Paragraph::new(text)
        .wrap(ratatui::widgets::Wrap { trim: false })
        .centered();
    frame.render_widget(paragraph, area);
}

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    // Extract style colors upfront to avoid borrow conflicts later.
    let background_color = super::effective_bg(&app.config);
    let text_color = app.config.style.text_color();
    let album_color = app.config.style.album_color();
    let album_year_color = app.config.style.album_year_color();
    let album_length_color = app.config.style.album_length_color();
    let track_number_color = app.config.style.track_number_color();
    let track_name_color = app.config.style.track_name_color();
    let track_name_playing_color = app.config.style.track_name_playing_color();
    let track_length_color = app.config.style.track_length_color();
    let track_duration_color = app.config.style.track_duration_color();
    let track_name_hovered_color = app.config.style.track_name_hovered_color();

    let has_loaded = app.logic.has_loaded_all_tracks();

    // Use the full area directly (no frame/border)
    let inner = area;

    if !has_loaded {
        // Check if the initial fetch failed (server unreachable, auth error, etc.).
        if let Some(bc::AppStateError::InitialFetchFailed { ref error }) = app.logic.get_error() {
            draw_connection_error(frame, &app.config.style, error, inner);
            return;
        }

        let track_count = app
            .logic
            .get_state()
            .read()
            .unwrap()
            .library
            .track_ids
            .len();
        super::loading::draw(frame, app.tick_count, &app.config.style, track_count, inner);
        return;
    }

    // Copy values we need before borrowing entries.
    let selected_index = app.library.selected_index;
    let playing_track_id = app.logic.get_playing_track_id();
    let album_art_style = app.config.layout.base.album_art_style;

    // Ensure the flat library cache is fresh and perform all library mutations
    // before taking an immutable borrow on entries.
    app.library.ensure_flat_library(&app.logic);
    app.library.set_album_art_style(album_art_style);
    app.library
        .set_album_spacing(app.config.layout.base.album_spacing);

    if app.library.flat_library().is_empty() {
        let empty =
            Paragraph::new("No tracks found").style(Style::default().fg(track_duration_color));
        frame.render_widget(empty, inner);
        return;
    }

    // Calculate total content height in lines to determine if scrollbar is needed.
    let total_lines = total_entry_lines(app.library.flat_library());
    let sort_order = app.logic.get_sort_order();
    let indicator_width = scroll_indicator_width(sort_order);
    let visible_height = inner.height as usize;
    app.library.viewport.visible_height = visible_height;

    let geo = super::layout::library_geometry(inner, total_lines, indicator_width);
    let has_scrollbar = geo.has_scrollbar;
    let list_width = geo.list_width;

    // Always use the decoupled viewport position, clamped to valid range.
    app.library.viewport.clamp(total_lines);
    let centered_offset = app.library.viewport.line;

    // Convert line offset back to item offset for ListState.
    let item_offset = compute_item_offset(app.library.flat_library(), centered_offset);

    // Update scroll_offset now so that hover hit-testing (below) uses the
    // current viewport position rather than the stale value from last frame.
    app.library.scroll_offset = item_offset;

    // Determine which entry's heart is being hovered (for hover color effect).
    let hovered_heart_index = compute_hovered_heart_index(app, area);
    // Determine which track entry is being hovered anywhere on the row.
    let hovered_entry_index = compute_hovered_entry_index(app, area);
    // During a content drag, keep the underline on the drag-selected row;
    // otherwise fall back to the normal hover detection.
    let underline_index = app.library.drag_selected_index.or(hovered_entry_index);

    // Now take the immutable borrow on entries for the rest of rendering.
    let entries = app.library.flat_library();

    // Determine the visible item range: walk forward from item_offset until we
    // exceed the visible height (plus a small buffer for partially visible items).
    let buffer_items = 5;
    let mut visible_item_end = item_offset;
    let mut accumulated_height = 0usize;
    let height_limit = visible_height + buffer_items;
    for entry in entries.iter().skip(item_offset) {
        if accumulated_height >= height_limit {
            break;
        }
        accumulated_height += entry.height();
        visible_item_end += 1;
    }
    visible_item_end = visible_item_end.min(entries.len());

    // Pre-compute quadrant colors only for visible group headers (used in LeftOfAlbum mode).
    let mut art_colors: HashMap<CoverArtId, QuadrantColors> = HashMap::new();
    if album_art_style == AlbumArtStyle::LeftOfAlbum {
        for entry in &entries[item_offset..visible_item_end] {
            if let LibraryEntry::GroupHeader {
                cover_art_id: Some(id),
                ..
            } = entry
                && !art_colors.contains_key(id)
            {
                let colors = app.cover_art_cache.get(&app.logic, Some(id));
                art_colors.insert(id.clone(), colors);
            }
        }
    }

    // Pre-compute large art grids for visible tracks and spacers (used in BelowAlbum mode).
    let large_art = super::layout::ArtColumn::large();
    let large_art_pixel_rows = large_art.rows as usize * 2;
    let mut large_art_grids: HashMap<CoverArtId, Arc<ArtColorGrid>> = HashMap::new();
    // Pre-fetch sliced protocols for visible groups (used in BelowAlbum mode).
    let mut sliced_protocols: HashMap<CoverArtId, Option<Arc<SlicedProtocol>>> = HashMap::new();
    // Pre-fetch fixed-size protocols for visible group headers (used in LeftOfAlbum mode).
    let mut thumbnail_protocols: HashMap<CoverArtId, Option<Arc<Protocol>>> = HashMap::new();
    let has_picker = app.cover_art_cache.has_picker();
    if album_art_style == AlbumArtStyle::BelowAlbum {
        for entry in &entries[item_offset..visible_item_end] {
            let id = match entry {
                LibraryEntry::Track {
                    cover_art_id: Some(id),
                    track_index_in_group,
                    ..
                } if *track_index_in_group < large_art.rows as usize => Some(id),
                LibraryEntry::GroupSpacer {
                    cover_art_id: Some(id),
                    ..
                } => Some(id),
                _ => None,
            };
            if let Some(id) = id {
                if !large_art_grids.contains_key(id) {
                    let (grid, _loading) = app.cover_art_cache.get_art_grid(
                        &app.logic,
                        Some(id),
                        large_art.cols as usize,
                        large_art_pixel_rows,
                    );
                    large_art_grids.insert(id.clone(), grid);
                }
                if has_picker && !sliced_protocols.contains_key(id) {
                    let proto = app.cover_art_cache.get_sliced_protocol(
                        &app.logic,
                        Some(id),
                        large_art.size(),
                    );
                    sliced_protocols.insert(id.clone(), proto);
                }
            }
        }
    } else if album_art_style == AlbumArtStyle::LeftOfAlbum && has_picker {
        // Pre-fetch thumbnail-sized protocols for visible group headers.
        let thumbnail = super::layout::ArtColumn::thumbnail();
        for entry in &entries[item_offset..visible_item_end] {
            if let LibraryEntry::GroupHeader {
                cover_art_id: Some(id),
                ..
            } = entry
                && !thumbnail_protocols.contains_key(id)
            {
                let proto = app.cover_art_cache.get_protocol(
                    &app.logic,
                    Some(id),
                    Resolution::Library,
                    thumbnail.cols,
                    thumbnail.rows,
                );
                thumbnail_protocols.insert(id.clone(), proto);
            }
        }
    }
    // Determine whether image protocols are available for the art columns.
    // When true, ListItems render blank spaces instead of half-block art,
    // reserving space for Image/SlicedImage widgets rendered on top afterward.
    let has_image_protocol = has_picker
        && ((album_art_style == AlbumArtStyle::BelowAlbum && !sliced_protocols.is_empty())
            || (album_art_style == AlbumArtStyle::LeftOfAlbum && !thumbnail_protocols.is_empty()));

    // Build ListItems only for the visible range.
    let render_ctx = EntryRenderContext {
        album_art_style,
        list_width,
        large_art,
        background_color,
        album_color,
        album_year_color,
        album_length_color,
        track_number_color,
        track_name_color,
        track_name_playing_color,
        track_name_hovered_color,
        track_length_color,
        track_duration_color,
        playing_track_id: playing_track_id.as_ref(),
        selected_index,
        underline_index,
        hovered_heart_index,
        hovered_entry_index,
        art_colors: &art_colors,
        large_art_grids: &large_art_grids,
        has_image_protocol,
    };

    let items: Vec<ListItem> = entries[item_offset..visible_item_end]
        .iter()
        .enumerate()
        .map(|(vi, entry)| {
            let i = item_offset + vi;
            render_library_entry(entry, i, &render_ctx)
        })
        .collect();

    let list = List::new(items);

    // Render with offset 0 since we only built visible items starting from
    // item_offset. We intentionally don't set a selection — we handle
    // highlighting manually in item styles, and setting one would cause
    // ratatui to auto-scroll the offset to keep the selected item visible,
    // fighting our viewport positioning.
    let mut state = ListState::default();
    *state.offset_mut() = 0;

    frame.render_stateful_widget(list, inner, &mut state);

    // Render image-protocol widgets on top of the List for art that has
    // a graphics protocol available. This overlays the blank space reserved
    // during List rendering.
    if has_image_protocol {
        let window = OverlayWindow {
            entries,
            item_offset,
            item_end: visible_item_end,
            inner,
        };
        if album_art_style == AlbumArtStyle::BelowAlbum {
            render_below_album_images(frame, &window, large_art, &sliced_protocols);
        } else if album_art_style == AlbumArtStyle::LeftOfAlbum {
            render_left_of_album_thumbnails(frame, &window, &thumbnail_protocols);
        }
    }

    // Render combined scrollbar + library indicator on the right column.
    render_scrollbar_with_library_indicator(
        frame,
        entries,
        inner,
        geo.visible_height,
        geo.total_lines,
        centered_offset,
        has_scrollbar,
        app.logic.get_sort_order(),
        text_color,
        background_color,
    );
}

/// Renders `SlicedImage` widgets on top of the List for visible groups in
/// `BelowAlbum` mode.
///
/// Walks the flat library entries to find `GroupHeader` positions, computes
/// the art Rect, and renders a `SlicedImage` with a `SignedPosition` that
/// offsets the image start relative to the render area when a group is
/// partially scrolled above the viewport.
///
/// Headers *before* `item_offset` are considered too: a group whose header
/// has scrolled above the viewport can still have visible art rows beside
/// its visible tracks.
fn render_below_album_images(
    frame: &mut Frame,
    window: &OverlayWindow<'_>,
    large_art: super::layout::ArtColumn,
    sliced_protocols: &HashMap<CoverArtId, Option<Arc<SlicedProtocol>>>,
) {
    let inner = window.inner;
    let display_origin = window.display_origin();

    // Walk entries, tracking the absolute line offset of each group header.
    let mut current_line = 0i32;
    for entry in &window.entries[..window.item_end] {
        if let LibraryEntry::GroupHeader {
            cover_art_id: Some(id),
            ..
        } = entry
            && let Some(Some(protocol)) = sliced_protocols.get(id)
        {
            // The art area starts below the 2-line GroupHeader.
            let art_start_line = current_line + 2;

            // Screen row of the art's top relative to `inner`; negative when
            // the group is partially scrolled above the viewport.
            let screen_offset = art_start_line - display_origin;

            // Rows of the art hidden above the viewport.
            let skip_rows = (-screen_offset).max(0);
            if skip_rows < i32::from(large_art.rows) {
                let art_y = inner.y + screen_offset.max(0) as u16;
                let art_rect = large_art.rect(inner, art_y);
                if art_rect.height > 0 && art_rect.width > 0 {
                    // `SignedPosition.y` tells `SlicedImage` how many image
                    // rows to skip from the top, showing only the lower
                    // portion of a partially scrolled group's art.
                    let position = SignedPosition {
                        x: 0,
                        y: -(skip_rows as i16),
                    };
                    frame.render_widget(SlicedImage::new(protocol, position), art_rect);
                }
            }
        }

        current_line += entry.height() as i32;
    }
}

/// Renders `Image` widgets on top of the List for visible group headers in
/// `LeftOfAlbum` mode (thumbnail-sized art).
fn render_left_of_album_thumbnails(
    frame: &mut Frame,
    window: &OverlayWindow<'_>,
    thumbnail_protocols: &HashMap<CoverArtId, Option<Arc<Protocol>>>,
) {
    let inner = window.inner;
    let display_origin = window.display_origin();
    let thumbnail = super::layout::ArtColumn::thumbnail();

    let mut current_line = 0i32;
    for (i, entry) in window.entries.iter().enumerate() {
        if i < window.item_offset {
            current_line += entry.height() as i32;
            continue;
        }
        if i >= window.item_end {
            break;
        }

        if let LibraryEntry::GroupHeader {
            cover_art_id: Some(id),
            ..
        } = entry
        {
            let Some(Some(protocol)) = thumbnail_protocols.get(id) else {
                current_line += entry.height() as i32;
                continue;
            };

            // The thumbnail sits on the first row of the 2-line header.
            let header_y = inner.y + (current_line - display_origin).max(0) as u16;
            let art_rect = thumbnail.rect(inner, header_y);

            if art_rect.height == 0 || art_rect.width == 0 {
                current_line += entry.height() as i32;
                continue;
            }

            frame.render_widget(ratatui::widgets::Clear, art_rect);
            frame.render_widget(ratatui_image::Image::new(protocol), art_rect);
        }

        current_line += entry.height() as i32;
    }
}

/// Map a [`HeartState`] to a TUI string and style.
/// The `dim_color` is used for the hidden state so that the space character
/// inherits the surrounding dim style.
fn heart_to_tui(
    state: blackbird_client_shared::style::HeartState,
    dim_color: Color,
) -> (&'static str, Style) {
    use blackbird_client_shared::style::HeartState;
    match state {
        HeartState::Hidden => (" ", Style::default().fg(dim_color)),
        HeartState::Preview => ("\u{2661}", Style::default().fg(Color::Red)),
        HeartState::Active => ("\u{2665}", Style::default().fg(Color::Red)),
        HeartState::HoveredActive => ("\u{2665}", Style::default().fg(Color::White)),
    }
}

/// Returns `true` if the given mouse X position falls within the art area
/// for a track that is within the art rows in `BelowAlbum` mode.
fn is_over_below_album_art(
    album_art_style: AlbumArtStyle,
    mx: u16,
    area: Rect,
    entry: &LibraryEntry,
) -> bool {
    if album_art_style != AlbumArtStyle::BelowAlbum {
        return false;
    }
    let track_index = match entry {
        LibraryEntry::Track {
            track_index_in_group,
            ..
        } => *track_index_in_group,
        LibraryEntry::GroupSpacer { art_row_index, .. } => *art_row_index,
        _ => return false,
    };
    let large_art = super::layout::ArtColumn::large();
    if track_index >= large_art.rows as usize {
        return false;
    }
    let art_end_col = area.x as usize + large_art.total_width() as usize;
    (mx as usize) < art_end_col
}

/// Computes which library entry's heart is being hovered by the mouse, if any.
/// The caller must ensure the flat library cache is fresh before calling this.
fn compute_hovered_heart_index(app: &App, area: Rect) -> Option<usize> {
    // Suppress hover when the playback mode dropdown is covering the library.
    if app.playback_mode_dropdown {
        return None;
    }

    let (mx, my) = app.mouse_position?;

    // Must be within library area
    if my < area.y || my >= area.y + area.height || mx < area.x || mx >= area.x + area.width {
        return None;
    }

    let scroll_offset = app.library.scroll_offset;
    let album_art_style = app.config.layout.base.album_art_style;
    let entries = app.library.flat_library();

    // Compute list_width and heart column.
    let total_lines = total_entry_lines(entries);
    let indicator_width = scroll_indicator_width(app.logic.get_sort_order());
    let geo = super::layout::library_geometry(area, total_lines, indicator_width);
    let heart_col = geo.heart_col;

    // Check if mouse is on the heart column
    if (mx as usize) < heart_col || (mx as usize) > heart_col + 1 {
        return None;
    }

    // Determine which entry is at this Y position
    let inner_y = my.saturating_sub(area.y) as usize;

    let mut line = 0usize;
    for (i, entry) in entries.iter().enumerate().skip(scroll_offset) {
        let h = entry.height();

        if inner_y >= line && inner_y < line + h {
            // Suppress hover for entries under the art area in BelowAlbum mode.
            if is_over_below_album_art(album_art_style, mx, area, entry) {
                return None;
            }
            match entry {
                // For group headers, heart is only on line 2 (index 1).
                LibraryEntry::GroupHeader { .. } => {
                    if inner_y - line == 1 {
                        return Some(i);
                    }
                    return None;
                }
                LibraryEntry::Track { .. } | LibraryEntry::GroupSpacer { .. } => {
                    return Some(i);
                }
                LibraryEntry::AlbumGap => return None,
            }
        }
        line += h;
    }
    None
}

/// Computes which library entry is being hovered by the mouse, if any.
/// Unlike `compute_hovered_heart_index`, this triggers on any X position within the row,
/// not just the heart column. For group headers, only the second line (album line) counts.
/// The caller must ensure the flat library cache is fresh before calling this.
fn compute_hovered_entry_index(app: &App, area: Rect) -> Option<usize> {
    // Suppress hover when the playback mode dropdown is covering the library.
    if app.playback_mode_dropdown {
        return None;
    }

    let (mx, my) = app.mouse_position?;

    // Must be within library area.
    if my < area.y || my >= area.y + area.height || mx < area.x || mx >= area.x + area.width {
        return None;
    }

    let scroll_offset = app.library.scroll_offset;
    let album_art_style = app.config.layout.base.album_art_style;
    let entries = app.library.flat_library();

    let inner_y = my.saturating_sub(area.y) as usize;

    let mut line = 0usize;
    for (i, entry) in entries.iter().enumerate().skip(scroll_offset) {
        let h = entry.height();

        if inner_y >= line && inner_y < line + h {
            // Suppress hover for entries under the art area in BelowAlbum mode.
            if is_over_below_album_art(album_art_style, mx, area, entry) {
                return None;
            }
            match entry {
                LibraryEntry::Track { .. } => return Some(i),
                // Only the second line (album name) triggers hover.
                LibraryEntry::GroupHeader { .. } if inner_y - line == 1 => return Some(i),
                _ => return None,
            }
        }
        line += h;
    }
    None
}

/// Renders a combined scrollbar + library scroll indicator on the rightmost column.
///
/// The indicator shows different labels based on sort order:
/// - Alphabetical: first letter of artist name (A-Z)
/// - NewestFirst: release year
/// - RecentlyAdded: year from the created date
///
/// Each cell in the column shows one of:
/// - Label on thumb: inverted (background fg, text bg) — shows both thumb and label
/// - Label off thumb: normal
/// - Thumb without label: "█" block
/// - Empty track: "│" line (only when scrollbar visible)
#[allow(clippy::too_many_arguments)]
fn render_scrollbar_with_library_indicator(
    frame: &mut Frame,
    entries: &[LibraryEntry],
    area: Rect,
    visible_height: usize,
    total_lines: usize,
    scroll_offset: usize,
    has_scrollbar: bool,
    sort_order: SortOrder,
    text_color: Color,
    background_color: Color,
) {
    use std::borrow::Cow;

    if visible_height == 0 {
        return;
    }

    // Aggregate entries into groups for the shared logic.
    let mut groups: Vec<(Cow<'_, str>, usize)> = Vec::new();
    for entry in entries {
        match entry {
            LibraryEntry::GroupHeader {
                artist,
                year,
                created,
                ..
            } => {
                let label: Cow<'_, str> = match sort_order {
                    SortOrder::Alphabetical => {
                        Cow::Owned(artist.chars().next().unwrap_or('?').to_string())
                    }
                    SortOrder::NewestFirst => Cow::Owned(
                        year.map(|y| y.to_string())
                            .unwrap_or_else(|| "?".to_string()),
                    ),
                    SortOrder::RecentlyAdded => Cow::Owned(
                        created
                            .as_ref()
                            .map(|c| c.chars().take(4).collect::<String>())
                            .filter(|s| !s.is_empty())
                            .unwrap_or_else(|| "?".to_string()),
                    ),
                    SortOrder::MostPlayed => Cow::Borrowed(""),
                };
                groups.push((label, entry.height()));
            }
            LibraryEntry::Track { .. }
            | LibraryEntry::GroupSpacer { .. }
            | LibraryEntry::AlbumGap => {
                if let Some(last) = groups.last_mut() {
                    last.1 += entry.height();
                }
            }
        }
    }

    let cluster_threshold = 1.0 / visible_height as f32;
    let positions = library_scroll::compute_positions(groups.into_iter(), cluster_threshold);

    // Build a map of screen row -> label for quick lookup.
    let mut label_at_row: HashMap<u16, &str> = HashMap::new();
    for (label, fraction) in &positions {
        let row = (fraction * visible_height as f32) as u16;
        if row < area.height {
            label_at_row.insert(row, label.as_str());
        }
    }

    // Calculate thumb row range.
    let thumb_range = if has_scrollbar && total_lines > 0 {
        let vh = visible_height as f32;
        let thumb_start_frac = scroll_offset as f32 / total_lines as f32;
        let thumb_size_frac = visible_height as f32 / total_lines as f32;
        let start = (thumb_start_frac * vh) as u16;
        let end = ((thumb_start_frac + thumb_size_frac) * vh).ceil() as u16;
        Some((start, end))
    } else {
        None
    };

    // Rightmost position for right-aligned labels.
    let right_edge = area.x + area.width;
    let buf = frame.buffer_mut();

    for row in 0..area.height {
        let is_thumb = thumb_range
            .map(|(s, e)| row >= s && row < e)
            .unwrap_or(false);
        let label = label_at_row.get(&row);

        // Write label or scrollbar indicator directly to the buffer.
        if let Some(lbl) = label {
            let label_width = lbl.len() as u16;
            let label_x = right_edge.saturating_sub(label_width);
            let style = if is_thumb {
                Style::default().fg(background_color).bg(text_color)
            } else {
                Style::default().fg(text_color)
            };
            for (ci, ch) in lbl.chars().enumerate() {
                let x = label_x + ci as u16;
                let y = area.y + row;
                let pos = Position::new(x, y);
                if area.contains(pos) {
                    let cell = &mut buf[pos];
                    cell.set_char(ch);
                    cell.set_style(style);
                }
            }
        } else {
            let (ch, should_render) = match (is_thumb, has_scrollbar) {
                (true, _) => ('█', true),
                (false, true) => ('│', true),
                (false, false) => (' ', false),
            };
            if should_render {
                let x = right_edge.saturating_sub(1);
                let y = area.y + row;
                let pos = Position::new(x, y);
                if area.contains(pos) {
                    let cell = &mut buf[pos];
                    cell.set_char(ch);
                    cell.set_style(Style::default().fg(text_color));
                }
            }
        }
    }
}

pub fn handle_key(app: &mut App, action: Action) {
    app.library.cancel_inertia(&app.logic);
    let entries_len = app.library.flat_library_len();

    match action {
        Action::Quit => app.quit_confirming = true,
        Action::PlayPause => app.logic.toggle_current(),
        Action::Next => app.logic.next(),
        Action::Previous => app.logic.previous(),
        Action::NextGroup => app.logic.next_group(),
        Action::PreviousGroup => app.logic.previous_group(),
        Action::Stop => app.logic.stop_current(),
        Action::CyclePlaybackMode(dir) => app.cycle_playback_mode(dir),
        Action::ToggleSortOrder(dir) => {
            let scroll_target = app.library.selected_track_id().cloned();
            let next = blackbird_client_shared::cycle(
                &bc::SortOrder::ALL,
                app.logic.get_sort_order(),
                dir,
            );
            app.logic.set_sort_order(next);
            app.library.mark_dirty();
            app.library.scroll_to_track = scroll_target;
            // Viewport will be re-centered when scroll_to_track resolves in tick().
        }
        Action::Search => app.toggle_search(),
        Action::Lyrics => app.toggle_lyrics(),
        Action::Logs => app.toggle_logs(),
        Action::Queue => app.toggle_queue(),
        Action::Settings => app.toggle_settings(),
        Action::VolumeMode => app.volume_editing = true,
        Action::GotoPlaying => {
            if let Some(track_id) = app.logic.get_playing_track_id() {
                app.library.scroll_to_track = Some(track_id);
            }
        }
        Action::SeekBackward => app.seek_relative(-super::layout::SEEK_STEP_SECS),
        Action::SeekForward => app.seek_relative(super::layout::SEEK_STEP_SECS),
        Action::Star => {
            if let Some(track_id) = app.logic.get_playing_track_id() {
                let state = app.logic.get_state();
                let starred = state
                    .read()
                    .unwrap()
                    .library
                    .track_map
                    .get(&track_id)
                    .is_some_and(|t| t.starred);
                app.logic.set_track_starred(&track_id, !starred);
                app.library.mark_dirty();
            }
        }
        Action::MoveUp => {
            let mut new_index = app.library.selected_index;
            while new_index > 0 {
                new_index -= 1;
                if let Some(LibraryEntry::Track { .. }) =
                    app.library.get_library_entry(&app.logic, new_index)
                {
                    break;
                }
            }
            if let Some(LibraryEntry::Track { .. }) =
                app.library.get_library_entry(&app.logic, new_index)
            {
                app.library.selected_index = new_index;
                app.library.ensure_viewport_shows_selection();
            }
        }
        Action::MoveDown => {
            let mut new_index = app.library.selected_index;
            while new_index < entries_len.saturating_sub(1) {
                new_index += 1;
                if let Some(LibraryEntry::Track { .. }) =
                    app.library.get_library_entry(&app.logic, new_index)
                {
                    break;
                }
            }
            if let Some(LibraryEntry::Track { .. }) =
                app.library.get_library_entry(&app.logic, new_index)
            {
                app.library.selected_index = new_index;
                app.library.ensure_viewport_shows_selection();
            }
        }
        Action::PageUp => {
            let target = app
                .library
                .selected_index
                .saturating_sub(super::layout::PAGE_SCROLL_SIZE);
            let mut new_index = target;
            while new_index < entries_len {
                if let Some(LibraryEntry::Track { .. }) =
                    app.library.get_library_entry(&app.logic, new_index)
                {
                    break;
                }
                new_index += 1;
            }
            if new_index < entries_len {
                app.library.selected_index = new_index;
                app.library.ensure_viewport_shows_selection();
            }
        }
        Action::PageDown if entries_len > 0 => {
            let target =
                (app.library.selected_index + super::layout::PAGE_SCROLL_SIZE).min(entries_len - 1);
            let mut new_index = target;
            loop {
                if let Some(LibraryEntry::Track { .. }) =
                    app.library.get_library_entry(&app.logic, new_index)
                {
                    break;
                }
                if new_index == 0 {
                    break;
                }
                new_index -= 1;
            }
            if let Some(LibraryEntry::Track { .. }) =
                app.library.get_library_entry(&app.logic, new_index)
            {
                app.library.selected_index = new_index;
                app.library.ensure_viewport_shows_selection();
            }
        }
        Action::GotoTop => {
            for i in 0..entries_len {
                if let Some(LibraryEntry::Track { .. }) =
                    app.library.get_library_entry(&app.logic, i)
                {
                    app.library.selected_index = i;
                    app.library.center_viewport_on_selection();
                    break;
                }
            }
        }
        Action::GotoBottom if entries_len > 0 => {
            for i in (0..entries_len).rev() {
                if let Some(LibraryEntry::Track { .. }) =
                    app.library.get_library_entry(&app.logic, i)
                {
                    app.library.selected_index = i;
                    app.library.center_viewport_on_selection();
                    break;
                }
            }
        }
        Action::Select => {
            let selected = app.library.selected_index;
            if let Some(LibraryEntry::Track { id, .. }) =
                app.library.get_library_entry(&app.logic, selected)
            {
                app.logic.request_play_track(&id);
            }
        }
        _ => {}
    }
}

/// Handle click in the library area.
pub fn handle_mouse_click(app: &mut App, library_area: Rect, x: u16, y: u16) {
    app.library.cancel_inertia(&app.logic);
    app.library.ensure_flat_library(&app.logic);
    let total_lines = total_entry_lines(app.library.flat_library());

    // Determine the scroll indicator area (rightmost columns based on sort order).
    let indicator_width = scroll_indicator_width(app.logic.get_sort_order()) as u16;
    let scroll_area_start = library_area.x + library_area.width.saturating_sub(indicator_width);

    // Click on scroll indicator area.
    if x >= scroll_area_start {
        scroll_to_y(app, total_lines, library_area, y);
        app.library.viewport.scrollbar_dragging = true;
        return;
    }

    // Calculate which entry was clicked.
    let inner_y = y.saturating_sub(library_area.y);
    let scroll_offset = app.library.scroll_offset;

    let mut line = 0usize;
    let mut clicked_index = None;
    let mut click_line_in_entry = 0usize;

    let entries = app.library.flat_library();
    for (i, entry) in entries.iter().enumerate().skip(scroll_offset) {
        let h = entry.height();

        if inner_y as usize >= line && (inner_y as usize) < line + h {
            clicked_index = Some(i);
            click_line_in_entry = inner_y as usize - line;
            break;
        }
        line += h;
    }

    let Some(index) = clicked_index else {
        return;
    };

    // Gather click geometry.
    let indicator_width = scroll_indicator_width(app.logic.get_sort_order());
    let geo = super::layout::library_geometry(library_area, total_lines, indicator_width);
    let heart_col = geo.heart_col;
    let is_heart_click = x as usize >= heart_col && x as usize <= heart_col + 1;
    let album_art_style = app.config.layout.base.album_art_style;

    // Extract the data we need from the entry before dropping the borrow.
    let entries = app.library.flat_library();
    let Some(entry) = entries.get(index) else {
        return;
    };

    // In BelowAlbum mode, clicking the art area on a track or spacer opens the overlay.
    if is_over_below_album_art(album_art_style, x, library_area, entry) {
        let cover_art_id = match entry {
            LibraryEntry::Track { cover_art_id, .. }
            | LibraryEntry::GroupSpacer { cover_art_id, .. } => cover_art_id.clone(),
            _ => None,
        };
        if let Some(id) = cover_art_id {
            let title = find_group_title_for_entry(entries, index);
            app.album_art_overlay = Some(AlbumArtOverlay {
                cover_art_id: id,
                title,
            });
        }
        return;
    }

    match entry {
        LibraryEntry::Track { id, starred, .. } => {
            if is_heart_click {
                let id = id.clone();
                let starred = *starred;
                app.logic.set_track_starred(&id, !starred);
                app.library.mark_dirty();
            } else {
                app.library.click_pending = Some((x, y, index));
                app.library.viewport.dragging = false;
                app.library.viewport.drag_last_y = Some(y);
            }
        }
        LibraryEntry::GroupHeader {
            artist,
            album,
            album_id,
            starred,
            cover_art_id,
            ..
        } => {
            let art_end_col = library_area.x + super::layout::art_end_col();
            if album_art_style == AlbumArtStyle::LeftOfAlbum && x < art_end_col {
                if let Some(id) = cover_art_id {
                    app.album_art_overlay = Some(AlbumArtOverlay {
                        cover_art_id: id.clone(),
                        title: format!("{artist} \u{2013} {album}"),
                    });
                }
            } else if is_heart_click && click_line_in_entry == 1 {
                let album_id = album_id.clone();
                let starred = *starred;
                app.logic.set_album_starred(&album_id, !starred);
                app.library.mark_dirty();
            } else {
                app.library.click_pending = Some((x, y, index));
                app.library.viewport.dragging = false;
                app.library.viewport.drag_last_y = Some(y);
            }
        }
        LibraryEntry::GroupSpacer { .. } | LibraryEntry::AlbumGap => {
            // Spacers and gaps can't be clicked to play, but should allow drag-scrolling.
            // Setting click_pending with the index is safe because
            // handle_mouse_up only plays Track entries.
            app.library.click_pending = Some((x, y, index));
            app.library.viewport.dragging = false;
            app.library.viewport.drag_last_y = Some(y);
        }
    }
}

/// Find the group header title (artist – album) for an entry by searching backward.
fn find_group_title_for_entry(entries: &[LibraryEntry], index: usize) -> String {
    for i in (0..=index).rev() {
        if let LibraryEntry::GroupHeader { artist, album, .. } = &entries[i] {
            return format!("{artist} \u{2013} {album}");
        }
    }
    String::new()
}

/// Handle mouse drag in the library area. Returns `true` if the drag was handled.
pub fn handle_mouse_drag(app: &mut App, library_area: Rect, x: u16, y: u16) -> bool {
    app.library.ensure_flat_library(&app.logic);
    let total_lines = total_entry_lines(app.library.flat_library());

    // Scrollbar drag — once started, continues regardless of x position.
    if app.library.viewport.scrollbar_dragging
        && y >= library_area.y
        && y < library_area.y + library_area.height
    {
        app.library.viewport.apply_scrollbar_drag(
            y,
            total_lines,
            library_area.y,
            library_area.height,
        );
        app.library.snap_cursor_to_viewport_center();
        app.library.click_pending = None;
        return true;
    }

    // Starting a drag in the scroll indicator column → scrollbar drag.
    let indicator_width = scroll_indicator_width(app.logic.get_sort_order()) as u16;
    if super::scroll::is_in_scrollbar_column(library_area, x, indicator_width)
        && y >= library_area.y
        && y < library_area.y + library_area.height
    {
        app.library.viewport.apply_scrollbar_drag(
            y,
            total_lines,
            library_area.y,
            library_area.height,
        );
        app.library.snap_cursor_to_viewport_center();
        app.library.click_pending = None;
        return true;
    }

    // Content drag → pan library by tracking viewport line offset.
    if app.library.click_pending.is_some() || app.library.viewport.dragging {
        app.library.click_pending = None;
        app.library.viewport.apply_content_drag(y, total_lines);

        // Select the entry under the cursor.
        let cursor_content_line =
            app.library.viewport.line + y.saturating_sub(library_area.y) as usize;
        let flat = app.library.flat_library();
        if let Some(entry_index) = entry_at_line(flat, cursor_content_line) {
            // Snap to the entry itself, or the nearest track if it's a header.
            let target = match flat.get(entry_index) {
                Some(LibraryEntry::Track { .. }) => Some(entry_index),
                Some(LibraryEntry::GroupHeader { .. }) => {
                    // Try the next entry (first track in the group).
                    let next = entry_index + 1;
                    match flat.get(next) {
                        Some(LibraryEntry::Track { .. }) => Some(next),
                        _ => None,
                    }
                }
                Some(LibraryEntry::GroupSpacer { .. }) | Some(LibraryEntry::AlbumGap) | None => {
                    None
                }
            };
            if let Some(idx) = target {
                app.library.selected_index = idx;
                app.library.drag_selected_index = Some(idx);
            }
        }
        return true;
    }

    false
}

/// Handle mouse button release in the library — confirm pending click or reset drag state.
pub fn handle_mouse_up(app: &mut App) {
    if let Some((_cx, _cy, index)) = app.library.click_pending.take()
        && !app.library.viewport.dragging
        && let Some(LibraryEntry::Track { id, .. }) =
            app.library.get_library_entry(&app.logic, index)
    {
        app.library.selected_index = index;
        app.logic.request_play_track(&id);
    }

    match app.library.viewport.end_drag() {
        super::scroll::EndDragOutcome::Settled => {
            app.library.snap_cursor_to_viewport_center();
        }
        super::scroll::EndDragOutcome::InertiaStarted | super::scroll::EndDragOutcome::Idle => {}
    }
    app.library.drag_selected_index = None;
}

/// Handle scroll wheel in the library. `direction` is -1 for up, 1 for down.
pub fn handle_scroll(app: &mut App, direction: i32, steps: usize) {
    app.library.cancel_inertia(&app.logic);
    let entries = app.library.get_flat_library(&app.logic);
    let total_lines = total_entry_lines(entries);
    app.library
        .viewport
        .apply_wheel(direction, steps, total_lines);
    app.library.snap_cursor_to_viewport_center();
}

/// Scroll library to a position based on Y coordinate (for scrollbar dragging).
pub fn scroll_to_y(app: &mut App, total_lines: usize, library_area: Rect, y: u16) {
    app.library
        .viewport
        .apply_scrollbar_drag(y, total_lines, library_area.y, library_area.height);
    app.library.snap_cursor_to_viewport_center();
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend, layout::Position};
    use ratatui_image::picker::Picker;

    use super::*;

    fn test_header(id: &str) -> LibraryEntry {
        LibraryEntry::GroupHeader {
            artist: "artist".to_string(),
            album: "album".to_string(),
            year: None,
            created: None,
            duration: 0,
            starred: false,
            album_id: blackbird_core::blackbird_state::AlbumId(id.into()),
            cover_art_id: Some(CoverArtId(id.into())),
        }
    }

    fn test_track(id: &str, index: usize) -> LibraryEntry {
        LibraryEntry::Track {
            id: TrackId(format!("{id}-{index}")),
            title: "track".to_string(),
            artist: None,
            album_artist: "artist".to_string(),
            track_number: Some(index as u32 + 1),
            disc_number: None,
            duration: None,
            starred: false,
            play_count: None,
            cover_art_id: Some(CoverArtId(id.into())),
            track_index_in_group: index,
        }
    }

    /// A header followed by enough tracks to hold a 4-row art area.
    fn test_entries(id: &str) -> Vec<LibraryEntry> {
        let mut entries = vec![test_header(id)];
        entries.extend((0..6).map(|i| test_track(id, i)));
        entries
    }

    /// A red 80×80 sliced protocol filling the test art column exactly (8×4
    /// cells at the halfblocks picker's 10×20 font).
    fn test_sliced_protocols(id: &str) -> HashMap<CoverArtId, Option<Arc<SlicedProtocol>>> {
        use image::{ImageBuffer, ImageEncoder, Rgba, codecs::png::PngEncoder};
        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(80, 80, Rgba([255, 0, 0, 255]));
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(img.as_raw(), 80, 80, image::ExtendedColorType::Rgba8)
            .unwrap();
        let dyn_img = image::load_from_memory(&png).unwrap();
        let sliced = SlicedProtocol::new(
            &Picker::halfblocks(),
            dyn_img,
            Some(ratatui::layout::Size::new(8, 4)),
        )
        .unwrap();

        let mut map = HashMap::new();
        map.insert(CoverArtId(id.into()), Some(Arc::new(sliced)));
        map
    }

    fn test_art_column() -> super::super::layout::ArtColumn {
        super::super::layout::ArtColumn {
            left_margin: 1,
            cols: 8,
            right_margin: 1,
            rows: 4,
        }
    }

    /// Returns which rows of the terminal have art (colored cells) in the
    /// art columns after rendering the below-album overlay.
    fn art_rows_after_render(entries: &[LibraryEntry], item_offset: usize) -> Vec<u16> {
        let protocols = test_sliced_protocols("a");
        let backend = TestBackend::new(20, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let window = OverlayWindow {
                    entries,
                    item_offset,
                    item_end: entries.len(),
                    inner: Rect::new(0, 0, 20, 10),
                };
                render_below_album_images(frame, &window, test_art_column(), &protocols);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        (0..10u16)
            .filter(|y| {
                (1..9u16).any(|x| {
                    let cell = buffer.cell(Position { x, y: *y }).unwrap();
                    cell.fg != ratatui::style::Color::default()
                        || cell.bg != ratatui::style::Color::default()
                })
            })
            .collect()
    }

    /// The scroll offset points one line into the 2-line header, which
    /// `compute_item_offset` still includes and the List renders in full —
    /// so the art must render below both header lines, not one row higher
    /// (where it would cover the album line).
    #[test]
    fn test_below_album_art_aligns_with_partially_scrolled_header() {
        let entries = test_entries("a");

        // Scroll offset 1 is inside the header; the header item is still
        // the first rendered item.
        let item_offset = compute_item_offset(&entries, 1);
        assert_eq!(item_offset, 0);

        // The header occupies screen rows 0-1, so art starts at row 2.
        assert_eq!(
            art_rows_after_render(&entries, item_offset),
            vec![2, 3, 4, 5]
        );
    }

    /// A group whose header has scrolled above the viewport still renders
    /// the visible lower portion of its art beside its visible tracks.
    #[test]
    fn test_below_album_art_renders_for_scrolled_off_header() {
        let entries = test_entries("a");

        // Scroll two art rows past the header: the first rendered item is
        // the third track, and the last two art rows are still visible.
        let item_offset = compute_item_offset(&entries, 4);
        assert_eq!(item_offset, 3);

        assert_eq!(art_rows_after_render(&entries, item_offset), vec![0, 1]);
    }
}
