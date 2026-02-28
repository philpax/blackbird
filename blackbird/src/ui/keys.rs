use std::borrow::Cow;

use blackbird_core as bc;
use egui::Key;

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

impl HelpEntry {
    /// Returns the combined key label and description for this help entry.
    /// For pairs, keys are joined with `/` and the explicit description is used.
    pub fn help_label(&self, logic: &bc::Logic) -> Option<(Cow<'static, str>, Cow<'static, str>)> {
        match self {
            HelpEntry::Single(a) => a.help_label(logic),
            HelpEntry::Pair(a, b, desc) => {
                let la = a.help_label(logic);
                let lb = b.help_label(logic);
                match (la, lb) {
                    (Some((ka, _)), Some((kb, _))) => {
                        Some((format!("{ka}/{kb}").into(), Cow::Borrowed(desc)))
                    }
                    (Some(l), None) | (None, Some(l)) => Some(l),
                    (None, None) => None,
                }
            }
        }
    }
}

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
pub const KEY_QUEUE: Key = Key::U;
pub const KEY_QUIT: Key = Key::Q;
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
    NextGroup,
    PreviousGroup,
    CyclePlaybackMode,
    ToggleSortOrder,
    Star,
    SeekForward,
    SeekBackward,
    GotoPlaying,
    SearchInline,
    Lyrics,
    Queue,
    Quit,
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
            Action::NextGroup => KEY_NEXT,
            Action::PreviousGroup => KEY_PREVIOUS,
            Action::CyclePlaybackMode => KEY_CYCLE_MODE,
            Action::ToggleSortOrder => KEY_TOGGLE_SORT,
            Action::Star => KEY_STAR,
            Action::SeekForward => KEY_SEEK_FWD,
            Action::SeekBackward => KEY_SEEK_BACK,
            Action::GotoPlaying => KEY_GOTO_PLAYING,
            Action::SearchInline => KEY_SEARCH_INLINE,
            Action::Lyrics => KEY_LYRICS,
            Action::Queue => KEY_QUEUE,
            Action::Quit => KEY_QUIT,
            Action::VolumeUp => KEY_VOLUME_UP,
            Action::VolumeDown => KEY_VOLUME_DOWN,
        }
    }

    /// Returns the key label and description for display in the help bar.
    /// Returns `None` for actions that shouldn't appear in the current context.
    pub fn help_label(&self, logic: &bc::Logic) -> Option<(Cow<'static, str>, Cow<'static, str>)> {
        // Group skip actions are only shown in group modes.
        if matches!(self, Action::NextGroup | Action::PreviousGroup)
            && !logic.get_playback_mode().is_group_mode()
        {
            return None;
        }

        let key_label: Cow<'static, str> = match self {
            // Star is Shift+8, so we display '*' instead of '8'.
            Action::Star => "*".into(),
            // Shifted actions: display the key in uppercase.
            Action::NextGroup | Action::PreviousGroup => {
                self.key().symbol_or_name().to_string().into()
            }
            _ => self.key().symbol_or_name().to_lowercase().into(),
        };

        let description: Cow<'static, str> = match self {
            Action::PlayPause => {
                if logic.get_playback_state() == bc::PlaybackState::Playing {
                    "pause".into()
                } else {
                    "play".into()
                }
            }
            Action::Stop => "stop".into(),
            Action::Next => "next".into(),
            Action::Previous => "prev".into(),
            Action::NextGroup => "next group".into(),
            Action::PreviousGroup => "prev group".into(),
            Action::CyclePlaybackMode => {
                format!("mode ({})", logic.get_playback_mode().as_str()).into()
            }
            Action::ToggleSortOrder => format!("sort ({})", logic.get_sort_order().as_str()).into(),
            Action::Star => "star".into(),
            Action::SeekForward => "seek+".into(),
            Action::SeekBackward => "seek-".into(),
            Action::GotoPlaying => "goto".into(),
            Action::SearchInline => "search".into(),
            Action::Lyrics => "lyrics".into(),
            Action::Queue => "queue".into(),
            Action::Quit => "quit".into(),
            Action::VolumeUp => "vol+".into(),
            Action::VolumeDown => "vol-".into(),
        };

        Some((key_label, description))
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
    HelpEntry::Single(Action::SearchInline),
    HelpEntry::Single(Action::Lyrics),
    HelpEntry::Single(Action::Queue),
    HelpEntry::Pair(Action::VolumeUp, Action::VolumeDown, "vol+/-"),
    HelpEntry::Single(Action::CyclePlaybackMode),
    HelpEntry::Single(Action::ToggleSortOrder),
];

/// Maps a key press to a library action.
/// Returns None if the key is not a shortcut.
pub fn library_action(key: Key, shift: bool) -> Option<Action> {
    match key {
        KEY_PLAY_PAUSE => Some(Action::PlayPause),
        KEY_STOP => Some(Action::Stop),
        KEY_NEXT if shift => Some(Action::NextGroup),
        KEY_NEXT => Some(Action::Next),
        KEY_PREVIOUS if shift => Some(Action::PreviousGroup),
        KEY_PREVIOUS => Some(Action::Previous),
        KEY_CYCLE_MODE => Some(Action::CyclePlaybackMode),
        KEY_TOGGLE_SORT => Some(Action::ToggleSortOrder),
        KEY_SEEK_BACK => Some(Action::SeekBackward),
        KEY_SEEK_FWD => Some(Action::SeekForward),
        KEY_GOTO_PLAYING => Some(Action::GotoPlaying),
        KEY_SEARCH_INLINE => Some(Action::SearchInline),
        KEY_LYRICS => Some(Action::Lyrics),
        KEY_QUEUE => Some(Action::Queue),
        KEY_QUIT => Some(Action::Quit),
        // '*' is Shift+8.
        KEY_STAR if shift => Some(Action::Star),
        KEY_VOLUME_UP => Some(Action::VolumeUp),
        KEY_VOLUME_DOWN => Some(Action::VolumeDown),
        _ => None,
    }
}
