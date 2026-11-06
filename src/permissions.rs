use anyhow::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Microphone,
    Screen,
    SystemAudio,
}
impl Kind {
    pub const ALL: [Self; 3] = [Self::Microphone, Self::Screen, Self::SystemAudio];
    pub fn index(self) -> usize {
        match self {
            Self::Microphone => 0,
            Self::Screen => 1,
            Self::SystemAudio => 2,
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Self::Microphone => "Microphone",
            Self::Screen => "Screen sharing",
            Self::SystemAudio => "System audio",
        }
    }
    pub fn detail(self) -> &'static str {
        match self {
            Self::Microphone => "For voice chat and your local microphone test.",
            Self::Screen => {
                "For the screen or window you choose to share. No camera access is needed."
            }
            Self::SystemAudio => {
                "For sharing sound from your playback device. Separate from microphone mute."
            }
        }
    }
}

/// What the operating system currently allows. Reading it never prompts the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
    Granted,
    Denied,
    /// The user has not been asked yet; requesting shows the system prompt.
    NotDetermined,
    /// This platform shares without a separate grant.
    NotRequired,
    /// The system offers no way to read it.
    #[default]
    Unknown,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Self::Granted => "Allowed",
            Self::Denied => "Not allowed",
            Self::NotDetermined => "Not allowed yet",
            Self::NotRequired => "Not needed",
            Self::Unknown => "Unknown",
        }
    }
}

#[derive(Default)]
pub struct Permissions {
    pub pending: Option<Kind>,
    pub messages: [Option<String>; 3],
    pub status: [Status; 3],
}

impl Permissions {
    /// Re-reads every status; cheap enough to run on the usage tick while the section shows.
    pub fn refresh(&mut self) {
        self.status = Kind::ALL.map(status);
        for kind in Kind::ALL {
            // The status says it now; an old "turn it on" hint would contradict it.
            if self.status[kind.index()] == Status::Granted && self.pending != Some(kind) {
                self.messages[kind.index()] = None;
            }
        }
    }
}

pub fn status(kind: Kind) -> Status {
    #[cfg(target_os = "macos")]
    {
        match kind {
            Kind::Microphone => {
                use objc2_avf_audio::{AVAudioApplication, AVAudioApplicationRecordPermission};
                match unsafe { AVAudioApplication::sharedInstance().recordPermission() } {
                    AVAudioApplicationRecordPermission::Granted => Status::Granted,
                    AVAudioApplicationRecordPermission::Denied => Status::Denied,
                    AVAudioApplicationRecordPermission::Undetermined => Status::NotDetermined,
                    _ => Status::Unknown,
                }
            }
            Kind::Screen => {
                if objc2_core_graphics::CGPreflightScreenCaptureAccess() {
                    Status::Granted
                } else {
                    // Core Graphics only answers yes/no; TCC tells "denied" from "never asked".
                    match tcc::preflight(c"kTCCServiceScreenCapture") {
                        Some(Status::Granted) | None => Status::NotDetermined,
                        Some(status) => status,
                    }
                }
            }
            Kind::SystemAudio => tcc::preflight(c"kTCCServiceAudioCapture").unwrap_or_default(),
        }
    }
    #[cfg(windows)]
    {
        match kind {
            Kind::Microphone => windows_consent::microphone(),
            Kind::Screen | Kind::SystemAudio => Status::NotRequired,
        }
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = kind;
        Status::Unknown
    }
}

/// macOS has no public status query for system-audio capture, so this reads TCC's own preflight,
/// the call System Settings uses. It is loaded at runtime and falls back to "unknown" if absent.
#[cfg(target_os = "macos")]
mod tcc {
    use super::Status;
    use std::ffi::{CStr, c_char, c_void};
    use std::sync::OnceLock;

    type Preflight = unsafe extern "C" fn(*const c_void, *const c_void) -> i32;

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringCreateWithCString(
            allocator: *const c_void,
            text: *const c_char,
            encoding: u32,
        ) -> *const c_void;
        fn CFRelease(object: *const c_void);
    }
    const UTF8: u32 = 0x0800_0100;

    fn function() -> Option<Preflight> {
        static FUNCTION: OnceLock<Option<Preflight>> = OnceLock::new();
        *FUNCTION.get_or_init(|| unsafe {
            let library = libc::dlopen(
                c"/System/Library/PrivateFrameworks/TCC.framework/Versions/A/TCC".as_ptr(),
                libc::RTLD_NOW,
            );
            if library.is_null() {
                return None;
            }
            let symbol = libc::dlsym(library, c"TCCAccessPreflight".as_ptr());
            (!symbol.is_null()).then(|| std::mem::transmute::<*mut c_void, Preflight>(symbol))
        })
    }

    pub fn preflight(service: &CStr) -> Option<Status> {
        let preflight = function()?;
        unsafe {
            let name = CFStringCreateWithCString(std::ptr::null(), service.as_ptr(), UTF8);
            if name.is_null() {
                return None;
            }
            let result = preflight(name, std::ptr::null());
            CFRelease(name);
            Some(match result {
                0 => Status::Granted,
                1 => Status::Denied,
                _ => Status::NotDetermined,
            })
        }
    }
}

