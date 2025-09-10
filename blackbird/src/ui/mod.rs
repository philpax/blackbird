use std::sync::Arc;
use std::time::Duration;

mod group;
mod style;
mod track;
mod util;

use blackbird_core::util::seconds_to_hms_string;
use blackbird_core::PlaybackMode;
pub use style::Style;

use crate::{bc, config::Config};

// UI Constants
const CONTROL_BUTTON_SIZE: f32 = 28.0;

pub fn initialize(cc: &eframe::CreationContext<'_>, config: &Config) {
    cc.egui_ctx.set_visuals(egui::Visuals::dark());
    cc.egui_ctx.style_mut(|style| {
        style.visuals.panel_fill = config.style.background();
        style.visuals.override_text_color = Some(config.style.text());
    });
    cc.egui_ctx.options_mut(|options| {
        options.input_options.line_scroll_speed = config.style.scroll_multiplier
    });

    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "GoNoto".into(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../../assets/GoNotoKurrent-Regular.ttf"
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
}

pub fn render(ctx: &egui::Context, config: &Config, logic: &mut bc::Logic) {
    if let Some(error) = logic.get_error() {
        let mut open = true;
        egui::Window::new("Error").open(&mut open).show(ctx, |ui| {
            ui.label(&error);
        });
        if !open {
            logic.clear_error();
        }
    }

    let margin = 8;
    let scroll_margin = 4;
    let has_loaded_all_tracks = logic.has_loaded_all_tracks();
    egui::CentralPanel::default()
        .frame(
            egui::Frame::default()
                .inner_margin(egui::Margin {
                    left: margin,
                    right: scroll_margin,
                    top: margin,
                    bottom: margin,
                })
                .fill(config.style.background()),
        )
        .show(ctx, |ui| {
            ui.input(|i| {
                if i.pointer.button_released(egui::PointerButton::Extra1) {
                    logic.previous();
                }

                if i.pointer.button_released(egui::PointerButton::Extra2) {
                    logic.next();
                }
            });

            playing_track_info(ui, logic, config, has_loaded_all_tracks);
            scrub_bar(ui, logic, config);

            ui.separator();

            library(
                ui,
                logic,
                config,
                has_loaded_all_tracks,
                scroll_margin.into(),
            );
        });
}

fn playing_track_info(
    ui: &mut egui::Ui,
    logic: &mut bc::Logic,
    config: &Config,
    has_loaded_all_tracks: bool,
) {
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.style_mut().spacing.item_spacing = egui::Vec2::ZERO;
            ui.horizontal(|ui| {
                if logic.should_show_loading_indicator() {
                    ui.add(egui::Spinner::new());
                    ui.add_space(16.0);
                }

                if let Some(pi) = logic.get_playing_info() {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            if let Some(artist) =
                                pi.track_artist.as_ref().filter(|a| **a != pi.album_artist)
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
                                    egui::RichText::new(&pi.track_title)
                                        .color(config.style.track_name_playing()),
                                )
                                .selectable(false),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&pi.album_name).color(config.style.album()),
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
                    });
                } else {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            let track_count = logic.get_state().read().unwrap().track_ids.len();
                            ui.add(
                                egui::Label::new(format!(
                                    "Nothing playing | {}{} tracks",
                                    if has_loaded_all_tracks {
                                        ""
                                    } else {
                                        "Loading tracks... | "
                                    },
                                    track_count,
                                ))
                                .selectable(false),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::Label::new("Click on a track to play it!").selectable(false),
                            );
                        });
                    });
                }
            });
        });

        if logic.is_track_loaded() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.style_mut().visuals.override_text_color = None;

                let default = config.style.text();
                let active = config.style.track_name_playing();

                if control_button(ui, egui_phosphor::regular::SKIP_FORWARD, default, active) {
                    logic.next();
                }
                if control_button(ui, egui_phosphor::regular::PLAY_PAUSE, default, active) {
                    logic.toggle_current();
                }
                if control_button(ui, egui_phosphor::regular::SKIP_BACK, default, active) {
                    logic.previous();
                }
                if control_button(ui, egui_phosphor::regular::STOP, default, active) {
                    logic.stop_current();
                }

                ui.add_space(24.0);

                // Playback mode buttons (Sequential, Shuffle, Repeat One)
                let playback = logic.get_playback_mode();
                for (mode, icon) in [
                    (PlaybackMode::Sequential, egui_phosphor::regular::LIST),
                    (PlaybackMode::Shuffle, egui_phosphor::regular::SHUFFLE),
                    (PlaybackMode::RepeatOne, egui_phosphor::regular::REPEAT_ONCE),
                ] {
                    let button_color = if playback == mode { active } else { default };
                    if control_button(ui, icon, button_color, active) {
                        logic.set_playback_mode(mode);
                    }
                }
            });
        }
    });
}

