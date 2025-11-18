use std::sync::Arc;
use std::time::Duration;

mod group;
pub use group::GROUP_ALBUM_ART_SIZE;

mod style;
mod track;
mod util;

use blackbird_core::{
    PlaybackMode, TrackDisplayDetails, blackbird_state::TrackId, util::seconds_to_hms_string,
};
use egui::{
    Align, Align2, CentralPanel, Color32, Context, FontData, FontDefinitions, FontFamily, Frame,
    Key, Label, Layout, Margin, PointerButton, Pos2, Rect, RichText, ScrollArea, Sense, Slider,
    Spinner, TextEdit, TextFormat, TextStyle, Ui, UiBuilder, Vec2, Vec2b, Visuals, Window, pos2,
    style::{HandleShape, ScrollAnimation, ScrollStyle},
    vec2,
};
pub use style::Style;

use crate::{App, bc, config::Config, cover_art_cache::CoverArtCache};

// UI Constants
const CONTROL_BUTTON_SIZE: f32 = 28.0;

#[derive(Default)]
pub struct UiState {
    search_open: bool,
    search_query: String,
    lyrics_open: bool,
    lyrics_track_id: Option<TrackId>,
    lyrics_data: Option<bc::bs::StructuredLyrics>,
    lyrics_loading: bool,
}

