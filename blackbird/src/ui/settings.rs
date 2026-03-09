use egui::{
    CollapsingHeader, ComboBox, Context, DragValue, RichText, ScrollArea, TextEdit, Vec2, Vec2b,
    Window, ecolor::Hsva,
};

use blackbird_client_shared::{config::AlbumArtStyle, style as shared_style};

use crate::config::{Config, General, Keybindings};

/// Fixed width for the label column, wide enough for the longest label.
const LABEL_WIDTH: f32 = 200.0;

/// Fixed width for the widget column.
const WIDGET_WIDTH: f32 = 200.0;

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
        .default_size(Vec2::new(560.0, 600.0))
        .collapsible(false)
        .show(ctx, |ui| {
            ScrollArea::vertical()
                .auto_shrink(Vec2b::FALSE)
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());

                    // ── Server ──────────────────────────────────────
                    let server_default = blackbird_shared::config::Server::default();
                    section(ui, "Server", |ui| {
                        ui.label(RichText::new("Changes reload the library.").small().weak());
                        ui.add_space(2.0);

                        server_changed |= text_row(
                            ui,
                            "Base URL",
                            &mut config.shared.server.base_url,
                            &server_default.base_url,
                        );
                        server_changed |= text_row(
                            ui,
                            "Username",
                            &mut config.shared.server.username,
                            &server_default.username,
                        );
                        server_changed |= password_row(
                            ui,
                            "Password",
                            &mut config.shared.server.password,
                            &server_default.password,
                            &mut settings.show_password,
                        );
                        server_changed |= bool_row(
                            ui,
                            "Transcode",
                            &mut config.shared.server.transcode,
                            &server_default.transcode,
                        );

                        reset_section_button(ui, config.shared.server != server_default, || {
                            config.shared.server = server_default;
                            server_changed = true;
                        });
                    });

                    // ── Layout ──────────────────────────────────────
                    let layout_default = blackbird_client_shared::config::Layout::default();
                    section(ui, "Layout", |ui| {
                        changed |= bool_row(
                            ui,
                            "Show inline lyrics",
                            &mut config.shared.layout.show_inline_lyrics,
                            &layout_default.show_inline_lyrics,
                        );
                        changed |= enum_row(
                            ui,
                            "Album art style",
                            &mut config.shared.layout.album_art_style,
                            &layout_default.album_art_style,
                        );
                        changed |= usize_row(
                            ui,
                            "Album spacing",
                            &mut config.shared.layout.album_spacing,
                            &layout_default.album_spacing,
                            0,
                            10,
                        );

                        reset_section_button(ui, config.shared.layout != layout_default, || {
                            config.shared.layout = layout_default;
                            changed = true;
                        });
                    });

                    // ── Colors ──────────────────────────────────────
                    let style_default = shared_style::Style::default();
                    CollapsingHeader::new(RichText::new("Colors").heading())
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.add_space(2.0);

                            // Two-column grid of color swatches.
                            ui.columns(2, |cols| {
                                let mid = shared_style::Style::FIELD_COUNT.div_ceil(2);
                                for (col_idx, col) in cols.iter_mut().enumerate() {
                                    let start = col_idx * mid;
                                    let end = (start + mid).min(shared_style::Style::FIELD_COUNT);
                                    for i in start..end {
                                        let (_, human_label) = shared_style::Style::FIELD_NAMES[i];
                                        let default_hsv = shared_style::Style::default_field(i);
                                        let current = config.style.field_mut(i);
                                        let label = human_readable_label(human_label);

                                        col.horizontal(|ui| {
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

                                            reset_field_button(ui, *current != default_hsv, || {
                                                *current = default_hsv;
                                                changed = true;
                                            });
                                        });
                                    }
                                }
                            });

                            reset_section_button(ui, config.style != style_default, || {
                                config.style = style_default;
                                changed = true;
                            });
                        });

                    ui.add_space(4.0);

                    // ── General ──────────────────────────────────────
                    let general_default = General::default();
                    let layout_default = blackbird_client_shared::config::Layout::default();
                    CollapsingHeader::new(RichText::new("General").heading())
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.add_space(2.0);

                            changed |= f32_row(
                                ui,
                                "Scroll multiplier",
                                &mut config.shared.layout.scroll_multiplier,
                                &layout_default.scroll_multiplier,
                                1.0,
                                200.0,
                                1.0,
                            );
                            changed |= f32_row(
                                ui,
                                "Repaint interval (s)",
                                &mut config.general.repaint_secs,
                                &general_default.repaint_secs,
                                0.1,
                                10.0,
                                0.1,
                            );
                            changed |= u64_row(
                                ui,
                                "Search timeout (ms)",
                                &mut config.general.incremental_search_timeout_ms,
                                &general_default.incremental_search_timeout_ms,
                                100,
                                30000,
                            );

                            reset_section_button(
                                ui,
                                config.shared.layout.scroll_multiplier
                                    != layout_default.scroll_multiplier
                                    || config.general.repaint_secs != general_default.repaint_secs
                                    || config.general.incremental_search_timeout_ms
                                        != general_default.incremental_search_timeout_ms,
                                || {
                                    config.shared.layout.scroll_multiplier =
                                        layout_default.scroll_multiplier;
                                    config.general.repaint_secs = general_default.repaint_secs;
                                    config.general.incremental_search_timeout_ms =
                                        general_default.incremental_search_timeout_ms;
                                    changed = true;
                                },
                            );
                        });

                    ui.add_space(4.0);

                    // ── Keybindings ──────────────────────────────────
                    let kb_default = Keybindings::default();
                    CollapsingHeader::new(RichText::new("Keybindings").heading())
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.add_space(2.0);

                            changed |= text_row(
                                ui,
                                "Global search",
                                &mut config.keybindings.global_search,
                                &kb_default.global_search,
                            );
                            changed |= text_row(
                                ui,
                                "Global mini library",
                                &mut config.keybindings.global_mini_library,
                                &kb_default.global_mini_library,
                            );
                            changed |= text_row(
                                ui,
                                "Local search",
                                &mut config.keybindings.local_search,
                                &kb_default.local_search,
                            );
                            changed |= text_row(
                                ui,
                                "Local lyrics",
                                &mut config.keybindings.local_lyrics,
                                &kb_default.local_lyrics,
                            );
                            changed |= text_row(
                                ui,
                                "Mouse previous track",
                                &mut config.keybindings.mouse_previous_track,
                                &kb_default.mouse_previous_track,
                            );
                            changed |= text_row(
                                ui,
                                "Mouse next track",
                                &mut config.keybindings.mouse_next_track,
                                &kb_default.mouse_next_track,
                            );

                            reset_section_button(ui, config.keybindings != kb_default, || {
                                config.keybindings = kb_default;
                                changed = true;
                            });
                        });
                });
        });

    server_changed
}

