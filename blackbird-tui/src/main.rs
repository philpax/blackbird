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
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, MouseButton, MouseEvent,
        MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
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

fn handle_mouse_event(app: &mut App, mouse: &MouseEvent, size: Rect) {
    // Compute layout areas matching ui::draw
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // now playing + controls (2 lines, no margin)
            Constraint::Length(1), // scrub bar + volume
            Constraint::Min(3),    // library / search / lyrics
            Constraint::Length(1), // help bar
        ])
        .split(size);

    let now_playing_area = main_chunks[0];
    let scrub_area = main_chunks[1];
    let library_area = main_chunks[2];

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
                handle_now_playing_click(app, now_playing_area, x, y);
                return;
            }

            // --- Scrub bar / Volume area ---
            if y == scrub_area.y && x >= scrub_area.x && x < scrub_area.x + scrub_area.width {
                handle_scrub_volume_click(app, scrub_area, x);
                return;
            }

            // --- Library area ---
            if y >= library_area.y
                && y < library_area.y + library_area.height
                && x >= library_area.x
                && x < library_area.x + library_area.width
                && app.focused_panel == FocusedPanel::Library
            {
                handle_library_click(app, library_area, x, y);
            }
        }
        MouseEventKind::Up(MouseButton::Left) => {
            // If we had a pending click (mouse down without drag), select and play the track.
            if let Some((_cx, _cy, index)) = app.library_click_pending.take()
                && !app.library_dragging
                && let Some(LibraryEntry::Track { id, .. }) = app.get_library_entry(index)
            {
                app.library_selected_index = index;
                app.logic.request_play_track(&id);
            }
            app.library_dragging = false;
            app.library_drag_last_y = None;
            app.scrollbar_dragging = false;
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            app.mouse_position = Some((x, y));

            // Scrollbar drag in library — once started, continues regardless of x position
            if app.focused_panel == FocusedPanel::Library {
                if app.scrollbar_dragging
                    && y >= library_area.y
                    && y < library_area.y + library_area.height
                {
                    let entries = app.get_flat_library().to_vec();
                    scroll_library_to_y(app, &entries, library_area, y);
                    app.library_click_pending = None;
                    app.library_dragging = true;
                    return;
                }

                let scrollbar_x = library_area.x + library_area.width - 1;

                if x == scrollbar_x
                    && y >= library_area.y
                    && y < library_area.y + library_area.height
                {
                    let entries = app.get_flat_library().to_vec();
                    scroll_library_to_y(app, &entries, library_area, y);
                    // Cancel any pending click since we're dragging
                    app.library_click_pending = None;
                    app.library_dragging = true;
                    app.scrollbar_dragging = true;
                    return;
                }

                // Content drag → pan library
                if app.library_click_pending.is_some() || app.library_dragging {
                    app.library_click_pending = None; // Cancel pending play
                    app.library_dragging = true;

                    if let Some(last_y) = app.library_drag_last_y {
                        let delta = y as i32 - last_y as i32;
                        if delta != 0 {
                            // Negative delta (drag up) → scroll down, positive → scroll up
                            let entries_len = app.flat_library_len();
                            let steps = delta.unsigned_abs() as usize;
                            for _ in 0..steps {
                                let mut new_index = app.library_selected_index;
                                if delta > 0 {
                                    // Dragged down → scroll up (show earlier content)
                                    while new_index > 0 {
                                        new_index -= 1;
                                        if let Some(LibraryEntry::Track { .. }) =
                                            app.get_library_entry(new_index)
                                        {
                                            break;
                                        }
                                    }
                                } else {
                                    // Dragged up → scroll down (show later content)
                                    while new_index < entries_len.saturating_sub(1) {
                                        new_index += 1;
                                        if let Some(LibraryEntry::Track { .. }) =
                                            app.get_library_entry(new_index)
                                        {
                                            break;
                                        }
                                    }
                                }
                                if let Some(LibraryEntry::Track { .. }) =
                                    app.get_library_entry(new_index)
                                {
                                    app.library_selected_index = new_index;
                                }
                            }
                        }
                    }
                    app.library_drag_last_y = Some(y);
                    return;
                }
            }

            // Scrub bar drag → seek
            if y == scrub_area.y && x >= scrub_area.x && x < scrub_area.x + scrub_area.width {
                handle_scrub_volume_click(app, scrub_area, x);
            }
        }
        MouseEventKind::ScrollUp => {
            if app.focused_panel == FocusedPanel::Library {
                // Scroll up in library (move selection up by 6 — 2x sensitivity)
                for _ in 0..6 {
                    let mut new_index = app.library_selected_index;
                    while new_index > 0 {
                        new_index -= 1;
                        if let Some(LibraryEntry::Track { .. }) = app.get_library_entry(new_index) {
                            break;
                        }
                    }
                    if let Some(LibraryEntry::Track { .. }) = app.get_library_entry(new_index) {
                        app.library_selected_index = new_index;
                    }
                }
            } else if app.focused_panel == FocusedPanel::Lyrics {
                app.lyrics_scroll_offset = app.lyrics_scroll_offset.saturating_sub(6);
            } else if app.focused_panel == FocusedPanel::Logs {
                app.logs_scroll_offset = app.logs_scroll_offset.saturating_sub(6);
            }
        }
        MouseEventKind::ScrollDown => {
            if app.focused_panel == FocusedPanel::Library {
                // Scroll down in library (move selection down by 6 — 2x sensitivity)
                let entries_len = app.flat_library_len();
                for _ in 0..6 {
                    let mut new_index = app.library_selected_index;
                    while new_index < entries_len.saturating_sub(1) {
                        new_index += 1;
                        if let Some(LibraryEntry::Track { .. }) = app.get_library_entry(new_index) {
                            break;
                        }
                    }
                    if let Some(LibraryEntry::Track { .. }) = app.get_library_entry(new_index) {
                        app.library_selected_index = new_index;
                    }
                }
            } else if app.focused_panel == FocusedPanel::Lyrics {
                app.lyrics_scroll_offset += 6;
            } else if app.focused_panel == FocusedPanel::Logs {
                let log_len = app.log_buffer.len();
                if log_len > 0 {
                    app.logs_scroll_offset = (app.logs_scroll_offset + 6).min(log_len - 1);
                }
            }
        }
        _ => {}
    }
}

