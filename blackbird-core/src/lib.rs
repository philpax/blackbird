pub mod state;
pub mod util;

mod logic;
pub use logic::{Logic, PlayingInfo, VisibleAlbumSet};

use blackbird_subsonic as bs;
