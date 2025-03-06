use blackbird_subsonic as bs;

mod style;
mod util;

mod config;

mod album;

mod song;

mod logic;
mod ui;

fn main() {
    let subscriber = tracing_subscriber::FmtSubscriber::new();
    tracing::subscriber::set_global_default(subscriber).unwrap();

    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "blackbird",
        native_options,
        Box::new(|cc| Ok(Box::new(ui::Ui::new(cc)))),
    )
    .unwrap();
}
