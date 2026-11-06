//! Whether the operating system is in dark mode, for the System colour scheme. Rune does not
//! report it, so each platform is asked directly. `None` means it could not be read.

/// The application's effective appearance, which follows System Settings (including Auto).
/// Call on the main thread; the UI thread is the main thread on macOS.
#[cfg(target_os = "macos")]
pub fn is_dark() -> Option<bool> {
    use objc2::{class, msg_send, runtime::AnyObject};
    use std::ffi::{CStr, c_char};

    objc2::rc::autoreleasepool(|_| {
        // SAFETY: each message is sent to a non-null object that responds to it, and the
        // returned name is copied before the pool drains.
        unsafe {
            let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
            if app.is_null() {
                return None;
            }
            let appearance: *mut AnyObject = msg_send![app, effectiveAppearance];
            if appearance.is_null() {
                return None;
            }
            let name: *mut AnyObject = msg_send![appearance, name];
            if name.is_null() {
                return None;
            }
            let utf8: *const c_char = msg_send![name, UTF8String];
            if utf8.is_null() {
                return None;
            }
            Some(
                CStr::from_ptr(utf8)
                    .to_bytes()
                    .windows(4)
                    .any(|w| w == b"Dark"),
            )
        }
    })
}

/// Windows' "app mode" in Personalisation › Colours.
#[cfg(windows)]
pub fn is_dark() -> Option<bool> {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};

    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let key = wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    let value = wide("AppsUseLightTheme");
    let mut light: u32 = 1;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: both strings are NUL-terminated and `light` holds the `size` bytes it is given.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&raw mut light).cast(),
            &mut size,
        )
    };
    (status == 0).then_some(light == 0)
}

/// GNOME's colour-scheme preference, read once: asking means starting a process.
#[cfg(not(any(target_os = "macos", windows)))]
pub fn is_dark() -> Option<bool> {
    use std::sync::OnceLock;

    static DARK: OnceLock<Option<bool>> = OnceLock::new();
    *DARK.get_or_init(|| {
        let output = std::process::Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "color-scheme"])
            .stderr(std::process::Stdio::null())
            .output()
            .ok()
            .filter(|output| output.status.success())?;
        let scheme = String::from_utf8_lossy(&output.stdout);
        Some(scheme.contains("dark"))
    })
}
