use egui::{Align2, Rect, TextStyle, Ui, pos2};

use crate::{
    bc,
    ui::{AlphabetScrollState, style},
};

use super::group;

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
    state: &AlphabetScrollState,
    viewport_rect: &Rect,
) {
    if state.positions.is_empty() {
        return;
    }

    let font_id = TextStyle::Body.resolve(ui.style());
    let letter_color = style.text();

    let viewport_height = viewport_rect.height();

    // Map fractions to pixel positions
    // Clustering is already done in the precomputation step
    let scroll_style = &ui.style().spacing.scroll;
    let letter_x =
        viewport_rect.right() - scroll_style.bar_inner_margin - scroll_style.bar_width / 2.0;

    for (letter, fraction) in &state.positions {
        let y = viewport_rect.top() + (fraction * viewport_height);
        ui.painter().text(
            pos2(letter_x, y),
            Align2::LEFT_CENTER,
            *letter,
            font_id.clone(),
            letter_color,
        );
    }
}
