use std::sync::{Arc, RwLock};

mod config;
mod controls;
mod cover_art_cache;
#[cfg(feature = "tray-icon")]
mod tray;
mod ui;

use blackbird_core as bc;

use config::Config;
use image::EncodableLayout;
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _};

fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("blackbird=info")),
        )
        .init();

    let icon = image::load_from_memory(include_bytes!("../assets/icon.png"))
        .unwrap()
        .to_rgba8();

    // Load and save config at startup
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
                rgba: icon.as_bytes().into(),
                width: icon.width() as u32,
                height: icon.height() as u32,
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
    config: Arc<RwLock<Config>>,
    _config_reload_thread: std::thread::JoinHandle<()>,
    _repaint_thread: std::thread::JoinHandle<()>,
    playback_to_logic_rx: bc::PlaybackToLogicRx,
    #[cfg(feature = "media-controls")]
    controls: controls::Controls,
    logic: bc::Logic,
    cover_art_cache: cover_art_cache::CoverArtCache,
    lyrics_loaded_rx: std::sync::mpsc::Receiver<bc::LyricsData>,
    library_populated_rx: std::sync::mpsc::Receiver<()>,
    current_window_position: Option<(i32, i32)>,
    current_window_size: Option<(u32, u32)>,
    ui_state: ui::UiState,
    shutdown_initiated: bool,
    #[cfg(feature = "tray-icon")]
    tray_icon: tray_icon::TrayIcon,
    #[cfg(feature = "tray-icon")]
    tray_menu: tray::TrayMenu,
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
            Some(cc),
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
            tray::TrayMenu::new(icon, current_playback_mode)
        };

        App {
            config,
            _config_reload_thread,
            _repaint_thread,
            playback_to_logic_rx: logic.subscribe_to_playback_events(),
            #[cfg(feature = "media-controls")]
            controls,
            logic,
            cover_art_cache,
            lyrics_loaded_rx,
            library_populated_rx,
            current_window_position: None,
            current_window_size: None,
            ui_state,
            shutdown_initiated: false,
            #[cfg(feature = "tray-icon")]
            tray_icon,
            #[cfg(feature = "tray-icon")]
            tray_menu,
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
            while let Ok(event) = tray_icon::TrayIconEvent::receiver().try_recv() {
                if let tray_icon::TrayIconEvent::Click {
                    button: tray_icon::MouseButton::Left,
                    ..
                } = event
                {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
            }

            // Handle menu events
            self.tray_menu.handle_events(&self.logic, ctx);
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
        config.last_playback.track_id = self.logic.get_playing_track_id();
        config.last_playback.track_position_secs = self
            .logic
            .get_playing_position()
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        config.last_playback.playback_mode = self.logic.get_playback_mode();
        config.save();
    }
}
