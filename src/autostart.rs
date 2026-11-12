//! Start with Windows: a value under the current user's Run key that opens this copy at sign-in,
//! with `--minimized` when it should start in the notification area. The registry is the only
//! record, so the switches show what Windows will really do. Other platforms have none.

/// The flag the Run entry passes for Start Minimized.
pub const MINIMIZED: &str = "--minimized";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Autostart {
    pub enabled: bool,
    pub minimized: bool,
}

pub const SUPPORTED: bool = cfg!(windows);

/// The command line Windows runs at sign-in.
pub fn command(exe: &std::path::Path, minimized: bool) -> String {
    let mut command = format!("\"{}\"", exe.display());
    if minimized {
        command.push(' ');
        command.push_str(MINIMIZED);
    }
    command
}

/// What an existing Run entry asks for.
pub fn parse(command: Option<&str>) -> Autostart {
    command.map_or_else(Autostart::default, |command| Autostart {
        enabled: true,
        minimized: command.split_whitespace().any(|part| part == MINIMIZED),
    })
}

#[cfg(windows)]
mod native {
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW,
        RegSetKeyValueW,
    };

    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const NAME: &str = "Flexspeak";

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }

    pub fn read() -> Option<String> {
        let (path, name) = (wide(RUN), wide(NAME));
        let mut data = vec![0_u16; 1024];
        let mut bytes = (data.len() * 2) as u32;
        // SAFETY: the buffers outlive the call and `bytes` is the size of `data` in bytes.
        let result = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                path.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                data.as_mut_ptr().cast(),
                &mut bytes,
            )
        };
        if result != 0 {
            return None;
        }
        let length = data.iter().position(|unit| *unit == 0).unwrap_or(data.len());
        Some(String::from_utf16_lossy(&data[..length]))
    }

    pub fn write(command: Option<&str>) -> Result<(), String> {
        let (path, name) = (wide(RUN), wide(NAME));
        // SAFETY: every pointer is to a NUL-terminated buffer that outlives the call.
        let result = unsafe {
            match command {
                Some(command) => {
                    let value = wide(command);
                    RegSetKeyValueW(
                        HKEY_CURRENT_USER,
                        path.as_ptr(),
                        name.as_ptr(),
                        REG_SZ,
                        value.as_ptr().cast(),
                        (value.len() * 2) as u32,
                    )
                }
                None => RegDeleteKeyValueW(HKEY_CURRENT_USER, path.as_ptr(), name.as_ptr()),
            }
        };
        // Removing an entry that isn't there leaves things as asked.
        const ERROR_FILE_NOT_FOUND: u32 = 2;
        if result == 0 || (command.is_none() && result == ERROR_FILE_NOT_FOUND) {
            Ok(())
        } else {
            Err(std::io::Error::from_raw_os_error(result as i32).to_string())
        }
    }
}

/// What Windows does at sign-in now.
#[cfg(windows)]
pub fn current() -> Autostart {
    parse(native::read().as_deref())
}

#[cfg(not(windows))]
pub fn current() -> Autostart {
    Autostart::default()
}

/// Adds, changes or removes the Run entry for this copy.
#[cfg(windows)]
pub fn set(wanted: Autostart) -> Result<(), String> {
    if !wanted.enabled {
        return native::write(None);
    }
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    native::write(Some(&command(&exe, wanted.minimized)))
}

#[cfg(not(windows))]
pub fn set(_: Autostart) -> Result<(), String> {
    Err("Starting with the computer is only available on Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_run_entry_round_trips() {
        let exe = std::path::Path::new(r"C:\Users\Jan\AppData\Local\Flexspeak\flexspeak.exe");
        for minimized in [false, true] {
            let parsed = parse(Some(&command(exe, minimized)));
            assert_eq!(
                parsed,
                Autostart {
                    enabled: true,
                    minimized
                }
            );
        }
        assert_eq!(parse(None), Autostart::default());
    }
}