// ── Layout helpers ─────────────────────────────────────────────

/// Renders a collapsing section that is open by default.
fn section(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    CollapsingHeader::new(RichText::new(title).heading())
        .default_open(true)
        .show(ui, body);
    ui.add_space(4.0);
}

/// Renders a fixed-width label cell for the first column.
fn label_cell(ui: &mut egui::Ui, text: &str) {
    let height = ui.spacing().interact_size.y;
    ui.allocate_ui_with_layout(
        egui::vec2(LABEL_WIDTH, height),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.label(text);
        },
    );
}

/// Renders the small reset-to-default button for a single field.
/// Only visible when the value differs from the default.
fn reset_field_button(ui: &mut egui::Ui, is_modified: bool, on_reset: impl FnOnce()) {
    if is_modified {
        if ui
            .small_button("\u{21BA}")
            .on_hover_text("Reset to default")
            .clicked()
        {
            on_reset();
        }
    } else {
        // Reserve space so rows don't shift when the button appears/disappears.
        ui.allocate_space(egui::vec2(16.0, 0.0));
    }
}

/// Renders a "Reset section" button, right-aligned, enabled only when modified.
fn reset_section_button(ui: &mut egui::Ui, is_modified: bool, on_reset: impl FnOnce()) {
    ui.add_space(2.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
        if ui
            .add_enabled(is_modified, egui::Button::new("Reset section"))
            .clicked()
        {
            on_reset();
        }
    });
}

