use std::sync::Arc;

use blackbird_state::Group;

use crate::Logic;

pub struct VisibleGroupSet {
    pub groups: Vec<Arc<Group>>,
    pub start_row: usize,
}

impl Logic {
    pub fn calculate_total_rows(
        &self,
        group_margin_bottom_row_count: usize,
        group_line_count_getter: impl Fn(&Group) -> usize,
    ) -> usize {
        self.read_state()
            .groups
            .iter()
            .map(|group| group_line_count_getter(group) + group_margin_bottom_row_count)
            .sum()
    }

    pub fn get_visible_groups(
        &self,
        visible_row_range: std::ops::Range<usize>,
        group_margin_bottom_row_count: usize,
        group_line_count_getter: impl Fn(&Group) -> usize,
    ) -> VisibleGroupSet {
        let state = self.read_state();
        let mut current_row = 0;
        let visible_groups = Vec::new();

        // Add buffer albums before and after visible range
        const BUFFER_ALBUMS: usize = 3;

        // First pass: find albums that intersect with visible range
        let mut intersecting_album_indices = Vec::new();
        for (album_index, group) in state.groups.iter().enumerate() {
            let group_lines = group_line_count_getter(group) + group_margin_bottom_row_count;
            let group_range = current_row..(current_row + group_lines);

            // Check if this album intersects with visible range
            if group_range.start < visible_row_range.end
                && group_range.end > visible_row_range.start
            {
                intersecting_album_indices.push(album_index);
            }

            current_row += group_lines;
        }

        if intersecting_album_indices.is_empty() {
            return VisibleGroupSet {
                groups: visible_groups,
                start_row: 0,
            };
        }

        // Determine the range of albums to include with buffer
        let first_intersecting = intersecting_album_indices[0];
        let last_intersecting = intersecting_album_indices[intersecting_album_indices.len() - 1];

        let start_album_index = first_intersecting.saturating_sub(BUFFER_ALBUMS);
        let end_album_index = (last_intersecting + BUFFER_ALBUMS + 1).min(state.groups.len());

        // Calculate start_row for the first album we'll include
        current_row = 0;
        for i in 0..start_album_index {
            let group = &state.groups[i];
            let group_lines = group_line_count_getter(group) + group_margin_bottom_row_count;
            current_row += group_lines;
        }
        let start_row = current_row;

        // Include the selected range of albums
        let mut visible_groups = Vec::new();
        for i in start_album_index..end_album_index {
            visible_groups.push(state.groups[i].clone());
        }

        VisibleGroupSet {
            groups: visible_groups,
            start_row,
        }
    }
}
