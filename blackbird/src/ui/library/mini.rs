//! Mini library view (popup window)

use egui::{CentralPanel, Context, Frame, Key, Margin, ViewportBuilder, ViewportId};

use crate::{bc, config::Config, cover_art_cache::CoverArtCache};

use super::shared::{LibraryViewConfig, LibraryViewState, render_library_view, render_player_controls};

/// Height of the mini-library as a fraction of screen height
const HEIGHT_FRACTION: f32 = 0.4;

/// State for the mini-library window
#[derive(Default)]
pub struct MiniLibraryState {
    pub open: bool,
    pub(crate) library_view: LibraryViewState,
}

/// Mini-library window UI
pub fn ui(
    logic: &mut bc::Logic,
    ctx: &Context,
    config: &Config,
    has_loaded_all_tracks: bool,
    cover_art_cache: &mut CoverArtCache,
    state: &mut MiniLibraryState,
) {
    if !state.open {
        ctx.send_viewport_cmd_to(viewport_id(), egui::ViewportCommand::Close);
        return;
    }

    let screen_rect = ctx.screen_rect();
    let window_height = screen_rect.height() * HEIGHT_FRACTION;
    let window_width = screen_rect.width() * 0.5;

    let mut close_window = false;

    ctx.show_viewport_immediate(
        viewport_id(),
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
                    if ui.input(|i| i.key_pressed(Key::Escape))
                        || ctx.input(|i| i.viewport().close_requested())
                    {
                        close_window = true;
                    }

                    render_player_controls(ui, logic, config, has_loaded_all_tracks, cover_art_cache);

                    render_library_view(
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

fn viewport_id() -> ViewportId {
    ViewportId::from_hash_of("mini_library_window")
}
