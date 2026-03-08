use std::{collections::HashMap, sync::LazyLock};

use blackbird_client_shared::{
    config::{AlbumArtStyle, Layout},
    style as shared_style,
};
use blackbird_core::blackbird_state::{AlbumId, CoverArtId, TrackId};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout as RatatuiLayout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
};

use crate::{
    cover_art::{ArtColorGrid, ArtColors, compute_art_grid, compute_quadrant_colors},
    keys::Action,
};

use super::{
    StyleExt,
    library::{EntryRenderContext, LibraryEntry, render_library_entry},
};

/// Actions returned to the caller so `app.rs` can apply side effects.
pub enum SettingsAction {
    ToggleSettings,
}

/// Which HSV component is being edited in the color picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HsvComponent {
    H,
    S,
    V,
}

/// Represents one row in the settings list.
#[derive(Debug, Clone)]
enum SettingsRow {
    /// Blank line between sections for visual spacing.
    SectionSpacer,
    SectionHeader(&'static str),
    BoolField {
        label: &'static str,
        section: Section,
        get: fn(&crate::config::Config) -> bool,
        set: fn(&mut crate::config::Config, bool),
        default: fn() -> bool,
    },
    StringField {
        label: &'static str,
        section: Section,
        get: fn(&crate::config::Config) -> String,
        set: fn(&mut crate::config::Config, String),
        default: fn() -> String,
        password: bool,
    },
    UsizeField {
        label: &'static str,
        section: Section,
        get: fn(&crate::config::Config) -> usize,
        set: fn(&mut crate::config::Config, usize),
        default: fn() -> usize,
        min: usize,
        max: usize,
    },
    F32Field {
        label: &'static str,
        section: Section,
        get: fn(&crate::config::Config) -> f32,
        set: fn(&mut crate::config::Config, f32),
        default: fn() -> f32,
        min: f32,
        max: f32,
    },
    U64Field {
        label: &'static str,
        section: Section,
        get: fn(&crate::config::Config) -> u64,
        set: fn(&mut crate::config::Config, u64),
        default: fn() -> u64,
        min: u64,
        max: u64,
    },
    EnumField {
        label: &'static str,
        section: Section,
        get: fn(&crate::config::Config) -> AlbumArtStyle,
        set: fn(&mut crate::config::Config, AlbumArtStyle),
        default: fn() -> AlbumArtStyle,
    },
    HsvField {
        label: &'static str,
        index: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Server,
    Layout,
    Colors,
    General,
}

pub struct SettingsState {
    pub selected_index: usize,
    pub editing: bool,
    pub edit_buffer: String,
    pub hsv_component: HsvComponent,
    rows: Vec<SettingsRow>,
    pub scroll_offset: usize,
    /// The inner area of the settings list from the last draw, used for mouse
    /// hit-testing.
    pub last_inner_area: Option<Rect>,
    /// The last visible height of the settings list.
    pub last_visible_height: usize,
}

impl SettingsState {
    pub fn new() -> Self {
        let rows = build_rows();
        // Find the first selectable row (skip spacers and headers).
        let initial_index = rows
            .iter()
            .position(|r| {
                !matches!(
                    r,
                    SettingsRow::SectionSpacer | SettingsRow::SectionHeader(_)
                )
            })
            .unwrap_or(0);
        Self {
            selected_index: initial_index,
            editing: false,
            edit_buffer: String::new(),
            hsv_component: HsvComponent::H,
            rows,
            scroll_offset: 0,
            last_inner_area: None,
            last_visible_height: 0,
        }
    }

    pub fn reset(&mut self) {
        let initial_index = self
            .rows
            .iter()
            .position(|r| {
                !matches!(
                    r,
                    SettingsRow::SectionSpacer | SettingsRow::SectionHeader(_)
                )
            })
            .unwrap_or(0);
        self.selected_index = initial_index;
        self.editing = false;
        self.edit_buffer.clear();
        self.hsv_component = HsvComponent::H;
        self.scroll_offset = 0;
    }
}

fn build_rows() -> Vec<SettingsRow> {
    let mut rows = vec![
        // Server section (no spacer before the first section).
        SettingsRow::SectionHeader("Server (changes reload library)"),
        SettingsRow::StringField {
            label: "Base URL",
            section: Section::Server,
            get: |c| c.shared.server.base_url.clone(),
            set: |c, v| c.shared.server.base_url = v,
            default: || blackbird_shared::config::Server::default().base_url,
            password: false,
        },
        SettingsRow::StringField {
            label: "Username",
            section: Section::Server,
            get: |c| c.shared.server.username.clone(),
            set: |c, v| c.shared.server.username = v,
            default: || blackbird_shared::config::Server::default().username,
            password: false,
        },
        SettingsRow::StringField {
            label: "Password",
            section: Section::Server,
            get: |c| c.shared.server.password.clone(),
            set: |c, v| c.shared.server.password = v,
            default: || blackbird_shared::config::Server::default().password,
            password: true,
        },
        SettingsRow::BoolField {
            label: "Transcode",
            section: Section::Server,
            get: |c| c.shared.server.transcode,
            set: |c, v| c.shared.server.transcode = v,
            default: || blackbird_shared::config::Server::default().transcode,
        },
        // Layout section.
        SettingsRow::SectionSpacer,
        SettingsRow::SectionHeader("Layout"),
        SettingsRow::BoolField {
            label: "Show inline lyrics",
            section: Section::Layout,
            get: |c| c.shared.layout.show_inline_lyrics,
            set: |c, v| c.shared.layout.show_inline_lyrics = v,
            default: || Layout::default().show_inline_lyrics,
        },
        SettingsRow::EnumField {
            label: "Album art style",
            section: Section::Layout,
            get: |c| c.shared.layout.album_art_style,
            set: |c, v| c.shared.layout.album_art_style = v,
            default: || Layout::default().album_art_style,
        },
        SettingsRow::UsizeField {
            label: "Album spacing",
            section: Section::Layout,
            get: |c| c.shared.layout.album_spacing,
            set: |c, v| c.shared.layout.album_spacing = v,
            default: || Layout::default().album_spacing,
            min: 0,
            max: 10,
        },
        // Colors section.
        SettingsRow::SectionSpacer,
        SettingsRow::SectionHeader("Colors"),
    ];

    // HSV color fields are generated dynamically from the style macro.
    for i in 0..shared_style::Style::FIELD_COUNT {
        let (_, human_label) = shared_style::Style::FIELD_NAMES[i];
        rows.push(SettingsRow::HsvField {
            label: human_label,
            index: i,
        });
    }

    rows.extend([
        SettingsRow::F32Field {
            label: "Scroll multiplier",
            section: Section::Colors,
            get: |c| c.style.scroll_multiplier,
            set: |c, v| c.style.scroll_multiplier = v,
            default: || shared_style::Style::default().scroll_multiplier,
            min: 1.0,
            max: 200.0,
        },
        // General section.
        SettingsRow::SectionSpacer,
        SettingsRow::SectionHeader("General"),
        SettingsRow::U64Field {
            label: "Tick rate (ms)",
            section: Section::General,
            get: |c| c.general.tick_rate_ms,
            set: |c, v| c.general.tick_rate_ms = v,
            default: || crate::config::General::default().tick_rate_ms,
            min: 10,
            max: 1000,
        },
    ]);

    rows
}

/// Returns `true` for rows that cannot be selected (spacers and section headers).
fn is_non_selectable(row: &SettingsRow) -> bool {
    matches!(
        row,
        SettingsRow::SectionSpacer | SettingsRow::SectionHeader(_)
    )
}

/// Moves selection by `delta` rows, skipping non-selectable rows.
fn move_selection(state: &mut SettingsState, delta: i32) {
    let len = state.rows.len();
    let mut idx = state.selected_index as i32 + delta;
    // Skip non-selectable rows in the direction of movement.
    while idx >= 0 && (idx as usize) < len && is_non_selectable(&state.rows[idx as usize]) {
        idx += delta.signum();
    }
    if idx >= 0 && (idx as usize) < len && !is_non_selectable(&state.rows[idx as usize]) {
        state.selected_index = idx as usize;
    }
}

/// Selects the row at `idx` if it is selectable. If it falls on a non-selectable
/// row, searches downward then upward for the nearest selectable row.
fn select_nearest(state: &mut SettingsState, idx: usize) {
    let len = state.rows.len();
    if idx >= len {
        return;
    }
    if !is_non_selectable(&state.rows[idx]) {
        state.selected_index = idx;
        return;
    }
    // Search downward.
    for i in (idx + 1)..len {
        if !is_non_selectable(&state.rows[i]) {
            state.selected_index = i;
            return;
        }
    }
    // Search upward.
    for i in (0..idx).rev() {
        if !is_non_selectable(&state.rows[i]) {
            state.selected_index = i;
            return;
        }
    }
}

pub fn draw(
    frame: &mut Frame,
    state: &mut SettingsState,
    style: &shared_style::Style,
    config: &crate::config::Config,
    area: Rect,
) {
    // Split into settings list (left) and library preview (right).
    let chunks = RatatuiLayout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    draw_settings_list(frame, state, style, config, chunks[0]);
    draw_library_preview(frame, style, config, chunks[1]);
}

fn draw_settings_list(
    frame: &mut Frame,
    state: &mut SettingsState,
    style: &shared_style::Style,
    config: &crate::config::Config,
    area: Rect,
) {
    let block = Block::default()
        .title(" Settings ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(style.album_color()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let visible_height = inner.height as usize;
    let mut items: Vec<ListItem> = Vec::with_capacity(state.rows.len());

    for (idx, row) in state.rows.iter().enumerate() {
        let is_selected = idx == state.selected_index;
        let line = render_row(row, config, style, is_selected, state);
        items.push(ListItem::new(line));
    }

    let list = List::new(items);
    let mut list_state = ListState::default();
    list_state.select(Some(state.selected_index));
    let offset = state.selected_index.saturating_sub(visible_height / 2);
    *list_state.offset_mut() = offset;

    // Store layout info for mouse hit-testing.
    state.last_inner_area = Some(inner);
    state.last_visible_height = visible_height;
    state.scroll_offset = offset;

    frame.render_stateful_widget(list, inner, &mut list_state);
}

fn render_row(
    row: &SettingsRow,
    config: &crate::config::Config,
    style: &shared_style::Style,
    is_selected: bool,
    state: &SettingsState,
) -> Line<'static> {
    let highlight = style.track_name_playing_color();
    let text_fg = style.text_color();
    let dim_fg = style.track_duration_color();

    match row {
        SettingsRow::SectionSpacer => Line::from(""),
        SettingsRow::SectionHeader(label) => Line::from(Span::styled(
            format!("── {label} ──"),
            Style::default()
                .fg(style.album_color())
                .add_modifier(Modifier::BOLD),
        )),
        SettingsRow::BoolField {
            label,
            get,
            default,
            ..
        } => {
            let value = get(config);
            let is_default = value == default();
            let indicator = if is_selected { "> " } else { "  " };
            let check = if value { "[x]" } else { "[ ]" };
            let mut spans = vec![
                Span::styled(
                    indicator.to_string(),
                    Style::default().fg(if is_selected { highlight } else { text_fg }),
                ),
                Span::styled(
                    format!("{check} {label}"),
                    Style::default().fg(if is_selected { highlight } else { text_fg }),
                ),
            ];
            if !is_default {
                spans.push(Span::styled(" *", Style::default().fg(dim_fg)));
            }
            Line::from(spans)
        }
        SettingsRow::StringField {
            label,
            get,
            default,
            password,
            ..
        } => {
            let value = get(config);
            let is_default = value == default();
            let indicator = if is_selected { "> " } else { "  " };
            let display_value = if is_selected && state.editing {
                state.edit_buffer.clone()
            } else if *password {
                "*".repeat(value.len())
            } else {
                value
            };
            let mut spans = vec![
                Span::styled(
                    indicator.to_string(),
                    Style::default().fg(if is_selected { highlight } else { text_fg }),
                ),
                Span::styled(
                    format!("{label}: "),
                    Style::default().fg(if is_selected { highlight } else { text_fg }),
                ),
                Span::styled(
                    display_value,
                    Style::default().fg(if is_selected && state.editing {
                        highlight
                    } else {
                        text_fg
                    }),
                ),
            ];
            if is_selected && state.editing {
                spans.push(Span::styled("_", Style::default().fg(highlight)));
            }
            if !is_default {
                spans.push(Span::styled(" *", Style::default().fg(dim_fg)));
            }
            Line::from(spans)
        }
        SettingsRow::UsizeField {
            label,
            get,
            default,
            ..
        } => {
            let value = get(config);
            let is_default = value == default();
            let indicator = if is_selected { "> " } else { "  " };
            let display_value = if is_selected && state.editing {
                state.edit_buffer.clone()
            } else {
                value.to_string()
            };
            let mut spans = vec![
                Span::styled(
                    indicator.to_string(),
                    Style::default().fg(if is_selected { highlight } else { text_fg }),
                ),
                Span::styled(
                    format!("{label}: "),
                    Style::default().fg(if is_selected { highlight } else { text_fg }),
                ),
                Span::styled(display_value, Style::default().fg(text_fg)),
            ];
            if is_selected && state.editing {
                spans.push(Span::styled("_", Style::default().fg(highlight)));
            }
            if !is_default {
                spans.push(Span::styled(" *", Style::default().fg(dim_fg)));
            }
            Line::from(spans)
        }
        SettingsRow::F32Field {
            label,
            get,
            default,
            ..
        } => {
            let value = get(config);
            let is_default = (value - default()).abs() < f32::EPSILON;
            let indicator = if is_selected { "> " } else { "  " };
            let display_value = if is_selected && state.editing {
                state.edit_buffer.clone()
            } else {
                format!("{value:.1}")
            };
            let mut spans = vec![
                Span::styled(
                    indicator.to_string(),
                    Style::default().fg(if is_selected { highlight } else { text_fg }),
                ),
                Span::styled(
                    format!("{label}: "),
                    Style::default().fg(if is_selected { highlight } else { text_fg }),
                ),
                Span::styled(display_value, Style::default().fg(text_fg)),
            ];
            if is_selected && state.editing {
                spans.push(Span::styled("_", Style::default().fg(highlight)));
            }
            if !is_default {
                spans.push(Span::styled(" *", Style::default().fg(dim_fg)));
            }
            Line::from(spans)
        }
        SettingsRow::U64Field {
            label,
            get,
            default,
            ..
        } => {
            let value = get(config);
            let is_default = value == default();
            let indicator = if is_selected { "> " } else { "  " };
            let display_value = if is_selected && state.editing {
                state.edit_buffer.clone()
            } else {
                value.to_string()
            };
            let mut spans = vec![
                Span::styled(
                    indicator.to_string(),
                    Style::default().fg(if is_selected { highlight } else { text_fg }),
                ),
                Span::styled(
                    format!("{label}: "),
                    Style::default().fg(if is_selected { highlight } else { text_fg }),
                ),
                Span::styled(display_value, Style::default().fg(text_fg)),
            ];
            if is_selected && state.editing {
                spans.push(Span::styled("_", Style::default().fg(highlight)));
            }
            if !is_default {
                spans.push(Span::styled(" *", Style::default().fg(dim_fg)));
            }
            Line::from(spans)
        }
        SettingsRow::EnumField {
            label,
            get,
            default,
            ..
        } => {
            let value = get(config);
            let is_default = value == default();
            let indicator = if is_selected { "> " } else { "  " };
            let mut spans = vec![
                Span::styled(
                    indicator.to_string(),
                    Style::default().fg(if is_selected { highlight } else { text_fg }),
                ),
                Span::styled(
                    format!("{label}: {}", value.as_str()),
                    Style::default().fg(if is_selected { highlight } else { text_fg }),
                ),
            ];
            if !is_default {
                spans.push(Span::styled(" *", Style::default().fg(dim_fg)));
            }
            Line::from(spans)
        }
        SettingsRow::HsvField { label, index } => {
            let hsv = *config.style.field(*index);
            let default_hsv = shared_style::Style::default_field(*index);
            let is_default = hsv == default_hsv;
            let indicator = if is_selected { "> " } else { "  " };

            // Convert HSV to an RGB swatch for preview.
            let swatch_color = super::hsv_to_color(hsv);

            let mut spans = vec![Span::styled(
                indicator.to_string(),
                Style::default().fg(if is_selected { highlight } else { text_fg }),
            )];

            // Color swatch.
            spans.push(Span::styled(
                "\u{2588}\u{2588}",
                Style::default().fg(swatch_color),
            ));
            spans.push(Span::raw(" "));

            let label_str = human_readable_label(label);

            if is_selected && state.editing {
                // Show editable H/S/V with the active component highlighted.
                spans.push(Span::styled(
                    format!("{label_str}: "),
                    Style::default().fg(highlight),
                ));
                let components = [
                    ("H", hsv[0], HsvComponent::H),
                    ("S", hsv[1], HsvComponent::S),
                    ("V", hsv[2], HsvComponent::V),
                ];
                for (i, (name, val, comp)) in components.iter().enumerate() {
                    if i > 0 {
                        spans.push(Span::raw(" "));
                    }
                    let is_active = state.hsv_component == *comp;
                    spans.push(Span::styled(
                        format!("{name}:{val:.2}"),
                        Style::default()
                            .fg(if is_active { highlight } else { text_fg })
                            .add_modifier(if is_active {
                                Modifier::BOLD | Modifier::UNDERLINED
                            } else {
                                Modifier::empty()
                            }),
                    ));
                }
            } else {
                spans.push(Span::styled(
                    format!(
                        "{label_str}: H:{:.2} S:{:.2} V:{:.2}",
                        hsv[0], hsv[1], hsv[2]
                    ),
                    Style::default().fg(if is_selected { highlight } else { text_fg }),
                ));
            }

            if !is_default {
                spans.push(Span::styled(" *", Style::default().fg(dim_fg)));
            }
            Line::from(spans)
        }
    }
}

/// Handle a key event in the settings panel. Returns an action if the caller
/// needs to toggle/close the panel or quit, and a bool indicating whether
/// the server config changed (requiring library reload).
pub fn handle_key(
    state: &mut SettingsState,
    config: &mut crate::config::Config,
    action: Action,
) -> (Option<SettingsAction>, bool) {
    let mut server_changed = false;

    if state.editing {
        match action {
            Action::Back => {
                // Cancel editing.
                state.editing = false;
                state.edit_buffer.clear();
                return (None, false);
            }
            Action::Select => {
                // Confirm editing.
                let row = &state.rows[state.selected_index];
                match row {
                    SettingsRow::StringField { set, section, .. } => {
                        set(config, state.edit_buffer.clone());
                        if *section == Section::Server {
                            server_changed = true;
                        }
                    }
                    SettingsRow::UsizeField {
                        set,
                        min,
                        max,
                        section,
                        ..
                    } => {
                        if let Ok(v) = state.edit_buffer.parse::<usize>() {
                            set(config, v.clamp(*min, *max));
                            if *section == Section::Server {
                                server_changed = true;
                            }
                        }
                    }
                    SettingsRow::F32Field {
                        set,
                        min,
                        max,
                        section,
                        ..
                    } => {
                        if let Ok(v) = state.edit_buffer.parse::<f32>() {
                            set(config, v.clamp(*min, *max));
                            if *section == Section::Server {
                                server_changed = true;
                            }
                        }
                    }
                    SettingsRow::U64Field {
                        set,
                        min,
                        max,
                        section,
                        ..
                    } => {
                        if let Ok(v) = state.edit_buffer.parse::<u64>() {
                            set(config, v.clamp(*min, *max));
                            if *section == Section::Server {
                                server_changed = true;
                            }
                        }
                    }
                    SettingsRow::HsvField { .. } => {
                        // HSV editing confirms on Enter — values are already applied live.
                    }
                    _ => {}
                }
                state.editing = false;
                state.edit_buffer.clear();
                return (None, server_changed);
            }
            Action::Char(c) => {
                let row = &state.rows[state.selected_index];
                if matches!(row, SettingsRow::HsvField { .. }) {
                    // In HSV edit mode, ignore character input.
                } else {
                    state.edit_buffer.push(c);
                }
                return (None, false);
            }
            // When editing a text/number field, treat d/D as regular characters.
            Action::ResetField => {
                let row = &state.rows[state.selected_index];
                if !matches!(row, SettingsRow::HsvField { .. }) {
                    state.edit_buffer.push('d');
                }
                return (None, false);
            }
            Action::ResetSection => {
                let row = &state.rows[state.selected_index];
                if !matches!(row, SettingsRow::HsvField { .. }) {
                    state.edit_buffer.push('D');
                }
                return (None, false);
            }
            Action::DeleteChar => {
                state.edit_buffer.pop();
                return (None, false);
            }
            Action::MoveLeft => {
                let row = &state.rows[state.selected_index];
                if let SettingsRow::HsvField { .. } = row {
                    state.hsv_component = match state.hsv_component {
                        HsvComponent::H => HsvComponent::V,
                        HsvComponent::S => HsvComponent::H,
                        HsvComponent::V => HsvComponent::S,
                    };
                }
                return (None, false);
            }
            Action::MoveRight => {
                let row = &state.rows[state.selected_index];
                if let SettingsRow::HsvField { .. } = row {
                    state.hsv_component = match state.hsv_component {
                        HsvComponent::H => HsvComponent::S,
                        HsvComponent::S => HsvComponent::V,
                        HsvComponent::V => HsvComponent::H,
                    };
                }
                return (None, false);
            }
            Action::MoveUp => {
                let row = &state.rows[state.selected_index];
                if let SettingsRow::HsvField { index, .. } = row {
                    let hsv = config.style.field_mut(*index);
                    let comp_idx = match state.hsv_component {
                        HsvComponent::H => 0,
                        HsvComponent::S => 1,
                        HsvComponent::V => 2,
                    };
                    hsv[comp_idx] = (hsv[comp_idx] + 0.01).min(1.0);
                }
                return (None, false);
            }
            Action::MoveDown => {
                let row = &state.rows[state.selected_index];
                if let SettingsRow::HsvField { index, .. } = row {
                    let hsv = config.style.field_mut(*index);
                    let comp_idx = match state.hsv_component {
                        HsvComponent::H => 0,
                        HsvComponent::S => 1,
                        HsvComponent::V => 2,
                    };
                    hsv[comp_idx] = (hsv[comp_idx] - 0.01).max(0.0);
                }
                return (None, false);
            }
            Action::PageUp => {
                let row = &state.rows[state.selected_index];
                if let SettingsRow::HsvField { index, .. } = row {
                    let hsv = config.style.field_mut(*index);
                    let comp_idx = match state.hsv_component {
                        HsvComponent::H => 0,
                        HsvComponent::S => 1,
                        HsvComponent::V => 2,
                    };
                    hsv[comp_idx] = (hsv[comp_idx] + 0.05).min(1.0);
                }
                return (None, false);
            }
            Action::PageDown => {
                let row = &state.rows[state.selected_index];
                if let SettingsRow::HsvField { index, .. } = row {
                    let hsv = config.style.field_mut(*index);
                    let comp_idx = match state.hsv_component {
                        HsvComponent::H => 0,
                        HsvComponent::S => 1,
                        HsvComponent::V => 2,
                    };
                    hsv[comp_idx] = (hsv[comp_idx] - 0.05).max(0.0);
                }
                return (None, false);
            }
            _ => return (None, false),
        }
    }

    match action {
        Action::Back => return (Some(SettingsAction::ToggleSettings), false),
        Action::MoveUp => {
            move_selection(state, -1);
        }
        Action::MoveDown => {
            move_selection(state, 1);
        }
        Action::Select => {
            let row = &state.rows[state.selected_index];
            match row {
                SettingsRow::SectionSpacer | SettingsRow::SectionHeader(_) => {}
                SettingsRow::BoolField {
                    get, set, section, ..
                } => {
                    let v = get(config);
                    set(config, !v);
                    if *section == Section::Server {
                        server_changed = true;
                    }
                }
                SettingsRow::StringField { get, .. } => {
                    state.editing = true;
                    state.edit_buffer = get(config);
                }
                SettingsRow::UsizeField { get, .. } => {
                    state.editing = true;
                    state.edit_buffer = get(config).to_string();
                }
                SettingsRow::F32Field { get, .. } => {
                    state.editing = true;
                    state.edit_buffer = format!("{:.1}", get(config));
                }
                SettingsRow::U64Field { get, .. } => {
                    state.editing = true;
                    state.edit_buffer = get(config).to_string();
                }
                SettingsRow::EnumField {
                    get, set, section, ..
                } => {
                    let current = get(config);
                    let all = AlbumArtStyle::ALL;
                    let idx = all.iter().position(|v| *v == current).unwrap_or(0);
                    let next = (idx + 1) % all.len();
                    set(config, all[next]);
                    if *section == Section::Server {
                        server_changed = true;
                    }
                }
                SettingsRow::HsvField { .. } => {
                    state.editing = true;
                    state.hsv_component = HsvComponent::H;
                }
            }
        }
        Action::ResetField => {
            // Reset field to default.
            let row = &state.rows[state.selected_index];
            match row {
                SettingsRow::BoolField {
                    default,
                    set,
                    section,
                    ..
                } => {
                    set(config, default());
                    if *section == Section::Server {
                        server_changed = true;
                    }
                }
                SettingsRow::StringField {
                    default,
                    set,
                    section,
                    ..
                } => {
                    set(config, default());
                    if *section == Section::Server {
                        server_changed = true;
                    }
                }
                SettingsRow::UsizeField {
                    default,
                    set,
                    section,
                    ..
                } => {
                    set(config, default());
                    if *section == Section::Server {
                        server_changed = true;
                    }
                }
                SettingsRow::F32Field {
                    default,
                    set,
                    section,
                    ..
                } => {
                    set(config, default());
                    if *section == Section::Server {
                        server_changed = true;
                    }
                }
                SettingsRow::U64Field {
                    default,
                    set,
                    section,
                    ..
                } => {
                    set(config, default());
                    if *section == Section::Server {
                        server_changed = true;
                    }
                }
                SettingsRow::EnumField {
                    default,
                    set,
                    section,
                    ..
                } => {
                    set(config, default());
                    if *section == Section::Server {
                        server_changed = true;
                    }
                }
                SettingsRow::HsvField { index, .. } => {
                    *config.style.field_mut(*index) = shared_style::Style::default_field(*index);
                }
                SettingsRow::SectionSpacer | SettingsRow::SectionHeader(_) => {}
            }
        }
        Action::ResetSection => {
            // Reset entire section.
            let row = &state.rows[state.selected_index];
            let section = match row {
                SettingsRow::SectionSpacer | SettingsRow::SectionHeader(_) => None,
                SettingsRow::BoolField { section, .. }
                | SettingsRow::StringField { section, .. }
                | SettingsRow::UsizeField { section, .. }
                | SettingsRow::F32Field { section, .. }
                | SettingsRow::U64Field { section, .. }
                | SettingsRow::EnumField { section, .. } => Some(*section),
                SettingsRow::HsvField { .. } => Some(Section::Colors),
            };
            if let Some(section) = section {
                match section {
                    Section::Server => {
                        config.shared.server = blackbird_shared::config::Server::default();
                        server_changed = true;
                    }
                    Section::Layout => {
                        config.shared.layout = Layout::default();
                    }
                    Section::Colors => {
                        config.style = shared_style::Style::default();
                    }
                    Section::General => {
                        let extra = config.general.extra.clone();
                        config.general = crate::config::General::default();
                        config.general.extra = extra;
                    }
                }
            }
        }
        _ => {}
    }

    (None, server_changed)
}

/// Handles a mouse click in the settings panel area. Selects the clicked row
/// and activates it (toggles bools, cycles enums, enters edit mode for text).
pub fn handle_mouse_click(
    state: &mut SettingsState,
    config: &mut crate::config::Config,
    _area: Rect,
    _x: u16,
    y: u16,
) -> bool {
    let Some(inner) = state.last_inner_area else {
        return false;
    };

    // Only handle clicks within the settings list (left half).
    if y < inner.y || y >= inner.y + inner.height {
        return false;
    }

    let row_in_list = (y - inner.y) as usize;
    let clicked_index = state.scroll_offset + row_in_list;

    if clicked_index >= state.rows.len() {
        return false;
    }

    // If we're already editing, clicking a different row cancels the edit.
    if state.editing && clicked_index != state.selected_index {
        state.editing = false;
        state.edit_buffer.clear();
    }

    // Select the clicked row (snapping to the nearest selectable row if needed).
    select_nearest(state, clicked_index);

    // Activate the clicked row (same as pressing Enter).
    let (_, server_changed) = handle_key(state, config, Action::Select);
    server_changed
}

/// Scrolls the settings selection by `delta` rows (used for mouse wheel).
pub fn scroll_selection(state: &mut SettingsState, delta: i32) {
    let steps = delta.unsigned_abs() as usize;
    let direction = delta.signum();
    for _ in 0..steps {
        move_selection(state, direction);
    }
}

/// Embedded placeholder image bytes for the preview art.
const PLACEHOLDER_IMAGE: &[u8] = include_bytes!("../../../blackbird/assets/no-album-art.png");

/// Sentinel cover art ID used by preview entries to look up placeholder art.
const PREVIEW_ART_ID: &str = "__preview_placeholder__";

/// Builds fake `LibraryEntry` values for the settings preview, using bird-themed
/// placeholder data. The entries include album gaps matching the configured spacing.
fn build_preview_entries(album_spacing: usize) -> Vec<LibraryEntry> {
    struct Album {
        artist: &'static str,
        album: &'static str,
        year: i32,
        duration: u32,
        starred: bool,
        tracks: &'static [Track],
    }
    struct Track {
        num: u32,
        title: &'static str,
        duration: u32,
        is_playing: bool,
    }

    const ALBUMS: &[Album] = &[
        Album {
            artist: "The Blackbirds",
            album: "Songs from the Nest",
            year: 2024,
            duration: 2527,
            starred: true,
            tracks: &[
                Track {
                    num: 1,
                    title: "Dawn Chorus",
                    duration: 263,
                    is_playing: true,
                },
                Track {
                    num: 2,
                    title: "Feather & Sky",
                    duration: 311,
                    is_playing: false,
                },
                Track {
                    num: 3,
                    title: "Wingspan",
                    duration: 362,
                    is_playing: false,
                },
                Track {
                    num: 4,
                    title: "Midnight Roost",
                    duration: 225,
                    is_playing: false,
                },
            ],
        },
        Album {
            artist: "Corvus & the Starlings",
            album: "Terminal Velocity",
            year: 2023,
            duration: 2331,
            starred: false,
            tracks: &[
                Track {
                    num: 1,
                    title: "Tailwind",
                    duration: 347,
                    is_playing: false,
                },
                Track {
                    num: 2,
                    title: "Hollow Bones",
                    duration: 258,
                    is_playing: false,
                },
                Track {
                    num: 3,
                    title: "Murmuration",
                    duration: 453,
                    is_playing: false,
                },
            ],
        },
    ];

    let art_id = CoverArtId(PREVIEW_ART_ID.into());
    let art_term_rows = super::layout::LARGE_ART_TERM_ROWS;
    let mut entries = Vec::new();

    // Track index used to find the "selected" track (third track overall).
    let mut playing_entry_index = None;
    let mut selected_entry_index = None;
    let mut track_counter = 0usize;

    for (album_idx, album) in ALBUMS.iter().enumerate() {
        if album_idx > 0 {
            for _ in 0..album_spacing {
                entries.push(LibraryEntry::AlbumGap);
            }
        }

        entries.push(LibraryEntry::GroupHeader {
            artist: album.artist.to_string(),
            album: album.album.to_string(),
            year: Some(album.year),
            created: None,
            duration: album.duration,
            starred: album.starred,
            album_id: AlbumId(format!("preview-album-{album_idx}").into()),
            cover_art_id: Some(art_id.clone()),
        });

        for (track_idx, track) in album.tracks.iter().enumerate() {
            let entry_idx = entries.len();
            if track.is_playing {
                playing_entry_index = Some(entry_idx);
            }
            // Select the third track overall.
            if track_counter == 2 {
                selected_entry_index = Some(entry_idx);
            }
            track_counter += 1;

            entries.push(LibraryEntry::Track {
                id: TrackId(format!("preview-track-{album_idx}-{track_idx}")),
                title: track.title.to_string(),
                artist: None,
                album_artist: album.artist.to_string(),
                track_number: Some(track.num),
                disc_number: None,
                duration: Some(track.duration),
                starred: false,
                play_count: None,
                cover_art_id: Some(art_id.clone()),
                track_index_in_group: track_idx,
            });
        }

        // Add spacer rows if needed for BelowAlbum art.
        let track_count = album.tracks.len();
        if track_count < art_term_rows {
            for spacer_idx in track_count..art_term_rows {
                entries.push(LibraryEntry::GroupSpacer {
                    cover_art_id: Some(art_id.clone()),
                    art_row_index: spacer_idx,
                });
            }
        }
    }

    // Store the playing/selected indices as a convention: the caller
    // reads these from the first two AlbumGap-free entries. We use a
    // simpler approach: return them embedded. Since we can't modify the
    // return type easily, the caller will compute them the same way.
    let _ = (playing_entry_index, selected_entry_index);

    entries
}

/// Draws a mini preview of the library using fake data to show the effect of
/// the current color/spacing configuration.
fn draw_library_preview(
    frame: &mut Frame,
    style: &shared_style::Style,
    config: &crate::config::Config,
    area: Rect,
) {
    let block = Block::default()
        .title(" Preview ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(style.album_color()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let album_art_style = config.shared.layout.album_art_style;
    let entries = build_preview_entries(config.shared.layout.album_spacing);

    // Build art lookup maps with the placeholder image.
    let art_id = CoverArtId(PREVIEW_ART_ID.into());

    static PLACEHOLDER_QUADRANT: LazyLock<ArtColors> =
        LazyLock::new(|| compute_quadrant_colors(PLACEHOLDER_IMAGE));

    let art_colors: HashMap<CoverArtId, ArtColors> = {
        let mut m = HashMap::new();
        m.insert(art_id.clone(), *PLACEHOLDER_QUADRANT);
        m
    };

    let large_art_cols = super::layout::large_art_cols() as usize;
    let large_art_pixel_rows = super::layout::LARGE_ART_TERM_ROWS * 2;

    let large_art_grids: HashMap<CoverArtId, ArtColorGrid> = {
        let mut m = HashMap::new();
        if album_art_style == AlbumArtStyle::BelowAlbum {
            m.insert(
                art_id,
                compute_art_grid(PLACEHOLDER_IMAGE, large_art_cols, large_art_pixel_rows),
            );
        }
        m
    };

    // Find the playing and selected entry indices.
    let playing_track_id = entries.iter().find_map(|e| {
        if let LibraryEntry::Track { id, .. } = e {
            // The first album's first track is playing.
            Some(id.clone())
        } else {
            None
        }
    });
    // Select the third track overall.
    let selected_index = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| matches!(e, LibraryEntry::Track { .. }))
        .nth(2)
        .map(|(i, _)| i)
        .unwrap_or(0);

    let render_ctx = EntryRenderContext {
        album_art_style,
        list_width: inner.width as usize,
        large_art_cols,
        background_color: style.background_color(),
        album_color: style.album_color(),
        album_year_color: style.album_year_color(),
        album_length_color: style.album_length_color(),
        track_number_color: style.track_number_color(),
        track_name_color: style.track_name_color(),
        track_name_playing_color: style.track_name_playing_color(),
        track_name_hovered_color: style.track_name_hovered_color(),
        track_length_color: style.track_length_color(),
        track_duration_color: style.track_duration_color(),
        playing_track_id: playing_track_id.as_ref(),
        selected_index,
        underline_index: None,
        hovered_heart_index: None,
        hovered_entry_index: None,
        art_colors: &art_colors,
        large_art_grids: &large_art_grids,
    };

    let items: Vec<ListItem> = entries
        .iter()
        .enumerate()
        .map(|(i, entry)| render_library_entry(entry, i, &render_ctx))
        .collect();

    let list = List::new(items).style(Style::default().bg(style.background_color()));
    let mut list_state = ListState::default();
    *list_state.offset_mut() = 0;
    frame.render_stateful_widget(list, inner, &mut list_state);
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
