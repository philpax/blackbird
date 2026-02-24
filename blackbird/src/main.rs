use std::sync::{Arc, RwLock};

mod config;
mod controls;
mod cover_art_cache;
mod ui;

use blackbird_core as bc;

use config::Config;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::HotKey};
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _};

fn main() {
    // Initialize platform-specific tray icon requirements (GTK on Linux).
    #[cfg(feature = "tray-icon")]
    blackbird_client_shared::tray::init_platform();

    // Log to a file so that shutdown diagnostics are visible even when the
    // GUI window has closed.
    let file_layer = std::fs::File::create("blackbird-gui.log")
        .map(|f| {
            tracing_subscriber::fmt::layer()
                .with_writer(std::sync::Mutex::new(f))
                .with_ansi(false)
        })
        .ok();

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(file_layer)
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("blackbird=info")),
        )
        .init();

    let icon = blackbird_client_shared::load_icon();

    // Load and save config at startup
    let config = Config::load();
    config.save();

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
        last_playback: config.shared.last_playback.track_id.clone().map(|id| {
            (
                id,
                std::time::Duration::from_secs_f64(config.shared.last_playback.track_position_secs),
            )
        }),
        cover_art_loaded_tx,
        lyrics_loaded_tx,
        library_populated_tx,
    });

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_position([
                config.general.window_position_x as f32,
                config.general.window_position_y as f32,
            ])
            .with_inner_size([
                config.general.window_width as f32,
                config.general.window_height as f32,
            ])
            .with_icon(egui::IconData {
                rgba: icon.as_raw().clone(),
                width: icon.width(),
                height: icon.height(),
            }),
        ..eframe::NativeOptions::default()
    };

    let config = Arc::new(RwLock::new(config));

    eframe::run_native(
        "blackbird",
        native_options,
        Box::new(move |cc| {
            Ok(Box::new(App::new(
                cc,
                config.clone(),
                logic,
                cover_art_loaded_rx,
                lyrics_loaded_rx,
                library_populated_rx,
                icon,
            )))
        }),
    )
    .unwrap();
}

pub struct App {
    // Logic must be declared (and thus dropped) before the background threads
    // so that the playback thread receives its Shutdown message promptly.
    logic: bc::Logic,

    // TrayIcon/TrayMenu/Controls destructors do synchronous D-Bus/GLib calls
    // that block for tens of seconds when the GLib main context isn't being
    // actively iterated. Wrap in ManuallyDrop to skip their destructors — the
    // process exit handles cleanup.
    #[cfg(feature = "tray-icon")]
    tray_menu: std::mem::ManuallyDrop<blackbird_client_shared::tray::TrayMenu>,
    #[cfg(feature = "tray-icon")]
    tray_icon: std::mem::ManuallyDrop<blackbird_client_shared::tray::TrayIcon>,
    #[cfg(feature = "media-controls")]
    controls: std::mem::ManuallyDrop<controls::Controls>,

