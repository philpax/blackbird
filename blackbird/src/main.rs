use std::sync::{Arc, RwLock};

mod config;
mod controls;
mod ui;

use blackbird_core as bc;

use config::Config;

fn main() {
    let subscriber = tracing_subscriber::FmtSubscriber::new();
    tracing::subscriber::set_global_default(subscriber).unwrap();

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
            .with_inner_size([config.general.window_width, config.general.window_height]),
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
    current_window_size: Option<egui::Rect>,
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
        )
        .expect("Failed to initialize media controls");

        ui::initialize(cc, &config.read().unwrap());

        App {
            config,
            _config_reload_thread,
            _repaint_thread,
            controls,
            logic,
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
            if let Some(inner_rect) = i.viewport().inner_rect {
                self.current_window_size = Some(inner_rect);
            }
        });

        ui::render(ctx, &self.config.read().unwrap(), &mut self.logic);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        let Some(inner_rect) = self.current_window_size else {
            return;
        };
        let mut config = self.config.write().unwrap();
        config.general.window_width = inner_rect.width();
        config.general.window_height = inner_rect.height();
        config.save();
    }
}
