mod app;
mod config;
mod cover_art;
mod keys;
mod log_buffer;
mod ui;

use std::time::{Duration, Instant};

use app::{App, FocusedPanel};
use blackbird_core as bc;
use config::Config;
use cover_art::CoverArtCache;
use keys::Action;
use log_buffer::{LogBuffer, LogBufferLayer};

use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, MouseButton, MouseEvent,
        MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::layout::Rect;
use ratatui::{Terminal, backend::CrosstermBackend};
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _};

fn main() -> anyhow::Result<()> {
    // Create log buffer for TUI display instead of stdout.
    let log_buffer = LogBuffer::new();

    // Also log to a file for debugging (especially shutdown issues).
    let log_file = std::fs::File::create("blackbird-tui.log")?;
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::sync::Mutex::new(log_file))
        .with_ansi(false);

    tracing_subscriber::registry()
        .with(LogBufferLayer::new(log_buffer.clone()))
        .with(file_layer)
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
        sort_order: config.shared.last_playback.sort_order,
        playback_mode: config.shared.last_playback.playback_mode,
        last_playback: config.shared.last_playback.as_track_and_position(),
        cover_art_loaded_tx,
        lyrics_loaded_tx,
        library_populated_tx,
    });

    // Initialize platform-specific tray icon requirements (GTK on Linux).
    #[cfg(feature = "tray-icon")]
    blackbird_client_shared::tray::init_platform();

    // Initialize media controls (MPRIS on Linux, SMTC on Windows) for global playback keys.
    #[cfg(feature = "media-controls")]
    let mut media_controls = blackbird_client_shared::controls::Controls::new(
        {
            #[cfg(target_os = "windows")]
            {
                create_hidden_media_window()
            }
            #[cfg(not(target_os = "windows"))]
            {
                None
            }
        },
        logic.subscribe_to_playback_events(),
        logic.request_handle(),
        logic.get_state(),
    )
    .map_err(|e| tracing::warn!("Failed to initialize media controls: {e}"))
    .ok();

    // Create tray icon and menu.
    #[cfg(feature = "tray-icon")]
    let (tray_icon, mut tray_menu) = {
        let icon = blackbird_client_shared::load_icon();
        blackbird_client_shared::tray::TrayMenu::new(icon, logic.get_playback_mode())
    };

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
    let result = run_app(
        &mut terminal,
        &mut app,
        tick_rate,
        #[cfg(feature = "media-controls")]
        &mut media_controls,
        #[cfg(feature = "tray-icon")]
        &mut tray_menu,
        #[cfg(feature = "tray-icon")]
        &tray_icon,
    );

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    // Save state on exit.
    app.save_state();

    // Drop app first — this drops Logic, which sends Shutdown to the playback
    // thread and stops audio. Must happen before tray/media_controls, whose
    // destructors block for tens of seconds on D-Bus/GLib cleanup.
    drop(app);

    // TrayIcon/TrayMenu/Controls destructors do synchronous D-Bus/GLib calls
    // that block when the GLib main context isn't being actively iterated.
    // Skip all their destructors — the process exit handles cleanup.
    #[cfg(feature = "tray-icon")]
    {
        std::mem::forget(tray_icon);
        std::mem::forget(tray_menu);
    }
    #[cfg(feature = "media-controls")]
    std::mem::forget(media_controls);

    result
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
    tick_rate: Duration,
    #[cfg(feature = "media-controls")] media_controls: &mut Option<
        blackbird_client_shared::controls::Controls,
    >,
    #[cfg(feature = "tray-icon")] tray_menu: &mut blackbird_client_shared::tray::TrayMenu,
    #[cfg(feature = "tray-icon")] tray_icon: &blackbird_client_shared::tray::TrayIcon,
) -> anyhow::Result<()> {
    let mut last_tick = Instant::now();

    loop {
        if app.needs_redraw {
            terminal.draw(|frame| ui::draw(frame, app))?;
            app.needs_redraw = false;
        }
        let term_size = terminal.size()?;
        let size = Rect::new(0, 0, term_size.width, term_size.height);

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            let mut scroll_delta: i32 = 0;

            // Process the first event, then drain all remaining queued events.
            let mut process_event = |evt: Event, app: &mut App| match evt {
                Event::Key(key) if key.kind == event::KeyEventKind::Press => {
                    handle_key_event(app, &key);
                    app.needs_redraw = true;
                }
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollUp => {
                        scroll_delta -= 1;
                        app.needs_redraw = true;
                    }
                    MouseEventKind::ScrollDown => {
                        scroll_delta += 1;
                        app.needs_redraw = true;
                    }
                    _ => {
                        handle_mouse_event(app, &mouse, size);
                        app.needs_redraw = true;
                    }
                },
                Event::Resize(_, _) => {
                    app.needs_redraw = true;
                }
                _ => {}
            };

            process_event(event::read()?, app);
            while event::poll(Duration::ZERO)? {
                process_event(event::read()?, app);
            }

            // Apply coalesced scroll as a single operation.
            if scroll_delta != 0 {
                apply_scroll(app, scroll_delta);
            }
        }

        if last_tick.elapsed() >= tick_rate {
            app.tick();
            #[cfg(feature = "media-controls")]
            if let Some(mc) = media_controls.as_mut() {
                mc.update();
            }
            #[cfg(feature = "tray-icon")]
            {
                if let Some(action) = tray_menu.handle_menu_events(&app.logic) {
                    match action {
                        blackbird_client_shared::tray::TrayAction::Quit => {
                            app.should_quit = true;
                        }
                        blackbird_client_shared::tray::TrayAction::Repaint
                        | blackbird_client_shared::tray::TrayAction::FocusWindow => {}
                    }
                }
                // Drain icon events to prevent accumulation.
                let _ = tray_menu.handle_icon_events();
                tray_menu.update(&app.logic, tray_icon);
                blackbird_client_shared::tray::pump_platform_events();
            }
            last_tick = Instant::now();
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

fn handle_key_event(app: &mut App, key: &event::KeyEvent) {
    // Close album art overlay on Escape, q, or Enter.
    if app.album_art_overlay.is_some() {
        if keys::album_art_overlay_action(key).is_some() {
            app.album_art_overlay = None;
        }
        return;
    }

    // Handle quit confirmation dialog
    if app.quit_confirming {
        match keys::quit_confirm_action(key) {
            Action::Select => app.should_quit = true,
            _ => app.quit_confirming = false,
        }
        return;
    }

    // Handle playback mode dropdown.
    if app.playback_mode_dropdown {
        if let Some(action) = keys::playback_mode_dropdown_action(key) {
            let modes = blackbird_core::PlaybackMode::ALL;
            match action {
                Action::Back | Action::Select => {
                    app.playback_mode_dropdown = false;
                }
                Action::MoveUp => {
                    let current = app.logic.get_playback_mode();
                    let idx = modes.iter().position(|m| *m == current).unwrap_or(0);
                    let prev = if idx == 0 { modes.len() - 1 } else { idx - 1 };
                    app.logic.set_playback_mode(modes[prev]);
                }
                Action::MoveDown => {
                    let current = app.logic.get_playback_mode();
                    let idx = modes.iter().position(|m| *m == current).unwrap_or(0);
                    let next = (idx + 1) % modes.len();
                    app.logic.set_playback_mode(modes[next]);
                }
                _ => {}
            }
        }
        return;
    }

    // Handle volume editing mode first
    if app.volume_editing {
        if let Some(action) = keys::volume_action(key) {
            match action {
                Action::VolumeUp => app.adjust_volume(ui::layout::VOLUME_STEP),
                Action::VolumeDown => app.adjust_volume(-ui::layout::VOLUME_STEP),
                Action::Back => app.volume_editing = false,
                _ => {}
            }
        }
        return;
    }

    match app.focused_panel {
        FocusedPanel::Library => {
            if let Some(action) = keys::library_action(key) {
                ui::library::handle_key(app, action);
            }
        }
        FocusedPanel::Search => {
            if let Some(action) = keys::search_action(key)
                && let Some(sa) = ui::search::handle_key(&mut app.search, &app.logic, action)
            {
                match sa {
                    ui::search::SearchAction::ToggleSearch => app.toggle_search(),
                }
            }
        }
        FocusedPanel::Lyrics => {
            if let Some(action) = keys::lyrics_action(key)
                && let Some(la) = ui::lyrics::handle_key(&mut app.lyrics, &app.logic, action)
            {
                match la {
                    ui::lyrics::LyricsAction::ToggleLyrics => app.toggle_lyrics(),
                    ui::lyrics::LyricsAction::Quit => app.should_quit = true,
                    ui::lyrics::LyricsAction::SeekRelative(secs) => app.seek_relative(secs),
                }
            }
        }
        FocusedPanel::Logs => {
            if let Some(action) = keys::logs_action(key)
                && let Some(la) = ui::logs::handle_key(&mut app.logs, action)
            {
                match la {
                    ui::logs::LogsAction::ToggleLogs => app.toggle_logs(),
                    ui::logs::LogsAction::Quit => app.should_quit = true,
                }
            }
        }
        FocusedPanel::Queue => {
            if let Some(action) = keys::queue_action(key)
                && let Some(qa) = ui::queue::handle_key(&mut app.queue, &app.logic, action)
            {
                match qa {
                    ui::queue::QueueAction::ToggleQueue => app.toggle_queue(),
                    ui::queue::QueueAction::Quit => app.should_quit = true,
                }
            }
        }
    }
}

fn handle_mouse_event(app: &mut App, mouse: &MouseEvent, size: Rect) {
    // Compute layout areas matching ui::draw
    let main = ui::layout::split_main(size);

    let now_playing_area = main.now_playing;
    let scrub_area = main.scrub_bar;
    let library_area = main.content;
    let help_bar_area = main.help_bar;

    let x = mouse.column;
    let y = mouse.row;

    // Check whether the cursor is over the inline lyrics overlay so we can
    // block interactions that would otherwise reach the library underneath.
    let over_inline_lyrics = app.config.shared.show_inline_lyrics
        && app.lyrics.shared.has_synced_lyrics()
        && ui::layout::inline_lyrics_overlay(main.content)
            .is_some_and(|r| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height);

    match mouse.kind {
        MouseEventKind::Moved => {
            // Suppress hover position when cursor is over the overlay so
            // library rows underneath don't get underlined.
            if over_inline_lyrics {
                app.mouse_position = None;
            } else {
                app.mouse_position = Some((x, y));
            }
        }
        MouseEventKind::Down(MouseButton::Left) => {
            app.mouse_position = Some((x, y));

            // --- Playback mode dropdown (handled before other areas) ---
            if app.playback_mode_dropdown {
                let dropdown_rect = ui::now_playing::playback_mode_dropdown_rect(size);
                let inner = Rect::new(
                    dropdown_rect.x + 1,
                    dropdown_rect.y + 1,
                    dropdown_rect.width.saturating_sub(2),
                    dropdown_rect.height.saturating_sub(2),
                );
                if x >= inner.x
                    && x < inner.x + inner.width
                    && y >= inner.y
                    && y < inner.y + inner.height
                {
                    let idx = (y - inner.y) as usize;
                    let modes = blackbird_core::PlaybackMode::ALL;
                    if idx < modes.len() {
                        app.logic.set_playback_mode(modes[idx]);
                        app.playback_mode_dropdown = false;
                    }
                } else {
                    app.playback_mode_dropdown = false;
                }
                return;
            }

            // --- Album art overlay (handled first, on top of everything) ---
            if app.album_art_overlay.is_some() {
                let aspect_ratio = app
                    .cover_art_cache
                    .get_aspect_ratio(app.album_art_overlay.as_ref().map(|o| &o.cover_art_id));
                let rect = ui::layout::overlay_rect(size, aspect_ratio);
                if ui::album_art_overlay::is_x_button_click(size, aspect_ratio, x, y) {
                    app.album_art_overlay = None;
                } else if x >= rect.x
                    && x < rect.x + rect.width
                    && y >= rect.y
                    && y < rect.y + rect.height
                {
                    // Click inside overlay but not on X → ignore
                } else {
                    app.album_art_overlay = None;
                }
                return;
            }

            // --- Now Playing area ---
            if y >= now_playing_area.y && y < now_playing_area.y + now_playing_area.height {
                ui::now_playing::handle_mouse_click(app, now_playing_area, x, y);
                return;
            }

            // --- Scrub bar / Volume area ---
            if y == scrub_area.y && x >= scrub_area.x && x < scrub_area.x + scrub_area.width {
                ui::handle_scrub_volume_click(app, scrub_area, x);
                return;
            }

            // --- Inline lyrics overlay (absorbs clicks) ---
            if over_inline_lyrics {
                return;
            }

            // --- Library area ---
            if y >= library_area.y
                && y < library_area.y + library_area.height
                && x >= library_area.x
                && x < library_area.x + library_area.width
            {
                if app.focused_panel == FocusedPanel::Library {
                    ui::library::handle_mouse_click(app, library_area, x, y);
                } else if app.focused_panel == FocusedPanel::Lyrics {
                    ui::lyrics::handle_mouse_click(&mut app.lyrics, &app.logic, library_area, x, y);
                } else if app.focused_panel == FocusedPanel::Queue {
                    ui::queue::handle_mouse_click(&mut app.queue, &app.logic, library_area, x, y);
                }
                return;
            }

            // --- Help bar area ---
            if y >= help_bar_area.y && y < help_bar_area.y + help_bar_area.height {
                handle_help_bar_click(app, x);
            }
        }
        MouseEventKind::Up(MouseButton::Left) => {
            ui::library::handle_mouse_up(app);
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            app.mouse_position = Some((x, y));

            if app.focused_panel == FocusedPanel::Library
                && ui::library::handle_mouse_drag(app, library_area, x, y)
            {
                return;
            }

            // Scrub bar drag → seek
            if y == scrub_area.y && x >= scrub_area.x && x < scrub_area.x + scrub_area.width {
                ui::handle_scrub_volume_click(app, scrub_area, x);
            }
        }
        MouseEventKind::ScrollUp => {
            if app.focused_panel == FocusedPanel::Library {
                ui::library::handle_scroll(app, -1, ui::layout::SCROLL_WHEEL_STEPS);
            } else if app.focused_panel == FocusedPanel::Lyrics {
                ui::lyrics::move_selection(
                    &mut app.lyrics,
                    app.logic.get_playing_position(),
                    -(ui::layout::SCROLL_WHEEL_STEPS as i32),
                );
            } else if app.focused_panel == FocusedPanel::Queue {
                ui::queue::scroll_selection(
                    &mut app.queue,
                    &app.logic,
                    -(ui::layout::SCROLL_WHEEL_STEPS as i32),
                );
            } else if app.focused_panel == FocusedPanel::Logs {
                app.logs.scroll_offset = app
                    .logs
                    .scroll_offset
                    .saturating_sub(ui::layout::SCROLL_WHEEL_STEPS);
            }
        }
        MouseEventKind::ScrollDown => {
            if app.focused_panel == FocusedPanel::Library {
                ui::library::handle_scroll(app, 1, ui::layout::SCROLL_WHEEL_STEPS);
            } else if app.focused_panel == FocusedPanel::Lyrics {
                ui::lyrics::move_selection(
                    &mut app.lyrics,
                    app.logic.get_playing_position(),
                    ui::layout::SCROLL_WHEEL_STEPS as i32,
                );
            } else if app.focused_panel == FocusedPanel::Queue {
                ui::queue::scroll_selection(
                    &mut app.queue,
                    &app.logic,
                    ui::layout::SCROLL_WHEEL_STEPS as i32,
                );
            } else if app.focused_panel == FocusedPanel::Logs {
                let log_len = app.logs.log_buffer.len();
                if log_len > 0 {
                    app.logs.scroll_offset =
                        (app.logs.scroll_offset + ui::layout::SCROLL_WHEEL_STEPS).min(log_len - 1);
                }
            }
        }
        _ => {}
    }
}

fn handle_help_bar_click(app: &mut App, x: u16) {
    let Some(&(_, _, action)) = app
        .help_bar_items
        .iter()
        .find(|(x_start, x_end, _)| x >= *x_start && x < *x_end)
    else {
        return;
    };

    match action {
        Action::Quit => app.quit_confirming = true,
        Action::PlayPause => app.logic.toggle_current(),
        Action::Next => app.logic.next(),
        Action::Previous => app.logic.previous(),
        Action::NextGroup => app.logic.next_group(),
        Action::PreviousGroup => app.logic.previous_group(),
        Action::Stop => app.logic.stop_current(),
        Action::Search => app.toggle_search(),
        Action::Lyrics => app.toggle_lyrics(),
        Action::Queue => app.toggle_queue(),
        Action::Logs => app.toggle_logs(),
        Action::VolumeMode => app.volume_editing = !app.volume_editing,
        Action::Star => {
            if let Some(track_id) = app.logic.get_playing_track_id() {
                let state = app.logic.get_state();
                let starred = state
                    .read()
                    .unwrap()
                    .library
                    .track_map
                    .get(&track_id)
                    .is_some_and(|t| t.starred);
                app.logic.set_track_starred(&track_id, !starred);
            }
        }
        Action::SeekForward => app.seek_relative(ui::layout::SEEK_STEP_SECS),
        Action::SeekBackward => app.seek_relative(-ui::layout::SEEK_STEP_SECS),
        Action::GotoPlaying => {
            if let Some(track_id) = app.logic.get_playing_track_id() {
                app.logic.set_scroll_target(&track_id);
                app.library.scroll_to_track = Some(track_id);
            }
        }
        Action::CyclePlaybackMode => app.cycle_playback_mode(),
        Action::ToggleSortOrder => {
            let scroll_target = app.library.selected_track_id().cloned();
            let next = blackbird_client_shared::toggle_sort_order(app.logic.get_sort_order());
            app.logic.set_sort_order(next);
            app.library.mark_dirty();
            app.library.scroll_to_track = scroll_target;
        }
        Action::Select => {
            if app.focused_panel == FocusedPanel::Library {
                ui::library::handle_key(app, Action::Select);
            }
        }
        Action::Back => {
            if app.focused_panel != FocusedPanel::Library {
                app.focused_panel = FocusedPanel::Library;
            }
        }
        _ => {}
    }
}

/// Applies a coalesced scroll delta to the currently focused panel.
fn apply_scroll(app: &mut App, scroll_delta: i32) {
    let steps = scroll_delta.unsigned_abs() as usize * ui::layout::SCROLL_WHEEL_STEPS;
    let direction = scroll_delta.signum();

    match app.focused_panel {
        FocusedPanel::Library => {
            ui::library::handle_scroll(app, direction, steps);
        }
        FocusedPanel::Lyrics => {
            ui::lyrics::move_selection(
                &mut app.lyrics,
                app.logic.get_playing_position(),
                direction * steps as i32,
            );
        }
        FocusedPanel::Queue => {
            ui::queue::scroll_selection(&mut app.queue, &app.logic, direction * steps as i32);
        }
        FocusedPanel::Logs => {
            if direction < 0 {
                app.logs.scroll_offset = app.logs.scroll_offset.saturating_sub(steps);
            } else {
                let log_len = app.logs.log_buffer.len();
                if log_len > 0 {
                    app.logs.scroll_offset = (app.logs.scroll_offset + steps).min(log_len - 1);
                }
            }
        }
        FocusedPanel::Search => {}
    }
}

/// Create a hidden Win32 window to act as a proxy for SMTC media controls.
/// Console windows don't support SMTC, so we create our own invisible window.
#[cfg(all(target_os = "windows", feature = "media-controls"))]
fn create_hidden_media_window() -> Option<*mut std::ffi::c_void> {
    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, RegisterClassW, WINDOW_EX_STYLE, WNDCLASSW,
        WS_OVERLAPPEDWINDOW,
    };
    use windows::core::w;

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    unsafe {
        let instance = GetModuleHandleW(None).ok()?;
        let hinstance = HINSTANCE(instance.0);
        let class_name = w!("BlackbirdMediaHidden");

        let wc = WNDCLASSW {
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinstance,
            lpszClassName: class_name,
            ..Default::default()
        };
        RegisterClassW(&wc);

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("Blackbird"),
            WS_OVERLAPPEDWINDOW,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hinstance),
            None,
        )
        .ok()?;

        Some(hwnd.0)
    }
}
