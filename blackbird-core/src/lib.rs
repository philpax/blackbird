use std::sync::{Arc, OnceLock};

pub mod state;
pub mod util;

mod config;
pub use config::Config;

mod logic;
pub use logic::{Logic, PlayingInfo, VisibleAlbumSet};

use blackbird_subsonic as bs;

pub trait Repainter: std::fmt::Debug {
    fn repaint(&self);
}
pub type SharedRepainter = Arc<OnceLock<Box<dyn Repainter + Send + Sync>>>;
