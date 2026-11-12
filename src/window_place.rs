//! The main window's place on screen, kept between launches, and the taskbar flash that asks
//! for attention while it is in the background. Windows only; elsewhere these do nothing.
//! Like `window_icon`, the window is found by its title on the calling UI thread.

use crate::settings::WindowPlace;

/// The smallest piece of a saved window that must lie on a connected display for it to open
/// there again: enough of the title bar to grab it.
const GRAB: i32 = 48;

/// Where the window is now, in physical pixels: its restored rectangle while it is maximized
/// or minimized. `None` before the window exists, as in headless previews.
#[cfg(windows)]
pub fn current() -> Option<WindowPlace> {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowPlacement, GetWindowRect, IsIconic, IsZoomed, WINDOWPLACEMENT,
    };
    let hwnd = window()?;
    // SAFETY: `hwnd` is a live window owned by this UI thread; both structs are plain data.
    unsafe {
        let maximized = IsZoomed(hwnd) != 0;
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if maximized || IsIconic(hwnd) != 0 {
            let mut placement: WINDOWPLACEMENT = std::mem::zeroed();
            placement.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
            if GetWindowPlacement(hwnd, &mut placement) == 0 {
                return None;
            }
            rect = placement.rcNormalPosition;
        } else if GetWindowRect(hwnd, &mut rect) == 0 {
            return None;
        }
        let width = u32::try_from(rect.right - rect.left).ok()?;
        let height = u32::try_from(rect.bottom - rect.top).ok()?;
        (width > 0 && height > 0).then_some(WindowPlace {
            x: rect.left,
            y: rect.top,
            width,
            height,
            maximized,
        })
    }
}

#[cfg(not(windows))]
pub fn current() -> Option<WindowPlace> {
    None
}

/// Whether the top of a saved window still lies on a connected display, so a monitor that has
/// gone since doesn't open it out of reach.
#[cfg(windows)]
pub fn on_screen(place: &WindowPlace) -> bool {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONULL, MonitorFromRect};
    let width = i32::try_from(place.width).unwrap_or(i32::MAX);
    let grab = RECT {
        left: place.x.saturating_add(width / 2 - GRAB / 2),
        top: place.y,
        right: place.x.saturating_add(width / 2 + GRAB / 2),
        bottom: place.y.saturating_add(GRAB / 2),
    };
    // SAFETY: plain value arguments; the returned handle is only compared with null.
    !unsafe { MonitorFromRect(&grab, MONITOR_DEFAULTTONULL) }.is_null()
}

#[cfg(not(windows))]
pub fn on_screen(_: &WindowPlace) -> bool {
    false
}

/// A saved size that is still at least the window's minimum; smaller ones are dropped.
pub fn usable(place: &WindowPlace) -> bool {
    place.width >= GRAB as u32 * 4 && place.height >= GRAB as u32 * 4
}

