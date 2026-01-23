//! Library views for displaying and navigating the music library.
//!
//! This module provides two library views:
//! - [`full`]: The main library view embedded in the main window
//! - [`mini`]: A standalone mini-library popup window (Cmd+Alt+Shift+L)
//!
//! Both views share common rendering logic via [`shared`].

mod alphabet_scroll;
pub mod full;
mod group;
mod incremental_search;
pub mod mini;
pub mod shared;
mod track;

pub use group::GROUP_ALBUM_ART_SIZE;
pub use mini::MiniLibraryState;
pub use shared::LibraryViewState;
