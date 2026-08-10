use blackbird_client_shared::{
    config::{AlbumArtStyle, Layout, Playback, SidebarComponent, SidebarPosition},
    style as shared_style,
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, ListState},
};

use crate::keys::Action;

use super::ToColor;

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
    /// Dynamic enum field using index-based get/set with a static variant list.
    EnumFieldDyn {
        label: &'static str,
        section: Section,
        get: fn(&crate::config::Config) -> usize,
        set: fn(&mut crate::config::Config, usize),
        default: fn() -> usize,
        variants: &'static [&'static str],
    },
    /// An ordered multi-component list with add/remove/reorder editing.
    ComponentList {
        label: &'static str,
        section: Section,
        get: fn(&crate::config::Config) -> Vec<SidebarComponent>,
        set: fn(&mut crate::config::Config, Vec<SidebarComponent>),
        default: fn() -> Vec<SidebarComponent>,
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
    Playback,
    Sidebar,
    Colors,
    General,
}

pub struct SettingsState {
    pub selected_index: usize,
    pub editing: bool,
    pub edit_buffer: String,
    pub hsv_component: HsvComponent,
    /// The in-row selection for the sidebar component list while editing.
    pub component_list_sel: usize,
    /// Whether the in-row list item is "armed" (selected for manipulation):
    /// while armed, MoveUp/Down slide it and DeleteChar removes it.
    pub component_list_armed: bool,
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
            component_list_sel: 0,
            component_list_armed: false,
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
        self.component_list_sel = 0;
        self.component_list_armed = false;
        self.scroll_offset = 0;
    }

    /// The editing substate of the panel, used to pick the help bar bindings.
    pub fn edit_mode(&self) -> crate::keys::SettingsEditMode {
        if !self.editing {
            return crate::keys::SettingsEditMode::Navigating;
        }
        match &self.rows[self.selected_index] {
            SettingsRow::HsvField { .. } => crate::keys::SettingsEditMode::HsvEdit,
            SettingsRow::ComponentList { .. } => crate::keys::SettingsEditMode::ComponentList {
                armed: self.component_list_armed,
            },
            _ => crate::keys::SettingsEditMode::TextEdit,
        }
    }
}

