use egui::{
    CentralPanel, Context, Frame, Key, Margin, ScrollArea, Spinner, ViewportBuilder, ViewportId,
    pos2, style::ScrollStyle, vec2,
};

use crate::{bc, config::Config, cover_art_cache::CoverArtCache, ui::util};

use super::{MiniLibraryState, library::{alphabet_scroll, group, incremental_search}};

/// Height of the mini-library as a fraction of main window height
const MINI_LIBRARY_HEIGHT_FRACTION: f32 = 0.4;

/// Main mini-library window UI
pub fn ui(
    logic: &mut bc::Logic,
    ctx: &Context,
    config: &Config,
    has_loaded_all_tracks: bool,
    cover_art_cache: &mut CoverArtCache,
    mini_library_state: &mut MiniLibraryState,
) {
    if !mini_library_state.open {
        ctx.send_viewport_cmd_to(mini_library_viewport_id(), egui::ViewportCommand::Close);
        return;
    }

    // Calculate window size: 40% height of main window, same width
    let screen_rect = ctx.screen_rect();
    let window_height = screen_rect.height() * MINI_LIBRARY_HEIGHT_FRACTION;
    let window_width = screen_rect.width() * 0.5; // Half screen width

    let mut close_window = false;

    ctx.show_viewport_immediate(
        mini_library_viewport_id(),
        ViewportBuilder::default()
            .with_title("Blackbird - Mini Library")
            .with_inner_size([window_width, window_height])
            .with_position([
                screen_rect.center().x - window_width / 2.0,
                screen_rect.center().y - window_height / 2.0,
            ])
            .with_active(true),
        |ctx, _class| {
            let margin = 8;
            let scroll_margin = 4;

            CentralPanel::default()
                .frame(
                    Frame::default()
                        .inner_margin(Margin {
                            left: margin,
                            right: scroll_margin,
                            top: margin,
                            bottom: margin,
                        })
                        .fill(config.style.background()),
                )
                .show(ctx, |ui| {
                    // Check for Escape key to close
                    if ui.input(|i| i.key_pressed(Key::Escape)) {
                        close_window = true;
                    }

                    // Check if viewport was closed
                    if ctx.input(|i| i.viewport().close_requested()) {
                        close_window = true;
                    }

                    // Handle mouse buttons for track navigation
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

                    // Playback controls and current track info
                    let mut track_to_scroll_to = None;
                    super::playing_track::ui(
                        ui,
                        logic,
                        config,
                        has_loaded_all_tracks,
                        &mut track_to_scroll_to,
                        cover_art_cache,
                    );

                    // Scrub bar
                    super::scrub_bar::ui(ui, logic, config);

                    ui.separator();

                    // Mini library content
                    render_library(
                        ui,
                        logic,
                        config,
                        has_loaded_all_tracks,
                        scroll_margin.into(),
                        cover_art_cache,
                        mini_library_state,
                    );
                });
        },
    );

    if close_window {
        mini_library_state.open = false;
    }
}

fn render_library(
    ui: &mut egui::Ui,
    logic: &mut bc::Logic,
    config: &Config,
    has_loaded_all_tracks: bool,
    scroll_margin: f32,
    cover_art_cache: &mut CoverArtCache,
    mini_library_state: &mut MiniLibraryState,
) {
    ui.scope(|ui| {
        if !has_loaded_all_tracks {
            ui.add_sized(ui.available_size(), Spinner::new());
            return;
        }

        // Compute alphabet scroll positions if needed
        if mini_library_state.alphabet_scroll.needs_update {
            alphabet_scroll::compute_positions(logic, &mut mini_library_state.alphabet_scroll);
            mini_library_state.alphabet_scroll.needs_update = false;
        }

        // Handle incremental search (type-to-search)
        let search_results = incremental_search::pre_render(
            ui,
            logic,
            config,
            &mut mini_library_state.incremental_search,
            true, // can_handle_incremental_search
        );

        let current_search_match = search_results.current_match.clone();
        let incremental_search_scroll_target = search_results.scroll_target.clone();

        // Configure scroll style
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

        // Get the currently playing track to center on
        let playing_track_id = logic.get_playing_track_id();

        ScrollArea::vertical()
            .auto_shrink(false)
            .show_viewport(ui, |ui, viewport| {
                // Determine scroll target - prioritize incremental search, then playing track
                let scroll_target = incremental_search_scroll_target
                    .as_ref()
                    .or(playing_track_id.as_ref());

                if let Some(scroll_to_height) = scroll_target.and_then(|id| {
                    group::target_scroll_height_for_track(
                        &logic.get_state().read().unwrap(),
                        spaced_row_height,
                        id,
                    )
                }) {
                    let target_height = area_offset_y + scroll_to_height - viewport.min.y;
                    ui.scroll_to_rect(
                        egui::Rect {
                            min: egui::Pos2::new(viewport.min.x, target_height),
                            max: egui::Pos2::new(viewport.max.x, target_height + spaced_row_height),
                        },
                        Some(egui::Align::Center),
                    );
                }

                // Set the total height for the virtual content
                ui.set_height(spaced_row_height * total_rows as f32);

                // Calculate visible rows with buffer
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

                    let positioned_rect = egui::Rect::from_min_size(
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

        // Render alphabet scroll indicator
        alphabet_scroll::render(
            ui,
            &config.style,
            &mut mini_library_state.alphabet_scroll,
            &ui.min_rect(),
            &logic.get_state().read().unwrap(),
            playing_track_id.as_ref(),
        );

        // Display incremental search query overlay
        incremental_search::post_render(ui, &mini_library_state.incremental_search, &search_results);
    });
}

/// Create mini-library viewport ID
fn mini_library_viewport_id() -> ViewportId {
    ViewportId::from_hash_of("mini_library_window")
}
