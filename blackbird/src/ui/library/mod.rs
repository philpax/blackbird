mod alphabet_scroll;
mod group;
mod incremental_search;
mod track;

pub use group::GROUP_ALBUM_ART_SIZE;

use blackbird_core::blackbird_state::TrackId;
use egui::{Align, Pos2, Rect, ScrollArea, Spinner, Ui, pos2, style::ScrollStyle, vec2};

use crate::{bc, config::Config, cover_art_cache::CoverArtCache, ui::util};

use super::UiState;

#[allow(clippy::too_many_arguments)]
pub fn ui(
    ui: &mut Ui,
    logic: &mut bc::Logic,
    config: &Config,
    has_loaded_all_tracks: bool,
    scroll_margin: f32,
    track_to_scroll_to: Option<&TrackId>,
    cover_art_cache: &mut CoverArtCache,
    ui_state: &mut UiState,
) {
    ui.scope(|ui| {
        if !has_loaded_all_tracks {
            ui.add_sized(ui.available_size(), Spinner::new());
            return;
        }

        // Compute alphabet scroll positions if library was populated
        if ui_state.alphabet_scroll.needs_update {
            alphabet_scroll::compute_positions(logic, &mut ui_state.alphabet_scroll);
            ui_state.alphabet_scroll.needs_update = false;
        }

        // Only capture keyboard input if search modal and lyrics window are not open
        let can_handle_incremental_search = !ui_state.search.open && !ui_state.lyrics.open;

        // Handle incremental search (type-to-search)
        let search_results = incremental_search::pre_render(
            ui,
            logic,
            config,
            &mut ui_state.incremental_search,
            can_handle_incremental_search,
        );

        let current_search_match = search_results.current_match.clone();
        let incremental_search_scroll_target = search_results.scroll_target.clone();

        // Make the scroll bar solid, and hide its background. Ideally, we'd set the opacity
        // to 0, but egui doesn't allow that for solid scroll bars.
        ui.style_mut().spacing.scroll = ScrollStyle {
            bar_inner_margin: scroll_margin,
            bar_width: 20.0,
            handle_min_length: 36.0,
            ..ScrollStyle::solid()
        };
        ui.style_mut().visuals.extreme_bg_color = config.style.background();

        let spaced_row_height = util::spaced_row_height(ui);
        let total_rows =
            logic.calculate_total_rows(group::line_count) - group::GROUP_MARGIN_BOTTOM_ROW_COUNT;

        let area_offset_y = ui.cursor().top();

        ScrollArea::vertical()
            .auto_shrink(false)
            .show_viewport(ui, |ui, viewport| {
                // Determine which track to scroll to (prioritize incremental search)
                let scroll_target = incremental_search_scroll_target
                    .as_ref()
                    .or(track_to_scroll_to);

                if let Some(scroll_to_height) = scroll_target.and_then(|id| {
                    group::target_scroll_height_for_track(
                        &logic.get_state().read().unwrap(),
                        spaced_row_height,
                        id,
                    )
                }) {
                    let target_height = area_offset_y + scroll_to_height - viewport.min.y;
                    ui.scroll_to_rect(
                        Rect {
                            min: Pos2::new(viewport.min.x, target_height),
                            max: Pos2::new(viewport.max.x, target_height + spaced_row_height),
                        },
                        Some(Align::Center),
                    );
                }

                // Set the total height for the virtual content (with spacing)
                ui.set_height(spaced_row_height * total_rows as f32);

                // Calculate which rows are visible with some buffer
                let first_visible_row =
                    ((viewport.min.y / spaced_row_height).floor().max(0.0)) as usize;
                let last_visible_row = (viewport.max.y / spaced_row_height).ceil() as usize + 5; // Add buffer
                let last_visible_row = last_visible_row.min(total_rows);

                if first_visible_row >= last_visible_row {
                    return;
                }

                let visible_row_range = first_visible_row..last_visible_row;

                // Calculate which groups are in view
                let visible_groups =
                    logic.get_visible_groups(visible_row_range.clone(), group::line_count);

                let playing_track_id = logic.get_playing_track_id();
                let mut current_row = visible_groups.start_row;

                for group in visible_groups.groups {
                    let group_lines = group::line_count(&group);

                    // Calculate the Y position for this group in viewport coordinates
                    let group_y = current_row as f32 * spaced_row_height;

                    // Always render complete albums (no partial visibility check)
                    let positioned_rect = Rect::from_min_size(
                        pos2(ui.min_rect().left(), ui.min_rect().top() + group_y),
                        vec2(
                            ui.available_width(),
                            (group_lines - 2 * group::GROUP_MARGIN_BOTTOM_ROW_COUNT) as f32
                                * spaced_row_height,
                        ),
                    );

                    // Display the complete group
                    let group_response = ui
                        .scope_builder(egui::UiBuilder::new().max_rect(positioned_rect), |ui| {
                            // Show the entire group (no row range filtering)
                            group::ui(
                                &group,
                                ui,
                                &config.style,
                                logic,
                                playing_track_id.as_ref(),
                                current_search_match.as_ref(),
                                cover_art_cache,
                            )
                        })
                        .inner;

                    // Handle track selection
                    if let Some(track_id) = group_response.clicked_track {
                        logic.request_play_track(track_id);
                    }

                    if group_response.clicked_heart {
                        logic.set_album_starred(&group.album_id, !group.starred);
                    }

                    current_row += group_lines;
                }
            });

        // Compute playing track position for indicator (only when track changes)
        let playing_track_id = logic.get_playing_track_id();
        if ui_state.alphabet_scroll.cached_playing_track_id != playing_track_id {
            ui_state.alphabet_scroll.cached_playing_track_id = playing_track_id.clone();
            ui_state.alphabet_scroll.cached_playing_track_position = playing_track_id
                .as_ref()
                .and_then(|track_id| alphabet_scroll::compute_track_position_fraction(logic, track_id));
        }

        // Render alphabet scroll indicator
        alphabet_scroll::render(
            ui,
            &config.style,
            &ui_state.alphabet_scroll,
            &ui.min_rect(),
            ui_state.alphabet_scroll.cached_playing_track_position,
        );

        // Display incremental search query overlay at the bottom
        incremental_search::post_render(ui, &ui_state.incremental_search, &search_results);
    });
}
