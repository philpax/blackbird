use blackbird_client_shared::Direction;
use blackbird_core as bc;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use smol_str::{SmolStr, ToSmolStr};

/// An entry in the help bar, either a single action or a merged pair.
///
/// For pairs, the description is provided explicitly so that shared
/// suffixes/prefixes can be factored out (e.g. "next/prev group"
/// instead of "next group/prev group").
///
/// `Custom` overrides the description for a single action; it is used when
/// the same key means something different in a substate (e.g. Enter confirms
/// an edit in the settings panel rather than playing a track).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpEntry {
    Single(Action),
    Pair(Action, Action, &'static str),
    Custom(Action, &'static str),
}

/// The editing substate of the settings panel. The help bar shows different
/// bindings depending on what is being edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsEditMode {
    /// Not editing any row; the panel-level bindings apply.
    Navigating,
    /// Editing a text or numeric field (String/Usize/F32/U64).
    TextEdit,
    /// Adjusting the HSV components of a color field.
    HsvEdit,
    /// Rearranging/adding/removing entries in the sidebar component list.
    /// `armed` is true when an item is selected for manipulation.
    ComponentList { armed: bool },
}

/// Centrally defined key actions for the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    PlayPause,
    Stop,
    Next,
    Previous,
    NextGroup,
    PreviousGroup,
    CyclePlaybackMode(Direction),
    ToggleSortOrder(Direction),
    Search,
    Lyrics,
    ToggleSidebar,
    Logs,
    Queue,
    VolumeMode,
    VolumeUp,
    VolumeDown,
    Star,
    SeekForward,
    SeekBackward,
    GotoPlaying,
    MoveUp,
    MoveDown,
    PageUp,
    PageDown,
    GotoTop,
    GotoBottom,
    Select,
    GotoSelected,
    Back,
    ClearLine,
    Char(char),
    DeleteChar,
    Settings,
    MoveLeft,
    MoveRight,
    ResetField,
    ResetSection,
}

// ── Key code constants ───────────────────────────────────────────

pub const KEY_QUIT: KeyCode = KeyCode::Char('q');
pub const KEY_PLAY_PAUSE: KeyCode = KeyCode::Char(' ');
pub const KEY_STOP: KeyCode = KeyCode::Char('s');
pub const KEY_NEXT: KeyCode = KeyCode::Char('n');
pub const KEY_PREVIOUS: KeyCode = KeyCode::Char('p');
pub const KEY_NEXT_GROUP: KeyCode = KeyCode::Char('N');
pub const KEY_PREVIOUS_GROUP: KeyCode = KeyCode::Char('P');
pub const KEY_CYCLE_MODE_FWD: KeyCode = KeyCode::Char('m');
pub const KEY_CYCLE_MODE_BWD: KeyCode = KeyCode::Char('M');
pub const KEY_TOGGLE_SORT_FWD: KeyCode = KeyCode::Char('o');
pub const KEY_TOGGLE_SORT_BWD: KeyCode = KeyCode::Char('O');
pub const KEY_SEARCH: KeyCode = KeyCode::Char('/');
pub const KEY_LYRICS: KeyCode = KeyCode::Char('l');
pub const KEY_TOGGLE_SIDEBAR: KeyCode = KeyCode::Char('t');
pub const KEY_LOGS: KeyCode = KeyCode::Char('L');
pub const KEY_QUEUE: KeyCode = KeyCode::Char('u');
pub const KEY_VOLUME: KeyCode = KeyCode::Char('v');
pub const KEY_GOTO_PLAYING: KeyCode = KeyCode::Char('g');
pub const KEY_SEEK_BACK: KeyCode = KeyCode::Char('<');
pub const KEY_SEEK_BACK_ALT: KeyCode = KeyCode::Char(',');
pub const KEY_SEEK_FWD: KeyCode = KeyCode::Char('>');
pub const KEY_SEEK_FWD_ALT: KeyCode = KeyCode::Char('.');
pub const KEY_STAR: KeyCode = KeyCode::Char('*');
pub const KEY_SELECT: KeyCode = KeyCode::Enter;
pub const KEY_BACK: KeyCode = KeyCode::Esc;
pub const KEY_UP: KeyCode = KeyCode::Up;
pub const KEY_DOWN: KeyCode = KeyCode::Down;
pub const KEY_LEFT: KeyCode = KeyCode::Left;
pub const KEY_RIGHT: KeyCode = KeyCode::Right;
pub const KEY_PAGE_UP: KeyCode = KeyCode::PageUp;
pub const KEY_PAGE_DOWN: KeyCode = KeyCode::PageDown;
pub const KEY_GOTO_TOP: KeyCode = KeyCode::Home;
pub const KEY_GOTO_BOTTOM: KeyCode = KeyCode::End;
pub const KEY_DELETE_CHAR: KeyCode = KeyCode::Backspace;
pub const KEY_SETTINGS: KeyCode = KeyCode::Char('i');
pub const KEY_CONFIRM_YES: KeyCode = KeyCode::Char('y');
pub const KEY_CONFIRM_NO: KeyCode = KeyCode::Char('n');

