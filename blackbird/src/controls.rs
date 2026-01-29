#[cfg(feature = "media-controls")]
pub use blackbird_client_shared::controls::Controls;

/// Extract the HWND from an eframe `CreationContext` (Windows only).
#[cfg(feature = "media-controls")]
pub fn get_egui_hwnd(cc: Option<&eframe::CreationContext<'_>>) -> Option<*mut std::ffi::c_void> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    cc.and_then(|cc| cc.window_handle().ok())
        .and_then(|handle| {
            if let RawWindowHandle::Win32(win32_handle) = handle.as_raw() {
                Some(win32_handle.hwnd.get() as *mut std::ffi::c_void)
            } else {
                None
            }
        })
}