/// Windows keeps privacy switches under CapabilityAccessManager; desktop (unpackaged) apps follow
/// the device switch, the "Let apps access your microphone" switch and the desktop-apps switch.
#[cfg(windows)]
mod windows_consent {
    use super::Status;
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW,
    };

    const MICROPHONE: &str = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone";

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }

    /// `Some(false)` for an explicit "Deny"; a missing value means the Windows default, allowed.
    fn allowed(root: HKEY, path: &str) -> Option<bool> {
        let path = wide(path);
        let name = wide("Value");
        let mut data = [0u16; 16];
        let mut bytes = std::mem::size_of_val(&data) as u32;
        let result = unsafe {
            RegGetValueW(
                root,
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
        let length = data
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(data.len());
        Some(!String::from_utf16_lossy(&data[..length]).eq_ignore_ascii_case("Deny"))
    }

    pub fn microphone() -> Status {
        let switches = [
            (HKEY_LOCAL_MACHINE, MICROPHONE.to_owned()),
            (HKEY_CURRENT_USER, MICROPHONE.to_owned()),
            (HKEY_CURRENT_USER, format!(r"{MICROPHONE}\NonPackaged")),
        ];
        if switches
            .iter()
            .any(|(root, path)| allowed(*root, path) == Some(false))
        {
            Status::Denied
        } else {
            Status::Granted
        }
    }
}

/// Called only after a user presses a permission button; startup does not prompt or capture.
pub async fn request(
    kind: Kind,
    output_device: Option<crate::audio::DeviceChoice>,
) -> Result<String> {
    #[cfg(not(target_os = "macos"))]
    let _ = output_device;
    #[cfg(target_os = "macos")]
    {
        match kind {
            Kind::Microphone => {
                use block2::RcBlock;
                use objc2::runtime::Bool;
                use objc2_avf_audio::AVAudioApplication;
                let (sender, receiver) = tokio::sync::oneshot::channel();
                {
                    let sender = std::sync::Mutex::new(Some(sender));
                    let callback = RcBlock::new(move |granted: Bool| {
                        if let Some(sender) = sender.lock().unwrap().take() {
                            let _ = sender.send(granted.as_bool());
                        }
                    });
                    // macOS copies the block; no Objective-C object crosses the await.
                    unsafe {
                        AVAudioApplication::requestRecordPermissionWithCompletionHandler(&callback);
                    }
                }
                Ok(if receiver.await? { "Microphone access allowed." } else { "Microphone access is off. Enable it in System Settings → Privacy & Security → Microphone." }.into())
            }
            Kind::Screen => {
                let allowed = tokio::task::spawn_blocking(|| {
                    objc2_core_graphics::CGPreflightScreenCaptureAccess()
                        || objc2_core_graphics::CGRequestScreenCaptureAccess()
                })
                .await?;
                Ok(if allowed { "Screen recording access allowed." } else { "Enable Flexspeak in System Settings → Privacy & Security → Screen & System Audio Recording. macOS may ask you to restart the app." }.into())
            }
            Kind::SystemAudio => {
                let device = output_device.ok_or_else(||anyhow::anyhow!("Choose a playback device in the Devices step or Audio settings, then request system-audio access again."))?;
                tokio::task::spawn_blocking(move || {
                    crate::streaming::request_system_audio_access(device)
                })
                .await??;
                Ok("System-audio access requested. Confirm the macOS prompt if shown; verify the audio meter when you share. Samples from this check are discarded; nothing is saved or transmitted.".into())
            }
        }
    }
    #[cfg(windows)]
    {
        match kind {
            Kind::Microphone => {
                open_settings(kind)?;
                Ok("Microphone privacy settings opened. Allow microphone access for desktop apps, then test your device in the next step.".into())
            },
            Kind::Screen => Ok("Windows desktop capture needs no setup grant. Select a screen or window when starting a share; OS or organization policies can still restrict capture.".into()),
            Kind::SystemAudio => Ok("Windows captures your chosen playback output without a separate setup grant. Protected audio may be excluded.".into()),
        }
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        Ok(match kind {
            Kind::Microphone => {
                "Use the device test to check microphone access through your system audio service."
            }
            _ => {
                "Native screen and system-audio sharing are currently available on Mac and Windows."
            }
        }
        .into())
    }
}

pub fn open_settings(kind: Kind) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let pane = match kind {
            Kind::Microphone => "Privacy_Microphone",
            // System-audio recording shares the "Screen & System Audio Recording" pane.
            Kind::Screen | Kind::SystemAudio => "Privacy_ScreenCapture",
        };
        let open = |args: &[&str]| {
            std::process::Command::new("/usr/bin/open")
                .args(args)
                .status()
                .is_ok_and(|status| status.success())
        };
        let url = format!("x-apple.systempreferences:com.apple.preference.security?{pane}");
        anyhow::ensure!(
            open(&[url.as_str()]) || open(&["-b", "com.apple.systempreferences"]),
            "Could not open System Settings"
        );
        Ok(())
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Com::{
            COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
        };
        use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};
        let uri = if kind == Kind::Microphone {
            "ms-settings:privacy-microphone"
        } else {
            "ms-settings:privacy"
        };
        let uri: Vec<_> = uri.encode_utf16().chain(Some(0)).collect();
        let verb: Vec<_> = "open".encode_utf16().chain(Some(0)).collect();
        // URI handlers may use COM; balance only a successful initialization on this thread.
        let initialized = unsafe {
            CoInitializeEx(
                std::ptr::null(),
                (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
            )
        } >= 0;
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                uri.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        } as isize;
        if initialized {
            unsafe {
                CoUninitialize();
            }
        }
        anyhow::ensure!(result > 32, "Could not open Windows privacy settings");
        Ok(())
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = kind;
        anyhow::bail!("Open your desktop's sound/privacy settings to change microphone access.")
    }
}
