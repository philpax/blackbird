use std::borrow::Cow;

use egui::Key;

// ── Key constants ───────────────────────────────────────────────────

pub const KEY_PLAY_PAUSE: Key = Key::Space;
pub const KEY_STOP: Key = Key::S;
pub const KEY_NEXT: Key = Key::N;
pub const KEY_PREVIOUS: Key = Key::P;
pub const KEY_CYCLE_MODE: Key = Key::M;
pub const KEY_SEEK_BACK: Key = Key::Comma;
pub const KEY_SEEK_FWD: Key = Key::Period;
pub const KEY_GOTO_PLAYING: Key = Key::G;
pub const KEY_SEARCH_INLINE: Key = Key::Slash;
pub const KEY_LYRICS: Key = Key::L;
pub const KEY_STAR: Key = Key::Num8; // '*' is Shift+8
pub const KEY_VOLUME_UP: Key = Key::ArrowUp;
pub const KEY_VOLUME_DOWN: Key = Key::ArrowDown;
pub const KEY_TOGGLE_SORT: Key = Key::O;

/// Actions that can be triggered by keyboard shortcuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    PlayPause,
    Stop,
    Next,
    Previous,
    CyclePlaybackMode,
    ToggleSortOrder,
    Star,
    SeekForward,
    SeekBackward,
    GotoPlaying,
    SearchInline,
    Lyrics,
    VolumeUp,
    VolumeDown,
}

impl Action {
    /// Returns the key associated with this action.
    pub fn key(&self) -> Key {
        match self {
            Action::PlayPause => KEY_PLAY_PAUSE,
            Action::Stop => KEY_STOP,
            Action::Next => KEY_NEXT,
            Action::Previous => KEY_PREVIOUS,
            Action::CyclePlaybackMode => KEY_CYCLE_MODE,
            Action::ToggleSortOrder => KEY_TOGGLE_SORT,
            Action::Star => KEY_STAR,
            Action::SeekForward => KEY_SEEK_FWD,
            Action::SeekBackward => KEY_SEEK_BACK,
            Action::GotoPlaying => KEY_GOTO_PLAYING,
            Action::SearchInline => KEY_SEARCH_INLINE,
            Action::Lyrics => KEY_LYRICS,
            Action::VolumeUp => KEY_VOLUME_UP,
            Action::VolumeDown => KEY_VOLUME_DOWN,
        }
    }

    /// Returns the key label and description for display in the help bar.
    pub fn help_label(&self) -> (Cow<'static, str>, &'static str) {
        let key_label = match self {
            // Star is Shift+8, so we display '*' instead of '8'.
            Action::Star => Cow::Borrowed("*"),
            _ => Cow::Borrowed(self.key().symbol_or_name()),
        };

        let description = match self {
            Action::PlayPause => "play",
            Action::Stop => "stop",
            Action::Next => "next",
            Action::Previous => "prev",
            Action::CyclePlaybackMode => "mode",
            Action::ToggleSortOrder => "order",
            Action::Star => "star",
            Action::SeekForward => "seek+",
            Action::SeekBackward => "seek-",
            Action::GotoPlaying => "goto",
            Action::SearchInline => "search",
            Action::Lyrics => "lyrics",
            Action::VolumeUp => "vol+",
            Action::VolumeDown => "vol-",
        };

        (key_label, description)
    }
}

/// Ordered list of actions to show in the library help bar.
pub const LIBRARY_HELP: &[Action] = &[
    Action::PlayPause,
    Action::Next,
    Action::Previous,
    Action::Stop,
    Action::SeekBackward,
    Action::SeekForward,
    Action::Star,
    Action::GotoPlaying,
    Action::SearchInline,
    Action::Lyrics,
    Action::VolumeUp,
    Action::VolumeDown,
    Action::CyclePlaybackMode,
    Action::ToggleSortOrder,
];

/// Maps a key press to a library action.
/// Returns None if the key is not a shortcut.
pub fn library_action(key: Key, shift: bool) -> Option<Action> {
    match key {
        KEY_PLAY_PAUSE => Some(Action::PlayPause),
        KEY_STOP => Some(Action::Stop),
        KEY_NEXT => Some(Action::Next),
        KEY_PREVIOUS => Some(Action::Previous),
        KEY_CYCLE_MODE => Some(Action::CyclePlaybackMode),
        KEY_TOGGLE_SORT => Some(Action::ToggleSortOrder),
        KEY_SEEK_BACK => Some(Action::SeekBackward),
        KEY_SEEK_FWD => Some(Action::SeekForward),
        KEY_GOTO_PLAYING => Some(Action::GotoPlaying),
        KEY_SEARCH_INLINE => Some(Action::SearchInline),
        KEY_LYRICS => Some(Action::Lyrics),
        // '*' is Shift+8.
        KEY_STAR if shift => Some(Action::Star),
        KEY_VOLUME_UP => Some(Action::VolumeUp),
        KEY_VOLUME_DOWN => Some(Action::VolumeDown),
        _ => None,
    }
}
