use std::sync::{Arc, RwLock};

mod config;
mod media_controls;
mod ui;

use blackbird_core as bc;

fn main() {
    let subscriber = tracing_subscriber::FmtSubscriber::new();
    tracing::subscriber::set_global_default(subscriber).unwrap();

    // Load config at startup
    let config = config::Config::load();

    // Save initial config
    config.save();

    // Create client with config values
    let base_url = config.server.base_url.clone();
    let username = config.server.username.clone();
    let password = config.server.password.clone();
    let transcode = config.server.transcode;
    let cache_size = config.general.track_cache_size;
    let default_shuffle = config.general.default_shuffle;
    let window_width = config.general.window_width;
    let window_height = config.general.window_height;

    // Now wrap config in Arc<RwLock> after using it for client creation
    let config = Arc::new(RwLock::new(config));
    let logic = Arc::new(bc::Logic::new(
        base_url,
        username,
        password,
        transcode,
        cache_size,
        default_shuffle,
    ));

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([window_width, window_height]),
        ..eframe::NativeOptions::default()
    };
    eframe::run_native(
        "blackbird",
        native_options,
        Box::new(move |cc| Ok(Box::new(ui::Ui::new(cc, config.clone(), logic.clone())))),
    )
    .unwrap();
}
