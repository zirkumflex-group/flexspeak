//! Saying goodbye to the servers when the app goes without its usual close.
//!
//! Closing the window disconnects every server before the app quits. Two other endings would
//! leave you on the server until it times you out: a crash, and Windows signing out, restarting
//! or shutting down, which ends the app without a close. Both now disconnect first. A process
//! killed outright (Task Manager's End Task) runs no code at all, so the server notices on its
//! own timeout; nothing in the app can change that.

use std::time::Duration;

use crate::client;

/// How long a crash waits for the servers to let go before the process goes.
const CRASH_LIMIT: Duration = Duration::from_millis(1500);
/// Windows allows a few seconds after sign-out before it calls an app hung.
#[cfg(windows)]
const SESSION_END_LIMIT: Duration = Duration::from_secs(2);

pub fn install() {
    // A panic on the UI thread takes the whole app down; disconnect before it does. Other
    // threads' panics are handled where they happen and leave the app running.
    let report = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        report(info);
        if std::thread::current().name() == Some("main") {
            client::stop_all(CRASH_LIMIT);
        }
    }));
    #[cfg(windows)]
    windows::watch();
}

#[cfg(windows)]
mod windows {
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, MSG, RegisterClassW,
        WM_ENDSESSION, WM_QUERYENDSESSION, WNDCLASSW,
    };

    /// Windows tells top-level windows that the session is ending, then ends the process once
    /// they have answered. Rune's window doesn't listen, so a hidden window of our own does, on
    /// its own thread, so a busy UI thread can't delay the goodbye.
    pub fn watch() {
        let _ = std::thread::Builder::new()
            .name("session-end".into())
            .spawn(|| {
                let class: Vec<u16> = "FlexspeakSessionEnd\0".encode_utf16().collect();
                // SAFETY: the class name outlives the window, which lives as long as this thread;
                // zeroed fields are the documented defaults for WNDCLASSW and MSG.
                unsafe {
                    let instance = GetModuleHandleW(std::ptr::null());
                    let mut wc: WNDCLASSW = std::mem::zeroed();
                    wc.lpfnWndProc = Some(procedure);
                    wc.hInstance = instance;
                    wc.lpszClassName = class.as_ptr();
                    if RegisterClassW(&wc) == 0 {
                        return;
                    }
                    // A top-level window that is never shown: message-only windows don't hear
                    // about the session ending.
                    let window = CreateWindowExW(
                        0,
                        class.as_ptr(),
                        class.as_ptr(),
                        0,
                        0,
                        0,
                        0,
                        0,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        instance,
                        std::ptr::null(),
                    );
                    if window.is_null() {
                        return;
                    }
                    let mut message: MSG = std::mem::zeroed();
                    while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
                        DispatchMessageW(&message);
                    }
                }
            });
    }

    unsafe extern "system" fn procedure(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            // Never stand in the way of signing out.
            WM_QUERYENDSESSION => 1,
            WM_ENDSESSION => {
                if wparam != 0 {
                    super::client::stop_all(super::SESSION_END_LIMIT);
                }
                0
            }
            // SAFETY: forwarding the message this procedure was given, unchanged.
            _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
        }
    }
}