impl Action {
    /// Label shown in the help bar. Returns `None` for actions that
    /// shouldn't appear (navigation, text input, etc.).
    pub fn help_label(&self, logic: &bc::Logic) -> Option<(SmolStr, SmolStr)> {
        let (key_str, desc): (SmolStr, SmolStr) = match self {
            Action::Quit => (key_label(KEY_QUIT), "quit".into()),
            Action::PlayPause => {
                let label = if logic.get_playback_state() == bc::PlaybackState::Playing {
                    "pause"
                } else {
                    "play"
                };
                (key_label(KEY_PLAY_PAUSE), label.into())
            }
            Action::Stop => (key_label(KEY_STOP), "stop".into()),
            Action::Next => (key_label(KEY_NEXT), "next".into()),
            Action::Previous => (key_label(KEY_PREVIOUS), "prev".into()),
            Action::NextGroup if logic.get_playback_mode().has_group_structure() => {
                (key_label(KEY_NEXT_GROUP), "next group".into())
            }
            Action::PreviousGroup if logic.get_playback_mode().has_group_structure() => {
                (key_label(KEY_PREVIOUS_GROUP), "prev group".into())
            }
            Action::Search => (key_label(KEY_SEARCH), "search".into()),
            Action::Lyrics => (key_label(KEY_LYRICS), "lyrics".into()),
            Action::ToggleSidebar => (key_label(KEY_TOGGLE_SIDEBAR), "sidebar".into()),
            Action::Logs => (key_label(KEY_LOGS), "logs".into()),
            Action::Queue => (key_label(KEY_QUEUE), "queue".into()),
            Action::VolumeMode => (key_label(KEY_VOLUME), "vol".into()),
            Action::Star => (key_label(KEY_STAR), "star".into()),
            Action::SeekForward => (key_label(KEY_SEEK_FWD), "seek+".into()),
            Action::SeekBackward => (key_label(KEY_SEEK_BACK), "seek-".into()),
            Action::GotoPlaying => (key_label(KEY_GOTO_PLAYING), "goto".into()),
            Action::Select => (key_label(KEY_SELECT), "play".into()),
            Action::GotoSelected => ("shift+enter".into(), "goto".into()),
            Action::Back => (key_label(KEY_BACK), "close".into()),
            Action::CyclePlaybackMode(Direction::Forward) => {
                let mode = logic.get_playback_mode().as_str();
                (
                    pair_label(KEY_CYCLE_MODE_FWD, KEY_CYCLE_MODE_BWD),
                    format!("mode ({mode})").into(),
                )
            }
            Action::ToggleSortOrder(Direction::Forward) => {
                let order = logic.get_sort_order().as_str();
                (
                    pair_label(KEY_TOGGLE_SORT_FWD, KEY_TOGGLE_SORT_BWD),
                    format!("sort ({order})").into(),
                )
            }
            Action::Settings => (key_label(KEY_SETTINGS), "settings".into()),
            Action::MoveLeft => (key_label(KEY_LEFT), "left".into()),
            Action::MoveRight => (key_label(KEY_RIGHT), "right".into()),
            Action::ResetField => (key_label(KeyCode::Char('d')), "reset field".into()),
            Action::ResetSection => (key_label(KeyCode::Char('D')), "reset section".into()),
            // Char keys are only rendered through `HelpEntry::Custom`, which
            // supplies its own description (the default label is the key).
            Action::Char(c) => (key_label(KeyCode::Char(*c)), c.to_string().into()),
            Action::DeleteChar => (key_label(KEY_DELETE_CHAR), "delete".into()),
            _ => return None,
        };
        Some((key_str, desc))
    }
}

