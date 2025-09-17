use std::sync::Arc;
use std::time::Duration;

mod group;
mod style;
mod track;
mod util;

use blackbird_core::{PlaybackMode, blackbird_state::TrackId, util::seconds_to_hms_string};
use egui::{
    Align, CentralPanel, Color32, Context, FontData, FontDefinitions, FontFamily, Frame, Label,
    Layout, Margin, PointerButton, Pos2, Rect, RichText, ScrollArea, Sense, Slider, Spinner,
    TextStyle, Ui, UiBuilder, Vec2, Visuals, Window, pos2,
    style::{HandleShape, ScrollAnimation, ScrollStyle},
    vec2,
};
pub use style::Style;

use crate::{App, bc, config::Config, cover_art_cache::CoverArtCache};

// UI Constants
const CONTROL_BUTTON_SIZE: f32 = 28.0;

pub fn initialize(cc: &eframe::CreationContext<'_>, config: &Config) {
    cc.egui_ctx.set_visuals(Visuals::dark());
    cc.egui_ctx.style_mut(|style| {
        style.visuals.panel_fill = config.style.background();
        style.visuals.override_text_color = Some(config.style.text());
        style.scroll_animation = ScrollAnimation::duration(0.2);
    });
    cc.egui_ctx.options_mut(|options| {
        options.input_options.line_scroll_speed = config.style.scroll_multiplier
    });

    let mut fonts = FontDefinitions::default();
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
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);

    cc.egui_ctx.set_fonts(fonts);

    egui_extras::install_image_loaders(&cc.egui_ctx);
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
                track_to_scroll_to = Some(track_and_position.track_id);
            }
        }

        if let Some(error) = logic.get_error() {
            let mut open = true;
            Window::new("Error").open(&mut open).show(ctx, |ui| {
                ui.label(RichText::new(error.display_name()).heading());
                ui.label(RichText::new(
                    error.display_message(&logic.get_state().read().unwrap()),
                ));
            });
            if !open {
                logic.clear_error();
            }
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
                    if i.pointer.button_released(PointerButton::Extra1) {
                        logic.previous();
                    }

                    if i.pointer.button_released(PointerButton::Extra2) {
                        logic.next();
                    }
                });

                playing_track_info(
                    ui,
                    logic,
                    config,
                    has_loaded_all_tracks,
                    &mut track_to_scroll_to,
                    &mut self.cover_art_cache,
                );
                scrub_bar(ui, logic, config);

                ui.separator();

                library(
                    ui,
                    logic,
                    config,
                    has_loaded_all_tracks,
                    scroll_margin.into(),
                    track_to_scroll_to,
                    &mut self.cover_art_cache,
                );
            });
    }
}

fn playing_track_info(
    ui: &mut Ui,
    logic: &mut bc::Logic,
    config: &Config,
    has_loaded_all_tracks: bool,
    track_to_scroll_to: &mut Option<TrackId>,
    cover_art_cache: &mut CoverArtCache,
) {
    let track_display_details = logic.get_track_display_details();
    let track_id = track_display_details
        .as_ref()
        .map(|tdd| tdd.track_id.clone());
    let mut track_clicked = false;

    ui.horizontal(|ui| {
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.style_mut().spacing.item_spacing = Vec2::ZERO;
            ui.horizontal(|ui| {
                if logic.should_show_loading_indicator() {
                    ui.add(Spinner::new());
                    ui.add_space(16.0);
                }

                if let Some(tdd) = track_display_details {
                    let ui_builder = UiBuilder::new()
                        .layout(Layout::left_to_right(Align::Min))
                        .sense(Sense::click());
                    let r = ui.scope_builder(ui_builder, |ui| {
                        let image_size = ui.text_style_height(&TextStyle::Body) * 2.5;
                        ui.add_sized(
                            vec2(image_size, image_size),
                            egui::Image::new(
                                cover_art_cache.get(logic, tdd.cover_art_id.as_deref()),
                            ),
                        );

                        ui.add_space(6.0);

                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                if let Some(artist) = tdd
                                    .track_artist
                                    .as_ref()
                                    .filter(|a| **a != tdd.album_artist)
                                {
                                    ui.add(
                                        Label::new(
                                            RichText::new(artist)
                                                .color(style::string_to_colour(artist)),
                                        )
                                        .selectable(false),
                                    );
                                    ui.add(Label::new(" - ").selectable(false));
                                }
                                ui.add(
                                    Label::new(
                                        RichText::new(&tdd.track_title)
                                            .color(config.style.track_name_playing()),
                                    )
                                    .selectable(false),
                                );
                            });
                            ui.horizontal(|ui| {
                                ui.add(
                                    Label::new(
                                        RichText::new(&tdd.album_name).color(config.style.album()),
                                    )
                                    .selectable(false),
                                );
                                ui.add(Label::new(" by ").selectable(false));
                                ui.add(
                                    Label::new(
                                        RichText::new(&tdd.album_artist)
                                            .color(style::string_to_colour(&tdd.album_artist)),
                                    )
                                    .selectable(false),
                                );
                            });
                        });
                    });
                    track_clicked = r.response.clicked();
                } else {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            let track_count = logic.get_state().read().unwrap().track_ids.len();
                            ui.add(
                                Label::new(format!(
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
                            ui.add(Label::new("Click on a track to play it!").selectable(false));
                        });
                    });
                }
            });
        });

        if logic.is_track_loaded() {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
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

    if track_clicked && let Some(track_id) = track_id {
        *track_to_scroll_to = Some(track_id);
    }
}

