//! Windows-specific functionality for blackbird.

use anyhow::Context as _;
use windows::core::HSTRING;
use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;

/// Sets the AppUserModelID for the current process.
///
/// This allows Windows to properly identify the application in the taskbar,
/// media controls, and other system UI elements. Without this, Windows may
/// not display the application name correctly in media controls.
///
/// The AppUserModelID follows the format: CompanyName.ProductName
///
/// See: https://learn.microsoft.com/en-us/windows/win32/shell/appids
pub fn register_host_process() -> anyhow::Result<()> {
    const APP_ID: &str = "com.philpax.blackbird";

    tracing::info!("Setting AppUserModelID to: {APP_ID}");

    // Convert to HSTRING (Windows UTF-16 string)
    let app_id = HSTRING::from(APP_ID);

    // Set the AppUserModelID for this process
    // SAFETY: This is a safe Windows API call that sets process-level metadata
    unsafe {
        SetCurrentProcessExplicitAppUserModelID(&app_id)
            .context("Failed to set AppUserModelID")?;
    }

    tracing::info!("Successfully set AppUserModelID to: {APP_ID}");

    Ok(())
}