fn key_label(key: KeyCode) -> SmolStr {
    match key {
        // Printable non-whitespace characters are already in the correct case.
        KeyCode::Char(c) if !c.is_whitespace() => SmolStr::new(c.to_string()),
        // Everything else (Space, Enter, Esc, PageUp, etc.) uses title case
        // in crossterm's Display impl; lowercase it for the help bar.
        other => other.to_smolstr().to_lowercase().into(),
    }
}

fn pair_label(forward: KeyCode, backward: KeyCode) -> SmolStr {
    format!("{}/{}", key_label(forward), key_label(backward)).into()
}

/// Resolve a key event into an action in library context.
pub fn library_action(key: &KeyEvent) -> Option<Action> {
    match key.code {
        KEY_QUIT => Some(Action::Quit),
        KEY_PLAY_PAUSE => Some(Action::PlayPause),
        KEY_NEXT => Some(Action::Next),
        KEY_PREVIOUS => Some(Action::Previous),
        KEY_NEXT_GROUP => Some(Action::NextGroup),
        KEY_PREVIOUS_GROUP => Some(Action::PreviousGroup),
        KEY_STOP => Some(Action::Stop),
        KEY_CYCLE_MODE_FWD => Some(Action::CyclePlaybackMode(Direction::Forward)),
        KEY_CYCLE_MODE_BWD => Some(Action::CyclePlaybackMode(Direction::Backward)),
        KEY_TOGGLE_SORT_FWD => Some(Action::ToggleSortOrder(Direction::Forward)),
        KEY_TOGGLE_SORT_BWD => Some(Action::ToggleSortOrder(Direction::Backward)),
        KEY_SEARCH => Some(Action::Search),
        KEY_LYRICS => Some(Action::Lyrics),
        KEY_TOGGLE_SIDEBAR => Some(Action::ToggleSidebar),
        KEY_LOGS => Some(Action::Logs),
        KEY_QUEUE => Some(Action::Queue),
        KEY_VOLUME => Some(Action::VolumeMode),
        KEY_GOTO_PLAYING => Some(Action::GotoPlaying),
        KEY_SEEK_BACK | KEY_SEEK_BACK_ALT => Some(Action::SeekBackward),
        KEY_SEEK_FWD | KEY_SEEK_FWD_ALT => Some(Action::SeekForward),
        KEY_STAR => Some(Action::Star),
        KEY_UP => Some(Action::MoveUp),
        KEY_DOWN => Some(Action::MoveDown),
        KEY_PAGE_UP => Some(Action::PageUp),
        KEY_PAGE_DOWN => Some(Action::PageDown),
        KEY_GOTO_TOP => Some(Action::GotoTop),
        KEY_GOTO_BOTTOM => Some(Action::GotoBottom),
        KEY_SELECT => Some(Action::Select),
        KEY_SETTINGS => Some(Action::Settings),
        _ => None,
    }
}