fn scrub_bar(ui: &mut Ui, logic: &mut bc::Logic, config: &Config) {
    ui.horizontal(|ui| {
        let (position_secs, duration_secs) = logic
            .get_track_display_details()
            .map(|pi| {
                (
                    pi.track_position.as_secs_f32(),
                    pi.track_duration.as_secs_f32(),
                )
            })
            .unwrap_or_default();

        // Position/duration text
        let [position_hms, duration_hms] =
            [position_secs, duration_secs].map(|s| seconds_to_hms_string(s as u32, true));
        ui.add(
            Label::new(
                RichText::new(format!("{position_hms} / {duration_hms}"))
                    .color(config.style.track_duration()),
            )
            .selectable(false),
        );

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // Volume slider
            ui.add_space(ui.style().spacing.window_margin.right as f32);
            let mut volume = logic.get_volume();
            let volume_response = ui.add_sized(
                [ui.available_width().min(80.0), ui.available_height()],
                Slider::new(&mut volume, 0.0..=1.0)
                    .show_value(false)
                    .handle_shape(HandleShape::Rect { aspect_ratio: 0.75 }),
            );
            if volume_response.changed() {
                logic.set_volume(volume);
            }
            ui.label(egui_phosphor::regular::SPEAKER_HIGH);

            // Separator
            ui.separator();

            // Scrub bar
            let mut slider_position = position_secs;
            let slider_duration = duration_secs.max(1.0);
            ui.style_mut().spacing.slider_width = ui.available_width();
            let slider_response = ui.add(
                Slider::new(&mut slider_position, 0.0..=slider_duration)
                    .show_value(false)
                    .handle_shape(HandleShape::Rect { aspect_ratio: 2.0 }),
            );
            if slider_response.changed() {
                let seek_position = Duration::from_secs_f32(slider_position);
                logic.seek_current(seek_position);
            }
        });
    });
}

fn library(
    ui: &mut Ui,
    logic: &mut bc::Logic,
    config: &Config,
    has_loaded_all_tracks: bool,
    scroll_margin: f32,
    track_to_scroll_to: Option<TrackId>,
    cover_art_cache: &mut CoverArtCache,
) {
    ui.scope(|ui| {
        if !has_loaded_all_tracks {
            ui.add_sized(ui.available_size(), Spinner::new());
            return;
        }

        // Make the scroll bar solid, and hide its background. Ideally, we'd set the opacity
        // to 0, but egui doesn't allow that for solid scroll bars.
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

        ScrollArea::vertical()
            .auto_shrink(false)
            .show_viewport(ui, |ui, viewport| {
                if let Some(scroll_to_height) = track_to_scroll_to.and_then(|id| {
                    group::target_scroll_height_for_track(
                        &logic.get_state().read().unwrap(),
                        spaced_row_height,
                        &id,
                    )
                }) {
                    let target_height = area_offset_y + scroll_to_height - viewport.min.y;
                    ui.scroll_to_rect(
                        Rect {
                            min: Pos2::new(viewport.min.x, target_height),
                            max: Pos2::new(viewport.max.x, target_height + spaced_row_height),
                        },
                        Some(Align::Center),
                    );
                }

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
                let visible_groups =
                    logic.get_visible_groups(visible_row_range.clone(), group::line_count);

                let playing_track_id = logic.get_playing_track_id();
                let mut current_row = visible_groups.start_row;

                for group in visible_groups.groups {
                    let group_lines = group::line_count(&group);

                    // Calculate the Y position for this group in viewport coordinates
                    let group_y = current_row as f32 * spaced_row_height;

                    // Always render complete albums (no partial visibility check)
                    let positioned_rect = Rect::from_min_size(
                        pos2(ui.min_rect().left(), ui.min_rect().top() + group_y),
                        vec2(
                            ui.available_width(),
                            (group_lines - 2 * group::GROUP_MARGIN_BOTTOM_ROW_COUNT) as f32
                                * spaced_row_height,
                        ),
                    );

                    // Display the complete group
                    let group_response = ui
                        .scope_builder(UiBuilder::new().max_rect(positioned_rect), |ui| {
                            // Show the entire group (no row range filtering)
                            group::ui(
                                &group,
                                ui,
                                &config.style,
                                logic,
                                playing_track_id.as_ref(),
                                cover_art_cache,
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
fn control_button(ui: &mut Ui, icon: &str, text_color: Color32, hover_color: Color32) -> bool {
    ui.scope(|ui| {
        let visuals = &mut ui.style_mut().visuals;
        visuals.widgets.inactive.fg_stroke.color = text_color;
        visuals.widgets.hovered.fg_stroke.color = hover_color;
        visuals.widgets.active.fg_stroke.color = hover_color;
        ui.add(
            Label::new(RichText::new(icon).size(CONTROL_BUTTON_SIZE))
                .selectable(false)
                .sense(Sense::click()),
        )
        .clicked()
    })
    .inner
}
