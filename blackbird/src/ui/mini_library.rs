use egui::{CentralPanel, Context, Frame, Key, Margin, ViewportBuilder, ViewportId};

use crate::{bc, config::Config, cover_art_cache::CoverArtCache};

use super::{MiniLibraryState, library::{self, LibraryViewConfig}};

/// Height of the mini-library as a fraction of screen height
const MINI_LIBRARY_HEIGHT_FRACTION: f32 = 0.4;

/// Main mini-library window UI
pub fn ui(
    logic: &mut bc::Logic,
    ctx: &Context,
    config: &Config,
    has_loaded_all_tracks: bool,
    cover_art_cache: &mut CoverArtCache,
    state: &mut MiniLibraryState,
) {
    if !state.open {
        ctx.send_viewport_cmd_to(mini_library_viewport_id(), egui::ViewportCommand::Close);
        return;
    }

    let screen_rect = ctx.screen_rect();
    let window_height = screen_rect.height() * MINI_LIBRARY_HEIGHT_FRACTION;
    let window_width = screen_rect.width() * 0.5;

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
                    if ui.input(|i| i.key_pressed(Key::Escape)) {
                        close_window = true;
                    }
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

                    // Playback controls
                    let mut track_to_scroll_to = None;
                    super::playing_track::ui(
                        ui,
                        logic,
                        config,
                        has_loaded_all_tracks,
                        &mut track_to_scroll_to,
                        cover_art_cache,
                    );

                    super::scrub_bar::ui(ui, logic, config);
                    ui.separator();

                    // Library view (auto-scrolls to playing track)
                    library::render_library_view(
                        ui,
                        logic,
                        config,
                        has_loaded_all_tracks,
                        scroll_margin.into(),
                        cover_art_cache,
                        &mut state.library_view,
                        LibraryViewConfig {
                            scroll_target: None,
                            auto_scroll_to_playing: true,
                            incremental_search_enabled: true,
                        },
                    );
                });
        },
    );

    if close_window {
        state.open = false;
    }
}

fn mini_library_viewport_id() -> ViewportId {
    ViewportId::from_hash_of("mini_library_window")
}