/// Handle click in the now-playing area (track info, album info, transport, playback mode).
fn handle_now_playing_click(app: &mut App, area: Rect, x: u16, y: u16) {
    // Recompute the now-playing horizontal layout.
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(6),  // album art
            Constraint::Min(20),    // track info
            Constraint::Length(24), // transport controls
        ])
        .split(area);

    let art_area = chunks[0];
    let info_area = chunks[1];
    let transport_area = chunks[2];

    let row = y.saturating_sub(area.y);

    // Click on album art → open overlay
    if x >= art_area.x
        && x < art_area.x + art_area.width
        && let Some(details) = app.logic.get_track_display_details()
        && let Some(cover_art_id) = details.cover_art_id
    {
        app.album_art_overlay = Some(crate::app::AlbumArtOverlay {
            cover_art_id,
            title: format!("{} \u{2013} {}", details.album_artist, details.album_name),
        });
        return;
    }

    // Click on track info area
    if x >= info_area.x && x < info_area.x + info_area.width {
        if x == info_area.x {
            // Click on heart column → toggle star
            if row == 0 {
                if let Some(track_id) = app.logic.get_playing_track_id() {
                    let starred = app
                        .logic
                        .get_state()
                        .read()
                        .unwrap()
                        .library
                        .track_map
                        .get(&track_id)
                        .map(|t| t.starred)
                        .unwrap_or(false);
                    app.logic.set_track_starred(&track_id, !starred);
                    app.mark_library_dirty();
                }
            } else if row == 1
                && let Some(details) = app.logic.get_track_display_details()
            {
                let starred = app
                    .logic
                    .get_state()
                    .read()
                    .unwrap()
                    .library
                    .albums
                    .get(&details.album_id)
                    .map(|a| a.starred)
                    .unwrap_or(false);
                app.logic.set_album_starred(&details.album_id, !starred);
                app.mark_library_dirty();
            }
        } else {
            // Click on text → navigate to playing track/album
            if row == 0 {
                if let Some(track_id) = app.logic.get_playing_track_id() {
                    app.scroll_to_track = Some(track_id);
                    app.focused_panel = FocusedPanel::Library;
                }
            } else if row == 1
                && let Some(details) = app.logic.get_track_display_details()
            {
                app.scroll_to_album(&details.album_id);
                app.focused_panel = FocusedPanel::Library;
            }
        }
        return;
    }

    // Click on transport area
    if x >= transport_area.x && x < transport_area.x + transport_area.width {
        if row == 0 {
            // Transport buttons row: "⏮  ▶  ⏹  ⏭" right-aligned in 24 chars
            // Total button text = 10 chars, so starts at offset 14 from transport_area.x
            let rel_x = x.saturating_sub(transport_area.x);
            let btn_start = transport_area.width.saturating_sub(10);
            if rel_x >= btn_start {
                let btn_x = rel_x - btn_start;
                match btn_x {
                    0 => app.logic.previous(),       // ⏮
                    3 => app.logic.toggle_current(), // ▶/⏸
                    6 => app.logic.stop_current(),   // ⏹
                    9 => app.logic.next(),           // ⏭
                    _ => {}
                }
            }
        } else if row == 1 {
            // Mode text row: "[mode]" right-aligned → cycle playback mode
            app.cycle_playback_mode();
        }
    }
}

