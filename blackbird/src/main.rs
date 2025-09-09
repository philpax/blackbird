use std::sync::{Arc, RwLock};

mod config;
mod controls;
mod ui;

use blackbird_core as bc;

fn main() {
    let subscriber = tracing_subscriber::FmtSubscriber::new();
    tracing::subscriber::set_global_default(subscriber).unwrap();

    // Load and save config at startup
    let config = config::Config::load();
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
        Box::new(move |cc| Ok(Box::new(ui::Ui::new(cc, config.clone(), logic)))),
    )
    .unwrap();
}
