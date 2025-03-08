use blackbird_subsonic as bs;
use std::sync::{Arc, OnceLock, RwLock};

mod config;
mod logic;
mod state;
mod ui;
mod util;

trait Repainter: std::fmt::Debug {
    fn repaint(&self);
}
impl Repainter for egui::Context {
    fn repaint(&self) {
        self.request_repaint();
    }
}
type SharedRepainter = Arc<OnceLock<Box<dyn Repainter + Send + Sync>>>;

fn main() {
    let subscriber = tracing_subscriber::FmtSubscriber::new();
    tracing::subscriber::set_global_default(subscriber).unwrap();

    // Load config at startup
    let config = config::Config::load();

    // Save initial config
    config.save();

    // Create client with config values
    let client = bs::Client::new(
        config.general.base_url.clone(),
        config.general.username.clone(),
        config.general.password.clone(),
        "blackbird".to_string(),
    );

    // Create a repainter handle that will get populated by the UI
    let repainter = SharedRepainter::new(Default::default());

    // Now wrap config in Arc<RwLock> after using it for client creation
    let config = Arc::new(RwLock::new(config));
    let logic = Arc::new(logic::Logic::new(client, config.clone(), repainter.clone()));

    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "blackbird",
        native_options,
        Box::new(move |cc| {
            Ok(Box::new(ui::Ui::new(
                cc,
                config.clone(),
                logic.clone(),
                repainter.clone(),
            )))
        }),
    )
    .unwrap();
}
