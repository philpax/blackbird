mod app;
mod config;
mod cover_art;
mod keys;
mod log_buffer;
mod ui;

use std::io::Write as _;
use std::time::{Duration, Instant};

use app::{App, FocusedPanel};
use blackbird_core as bc;
use blackbird_shared::config::ConfigFile as _;
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
use ratatui::layout::{Position, Rect};
use ratatui::{Terminal, backend::CrosstermBackend};
use ratatui_image::picker::{Capability, Picker, ProtocolType};
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _};

fn main() -> anyhow::Result<()> {
    // Create log buffer for TUI display instead of stdout.
    let log_buffer = LogBuffer::new();

    // Also log to a file for debugging (especially shutdown issues).
    let log_dir = blackbird_shared::paths::data_dir();
    std::fs::create_dir_all(&log_dir)?;
    let log_file = std::fs::File::create(log_dir.join("blackbird.log"))?;
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
    let (similar_songs_loaded_tx, similar_songs_loaded_rx) =
        std::sync::mpsc::channel::<bc::SimilarSongsData>();
    let (library_populated_tx, library_populated_rx) = std::sync::mpsc::channel::<()>();
    let (track_updated_tx, track_updated_rx) = std::sync::mpsc::channel::<()>();

    let logic = bc::Logic::new(bc::LogicArgs {
        base_url: config.server.base_url.clone(),
        username: config.server.username.clone(),
        password: config.server.password.clone(),
        transcode: config.server.transcode,
        volume: config.general.volume,
        apply_replaygain: config.playback.apply_replaygain,
        replaygain_preamp_db: config.playback.replaygain_preamp_db,
        sort_order: config.last_playback.sort_order,
        playback_mode: config.last_playback.playback_mode,
        last_playback: config.last_playback.as_track_and_position(),
        cover_art_loaded_tx,
        lyrics_loaded_tx,
        similar_songs_loaded_tx,
        library_populated_tx,
        track_updated_tx,
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
        similar_songs_loaded_rx,
        library_populated_rx,
        track_updated_rx,
        log_buffer,
    );

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Query the terminal for graphics protocol support (Kitty/iTerm2/Sixel).
    // This must happen after entering the alternate screen but before reading
    // terminal events. `from_query_stdio()` writes escape sequences to stdout
    // and reads the terminal response from stdin with an internal timeout.
    //
    // When `AlbumArtProtocol::Halfblock` is configured, skip the query entirely
    // and leave the picker unset — all three art sites use the existing
    // hand-rolled half-block rendering.
    let picker = match app.config.layout.album_art_protocol {
        config::AlbumArtProtocol::Halfblock => {
            tracing::info!("album_art_protocol = halfblock, skipping graphics protocol query");
            None
        }
        config::AlbumArtProtocol::Auto => {
            // Only enable protocol rendering when a real graphics protocol is
            // detected. If detection falls back to halfblocks, leave the picker
            // unset so callers use the existing hand-rolled half-block rendering.
            match Picker::from_query_stdio() {
                Ok(p) if p.protocol_type() != ProtocolType::Halfblocks => {
                    tracing::info!(
                        "detected terminal graphics protocol: {:?}",
                        p.protocol_type()
                    );
                    Some(p)
                }
                Ok(_) => {
                    tracing::info!(
                        "terminal supports only halfblocks, using existing half-block rendering"
                    );
                    None
                }
                Err(e) => {
                    tracing::info!(
                        "no terminal graphics protocol detected ({e}), using half-blocks"
                    );
                    None
                }
            }
        }
        config::AlbumArtProtocol::Image => {
            // Always use ratatui-image. If no real protocol is detected, fall
            // back to the halfblocks backend (higher fidelity than the quantized
            // 4×4 / 16-row grids).
            match Picker::from_query_stdio() {
                Ok(p) => {
                    tracing::info!(
                        "detected terminal graphics protocol: {:?}",
                        p.protocol_type()
                    );
                    Some(p)
                }
                Err(e) => {
                    tracing::info!(
                        "no terminal graphics protocol detected ({e}), using halfblocks backend"
                    );
                    Some(Picker::halfblocks())
                }
            }
        }
    };
    app.cover_art_cache
        .set_picker(picker.map(correct_picker_font_size));

    let result = run_app(
        &mut terminal,
        &mut app,
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

/// Corrects the picker's font size using the terminal's reported window size
/// when the capability query did not return a cell size.
/// `Picker::from_query_stdio()` silently falls back to a guessed 10×20 font in
/// that case; images are pre-resized to `cells × font size` pixels before
/// transmission, so a wrong guess makes Kitty and iTerm2 render album art over
/// the wrong number of cells (typically far too few on HiDPI displays).
fn correct_picker_font_size(picker: Picker) -> Picker {
    let cell_size_queried = picker
        .capabilities()
        .iter()
        .any(|c| matches!(c, Capability::CellSize(Some(_))));
    if cell_size_queried {
        return picker;
    }
    let Some((cell_width, cell_height)) = ui::layout::cell_pixel_size() else {
        tracing::warn!(
            "the terminal did not report a cell size; album art may render at the wrong scale \
             with the guessed font size {:?}",
            picker.font_size()
        );
        return picker;
    };
    tracing::info!(
        "the cell size query was not answered; correcting the picker font size from {:?} to \
         {cell_width}×{cell_height} using the reported window size",
        picker.font_size()
    );
    // `from_fontsize` is deprecated in favour of `from_query_stdio`, but the
    // query is exactly what failed to produce a cell size here, and it is the
    // only constructor that both accepts an explicit font size and performs
    // tmux detection.
    #[allow(deprecated)]
    let mut corrected = Picker::from_fontsize((cell_width, cell_height).into());
    corrected.set_protocol_type(picker.protocol_type());
    corrected
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
    #[cfg(feature = "media-controls")] media_controls: &mut Option<
        blackbird_client_shared::controls::Controls,
    >,
    #[cfg(feature = "tray-icon")] tray_menu: &mut blackbird_client_shared::tray::TrayMenu,
    #[cfg(feature = "tray-icon")] tray_icon: &blackbird_client_shared::tray::TrayIcon,
) -> anyhow::Result<()> {
    let mut last_tick = Instant::now();
    let mut last_full_redraw = Instant::now();

    /// Interval between full terminal redraws to repair damage from
    /// rogue library output (e.g. glib warnings written to stderr).
    const FULL_REDRAW_INTERVAL: Duration = Duration::from_secs(5);

    loop {
        if app.needs_redraw {
            // Periodically invalidate the diff buffer so the next draw
            // rewrites every cell, repairing any terminal corruption
            // caused by external library output (e.g. glib warnings).
            // Unlike `terminal.clear()`, this doesn't send a clear
            // escape sequence, so there's no visible flicker.
            //
            // When using the terminal's native background, swap every frame
            // to force full redraws. Without this, ratatui's diff skips cells
            // that haven't changed from the default state, causing artifacts
            // during rapid scrolling.
            if app.config.layout.use_terminal_background
                || last_full_redraw.elapsed() >= FULL_REDRAW_INTERVAL
            {
                terminal.swap_buffers();
                last_full_redraw = Instant::now();
            }
            app.cover_art_cache.begin_frame();
            app.begin_render();
            terminal.draw(|frame| ui::draw(frame, app))?;
        }
        let term_size = terminal.size()?;
        let size = Rect::new(0, 0, term_size.width, term_size.height);

        // Run at the shortest interval anything currently animating asked for,
        // falling back to the configured tick rate when nothing is animating.
        let frame_interval = app.frame_interval();
        let timeout = frame_interval.saturating_sub(last_tick.elapsed());
        let mut input_arrived = false;
        if event::poll(timeout)? {
            input_arrived = true;
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
                apply_scroll(app, scroll_delta, size);
            }
        }

        // Tick on input as well as on the interval. Input can start an
        // animation — a scroll fling begins on mouse release — and a tick is
        // where an animation states the frame interval it wants, so a slow
        // configured tick rate would otherwise delay the animation's first
        // frame by up to that interval. A burst of events is drained into a
        // single tick above, so this costs at most one tick per iteration.
        if input_arrived || last_tick.elapsed() >= frame_interval {
            app.tick();
            // Delete the terminal images for cover art evicted during the
            // tick, so a graphics-protocol terminal's image store stays
            // bounded to what is on screen. Written after the tick (and thus
            // after the most recent draw stopped placing that art), and
            // harmless if the terminal already dropped the image.
            if let Some(deletes) = app.cover_art_cache.take_pending_deletes() {
                let backend = terminal.backend_mut();
                let _ = backend.write_all(deletes.as_bytes());
                let _ = backend.flush();
            }
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
                && let Some(sa) = app.search.handle_key(&app.logic, action)
            {
                match sa {
                    ui::search::SearchAction::ToggleSearch => app.toggle_search(),
                    ui::search::SearchAction::GotoTrack(track_id) => {
                        app.logic.set_scroll_target(&track_id);
                        app.library.scroll_to_track = Some(track_id);
                        app.toggle_search();
                    }
                }
            }
        }
        FocusedPanel::Lyrics => {
            if let Some(action) = keys::lyrics_action(key) {
                if action == keys::Action::ToggleSidebar {
                    app.toggle_sidebar();
                } else if let Some(sa) = ui::sidebar::handle_key(app, action) {
                    match sa {
                        ui::sidebar::SidebarKeyAction::TogglePanel => app.toggle_lyrics(),
                        ui::sidebar::SidebarKeyAction::Quit => app.should_quit = true,
                        ui::sidebar::SidebarKeyAction::SeekRelative(secs) => {
                            app.seek_relative(secs)
                        }
                    }
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
        FocusedPanel::Settings => {
            if let Some(action) = keys::settings_action(key, app.settings.editing) {
                apply_settings_action(app, action);
            }
        }
    }
}

/// Applies a settings action with its side effects. Shared by the keyboard
/// dispatch and the help bar click handler so both paths behave identically.
fn apply_settings_action(app: &mut App, action: keys::Action) {
    let (settings_action, server_changed) =
        ui::settings::handle_key(&mut app.settings, &mut app.config, action);
    if server_changed {
        app.config.save();
        app.logic.reload_library(
            app.config.server.base_url.clone(),
            app.config.server.username.clone(),
            app.config.server.password.clone(),
            app.config.server.transcode,
        );
    }
    // Sync the runtime sidebar order immediately so the next draw reflects
    // component additions/removals without waiting for the next tick.
    app.sidebar.update_from_config(&mut app.config);
    // Config changes are applied in-memory for live preview;
    // disk save is deferred to settings exit or app exit.
    if let Some(sa) = settings_action {
        match sa {
            ui::settings::SettingsAction::ToggleSettings => {
                app.config.save();
                app.toggle_settings();
            }
        }
    }
}

/// Handles mouse events using the unified screen layout computed by
/// `ui::layout::layout_for` — the same rects `ui::draw` uses, so rendering
/// and hit-testing always agree.
fn handle_mouse_event(app: &mut App, mouse: &MouseEvent, size: Rect) {
    app.terminal_size = size;
    app.dropdown_rect = ui::now_playing::playback_mode_dropdown_rect(size);
    let layout = ui::layout::layout_for(app, size);

    let now_playing_area = layout.main.now_playing;
    let scrub_area = layout.main.scrub_bar;
    let help_bar_area = layout.main.help_bar;

    let x = mouse.column;
    let y = mouse.row;

    // Check whether the cursor is over the inline lyrics panel so we can
    // block interactions that would otherwise reach the library above.
    let over_inline_lyrics = layout
        .inline_lyrics
        .is_some_and(|r| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height);

    // Check whether the cursor is over the lyrics sidebar.
    let over_lyrics_sidebar = layout
        .lyrics_sidebar
        .is_some_and(|r| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height);

    // Check whether the cursor is over the lyrics sidebar border (drag handle).
    // The border is 1 column wide, so we add tolerance toward the sidebar side
    // to make it easier to grab. Tolerance extends away from the main content
    // so it doesn't interfere with the library's scrollbar.
    let over_lyrics_border = layout.over_lyrics_border(x, y);

    match mouse.kind {
        MouseEventKind::Moved => {
            // The cursor position drives hover underlines in every panel. The
            // individual panels compute whether the cursor is over *their*
            // area, so keep the raw position; a cursor over the sidebar still
            // needs it for the sidebar's own hover underline.
            app.mouse_position = Some((x, y));
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
                app.scrub_dragging = true;
                return;
            }

            // --- Lyrics sidebar border (start drag-to-resize) ---
            if over_lyrics_border {
                app.lyrics_sidebar_dragging = true;
                return;
            }

            // --- Settings sidebar border (start drag-to-resize) ---
            if app.focused_panel == FocusedPanel::Settings
                && let Some(settings_rect) = layout.settings
            {
                // Anchor the 2-column grab band to the rendered settings
                // border so the drag handle matches the drawn border.
                let border_x = settings_rect.x + settings_rect.width - 1;
                if x >= border_x && x <= border_x + 1 {
                    app.settings_sidebar_dragging = true;
                    return;
                }
            }

            // --- Sidebar content (click to focus + dispatch to component) ---
            if over_lyrics_sidebar && let Some(sidebar_rect) = layout.lyrics_sidebar {
                // Focus the sidebar if not already focused, mirroring
                // toggle_lyrics for the sidebar case (no reset_view —
                // preserve scroll position).
                if app.focused_panel != FocusedPanel::Lyrics {
                    app.focus_lyrics_panel(false);
                }
                // If the click is on a component boundary, start a drag to
                // resize the two adjacent components' proportions.
                if let Some(boundary) = ui::sidebar::boundary_at_y(app, sidebar_rect, y) {
                    app.sidebar_component_drag = Some(boundary);
                } else {
                    ui::sidebar::handle_mouse_click(app, sidebar_rect, x, y);
                }
                return;
            }

            // --- Inline lyrics overlay (display-only; blocks clicks) ---
            if over_inline_lyrics {
                return;
            }

            // --- Library area ---
            if y >= layout.library.y
                && y < layout.library.y + layout.library.height
                && x >= layout.library.x
                && x < layout.library.x + layout.library.width
            {
                // When the sidebar is focused, clicking the library area
                // switches focus to the library and handles the click as a
                // library click (play track, star, etc.). When the full lyrics
                // panel is active (no sidebar), the lyrics are rendered in
                // this area, so clicks route to the lyrics handler.
                if app.focused_panel == FocusedPanel::Lyrics && layout.show_sidebar {
                    app.focused_panel = FocusedPanel::Library;
                    ui::library::handle_mouse_click(app, layout.library, x, y);
                } else if app.focused_panel == FocusedPanel::Library {
                    ui::library::handle_mouse_click(app, layout.library, x, y);
                } else if app.focused_panel == FocusedPanel::Search {
                    app.search.handle_mouse_click(layout.library, x, y);
                } else if app.focused_panel == FocusedPanel::Lyrics {
                    // Full-panel case (no sidebar visible): the components are
                    // rendered in this area, so clicks route through the
                    // sidebar dispatcher with the content rect.
                    ui::sidebar::handle_mouse_click(app, layout.library, x, y);
                } else if app.focused_panel == FocusedPanel::Queue {
                    ui::queue::handle_mouse_click(&mut app.queue, &app.logic, layout.library, x, y);
                }
                // FocusedPanel::Settings intentionally falls through: the
                // library preview is display-only, so clicks to the right of
                // the settings panel are a true no-op.
                return;
            }

            // --- Settings sidebar (x-scoped dispatch) ---
            if app.focused_panel == FocusedPanel::Settings
                && let Some(settings_rect) = layout.settings
                && x >= settings_rect.x
                && x < settings_rect.x + settings_rect.width
                && y >= settings_rect.y
                && y < settings_rect.y + settings_rect.height
            {
                let server_changed =
                    ui::settings::handle_mouse_click(&mut app.settings, &mut app.config, y);
                if server_changed {
                    app.config.save();
                    app.logic.reload_library(
                        app.config.server.base_url.clone(),
                        app.config.server.username.clone(),
                        app.config.server.password.clone(),
                        app.config.server.transcode,
                    );
                }
                return;
            }

            // --- Help bar area ---
            if y >= help_bar_area.y && y < help_bar_area.y + help_bar_area.height {
                handle_help_bar_click(app, x);
            }
        }
        MouseEventKind::Up(MouseButton::Left) => {
            // Commit the scrub bar position on release with an immediate
            // (non-debounced) seek so it always takes effect.
            if app.scrub_dragging
                && let Some(preview) = app.scrub_preview_ratio
                && let Some(details) = app.logic.get_track_display_details()
            {
                let seek_pos = std::time::Duration::from_secs_f32(
                    details.track_duration.as_secs_f32() * preview,
                );
                app.logic.seek_current_immediate(seek_pos);
            }
            app.scrub_dragging = false;
            app.scrub_preview_ratio = None;
            // Save config if the sidebar was being resized.
            if app.lyrics_sidebar_dragging {
                app.lyrics_sidebar_dragging = false;
                app.config.save();
            }
            // Save config if a sidebar component boundary was being dragged.
            if app.sidebar_component_drag.take().is_some() {
                app.config.save();
            }
            // Save config if the settings sidebar border was being dragged.
            if app.settings_sidebar_dragging {
                app.settings_sidebar_dragging = false;
                app.config.save();
            }
            ui::library::handle_mouse_up(app);
            // Resolve a pending similar-songs click (sidebar or panel).
            if let Some(track_id) = app.similar_songs.handle_mouse_up() {
                app.logic.request_play_track(&track_id);
            }
            // Resolve a pending queue click (sidebar component).
            if let Some(track_id) = app.queue_sidebar.handle_mouse_up(&app.logic) {
                app.logic.request_play_track(&track_id);
            }
            if app.focused_panel == FocusedPanel::Search
                && let Some(sa) = app.search.handle_mouse_up(&app.logic)
            {
                match sa {
                    ui::search::SearchAction::ToggleSearch => app.toggle_search(),
                    ui::search::SearchAction::GotoTrack(track_id) => {
                        app.logic.set_scroll_target(&track_id);
                        app.library.scroll_to_track = Some(track_id);
                        app.toggle_search();
                    }
                }
            }
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            app.mouse_position = Some((x, y));

            // Continue scrub bar / volume drag regardless of Y position.
            if app.scrub_dragging {
                let clamped_x = x.clamp(scrub_area.x, scrub_area.x + scrub_area.width - 1);
                ui::handle_scrub_volume_click(app, scrub_area, clamped_x);
                return;
            }

            // Continue lyrics sidebar border drag.
            if app.lyrics_sidebar_dragging {
                if let Some(sidebar_rect) = layout.lyrics_sidebar {
                    let is_left = layout.sidebar_position
                        == blackbird_client_shared::config::SidebarPosition::Left;
                    // For a left sidebar, the outer edge is sidebar_rect.x.
                    // For a right sidebar, the outer edge is sidebar_rect.x + sidebar_rect.width - 1.
                    let sidebar_outer_x = if is_left {
                        sidebar_rect.x
                    } else {
                        sidebar_rect.x + sidebar_rect.width
                    };
                    let content_width = layout.main.content.width;
                    let max_width = (content_width / 2).max(ui::layout::LYRICS_SIDEBAR_MIN_WIDTH);
                    let new_width = ui::lyrics::compute_sidebar_width_from_drag(
                        x,
                        sidebar_outer_x,
                        is_left,
                        ui::layout::LYRICS_SIDEBAR_MIN_WIDTH,
                        max_width,
                    );
                    app.config.layout.sidebar_width = new_width;
                }
                return;
            }

            // Continue a sidebar component-boundary drag (adjust the heights
            // of the two adjacent components, and keep scrolling the
            // similar-songs component under the boundary).
            if let Some(boundary) = app.sidebar_component_drag {
                if let Some(sidebar_rect) = layout.lyrics_sidebar {
                    ui::sidebar::adjust_component_heights(app, sidebar_rect, boundary, y);
                    ui::sidebar::handle_boundary_drag(app, sidebar_rect, boundary, x, y);
                }
                return;
            }

            // Continue the settings sidebar border drag. The new width uses
            // the same clamp as the render (via `ui::layout::settings_width`),
            // so the border tracks the rendered border while dragging and the
            // 20-column library minimum is preserved.
            if app.settings_sidebar_dragging {
                if let Some(settings_rect) = layout.settings {
                    // The main-column width available to the settings sidebar
                    // (after the lyrics sidebar, before settings).
                    let main_column_width = settings_rect.width + layout.panel.width;
                    let width = ui::layout::settings_width(
                        main_column_width,
                        x.saturating_sub(settings_rect.x),
                    );
                    app.config.layout.settings_sidebar_width = width;
                }
                return;
            }

            if app.focused_panel == FocusedPanel::Library {
                ui::library::handle_mouse_drag(app, layout.library, x, y);
            } else if app.focused_panel == FocusedPanel::Search {
                app.search.handle_mouse_drag(layout.library, x, y);
            } else if app.focused_panel == FocusedPanel::Lyrics
                && app.sidebar.similar_songs_enabled()
            {
                // Route content drags in the sidebar (or full panel) to the
                // similar-songs component if it's under the cursor and enabled.
                // When the sidebar is visible but the similar-songs component
                // isn't in the order, there's nothing to drag. The lyrics
                // component handles its own drag-less scroll.
                let sidebar_area = layout.lyrics_sidebar.unwrap_or(layout.library);
                ui::sidebar::handle_mouse_drag(app, sidebar_area, x, y);
            }
        }
        // ScrollUp and ScrollDown are handled by the coalesced scroll path
        // in run_app (via apply_scroll), not here.
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
        Action::ToggleSidebar => app.toggle_sidebar(),
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
                app.library.mark_dirty();
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
        Action::CyclePlaybackMode(dir) => app.cycle_playback_mode(dir),
        Action::ToggleSortOrder(dir) => {
            let scroll_target = app.library.selected_track_id().cloned();
            let next = blackbird_client_shared::cycle(
                &bc::SortOrder::ALL,
                app.logic.get_sort_order(),
                dir,
            );
            app.logic.set_sort_order(next);
            app.library.mark_dirty();
            app.library.scroll_to_track = scroll_target;
        }
        Action::Settings => app.toggle_settings(),
        // In the settings panel, the help bar entries are the same actions the
        // keyboard dispatches; run them through the shared handler so clicks
        // and keys behave identically. `Char` is used for the component-list
        // "add" binding ('a').
        Action::Select
        | Action::Back
        | Action::MoveLeft
        | Action::MoveRight
        | Action::ResetField
        | Action::ResetSection
        | Action::DeleteChar
        | Action::Char('a')
            if app.focused_panel == FocusedPanel::Settings =>
        {
            apply_settings_action(app, action);
        }
        Action::Select if app.focused_panel == FocusedPanel::Library => {
            ui::library::handle_key(app, Action::Select);
        }
        Action::Back if app.focused_panel != FocusedPanel::Library => {
            app.focused_panel = FocusedPanel::Library;
        }
        _ => {}
    }
}

/// Applies a coalesced scroll delta. If the mouse cursor is over a scrollable
/// region (library, sidebar component, search, queue, logs, settings), scrolls
/// that region; otherwise falls back to the currently focused panel.
fn apply_scroll(app: &mut App, scroll_delta: i32, size: Rect) {
    app.terminal_size = size;
    app.dropdown_rect = ui::now_playing::playback_mode_dropdown_rect(size);

    // The dropdown owns its region: a wheel event over it is a no-op rather
    // than falling back to the focused panel.
    if app.playback_mode_dropdown
        && app
            .mouse_position
            .is_some_and(|pos| app.dropdown_rect.contains(Position::new(pos.0, pos.1)))
    {
        return;
    }

    let steps = scroll_delta.unsigned_abs() as usize * ui::layout::SCROLL_WHEEL_STEPS;
    let direction = scroll_delta.signum();

    let layout = ui::layout::layout_for(app, size);
    let sidebar_visible = layout.show_sidebar;

    let region = ui::panel::scroll_region_at(
        app.panel_mouse_position(),
        layout.library,
        layout.lyrics_sidebar,
        app,
    );

    // Cursor over the sidebar: scroll the component under the cursor, not the
    // keyboard-focused component.
    if let ui::panel::ScrollRegion::Sidebar { component } = region {
        app.sidebar.focused_component = component;
        ui::sidebar::handle_scroll(app, direction, steps);
        return;
    }

    // Cursor over the library region: scroll it only when the library is
    // actually rendered there — Library focused, or Lyrics-with-sidebar (which
    // renders the library). When Search/Queue/Logs/Settings is focused, the
    // cursor is over *their* content, so fall through and scroll that panel
    // instead of the hidden library. The Lyrics full panel (no sidebar) renders
    // the components, so scroll the focused component.
    if region == ui::panel::ScrollRegion::Library {
        if app.focused_panel == FocusedPanel::Lyrics {
            if sidebar_visible {
                ui::library::handle_scroll(app, direction, steps);
            } else {
                ui::sidebar::handle_scroll(app, direction, steps);
            }
            return;
        }
        if app.focused_panel == FocusedPanel::Library {
            ui::library::handle_scroll(app, direction, steps);
            return;
        }
    }

    // Otherwise (cursor not over a region, or over the now-playing/scrub/help
    // bars) fall back to the focused panel.
    match app.focused_panel {
        FocusedPanel::Library => {
            ui::library::handle_scroll(app, direction, steps);
        }
        FocusedPanel::Lyrics if sidebar_visible || !app.sidebar.is_empty() => {
            // Sidebar focused (or full panel with components): scroll the
            // focused component via the sidebar dispatcher.
            ui::sidebar::handle_scroll(app, direction, steps);
        }
        FocusedPanel::Lyrics => {
            // Empty component list — nothing to scroll.
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
        FocusedPanel::Search => {
            app.search.handle_scroll(direction, steps);
        }
        FocusedPanel::Settings => {
            ui::settings::scroll_selection(&mut app.settings, direction * steps as i32);
        }
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
