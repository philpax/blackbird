//! Windows-specific functionality for blackbird.

use anyhow::Context as _;
use raw_window_handle::{RawWindowHandle, WindowHandle};
use windows::{
    Win32::{
        Foundation::{HWND, PROPERTYKEY},
        Storage::EnhancedStorage::{
            PKEY_AppUserModel_ID, PKEY_AppUserModel_RelaunchCommand,
            PKEY_AppUserModel_RelaunchDisplayNameResource,
        },
        System::{
            Com::{
                COINIT_APARTMENTTHREADED, CoInitializeEx,
                StructuredStorage::{PROPVARIANT, PropVariantClear},
            },
            Variant::VT_LPWSTR,
        },
        UI::Shell::{
            PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow},
            SetCurrentProcessExplicitAppUserModelID,
        },
    },
    core::{HSTRING, PWSTR},
};

/// Application User Model ID for blackbird
pub const APP_ID: &str = "com.philpax.blackbird";

/// Display name shown in Windows UI
pub const DISPLAY_NAME: &str = "Blackbird";

/// Sets the AppUserModelID for the current process.
///
/// This allows Windows to properly identify the application in the taskbar,
/// media controls, and other system UI elements. Without this, Windows may
/// not display the application name correctly in media controls.
///
/// The AppUserModelID follows the format: CompanyName.ProductName
///
/// See: https://learn.microsoft.com/en-us/windows/win32/shell/appids
pub fn set_app_user_model_id() -> anyhow::Result<()> {
    tracing::info!("Setting AppUserModelID to: {APP_ID}");

    // Convert to HSTRING (Windows UTF-16 string)
    let app_id = HSTRING::from(APP_ID);

    // Set the AppUserModelID for this process
    // SAFETY: This is a safe Windows API call that sets process-level metadata
    unsafe {
        SetCurrentProcessExplicitAppUserModelID(&app_id).context("Failed to set AppUserModelID")?;
    }

    tracing::info!("Successfully set AppUserModelID to: {APP_ID}");

    Ok(())
}

/// Sets window-level properties for proper display in Windows media controls.
///
/// This function sets the AppUserModel properties on a specific window, including:
/// - AppUserModelID: Identifies the application
/// - RelaunchDisplayNameResource: The display name shown in Windows UI
/// - RelaunchCommand: Command to relaunch the application
///
/// These properties must be set on the window for Windows to properly display
/// the application name in media controls.
pub fn set_window_app_id(window_handle: WindowHandle) -> anyhow::Result<()> {
    let window_handle = window_handle.as_raw();
    let RawWindowHandle::Win32(window_handle) = window_handle else {
        anyhow::bail!("Window handle is not a Win32 handle");
    };

    tracing::info!("Setting window AppUserModel properties");

    fn update_property_store(hwnd: HWND) -> anyhow::Result<()> {
        // Get the property store for the window
        // SAFETY: hwnd is assumed to be valid per function contract
        let property_store: IPropertyStore = unsafe {
            SHGetPropertyStoreForWindow(hwnd).context("Failed to get property store for window")?
        };

        // Get the executable path for the relaunch command
        let exe_path = std::env::current_exe()
            .context("Failed to get executable path")?
            .to_string_lossy()
            .to_string();

        // Set AppUserModelID
        set_property_string(&property_store, &PKEY_AppUserModel_ID, APP_ID)
            .context("Failed to set AppUserModelID property")?;

        // Set display name
        set_property_string(
            &property_store,
            &PKEY_AppUserModel_RelaunchDisplayNameResource,
            DISPLAY_NAME,
        )
        .context("Failed to set display name property")?;

        // Set relaunch command
        set_property_string(
            &property_store,
            &PKEY_AppUserModel_RelaunchCommand,
            &exe_path,
        )
        .context("Failed to set relaunch command property")?;

        // Commit the changes
        // SAFETY: property_store is a valid IPropertyStore
        unsafe {
            property_store
                .Commit()
                .context("Failed to commit property changes")?;
        }

        tracing::info!("Successfully set window AppUserModel properties");
        Ok(())
    }

    let hwnd = HWND(window_handle.hwnd.get() as *mut std::ffi::c_void);
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .ok()
            .context("Failed to initialize COM")?;
        update_property_store(hwnd)
    }
}

/// Helper function to set a string property in the property store
fn set_property_string(
    property_store: &IPropertyStore,
    key: &PROPERTYKEY,
    value: &str,
) -> anyhow::Result<()> {
    let value_hstring = HSTRING::from(value);
    let value_pwstr = PWSTR::from_raw(value_hstring.as_ptr() as *mut u16);

    unsafe {
        // Create a PROPVARIANT with VT_LPWSTR type (single string, not vector)
        let mut prop_variant = PROPVARIANT::default();
        (*prop_variant.Anonymous.Anonymous).vt = VT_LPWSTR;
        (*prop_variant.Anonymous.Anonymous).Anonymous.pwszVal = value_pwstr;

        let result = property_store.SetValue(key, &prop_variant);

        // Clear the PROPVARIANT (but don't free the string - it's owned by value_hstring)
        (*prop_variant.Anonymous.Anonymous).Anonymous.pwszVal = PWSTR::null();
        PropVariantClear(&mut prop_variant).ok();

        result
    }
    .with_context(|| format!("Failed to set property value for {value}"))
}
