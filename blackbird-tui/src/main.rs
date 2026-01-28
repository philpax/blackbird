mod app;
mod config;
mod cover_art;
mod ui;

use std::time::{Duration, Instant};

use app::{App, FocusedPanel, LibraryEntry};
use blackbird_core as bc;
use config::Config;
use cover_art::CoverArtCache;

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _};

fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("blackbird=info")),
        )
        .init();

    let config = Config::load();
    config.save();

    let (cover_art_loaded_tx, cover_art_loaded_rx) = std::sync::mpsc::channel::<bc::CoverArt>();
    let (lyrics_loaded_tx, lyrics_loaded_rx) = std::sync::mpsc::channel::<bc::LyricsData>();
    let (library_populated_tx, library_populated_rx) = std::sync::mpsc::channel::<()>();

    let logic = bc::Logic::new(bc::LogicArgs {
        base_url: config.server.base_url.clone(),
        username: config.server.username.clone(),
        password: config.server.password.clone(),
        transcode: config.server.transcode,
        volume: config.general.volume,
        cover_art_loaded_tx,
        lyrics_loaded_tx,
        library_populated_tx,
    });

    // Restore last playback mode
    logic.set_playback_mode(config.last_playback.playback_mode);

    // Set the scroll target to the last played track
    if let Some(track_id) = &config.last_playback.track_id {
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
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                handle_key_event(app, key);
            }
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

fn handle_key_event(app: &mut App, key: event::KeyEvent) {
    // Handle volume editing mode first
    if app.volume_editing {
        match key.code {
            KeyCode::Up | KeyCode::Right => app.adjust_volume(0.05),
            KeyCode::Down | KeyCode::Left => app.adjust_volume(-0.05),
            KeyCode::Esc | KeyCode::Char('v') | KeyCode::Enter => {
                app.volume_editing = false;
            }
            _ => {}
        }
        return;
    }

    match app.focused_panel {
        FocusedPanel::Library => handle_library_keys(app, key),
        FocusedPanel::Search => handle_search_keys(app, key),
        FocusedPanel::Lyrics => handle_lyrics_keys(app, key),
    }
}

fn handle_library_keys(app: &mut App, key: event::KeyEvent) {
    let entries_len = app.build_flat_library().len();

    match key.code {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Char(' ') => app.logic.toggle_current(),
        KeyCode::Char('n') => app.logic.next(),
        KeyCode::Char('p') => app.logic.previous(),
        KeyCode::Char('s') => app.logic.stop_current(),
        KeyCode::Char('m') => app.cycle_playback_mode(),
        KeyCode::Char('/') => app.toggle_search(),
        KeyCode::Char('l') => app.toggle_lyrics(),
        KeyCode::Char('v') => app.volume_editing = true,
        KeyCode::Char('g') => {
            // Go to currently playing track
            if let Some(track_id) = app.logic.get_playing_track_id() {
                app.scroll_to_track = Some(track_id);
            }
        }
        KeyCode::Char('<') | KeyCode::Char(',') => app.seek_relative(-5),
        KeyCode::Char('>') | KeyCode::Char('.') => app.seek_relative(5),
        KeyCode::Char('*') => {
            // Toggle star on selected item
            let entries = app.build_flat_library();
            if let Some(entry) = entries.get(app.library_selected_index) {
                match entry {
                    LibraryEntry::Track { id, starred, .. } => {
                        app.logic.set_track_starred(id, !starred);
                    }
                    LibraryEntry::GroupHeader {
                        album_id, starred, ..
                    } => {
                        app.logic.set_album_starred(album_id, !starred);
                    }
                }
            }
        }
        KeyCode::Up => {
            if app.library_selected_index > 0 {
                app.library_selected_index -= 1;
            }
        }
        KeyCode::Down => {
            if entries_len > 0 && app.library_selected_index < entries_len - 1 {
                app.library_selected_index += 1;
            }
        }
        KeyCode::PageUp => {
            app.library_selected_index = app.library_selected_index.saturating_sub(20);
        }
        KeyCode::PageDown => {
            if entries_len > 0 {
                app.library_selected_index =
                    (app.library_selected_index + 20).min(entries_len - 1);
            }
        }
        KeyCode::Home => {
            app.library_selected_index = 0;
        }
        KeyCode::End => {
            if entries_len > 0 {
                app.library_selected_index = entries_len - 1;
            }
        }
        KeyCode::Enter => {
            let entries = app.build_flat_library();
            if let Some(entry) = entries.get(app.library_selected_index) {
                if let LibraryEntry::Track { id, .. } = entry {
                    app.logic.request_play_track(id);
                }
            }
        }
        _ => {}
    }
}

fn handle_search_keys(app: &mut App, key: event::KeyEvent) {
    match key.code {
        KeyCode::Esc => app.toggle_search(),
        KeyCode::Enter => {
            if let Some(track_id) = app.search_results.get(app.search_selected_index) {
                app.logic.request_play_track(track_id);
                app.toggle_search();
            }
        }
        KeyCode::Up => {
            if app.search_selected_index > 0 {
                app.search_selected_index -= 1;
            }
        }
        KeyCode::Down => {
            if !app.search_results.is_empty()
                && app.search_selected_index < app.search_results.len() - 1
            {
                app.search_selected_index += 1;
            }
        }
        KeyCode::Backspace => {
            app.search_query.pop();
            app.update_search();
        }
        KeyCode::Char(c) => {
            if key.modifiers.contains(KeyModifiers::CONTROL) && c == 'u' {
                app.search_query.clear();
                app.update_search();
            } else {
                app.search_query.push(c);
                app.update_search();
            }
        }
        _ => {}
    }
}

fn handle_lyrics_keys(app: &mut App, key: event::KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('l') => app.toggle_lyrics(),
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Up => {
            app.lyrics_scroll_offset = app.lyrics_scroll_offset.saturating_sub(1);
        }
        KeyCode::Down => {
            app.lyrics_scroll_offset += 1;
        }
        KeyCode::PageUp => {
            app.lyrics_scroll_offset = app.lyrics_scroll_offset.saturating_sub(20);
        }
        KeyCode::PageDown => {
            app.lyrics_scroll_offset += 20;
        }
        KeyCode::Char(' ') => app.logic.toggle_current(),
        KeyCode::Char('n') => app.logic.next(),
        KeyCode::Char('p') => app.logic.previous(),
        _ => {}
    }
}
