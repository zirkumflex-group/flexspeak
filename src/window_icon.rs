//! Windows takes a window's title-bar icon from `WM_SETICON`, not from the executable, so
//! without this the main window shows the generic application icon. Rune does not expose the
//! window handle, so the UI thread's titled windows are found by enumeration once they show.

/// Cheap after the first success: called at the start of every frame.
#[cfg(windows)]
pub fn apply() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumThreadWindows, GWL_STYLE, GetSystemMetrics, GetWindowLongW, ICON_BIG, ICON_SMALL,
        IMAGE_ICON, IsWindowVisible, LR_DEFAULTCOLOR, LoadImageW, SM_CXICON, SM_CXSMICON,
        SM_CYICON, SM_CYSMICON, SendMessageW, WM_SETICON, WS_CAPTION,
    };

    /// `assets/icon/flexspeak.rc` embeds the icon as resource 1.
    const ICON_RESOURCE: usize = 1;
    static DONE: AtomicBool = AtomicBool::new(false);

    unsafe extern "system" fn visit(hwnd: HWND, found: LPARAM) -> i32 {
        // SAFETY: `hwnd` comes from EnumThreadWindows, and `found` is the `bool` passed below,
        // which outlives the enumeration.
        unsafe {
            let titled = GetWindowLongW(hwnd, GWL_STYLE) as u32 & WS_CAPTION == WS_CAPTION;
            if IsWindowVisible(hwnd) == 0 || !titled {
                return 1;
            }
            let module = GetModuleHandleW(std::ptr::null());
            for (kind, width, height) in [
                (ICON_BIG, SM_CXICON, SM_CYICON),
                (ICON_SMALL, SM_CXSMICON, SM_CYSMICON),
            ] {
                let icon = LoadImageW(
                    module,
                    ICON_RESOURCE as *const u16,
                    IMAGE_ICON,
                    GetSystemMetrics(width),
                    GetSystemMetrics(height),
                    LR_DEFAULTCOLOR,
                );
                if !icon.is_null() {
                    SendMessageW(hwnd, WM_SETICON, kind as WPARAM, icon as LPARAM);
                    *(found as *mut bool) = true;
                }
            }
        }
        1
    }

    if DONE.load(Ordering::Relaxed) {
        return;
    }
    let mut found = false;
    // SAFETY: the callback only touches `found` for the duration of this call.
    unsafe {
        EnumThreadWindows(
            GetCurrentThreadId(),
            Some(visit),
            &mut found as *mut bool as LPARAM,
        );
    }
    if found {
        DONE.store(true, Ordering::Relaxed);
    }
}

#[cfg(not(windows))]
pub fn apply() {}
