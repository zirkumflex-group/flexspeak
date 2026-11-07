//! Windows icon and borderless-window behavior. Rune does not expose its HWND, so find
//! the main window on its owning UI thread. Keep the task-switcher icon and native resizing.

/// Cheap after the first success: called at the start of every frame.
#[cfg(windows)]
pub fn apply() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumThreadWindows, GetSystemMetrics, GetWindowTextW, ICON_BIG, ICON_SMALL, IMAGE_ICON,
        IsWindowVisible, LR_DEFAULTCOLOR, LoadImageW, SM_CXICON, SM_CXSMICON, SM_CYICON,
        SM_CYSMICON, SendMessageW, WM_SETICON,
    };

    /// `assets/icon/flexspeak.rc` embeds the icon as resource 1.
    const ICON_RESOURCE: usize = 1;
    static DONE: AtomicBool = AtomicBool::new(false);

    unsafe extern "system" fn visit(hwnd: HWND, found: LPARAM) -> i32 {
        // SAFETY: `hwnd` comes from EnumThreadWindows, and `found` is the `bool` passed below,
        // which outlives the enumeration.
        unsafe {
            let mut title = [0_u16; 64];
            let length = GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32);
            if IsWindowVisible(hwnd) == 0
                || String::from_utf16_lossy(&title[..length.max(0) as usize]) != "Flexspeak"
            {
                return 1;
            }
            install_resize_handler(hwnd);
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

/// A full client area has no native resize border; return native edge hits at its perimeter.
#[cfg(windows)]
unsafe fn install_resize_handler(hwnd: windows_sys::Win32::Foundation::HWND) {
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTLEFT, HTRIGHT, HTTOP, HTTOPLEFT,
        HTTOPRIGHT, IsZoomed, WM_NCDESTROY, WM_NCHITTEST,
    };
    unsafe extern "system" fn hit_test(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        id: usize,
        _: usize,
    ) -> LRESULT {
        // SAFETY: comctl32 supplies this live HWND on its owning thread. The subclass
        // stores no borrowed state and removes itself when the native window is destroyed.
        unsafe {
            if message == WM_NCHITTEST && IsZoomed(hwnd) == 0 {
                let mut rect = RECT::default();
                if GetWindowRect(hwnd, &mut rect) != 0 {
                    let x = (lparam as u16 as i16) as i32;
                    let y = ((lparam >> 16) as u16 as i16) as i32;
                    let edge = (8 * GetDpiForWindow(hwnd).max(96) / 96) as i32;
                    let left = x >= rect.left && x < rect.left + edge;
                    let right = x < rect.right && x >= rect.right - edge;
                    let top = y >= rect.top && y < rect.top + edge;
                    let bottom = y < rect.bottom && y >= rect.bottom - edge;
                    let hit = match (left, right, top, bottom) {
                        (true, _, true, _) => Some(HTTOPLEFT),
                        (_, true, true, _) => Some(HTTOPRIGHT),
                        (true, _, _, true) => Some(HTBOTTOMLEFT),
                        (_, true, _, true) => Some(HTBOTTOMRIGHT),
                        (true, _, _, _) => Some(HTLEFT),
                        (_, true, _, _) => Some(HTRIGHT),
                        (_, _, true, _) => Some(HTTOP),
                        (_, _, _, true) => Some(HTBOTTOM),
                        _ => None,
                    };
                    if let Some(hit) = hit {
                        return hit as LRESULT;
                    }
                }
            }
            if message == WM_NCDESTROY {
                RemoveWindowSubclass(hwnd, Some(hit_test), id);
            }
            DefSubclassProc(hwnd, message, wparam, lparam)
        }
    }
    // SAFETY: called only while enumerating this UI thread's windows.
    unsafe {
        SetWindowSubclass(hwnd, Some(hit_test), 1, 0);
    }
}

/// Rune ignores `Tx::minimize` on a borderless window, so both header buttons act on the
/// UI thread's main HWND directly.
#[cfg(windows)]
pub fn minimize() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SW_MINIMIZE, ShowWindow};
    // SAFETY: `main_window` passes a live window owned by this UI thread.
    main_window(|hwnd| unsafe {
        ShowWindow(hwnd, SW_MINIMIZE);
    });
}

#[cfg(windows)]
pub fn toggle_maximized() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        IsZoomed, SW_MAXIMIZE, SW_RESTORE, ShowWindow,
    };
    // SAFETY: `main_window` passes a live window owned by this UI thread.
    main_window(|hwnd| unsafe {
        ShowWindow(
            hwnd,
            if IsZoomed(hwnd) != 0 {
                SW_RESTORE
            } else {
                SW_MAXIMIZE
            },
        );
    });
}

/// Runs `action` on this UI thread's visible window titled "Flexspeak".
#[cfg(windows)]
fn main_window(action: fn(windows_sys::Win32::Foundation::HWND)) {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumThreadWindows, GetWindowTextW, IsWindowVisible,
    };
    unsafe extern "system" fn visit(hwnd: HWND, action: LPARAM) -> i32 {
        // SAFETY: enumeration supplies a valid window owned by this UI thread, and `action`
        // is the function pointer passed below.
        unsafe {
            let mut title = [0_u16; 64];
            let length = GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32);
            if IsWindowVisible(hwnd) != 0
                && String::from_utf16_lossy(&title[..length.max(0) as usize]) == "Flexspeak"
            {
                let action = std::mem::transmute::<LPARAM, fn(HWND)>(action);
                action(hwnd);
                return 0;
            }
        }
        1
    }
    // SAFETY: only windows belonging to the calling UI thread are visited.
    unsafe {
        EnumThreadWindows(GetCurrentThreadId(), Some(visit), action as usize as LPARAM);
    }
}
