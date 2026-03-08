use egui::{
    CollapsingHeader, ComboBox, Context, DragValue, RichText, ScrollArea, TextEdit, Vec2, Vec2b,
    Window, ecolor::Hsva,
};

use blackbird_client_shared::{config::AlbumArtStyle, style as shared_style};

use crate::config::{Config, General, Keybindings};

/// State for the settings window.
#[derive(Default)]
pub struct SettingsState {
    pub open: bool,
    /// Whether the password field is visible.
    show_password: bool,
}

/// Renders the settings window. Returns `true` if the server config changed
/// (meaning the library should be reloaded).
pub fn ui(ctx: &Context, config: &mut Config, settings: &mut SettingsState) -> bool {
    let mut server_changed = false;
    let mut changed = false;

    Window::new("Settings")
        .open(&mut settings.open)
        .default_size(Vec2::new(450.0, 500.0))
        .collapsible(false)
        .show(ctx, |ui| {
            ScrollArea::vertical()
                .auto_shrink(Vec2b::FALSE)
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());

                    // ── Server ──────────────────────────────────────
                    let server_default = blackbird_shared::config::Server::default();
                    CollapsingHeader::new(RichText::new("Server").heading())
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.label(RichText::new("Changes reload the library.").small().weak());
                            ui.add_space(4.0);

                            server_changed |= text_field(
                                ui,
                                "Base URL",
                                &mut config.shared.server.base_url,
                                &server_default.base_url,
                                false,
                            );
                            server_changed |= text_field(
                                ui,
                                "Username",
                                &mut config.shared.server.username,
                                &server_default.username,
                                false,
                            );
                            server_changed |= password_field(
                                ui,
                                "Password",
                                &mut config.shared.server.password,
                                &server_default.password,
                                &mut settings.show_password,
                            );
                            server_changed |= bool_field(
                                ui,
                                "Transcode",
                                &mut config.shared.server.transcode,
                                &server_default.transcode,
                            );

                            if ui
                                .add_enabled(
                                    config.shared.server != server_default,
                                    egui::Button::new("Reset section"),
                                )
                                .clicked()
                            {
                                config.shared.server = server_default;
                                server_changed = true;
                            }
                        });

                    ui.add_space(8.0);

                    // ── Layout ──────────────────────────────────────
                    let layout_default = blackbird_client_shared::config::Layout::default();
                    CollapsingHeader::new(RichText::new("Layout").heading())
                        .default_open(true)
                        .show(ui, |ui| {
                            changed |= bool_field(
                                ui,
                                "Show inline lyrics",
                                &mut config.shared.layout.show_inline_lyrics,
                                &layout_default.show_inline_lyrics,
                            );
                            changed |= enum_field(
                                ui,
                                "Album art style",
                                &mut config.shared.layout.album_art_style,
                                &layout_default.album_art_style,
                            );
                            changed |= usize_field(
                                ui,
                                "Album spacing",
                                &mut config.shared.layout.album_spacing,
                                &layout_default.album_spacing,
                                0,
                                10,
                            );

                            if ui
                                .add_enabled(
                                    config.shared.layout != layout_default,
                                    egui::Button::new("Reset section"),
                                )
                                .clicked()
                            {
                                config.shared.layout = layout_default;
                                changed = true;
                            }
                        });

                    ui.add_space(8.0);

                    // ── Colors ──────────────────────────────────────
                    let style_default = shared_style::Style::default();
                    CollapsingHeader::new(RichText::new("Colors").heading())
                        .default_open(false)
                        .show(ui, |ui| {
                            for i in 0..shared_style::Style::FIELD_COUNT {
                                let (_, human_label) = shared_style::Style::FIELD_NAMES[i];
                                let default_hsv = shared_style::Style::default_field(i);
                                let current = config.style.field_mut(i);

                                ui.horizontal(|ui| {
                                    let label = human_readable_label(human_label);
                                    ui.label(&label);

                                    let mut hsva =
                                        Hsva::new(current[0], current[1], current[2], 1.0);
                                    if egui::color_picker::color_edit_button_hsva(
                                        ui,
                                        &mut hsva,
                                        egui::color_picker::Alpha::Opaque,
                                    )
                                    .changed()
                                    {
                                        *current = [hsva.h, hsva.s, hsva.v];
                                        changed = true;
                                    }

                                    if *current != default_hsv
                                        && ui
                                            .small_button("\u{21BA}")
                                            .on_hover_text("Reset to default")
                                            .clicked()
                                    {
                                        *current = default_hsv;
                                        changed = true;
                                    }
                                });
                            }

                            ui.add_space(4.0);
                            changed |= f32_field(
                                ui,
                                "Scroll multiplier",
                                &mut config.style.scroll_multiplier,
                                &style_default.scroll_multiplier,
                                1.0,
                                200.0,
                                1.0,
                            );

                            if ui
                                .add_enabled(
                                    config.style != style_default,
                                    egui::Button::new("Reset section"),
                                )
                                .clicked()
                            {
                                config.style = style_default;
                                changed = true;
                            }
                        });

                    ui.add_space(8.0);

                    // ── General ──────────────────────────────────────
                    let general_default = General::default();
                    CollapsingHeader::new(RichText::new("General").heading())
                        .default_open(false)
                        .show(ui, |ui| {
                            changed |= f32_field(
                                ui,
                                "Repaint interval (s)",
                                &mut config.general.repaint_secs,
                                &general_default.repaint_secs,
                                0.1,
                                10.0,
                                0.1,
                            );
                            changed |= u64_field(
                                ui,
                                "Search timeout (ms)",
                                &mut config.general.incremental_search_timeout_ms,
                                &general_default.incremental_search_timeout_ms,
                                100,
                                30000,
                            );

                            if ui
                                .add_enabled(
                                    config.general.repaint_secs != general_default.repaint_secs
                                        || config.general.incremental_search_timeout_ms
                                            != general_default.incremental_search_timeout_ms,
                                    egui::Button::new("Reset section"),
                                )
                                .clicked()
                            {
                                // Preserve extra and window/volume fields.
                                config.general.repaint_secs = general_default.repaint_secs;
                                config.general.incremental_search_timeout_ms =
                                    general_default.incremental_search_timeout_ms;
                                changed = true;
                            }
                        });

                    ui.add_space(8.0);

                    // ── Keybindings ──────────────────────────────────
                    let kb_default = Keybindings::default();
                    CollapsingHeader::new(RichText::new("Keybindings").heading())
                        .default_open(false)
                        .show(ui, |ui| {
                            changed |= text_field(
                                ui,
                                "Global search",
                                &mut config.keybindings.global_search,
                                &kb_default.global_search,
                                false,
                            );
                            changed |= text_field(
                                ui,
                                "Global mini library",
                                &mut config.keybindings.global_mini_library,
                                &kb_default.global_mini_library,
                                false,
                            );
                            changed |= text_field(
                                ui,
                                "Local search",
                                &mut config.keybindings.local_search,
                                &kb_default.local_search,
                                false,
                            );
                            changed |= text_field(
                                ui,
                                "Local lyrics",
                                &mut config.keybindings.local_lyrics,
                                &kb_default.local_lyrics,
                                false,
                            );
                            changed |= text_field(
                                ui,
                                "Mouse previous track",
                                &mut config.keybindings.mouse_previous_track,
                                &kb_default.mouse_previous_track,
                                false,
                            );
                            changed |= text_field(
                                ui,
                                "Mouse next track",
                                &mut config.keybindings.mouse_next_track,
                                &kb_default.mouse_next_track,
                                false,
                            );

                            if ui
                                .add_enabled(
                                    config.keybindings != kb_default,
                                    egui::Button::new("Reset section"),
                                )
                                .clicked()
                            {
                                config.keybindings = kb_default;
                                changed = true;
                            }
                        });
                });
        });

    server_changed
}

