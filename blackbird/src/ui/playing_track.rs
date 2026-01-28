use blackbird_core::{PlaybackMode, blackbird_state::TrackId};
use egui::{
    Align, Color32, Label, Layout, RichText, Sense, Spinner, TextStyle, Ui, UiBuilder, Vec2, vec2,
};

use crate::{
    bc,
    config::Config,
    cover_art_cache::{CachePriority, CoverArtCache},
    ui::{style, util},
};

const CONTROL_BUTTON_SIZE: f32 = 28.0;

pub fn ui(
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
                                tdd.cover_art_id.as_ref(),
                                CachePriority::Visible,
                            ))
                            .show_loading_spinner(false),
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
                                    .filter(|a| a.as_str() != tdd.album_artist.as_str())
                                {
                                    ui.add(
                                        Label::new(
                                            RichText::new(artist.as_str())
                                                .color(style::string_to_colour(artist)),
                                        )
                                        .selectable(false),
                                    );
                                    ui.add(Label::new(" - ").selectable(false));
                                }
                                ui.add(
                                    Label::new(
                                        RichText::new(tdd.track_title.as_str())
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
                                        RichText::new(tdd.album_name.as_str())
                                            .color(config.style.album()),
                                    )
                                    .selectable(false),
                                );
                                ui.add(Label::new(" by ").selectable(false));
                                ui.add(
                                    Label::new(
                                        RichText::new(tdd.album_artist.as_str())
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