fn build_rows() -> Vec<SettingsRow> {
    let mut rows = vec![
        // Server section (no spacer before the first section).
        SettingsRow::SectionHeader("Server (changes reload library)"),
        SettingsRow::StringField {
            label: "Base URL",
            section: Section::Server,
            get: |c| c.server.base_url.clone(),
            set: |c, v| c.server.base_url = v,
            default: || blackbird_shared::config::Server::default().base_url,
            password: false,
        },
        SettingsRow::StringField {
            label: "Username",
            section: Section::Server,
            get: |c| c.server.username.clone(),
            set: |c, v| c.server.username = v,
            default: || blackbird_shared::config::Server::default().username,
            password: false,
        },
        SettingsRow::StringField {
            label: "Password",
            section: Section::Server,
            get: |c| c.server.password.clone(),
            set: |c, v| c.server.password = v,
            default: || blackbird_shared::config::Server::default().password,
            password: true,
        },
        SettingsRow::BoolField {
            label: "Transcode",
            section: Section::Server,
            get: |c| c.server.transcode,
            set: |c, v| c.server.transcode = v,
            default: || blackbird_shared::config::Server::default().transcode,
        },
        // Layout section.
        SettingsRow::SectionSpacer,
        SettingsRow::SectionHeader("Layout"),
        SettingsRow::EnumField {
            label: "Album art style",
            section: Section::Layout,
            get: |c| c.layout.base.album_art_style,
            set: |c, v| c.layout.base.album_art_style = v,
            default: || Layout::default().album_art_style,
        },
        SettingsRow::UsizeField {
            label: "Album spacing",
            section: Section::Layout,
            get: |c| c.layout.base.album_spacing,
            set: |c, v| c.layout.base.album_spacing = v,
            default: || Layout::default().album_spacing,
            min: 0,
            max: 10,
        },
        SettingsRow::BoolField {
            label: "Use terminal background",
            section: Section::Layout,
            get: |c| c.layout.use_terminal_background,
            set: |c, v| c.layout.use_terminal_background = v,
            default: || crate::config::Layout::default().use_terminal_background,
        },
        SettingsRow::BoolField {
            label: "Inline lyrics overlay",
            section: Section::Layout,
            get: |c| c.layout.show_inline_lyrics,
            set: |c, v| c.layout.show_inline_lyrics = v,
            default: || crate::config::Layout::default().show_inline_lyrics,
        },
        SettingsRow::F32Field {
            label: "Scroll multiplier",
            section: Section::Layout,
            get: |c| c.layout.base.scroll_multiplier,
            set: |c, v| c.layout.base.scroll_multiplier = v,
            default: || Layout::default().scroll_multiplier,
            min: 1.0,
            max: 200.0,
        },
        // Sidebar section.
        SettingsRow::SectionSpacer,
        SettingsRow::SectionHeader("Sidebar"),
        SettingsRow::BoolField {
            label: "Sidebar enabled",
            section: Section::Sidebar,
            get: |c| c.layout.base.sidebar.enabled,
            set: |c, v| c.layout.base.sidebar.enabled = v,
            default: || blackbird_client_shared::config::SidebarSettings::default().enabled,
        },
        SettingsRow::EnumFieldDyn {
            label: "Sidebar position",
            section: Section::Sidebar,
            get: |c| {
                let pos = c.layout.base.sidebar.position;
                SidebarPosition::ALL
                    .iter()
                    .position(|p| *p == pos)
                    .unwrap_or(0)
            },
            set: |c, idx| {
                c.layout.base.sidebar.position =
                    SidebarPosition::ALL.get(idx).copied().unwrap_or_default();
            },
            default: || {
                let pos = SidebarPosition::default();
                SidebarPosition::ALL
                    .iter()
                    .position(|p| *p == pos)
                    .unwrap_or(0)
            },
            variants: &["left", "right"],
        },
        SettingsRow::ComponentList {
            label: "Sidebar components",
            section: Section::Sidebar,
            get: |c| c.layout.base.sidebar.components.clone(),
            set: |c, v| {
                c.layout.base.sidebar.components = v;
            },
            default: || blackbird_client_shared::config::SidebarSettings::default().components,
        },
        SettingsRow::UsizeField {
            label: "Similar songs count",
            section: Section::Sidebar,
            get: |c| c.layout.base.sidebar.similar_songs_count,
            set: |c, v| c.layout.base.sidebar.similar_songs_count = v,
            default: || {
                blackbird_client_shared::config::SidebarSettings::default().similar_songs_count
            },
            min: 1,
            max: 100,
        },
        // Playback section.
        SettingsRow::SectionSpacer,
        SettingsRow::SectionHeader("Playback"),
        SettingsRow::BoolField {
            label: "Apply ReplayGain",
            section: Section::Playback,
            get: |c| c.playback.apply_replaygain,
            set: |c, v| c.playback.apply_replaygain = v,
            default: || Playback::default().apply_replaygain,
        },
        SettingsRow::F32Field {
            label: "ReplayGain preamp (dB)",
            section: Section::Playback,
            get: |c| c.playback.replaygain_preamp_db,
            set: |c, v| c.playback.replaygain_preamp_db = v,
            default: || Playback::default().replaygain_preamp_db,
            min: -12.0,
            max: 12.0,
        },
        // Colors section (grouped by concept).
        SettingsRow::SectionSpacer,
    ];

    // HSV color fields are generated dynamically from the style groups,
    // rendered with a group header per concept.
    rows.push(SettingsRow::SectionSpacer);
    for (group_idx, group) in shared_style::GROUPS.iter().enumerate() {
        rows.push(SettingsRow::SectionHeader(group.name));
        for (field_idx, _) in group.fields.iter().enumerate() {
            let global_index = shared_style::Style::group_start(group_idx) + field_idx;
            rows.push(SettingsRow::HsvField {
                label: group.fields[field_idx].label,
                index: global_index,
            });
        }
    }

    rows.extend([
        // App section.
        SettingsRow::SectionSpacer,
        SettingsRow::SectionHeader("App"),
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

/// Swaps the component at `sel` with the one above it (only valid when the
/// user is editing the component list row). Returns the new selection index.
fn move_component_up(
    config: &mut crate::config::Config,
    get: fn(&crate::config::Config) -> Vec<SidebarComponent>,
    set: fn(&mut crate::config::Config, Vec<SidebarComponent>),
    sel: usize,
) -> usize {
    let mut components = get(config);
    if sel == 0 || sel >= components.len() {
        return sel;
    }
    components.swap(sel, sel - 1);
    set(config, components);
    sel - 1
}

/// Swaps the component at `sel` with the one below it. Returns the new
/// selection index.
fn move_component_down(
    config: &mut crate::config::Config,
    get: fn(&crate::config::Config) -> Vec<SidebarComponent>,
    set: fn(&mut crate::config::Config, Vec<SidebarComponent>),
    sel: usize,
) -> usize {
    let mut components = get(config);
    if components.len() < 2 || sel >= components.len() - 1 {
        return sel;
    }
    components.swap(sel, sel + 1);
    set(config, components);
    sel + 1
}

/// Adds the first component not currently in the list (or cycles through
/// absent ones). Returns the new selection index.
fn add_component(
    config: &mut crate::config::Config,
    get: fn(&crate::config::Config) -> Vec<SidebarComponent>,
    set: fn(&mut crate::config::Config, Vec<SidebarComponent>),
) -> usize {
    let mut components = get(config);
    for c in SidebarComponent::ALL {
        if !components.contains(c) {
            components.push(*c);
            let new_len = components.len();
            set(config, components);
            // Rebalance heights after the component-list change.
            config.layout.base.sidebar.rebalance_heights();
            return new_len - 1;
        }
    }
    // All components present.
    components.len().saturating_sub(1)
}

/// Removes the component at `sel`. Returns the new selection index (clamped).
fn remove_component(
    config: &mut crate::config::Config,
    get: fn(&crate::config::Config) -> Vec<SidebarComponent>,
    set: fn(&mut crate::config::Config, Vec<SidebarComponent>),
    sel: usize,
) -> usize {
    let mut components = get(config);
    if components.is_empty() || sel >= components.len() {
        return sel;
    }
    components.remove(sel);
    let new_len = components.len();
    set(config, components);
    config.layout.base.sidebar.rebalance_heights();
    sel.min(new_len.saturating_sub(1))
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

/// Draws the settings list into the given rect. The rect comes from the
/// unified screen layout (`ui::layout::layout_for`); settings no longer plans
/// its own split.
pub fn draw(
    frame: &mut Frame,
    state: &mut SettingsState,
    config: &crate::config::Config,
    area: Rect,
) {
    draw_settings_list(frame, state, &config.style, config, area);
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
        .border_style(Style::default().fg(style.panels.border().to_color()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let visible_height = inner.height as usize;
    let mut items: Vec<ListItem> = Vec::with_capacity(state.rows.len());

    for (idx, row) in state.rows.iter().enumerate() {
        let is_selected = idx == state.selected_index;
        let text = render_row(row, config, style, is_selected, state);
        items.push(ListItem::new(text));
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
) -> Text<'static> {
    let highlight = style.library.track_name_playing().to_color();
    let text_fg = style.general.text().to_color();
    let dim_fg = style.library.track_duration().to_color();

    match row {
        SettingsRow::SectionSpacer => Text::from(Line::from("")),
        SettingsRow::SectionHeader(label) => Text::from(Line::from(Span::styled(
            format!("── {label} ──"),
            Style::default()
                .fg(style.library.album().to_color())
                .add_modifier(Modifier::BOLD),
        ))),
        SettingsRow::BoolField {
            label,
            get,
            default,
            ..
        } => {
            let value = get(config);
            let is_default = value == default();
            let check = if value { "[x]" } else { "[ ]" };
            let mut spans = vec![Span::styled(
                format!("{check} {label}"),
                Style::default().fg(if is_selected { highlight } else { text_fg }),
            )];
            if !is_default {
                spans.push(Span::styled(" *", Style::default().fg(dim_fg)));
            }
            Text::from(Line::from(spans))
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
            let display_value = if is_selected && state.editing {
                state.edit_buffer.clone()
            } else if *password {
                "*".repeat(value.len())
            } else {
                value
            };
            let mut spans = vec![
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
            Text::from(Line::from(spans))
        }
        SettingsRow::UsizeField {
            label,
            get,
            default,
            ..
        } => {
            let value = get(config);
            let is_default = value == default();
            let display_value = if is_selected && state.editing {
                state.edit_buffer.clone()
            } else {
                value.to_string()
            };
            let mut spans = vec![
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
            Text::from(Line::from(spans))
        }
        SettingsRow::F32Field {
            label,
            get,
            default,
            ..
        } => {
            let value = get(config);
            let is_default = (value - default()).abs() < f32::EPSILON;
            let display_value = if is_selected && state.editing {
                state.edit_buffer.clone()
            } else {
                format!("{value:.1}")
            };
            let mut spans = vec![
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
            Text::from(Line::from(spans))
        }
        SettingsRow::U64Field {
            label,
            get,
            default,
            ..
        } => {
            let value = get(config);
            let is_default = value == default();
            let display_value = if is_selected && state.editing {
                state.edit_buffer.clone()
            } else {
                value.to_string()
            };
            let mut spans = vec![
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
            Text::from(Line::from(spans))
        }
        SettingsRow::EnumField {
            label,
            get,
            default,
            ..
        } => {
            let value = get(config);
            let is_default = value == default();
            let mut spans = vec![Span::styled(
                format!("{label}: {}", value.as_str()),
                Style::default().fg(if is_selected { highlight } else { text_fg }),
            )];
            if !is_default {
                spans.push(Span::styled(" *", Style::default().fg(dim_fg)));
            }
            Text::from(Line::from(spans))
        }
        SettingsRow::EnumFieldDyn {
            label,
            get,
            default,
            variants,
            ..
        } => {
            let value_idx = get(config);
            let default_idx = default();
            let is_default = value_idx == default_idx;
            let value_str = variants.get(value_idx).copied().unwrap_or("?");
            let mut spans = vec![Span::styled(
                format!("{label}: {value_str}"),
                Style::default().fg(if is_selected { highlight } else { text_fg }),
            )];
            if !is_default {
                spans.push(Span::styled(" *", Style::default().fg(dim_fg)));
            }
            Text::from(Line::from(spans))
        }
        SettingsRow::ComponentList {
            label,
            get,
            default,
            ..
        } => {
            let components = get(config);
            let is_default = components == default();
            if is_selected && state.editing {
                // Vertical list: a header line, one line per component with the
                // in-row selection highlighted.
                let mut lines = vec![Line::from(vec![Span::styled(
                    format!("{label}:"),
                    Style::default().fg(if is_selected { highlight } else { text_fg }),
                )])];
                for (i, c) in components.iter().enumerate() {
                    let name = match c {
                        SidebarComponent::Lyrics => "lyrics",
                        SidebarComponent::SimilarSongs => "similar songs",
                    };
                    let is_sel = i == state.component_list_sel;
                    let marker = if is_sel && state.component_list_armed {
                        ">>"
                    } else if is_sel {
                        "> "
                    } else {
                        "  "
                    };
                    lines.push(Line::from(vec![Span::styled(
                        format!("  {marker} {name}"),
                        Style::default()
                            .fg(if is_sel { highlight } else { text_fg })
                            .add_modifier(if is_sel {
                                Modifier::BOLD
                            } else {
                                Modifier::empty()
                            }),
                    )]));
                }
                if !is_default {
                    lines[0]
                        .spans
                        .push(Span::styled(" *", Style::default().fg(dim_fg)));
                }
                Text::from(lines)
            } else {
                let mut spans = vec![Span::styled(
                    format!("{label}:"),
                    Style::default().fg(if is_selected { highlight } else { text_fg }),
                )];
                let names: Vec<&str> = components
                    .iter()
                    .map(|c| match c {
                        SidebarComponent::Lyrics => "lyrics",
                        SidebarComponent::SimilarSongs => "similar songs",
                    })
                    .collect();
                spans.push(Span::styled(
                    format!(" {}", names.join(", ")),
                    Style::default().fg(if is_selected { highlight } else { text_fg }),
                ));
                if !is_default {
                    spans.push(Span::styled(" *", Style::default().fg(dim_fg)));
                }
                Text::from(Line::from(spans))
            }
        }
        SettingsRow::HsvField { label, index } => {
            let hsv = *config.style.field(*index);
            let default_hsv = shared_style::Style::default_field(*index);
            let is_default = hsv == default_hsv;

            // Convert HSV to an RGB swatch for preview.
            let swatch_color = super::style_color(hsv);

            let mut spans = vec![Span::styled(
                "\u{2588}\u{2588}",
                Style::default().fg(swatch_color),
            )];
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
            Text::from(Line::from(spans))
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
                // If an item in the component list is armed, Esc disarms it
                // first; a second Esc cancels editing.
                if matches!(
                    &state.rows[state.selected_index],
                    SettingsRow::ComponentList { .. }
                ) && state.component_list_armed
                {
                    state.component_list_armed = false;
                    return (None, false);
                }
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
                    SettingsRow::ComponentList { .. } => {
                        // Enter confirms the list edit while armed, or arms an
                        // item while navigating.
                        if state.component_list_armed {
                            state.editing = false;
                            state.edit_buffer.clear();
                            state.component_list_armed = false;
                            return (None, false);
                        }
                        state.component_list_armed = true;
                        return (None, false);
                    }
                    _ => {}
                }
                state.editing = false;
                state.edit_buffer.clear();
                return (None, server_changed);
            }
            Action::Char(c) => {
                let row = &state.rows[state.selected_index];
                if matches!(row, SettingsRow::HsvField { .. })
                    || matches!(row, SettingsRow::ComponentList { .. })
                {
                    // In HSV edit mode and the component-list row, ignore
                    // character input (the list is edited with MoveUp/Down,
                    // Backspace to remove; adding uses 'a').
                    if let SettingsRow::ComponentList { get, set, .. } = row
                        && c == 'a'
                    {
                        state.component_list_sel = add_component(config, *get, *set);
                        state.component_list_armed = true;
                    }
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
                let row = &state.rows[state.selected_index];
                if let SettingsRow::ComponentList { get, set, .. } = row {
                    if state.component_list_armed {
                        state.component_list_sel =
                            remove_component(config, *get, *set, state.component_list_sel);
                        state.component_list_armed = false;
                    }
                } else {
                    state.edit_buffer.pop();
                }
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
                } else if let SettingsRow::ComponentList { get, set, .. } = row {
                    if state.component_list_armed {
                        // Slide the armed item up.
                        state.component_list_sel =
                            move_component_up(config, *get, *set, state.component_list_sel);
                    } else {
                        // Navigate up within the list.
                        let len = get(config).len();
                        if state.component_list_sel > 0 {
                            state.component_list_sel -= 1;
                        } else if len > 0 {
                            state.component_list_sel = len - 1;
                        }
                    }
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
                } else if let SettingsRow::ComponentList { get, set, .. } = row {
                    if state.component_list_armed {
                        // Slide the armed item down.
                        state.component_list_sel =
                            move_component_down(config, *get, *set, state.component_list_sel);
                    } else {
                        // Navigate down within the list.
                        let len = get(config).len();
                        if len > 0 && state.component_list_sel + 1 < len {
                            state.component_list_sel += 1;
                        } else if len > 0 {
                            state.component_list_sel = 0;
                        }
                    }
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
                SettingsRow::EnumFieldDyn {
                    get,
                    set,
                    section,
                    variants,
                    ..
                } => {
                    let current = get(config);
                    let next = (current + 1) % variants.len();
                    set(config, next);
                    if *section == Section::Server {
                        server_changed = true;
                    }
                }
                SettingsRow::ComponentList { .. } => {
                    state.editing = true;
                    state.edit_buffer.clear();
                    state.component_list_sel = 0;
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
                SettingsRow::EnumFieldDyn {
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
                SettingsRow::ComponentList { default, set, .. } => {
                    set(config, default());
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
                | SettingsRow::EnumField { section, .. }
                | SettingsRow::EnumFieldDyn { section, .. }
                | SettingsRow::ComponentList { section, .. } => Some(*section),
                SettingsRow::HsvField { .. } => Some(Section::Colors),
            };
            if let Some(section) = section {
                match section {
                    Section::Server => {
                        config.server = blackbird_shared::config::Server::default();
                        server_changed = true;
                    }
                    Section::Layout => {
                        config.layout = crate::config::Layout::default();
                    }
                    Section::Playback => {
                        config.playback = Playback::default();
                    }
                    Section::Sidebar => {
                        config.layout.base.sidebar =
                            blackbird_client_shared::config::SidebarSettings::default();
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
/// The x coordinate is deliberately unused: the caller already routes clicks
/// x-scoped to the settings rect, and row mapping only needs the row index
/// within the last-drawn list (`last_inner_area`).
pub fn handle_mouse_click(
    state: &mut SettingsState,
    config: &mut crate::config::Config,
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

    // While editing the multi-line component-list row, clicks that land on the
    // row's in-row item lines (below its header) must not be mis-mapped to
    // subsequent rows: the naive per-line mapping counts each item line as a
    // row. Only the header line of the expanded row is a click target. Clicks
    // on other rows still proceed normally (and cancel the edit as usual).
    if state.editing
        && matches!(
            &state.rows[state.selected_index],
            SettingsRow::ComponentList { .. }
        )
        && clicked_index == state.selected_index
    {
        let header_line = state.selected_index.saturating_sub(state.scroll_offset);
        if row_in_list != header_line {
            return false;
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_width_stays_within_area() {
        // The unified clamp (shared with the render/drag paths) lives in
        // `ui::layout`; this test pins its contract here alongside the
        // settings module.
        let w = crate::ui::layout::settings_width;
        // The panel must never collapse below 20 and must leave at least 20
        // columns for the library preview.
        // width 20: max(20, 0) = 20, so even a huge configured value clamps to 20.
        assert_eq!(w(20, 100), 20);
        assert_eq!(w(20, 20), 20);
        // width 39: max(20, 19) = 20, so 40 configured clamps to 20 (leaves 19).
        assert_eq!(w(39, 40), 20);
        assert_eq!(w(39, 20), 20);
        // width 40: max(20, 20) = 20, so 40 clamps to 20 (leaves exactly 20).
        assert_eq!(w(40, 40), 20);
        // width 100: room for the full configured width (40) plus 60 preview.
        assert_eq!(w(100, 40), 40);
        // Tiny width (e.g. 10): max(20, 0) = 20, configured clamps up to 20.
        assert_eq!(w(10, 5), 20);
    }

    #[test]
    fn test_settings_component_order_row() {
        let mut state = SettingsState::new();
        let mut config = crate::config::Config::default();

        // Find the "Sidebar components" row.
        let row_idx = state
            .rows
            .iter()
            .position(|r| {
                matches!(r, SettingsRow::ComponentList { label, .. } if *label == "Sidebar components")
            })
            .expect("Sidebar components row should exist");
        state.selected_index = row_idx;

        // Default order is [Lyrics, SimilarSongs].
        assert_eq!(
            config.layout.base.sidebar.components,
            vec![SidebarComponent::Lyrics, SidebarComponent::SimilarSongs]
        );

        // Enter editing mode.
        let _ = handle_key(&mut state, &mut config, Action::Select);
        assert!(state.editing);
        assert_eq!(state.component_list_sel, 0);
        assert!(!state.component_list_armed);

        // Navigate down (not armed → moves selection, no reorder).
        let _ = handle_key(&mut state, &mut config, Action::MoveDown);
        assert_eq!(state.component_list_sel, 1);
        assert_eq!(
            config.layout.base.sidebar.components,
            vec![SidebarComponent::Lyrics, SidebarComponent::SimilarSongs]
        );

        // Enter arms the item; armed MoveDown slides the selected item down.
        let _ = handle_key(&mut state, &mut config, Action::Select);
        assert!(state.component_list_armed);
        // (Selection is at 1 from the navigation above; navigate to the top
        // while armed slides: move up once swaps 1↔0.)
        let _ = handle_key(&mut state, &mut config, Action::MoveUp);
        assert_eq!(state.component_list_sel, 0);
        assert_eq!(
            config.layout.base.sidebar.components,
            vec![SidebarComponent::SimilarSongs, SidebarComponent::Lyrics]
        );
        // Armed MoveDown slides lyrics back below similar songs.
        let _ = handle_key(&mut state, &mut config, Action::MoveDown);
        assert_eq!(
            config.layout.base.sidebar.components,
            vec![SidebarComponent::Lyrics, SidebarComponent::SimilarSongs]
        );
        assert_eq!(state.component_list_sel, 1);

        // Add: with both present, 'a' is a no-op (all components present).
        let list_len = config.layout.base.sidebar.components.len();
        let _ = handle_key(&mut state, &mut config, Action::Char('a'));
        assert_eq!(config.layout.base.sidebar.components.len(), list_len);

        // Esc disarms the armed item (armed Select now confirms and exits).
        let _ = handle_key(&mut state, &mut config, Action::Back);
        assert!(!state.component_list_armed);
        assert!(state.editing);
        // Re-arm to verify armed Select confirms and exits editing.
        let _ = handle_key(&mut state, &mut config, Action::Select);
        assert!(state.component_list_armed);
        let _ = handle_key(&mut state, &mut config, Action::Select);
        assert!(!state.component_list_armed);
        assert!(!state.editing);
        assert_eq!(
            config.layout.base.sidebar.components,
            vec![SidebarComponent::Lyrics, SidebarComponent::SimilarSongs]
        );

        // Re-enter editing and delete similar songs.
        let _ = handle_key(&mut state, &mut config, Action::Select);
        assert!(state.editing);
        let _ = handle_key(&mut state, &mut config, Action::MoveDown);
        assert_eq!(state.component_list_sel, 1);
        let _ = handle_key(&mut state, &mut config, Action::Select);
        assert!(state.component_list_armed);
        let _ = handle_key(&mut state, &mut config, Action::DeleteChar);
        assert_eq!(
            config.layout.base.sidebar.components,
            vec![SidebarComponent::Lyrics]
        );
        assert_eq!(state.component_list_sel, 0);
        assert!(!state.component_list_armed);

        // Add 'a' again: re-adds the absent similar songs (heights rebalanced).
        let _ = handle_key(&mut state, &mut config, Action::Char('a'));
        assert_eq!(
            config.layout.base.sidebar.components,
            vec![SidebarComponent::Lyrics, SidebarComponent::SimilarSongs]
        );
        assert_eq!(config.layout.base.sidebar.heights.len(), 2);
        // Adding arms the new item; Esc disarms it first...
        assert!(state.component_list_armed);
        let _ = handle_key(&mut state, &mut config, Action::Back);
        assert!(!state.component_list_armed);
        assert!(state.editing);
        // ...and a second Esc exits editing.
        let _ = handle_key(&mut state, &mut config, Action::Back);
        assert!(!state.editing);
    }

    #[test]
    fn sidebar_enabled_row_toggles_config() {
        let mut state = SettingsState::new();
        let mut config = crate::config::Config::default();

        let row_idx = state
            .rows
            .iter()
            .position(|r| matches!(r, SettingsRow::BoolField { label, .. } if *label == "Sidebar enabled"))
            .expect("Sidebar enabled row should exist");
        state.selected_index = row_idx;

        assert!(config.layout.base.sidebar.enabled);
        let _ = handle_key(&mut state, &mut config, Action::Select);
        assert!(!config.layout.base.sidebar.enabled);
    }

    #[test]
    fn similar_songs_count_row_edits() {
        let mut state = SettingsState::new();
        let mut config = crate::config::Config::default();

        let row_idx = state
            .rows
            .iter()
            .position(|r| matches!(r, SettingsRow::UsizeField { label, .. } if *label == "Similar songs count"))
            .expect("Similar songs count row should exist");
        state.selected_index = row_idx;

        assert_eq!(config.layout.base.sidebar.similar_songs_count, 20);
        let _ = handle_key(&mut state, &mut config, Action::Select);
        assert!(state.editing);
        // The buffer starts as "20"; clear it then type "50".
        for _ in 0..2 {
            let _ = handle_key(&mut state, &mut config, Action::DeleteChar);
        }
        for c in "50".chars() {
            let _ = handle_key(&mut state, &mut config, Action::Char(c));
        }
        let _ = handle_key(&mut state, &mut config, Action::Select);
        assert_eq!(config.layout.base.sidebar.similar_songs_count, 50);
    }
}