/// Resolve a key event into an action in settings context.
/// When `editing` is true, `KEY_QUIT` falls through to `Char` input instead
/// of closing the panel.
pub fn settings_action(key: &KeyEvent, editing: bool) -> Option<Action> {
    match key.code {
        KEY_QUIT if !editing => Some(Action::Back),
        KEY_BACK => Some(Action::Back),
        KEY_SELECT => Some(Action::Select),
        KEY_UP => Some(Action::MoveUp),
        KEY_DOWN => Some(Action::MoveDown),
        KEY_LEFT => Some(Action::MoveLeft),
        KEY_RIGHT => Some(Action::MoveRight),
        KEY_PAGE_UP => Some(Action::PageUp),
        KEY_PAGE_DOWN => Some(Action::PageDown),
        KEY_DELETE_CHAR => Some(Action::DeleteChar),
        KeyCode::Char('d') => Some(Action::ResetField),
        KeyCode::Char('D') => Some(Action::ResetSection),
        KeyCode::Char(c) => Some(Action::Char(c)),
        _ => None,
    }
}

/// Resolve a key event into an action in search context.
pub fn search_action(key: &KeyEvent) -> Option<Action> {
    match key.code {
        KEY_BACK => Some(Action::Back),
        KEY_SELECT if key.modifiers.contains(KeyModifiers::SHIFT) => Some(Action::GotoSelected),
        KEY_SELECT => Some(Action::Select),
        KEY_UP => Some(Action::MoveUp),
        KEY_DOWN => Some(Action::MoveDown),
        KEY_DELETE_CHAR => Some(Action::DeleteChar),
        KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::CONTROL) => match c {
            // Terminals that don't disambiguate shift+enter send a raw `\n`
            // (0x0A), which crossterm parses as Ctrl+J in raw mode. Treat it
            // as GotoSelected so shift+enter works there too.
            'j' => Some(Action::GotoSelected),
            'u' => Some(Action::ClearLine),
            _ => Some(Action::Char(c)),
        },
        KeyCode::Char(c) => Some(Action::Char(c)),
        _ => None,
    }
}

/// Resolve a key event into an action in lyrics context.
pub fn lyrics_action(key: &KeyEvent) -> Option<Action> {
    match key.code {
        KEY_BACK | KEY_LYRICS | KEY_QUIT => Some(Action::Back),
        KEY_TOGGLE_SIDEBAR => Some(Action::ToggleSidebar),
        KEY_UP => Some(Action::MoveUp),
        KEY_DOWN => Some(Action::MoveDown),
        KEY_PAGE_UP => Some(Action::PageUp),
        KEY_PAGE_DOWN => Some(Action::PageDown),
        KEY_SELECT => Some(Action::Select),
        KEY_SEEK_BACK | KEY_SEEK_BACK_ALT => Some(Action::SeekBackward),
        KEY_SEEK_FWD | KEY_SEEK_FWD_ALT => Some(Action::SeekForward),
        KEY_PLAY_PAUSE => Some(Action::PlayPause),
        KEY_NEXT => Some(Action::Next),
        KEY_PREVIOUS => Some(Action::Previous),
        KEY_NEXT_GROUP => Some(Action::NextGroup),
        KEY_PREVIOUS_GROUP => Some(Action::PreviousGroup),
        _ => None,
    }
}

/// Resolve a key event into an action in volume-editing context.
pub fn volume_action(key: &KeyEvent) -> Option<Action> {
    match key.code {
        KEY_UP | KEY_RIGHT => Some(Action::VolumeUp),
        KEY_DOWN | KEY_LEFT => Some(Action::VolumeDown),
        KEY_BACK | KEY_VOLUME | KEY_SELECT => Some(Action::Back),
        _ => None,
    }
}

/// Resolve a key event into an action in album art overlay context.
pub fn album_art_overlay_action(key: &KeyEvent) -> Option<Action> {
    match key.code {
        KEY_BACK | KEY_QUIT | KEY_SELECT => Some(Action::Back),
        _ => None,
    }
}

