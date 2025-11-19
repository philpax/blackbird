use std::sync::Arc;
use std::time::Instant;

mod library;
mod lyrics;
mod playing_track;
mod scrub_bar;
mod search;
mod style;
mod util;

pub use library::GROUP_ALBUM_ART_SIZE;
pub use style::Style;

use egui::{
    CentralPanel, Context, FontData, FontDefinitions, FontFamily, Frame, Margin, RichText, Visuals,
};

use crate::{App, bc, config::Config};

#[derive(Default)]
pub struct SearchState {
    pub(crate) open: bool,
    pub(crate) query: String,
}

#[derive(Default)]
pub struct LyricsState {
    pub(crate) open: bool,
    pub(crate) track_id: Option<bc::blackbird_state::TrackId>,
    pub(crate) data: Option<bc::bs::StructuredLyrics>,
    pub(crate) loading: bool,
    pub(crate) auto_scroll: bool,
}

#[derive(Default)]
pub struct IncrementalSearchState {
    pub(crate) query: String,
    pub(crate) last_input: Option<Instant>,
    pub(crate) result_index: usize,
}

#[derive(Default)]
pub struct AlphabetScrollState {
    pub(crate) positions: Vec<(char, f32)>, // (letter, position fraction 0.0-1.0)
    pub(crate) needs_update: bool,
}

#[derive(Default)]
pub struct UiState {
    pub search: SearchState,
    pub lyrics: LyricsState,
    pub incremental_search: IncrementalSearchState,
    pub alphabet_scroll: AlphabetScrollState,
}

pub fn initialize(cc: &eframe::CreationContext<'_>, config: &Config) -> UiState {
    cc.egui_ctx.set_visuals(Visuals::dark());
    cc.egui_ctx.style_mut(|style| {
        style.visuals.panel_fill = config.style.background();
        style.visuals.override_text_color = Some(config.style.text());
        style.scroll_animation = egui::style::ScrollAnimation::duration(0.2);
    });
    cc.egui_ctx.options_mut(|options| {
        options.input_options.line_scroll_speed = config.style.scroll_multiplier
    });

    let mut fonts = FontDefinitions::default();
    // Replace the default font with GoNoto
    fonts.font_data.insert(
        "GoNoto".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../assets/GoNotoKurrent-Regular.ttf"
        ))),
    );
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .push("GoNoto".into());

    // Add Phosphor regular icons as fallback
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);

    // Add Phosphor fill as an explicit font
    fonts.font_data.insert(
        "phosphor-fill".into(),
        Arc::new(egui_phosphor::Variant::Fill.font_data()),
    );
    fonts.families.insert(
        egui::FontFamily::Name("phosphor-fill".into()),
        vec!["GoNoto".into(), "phosphor-fill".into()],
    );

    cc.egui_ctx.set_fonts(fonts);

    egui_extras::install_image_loaders(&cc.egui_ctx);

    UiState::default()
}

