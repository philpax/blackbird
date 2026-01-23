use egui::{Align2, Rect, Stroke, TextStyle, Ui, pos2};

use crate::{
    bc::{self, blackbird_state::TrackId},
    ui::style,
};

use super::{group, shared::AlphabetScrollState};

/// Computes alphabet scroll positions as fractions of total content
pub fn compute_positions(logic: &mut bc::Logic, state: &mut AlphabetScrollState) {
    let app_state = logic.get_state();
    let app_state = app_state.read().unwrap();

    state.positions.clear();

    if app_state.library.groups.is_empty() {
        return;
    }

    let collator = bc::blackbird_state::create_collator();

    // Group information: stores all character variants and their counts for each collator-equal group
    // along with the position where this group starts
    struct LetterGroup {
        position: usize,
        variants: std::collections::HashMap<char, usize>, // char -> count
    }

    let mut current_row = 0;
    let mut letter_groups: Vec<LetterGroup> = Vec::new();

    for group in &app_state.library.groups {
        // Get the first letter of the artist name (uppercase)
        if let Some(first_char) = group.artist.chars().next() {
            let initial = first_char.to_uppercase().next().unwrap_or(first_char);
            let initial_str = initial.to_string();

            // Find which group this letter belongs to (using collator)
            let group_idx = letter_groups.iter().position(|g| {
                // Get any variant from this group to compare against
                let representative = g.variants.keys().next().unwrap();
                let representative_str = representative.to_string();
                collator.compare(&initial_str, &representative_str) == std::cmp::Ordering::Equal
            });

            if let Some(idx) = group_idx {
                // Add to existing group
                *letter_groups[idx].variants.entry(initial).or_insert(0) += 1;
            } else {
                // Create new group
                let mut variants = std::collections::HashMap::new();
                variants.insert(initial, 1);
                letter_groups.push(LetterGroup {
                    position: current_row,
                    variants,
                });
            }
        }

        current_row += group::line_count(group);
    }

    let total_rows = current_row;

    // For each group, select the variant with the highest count and convert position to fraction
    let positions_with_fractions: Vec<(char, f32, usize)> = letter_groups
        .into_iter()
        .map(|group| {
            let (&best_char, &count) = group
                .variants
                .iter()
                .max_by_key(|&(_char, count)| count)
                .unwrap();
            let fraction = group.position as f32 / total_rows as f32;
            (best_char, fraction, count)
        })
        .collect();

    // Cluster nearby letters and select the one with the highest count
    // Threshold: letters within ~1.5% of viewport height are considered overlapping
    // This corresponds to roughly 15-20 pixels on a typical 1000-1200px viewport
    const CLUSTER_THRESHOLD: f32 = 0.015;

    let mut clustered_positions: Vec<(char, f32)> = Vec::new();
    let mut cluster_start = 0;

    while cluster_start < positions_with_fractions.len() {
        let mut cluster_end = cluster_start + 1;

        // Find all letters in this cluster (within threshold distance)
        while cluster_end < positions_with_fractions.len() {
            let distance =
                positions_with_fractions[cluster_end].1 - positions_with_fractions[cluster_start].1;
            if distance >= CLUSTER_THRESHOLD {
                break;
            }
            cluster_end += 1;
        }

        // Select the letter with the highest count in this cluster
        let best_in_cluster = positions_with_fractions[cluster_start..cluster_end]
            .iter()
            .max_by_key(|(_letter, _fraction, count)| count)
            .unwrap();

        clustered_positions.push((best_in_cluster.0, best_in_cluster.1));

        cluster_start = cluster_end;
    }

    state.positions = clustered_positions;
}

/// Renders alphabet letters to the right side where the scrollbar would be
pub fn render(
    ui: &mut Ui,
    style: &style::Style,
    state: &mut AlphabetScrollState,
    viewport_rect: &Rect,
    app_state: &bc::AppState,
    playing_track_id: Option<&TrackId>,
) {
    if state.positions.is_empty() {
        return;
    }

    // Update cached playing track position if track changed
    if state.cached_playing_track_id.as_ref() != playing_track_id {
        state.cached_playing_track_id = playing_track_id.cloned();
        state.cached_playing_track_position = playing_track_id
            .and_then(|track_id| compute_track_position_fraction(app_state, track_id));
    }

    let font_id = TextStyle::Body.resolve(ui.style());
    let letter_color = style.text();

    let viewport_height = viewport_rect.height();

    // Map fractions to pixel positions
    // Clustering is already done in the precomputation step
    let scroll_style = &ui.style().spacing.scroll;
    let letter_x =
        viewport_rect.right() - scroll_style.bar_outer_margin - scroll_style.bar_width / 2.0;

    for (letter, fraction) in &state.positions {
        let y = viewport_rect.top() + (fraction * viewport_height);
        ui.painter().text(
            pos2(letter_x, y),
            Align2::CENTER_CENTER,
            *letter,
            font_id.clone(),
            letter_color,
        );
    }

    // Draw indicator line for currently playing track
    if let Some(position_fraction) = state.cached_playing_track_position {
        let y = viewport_rect.top() + (position_fraction * viewport_height);
        let line_start_x = viewport_rect.right() - scroll_style.bar_width - 1.0;
        let line_end_x = viewport_rect.right() + 1.0;

        ui.painter().line_segment(
            [pos2(line_start_x, y), pos2(line_end_x, y)],
            Stroke::new(2.0, style.track_name_playing()),
        );
    }
}

/// Computes the position fraction (0.0-1.0) of a track in the library
fn compute_track_position_fraction(app_state: &bc::AppState, track_id: &TrackId) -> Option<f32> {
    let track = app_state.library.track_map.get(track_id)?;
    let album_id = track.album_id.as_ref()?;

    let mut current_row = 0;
    let mut track_row = None;

    for group in &app_state.library.groups {
        if group.album_id == *album_id {
            track_row = Some(current_row + group::line_count_for_group_and_track(group, track_id));
            break;
        }

        current_row += group::line_count(group);
    }

    let track_row = track_row?;
    let total_rows: usize = app_state
        .library
        .groups
        .iter()
        .map(|g| group::line_count(g))
        .sum();

    if total_rows == 0 {
        return None;
    }

    Some(track_row as f32 / total_rows as f32)
}
