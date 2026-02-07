use std::time::{Duration, Instant};

use blackbird_core::blackbird_state::TrackId;
use egui::{Align2, Color32, Key, Rect, TextStyle, Ui, pos2, vec2};

use crate::{bc, config::Config};

use super::shared::IncrementalSearchState;

pub struct SearchResults {
    pub results: Vec<TrackId>,
    pub current_match: Option<TrackId>,
    pub scroll_target: Option<TrackId>,
}

/// Pre-render: Handle input and compute search results
/// Returns search results to be used for rendering
pub fn pre_render(
    ui: &mut Ui,
    logic: &mut bc::Logic,
    config: &Config,
    state: &mut IncrementalSearchState,
    can_handle_input: bool,
) -> SearchResults {
    // Timeout for clearing the search buffer (from config)
    let search_timeout = Duration::from_millis(config.general.incremental_search_timeout_ms);

    // Clear search query and deactivate if timeout has elapsed
    if let Some(last_input) = state.last_input
        && last_input.elapsed() > search_timeout
    {
        state.query.clear();
        state.last_input = None;
        state.active = false;
    }

    // Get all search results
    let results = if !state.query.is_empty() {
        logic
            .get_state()
            .write()
            .unwrap()
            .library
            .search(&state.query)
    } else {
        Vec::new()
    };

    // Ensure the result index is within bounds
    if state.result_index >= results.len() && !results.is_empty() {
        state.result_index = results.len() - 1;
    }

    // Get the currently selected track from search results
    let current_match = results.get(state.result_index).cloned();

    // Capture keyboard input for incremental search (only when active)
    if can_handle_input && state.active {
        ui.input(|i| {
            // Track if query changed (to reset index)
            let mut query_changed = false;

            // Handle text input (printable characters)
            for event in &i.events {
                if let egui::Event::Text(text) = event {
                    // Only capture single characters (ignore paste operations).
                    // Also ignore '/' which is used to activate search.
                    if text.len() == 1 && !text.chars().all(|c| c.is_control()) && text != "/" {
                        state.query.push_str(text);
                        state.last_input = Some(Instant::now());
                        query_changed = true;
                    }
                }
            }

            // Handle backspace
            if i.key_pressed(Key::Backspace) {
                if !state.query.is_empty() {
                    state.query.pop();
                    state.last_input = Some(Instant::now());
                    query_changed = true;
                } else {
                    // Backspace with empty query deactivates search
                    state.active = false;
                }
            }

            // Reset index when query changes
            if query_changed {
                state.result_index = 0;
            }

            // Handle Up/Down arrows to navigate results
            if !results.is_empty() {
                if i.key_pressed(Key::ArrowDown) {
                    state.result_index = (state.result_index + 1).min(results.len() - 1);
                    state.last_input = Some(Instant::now());
                }
                if i.key_pressed(Key::ArrowUp) {
                    state.result_index = state.result_index.saturating_sub(1);
                    state.last_input = Some(Instant::now());
                }
            }

            // Handle escape to clear search and deactivate
            if i.key_pressed(Key::Escape) {
                state.query.clear();
                state.last_input = None;
                state.result_index = 0;
                state.active = false;
            }

            // Handle enter to play the matched track and deactivate
            if i.key_pressed(Key::Enter) {
                if let Some(track_id) = &current_match {
                    logic.request_play_track(track_id);
                }
                state.query.clear();
                state.last_input = None;
                state.result_index = 0;
                state.active = false;
            }
        });
    }

    let scroll_target = current_match.clone();

    SearchResults {
        results,
        current_match,
        scroll_target,
    }
}

/// Post-render: Display the search overlay UI
pub fn post_render(ui: &mut Ui, state: &IncrementalSearchState, search_results: &SearchResults) {
    // Only show overlay when search is active
    if !state.active {
        return;
    }

    // Position the overlay at the bottom of the UI
    let overlay_height = 30.0;
    let overlay_padding = 8.0;
    let overlay_rect = Rect::from_min_size(
        pos2(
            ui.min_rect().left() + overlay_padding,
            ui.min_rect().bottom() - overlay_height - overlay_padding,
        ),
        vec2(ui.available_width() - 2.0 * overlay_padding, overlay_height),
    );

    // Draw a semi-transparent background
    ui.painter().rect_filled(
        overlay_rect,
        4.0, // rounded corners
        Color32::from_black_alpha(200),
    );

    // Draw the search query text with result count
    let display_text = if state.query.is_empty() {
        "Search: _".to_string()
    } else if search_results.results.is_empty() {
        format!("Search: {}_ (no results)", state.query)
    } else {
        format!(
            "Search: {}_ ({}/{})",
            state.query,
            state.result_index + 1,
            search_results.results.len()
        )
    };
    ui.painter().text(
        pos2(overlay_rect.left() + 10.0, overlay_rect.center().y),
        Align2::LEFT_CENTER,
        display_text,
        TextStyle::Body.resolve(ui.style()),
        Color32::WHITE,
    );
}