impl App {
    pub fn render(&mut self, ctx: &Context) {
        let logic = &mut self.logic;
        let config = &self.config.read().unwrap();

        let mut track_to_scroll_to = logic
            .get_state()
            .write()
            .unwrap()
            .last_requested_track_for_ui_scroll
            .take();
        while let Ok(event) = self.playback_to_logic_rx.try_recv() {
            if let bc::PlaybackToLogicMessage::TrackStarted(track_and_position) = event {
                track_to_scroll_to = Some(track_and_position.track_id.clone());

                // If lyrics window is open, request lyrics for the new track
                if self.ui_state.lyrics.open {
                    self.ui_state.lyrics.track_id = Some(track_and_position.track_id.clone());
                    self.ui_state.lyrics.loading = true;
                    self.ui_state.lyrics.data = None; // Clear old lyrics while loading
                    self.ui_state.lyrics.auto_scroll = true; // Re-enable auto-scroll for new track
                    logic.request_lyrics(&track_and_position.track_id);
                }
            }
        }

        if let Some(error) = logic.get_error() {
            let mut open = true;
            egui::Window::new("Error").open(&mut open).show(ctx, |ui| {
                ui.label(RichText::new(error.display_name()).heading());
                ui.label(RichText::new(
                    error.display_message(&logic.get_state().read().unwrap()),
                ));
            });
            if !open {
                logic.clear_error();
            }
        }

        ctx.input(|i| {
            // Handle local search keybinding
            if let Some(search_key) = config
                .keybindings
                .parse_local_key(&config.keybindings.local_search)
            {
                let requires_cmd = config
                    .keybindings
                    .requires_command(&config.keybindings.local_search);
                if (!requires_cmd || i.modifiers.command) && i.key_released(search_key) {
                    self.ui_state.search.open = !self.ui_state.search.open;
                }
            }

            // Handle local lyrics keybinding
            if let Some(lyrics_key) = config
                .keybindings
                .parse_local_key(&config.keybindings.local_lyrics)
            {
                let requires_cmd = config
                    .keybindings
                    .requires_command(&config.keybindings.local_lyrics);
                if (!requires_cmd || i.modifiers.command) && i.key_released(lyrics_key) {
                    self.ui_state.lyrics.open = !self.ui_state.lyrics.open;
                    // Request lyrics for the currently playing track when opening the window
                    if self.ui_state.lyrics.open
                        && let Some(track_id) = logic.get_playing_track_id()
                    {
                        self.ui_state.lyrics.track_id = Some(track_id.clone());
                        self.ui_state.lyrics.loading = true;
                        self.ui_state.lyrics.auto_scroll = true; // Enable auto-scroll by default
                        logic.request_lyrics(&track_id);
                    }
                }
            }
        });

        // Process incoming lyrics data
        while let Ok(lyrics_data) = self.lyrics_loaded_rx.try_recv() {
            if Some(&lyrics_data.track_id) == self.ui_state.lyrics.track_id.as_ref() {
                self.ui_state.lyrics.data = lyrics_data.lyrics;
                self.ui_state.lyrics.loading = false;
            }
        }

        // Process library population signal
        while let Ok(()) = self.library_populated_rx.try_recv() {
            self.ui_state.alphabet_scroll.needs_update = true;
        }

        if self.ui_state.search.open {
            search::ui(
                logic,
                ctx,
                &config.style,
                &mut self.ui_state.search.open,
                &mut self.ui_state.search.query,
            );
        }

        if self.ui_state.lyrics.open {
            lyrics::ui(
                logic,
                ctx,
                &config.style,
                &mut self.ui_state.lyrics.open,
                &mut self.ui_state.lyrics.data,
                &mut self.ui_state.lyrics.loading,
                &mut self.ui_state.lyrics.auto_scroll,
            );
        }

        let margin = 8;
        let scroll_margin = 4;
        let has_loaded_all_tracks = logic.has_loaded_all_tracks();
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
                ui.input(|i| {
                    // Handle mouse button for previous track
                    if let Some(button) = config
                        .keybindings
                        .parse_mouse_button(&config.keybindings.mouse_previous_track)
                        && i.pointer.button_released(button)
                    {
                        logic.previous();
                    }

                    // Handle mouse button for next track
                    if let Some(button) = config
                        .keybindings
                        .parse_mouse_button(&config.keybindings.mouse_next_track)
                        && i.pointer.button_released(button)
                    {
                        logic.next();
                    }
                });

                playing_track::ui(
                    ui,
                    logic,
                    config,
                    has_loaded_all_tracks,
                    &mut track_to_scroll_to,
                    &mut self.cover_art_cache,
                );
                scrub_bar::ui(ui, logic, config);

                ui.separator();

                library::ui(
                    ui,
                    logic,
                    config,
                    has_loaded_all_tracks,
                    scroll_margin.into(),
                    track_to_scroll_to.as_ref(),
                    &mut self.cover_art_cache,
                    &mut self.ui_state,
                );
            });

        // If the track-to-scroll-to doesn't exist yet in the library, save it back
        // and it will hopefully become available at some point in the future
        if let Some(track_id) = track_to_scroll_to {
            let state = logic.get_state();
            let mut state = state.write().unwrap();
            if !state.library.track_map.contains_key(&track_id) {
                state.last_requested_track_for_ui_scroll = Some(track_id);
            }
        }
    }
}