fn scrub_bar(ui: &mut egui::Ui, logic: &mut bc::Logic, config: &Config) {
    ui.horizontal(|ui| {
        let (position_secs, duration_secs) = logic
            .get_playing_info()
            .map(|pi| {
                (
                    pi.track_position.as_secs_f32(),
                    pi.track_duration.as_secs_f32(),
                )
            })
            .unwrap_or_default();

        // Add position/duration text
        let [position_hms, duration_hms] =
            [position_secs, duration_secs].map(|s| seconds_to_hms_string(s as u32, true));
        ui.add(
            egui::Label::new(
                egui::RichText::new(format!("{position_hms} / {duration_hms}"))
                    .color(config.style.track_duration()),
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
            logic.seek_current(seek_position);
        }
    });
}

fn library(
    ui: &mut egui::Ui,
    logic: &mut bc::Logic,
    config: &Config,
    has_loaded_all_tracks: bool,
    scroll_margin: f32,
) {
    ui.scope(|ui| {
        if !has_loaded_all_tracks {
            ui.add_sized(ui.available_size(), egui::Spinner::new());
            return;
        }

        // Make the scroll bar solid, and hide its background. Ideally, we'd set the opacity
        // to 0, but egui doesn't allow that for solid scroll bars.
        ui.style_mut().spacing.scroll = egui::style::ScrollStyle {
            bar_inner_margin: scroll_margin,
            bar_width: 20.0,
            handle_min_length: 36.0,
            ..egui::style::ScrollStyle::solid()
        };
        ui.style_mut().visuals.extreme_bg_color = config.style.background();

        let spaced_row_height = util::spaced_row_height(ui);
        let group_margin_bottom_row_count = 1;

        // Get total rows for virtual rendering
        let total_rows =
            logic.calculate_total_rows(group_margin_bottom_row_count, group::line_count);

        // Use custom virtual rendering for better performance
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show_viewport(ui, |ui, viewport| {
                // Set the total height for the virtual content (with spacing)
                ui.set_height(spaced_row_height * total_rows as f32);

                // Calculate which rows are visible with some buffer
                let first_visible_row =
                    ((viewport.min.y / spaced_row_height).floor().max(0.0)) as usize;
                let last_visible_row = (viewport.max.y / spaced_row_height).ceil() as usize + 5; // Add buffer
                let last_visible_row = last_visible_row.min(total_rows);

                if first_visible_row >= last_visible_row {
                    return;
                }

                let visible_row_range = first_visible_row..last_visible_row;

                // Calculate which groups are in view
                let visible_groups = logic.get_visible_groups(
                    visible_row_range.clone(),
                    group_margin_bottom_row_count,
                    group::line_count,
                );

                let playing_track_id = logic.get_playing_track_id();
                let mut current_row = visible_groups.start_row;

                for group in visible_groups.groups {
                    let group_lines = group::line_count(&group) + group_margin_bottom_row_count;

                    // Handle cover art if enabled
                    if config.general.album_art_enabled {
                        if let Some(cover_art_id) = &group.cover_art_id {
                            if !logic.has_cover_art(cover_art_id) {
                                logic.fetch_cover_art(cover_art_id);
                            }
                        }
                    }

                    // Get cover art if needed
                    let cover_art = if config.general.album_art_enabled {
                        group
                            .cover_art_id
                            .as_deref()
                            .and_then(|id| Some((id.to_string(), logic.get_cover_art(id)?)))
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
                        .scope_builder(egui::UiBuilder::new().max_rect(positioned_rect), |ui| {
                            // Show the entire group (no row range filtering)
                            group::ui(
                                &group,
                                ui,
                                &config.style,
                                0..usize::MAX, // Show all rows of this group
                                cover_art.map(|(id, bytes)| egui::ImageSource::Bytes {
                                    uri: id.into(),
                                    bytes: bytes.into(),
                                }),
                                config.general.album_art_enabled,
                                logic.get_state(),
                                playing_track_id.as_ref(),
                            )
                        })
                        .inner;

                    // Handle track selection
                    if let Some(track_id) = group_response.clicked_track {
                        logic.request_play_track(track_id);
                    }

                    current_row += group_lines;
                }
            });
    });
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
