use std::time::Instant;

use blackbird_core::blackbird_state::TrackId;
use egui::{Align, Pos2, Rect, ScrollArea, Spinner, Ui, pos2, style::ScrollStyle, vec2};

use crate::{
    bc,
    config::Config,
    cover_art_cache::CoverArtCache,
    ui::{style::StyleExt, util},
};

use super::{group, incremental_search, library_scroll};

// ============================================================================
// State types
// ============================================================================

#[derive(Default)]
pub struct IncrementalSearchState {
    pub(crate) query: String,
    pub(crate) last_input: Option<Instant>,
    pub(crate) result_index: usize,
    /// Whether the search is currently active (activated with `/`).
    pub(crate) active: bool,
}

#[derive(Default)]
pub struct LibraryScrollState {
    pub(crate) positions: Vec<(String, f32)>,
    pub(crate) needs_update: bool,
    pub(crate) cached_playing_track_id: Option<TrackId>,
    pub(crate) cached_playing_track_position: Option<f32>,
}

/// Shared state for library view rendering (used by both main library and mini-library)
#[derive(Default)]
pub struct LibraryViewState {
    pub(crate) library_scroll: LibraryScrollState,
    pub(crate) incremental_search: IncrementalSearchState,
}

impl LibraryViewState {
    pub fn invalidate_library_scroll(&mut self) {
        self.library_scroll.needs_update = true;
        self.library_scroll.cached_playing_track_id = None;
        self.library_scroll.cached_playing_track_position = None;
    }
}

// ============================================================================
// Rendering
// ============================================================================

/// Configuration for library view rendering behavior
pub(crate) struct LibraryViewConfig<'a> {
    /// External scroll target (e.g., from track started event)
    pub scroll_target: Option<&'a TrackId>,
    /// Whether to auto-scroll to the currently playing track when no other target
    pub auto_scroll_to_playing: bool,
    /// Whether incremental search input is enabled
    pub incremental_search_enabled: bool,
}

/// Render player controls: mouse button handling, now playing, scrub bar, and separator.
/// Returns track_to_scroll_to if the user clicked on the playing track info.
pub(crate) fn render_player_controls(
    ui: &mut Ui,
    logic: &mut bc::Logic,
    config: &Config,
    has_loaded_all_tracks: bool,
    cover_art_cache: &mut CoverArtCache,
) -> Option<TrackId> {
    ui.input(|i| {
        if let Some(button) = config
            .keybindings
            .parse_mouse_button(&config.keybindings.mouse_previous_track)
            && i.pointer.button_released(button)
        {
            logic.previous();
        }
        if let Some(button) = config
            .keybindings
            .parse_mouse_button(&config.keybindings.mouse_next_track)
            && i.pointer.button_released(button)
        {
            logic.next();
        }
    });

    let mut track_to_scroll_to = None;
    crate::ui::playing_track::ui(
        ui,
        logic,
        config,
        has_loaded_all_tracks,
        &mut track_to_scroll_to,
        cover_art_cache,
    );

    crate::ui::scrub_bar::ui(ui, logic, config);
    ui.separator();

    track_to_scroll_to
}

/// Render the library view with the given configuration
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_library_view(
    ui: &mut Ui,
    logic: &mut bc::Logic,
    config: &Config,
    has_loaded_all_tracks: bool,
    scroll_margin: f32,
    cover_art_cache: &mut CoverArtCache,
    view_state: &mut LibraryViewState,
    view_config: LibraryViewConfig<'_>,
) {
    ui.scope(|ui| {
        if !has_loaded_all_tracks {
            ui.add_sized(ui.available_size(), Spinner::new());
            return;
        }

        // Compute library scroll positions if library was populated
        if view_state.library_scroll.needs_update {
            library_scroll::compute_positions(logic, &mut view_state.library_scroll);
            view_state.library_scroll.needs_update = false;
        }

        // Handle incremental search (type-to-search)
        let search_results = incremental_search::pre_render(
            ui,
            logic,
            config,
            &mut view_state.incremental_search,
            view_config.incremental_search_enabled,
        );

        let current_search_match = search_results.current_match.clone();
        let incremental_search_scroll_target = search_results.scroll_target.clone();

        // Make the scroll bar solid, and hide its background
        ui.style_mut().spacing.scroll = ScrollStyle {
            bar_inner_margin: scroll_margin,
            bar_width: 20.0,
            handle_min_length: 36.0,
            ..ScrollStyle::solid()
        };
        ui.style_mut().visuals.extreme_bg_color = config.style.background_color32();

        let spaced_row_height = util::spaced_row_height(ui);
        let total_rows =
            logic.calculate_total_rows(group::line_count) - group::GROUP_MARGIN_BOTTOM_ROW_COUNT;

        let area_offset_y = ui.cursor().top();
        let playing_track_id = logic.get_playing_track_id();

        ScrollArea::vertical()
            .auto_shrink(false)
            .show_viewport(ui, |ui, viewport| {
                // Determine scroll target priority:
                // 1. Incremental search target
                // 2. External scroll target (track_to_scroll_to)
                // 3. Playing track (if auto_scroll_to_playing)
                let auto_scroll_target = if view_config.auto_scroll_to_playing {
                    playing_track_id.as_ref()
                } else {
                    None
                };
                let scroll_target = incremental_search_scroll_target
                    .as_ref()
                    .or(view_config.scroll_target)
                    .or(auto_scroll_target);

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

                // Set the total height for the virtual content
                ui.set_height(spaced_row_height * total_rows as f32);

                // Calculate which rows are visible with some buffer
                let first_visible_row =
                    ((viewport.min.y / spaced_row_height).floor().max(0.0)) as usize;
                let last_visible_row = (viewport.max.y / spaced_row_height).ceil() as usize + 5;
                let last_visible_row = last_visible_row.min(total_rows);

                if first_visible_row >= last_visible_row {
                    return;
                }

                let visible_row_range = first_visible_row..last_visible_row;

                // Calculate which groups are in view
                let visible_groups =
                    logic.get_visible_groups(visible_row_range.clone(), group::line_count);

                let mut current_row = visible_groups.start_row;

                for grp in visible_groups.groups {
                    let group_lines = group::line_count(&grp);

                    // Calculate the Y position for this group
                    let group_y = current_row as f32 * spaced_row_height;

                    let positioned_rect = Rect::from_min_size(
                        pos2(ui.min_rect().left(), ui.min_rect().top() + group_y),
                        vec2(
                            ui.available_width(),
                            (group_lines - 2 * group::GROUP_MARGIN_BOTTOM_ROW_COUNT) as f32
                                * spaced_row_height,
                        ),
                    );

                    let group_response = ui
                        .scope_builder(egui::UiBuilder::new().max_rect(positioned_rect), |ui| {
                            group::ui(
                                &grp,
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
                        logic.set_album_starred(&grp.album_id, !grp.starred);
                    }

                    current_row += group_lines;
                }
            });

        // Render library scroll indicator
        library_scroll::render(
            ui,
            &config.style,
            &mut view_state.library_scroll,
            &ui.min_rect(),
            &logic.get_state().read().unwrap(),
            playing_track_id.as_ref(),
        );

        // Display incremental search query overlay
        incremental_search::post_render(ui, &view_state.incremental_search, &search_results);
    });
}