// ── Field row helpers ───────────────────────────────────────────

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

/// A text field row (label | input | reset). Returns `true` if the value changed.
fn text_row(ui: &mut egui::Ui, label: &str, value: &mut String, default: &str) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        label_cell(ui, label);
        if ui
            .add(TextEdit::singleline(value).desired_width(WIDGET_WIDTH))
            .changed()
        {
            changed = true;
        }
        reset_field_button(ui, *value != default, || {
            *value = default.to_string();
            changed = true;
        });
    });
    changed
}

/// A password field row (label | input + eye toggle | reset). Returns `true` if the value changed.
fn password_row(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut String,
    default: &str,
    show_password: &mut bool,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        label_cell(ui, label);
        if ui
            .add(
                TextEdit::singleline(value)
                    .password(!*show_password)
                    .desired_width(WIDGET_WIDTH - 28.0),
            )
            .changed()
        {
            changed = true;
        }
        let eye = if *show_password {
            "\u{1F441}"
        } else {
            "\u{1F441}\u{200D}\u{1F5E8}"
        };
        if ui
            .selectable_label(*show_password, eye)
            .on_hover_text(if *show_password {
                "Hide password"
            } else {
                "Show password"
            })
            .clicked()
        {
            *show_password = !*show_password;
        }
        reset_field_button(ui, *value != default, || {
            *value = default.to_string();
            changed = true;
        });
    });
    changed
}

/// A bool field row (label | checkbox | reset). Returns `true` if the value changed.
fn bool_row(ui: &mut egui::Ui, label: &str, value: &mut bool, default: &bool) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        label_cell(ui, label);
        if ui.checkbox(value, "").changed() {
            changed = true;
        }
        reset_field_button(ui, value != default, || {
            *value = *default;
            changed = true;
        });
    });
    changed
}

/// An enum field row (label | combo box | reset). Returns `true` if the value changed.
fn enum_row(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut AlbumArtStyle,
    default: &AlbumArtStyle,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        label_cell(ui, label);
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
        reset_field_button(ui, value != default, || {
            *value = *default;
            changed = true;
        });
    });
    changed
}

/// A usize field row (label | drag value | reset). Returns `true` if the value changed.
fn usize_row(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut usize,
    default: &usize,
    min: usize,
    max: usize,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        label_cell(ui, label);
        if ui.add(DragValue::new(value).range(min..=max)).changed() {
            changed = true;
        }
        reset_field_button(ui, value != default, || {
            *value = *default;
            changed = true;
        });
    });
    changed
}

/// An f32 field row (label | drag value | reset). Returns `true` if the value changed.
fn f32_row(
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
        label_cell(ui, label);
        if ui
            .add(DragValue::new(value).range(min..=max).speed(speed))
            .changed()
        {
            changed = true;
        }
        reset_field_button(ui, *value != *default, || {
            *value = *default;
            changed = true;
        });
    });
    changed
}

/// A u64 field row (label | drag value | reset). Returns `true` if the value changed.
fn u64_row(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut u64,
    default: &u64,
    min: u64,
    max: u64,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        label_cell(ui, label);
        if ui.add(DragValue::new(value).range(min..=max)).changed() {
            changed = true;
        }
        reset_field_button(ui, value != default, || {
            *value = *default;
            changed = true;
        });
    });
    changed
}