/// Resolve a key event into an action in playback mode dropdown context.
pub fn playback_mode_dropdown_action(key: &KeyEvent) -> Option<Action> {
    match key.code {
        KEY_BACK | KEY_QUIT => Some(Action::Back),
        KEY_SELECT => Some(Action::Select),
        KEY_UP => Some(Action::MoveUp),
        KEY_DOWN => Some(Action::MoveDown),
        _ => None,
    }
}

/// Resolve a key event into an action in quit-confirmation context.
/// `y` / Enter confirms; any other key cancels.
pub fn quit_confirm_action(key: &KeyEvent) -> Action {
    match key.code {
        KEY_CONFIRM_YES | KEY_SELECT => Action::Select,
        _ => Action::Back,
    }
}

/// Resolve a key event into an action in queue context.
pub fn queue_action(key: &KeyEvent) -> Option<Action> {
    match key.code {
        KEY_BACK | KEY_QUEUE | KEY_QUIT => Some(Action::Back),
        KEY_UP => Some(Action::MoveUp),
        KEY_DOWN => Some(Action::MoveDown),
        KEY_PAGE_UP => Some(Action::PageUp),
        KEY_PAGE_DOWN => Some(Action::PageDown),
        KEY_SELECT => Some(Action::Select),
        KEY_PLAY_PAUSE => Some(Action::PlayPause),
        KEY_NEXT => Some(Action::Next),
        KEY_PREVIOUS => Some(Action::Previous),
        KEY_NEXT_GROUP => Some(Action::NextGroup),
        KEY_PREVIOUS_GROUP => Some(Action::PreviousGroup),
        KEY_CYCLE_MODE_FWD => Some(Action::CyclePlaybackMode(Direction::Forward)),
        KEY_CYCLE_MODE_BWD => Some(Action::CyclePlaybackMode(Direction::Backward)),
        _ => None,
    }
}

/// Resolve a key event into an action in logs context.
pub fn logs_action(key: &KeyEvent) -> Option<Action> {
    match key.code {
        KEY_BACK | KEY_LOGS | KEY_QUIT => Some(Action::Back),
        KEY_UP => Some(Action::MoveUp),
        KEY_DOWN => Some(Action::MoveDown),
        KEY_PAGE_UP => Some(Action::PageUp),
        KEY_PAGE_DOWN => Some(Action::PageDown),
        KEY_GOTO_TOP => Some(Action::GotoTop),
        KEY_GOTO_BOTTOM => Some(Action::GotoBottom),
        _ => None,
    }
}

/// Ordered list of entries to show in the library help bar.
pub const LIBRARY_HELP: &[HelpEntry] = &[
    HelpEntry::Single(Action::Quit),
    HelpEntry::Single(Action::PlayPause),
    HelpEntry::Pair(Action::Next, Action::Previous, "next/prev"),
    HelpEntry::Pair(Action::NextGroup, Action::PreviousGroup, "next/prev group"),
    HelpEntry::Single(Action::Stop),
    HelpEntry::Pair(Action::SeekBackward, Action::SeekForward, "seek-/+"),
    HelpEntry::Single(Action::Star),
    HelpEntry::Single(Action::GotoPlaying),
    HelpEntry::Single(Action::Search),
    HelpEntry::Single(Action::Lyrics),
    HelpEntry::Single(Action::ToggleSidebar),
    HelpEntry::Single(Action::Queue),
    HelpEntry::Single(Action::VolumeMode),
    HelpEntry::Single(Action::Select),
    HelpEntry::Single(Action::CyclePlaybackMode(Direction::Forward)),
    HelpEntry::Single(Action::ToggleSortOrder(Direction::Forward)),
    HelpEntry::Single(Action::Settings),
];

/// Ordered list of entries to show in the settings help bar while navigating
/// (no row is being edited).
pub const SETTINGS_HELP: &[HelpEntry] = &[
    HelpEntry::Custom(Action::Back, "close"),
    HelpEntry::Pair(Action::MoveUp, Action::MoveDown, "nav/adjust"),
    HelpEntry::Pair(Action::MoveLeft, Action::MoveRight, "hsv comp"),
    HelpEntry::Custom(Action::Select, "select"),
    HelpEntry::Single(Action::ResetField),
    HelpEntry::Single(Action::ResetSection),
];

