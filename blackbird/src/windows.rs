//! Windows-specific functionality for blackbird.

use std::io;

use anyhow::Context as _;
use winreg::{RegKey, enums::*};

/// Registers the application as a Windows host process.
///
/// This sets up the registry key `HKEY_CLASSES_ROOT\Applications\blackbird.exe\IsHostApp`
/// as a REG_NULL value, which allows Windows to properly display the application name
/// in media controls and other system UI elements.
///
/// See: https://learn.microsoft.com/en-us/windows/win32/shell/appids#registering-an-application-as-a-host-process
pub fn register_host_process() -> anyhow::Result<()> {
    // Get the current executable path
    let exe_path = std::env::current_exe()?;
    let exe_name = exe_path
        .file_name()
        .and_then(|n| n.to_str())
        .context("Failed to get executable name")?;

    tracing::info!("Registering {exe_name} as a Windows host process");

    // Open HKEY_CLASSES_ROOT\Applications
    let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
    let applications = hkcr
        .create_subkey("Applications")
        .context("Failed to open/create HKEY_CLASSES_ROOT\\Applications")?
        .0;

    // Create/open the key for our executable
    let app_key = applications
        .create_subkey(exe_name)
        .with_context(|| {
            format!("Failed to create/open HKEY_CLASSES_ROOT\\Applications\\{exe_name}")
        })?
        .0;

    // Set IsHostApp as a REG_NONE (null) value
    // According to Microsoft docs, the presence of this key (regardless of value) is what matters
    app_key
        .set_raw_value(
            "IsHostApp",
            &winreg::RegValue {
                bytes: vec![],
                vtype: REG_NONE,
            },
        )
        .map_err(|e| match e.kind() {
            io::ErrorKind::PermissionDenied => {
                anyhow::anyhow!(
                    "Permission denied when setting IsHostApp registry value. \
                     Try running the application as administrator on first launch."
                )
            }
            _ => anyhow::anyhow!("Failed to set IsHostApp registry value: {e}"),
        })?;

    tracing::info!(
        "Successfully registered {exe_name} as a host process at HKEY_CLASSES_ROOT\\Applications\\{exe_name}\\IsHostApp",
    );

    Ok(())
}
