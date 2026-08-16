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

#[cfg(feature = "opensubsonic")]
mod lyrics;
#[cfg(feature = "opensubsonic")]
pub use lyrics::*;

#[cfg(feature = "opensubsonic")]
mod extensions;
#[cfg(feature = "opensubsonic")]
pub use extensions::*;

#[cfg(feature = "opensubsonic")]
mod similar;
#[cfg(feature = "opensubsonic")]
#[allow(unused_imports)]
pub use similar::*;

mod request;
