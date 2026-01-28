//! Mini library view (popup window)

use blackbird_core::blackbird_state::TrackId;
use egui::{CentralPanel, Context, Frame, Key, Margin, ViewportId, vec2};

use crate::{bc, config::Config, cover_art_cache::CoverArtCache, ui::util::global_window_builder};

use super::shared::{
    LibraryViewConfig, LibraryViewState, render_library_view, render_player_controls,
};

/// Height of the mini-library as a fraction of main window height
const HEIGHT_FRACTION: f32 = 0.4;

/// State for the mini-library window
#[derive(Default)]
pub struct MiniLibraryState {
    pub open: bool,
    pub(crate) library_view: LibraryViewState,
    /// Track to scroll to when window opens (set on open, cleared after scroll)
    scroll_to_track: Option<TrackId>,
}

impl MiniLibraryState {
    /// Call when opening the window to scroll to the currently playing track
    pub fn open_with_playing_track(&mut self, playing_track: Option<TrackId>) {
        self.open = true;
        self.scroll_to_track = playing_track;
    }
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

    // Use main window dimensions from config
    let window_size = vec2(
        config.general.window_width as f32,
        config.general.window_height as f32 * HEIGHT_FRACTION,
    );

    let viewport_builder =
        global_window_builder(ctx, window_size).with_title("blackbird: mini library");

    let mut close_window = false;

    ctx.show_viewport_immediate(viewport_id(), viewport_builder, |ctx, _class| {
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

                // Take the scroll target (only scrolls once)
                let scroll_target = state.scroll_to_track.take();

                render_library_view(
                    ui,
                    logic,
                    config,
                    has_loaded_all_tracks,
                    scroll_margin.into(),
                    cover_art_cache,
                    &mut state.library_view,
                    LibraryViewConfig {
                        scroll_target: scroll_target.as_ref(),
                        auto_scroll_to_playing: false,
                        incremental_search_enabled: true,
                    },
                );
            });
    });

    if close_window {
        state.open = false;
    }
}

fn viewport_id() -> ViewportId {
    ViewportId::from_hash_of("mini_library_window")
}
