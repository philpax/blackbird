use blackbird_core as bc;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use smol_str::{SmolStr, ToSmolStr};

/// An entry in the help bar, either a single action or a merged pair.
///
/// For pairs, the description is provided explicitly so that shared
/// suffixes/prefixes can be factored out (e.g. "next/prev group"
/// instead of "next group/prev group").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpEntry {
    Single(Action),
    Pair(Action, Action, &'static str),
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
    CyclePlaybackMode,
    ToggleSortOrder,
    Search,
    Lyrics,
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
    Back,
    ClearLine,
    Char(char),
    DeleteChar,
}

// ── Key code constants ───────────────────────────────────────────

pub const KEY_QUIT: KeyCode = KeyCode::Char('q');
pub const KEY_PLAY_PAUSE: KeyCode = KeyCode::Char(' ');
pub const KEY_STOP: KeyCode = KeyCode::Char('s');
pub const KEY_NEXT: KeyCode = KeyCode::Char('n');
pub const KEY_PREVIOUS: KeyCode = KeyCode::Char('p');
pub const KEY_NEXT_GROUP: KeyCode = KeyCode::Char('N');
pub const KEY_PREVIOUS_GROUP: KeyCode = KeyCode::Char('P');
pub const KEY_CYCLE_MODE: KeyCode = KeyCode::Char('m');
pub const KEY_TOGGLE_SORT: KeyCode = KeyCode::Char('o');
pub const KEY_SEARCH: KeyCode = KeyCode::Char('/');
pub const KEY_LYRICS: KeyCode = KeyCode::Char('l');
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
pub const KEY_CONFIRM_YES: KeyCode = KeyCode::Char('y');
pub const KEY_CONFIRM_NO: KeyCode = KeyCode::Char('n');

impl Action {
    /// Label shown in the help bar. Returns `None` for actions that
    /// shouldn't appear (navigation, text input, etc.).
    pub fn help_label(&self, logic: &bc::Logic) -> Option<(SmolStr, SmolStr)> {
        let (key, desc): (KeyCode, SmolStr) = match self {
            Action::Quit => (KEY_QUIT, "quit".into()),
            Action::PlayPause => {
                let label = if logic.get_playback_state() == bc::PlaybackState::Playing {
                    "pause"
                } else {
                    "play"
                };
                (KEY_PLAY_PAUSE, label.into())
            }
            Action::Stop => (KEY_STOP, "stop".into()),
            Action::Next => (KEY_NEXT, "next".into()),
            Action::Previous => (KEY_PREVIOUS, "prev".into()),
            Action::NextGroup if logic.get_playback_mode().has_group_structure() => {
                (KEY_NEXT_GROUP, "next group".into())
            }
            Action::PreviousGroup if logic.get_playback_mode().has_group_structure() => {
                (KEY_PREVIOUS_GROUP, "prev group".into())
            }
            Action::Search => (KEY_SEARCH, "search".into()),
            Action::Lyrics => (KEY_LYRICS, "lyrics".into()),
            Action::Logs => (KEY_LOGS, "logs".into()),
            Action::Queue => (KEY_QUEUE, "queue".into()),
            Action::VolumeMode => (KEY_VOLUME, "vol".into()),
            Action::Star => (KEY_STAR, "star".into()),
            Action::SeekForward => (KEY_SEEK_FWD, "seek+".into()),
            Action::SeekBackward => (KEY_SEEK_BACK, "seek-".into()),
            Action::GotoPlaying => (KEY_GOTO_PLAYING, "goto".into()),
            Action::Select => (KEY_SELECT, "play".into()),
            Action::Back => (KEY_BACK, "close".into()),
            Action::CyclePlaybackMode => {
                let mode = logic.get_playback_mode().as_str();
                (KEY_CYCLE_MODE, format!("mode ({mode})").into())
            }
            Action::ToggleSortOrder => {
                let order = logic.get_sort_order().as_str();
                (KEY_TOGGLE_SORT, format!("sort ({order})").into())
            }
            _ => return None,
        };
        let key_str: SmolStr = match key {
            // Printable non-whitespace characters are already in the correct case.
            KeyCode::Char(c) if !c.is_whitespace() => SmolStr::new(c.to_string()),
            // Everything else (Space, Enter, Esc, PageUp, etc.) uses title case
            // in crossterm's Display impl; lowercase it for the help bar.
            other => other.to_smolstr().to_lowercase().into(),
        };
        Some((key_str, desc))
    }
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
        KEY_CYCLE_MODE => Some(Action::CyclePlaybackMode),
        KEY_TOGGLE_SORT => Some(Action::ToggleSortOrder),
        KEY_SEARCH => Some(Action::Search),
        KEY_LYRICS => Some(Action::Lyrics),
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
        _ => None,
    }
}

/// Resolve a key event into an action in search context.
pub fn search_action(key: &KeyEvent) -> Option<Action> {
    match key.code {
        KEY_BACK => Some(Action::Back),
        KEY_SELECT => Some(Action::Select),
        KEY_UP => Some(Action::MoveUp),
        KEY_DOWN => Some(Action::MoveDown),
        KEY_DELETE_CHAR => Some(Action::DeleteChar),
        KeyCode::Char(c) => {
            if key.modifiers.contains(KeyModifiers::CONTROL) && c == 'u' {
                Some(Action::ClearLine)
            } else {
                Some(Action::Char(c))
            }
        }
        _ => None,
    }
}

/// Resolve a key event into an action in lyrics context.
pub fn lyrics_action(key: &KeyEvent) -> Option<Action> {
    match key.code {
        KEY_BACK | KEY_LYRICS | KEY_QUIT => Some(Action::Back),
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
        KEY_CYCLE_MODE => Some(Action::CyclePlaybackMode),
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
    HelpEntry::Single(Action::Queue),
    HelpEntry::Single(Action::VolumeMode),
    HelpEntry::Single(Action::Select),
    HelpEntry::Single(Action::CyclePlaybackMode),
    HelpEntry::Single(Action::ToggleSortOrder),
];

/// Ordered list of entries to show in the search help bar.
pub const SEARCH_HELP: &[HelpEntry] = &[
    HelpEntry::Single(Action::Back),
    HelpEntry::Single(Action::Select),
    HelpEntry::Pair(Action::MoveUp, Action::MoveDown, "up/down"),
];

/// Ordered list of entries to show in the lyrics help bar.
pub const LYRICS_HELP: &[HelpEntry] = &[
    HelpEntry::Single(Action::Back),
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
    HelpEntry::Single(Action::CyclePlaybackMode),
];

/// Ordered list of entries to show in the logs help bar.
pub const LOGS_HELP: &[HelpEntry] = &[
    HelpEntry::Single(Action::Back),
    HelpEntry::Pair(Action::MoveUp, Action::MoveDown, "up/down"),
];