static MAXIMIZE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The window maximizes once it is first on screen, as it was when it last closed.
pub fn maximize_when_shown() {
    MAXIMIZE.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Cheap: called at the start of every frame, does something once at most.
#[cfg(windows)]
pub fn apply() {
    use std::sync::atomic::Ordering;
    use windows_sys::Win32::UI::WindowsAndMessaging::{IsWindowVisible, SW_MAXIMIZE, ShowWindow};
    if FRAME_FIXED.load(Ordering::Relaxed) && !MAXIMIZE.load(Ordering::Relaxed) {
        return;
    }
    let Some(hwnd) = window() else {
        return;
    };
    if !FRAME_FIXED.swap(true, Ordering::Relaxed) {
        fix_full_screen_frame(hwnd);
    }
    if !MAXIMIZE.load(Ordering::Relaxed) {
        return;
    }
    // SAFETY: `hwnd` is a live window owned by this UI thread.
    unsafe {
        if IsWindowVisible(hwnd) != 0 {
            MAXIMIZE.store(false, Ordering::Relaxed);
            ShowWindow(hwnd, SW_MAXIMIZE);
        }
    }
}

#[cfg(not(windows))]
pub fn apply() {}

#[cfg(windows)]
static FRAME_FIXED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Full screen, the whole window is the picture. Rune's frame treats a window that was
/// maximized before going full screen as still maximized and keeps the sizing border's band at
/// the top, plus a pixel at each edge with an auto-hiding taskbar, so the stream sat inside
/// empty edges. Full screen has no caption (winit drops the frame styles), and then this hands
/// the client area the whole window rect, after the procedures below have run.
#[cfg(windows)]
fn fix_full_screen_frame(hwnd: windows_sys::Win32::Foundation::HWND) {
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GWL_STYLE, GetWindowLongPtrW, NCCALCSIZE_PARAMS, WM_NCCALCSIZE, WS_CAPTION,
    };
    unsafe extern "system" fn procedure(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _: usize,
        _: usize,
    ) -> LRESULT {
        // SAFETY: forwards to the next procedure; for `WM_NCCALCSIZE` with a nonzero
        // `wparam`, `lparam` points to the `NCCALCSIZE_PARAMS` Windows documents.
        unsafe {
            let full_screen = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32 & WS_CAPTION == 0;
            if message != WM_NCCALCSIZE || wparam == 0 || !full_screen {
                return DefSubclassProc(hwnd, message, wparam, lparam);
            }
            let params = &mut *(lparam as *mut NCCALCSIZE_PARAMS);
            let outer = params.rgrc[0];
            DefSubclassProc(hwnd, message, wparam, lparam);
            params.rgrc[0] = outer;
            0
        }
    }
    // SAFETY: `hwnd` is a live window owned by this UI thread, which runs its procedures.
    unsafe {
        SetWindowSubclass(hwnd, Some(procedure), 0x464C_5853, 0);
    }
}

/// Flashes the taskbar button until the window comes to the front, or stops it.
#[cfg(windows)]
pub fn flash(on: bool) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FLASHW_STOP, FLASHW_TIMERNOFG, FLASHW_TRAY, FLASHWINFO, FlashWindowEx,
    };
    let Some(hwnd) = window() else {
        return;
    };
    let info = FLASHWINFO {
        cbSize: std::mem::size_of::<FLASHWINFO>() as u32,
        hwnd,
        dwFlags: if on {
            FLASHW_TRAY | FLASHW_TIMERNOFG
        } else {
            FLASHW_STOP
        },
        uCount: 0,
        dwTimeout: 0,
    };
    // SAFETY: `info` describes a live window owned by this UI thread.
    unsafe {
        FlashWindowEx(&info);
    }
}

#[cfg(not(windows))]
pub fn flash(_: bool) {}

/// This UI thread's window titled "Flexspeak", shown or hidden.
#[cfg(windows)]
fn window() -> Option<windows_sys::Win32::Foundation::HWND> {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{EnumThreadWindows, GetWindowTextW};
    unsafe extern "system" fn visit(hwnd: HWND, found: LPARAM) -> i32 {
        // SAFETY: enumeration supplies a valid window of this thread, and `found` is the
        // `HWND` slot passed below, which outlives the enumeration.
        unsafe {
            let mut title = [0_u16; 64];
            let length = GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32);
            if String::from_utf16_lossy(&title[..length.max(0) as usize]) == "Flexspeak" {
                *(found as *mut HWND) = hwnd;
                return 0;
            }
        }
        1
    }
    let mut found: HWND = std::ptr::null_mut();
    // SAFETY: the callback only writes `found` for the duration of this call.
    unsafe {
        EnumThreadWindows(
            GetCurrentThreadId(),
            Some(visit),
            &mut found as *mut HWND as LPARAM,
        );
    }
    (!found.is_null()).then_some(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiny_saved_windows_are_not_restored() {
        let place = |width, height| WindowPlace {
            x: 0,
            y: 0,
            width,
            height,
            maximized: false,
        };
        assert!(usable(&place(1180, 790)));
        assert!(!usable(&place(0, 790)));
        assert!(!usable(&place(1180, 40)));
    }
}
