//! A barebones client for the Subsonic API.
#![deny(missing_docs)]

mod client;
pub use client::*;

mod album;
pub use album::*;

mod song;
pub use song::*;

mod misc;

mod request;
