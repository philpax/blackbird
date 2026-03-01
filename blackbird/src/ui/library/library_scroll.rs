use std::borrow::Cow;

use blackbird_client_shared::{config::AlbumArtStyle, library_scroll as shared_scroll};
use blackbird_core::SortOrder;
use egui::{Align2, Rect, Stroke, TextStyle, Ui, pos2};

use crate::{
    bc::{self, blackbird_state::TrackId},
    ui::{style, style::StyleExt},
};

use super::{group, shared::LibraryScrollState};

/// Computes scroll indicator positions as fractions of total content.
/// The labels shown depend on the current sort order:
/// - Alphabetical: first letter of artist name (A-Z)
/// - NewestFirst: release year (full year like "2024")
/// - RecentlyAdded: year from the created date (full year like "2024")
pub fn compute_positions(
    logic: &mut bc::Logic,
    state: &mut LibraryScrollState,
    album_art_style: AlbumArtStyle,
) {
    let app_state = logic.get_state();
    let app_state = app_state.read().unwrap();

    state.positions.clear();

    if app_state.library.groups.is_empty() {
        return;
    }

    let sort_order = app_state.sort_order;

    // Convert groups to (label, line_count) pairs based on sort order.
    let group_data: Vec<(Cow<'_, str>, usize)> = app_state
        .library
        .groups
        .iter()
        .map(|grp| {
            let label: Cow<'_, str> = match sort_order {
                SortOrder::Alphabetical => {
                    // First letter of artist name.
                    Cow::Owned(grp.artist.chars().next().unwrap_or('?').to_string())
                }
                SortOrder::NewestFirst => {
                    // Full release year.
                    Cow::Owned(
                        grp.year
                            .map(|y| y.to_string())
                            .unwrap_or_else(|| "?".to_string()),
                    )
                }
                SortOrder::RecentlyAdded => {
                    // Full year from created date (first 4 chars of ISO date).
                    Cow::Owned(
                        app_state
                            .library
                            .albums
                            .get(&grp.album_id)
                            .map(|a| a.created.chars().take(4).collect::<String>())
                            .filter(|s| !s.is_empty())
                            .unwrap_or_else(|| "?".to_string()),
                    )
                }
            };
            let line_count = group::line_count(grp, album_art_style);
            (label, line_count)
        })
        .collect();

    // Use egui's standard cluster threshold (1.5% of viewport).
    const CLUSTER_THRESHOLD: f32 = 0.015;

    state.positions = shared_scroll::compute_positions(group_data.into_iter(), CLUSTER_THRESHOLD);
}

/// Renders scroll indicator labels to the right side where the scrollbar would be
pub fn render(
    ui: &mut Ui,
    style: &style::Style,
    state: &mut LibraryScrollState,
    viewport_rect: &Rect,
    app_state: &bc::AppState,
    playing_track_id: Option<&TrackId>,
    album_art_style: AlbumArtStyle,
) {
    if state.positions.is_empty() {
        return;
    }

    // Update cached playing track position if track changed
    if state.cached_playing_track_id.as_ref() != playing_track_id {
        state.cached_playing_track_id = playing_track_id.cloned();
        state.cached_playing_track_position = playing_track_id.and_then(|track_id| {
            compute_track_position_fraction(app_state, track_id, album_art_style)
        });
    }

    let font_id = TextStyle::Body.resolve(ui.style());
    let label_color = style.text_color32();

    let viewport_height = viewport_rect.height();

    // Map fractions to pixel positions
    // Clustering is already done in the precomputation step
    let scroll_style = &ui.style().spacing.scroll;
    let label_x =
        viewport_rect.right() - scroll_style.bar_outer_margin - scroll_style.bar_width / 2.0;

    for (label, fraction) in &state.positions {
        let y = viewport_rect.top() + (fraction * viewport_height);
        ui.painter().text(
            pos2(label_x, y),
            Align2::CENTER_CENTER,
            label,
            font_id.clone(),
            label_color,
        );
    }

    // Draw indicator line for currently playing track
    if let Some(position_fraction) = state.cached_playing_track_position {
        let y = viewport_rect.top() + (position_fraction * viewport_height);
        let line_start_x = viewport_rect.right() - scroll_style.bar_width - 1.0;
        let line_end_x = viewport_rect.right() + 1.0;

        ui.painter().line_segment(
            [pos2(line_start_x, y), pos2(line_end_x, y)],
            Stroke::new(2.0, style.track_name_playing_color32()),
        );
    }
}

/// Computes the position fraction (0.0-1.0) of a track in the library.
fn compute_track_position_fraction(
    app_state: &bc::AppState,
    track_id: &TrackId,
    album_art_style: AlbumArtStyle,
) -> Option<f32> {
    let track = app_state.library.track_map.get(track_id)?;
    let album_id = track.album_id.as_ref()?;

    let mut current_row = 0;
    let mut track_row = None;

    for group in &app_state.library.groups {
        if group.album_id == *album_id {
            track_row = Some(current_row + group::line_count_for_group_and_track(group, track_id));
            break;
        }

        current_row += group::line_count(group, album_art_style);
    }

    let track_row = track_row?;
    let total_rows: usize = app_state
        .library
        .groups
        .iter()
        .map(|g| group::line_count(g, album_art_style))
        .sum();

    if total_rows == 0 {
        return None;
    }

    Some(track_row as f32 / total_rows as f32)
}
