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
pub use search::*;

mod misc;

mod request;