/// Handle click on scrub bar or volume slider area.
fn handle_scrub_volume_click(app: &mut App, scrub_area: Rect, x: u16) {
    // Recompute the scrub bar layout matching ui::draw_scrub_bar.
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(20),
            Constraint::Length(ui::VOLUME_SLIDER_WIDTH),
        ])
        .split(scrub_area);

    let scrub_bar = chunks[0];
    let vol_area = chunks[1];

    if x >= vol_area.x && x < vol_area.x + vol_area.width {
        // Click on volume slider: "♪ ████░░░░ nnn%"
        // The slider bar starts at offset 2 ("♪ ") and ends 5 before the end (" nnn%")
        let bar_start = vol_area.x + 2;
        let bar_width = vol_area.width.saturating_sub(7);
        if bar_width > 1 && x >= bar_start && x < bar_start + bar_width {
            let ratio = (x - bar_start) as f32 / (bar_width - 1) as f32;
            app.logic.set_volume(ratio.clamp(0.0, 1.0));
        }
    } else if x >= scrub_bar.x && x < scrub_bar.x + scrub_bar.width {
        // Click on scrub bar → seek
        let ratio = (x - scrub_bar.x) as f32 / scrub_bar.width as f32;
        if let Some(details) = app.logic.get_track_display_details() {
            let seek_pos = Duration::from_secs_f32(details.track_duration.as_secs_f32() * ratio);
            app.logic.seek_current(seek_pos);
        }
    }
}

/// Handle click in the library area.
fn handle_library_click(app: &mut App, library_area: Rect, x: u16, y: u16) {
    let entries = app.get_flat_library().to_vec();
    let scrollbar_x = library_area.x + library_area.width - 1;

    // Click on scrollbar (rightmost column)
    if x == scrollbar_x {
        scroll_library_to_y(app, &entries, library_area, y);
        app.scrollbar_dragging = true;
        return;
    }

    // Calculate which entry was clicked
    let inner_y = y.saturating_sub(library_area.y);
    let scroll_offset = app.library_scroll_offset;

    let mut line = 0usize;
    let mut clicked_index = None;
    let mut click_line_in_entry = 0usize;

    for (i, entry) in entries.iter().enumerate().skip(scroll_offset) {
        let entry_height = match entry {
            LibraryEntry::GroupHeader { .. } => 2,
            LibraryEntry::Track { .. } => 1,
        };

        if inner_y as usize >= line && (inner_y as usize) < line + entry_height {
            clicked_index = Some(i);
            click_line_in_entry = inner_y as usize - line;
            break;
        }
        line += entry_height;
    }

    let Some(index) = clicked_index else {
        return;
    };
    let Some(entry) = entries.get(index).cloned() else {
        return;
    };

    // Check if clicking on the heart (last content character before scrollbar).
    // Content width = area.width - 1 (alphabet column) - 1 (scrollbar if present)
    // The heart is the last character of the content line.
    let total_lines: usize = entries
        .iter()
        .map(|e| match e {
            LibraryEntry::GroupHeader { .. } => 2,
            LibraryEntry::Track { .. } => 1,
        })
        .sum();
    let has_scrollbar = total_lines > library_area.height as usize;
    let list_width = library_area.width as usize - 1 - if has_scrollbar { 1 } else { 0 };
    // Heart is at column list_width - 2 from area.x (due to padding saturating_sub(1))
    let heart_col = library_area.x as usize + list_width.saturating_sub(2);
    let is_heart_click = x as usize >= heart_col && x as usize <= heart_col + 1;

    match &entry {
        LibraryEntry::Track { id, starred, .. } => {
            if is_heart_click {
                // Toggle star
                app.logic.set_track_starred(id, !starred);
                app.mark_library_dirty();
            } else {
                // Set up deferred play (confirmed on MouseUp if no drag).
                // Don't set library_selected_index here — that would jump
                // the view to the click position, breaking touch-style drag.
                app.library_click_pending = Some((x, y, index));
                app.library_dragging = false;
                app.library_drag_last_y = Some(y);
            }
        }
        LibraryEntry::GroupHeader {
            artist,
            album,
            album_id,
            starred,
            cover_art_id,
            ..
        } => {
            // Album art occupies first 5 columns (1 margin + 4 art blocks)
            let art_end_col = library_area.x + 5;
            if x < art_end_col {
                // Click on album art → open overlay
                if let Some(id) = cover_art_id {
                    app.album_art_overlay = Some(crate::app::AlbumArtOverlay {
                        cover_art_id: id.clone(),
                        title: format!("{artist} \u{2013} {album}"),
                    });
                }
            } else if is_heart_click && click_line_in_entry == 1 {
                // Heart is on line 2 of group header
                app.logic.set_album_starred(album_id, !starred);
                app.mark_library_dirty();
            } else {
                // Allow drag initiation from group headers too
                app.library_click_pending = Some((x, y, index));
                app.library_dragging = false;
                app.library_drag_last_y = Some(y);
            }
        }
    }
}

