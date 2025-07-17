use std::sync::{Arc, RwLock};
use std::time::Duration;

mod group;
mod song;
mod style;
mod util;

use blackbird_core::util::seconds_to_hms_string;
use blackbird_core::PlaybackMode;
pub use style::Style;

use crate::{bc, config::Config, media_controls};

// UI Constants
const CONTROL_BUTTON_SIZE: f32 = 28.0;

pub struct Ui {
    config: Arc<RwLock<Config>>,
    _config_reload_thread: std::thread::JoinHandle<()>,
    _repaint_thread: std::thread::JoinHandle<()>,
    logic: Arc<bc::Logic>,
    current_window_size: Option<egui::Rect>,
}

impl Ui {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        config: Arc<RwLock<Config>>,
        logic: Arc<bc::Logic>,
    ) -> Self {
        {
            let config_read = config.read().unwrap();

            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            cc.egui_ctx.style_mut(|style| {
                style.visuals.panel_fill = config_read.style.background();
                style.visuals.override_text_color = Some(config_read.style.text());
            });
            cc.egui_ctx.options_mut(|options| {
                options.input_options.line_scroll_speed = config_read.style.scroll_multiplier
            });
        }

        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "GoNoto".into(),
            Arc::new(egui::FontData::from_static(include_bytes!(
                "../../assets/GoNotoCurrent-Regular.ttf"
            ))),
        );
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .push("GoNoto".into());
        egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);

        cc.egui_ctx.set_fonts(fonts);

        egui_extras::install_image_loaders(&cc.egui_ctx);

        let _config_reload_thread = std::thread::spawn({
            let config = config.clone();
            let egui_ctx = cc.egui_ctx.clone();
            move || loop {
                std::thread::sleep(std::time::Duration::from_secs(1));

                let new_config = Config::load();
                let current_config = config.read().unwrap();
                if new_config != *current_config {
                    drop(current_config);
                    *config.write().unwrap() = new_config;
                    config.read().unwrap().save();
                    egui_ctx.request_repaint();
                }
            }
        });

        let _repaint_thread = std::thread::spawn({
            let egui_ctx = cc.egui_ctx.clone();
            let logic = logic.clone();
            move || loop {
                std::thread::sleep(std::time::Duration::from_millis(500));
                // Only repaint if music is currently playing
                if logic.get_playing_info().is_some() {
                    egui_ctx.request_repaint();
                }
            }
        });

        match media_controls::setup(logic.clone(), Some(cc)) {
            Ok(()) => {
                tracing::info!("Media controls initialized successfully");
            }
            Err(e) => {
                tracing::warn!("Failed to initialize media controls: {:?}", e);
            }
        }

        Ui {
            config,
            _config_reload_thread,
            _repaint_thread,
            logic,
            current_window_size: None,
        }
    }
}

