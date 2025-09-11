use std::sync::{Arc, RwLock};

mod config;
mod controls;
mod ui;

use blackbird_core as bc;

use config::Config;
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _};

fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("blackbird=info")),
        )
        .init();

    // Load and save config at startup
    let config = Config::load();
    config.save();

    let logic = bc::Logic::new(
        config.server.base_url.clone(),
        config.server.username.clone(),
        config.server.password.clone(),
        config.server.transcode,
    );

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_position([
                config.general.window_position_x as f32,
                config.general.window_position_y as f32,
            ])
            .with_inner_size([
                config.general.window_width as f32,
                config.general.window_height as f32,
            ]),
        ..eframe::NativeOptions::default()
    };

    let config = Arc::new(RwLock::new(config));

    eframe::run_native(
        "blackbird",
        native_options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, config.clone(), logic)))),
    )
    .unwrap();
}

pub struct App {
    config: Arc<RwLock<Config>>,
    _config_reload_thread: std::thread::JoinHandle<()>,
    _repaint_thread: std::thread::JoinHandle<()>,
    controls: controls::Controls,
    logic: bc::Logic,
    current_window_position: Option<(i32, i32)>,
    current_window_size: Option<(u32, u32)>,
}
impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        config: Arc<RwLock<Config>>,
        logic: bc::Logic,
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

        ui::initialize(cc, &config.read().unwrap());

        App {
            config,
            _config_reload_thread,
            _repaint_thread,
            controls,
            logic,
            current_window_position: None,
            current_window_size: None,
        }
    }
}
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.controls.update();
        self.logic.update();

        // Update current window size
        ctx.input(|i| {
            if let Some(rect) = i.viewport().outer_rect {
                self.current_window_position = Some((rect.left() as i32, rect.top() as i32));
            }
            if let Some(rect) = i.viewport().inner_rect {
                self.current_window_size = Some((rect.width() as u32, rect.height() as u32));
            }
        });

        ui::render(ctx, &self.config.read().unwrap(), &mut self.logic);
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
        config.save();
    }
}