/// Scroll library to a position based on Y coordinate (for scrollbar dragging).
fn scroll_library_to_y(app: &mut App, entries: &[LibraryEntry], library_area: Rect, y: u16) {
    let visible_height = library_area.height as usize;
    let inner_y = y.saturating_sub(library_area.y);
    let ratio = inner_y as f32 / visible_height as f32;

    let total_lines: usize = entries
        .iter()
        .map(|e| match e {
            LibraryEntry::GroupHeader { .. } => 2,
            LibraryEntry::Track { .. } => 1,
        })
        .sum();

    let target_line = ((total_lines as f32) * ratio) as usize;

    let mut current_line = 0usize;
    for (i, entry) in entries.iter().enumerate() {
        let entry_height = match entry {
            LibraryEntry::GroupHeader { .. } => 2,
            LibraryEntry::Track { .. } => 1,
        };

        if current_line + entry_height > target_line {
            let mut track_index = i;
            while track_index < entries.len() {
                if let LibraryEntry::Track { .. } = &entries[track_index] {
                    break;
                }
                track_index += 1;
            }
            if track_index < entries.len() {
                app.library_selected_index = track_index;
            }
            return;
        }
        current_line += entry_height;
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
                        app.mark_library_dirty();
                    }
                    LibraryEntry::GroupHeader {
                        album_id, starred, ..
                    } => {
                        app.logic.set_album_starred(&album_id, !starred);
                        app.mark_library_dirty();
                    }
                }
            }
        }
        Action::MoveUp => {
            // Skip album headers, only select tracks
            let mut new_index = app.library_selected_index;
            while new_index > 0 {
                new_index -= 1;
                if let Some(LibraryEntry::Track { .. }) = app.get_library_entry(new_index) {
                    break;
                }
            }
            // If we ended up on a header and there's no track above, stay put
            if let Some(LibraryEntry::Track { .. }) = app.get_library_entry(new_index) {
                app.library_selected_index = new_index;
            }
        }
        Action::MoveDown => {
            // Skip album headers, only select tracks
            let mut new_index = app.library_selected_index;
            while new_index < entries_len.saturating_sub(1) {
                new_index += 1;
                if let Some(LibraryEntry::Track { .. }) = app.get_library_entry(new_index) {
                    break;
                }
            }
            // If we ended up on a header at the end, stay put
            if let Some(LibraryEntry::Track { .. }) = app.get_library_entry(new_index) {
                app.library_selected_index = new_index;
            }
        }
        Action::PageUp => {
            let target = app.library_selected_index.saturating_sub(20);
            // Find nearest track at or after target
            let mut new_index = target;
            while new_index < entries_len {
                if let Some(LibraryEntry::Track { .. }) = app.get_library_entry(new_index) {
                    break;
                }
                new_index += 1;
            }
            if new_index < entries_len {
                app.library_selected_index = new_index;
            }
        }
        Action::PageDown => {
            if entries_len > 0 {
                let target = (app.library_selected_index + 20).min(entries_len - 1);
                // Find nearest track at or before target
                let mut new_index = target;
                loop {
                    if let Some(LibraryEntry::Track { .. }) = app.get_library_entry(new_index) {
                        break;
                    }
                    if new_index == 0 {
                        break;
                    }
                    new_index -= 1;
                }
                if let Some(LibraryEntry::Track { .. }) = app.get_library_entry(new_index) {
                    app.library_selected_index = new_index;
                }
            }
        }
        Action::Home => {
            // Find first track
            for i in 0..entries_len {
                if let Some(LibraryEntry::Track { .. }) = app.get_library_entry(i) {
                    app.library_selected_index = i;
                    break;
                }
            }
        }
        Action::End => {
            // Find last track
            if entries_len > 0 {
                for i in (0..entries_len).rev() {
                    if let Some(LibraryEntry::Track { .. }) = app.get_library_entry(i) {
                        app.library_selected_index = i;
                        break;
                    }
                }
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

        Some(hwnd.0 as *mut std::ffi::c_void)
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
