use std::sync::{Arc, RwLock};

mod config;
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

    // Now wrap config in Arc<RwLock> after using it for client creation
    let config = Arc::new(RwLock::new(config));
    let logic = Arc::new(bc::Logic::new(base_url, username, password, transcode));

    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "blackbird",
        native_options,
        Box::new(move |cc| Ok(Box::new(ui::Ui::new(cc, config.clone(), logic.clone())))),
    )
    .unwrap();
}