pub fn initialize(cc: &eframe::CreationContext<'_>, config: &Config) -> UiState {
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
                if self.ui_state.lyrics_open {
                    self.ui_state.lyrics_track_id = Some(track_and_position.track_id.clone());
                    self.ui_state.lyrics_loading = true;
                    self.ui_state.lyrics_data = None; // Clear old lyrics while loading
                    logic.request_lyrics(&track_and_position.track_id);
                }
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

        ctx.input(|i| {
            if i.modifiers.command && i.key_released(egui::Key::F) {
                self.ui_state.search_open = !self.ui_state.search_open;
            }
            if i.modifiers.command && i.key_released(egui::Key::L) {
                self.ui_state.lyrics_open = !self.ui_state.lyrics_open;
                // Request lyrics for the currently playing track when opening the window
                if self.ui_state.lyrics_open
                    && let Some(track_id) = logic.get_playing_track_id()
                {
                    self.ui_state.lyrics_track_id = Some(track_id.clone());
                    self.ui_state.lyrics_loading = true;
                    logic.request_lyrics(&track_id);
                }
            }
        });

        // Process incoming lyrics data
        while let Ok(lyrics_data) = self.lyrics_loaded_rx.try_recv() {
            if Some(&lyrics_data.track_id) == self.ui_state.lyrics_track_id.as_ref() {
                self.ui_state.lyrics_data = lyrics_data.lyrics;
                self.ui_state.lyrics_loading = false;
            }
        }

        if self.ui_state.search_open {
            search(
                logic,
                ctx,
                &config.style,
                &mut self.ui_state.search_open,
                &mut self.ui_state.search_query,
            );
        }

        if self.ui_state.lyrics_open {
            lyrics_window(
                logic,
                ctx,
                &config.style,
                &mut self.ui_state.lyrics_open,
                &mut self.ui_state.lyrics_data,
                &mut self.ui_state.lyrics_loading,
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
                    track_to_scroll_to.as_ref(),
                    &mut self.cover_art_cache,
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
    let album_id = track_display_details
        .as_ref()
        .map(|tdd| tdd.album_id.clone());
    let mut track_clicked = false;
    let mut track_heart_clicked = false;
    let mut album_heart_clicked = false;

    ui.horizontal(|ui| {
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.style_mut().spacing.item_spacing = Vec2::ZERO;
            ui.horizontal(|ui| {
                if logic.should_show_loading_indicator() {
                    ui.add(Spinner::new());
                    ui.add_space(16.0);
                }

                if let Some(tdd) = track_display_details {
                    // Get album starred status
                    let album_starred = logic
                        .get_state()
                        .read()
                        .unwrap()
                        .library
                        .albums
                        .get(&tdd.album_id)
                        .map(|album| album.starred)
                        .unwrap_or(false);

                    let ui_builder = UiBuilder::new()
                        .layout(Layout::left_to_right(Align::Min))
                        .sense(Sense::click());
                    let r = ui.scope_builder(ui_builder, |ui| {
                        let image_size = ui.text_style_height(&TextStyle::Body) * 2.5;
                        ui.add_sized(
                            vec2(image_size, image_size),
                            egui::Image::new(cover_art_cache.get(
                                logic,
                                tdd.cover_art_id.as_deref(),
                                true,
                            )),
                        );

                        ui.add_space(6.0);

                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                // Add heart for track
                                let (heart_response, _) = util::draw_heart(
                                    ui,
                                    TextStyle::Body.resolve(ui.style()),
                                    util::HeartPlacement::Space,
                                    tdd.starred,
                                    true,
                                );
                                if heart_response.clicked() {
                                    track_heart_clicked = true;
                                }
                                ui.add_space(4.0);

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
                                // Add heart for album
                                let (heart_response, _) = util::draw_heart(
                                    ui,
                                    TextStyle::Body.resolve(ui.style()),
                                    util::HeartPlacement::Space,
                                    album_starred,
                                    true,
                                );
                                if heart_response.clicked() {
                                    album_heart_clicked = true;
                                }
                                ui.add_space(4.0);

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
                            let track_count =
                                logic.get_state().read().unwrap().library.track_ids.len();
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

                if control_button(
                    ui,
                    egui_phosphor::regular::SKIP_FORWARD,
                    default,
                    active,
                    "Next Track",
                ) {
                    logic.next();
                }
                if control_button(
                    ui,
                    egui_phosphor::regular::PLAY_PAUSE,
                    default,
                    active,
                    "Play/Pause",
                ) {
                    logic.toggle_current();
                }
                if control_button(
                    ui,
                    egui_phosphor::regular::SKIP_BACK,
                    default,
                    active,
                    "Previous Track",
                ) {
                    logic.previous();
                }
                if control_button(ui, egui_phosphor::regular::STOP, default, active, "Stop") {
                    logic.stop_current();
                }

                ui.add_space(24.0);

                // Playback mode buttons
                let playback = logic.get_playback_mode();
                for (mode, icon, render_separator) in [
                    (
                        PlaybackMode::Sequential,
                        egui_phosphor::regular::QUEUE,
                        true,
                    ),
                    (
                        PlaybackMode::RepeatOne,
                        egui_phosphor::regular::REPEAT_ONCE,
                        false,
                    ),
                    (
                        PlaybackMode::GroupRepeat,
                        egui_phosphor::regular::REPEAT,
                        true,
                    ),
                    (
                        PlaybackMode::Shuffle,
                        egui_phosphor::regular::SHUFFLE,
                        false,
                    ),
                    (
                        PlaybackMode::LikedShuffle,
                        egui_phosphor::regular::STAR,
                        true,
                    ),
                    (
                        PlaybackMode::GroupShuffle,
                        egui_phosphor::regular::VINYL_RECORD,
                        false,
                    ),
                    (
                        PlaybackMode::LikedGroupShuffle,
                        egui_phosphor::regular::DISC,
                        false,
                    ),
                ]
                .iter()
                .rev()
                .copied()
                {
                    if render_separator {
                        ui.separator();
                    }

                    let button_color = if playback == mode { active } else { default };
                    if control_button(ui, icon, button_color, active, mode.as_str()) {
                        logic.set_playback_mode(mode);
                    }
                }
            });
        }
    });

    if track_clicked && let Some(ref track_id) = track_id {
        *track_to_scroll_to = Some(track_id.clone());
    }

    if track_heart_clicked && let Some(ref track_id) = track_id {
        let starred = logic
            .get_state()
            .read()
            .unwrap()
            .library
            .track_map
            .get(track_id)
            .map(|track| track.starred)
            .unwrap_or(false);
        logic.set_track_starred(track_id, !starred);
    }

    if album_heart_clicked && let Some(ref album_id) = album_id {
        let starred = logic
            .get_state()
            .read()
            .unwrap()
            .library
            .albums
            .get(album_id)
            .map(|album| album.starred)
            .unwrap_or(false);
        logic.set_album_starred(album_id, !starred);
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
    track_to_scroll_to: Option<&TrackId>,
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
                        id,
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

                    if group_response.clicked_heart {
                        logic.set_album_starred(&group.album_id, !group.starred);
                    }

                    current_row += group_lines;
                }
            });
    });
}

