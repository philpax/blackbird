use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Centrally defined key actions for the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    PlayPause,
    Stop,
    Next,
    Previous,
    CyclePlaybackMode,
    Search,
    Lyrics,
    Logs,
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
    Home,
    End,
    Select,
    Back,
    ClearLine,
    Char(char),
    Backspace,
}

impl Action {
    /// Label shown in the help bar. Returns `None` for actions that
    /// shouldn't appear (navigation, text input, etc.).
    pub fn help_label(&self) -> Option<(&str, &str)> {
        match self {
            Action::Quit => Some(("q", "quit")),
            Action::PlayPause => Some(("Space", "play/pause")),
            Action::Stop => Some(("s", "stop")),
            Action::Next => Some(("n", "next")),
            Action::Previous => Some(("p", "prev")),
            Action::CyclePlaybackMode => None, // rendered separately with current mode
            Action::Search => Some(("/", "search")),
            Action::Lyrics => Some(("l", "lyrics")),
            Action::Logs => Some(("L", "logs")),
            Action::VolumeMode => Some(("v", "vol")),
            Action::Star => Some(("*", "star")),
            Action::SeekForward => Some((">", "seek+")),
            Action::SeekBackward => Some(("<", "seek-")),
            Action::GotoPlaying => Some(("g", "goto")),
            Action::Select => Some(("Enter", "play")),
            Action::Back => Some(("Esc", "close")),
            _ => None,
        }
    }
}

/// Resolve a key event into an action in library context.
pub fn library_action(key: &KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Char('q') => Some(Action::Quit),
        KeyCode::Char(' ') => Some(Action::PlayPause),
        KeyCode::Char('n') => Some(Action::Next),
        KeyCode::Char('p') => Some(Action::Previous),
        KeyCode::Char('s') => Some(Action::Stop),
        KeyCode::Char('m') => Some(Action::CyclePlaybackMode),
        KeyCode::Char('/') => Some(Action::Search),
        KeyCode::Char('l') => Some(Action::Lyrics),
        KeyCode::Char('L') => Some(Action::Logs),
        KeyCode::Char('v') => Some(Action::VolumeMode),
        KeyCode::Char('g') => Some(Action::GotoPlaying),
        KeyCode::Char('<') | KeyCode::Char(',') => Some(Action::SeekBackward),
        KeyCode::Char('>') | KeyCode::Char('.') => Some(Action::SeekForward),
        KeyCode::Char('*') => Some(Action::Star),
        KeyCode::Up => Some(Action::MoveUp),
        KeyCode::Down => Some(Action::MoveDown),
        KeyCode::PageUp => Some(Action::PageUp),
        KeyCode::PageDown => Some(Action::PageDown),
        KeyCode::Home => Some(Action::Home),
        KeyCode::End => Some(Action::End),
        KeyCode::Enter => Some(Action::Select),
        _ => None,
    }
}

/// Resolve a key event into an action in search context.
pub fn search_action(key: &KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Esc => Some(Action::Back),
        KeyCode::Enter => Some(Action::Select),
        KeyCode::Up => Some(Action::MoveUp),
        KeyCode::Down => Some(Action::MoveDown),
        KeyCode::Backspace => Some(Action::Backspace),
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
        KeyCode::Esc | KeyCode::Char('l') => Some(Action::Back),
        KeyCode::Char('q') => Some(Action::Quit),
        KeyCode::Up => Some(Action::MoveUp),
        KeyCode::Down => Some(Action::MoveDown),
        KeyCode::PageUp => Some(Action::PageUp),
        KeyCode::PageDown => Some(Action::PageDown),
        KeyCode::Char(' ') => Some(Action::PlayPause),
        KeyCode::Char('n') => Some(Action::Next),
        KeyCode::Char('p') => Some(Action::Previous),
        _ => None,
    }
}

/// Resolve a key event into an action in volume-editing context.
pub fn volume_action(key: &KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Up | KeyCode::Right => Some(Action::VolumeUp),
        KeyCode::Down | KeyCode::Left => Some(Action::VolumeDown),
        KeyCode::Esc | KeyCode::Char('v') | KeyCode::Enter => Some(Action::Back),
        _ => None,
    }
}

/// Resolve a key event into an action in logs context.
pub fn logs_action(key: &KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Esc | KeyCode::Char('L') => Some(Action::Back),
        KeyCode::Char('q') => Some(Action::Quit),
        KeyCode::Up => Some(Action::MoveUp),
        KeyCode::Down => Some(Action::MoveDown),
        KeyCode::PageUp => Some(Action::PageUp),
        KeyCode::PageDown => Some(Action::PageDown),
        KeyCode::Home => Some(Action::Home),
        KeyCode::End => Some(Action::End),
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
    Action::VolumeMode,
    Action::Select,
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
    Action::PlayPause,
    Action::Next,
    Action::Previous,
];

/// Ordered list of actions to show in the logs help bar.
pub const LOGS_HELP: &[Action] = &[Action::Back, Action::MoveUp, Action::MoveDown, Action::Quit];
