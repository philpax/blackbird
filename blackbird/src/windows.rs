//! Windows-specific functionality for blackbird.

use anyhow::Context as _;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, STGM_READ,
};
use windows::Win32::UI::Shell::PropertiesSystem::{
    InitPropVariantFromStringAsVector, PropVariantClear, PROPERTYKEY,
};
use windows::Win32::UI::Shell::{
    SHGetPropertyStoreForWindow, SetCurrentProcessExplicitAppUserModelID, IPropertyStore,
};

/// Application User Model ID for blackbird
pub const APP_ID: &str = "com.philpax.blackbird";

/// Display name shown in Windows UI
pub const DISPLAY_NAME: &str = "Blackbird";

// Property keys for AppUserModel properties
// These are from propkey.h / shellapi.h
const PKEY_APPUSERMODEL_ID: PROPERTYKEY = PROPERTYKEY {
    fmtid: windows::core::GUID::from_u128(0x9F4C2855_9F79_4B39_A8D0_E1D42DE1D5F3),
    pid: 5,
};

const PKEY_APPUSERMODEL_RELAUNCHDISPLAYNAMERESOURCE: PROPERTYKEY = PROPERTYKEY {
    fmtid: windows::core::GUID::from_u128(0x9F4C2855_9F79_4B39_A8D0_E1D42DE1D5F3),
    pid: 4,
};

const PKEY_APPUSERMODEL_RELAUNCHCOMMAND: PROPERTYKEY = PROPERTYKEY {
    fmtid: windows::core::GUID::from_u128(0x9F4C2855_9F79_4B39_A8D0_E1D42DE1D5F3),
    pid: 2,
};

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

/// Sets window-level properties for proper display in Windows media controls.
///
/// This function sets the AppUserModel properties on a specific window, including:
/// - AppUserModelID: Identifies the application
/// - RelaunchDisplayNameResource: The display name shown in Windows UI
/// - RelaunchCommand: Command to relaunch the application
///
/// These properties must be set on the window for Windows to properly display
/// the application name in media controls.
///
/// # Safety
///
/// The hwnd parameter must be a valid window handle.
pub unsafe fn set_window_app_id(hwnd: *mut std::ffi::c_void) -> anyhow::Result<()> {
    tracing::info!("Setting window AppUserModel properties for HWND: {:?}", hwnd);

    // Initialize COM for this thread
    // SAFETY: CoInitializeEx is safe to call multiple times
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .context("Failed to initialize COM")?;
    }

    let result = (|| -> anyhow::Result<()> {
        let hwnd = HWND(hwnd as isize);

        // Get the property store for the window
        // SAFETY: hwnd is assumed to be valid per function contract
        let property_store: IPropertyStore = unsafe {
            SHGetPropertyStoreForWindow(hwnd, &IPropertyStore::IID)
                .context("Failed to get property store for window")?
        };

        // Get the executable path for the relaunch command
        let exe_path = std::env::current_exe()
            .context("Failed to get executable path")?
            .to_string_lossy()
            .to_string();

        // Set AppUserModelID
        set_property_string(&property_store, &PKEY_APPUSERMODEL_ID, APP_ID)
            .context("Failed to set AppUserModelID property")?;

        // Set display name
        set_property_string(
            &property_store,
            &PKEY_APPUSERMODEL_RELAUNCHDISPLAYNAMERESOURCE,
            DISPLAY_NAME,
        )
        .context("Failed to set display name property")?;

        // Set relaunch command
        set_property_string(
            &property_store,
            &PKEY_APPUSERMODEL_RELAUNCHCOMMAND,
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
    })();

    // Uninitialize COM
    // SAFETY: We initialized COM earlier in this function
    unsafe {
        CoUninitialize();
    }

    result
}

/// Helper function to set a string property in the property store
fn set_property_string(
    property_store: &IPropertyStore,
    key: &PROPERTYKEY,
    value: &str,
) -> anyhow::Result<()> {
    let value_hstring = HSTRING::from(value);
    let value_pcwstr = PCWSTR::from_raw(value_hstring.as_ptr());

    let mut prop_variant = unsafe { std::mem::zeroed() };

    // Initialize the PROPVARIANT with the string value
    // SAFETY: prop_variant is a valid PROPVARIANT, value_pcwstr is a valid string
    unsafe {
        InitPropVariantFromStringAsVector(value_pcwstr, &mut prop_variant)
            .with_context(|| format!("Failed to initialize PROPVARIANT for {value}"))?;
    }

    // Set the property value
    // SAFETY: property_store is valid, key is valid, prop_variant is initialized
    let result = unsafe { property_store.SetValue(key, &prop_variant) };

    // Clean up the PROPVARIANT
    // SAFETY: prop_variant was initialized by InitPropVariantFromStringAsVector
    unsafe {
        PropVariantClear(&mut prop_variant).ok();
    }

    result.with_context(|| format!("Failed to set property value for {value}"))?;

    Ok(())
}