/// Converts a snake_case identifier to a human-readable label.
fn human_readable_label(name: &str) -> String {
    let mut result = String::with_capacity(name.len());
    for (i, c) in name.chars().enumerate() {
        if i == 0 {
            result.push(c.to_ascii_uppercase());
        } else if c == '_' {
            result.push(' ');
        } else {
            result.push(c);
        }
    }
    result
}

/// A text field with reset button. Returns `true` if the value changed.
fn text_field(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut String,
    default: &str,
    _password: bool,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        if ui
            .add(TextEdit::singleline(value).desired_width(200.0))
            .changed()
        {
            changed = true;
        }
        if *value != default
            && ui
                .small_button("\u{21BA}")
                .on_hover_text("Reset to default")
                .clicked()
        {
            *value = default.to_string();
            changed = true;
        }
    });
    changed
}

/// A password field with show/hide toggle and reset button. Returns `true` if the value changed.
fn password_field(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut String,
    default: &str,
    show_password: &mut bool,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        if ui
            .add(
                TextEdit::singleline(value)
                    .password(!*show_password)
                    .desired_width(200.0),
            )
            .changed()
        {
            changed = true;
        }
        if ui
            .selectable_label(
                *show_password,
                if *show_password {
                    "\u{1F441}"
                } else {
                    "\u{1F441}\u{200D}\u{1F5E8}"
                },
            )
            .on_hover_text(if *show_password {
                "Hide password"
            } else {
                "Show password"
            })
            .clicked()
        {
            *show_password = !*show_password;
        }
        if *value != default
            && ui
                .small_button("\u{21BA}")
                .on_hover_text("Reset to default")
                .clicked()
        {
            *value = default.to_string();
            changed = true;
        }
    });
    changed
}

