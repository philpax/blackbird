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
    );

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
    #[cfg(feature = "media-controls")] media_controls: &mut Option<
        blackbird_client_shared::controls::Controls,
    >,
) -> anyhow::Result<()> {
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|frame| ui::draw(frame, app))?;
        let term_size = terminal.size()?;
        let size = Rect::new(0, 0, term_size.width, term_size.height);

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) if key.kind == event::KeyEventKind::Press => {
                    handle_key_event(app, &key);
                }
                Event::Mouse(mouse) => {
                    handle_mouse_event(app, &mouse, size);
                }
                _ => {}
            }
        }

        if last_tick.elapsed() >= tick_rate {
            app.tick();
            #[cfg(feature = "media-controls")]
            if let Some(mc) = media_controls.as_mut() {
                mc.update();
            }
            last_tick = Instant::now();
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

fn handle_key_event(app: &mut App, key: &event::KeyEvent) {
    // Close album art overlay on Escape or any key.
    if app.album_art_overlay.is_some() {
        if matches!(
            key.code,
            event::KeyCode::Esc | event::KeyCode::Char('q') | event::KeyCode::Enter
        ) {
            app.album_art_overlay = None;
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
            if let Some(action) = keys::search_action(key) {
                ui::search::handle_key(app, action);
            }
        }
        FocusedPanel::Lyrics => {
            if let Some(action) = keys::lyrics_action(key) {
                ui::lyrics::handle_key(app, action);
            }
        }
        FocusedPanel::Logs => {
            if let Some(action) = keys::logs_action(key) {
                ui::logs::handle_key(app, action);
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

    let x = mouse.column;
    let y = mouse.row;

    match mouse.kind {
        MouseEventKind::Moved => {
            app.mouse_position = Some((x, y));
        }
        MouseEventKind::Down(MouseButton::Left) => {
            app.mouse_position = Some((x, y));

            // --- Album art overlay (handled first, on top of everything) ---
            if app.album_art_overlay.is_some() {
                if ui::album_art_overlay::is_x_button_click(app, size, x, y) {
                    app.album_art_overlay = None;
                } else if let Some(rect) = ui::album_art_overlay::overlay_rect(app, size) {
                    // Click inside the overlay but not on X → ignore
                    if x >= rect.x
                        && x < rect.x + rect.width
                        && y >= rect.y
                        && y < rect.y + rect.height
                    {
                        return;
                    }
                    // Click outside overlay → close it
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

            // --- Library area ---
            if y >= library_area.y
                && y < library_area.y + library_area.height
                && x >= library_area.x
                && x < library_area.x + library_area.width
            {
                if app.focused_panel == FocusedPanel::Library {
                    ui::library::handle_mouse_click(app, library_area, x, y);
                } else if app.focused_panel == FocusedPanel::Lyrics {
                    ui::lyrics::handle_mouse_click(app, library_area, x, y);
                }
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
                ui::lyrics::move_selection(app, -(ui::layout::SCROLL_WHEEL_STEPS as i32));
            } else if app.focused_panel == FocusedPanel::Logs {
                app.logs_scroll_offset = app
                    .logs_scroll_offset
                    .saturating_sub(ui::layout::SCROLL_WHEEL_STEPS);
            }
        }
        MouseEventKind::ScrollDown => {
            if app.focused_panel == FocusedPanel::Library {
                ui::library::handle_scroll(app, 1, ui::layout::SCROLL_WHEEL_STEPS);
            } else if app.focused_panel == FocusedPanel::Lyrics {
                ui::lyrics::move_selection(app, ui::layout::SCROLL_WHEEL_STEPS as i32);
            } else if app.focused_panel == FocusedPanel::Logs {
                let log_len = app.log_buffer.len();
                if log_len > 0 {
                    app.logs_scroll_offset =
                        (app.logs_scroll_offset + ui::layout::SCROLL_WHEEL_STEPS).min(log_len - 1);
                }
            }
        }
        _ => {}
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