/// Helper function to create a control button with optional color override
/// Returns true if the button was clicked
fn control_button(
    ui: &mut Ui,
    icon: &str,
    text_color: Color32,
    hover_color: Color32,
    tooltip: &str,
) -> bool {
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
        .on_hover_text(tooltip)
        .clicked()
    })
    .inner
}

fn search(
    logic: &mut bc::Logic,
    ctx: &Context,
    style: &style::Style,
    search_open: &mut bool,
    search_query: &mut String,
) {
    let mut requested_track_id = None;
    let mut clear = false;

    Window::new("Search")
        .open(search_open)
        .default_pos(ctx.screen_rect().center())
        .default_size(ctx.screen_rect().size() * Vec2::new(0.75, 0.3))
        .pivot(Align2::CENTER_CENTER)
        .collapsible(false)
        .show(ctx, |ui| {
            let response = ui.add_sized(
                Vec2::new(ui.available_width(), ui.text_style_height(&TextStyle::Body)),
                TextEdit::singleline(search_query).hint_text("Your search here..."),
            );
            response.request_focus();

            let mut play_first_track = false;
            if response.has_focus() {
                if ui.input(|i| i.key_pressed(Key::Escape)) {
                    clear = true;
                } else if ui.input(|i| i.key_pressed(Key::Enter)) {
                    play_first_track = true;
                }
            }

            egui::Frame::dark_canvas(ui.style()).show(ui, |ui| {
                ui.set_min_size(ui.available_size());

                let length = search_query.len();
                if length == 0 {
                    ui.label("Type something in to search...");
                    return;
                } else if length < 3 {
                    ui.label("Query too short, please enter at least 3 characters...");
                    return;
                }

                let app_state = logic.get_state();
                let mut app_state = app_state.write().unwrap();
                let results = app_state.library.search(search_query);
                if results.is_empty() {
                    ui.label("No results found...");
                    return;
                }

                // If Enter was pressed and we have results, select the first item
                if play_first_track && !results.is_empty() {
                    requested_track_id = Some(results[0].clone());
                }

                let response = egui::ScrollArea::new(Vec2b::TRUE)
                    .auto_shrink(Vec2b::FALSE)
                    .show_rows(
                        ui,
                        ui.text_style_height(&TextStyle::Body),
                        results.len(),
                        |ui, row_indices| {
                            let mut requested_track_id = None;
                            for id in &results[row_indices] {
                                let Some(details) =
                                    TrackDisplayDetails::from_track_id(id, &app_state)
                                else {
                                    continue;
                                };

                                let font_id = TextStyle::Body.resolve(ui.style());

                                // Allocate space for this row and sense interaction
                                let (rect, response) = ui.allocate_exact_size(
                                    vec2(
                                        ui.available_width(),
                                        ui.text_style_height(&TextStyle::Body),
                                    ),
                                    Sense::click(),
                                );

                                let darken = |color: Color32| -> Color32 {
                                    const DARKEN_FACTOR: f32 = 0.75;
                                    let [r, g, b, a] = color.to_array();
                                    Color32::from_rgba_unmultiplied(
                                        (r as f32 * DARKEN_FACTOR) as u8,
                                        (g as f32 * DARKEN_FACTOR) as u8,
                                        (b as f32 * DARKEN_FACTOR) as u8,
                                        a,
                                    )
                                };

                                let is_hovered = response.hovered();
                                let artist = details.artist();
                                let [artist_color, track_color, length_color] = [
                                    style::string_to_colour(artist).into(),
                                    style.track_name(),
                                    style.track_length(),
                                ]
                                .map(|color| if is_hovered { color } else { darken(color) });
                                let layout_job = {
                                    let mut layout_job = egui::text::LayoutJob::default();
                                    layout_job.append(
                                        artist,
                                        0.0,
                                        TextFormat {
                                            color: artist_color,
                                            font_id: font_id.clone(),
                                            ..Default::default()
                                        },
                                    );
                                    layout_job.append(
                                        " - ",
                                        0.0,
                                        TextFormat {
                                            font_id: font_id.clone(),
                                            ..Default::default()
                                        },
                                    );
                                    layout_job.append(
                                        &details.track_title,
                                        0.0,
                                        TextFormat {
                                            color: track_color,
                                            font_id: font_id.clone(),
                                            ..Default::default()
                                        },
                                    );
                                    layout_job.append(
                                        &format!(
                                            " [{}]",
                                            seconds_to_hms_string(
                                                details.track_duration.as_secs() as u32,
                                                false
                                            )
                                        ),
                                        0.0,
                                        TextFormat {
                                            color: length_color,
                                            font_id: font_id.clone(),
                                            ..Default::default()
                                        },
                                    );
                                    layout_job.wrap.max_width = f32::INFINITY;
                                    layout_job
                                };
                                let galley = ui.fonts(|fonts| fonts.layout_job(layout_job));
                                ui.painter()
                                    .galley(rect.left_top(), galley, Color32::PLACEHOLDER);

                                if response.clicked() {
                                    requested_track_id = Some(id.clone());
                                }
                            }
                            requested_track_id
                        },
                    );

                if requested_track_id.is_none() {
                    requested_track_id = response.inner;
                }
            });
        });

    if let Some(track_id) = requested_track_id {
        logic.request_play_track(&track_id);
        clear = true;
    }

    if clear {
        *search_open = false;
        search_query.clear();
    }
}

