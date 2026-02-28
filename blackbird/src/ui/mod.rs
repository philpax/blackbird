use std::sync::Arc;
use std::time::Duration;

mod keys;
mod library;
mod lyrics;
mod playing_track;
mod queue;
mod scrub_bar;
mod search;
mod style;
mod util;

pub use library::GROUP_ALBUM_ART_SIZE;
pub use style::{Style, StyleExt};

use egui::{
    CentralPanel, Color32, Context, FontData, FontDefinitions, FontFamily, Frame, Margin, RichText,
    TextFormat, TopBottomPanel, Visuals, text::LayoutJob,
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
    pub(crate) shared: blackbird_client_shared::lyrics::LyricsState,
    pub(crate) auto_scroll: bool,
}

#[derive(Default)]
pub struct QueueState {
    pub(crate) open: bool,
}

#[derive(Default)]
pub struct UiState {
    pub search: SearchState,
    pub lyrics: LyricsState,
    pub queue: QueueState,
    pub library_view: library::LibraryViewState,
    pub mini_library: library::MiniLibraryState,
}

pub fn initialize(cc: &eframe::CreationContext<'_>, config: &Config) -> UiState {
    cc.egui_ctx.set_visuals(Visuals::dark());
    cc.egui_ctx.style_mut(|style| {
        style.visuals.panel_fill = config.style.background_color32();
        style.visuals.override_text_color = Some(config.style.text_color32());
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

                // Request lyrics if inline lyrics are enabled or the panel is open.
                if self.ui_state.lyrics.shared.on_track_started(
                    &track_and_position.track_id,
                    config.shared.show_inline_lyrics,
                    self.ui_state.lyrics.open,
                ) {
                    self.ui_state.lyrics.auto_scroll = true;
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
                    if self.ui_state.lyrics.open {
                        let playing_id = logic.get_playing_track_id();
                        if self
                            .ui_state
                            .lyrics
                            .shared
                            .on_panel_opened(playing_id.as_ref())
                            && let Some(track_id) = playing_id
                        {
                            logic.request_lyrics(&track_id);
                        }
                        self.ui_state.lyrics.auto_scroll = true;
                    }
                }
            }
        });

        // Handle keyboard shortcuts when no modal is consuming input
        let search_active = self.ui_state.library_view.incremental_search.active;
        let can_handle_shortcuts = !self.ui_state.search.open
            && !self.ui_state.lyrics.open
            && !self.ui_state.queue.open
            && !search_active;

        if can_handle_shortcuts {
            ctx.input(|i| {
                for event in &i.events {
                    let egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } = event
                    else {
                        continue;
                    };
                    // Only handle shortcuts without modifiers (except shift for '*')
                    if modifiers.command || modifiers.alt || modifiers.ctrl {
                        continue;
                    }

                    let Some(action) = keys::library_action(*key, modifiers.shift) else {
                        continue;
                    };
                    match action {
                        keys::Action::PlayPause => logic.toggle_current(),
                        keys::Action::Stop => logic.stop_current(),
                        keys::Action::Next => logic.next(),
                        keys::Action::Previous => logic.previous(),
                        keys::Action::NextGroup => logic.next_group(),
                        keys::Action::PreviousGroup => logic.previous_group(),
                        keys::Action::CyclePlaybackMode => {
                            let next = blackbird_client_shared::next_playback_mode(
                                logic.get_playback_mode(),
                            );
                            logic.set_playback_mode(next);
                        }
                        keys::Action::ToggleSortOrder => {
                            let next =
                                blackbird_client_shared::toggle_sort_order(logic.get_sort_order());
                            logic.set_sort_order(next);
                            self.ui_state.library_view.invalidate_library_scroll();
                            self.ui_state
                                .mini_library
                                .library_view
                                .invalidate_library_scroll();
                            // Re-center on the playing track after re-sorting.
                            if let Some(track_id) = logic.get_playing_track_id() {
                                logic
                                    .get_state()
                                    .write()
                                    .unwrap()
                                    .last_requested_track_for_ui_scroll = Some(track_id);
                            }
                        }
                        keys::Action::SeekBackward => {
                            seek_relative(logic, -blackbird_client_shared::SEEK_STEP_SECS);
                        }
                        keys::Action::SeekForward => {
                            seek_relative(logic, blackbird_client_shared::SEEK_STEP_SECS);
                        }
                        keys::Action::GotoPlaying => {
                            if let Some(track_id) = logic.get_playing_track_id() {
                                let state = logic.get_state();
                                let mut state = state.write().unwrap();
                                state.last_requested_track_for_ui_scroll = Some(track_id);
                            }
                        }
                        keys::Action::SearchInline => {
                            self.ui_state.library_view.incremental_search.active = true;
                        }
                        keys::Action::Lyrics => {
                            self.ui_state.lyrics.open = !self.ui_state.lyrics.open;
                            if self.ui_state.lyrics.open {
                                let playing_id = logic.get_playing_track_id();
                                if self
                                    .ui_state
                                    .lyrics
                                    .shared
                                    .on_panel_opened(playing_id.as_ref())
                                    && let Some(track_id) = playing_id
                                {
                                    logic.request_lyrics(&track_id);
                                }
                                self.ui_state.lyrics.auto_scroll = true;
                            }
                        }
                        keys::Action::Queue => {
                            self.ui_state.queue.open = !self.ui_state.queue.open;
                        }
                        keys::Action::Star => {
                            let Some(track_id) = logic.get_playing_track_id() else {
                                continue;
                            };
                            let state = logic.get_state();
                            let state = state.read().unwrap();
                            let starred = state
                                .library
                                .track_map
                                .get(&track_id)
                                .is_some_and(|t| t.starred);
                            drop(state);
                            logic.set_track_starred(&track_id, !starred);
                        }
                        keys::Action::VolumeUp => {
                            let vol = (logic.get_volume() + blackbird_client_shared::VOLUME_STEP)
                                .min(1.0);
                            logic.set_volume(vol);
                        }
                        keys::Action::VolumeDown => {
                            let vol = (logic.get_volume() - blackbird_client_shared::VOLUME_STEP)
                                .max(0.0);
                            logic.set_volume(vol);
                        }
                    }
                }
            });
        }

        // Process incoming lyrics data.
        while let Ok(lyrics_data) = self.lyrics_loaded_rx.try_recv() {
            self.ui_state.lyrics.shared.on_lyrics_loaded(&lyrics_data);
        }

        // Process library population signal
        while let Ok(()) = self.library_populated_rx.try_recv() {
            self.ui_state.library_view.invalidate_library_scroll();
            self.ui_state
                .mini_library
                .library_view
                .invalidate_library_scroll();

            // Populate the background art prefetch queue with all album cover art IDs.
            let state = logic.get_state();
            let state = state.read().unwrap();
            let ids: Vec<_> = state
                .library
                .groups
                .iter()
                .filter_map(|g| g.cover_art_id.clone())
                .collect();
            drop(state);
            self.cover_art_cache.populate_prefetch_queue(ids);
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
                &mut self.ui_state.lyrics.shared.data,
                &mut self.ui_state.lyrics.shared.loading,
                &mut self.ui_state.lyrics.auto_scroll,
            );
        }

        if self.ui_state.queue.open {
            queue::ui(logic, ctx, &config.style, &mut self.ui_state.queue.open);
        }

        let margin = 8;
        let scroll_margin = 4;
        let has_loaded_all_tracks = logic.has_loaded_all_tracks();

        if self.ui_state.mini_library.open {
            library::mini::ui(
                logic,
                ctx,
                config,
                has_loaded_all_tracks,
                &mut self.cover_art_cache,
                &mut self.ui_state.mini_library,
            );
        }

        // Help bar at the bottom
        TopBottomPanel::bottom("help_bar")
            .frame(
                Frame::default()
                    .inner_margin(Margin::symmetric(8, 4))
                    .fill(config.style.background_color32()),
            )
            .show(ctx, |ui| {
                let highlight_color = config.style.track_name_playing_color32();
                let text_color = Color32::from_rgba_unmultiplied(180, 180, 180, 255);
                let font_id = egui::TextStyle::Body.resolve(ui.style());

                ui.horizontal(|ui| {
                    for entry in keys::LIBRARY_HELP {
                        let Some((key, label)) = entry.help_label(logic) else {
                            continue;
                        };
                        let mut job = LayoutJob::default();

                        job.append(
                            &key,
                            0.0,
                            TextFormat {
                                color: highlight_color,
                                font_id: font_id.clone(),
                                ..Default::default()
                            },
                        );
                        job.append(
                            &format!(":{label}"),
                            0.0,
                            TextFormat {
                                color: text_color,
                                font_id: font_id.clone(),
                                ..Default::default()
                            },
                        );
                        ui.label(job);
                    }
                });
            });

        CentralPanel::default()
            .frame(
                Frame::default()
                    .inner_margin(Margin {
                        left: margin,
                        right: scroll_margin,
                        top: margin,
                        bottom: margin,
                    })
                    .fill(config.style.background_color32()),
            )
            .show(ctx, |ui| {
                if let Some(id) = library::shared::render_player_controls(
                    ui,
                    logic,
                    config,
                    has_loaded_all_tracks,
                    &mut self.cover_art_cache,
                ) {
                    track_to_scroll_to = Some(id);
                }

                library::full::ui(
                    ui,
                    logic,
                    config,
                    has_loaded_all_tracks,
                    scroll_margin.into(),
                    track_to_scroll_to.as_ref(),
                    &mut self.cover_art_cache,
                    &mut self.ui_state.library_view,
                    &library::full::FullLibraryState {
                        search_open: self.ui_state.search.open,
                        lyrics_open: self.ui_state.lyrics.open,
                        queue_open: self.ui_state.queue.open,
                    },
                );
            });

        // Draw inline lyrics as an overlay at the bottom of the central panel.
        if config.shared.show_inline_lyrics && self.ui_state.lyrics.shared.has_synced_lyrics() {
            let panel_rect = ctx.available_rect();
            let font_id = egui::TextStyle::Body.resolve(&ctx.style());
            let row_height = ctx.fonts(|f| f.row_height(&font_id));
            // Height: separator (2px) + text row + small padding.
            let overlay_height = row_height + 6.0;
            let overlay_rect = egui::Rect::from_min_size(
                egui::pos2(panel_rect.left(), panel_rect.bottom() - overlay_height),
                egui::vec2(panel_rect.width(), overlay_height),
            );

            egui::Area::new(egui::Id::new("inline_lyrics_overlay"))
                .fixed_pos(overlay_rect.min)
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    ui.set_min_size(overlay_rect.size());
                    ui.set_max_size(overlay_rect.size());

                    // Fill background so library content doesn't bleed through.
                    ui.painter()
                        .rect_filled(overlay_rect, 0.0, config.style.background_color32());

                    // Top separator line.
                    let sep_rect = egui::Rect::from_min_size(
                        overlay_rect.min,
                        egui::vec2(overlay_rect.width(), 1.0),
                    );
                    ui.painter()
                        .rect_filled(sep_rect, 0.0, config.style.track_duration_color32());

                    let position = logic.get_playing_position();
                    let mut job = egui::text::LayoutJob::default();
                    if let Some(line) = self.ui_state.lyrics.shared.current_inline_line(position) {
                        if let Some(start_ms) = line.start {
                            let timestamp_secs = (start_ms / 1000) as u32;
                            let timestamp_str =
                                blackbird_core::util::seconds_to_hms_string(timestamp_secs, false);
                            job.append(
                                &format!("{timestamp_str} "),
                                0.0,
                                egui::text::TextFormat {
                                    color: config.style.track_name_playing_color32(),
                                    font_id: font_id.clone(),
                                    ..Default::default()
                                },
                            );
                        }
                        job.append(
                            &line.value,
                            0.0,
                            egui::text::TextFormat {
                                color: config.style.text_color32(),
                                font_id,
                                ..Default::default()
                            },
                        );
                    } else {
                        let [r, g, b, a] = config.style.text_color32().to_array();
                        job.append(
                            "[no lyrics]",
                            0.0,
                            egui::text::TextFormat {
                                color: egui::Color32::from_rgba_unmultiplied(
                                    (r as f32 * 0.5) as u8,
                                    (g as f32 * 0.5) as u8,
                                    (b as f32 * 0.5) as u8,
                                    a,
                                ),
                                font_id,
                                ..Default::default()
                            },
                        );
                    }
                    // Add some left margin and vertical centering.
                    let text_pos = egui::pos2(
                        overlay_rect.left() + margin as f32,
                        overlay_rect.top() + 3.0,
                    );
                    ui.painter().galley(
                        text_pos,
                        ui.fonts(|f| f.layout_job(job)),
                        config.style.text_color32(),
                    );
                });
        }

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

/// Seek relative to the current position by the given number of seconds.
fn seek_relative(logic: &mut bc::Logic, seconds: i64) {
    let Some(details) = logic.get_track_display_details() else {
        return;
    };
    let current = details.track_position;
    let delta = Duration::from_secs(seconds.unsigned_abs());
    let new_pos = if seconds > 0 {
        current + delta
    } else {
        current.saturating_sub(delta)
    };
    logic.seek_current(new_pos);
}
