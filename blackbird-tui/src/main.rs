mod app;
mod config;
mod cover_art;
mod keys;
mod log_buffer;
mod ui;

use std::time::{Duration, Instant};

use app::{App, FocusedPanel, LibraryEntry};
use blackbird_core as bc;
use config::Config;
use cover_art::CoverArtCache;
use keys::Action;
use log_buffer::{LogBuffer, LogBufferLayer};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _};

fn main() -> anyhow::Result<()> {
    // Create log buffer for TUI display instead of stdout.
    let log_buffer = LogBuffer::new();

    tracing_subscriber::registry()
        .with(LogBufferLayer::new(log_buffer.clone()))
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("blackbird=info")),
        )
        .init();

    let config = Config::load();

    let (cover_art_loaded_tx, cover_art_loaded_rx) = std::sync::mpsc::channel::<bc::CoverArt>();
    let (lyrics_loaded_tx, lyrics_loaded_rx) = std::sync::mpsc::channel::<bc::LyricsData>();
    let (library_populated_tx, library_populated_rx) = std::sync::mpsc::channel::<()>();

    let logic = bc::Logic::new(bc::LogicArgs {
        base_url: config.shared.server.base_url.clone(),
        username: config.shared.server.username.clone(),
        password: config.shared.server.password.clone(),
        transcode: config.shared.server.transcode,
        volume: config.general.volume,
        cover_art_loaded_tx,
        lyrics_loaded_tx,
        library_populated_tx,
    });

    // Restore last playback state.
    logic.set_playback_mode(config.shared.last_playback.playback_mode);
    if let Some(track_id) = &config.shared.last_playback.track_id {
        logic.set_scroll_target(track_id);
    }

    let playback_rx = logic.subscribe_to_playback_events();
    let cover_art_cache = CoverArtCache::new(cover_art_loaded_rx);

    let mut app = App::new(
        config,
        logic,
        playback_rx,
        cover_art_cache,
        lyrics_loaded_rx,
        library_populated_rx,
        log_buffer,
    );

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let tick_rate = Duration::from_millis(app.config.general.tick_rate_ms);
    let result = run_app(&mut terminal, &mut app, tick_rate);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    // Save state on exit
    app.save_state();

    result
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
    tick_rate: Duration,
) -> anyhow::Result<()> {
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|frame| ui::draw(frame, app))?;

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)?
            && let Event::Key(key) = event::read()?
            && key.kind == event::KeyEventKind::Press
        {
            handle_key_event(app, &key);
        }

        if last_tick.elapsed() >= tick_rate {
            app.tick();
            last_tick = Instant::now();
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

fn handle_key_event(app: &mut App, key: &event::KeyEvent) {
    // Handle volume editing mode first
    if app.volume_editing {
        if let Some(action) = keys::volume_action(key) {
            match action {
                Action::VolumeUp => app.adjust_volume(0.05),
                Action::VolumeDown => app.adjust_volume(-0.05),
                Action::Back => app.volume_editing = false,
                _ => {}
            }
        }
        return;
    }

    match app.focused_panel {
        FocusedPanel::Library => {
            if let Some(action) = keys::library_action(key) {
                handle_library_action(app, action);
            }
        }
        FocusedPanel::Search => {
            if let Some(action) = keys::search_action(key) {
                handle_search_action(app, action);
            }
        }
        FocusedPanel::Lyrics => {
            if let Some(action) = keys::lyrics_action(key) {
                handle_lyrics_action(app, action);
            }
        }
        FocusedPanel::Logs => {
            if let Some(action) = keys::logs_action(key) {
                handle_logs_action(app, action);
            }
        }
    }
}

fn handle_library_action(app: &mut App, action: Action) {
    let entries_len = app.flat_library_len();

    match action {
        Action::Quit => app.should_quit = true,
        Action::PlayPause => app.logic.toggle_current(),
        Action::Next => app.logic.next(),
        Action::Previous => app.logic.previous(),
        Action::Stop => app.logic.stop_current(),
        Action::CyclePlaybackMode => app.cycle_playback_mode(),
        Action::Search => app.toggle_search(),
        Action::Lyrics => app.toggle_lyrics(),
        Action::Logs => app.toggle_logs(),
        Action::VolumeMode => app.volume_editing = true,
        Action::GotoPlaying => {
            if let Some(track_id) = app.logic.get_playing_track_id() {
                app.scroll_to_track = Some(track_id);
            }
        }
        Action::SeekBackward => app.seek_relative(-5),
        Action::SeekForward => app.seek_relative(5),
        Action::Star => {
            if let Some(entry) = app.get_library_entry(app.library_selected_index) {
                match entry {
                    LibraryEntry::Track { id, starred, .. } => {
                        app.logic.set_track_starred(&id, !starred);
                    }
                    LibraryEntry::GroupHeader {
                        album_id, starred, ..
                    } => {
                        app.logic.set_album_starred(&album_id, !starred);
                    }
                }
            }
        }
        Action::MoveUp => {
            if app.library_selected_index > 0 {
                app.library_selected_index -= 1;
            }
        }
        Action::MoveDown => {
            if entries_len > 0 && app.library_selected_index < entries_len - 1 {
                app.library_selected_index += 1;
            }
        }
        Action::PageUp => {
            app.library_selected_index = app.library_selected_index.saturating_sub(20);
        }
        Action::PageDown => {
            if entries_len > 0 {
                app.library_selected_index = (app.library_selected_index + 20).min(entries_len - 1);
            }
        }
        Action::Home => app.library_selected_index = 0,
        Action::End => {
            if entries_len > 0 {
                app.library_selected_index = entries_len - 1;
            }
        }
        Action::Select => {
            if let Some(LibraryEntry::Track { id, .. }) =
                app.get_library_entry(app.library_selected_index)
            {
                app.logic.request_play_track(&id);
            }
        }
        _ => {}
    }
}

fn handle_search_action(app: &mut App, action: Action) {
    match action {
        Action::Back => app.toggle_search(),
        Action::Select => {
            if let Some(track_id) = app.search_results.get(app.search_selected_index) {
                app.logic.request_play_track(track_id);
                app.toggle_search();
            }
        }
        Action::MoveUp => {
            if app.search_selected_index > 0 {
                app.search_selected_index -= 1;
            }
        }
        Action::MoveDown => {
            if !app.search_results.is_empty()
                && app.search_selected_index < app.search_results.len() - 1
            {
                app.search_selected_index += 1;
            }
        }
        Action::Backspace => {
            app.search_query.pop();
            app.update_search();
        }
        Action::ClearLine => {
            app.search_query.clear();
            app.update_search();
        }
        Action::Char(c) => {
            app.search_query.push(c);
            app.update_search();
        }
        _ => {}
    }
}

fn handle_lyrics_action(app: &mut App, action: Action) {
    match action {
        Action::Back => app.toggle_lyrics(),
        Action::Quit => app.should_quit = true,
        Action::MoveUp => {
            app.lyrics_scroll_offset = app.lyrics_scroll_offset.saturating_sub(1);
        }
        Action::MoveDown => {
            app.lyrics_scroll_offset += 1;
        }
        Action::PageUp => {
            app.lyrics_scroll_offset = app.lyrics_scroll_offset.saturating_sub(20);
        }
        Action::PageDown => {
            app.lyrics_scroll_offset += 20;
        }
        Action::PlayPause => app.logic.toggle_current(),
        Action::Next => app.logic.next(),
        Action::Previous => app.logic.previous(),
        _ => {}
    }
}

fn handle_logs_action(app: &mut App, action: Action) {
    let log_len = app.log_buffer.len();

    match action {
        Action::Back => app.toggle_logs(),
        Action::Quit => app.should_quit = true,
        Action::MoveUp => {
            app.logs_scroll_offset = app.logs_scroll_offset.saturating_sub(1);
        }
        Action::MoveDown => {
            if log_len > 0 {
                app.logs_scroll_offset = (app.logs_scroll_offset + 1).min(log_len - 1);
            }
        }
        Action::PageUp => {
            app.logs_scroll_offset = app.logs_scroll_offset.saturating_sub(20);
        }
        Action::PageDown => {
            if log_len > 0 {
                app.logs_scroll_offset = (app.logs_scroll_offset + 20).min(log_len - 1);
            }
        }
        Action::Home => {
            app.logs_scroll_offset = 0;
        }
        Action::End => {
            if log_len > 0 {
                app.logs_scroll_offset = log_len - 1;
            }
        }
        _ => {}
    }
}