    config: Arc<RwLock<Config>>,
    _config_reload_thread: std::thread::JoinHandle<()>,
    _repaint_thread: std::thread::JoinHandle<()>,
    playback_to_logic_rx: bc::PlaybackToLogicRx,
    cover_art_cache: cover_art_cache::CoverArtCache,
    lyrics_loaded_rx: std::sync::mpsc::Receiver<bc::LyricsData>,
    library_populated_rx: std::sync::mpsc::Receiver<()>,
    current_window_position: Option<(i32, i32)>,
    current_window_size: Option<(u32, u32)>,
    pub(crate) ui_state: ui::UiState,
    shutdown_initiated: bool,
    _global_hotkey_manager: GlobalHotKeyManager,
    search_hotkey: HotKey,
    mini_library_hotkey: HotKey,
}
impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        config: Arc<RwLock<Config>>,
        logic: bc::Logic,
        cover_art_loaded_rx: std::sync::mpsc::Receiver<bc::CoverArt>,
        lyrics_loaded_rx: std::sync::mpsc::Receiver<bc::LyricsData>,
        library_populated_rx: std::sync::mpsc::Receiver<()>,
        #[cfg_attr(not(feature = "tray-icon"), allow(unused_variables))] icon: image::RgbaImage,
    ) -> Self {
        let _config_reload_thread = std::thread::spawn({
            let config = config.clone();
            let egui_ctx = cc.egui_ctx.clone();
            move || loop {
                std::thread::sleep(std::time::Duration::from_secs(1));

                let new_config = Config::load();
                let current_config = config.read().unwrap();
                if new_config != *current_config {
                    drop(current_config);
                    *config.write().unwrap() = new_config;
                    config.read().unwrap().save();
                    egui_ctx.request_repaint();
                }
            }
        });

        let _repaint_thread = std::thread::spawn({
            let egui_ctx = cc.egui_ctx.clone();
            move || loop {
                std::thread::sleep(std::time::Duration::from_millis(500));
                egui_ctx.request_repaint();
            }
        });

        #[cfg(feature = "media-controls")]
        let controls = controls::Controls::new(
            {
                use raw_window_handle::{HasWindowHandle, RawWindowHandle};
                cc.window_handle().ok().and_then(|handle| {
                    if let RawWindowHandle::Win32(h) = handle.as_raw() {
                        Some(h.hwnd.get() as *mut std::ffi::c_void)
                    } else {
                        None
                    }
                })
            },
            logic.subscribe_to_playback_events(),
            logic.request_handle(),
            logic.get_state(),
        )
        .expect("Failed to initialize media controls");

        let cover_art_cache = cover_art_cache::CoverArtCache::new(
            cover_art_loaded_rx,
            Some(ui::GROUP_ALBUM_ART_SIZE as usize),
        );

        let ui_state = ui::initialize(cc, &config.read().unwrap());

        #[cfg(feature = "tray-icon")]
        let (tray_icon, tray_menu) = {
            let current_playback_mode = logic.get_playback_mode();
            blackbird_client_shared::tray::TrayMenu::new(icon, current_playback_mode)
        };

        let global_hotkey_manager =
            GlobalHotKeyManager::new().expect("Failed to create global hotkey manager");

        // Parse global search hotkey from config
        let (code, modifiers) = {
            let cfg = config.read().unwrap();
            cfg.keybindings
                .parse_global_hotkey(&cfg.keybindings.global_search)
                .expect("Failed to parse global search hotkey from config")
        };

        let search_hotkey = HotKey::new(Some(modifiers), code);
        global_hotkey_manager
            .register(search_hotkey)
            .expect("Failed to register global search hotkey");

        // Parse global mini-library hotkey from config
        let (code, modifiers) = {
            let cfg = config.read().unwrap();
            cfg.keybindings
                .parse_global_hotkey(&cfg.keybindings.global_mini_library)
                .expect("Failed to parse global mini-library hotkey from config")
        };

        let mini_library_hotkey = HotKey::new(Some(modifiers), code);
        global_hotkey_manager
            .register(mini_library_hotkey)
            .expect("Failed to register global mini-library hotkey");

        App {
            #[cfg(feature = "tray-icon")]
            tray_menu: std::mem::ManuallyDrop::new(tray_menu),
            #[cfg(feature = "tray-icon")]
            tray_icon: std::mem::ManuallyDrop::new(tray_icon),
            #[cfg(feature = "media-controls")]
            controls: std::mem::ManuallyDrop::new(controls),

            config,
            _config_reload_thread,
            _repaint_thread,
            playback_to_logic_rx: logic.subscribe_to_playback_events(),
            logic,
            cover_art_cache,
            lyrics_loaded_rx,
            library_populated_rx,
            current_window_position: None,
            current_window_size: None,
            ui_state,
            shutdown_initiated: false,
            _global_hotkey_manager: global_hotkey_manager,
            search_hotkey,
            mini_library_hotkey,
        }
    }
}
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Exit immediately if shutdown already initiated
        if self.shutdown_initiated {
            return;
        }

        #[cfg(feature = "tray-icon")]
        {
            if let Some(blackbird_client_shared::tray::TrayAction::FocusWindow) =
                self.tray_menu.handle_icon_events()
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }

            if let Some(action) = self.tray_menu.handle_menu_events(&self.logic) {
                match action {
                    blackbird_client_shared::tray::TrayAction::Quit => {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    blackbird_client_shared::tray::TrayAction::Repaint => {
                        ctx.request_repaint();
                    }
                    blackbird_client_shared::tray::TrayAction::FocusWindow => {}
                }
            }
        }

        // Handle global hotkey events
        while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if event.state == HotKeyState::Released {
                if event.id == self.search_hotkey.id() {
                    self.ui_state.search.open = !self.ui_state.search.open;
                    ctx.request_repaint();
                } else if event.id == self.mini_library_hotkey.id() {
                    if self.ui_state.mini_library.open {
                        self.ui_state.mini_library.open = false;
                    } else {
                        let playing_track = self.logic.get_playing_track_id();
                        self.ui_state
                            .mini_library
                            .open_with_playing_track(playing_track);
                    }
                    ctx.request_repaint();
                }
            }
        }

        // Check for shutdown signal from Tokio thread
        if self.logic.should_shutdown() {
            self.shutdown_initiated = true;
            tracing::info!("Shutdown requested, closing application");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        #[cfg(feature = "media-controls")]
        self.controls.update();
        self.logic.update();
        self.cover_art_cache.update(ctx);
        // Preload album art for tracks surrounding the next track in queue
        self.cover_art_cache
            .preload_next_track_surrounding_art(&self.logic);
        self.cover_art_cache.tick_prefetch(&self.logic);

        // Update tray menu
        #[cfg(feature = "tray-icon")]
        self.tray_menu.update(&self.logic, &self.tray_icon);

        // Update current window size
        ctx.input(|i| {
            if let Some(rect) = i.viewport().outer_rect {
                self.current_window_position = Some((rect.left() as i32, rect.top() as i32));
            }
            if let Some(rect) = i.viewport().inner_rect {
                self.current_window_size = Some((rect.width() as u32, rect.height() as u32));
            }
        });

        self.render(ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        let mut config = self.config.write().unwrap();
        if let Some((x, y)) = self.current_window_position {
            config.general.window_position_x = x;
            config.general.window_position_y = y;
        }
        if let Some((width, height)) = self.current_window_size {
            config.general.window_width = width;
            config.general.window_height = height;
        }
        config.general.volume = self.logic.get_volume();
        if let Some(track_and_position) = self.logic.get_playing_track_and_position() {
            config.shared.last_playback.track_id = Some(track_and_position.track_id);
            config.shared.last_playback.track_position_secs =
                track_and_position.position.as_secs_f64();
        }
        config.shared.last_playback.playback_mode = self.logic.get_playback_mode();
        config.shared.last_playback.sort_order = self.logic.get_sort_order();
        config.save();
    }
}
