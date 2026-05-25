//! Platform-specific directory resolution for blackbird.
//!
//! Uses the `etcetera` crate with the native platform strategy: XDG on Linux,
//! Apple Standard Directories on macOS, and Known Folders on Windows.
use std::path::PathBuf;

use etcetera::AppStrategy as _;

fn app_strategy() -> impl etcetera::AppStrategy {
    etcetera::choose_app_strategy(etcetera::AppStrategyArgs {
        top_level_domain: "me".to_string(),
        author: "philpax".to_string(),
        app_name: "blackbird".to_string(),
    })
    .expect("failed to resolve home directory")
}

/// Returns the platform-specific configuration directory for the application.
///
/// - Linux: `$XDG_CONFIG_HOME/blackbird` (typically `~/.config/blackbird`)
/// - macOS: `~/Library/Application Support/me.philpax.blackbird`
/// - Windows: `{FOLDERID_RoamingAppData}/philpax/blackbird/config`
pub fn config_dir() -> PathBuf {
    app_strategy().config_dir()
}

/// Returns the platform-specific cache directory for the application.
///
/// - Linux: `$XDG_CACHE_HOME/blackbird` (typically `~/.cache/blackbird`)
/// - macOS: `~/Library/Caches/me.philpax.blackbird`
/// - Windows: `{FOLDERID_LocalAppData}/philpax/blackbird/cache`
pub fn cache_dir() -> PathBuf {
    app_strategy().cache_dir()
}

/// Returns the platform-specific data directory for the application (used for logs).
///
/// - Linux: `$XDG_DATA_HOME/blackbird` (typically `~/.local/share/blackbird`)
/// - macOS: `~/Library/Application Support/me.philpax.blackbird`
/// - Windows: `{FOLDERID_RoamingAppData}/philpax/blackbird/data`
pub fn data_dir() -> PathBuf {
    app_strategy().data_dir()
}
