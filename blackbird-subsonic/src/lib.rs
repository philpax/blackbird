//! A barebones client for the Subsonic API.
#![deny(missing_docs)]

mod client;
pub use client::*;

mod album;
pub use album::*;

mod artist;
pub use artist::*;

mod song;
pub use song::*;

mod search;
#[allow(unused_imports)]
pub use search::*;

mod misc;

mod lyrics;
pub use lyrics::*;

mod request;
