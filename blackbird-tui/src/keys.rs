use blackbird_core as bc;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use smol_str::{SmolStr, ToSmolStr};

/// Centrally defined key actions for the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    PlayPause,
    Stop,
    Next,
    Previous,
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
        let key_str: SmolStr = key.to_smolstr().to_lowercase().into();
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

/// Ordered list of actions to show in the library help bar.
pub const LIBRARY_HELP: &[Action] = &[
    Action::Quit,
    Action::PlayPause,
    Action::Next,
    Action::Previous,
    Action::Stop,
    Action::SeekBackward,
    Action::SeekForward,
    Action::Star,
    Action::GotoPlaying,
    Action::Search,
    Action::Lyrics,
    Action::Queue,
    Action::VolumeMode,
    Action::Select,
    Action::CyclePlaybackMode,
    Action::ToggleSortOrder,
];

/// Ordered list of actions to show in the search help bar.
pub const SEARCH_HELP: &[Action] = &[
    Action::Back,
    Action::Select,
    Action::MoveUp,
    Action::MoveDown,
];

/// Ordered list of actions to show in the lyrics help bar.
pub const LYRICS_HELP: &[Action] = &[
    Action::Back,
    Action::MoveUp,
    Action::MoveDown,
    Action::Select,
    Action::SeekBackward,
    Action::SeekForward,
    Action::PlayPause,
    Action::Next,
    Action::Previous,
];

/// Ordered list of actions to show in the queue help bar.
pub const QUEUE_HELP: &[Action] = &[
    Action::Back,
    Action::MoveUp,
    Action::MoveDown,
    Action::Select,
    Action::PlayPause,
    Action::Next,
    Action::Previous,
    Action::CyclePlaybackMode,
];

/// Ordered list of actions to show in the logs help bar.
pub const LOGS_HELP: &[Action] = &[Action::Back, Action::MoveUp, Action::MoveDown];
