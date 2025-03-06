use crate::{bs, config::Config, logic::Logic, style};

pub struct Ui {
    config: Config,
    last_config_update: std::time::Instant,
    logic: Logic,
}
impl Ui {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let config = Config::load();
        config.save();

        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        cc.egui_ctx.style_mut(|style| {
            style.visuals.panel_fill = config.style.background();
            style.visuals.override_text_color = Some(config.style.text());
        });

        let mut fonts = egui::FontDefinitions::default();
        egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);

        cc.egui_ctx.set_fonts(fonts);

        egui_extras::install_image_loaders(&cc.egui_ctx);

        let logic = Logic::new(
            bs::Client::new(
                config.general.base_url.clone(),
                config.general.username.clone(),
                config.general.password.clone(),
                "blackbird".to_string(),
            ),
            cc.egui_ctx.clone(),
        );

        Ui {
            config,
            last_config_update: std::time::Instant::now(),
            logic,
        }
    }

    fn poll_for_config_updates(&mut self) {
        if self.last_config_update.elapsed() > std::time::Duration::from_secs(1) {
            let new_config = Config::load();
            if new_config != self.config {
                self.config = new_config;
                self.config.save();
            }
            self.last_config_update = std::time::Instant::now();
        }
    }
}
impl eframe::App for Ui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_for_config_updates();

        if let Some(error) = self.logic.get_error() {
            let mut open = true;
            egui::Window::new("Error").open(&mut open).show(ctx, |ui| {
                ui.label(&error);
            });
            if !open {
                self.logic.clear_error();
            }
        }

        let margin = 8;
        let scroll_margin = 4;
        egui::CentralPanel::default()
            .frame(
                egui::Frame::default()
                    .inner_margin(egui::Margin {
                        left: margin,
                        right: scroll_margin,
                        top: margin,
                        bottom: margin,
                    })
                    .fill(self.config.style.background()),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.vertical(|ui| {
                            ui.style_mut().spacing.item_spacing = egui::Vec2::ZERO;
                            if let Some(pi) = self.logic.get_playing_info() {
                                ui.horizontal(|ui| {
                                    if let Some(artist) =
                                        pi.song_artist.as_ref().filter(|a| **a != pi.album_artist)
                                    {
                                        ui.add(
                                            egui::Label::new(
                                                egui::RichText::new(artist)
                                                    .color(style::string_to_colour(artist)),
                                            )
                                            .selectable(false),
                                        );
                                        ui.add(egui::Label::new(" - ").selectable(false));
                                    }
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(&pi.song_title)
                                                .color(self.config.style.track_name_playing()),
                                        )
                                        .selectable(false),
                                    );
                                });
                                ui.horizontal(|ui| {
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(&pi.album_name)
                                                .color(self.config.style.album()),
                                        )
                                        .selectable(false),
                                    );
                                    ui.add(egui::Label::new(" by ").selectable(false));
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(&pi.album_artist)
                                                .color(style::string_to_colour(&pi.album_artist)),
                                        )
                                        .selectable(false),
                                    );
                                });
                            } else {
                                ui.horizontal(|ui| {
                                    let percent_loaded = self.logic.get_loaded_0_to_1();
                                    ui.add(
                                        egui::Label::new(format!(
                                            "Nothing playing | {:0.1}% loaded",
                                            percent_loaded * 100.0
                                        ))
                                        .selectable(false),
                                    );
                                });
                                ui.horizontal(|ui| {
                                    ui.add(
                                        egui::Label::new("Double-click a song to play it!")
                                            .selectable(false),
                                    );
                                });
                            }
                        });
                    });

                    if self.logic.is_song_loaded() {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.style_mut().visuals.override_text_color = None;
                            if ui
                                .add(
                                    egui::Label::new(
                                        egui::RichText::new(egui_phosphor::regular::STOP)
                                            .size(32.0),
                                    )
                                    .selectable(false)
                                    .sense(egui::Sense::click()),
                                )
                                .clicked()
                            {
                                self.logic.stop_playback();
                            }
                            if ui
                                .add(
                                    egui::Label::new(
                                        egui::RichText::new(egui_phosphor::regular::PLAY_PAUSE)
                                            .size(32.0),
                                    )
                                    .selectable(false)
                                    .sense(egui::Sense::click()),
                                )
                                .clicked()
                            {
                                self.logic.toggle_playback();
                            }
                        });
                    }
                });

                ui.separator();

                ui.scope(|ui| {
                    // Make the scroll bar solid, and hide its background. Ideally, we'd set the opacity
                    // to 0, but egui doesn't allow that for solid scroll bars.
                    ui.style_mut().spacing.scroll = egui::style::ScrollStyle {
                        bar_inner_margin: scroll_margin.into(),
                        ..egui::style::ScrollStyle::solid()
                    };
                    ui.style_mut().visuals.extreme_bg_color = self.config.style.background();

                    let row_height = ui.text_style_height(&egui::TextStyle::Body);
                    let album_margin_bottom_row_count = 1;

                    // Get album data for rendering
                    let num_rows = self
                        .logic
                        .calculate_total_rows(album_margin_bottom_row_count);

                    egui::ScrollArea::vertical().auto_shrink(false).show_rows(
                        ui,
                        row_height,
                        num_rows,
                        |ui, visible_row_range| {
                            // Calculate which albums are in view
                            let visible_albums = self.logic.get_visible_albums(
                                visible_row_range.clone(),
                                album_margin_bottom_row_count,
                            );

                            let playing_song_id = self.logic.get_playing_song_id();

                            let mut current_row = visible_albums.start_row;

                            for album in visible_albums.albums {
                                let album_lines =
                                    album.line_count() + album_margin_bottom_row_count;

                                // If the album needs to be loaded
                                if album.songs.is_none() {
                                    self.logic.fetch_album(&album.id);
                                }

                                // Handle cover art if enabled
                                if self.config.general.album_art_enabled {
                                    if let Some(cover_art_id) = &album.cover_art_id {
                                        if !self.logic.has_cover_art(cover_art_id) {
                                            self.logic.fetch_cover_art(cover_art_id);
                                        }
                                    }
                                }

                                // Compute the visible portion of the album's rows, rebased to the album
                                let local_start =
                                    visible_row_range.start.saturating_sub(current_row);
                                let local_end = visible_row_range
                                    .end
                                    .saturating_sub(current_row)
                                    .min(album_lines - album_margin_bottom_row_count);

                                // Ensure we have a valid range (start <= end)
                                let local_visible_range = local_start..local_end.max(local_start);

                                // Get cover art if needed
                                let cover_art = if self.config.general.album_art_enabled {
                                    album
                                        .cover_art_id
                                        .as_deref()
                                        .and_then(|id| self.logic.get_cover_art(id))
                                } else {
                                    None
                                };

                                // Display the album
                                let clicked_song_id = album.ui(
                                    ui,
                                    &self.config.style,
                                    local_visible_range,
                                    cover_art,
                                    self.config.general.album_art_enabled,
                                    &self.logic.get_song_map(),
                                    playing_song_id.as_ref(),
                                );

                                // Handle song selection
                                if let Some(song_id) = clicked_song_id {
                                    self.logic.play_song(song_id);
                                }

                                ui.add_space(row_height * album_margin_bottom_row_count as f32);
                                current_row += album_lines;
                            }
                        },
                    );
                });
            });
    }
}