/// Ordered list of entries to show in the settings help bar while editing a
/// text or numeric field.
pub const SETTINGS_HELP_EDITING: &[HelpEntry] = &[
    HelpEntry::Custom(Action::Back, "cancel"),
    HelpEntry::Single(Action::Select),
    HelpEntry::Single(Action::DeleteChar),
];

/// Ordered list of entries to show in the settings help bar while adjusting
/// the HSV components of a color field.
pub const SETTINGS_HELP_HSV: &[HelpEntry] = &[
    HelpEntry::Custom(Action::Back, "cancel"),
    HelpEntry::Custom(Action::Select, "done"),
    HelpEntry::Pair(Action::MoveUp, Action::MoveDown, "value"),
    HelpEntry::Pair(Action::MoveLeft, Action::MoveRight, "hsv comp"),
    HelpEntry::Pair(Action::PageUp, Action::PageDown, "jump"),
];

/// Ordered list of entries to show in the settings help bar while editing the
/// sidebar component list, navigating (no item is armed).
pub const SETTINGS_HELP_COMPONENT_LIST: &[HelpEntry] = &[
    HelpEntry::Custom(Action::Back, "cancel"),
    HelpEntry::Custom(Action::Select, "select"),
    HelpEntry::Pair(Action::MoveUp, Action::MoveDown, "nav"),
    HelpEntry::Custom(Action::Char('a'), "add"),
];

/// Ordered list of entries to show in the settings help bar while editing the
/// sidebar component list with an item armed (selected for manipulation).
pub const SETTINGS_HELP_COMPONENT_LIST_ARMED: &[HelpEntry] = &[
    HelpEntry::Custom(Action::Back, "deselect"),
    HelpEntry::Custom(Action::Select, "done"),
    HelpEntry::Pair(Action::MoveUp, Action::MoveDown, "move"),
    HelpEntry::Custom(Action::DeleteChar, "remove"),
];

/// Returns the help entries for the settings panel given its editing substate.
pub fn settings_help(mode: SettingsEditMode) -> &'static [HelpEntry] {
    match mode {
        SettingsEditMode::Navigating => SETTINGS_HELP,
        SettingsEditMode::TextEdit => SETTINGS_HELP_EDITING,
        SettingsEditMode::HsvEdit => SETTINGS_HELP_HSV,
        SettingsEditMode::ComponentList { armed } => {
            if armed {
                SETTINGS_HELP_COMPONENT_LIST_ARMED
            } else {
                SETTINGS_HELP_COMPONENT_LIST
            }
        }
    }
}

/// Ordered list of entries to show in the search help bar.
pub const SEARCH_HELP: &[HelpEntry] = &[
    HelpEntry::Single(Action::Back),
    HelpEntry::Single(Action::Select),
    HelpEntry::Single(Action::GotoSelected),
    HelpEntry::Pair(Action::MoveUp, Action::MoveDown, "up/down"),
];

/// Ordered list of entries to show in the lyrics/sidebar help bar.
pub const LYRICS_HELP: &[HelpEntry] = &[
    HelpEntry::Single(Action::Back),
    HelpEntry::Single(Action::ToggleSidebar),
    HelpEntry::Pair(Action::MoveUp, Action::MoveDown, "up/down"),
    HelpEntry::Single(Action::Select),
    HelpEntry::Pair(Action::SeekBackward, Action::SeekForward, "seek-/+"),
    HelpEntry::Single(Action::PlayPause),
    HelpEntry::Pair(Action::Next, Action::Previous, "next/prev"),
    HelpEntry::Pair(Action::NextGroup, Action::PreviousGroup, "next/prev group"),
];

