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

#[derive(Default)]
pub struct Permissions {
    pub pending: Option<Kind>,
    pub messages: [Option<String>; 3],
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
        let _ = kind;
        anyhow::ensure!(
            std::process::Command::new("/usr/bin/open")
                .args(["-b", "com.apple.systempreferences"])
                .status()?
                .success(),
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
