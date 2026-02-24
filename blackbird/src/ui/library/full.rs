//! Full library view (main window)

use blackbird_core::blackbird_state::TrackId;
use egui::Ui;

use crate::{bc, config::Config, cover_art_cache::CoverArtCache};

use super::shared::{LibraryViewConfig, render_library_view};

/// UI state specific to the full library view
pub struct FullLibraryState {
    pub search_open: bool,
    pub lyrics_open: bool,
    pub queue_open: bool,
}

/// Main library UI
#[allow(clippy::too_many_arguments)]
pub fn ui(
    ui: &mut Ui,
    logic: &mut bc::Logic,
    config: &Config,
    has_loaded_all_tracks: bool,
    scroll_margin: f32,
    track_to_scroll_to: Option<&TrackId>,
    cover_art_cache: &mut CoverArtCache,
    view_state: &mut super::shared::LibraryViewState,
    ui_state: &FullLibraryState,
) {
    // Only capture keyboard input if search modal and lyrics window are not open
    let can_handle_incremental_search =
        !ui_state.search_open && !ui_state.lyrics_open && !ui_state.queue_open;

    render_library_view(
        ui,
        logic,
        config,
        has_loaded_all_tracks,
        scroll_margin,
        cover_art_cache,
        view_state,
        LibraryViewConfig {
            scroll_target: track_to_scroll_to,
            auto_scroll_to_playing: false,
            incremental_search_enabled: can_handle_incremental_search,
        },
    );
}