impl eframe::App for Ui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let config_read = self.config.read().unwrap();

        // Update current window size
        ctx.input(|i| {
            if let Some(inner_rect) = i.viewport().inner_rect {
                self.current_window_size = Some(inner_rect);
            }
        });

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
        let has_loaded_all_songs = self.logic.has_loaded_all_songs();
        egui::CentralPanel::default()
            .frame(
                egui::Frame::default()
                    .inner_margin(egui::Margin {
                        left: margin,
                        right: scroll_margin,
                        top: margin,
                        bottom: margin,
                    })
                    .fill(config_read.style.background()),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.style_mut().spacing.item_spacing = egui::Vec2::ZERO;
                        ui.horizontal(|ui| {
                            if self.logic.is_song_loading() {
                                ui.add(egui::Spinner::new());
                                ui.add_space(16.0);
                            }

                            if let Some(pi) = self.logic.get_playing_info() {
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        if let Some(artist) = pi
                                            .song_artist
                                            .as_ref()
                                            .filter(|a| **a != pi.album_artist)
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
                                                    .color(config_read.style.track_name_playing()),
                                            )
                                            .selectable(false),
                                        );
                                    });
                                    ui.horizontal(|ui| {
                                        ui.add(
                                            egui::Label::new(
                                                egui::RichText::new(&pi.album_name)
                                                    .color(config_read.style.album()),
                                            )
                                            .selectable(false),
                                        );
                                        ui.add(egui::Label::new(" by ").selectable(false));
                                        ui.add(
                                            egui::Label::new(
                                                egui::RichText::new(&pi.album_artist).color(
                                                    style::string_to_colour(&pi.album_artist),
                                                ),
                                            )
                                            .selectable(false),
                                        );
                                    });
                                });
                            } else {
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        let song_count = self.logic.get_song_map().len();
                                        ui.add(
                                            egui::Label::new(format!(
                                                "Nothing playing | {}{} songs",
                                                if has_loaded_all_songs {
                                                    ""
                                                } else {
                                                    "Loading songs... | "
                                                },
                                                song_count,
                                            ))
                                            .selectable(false),
                                        );
                                    });
                                    ui.horizontal(|ui| {
                                        ui.add(
                                            egui::Label::new("Click a song to play it!")
                                                .selectable(false),
                                        );
                                    });
                                });
                            }
                        });
                    });

                    if self.logic.is_song_loaded() {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.style_mut().visuals.override_text_color = None;

                            let default_color = config_read.style.text();
                            let active_color = config_read.style.track_name_playing();

                            // Next track button
                            if control_button(
                                ui,
                                egui_phosphor::regular::SKIP_FORWARD,
                                default_color,
                                active_color,
                            ) {
                                self.logic.next_track();
                            }

                            // Play/Pause button
                            if control_button(
                                ui,
                                egui_phosphor::regular::PLAY_PAUSE,
                                default_color,
                                active_color,
                            ) {
                                self.logic.toggle_playback();
                            }

                            // Previous track button
                            if control_button(
                                ui,
                                egui_phosphor::regular::SKIP_BACK,
                                default_color,
                                active_color,
                            ) {
                                self.logic.previous_track();
                            }

                            // Stop button
                            if control_button(
                                ui,
                                egui_phosphor::regular::STOP,
                                default_color,
                                active_color,
                            ) {
                                self.logic.stop_playback();
                            }

                            ui.add_space(24.0);

                            // Playback mode buttons (Sequential, Shuffle, Repeat One)
                            let current_mode = self.logic.get_playback_mode();

                            // Sequential button
                            if control_button(
                                ui,
                                egui_phosphor::regular::LIST,
                                if current_mode == PlaybackMode::Sequential {
                                    active_color
                                } else {
                                    default_color
                                },
                                active_color,
                            ) {
                                self.logic.set_playback_mode(PlaybackMode::Sequential);
                            }

                            // Shuffle button
                            if control_button(
                                ui,
                                egui_phosphor::regular::SHUFFLE,
                                if current_mode == PlaybackMode::Shuffle {
                                    active_color
                                } else {
                                    default_color
                                },
                                active_color,
                            ) {
                                self.logic.set_playback_mode(PlaybackMode::Shuffle);
                            }

                            // Repeat One button
                            if control_button(
                                ui,
                                egui_phosphor::regular::REPEAT_ONCE,
                                if current_mode == PlaybackMode::RepeatOne {
                                    active_color
                                } else {
                                    default_color
                                },
                                active_color,
                            ) {
                                self.logic.set_playback_mode(PlaybackMode::RepeatOne);
                            }
                        });
                    }
                });

                ui.horizontal(|ui| {
                    let (position_secs, duration_secs) = self
                        .logic
                        .get_playing_info()
                        .map(|pi| {
                            (
                                pi.song_position.as_secs_f32(),
                                pi.song_duration.as_secs_f32(),
                            )
                        })
                        .unwrap_or_default();

                    // Add position/duration text
                    let [position_hms, duration_hms] = [position_secs, duration_secs]
                        .map(|s| seconds_to_hms_string(s as u32, true));
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(format!("{position_hms} / {duration_hms}"))
                                .color(config_read.style.track_duration()),
                        )
                        .selectable(false),
                    );

                    // Convert durations to seconds for the slider
                    let mut slider_position = position_secs;
                    let slider_duration = duration_secs.max(1.0);

                    // Add a slider for scrubbing - takes up available horizontal space
                    ui.style_mut().spacing.slider_width =
                        ui.available_width() - ui.style().spacing.window_margin.right as f32;
                    let slider_response = ui.add(
                        egui::Slider::new(&mut slider_position, 0.0..=slider_duration)
                            .show_value(false)
                            .handle_shape(egui::style::HandleShape::Rect { aspect_ratio: 2.0 }),
                    );

                    // If the user interacted with the slider, seek to that position
                    if slider_response.changed() {
                        let seek_position = Duration::from_secs_f32(slider_position);
                        self.logic.seek(seek_position);
                    }
                });

                ui.separator();

                ui.scope(|ui| {
                    if !has_loaded_all_songs {
                        ui.add_sized(ui.available_size(), egui::Spinner::new());
                        return;
                    }

                    // Make the scroll bar solid, and hide its background. Ideally, we'd set the opacity
                    // to 0, but egui doesn't allow that for solid scroll bars.
                    ui.style_mut().spacing.scroll = egui::style::ScrollStyle {
                        bar_inner_margin: scroll_margin.into(),
                        bar_width: 12.0,
                        handle_min_length: 24.0,
                        ..egui::style::ScrollStyle::solid()
                    };
                    ui.style_mut().visuals.extreme_bg_color = config_read.style.background();

                    let spaced_row_height = util::spaced_row_height(ui);
                    let group_margin_bottom_row_count = 1;

                    // Get total rows for virtual rendering
                    let total_rows = self
                        .logic
                        .calculate_total_rows(group_margin_bottom_row_count, group::line_count);

                    // Use custom virtual rendering for better performance
                    egui::ScrollArea::vertical()
                        .auto_shrink(false)
                        .show_viewport(ui, |ui, viewport| {
                            // Set the total height for the virtual content (with spacing)
                            ui.set_height(spaced_row_height * total_rows as f32);

                            // Calculate which rows are visible with some buffer
                            let first_visible_row =
                                ((viewport.min.y / spaced_row_height).floor().max(0.0)) as usize;
                            let last_visible_row =
                                (viewport.max.y / spaced_row_height).ceil() as usize + 5; // Add buffer
                            let last_visible_row = last_visible_row.min(total_rows);

                            if first_visible_row >= last_visible_row {
                                return;
                            }

                            let visible_row_range = first_visible_row..last_visible_row;

                            // Calculate which groups are in view
                            let visible_groups = self.logic.get_visible_groups(
                                visible_row_range.clone(),
                                group_margin_bottom_row_count,
                                group::line_count,
                            );

                            let playing_song_id = self.logic.get_playing_song_id();
                            let mut current_row = visible_groups.start_row;

                            for group in visible_groups.groups {
                                let group_lines =
                                    group::line_count(&group) + group_margin_bottom_row_count;

                                // Handle cover art if enabled
                                if config_read.general.album_art_enabled {
                                    if let Some(cover_art_id) = &group.cover_art_id {
                                        if !self.logic.has_cover_art(cover_art_id) {
                                            self.logic.fetch_cover_art(cover_art_id);
                                        }
                                    }
                                }

                                // Get cover art if needed
                                let cover_art = if config_read.general.album_art_enabled {
                                    group.cover_art_id.as_deref().and_then(|id| {
                                        Some((id.to_string(), self.logic.get_cover_art(id)?))
                                    })
                                } else {
                                    None
                                };

                                // Calculate the Y position for this group in viewport coordinates
                                let group_y = current_row as f32 * spaced_row_height;

                                // Always render complete albums (no partial visibility check)
                                let positioned_rect = egui::Rect::from_min_size(
                                    egui::pos2(ui.min_rect().left(), ui.min_rect().top() + group_y),
                                    egui::vec2(
                                        ui.available_width(),
                                        (group_lines - group_margin_bottom_row_count) as f32
                                            * spaced_row_height,
                                    ),
                                );

                                // Display the complete group
                                let group_response = ui
                                    .scope_builder(
                                        egui::UiBuilder::new().max_rect(positioned_rect),
                                        |ui| {
                                            // Show the entire group (no row range filtering)
                                            group::ui(
                                                &group,
                                                ui,
                                                &config_read.style,
                                                0..usize::MAX, // Show all rows of this group
                                                cover_art.map(|(id, bytes)| {
                                                    egui::ImageSource::Bytes {
                                                        uri: id.into(),
                                                        bytes: bytes.into(),
                                                    }
                                                }),
                                                config_read.general.album_art_enabled,
                                                &self.logic.get_song_map(),
                                                playing_song_id.as_ref(),
                                            )
                                        },
                                    )
                                    .inner;

                                // Handle song selection
                                if let Some(song_id) = group_response.clicked_song {
                                    self.logic.play_song(song_id);
                                }

                                current_row += group_lines;
                            }
                        });
                });
            });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        let Some(inner_rect) = self.current_window_size else {
            return;
        };
        let mut config = self.config.write().unwrap();
        config.general.window_width = inner_rect.width();
        config.general.window_height = inner_rect.height();
        config.save();
    }
}

/// Helper function to create a control button with optional color override
/// Returns true if the button was clicked
fn control_button(
    ui: &mut egui::Ui,
    icon: &str,
    text_color: egui::Color32,
    hover_color: egui::Color32,
) -> bool {
    ui.scope(|ui| {
        let visuals = &mut ui.style_mut().visuals;
        visuals.widgets.inactive.fg_stroke.color = text_color;
        visuals.widgets.hovered.fg_stroke.color = hover_color;
        visuals.widgets.active.fg_stroke.color = hover_color;
        ui.add(
            egui::Label::new(egui::RichText::new(icon).size(CONTROL_BUTTON_SIZE))
                .selectable(false)
                .sense(egui::Sense::click()),
        )
        .clicked()
    })
    .inner
}
