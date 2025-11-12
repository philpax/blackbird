use std::sync::{Arc, RwLock};

mod config;
mod controls;
mod cover_art_cache;
mod ui;

use blackbird_core as bc;

use config::Config;
use image::EncodableLayout;
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, CheckMenuItem};

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

    let logic = bc::Logic::new(bc::LogicArgs {
        base_url: config.server.base_url.clone(),
        username: config.server.username.clone(),
        password: config.server.password.clone(),
        transcode: config.server.transcode,
        volume: config.general.volume,
        cover_art_loaded_tx,
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
    controls: controls::Controls,
    logic: bc::Logic,
    cover_art_cache: cover_art_cache::CoverArtCache,
    current_window_position: Option<(i32, i32)>,
    current_window_size: Option<(u32, u32)>,
    ui_state: ui::UiState,
    shutdown_initiated: bool,
    tray_icon: tray_icon::TrayIcon,
    tray_menu: Menu,
    current_track_item: MenuItem,
    prev_item: MenuItem,
    next_item: MenuItem,
    sequential_item: CheckMenuItem,
    repeat_one_item: CheckMenuItem,
    group_repeat_item: CheckMenuItem,
    shuffle_item: CheckMenuItem,
    group_shuffle_item: CheckMenuItem,
    quit_item: MenuItem,
    last_track_display: Option<String>,
    last_playback_mode: bc::PlaybackMode,
}
impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        config: Arc<RwLock<Config>>,
        logic: bc::Logic,
        cover_art_loaded_rx: std::sync::mpsc::Receiver<bc::CoverArt>,
        icon: image::RgbaImage,
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

        let current_playback_mode = logic.get_playback_mode();
        let (
            tray_menu,
            current_track_item,
            prev_item,
            next_item,
            sequential_item,
            repeat_one_item,
            group_repeat_item,
            shuffle_item,
            group_shuffle_item,
            quit_item,
        ) = Self::build_tray_menu(current_playback_mode);

        let tray_icon = Self::build_tray_icon(icon, &tray_menu);

        App {
            config,
            _config_reload_thread,
            _repaint_thread,
            playback_to_logic_rx: logic.subscribe_to_playback_events(),
            controls,
            logic,
            cover_art_cache,
            current_window_position: None,
            current_window_size: None,
            ui_state,
            shutdown_initiated: false,
            tray_icon,
            tray_menu,
            current_track_item,
            prev_item,
            next_item,
            sequential_item,
            repeat_one_item,
            group_repeat_item,
            shuffle_item,
            group_shuffle_item,
            quit_item,
            last_track_display: None,
            last_playback_mode: current_playback_mode,
        }
    }

    fn build_tray_icon(icon: image::RgbaImage, menu: &Menu) -> tray_icon::TrayIcon {
        let (icon_width, icon_height) = icon.dimensions();
        tray_icon::TrayIconBuilder::new()
            .with_tooltip(Self::build_tooltip(None))
            .with_icon(
                tray_icon::Icon::from_rgba(icon.into_vec(), icon_width, icon_height).unwrap(),
            )
            .with_menu(Box::new(menu.clone()))
            .build()
            .unwrap()
    }

    fn build_tray_menu(
        current_playback_mode: bc::PlaybackMode,
    ) -> (
        Menu,
        MenuItem,
        MenuItem,
        MenuItem,
        CheckMenuItem,
        CheckMenuItem,
        CheckMenuItem,
        CheckMenuItem,
        CheckMenuItem,
        MenuItem,
    ) {
        let menu = Menu::new();

        // Current track (disabled, non-clickable)
        let current_track_item = MenuItem::new("Not playing", false, None);
        menu.append(&current_track_item).unwrap();

        // Separator
        menu.append(&PredefinedMenuItem::separator()).unwrap();

        // Previous
        let prev_item = MenuItem::new("Previous", true, None);
        menu.append(&prev_item).unwrap();

        // Next
        let next_item = MenuItem::new("Next", true, None);
        menu.append(&next_item).unwrap();

        // Separator
        menu.append(&PredefinedMenuItem::separator()).unwrap();

        // Playback modes
        let sequential_item = CheckMenuItem::new(
            "Sequential",
            true,
            current_playback_mode == bc::PlaybackMode::Sequential,
            None,
        );
        menu.append(&sequential_item).unwrap();

        let repeat_one_item = CheckMenuItem::new(
            "Repeat One",
            true,
            current_playback_mode == bc::PlaybackMode::RepeatOne,
            None,
        );
        menu.append(&repeat_one_item).unwrap();

        let group_repeat_item = CheckMenuItem::new(
            "Group Repeat",
            true,
            current_playback_mode == bc::PlaybackMode::GroupRepeat,
            None,
        );
        menu.append(&group_repeat_item).unwrap();

        let shuffle_item = CheckMenuItem::new(
            "Shuffle",
            true,
            current_playback_mode == bc::PlaybackMode::Shuffle,
            None,
        );
        menu.append(&shuffle_item).unwrap();

        let group_shuffle_item = CheckMenuItem::new(
            "Group Shuffle",
            true,
            current_playback_mode == bc::PlaybackMode::GroupShuffle,
            None,
        );
        menu.append(&group_shuffle_item).unwrap();

        // Separator
        menu.append(&PredefinedMenuItem::separator()).unwrap();

        // Quit
        let quit_item = MenuItem::new("Quit", true, None);
        menu.append(&quit_item).unwrap();

        (
            menu,
            current_track_item,
            prev_item,
            next_item,
            sequential_item,
            repeat_one_item,
            group_repeat_item,
            shuffle_item,
            group_shuffle_item,
            quit_item,
        )
    }

    fn build_tooltip(track_display_details: Option<&bc::TrackDisplayDetails>) -> String {
        if let Some(track_display_details) = track_display_details {
            format!("{track_display_details}")
        } else {
            "Not playing".to_string()
        }
    }
}
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Exit immediately if shutdown already initiated
        if self.shutdown_initiated {
            return;
        }

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
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == self.prev_item.id() {
                self.logic.previous();
            } else if event.id == self.next_item.id() {
                self.logic.next();
            } else if event.id == self.sequential_item.id() {
                self.logic.set_playback_mode(bc::PlaybackMode::Sequential);
            } else if event.id == self.repeat_one_item.id() {
                self.logic.set_playback_mode(bc::PlaybackMode::RepeatOne);
            } else if event.id == self.group_repeat_item.id() {
                self.logic.set_playback_mode(bc::PlaybackMode::GroupRepeat);
            } else if event.id == self.shuffle_item.id() {
                self.logic.set_playback_mode(bc::PlaybackMode::Shuffle);
            } else if event.id == self.group_shuffle_item.id() {
                self.logic.set_playback_mode(bc::PlaybackMode::GroupShuffle);
            } else if event.id == self.quit_item.id() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }

        // Check for shutdown signal from Tokio thread
        if self.logic.should_shutdown() {
            self.shutdown_initiated = true;
            tracing::info!("Shutdown requested, closing application");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        self.controls.update();
        self.logic.update();
        self.cover_art_cache.update(ctx);

        self.tray_icon
            .set_tooltip(Some(Self::build_tooltip(
                self.logic.get_track_display_details().as_ref(),
            )))
            .ok();

        // Update menu current track display
        let track_display = self
            .logic
            .get_track_display_details()
            .map(|details| format!("{}", details));
        if track_display != self.last_track_display {
            let text = track_display.clone().unwrap_or_else(|| "Not playing".to_string());
            self.current_track_item.set_text(text);
            self.last_track_display = track_display;
        }

        // Update menu playback mode checkmarks
        let current_mode = self.logic.get_playback_mode();
        if current_mode != self.last_playback_mode {
            self.sequential_item
                .set_checked(current_mode == bc::PlaybackMode::Sequential);
            self.repeat_one_item
                .set_checked(current_mode == bc::PlaybackMode::RepeatOne);
            self.group_repeat_item
                .set_checked(current_mode == bc::PlaybackMode::GroupRepeat);
            self.shuffle_item
                .set_checked(current_mode == bc::PlaybackMode::Shuffle);
            self.group_shuffle_item
                .set_checked(current_mode == bc::PlaybackMode::GroupShuffle);
            self.last_playback_mode = current_mode;
        }

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
