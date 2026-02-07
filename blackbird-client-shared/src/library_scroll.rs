//! Library scroll indicator logic shared between egui and TUI clients.
//!
//! This module computes scroll indicator positions based on the current sort order.
//! For alphabetical sorting, it shows letters (A-Z). For year-based sorting (newest
//! first or recently added), it shows full years (e.g., "2024").

use std::borrow::Cow;

/// Computes scroll indicator positions as fractions of total content.
///
/// Takes an iterator of (label, line_count) pairs for each group/entry,
/// and returns a list of (label, fraction) pairs representing where each
/// section starts in the library.
///
/// The `cluster_threshold` parameter controls how close labels can be before
/// they are merged (typically 0.015 for GUI, or 1.0/visible_height for TUI).
pub fn compute_positions<'a>(
    entries: impl Iterator<Item = (Cow<'a, str>, usize)>,
    cluster_threshold: f32,
) -> Vec<(String, f32)> {
    // Collect label positions with counts for clustering.
    let mut label_positions: Vec<(String, f32, usize)> = Vec::new();
    let mut current_line = 0usize;
    let mut last_label: Option<String> = None;

    let entries: Vec<_> = entries.collect();
    let total_lines: usize = entries.iter().map(|(_, lines)| lines).sum();

    if total_lines == 0 {
        return Vec::new();
    }

    for (label, line_count) in entries {
        let label_upper = label.to_uppercase();

        if last_label.as_ref() != Some(&label_upper) {
            let fraction = current_line as f32 / total_lines as f32;
            label_positions.push((label_upper.clone(), fraction, 1));
            last_label = Some(label_upper);
        } else if let Some(last) = label_positions.last_mut() {
            // Increment count for clustering (prefer labels with more entries).
            last.2 += 1;
        }

        current_line += line_count;
    }

    if label_positions.is_empty() {
        return Vec::new();
    }

    // Cluster nearby labels to avoid overlap.
    cluster_labels(label_positions, cluster_threshold)
}

/// Clusters labels that are too close together, keeping the one with highest count.
fn cluster_labels(positions: Vec<(String, f32, usize)>, threshold: f32) -> Vec<(String, f32)> {
    let mut clustered: Vec<(String, f32)> = Vec::new();
    let mut i = 0;

    while i < positions.len() {
        let mut cluster_end = i + 1;

        // Find all labels within threshold distance.
        while cluster_end < positions.len() {
            let distance = positions[cluster_end].1 - positions[i].1;
            if distance >= threshold {
                break;
            }
            cluster_end += 1;
        }

        // Select the label with the highest count in this cluster.
        let best = positions[i..cluster_end]
            .iter()
            .max_by_key(|(_, _, count)| count)
            .unwrap();

        clustered.push((best.0.clone(), best.1));
        i = cluster_end;
    }

    clustered
}

/// Computes the position fraction (0.0-1.0) for a specific item in the library.
///
/// Takes an iterator of (is_target, line_count) pairs and returns the fraction
/// where the first item with is_target=true appears.
pub fn compute_item_position(entries: impl Iterator<Item = (bool, usize)>) -> Option<f32> {
    let mut current_line = 0usize;
    let mut target_line = None;
    let mut total_lines = 0usize;

    for (is_target, line_count) in entries {
        if is_target && target_line.is_none() {
            target_line = Some(current_line);
        }
        current_line += line_count;
        total_lines += line_count;
    }

    let target = target_line?;
    if total_lines == 0 {
        return None;
    }

    Some(target as f32 / total_lines as f32)
}