/// Ordered list of entries to show in the queue help bar.
pub const QUEUE_HELP: &[HelpEntry] = &[
    HelpEntry::Single(Action::Back),
    HelpEntry::Pair(Action::MoveUp, Action::MoveDown, "up/down"),
    HelpEntry::Single(Action::Select),
    HelpEntry::Single(Action::PlayPause),
    HelpEntry::Pair(Action::Next, Action::Previous, "next/prev"),
    HelpEntry::Pair(Action::NextGroup, Action::PreviousGroup, "next/prev group"),
    HelpEntry::Single(Action::CyclePlaybackMode(Direction::Forward)),
];

/// Ordered list of entries to show in the logs help bar.
pub const LOGS_HELP: &[HelpEntry] = &[
    HelpEntry::Single(Action::Back),
    HelpEntry::Pair(Action::MoveUp, Action::MoveDown, "up/down"),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// No two distinct char-key actions may share a single Char key, since
    /// binding collisions are resolved only by call-site scoping. Char keys
    /// that are intentionally *shared* across contexts (e.g. the confirmation
    /// keys) are asserted here. The intentional overlaps:
    /// - `n`: Next (transport) / ConfirmNo (quit confirmation)
    /// - `q`: Quit (transport) / Back-in-context (settings/search: "q" maps to
    ///   Back via KEY_QUIT fallthrough; scoped by panel)
    #[test]
    fn no_distinct_char_actions_share_a_key() {
        let char_keys = [
            KEY_QUIT,            // 'q' — Quit, also Back in editing contexts
            KEY_PLAY_PAUSE,      // ' '
            KEY_STOP,            // 's'
            KEY_NEXT,            // 'n' — also ConfirmNo
            KEY_PREVIOUS,        // 'p'
            KEY_NEXT_GROUP,      // 'N'
            KEY_PREVIOUS_GROUP,  // 'P'
            KEY_CYCLE_MODE_FWD,  // 'm'
            KEY_CYCLE_MODE_BWD,  // 'M'
            KEY_TOGGLE_SORT_FWD, // 'o'
            KEY_TOGGLE_SORT_BWD, // 'O'
            KEY_SEARCH,          // '/'
            KEY_LYRICS,          // 'l'
            KEY_TOGGLE_SIDEBAR,  // 't'
            KEY_LOGS,            // 'L'
            KEY_QUEUE,           // 'u'
            KEY_VOLUME,          // 'v'
            KEY_GOTO_PLAYING,    // 'g'
            KEY_SEEK_BACK,       // '<'
            KEY_SEEK_BACK_ALT,   // ','
            KEY_SEEK_FWD,        // '>'
            KEY_SEEK_FWD_ALT,    // '.'
            KEY_STAR,            // '*'
            KEY_SETTINGS,        // 'i'
            KEY_CONFIRM_YES,     // 'y'
            KEY_CONFIRM_NO,      // 'n' — shared with Next
        ];
        let mut seen = std::collections::HashMap::new();
        // Intentional shared characters and their explanations.
        let intentional: std::collections::HashMap<char, &str> =
            [('n', "Next/ConfirmNo"), ('q', "Quit/Back-in-editing")]
                .into_iter()
                .collect();
        for key in char_keys {
            if let KeyCode::Char(c) = key {
                let action_label = match c {
                    'l' => "Lyrics",
                    't' => "ToggleSidebar",
                    'L' => "Logs",
                    'u' => "Queue",
                    'v' => "VolumeMode",
                    'g' => "GotoPlaying",
                    '*' => "Star",
                    'i' => "Settings",
                    'y' => "ConfirmYes",
                    'n' => "Next/ConfirmNo",
                    _ => "other",
                };
                let prev = seen.insert(c, action_label);
                if let Some(prev_label) = prev {
                    assert!(
                        intentional.contains_key(&c),
                        "key '{c}' is bound to both '{prev_label:?}' and '{action_label}' without an intentional-overlap entry"
                    );
                }
            }
        }
    }
}