/// A bool field with reset button. Returns `true` if the value changed.
fn bool_field(ui: &mut egui::Ui, label: &str, value: &mut bool, default: &bool) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        if ui.checkbox(value, label).changed() {
            changed = true;
        }
        if value != default
            && ui
                .small_button("\u{21BA}")
                .on_hover_text("Reset to default")
                .clicked()
        {
            *value = *default;
            changed = true;
        }
    });
    changed
}

/// An enum field (AlbumArtStyle) with reset button. Returns `true` if the value changed.
fn enum_field(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut AlbumArtStyle,
    default: &AlbumArtStyle,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        ComboBox::from_id_salt(label)
            .selected_text(value.as_str())
            .show_ui(ui, |ui| {
                for variant in AlbumArtStyle::ALL {
                    if ui
                        .selectable_value(value, *variant, variant.as_str())
                        .changed()
                    {
                        changed = true;
                    }
                }
            });
        if value != default
            && ui
                .small_button("\u{21BA}")
                .on_hover_text("Reset to default")
                .clicked()
        {
            *value = *default;
            changed = true;
        }
    });
    changed
}

/// A usize field with DragValue and reset button. Returns `true` if the value changed.
fn usize_field(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut usize,
    default: &usize,
    min: usize,
    max: usize,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        if ui.add(DragValue::new(value).range(min..=max)).changed() {
            changed = true;
        }
        if value != default
            && ui
                .small_button("\u{21BA}")
                .on_hover_text("Reset to default")
                .clicked()
        {
            *value = *default;
            changed = true;
        }
    });
    changed
}

/// An f32 field with DragValue and reset button. Returns `true` if the value changed.
fn f32_field(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f32,
    default: &f32,
    min: f32,
    max: f32,
    speed: f32,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        if ui
            .add(DragValue::new(value).range(min..=max).speed(speed))
            .changed()
        {
            changed = true;
        }
        if *value != *default
            && ui
                .small_button("\u{21BA}")
                .on_hover_text("Reset to default")
                .clicked()
        {
            *value = *default;
            changed = true;
        }
    });
    changed
}

/// A u64 field with DragValue and reset button. Returns `true` if the value changed.
fn u64_field(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut u64,
    default: &u64,
    min: u64,
    max: u64,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        if ui.add(DragValue::new(value).range(min..=max)).changed() {
            changed = true;
        }
        if value != default
            && ui
                .small_button("\u{21BA}")
                .on_hover_text("Reset to default")
                .clicked()
        {
            *value = *default;
            changed = true;
        }
    });
    changed
}