fn lyrics_window(
    logic: &mut bc::Logic,
    ctx: &Context,
    style: &style::Style,
    lyrics_open: &mut bool,
    lyrics_data: &mut Option<bc::bs::StructuredLyrics>,
    lyrics_loading: &mut bool,
) {
    Window::new("Lyrics")
        .open(lyrics_open)
        .default_pos(ctx.screen_rect().center())
        .default_size(ctx.screen_rect().size() * Vec2::new(0.5, 0.6))
        .pivot(Align2::CENTER_CENTER)
        .collapsible(false)
        .show(ctx, |ui| {
            if *lyrics_loading {
                ui.vertical_centered(|ui| {
                    ui.add_space(20.0);
                    ui.add(Spinner::new());
                    ui.add_space(10.0);
                    ui.label("Loading lyrics...");
                });
                return;
            }

            let Some(lyrics) = lyrics_data else {
                ui.vertical_centered(|ui| {
                    ui.add_space(20.0);
                    ui.label("No lyrics available for this track.");
                });
                return;
            };

            if lyrics.line.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(20.0);
                    ui.label("No lyrics available for this track.");
                });
                return;
            }

            // Get current playback position in milliseconds
            let current_position_ms = logic
                .get_playing_position()
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);

            // Apply offset if present
            let adjusted_position_ms = current_position_ms + lyrics.offset.unwrap_or(0);

            // Find the current line index based on playback position
            let current_line_idx = if lyrics.synced {
                lyrics
                    .line
                    .iter()
                    .enumerate()
                    .rev()
                    .find(|(_, line)| {
                        line.start.unwrap_or(0) <= adjusted_position_ms
                    })
                    .map(|(idx, _)| idx)
                    .unwrap_or(0)
            } else {
                0 // For unsynced lyrics, don't highlight any line
            };

            ScrollArea::vertical()
                .auto_shrink(Vec2b::FALSE)
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());

                    for (idx, line) in lyrics.line.iter().enumerate() {
                        let is_current = lyrics.synced && idx == current_line_idx;
                        let is_past = lyrics.synced && idx < current_line_idx;

                        let text_color = if is_current {
                            style.text()
                        } else if is_past {
                            // Dim past lyrics
                            let [r, g, b, a] = style.text().to_array();
                            Color32::from_rgba_unmultiplied(
                                (r as f32 * 0.5) as u8,
                                (g as f32 * 0.5) as u8,
                                (b as f32 * 0.5) as u8,
                                a,
                            )
                        } else {
                            // Dim future lyrics
                            let [r, g, b, a] = style.text().to_array();
                            Color32::from_rgba_unmultiplied(
                                (r as f32 * 0.7) as u8,
                                (g as f32 * 0.7) as u8,
                                (b as f32 * 0.7) as u8,
                                a,
                            )
                        };

                        let rich_text = RichText::new(&line.value)
                            .color(text_color)
                            .size(if is_current { 18.0 } else { 16.0 });

                        if is_current {
                            let response = ui.label(rich_text.strong());
                            // Scroll to keep the current line visible
                            response.scroll_to_me(Some(Align::Center));
                        } else {
                            ui.label(rich_text);
                        }

                        ui.add_space(4.0);
                    }
                });
        });
}
