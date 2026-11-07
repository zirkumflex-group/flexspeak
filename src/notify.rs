//! System notifications, for pokes that arrive while Flexspeak is behind other windows.
//!
//! Each platform uses what it ships with: Notification Center on macOS (an `osascript`
//! fallback outside an app bundle), a toast through Windows PowerShell's WinRT bridge on
//! Windows, and `notify-send` on Linux. Delivery is best effort: a missing helper or a denied
//! permission only means no banner, never an error dialog. The text never passes through a
//! shell; it travels as arguments or environment variables.

/// Shows `title` and `body` as a system notification without blocking the caller.
pub fn show(title: &str, body: &str) {
    let (title, body) = (clean(title, 64), clean(body, 240));
    let _ = std::thread::Builder::new()
        .name("flexspeak-notify".into())
        .spawn(move || {
            if let Err(error) = platform::show(&title, &body) {
                tracing::debug!("System notification not shown: {error}");
            }
        });
}

/// Control characters dropped, length bounded.
fn clean(text: &str, limit: usize) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .take(limit)
        .collect()
}

#[cfg(target_os = "macos")]
mod platform {
    use std::process::Command;

    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2_foundation::{NSError, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest,
        UNUserNotificationCenter,
    };

    pub fn show(title: &str, body: &str) -> Result<(), String> {
        // Notification Center needs the bundle's identity; `cargo run` has none.
        let bundled = std::env::current_exe().is_ok_and(|exe| {
            exe.ancestors()
                .any(|path| path.extension().is_some_and(|extension| extension == "app"))
        });
        if bundled {
            center(title, body);
            Ok(())
        } else {
            let status = Command::new("/usr/bin/osascript")
                .args([
                    "-e",
                    "on run argv",
                    "-e",
                    "display notification (item 2 of argv) with title (item 1 of argv)",
                    "-e",
                    "end run",
                    title,
                    body,
                ])
                .status()
                .map_err(|error| error.to_string())?;
            status
                .success()
                .then_some(())
                .ok_or_else(|| format!("osascript exited with {status}"))
        }
    }

    fn center(title: &str, body: &str) {
        let center = UNUserNotificationCenter::currentNotificationCenter();
        // macOS asks once; later calls return the stored answer at once.
        let asked = RcBlock::new(|_granted: Bool, _error: *mut NSError| {});
        center.requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert,
            &asked,
        );
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(title));
        content.setBody(&NSString::from_str(body));
        let id = format!(
            "flexspeak-poke-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_millis())
                .unwrap_or_default()
        );
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &NSString::from_str(&id),
            &content,
            None,
        );
        center.addNotificationRequest_withCompletionHandler(&request, None);
    }
}

#[cfg(windows)]
mod platform {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    /// The toast's sender. Windows shows toasts from unpackaged apps once this ID is registered.
    pub const APP_ID: &str = "io.venator.flexspeak";
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    const SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$key = 'HKCU:\Software\Classes\AppUserModelId\' + $env:FLEXSPEAK_APP_ID
if (-not (Test-Path $key)) {
  New-Item -Path $key -Force | Out-Null
  New-ItemProperty -Path $key -Name DisplayName -Value 'Flexspeak' -PropertyType String -Force | Out-Null
}
[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] | Out-Null
[Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom.XmlDocument, ContentType = WindowsRuntime] | Out-Null
$title = [Security.SecurityElement]::Escape($env:FLEXSPEAK_TITLE)
$body = [Security.SecurityElement]::Escape($env:FLEXSPEAK_BODY)
$xml = New-Object Windows.Data.Xml.Dom.XmlDocument
$xml.LoadXml("<toast><visual><binding template='ToastGeneric'><text>$title</text><text>$body</text></binding></visual><audio silent='true'/></toast>")
$toast = New-Object Windows.UI.Notifications.ToastNotification $xml
[Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier($env:FLEXSPEAK_APP_ID).Show($toast)
"#;

    pub fn show(title: &str, body: &str) -> Result<(), String> {
        let status = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                SCRIPT,
            ])
            .env("FLEXSPEAK_APP_ID", APP_ID)
            .env("FLEXSPEAK_TITLE", title)
            .env("FLEXSPEAK_BODY", body)
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map_err(|error| error.to_string())?;
        status
            .success()
            .then_some(())
            .ok_or_else(|| format!("PowerShell exited with {status}"))
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod platform {
    pub fn show(title: &str, body: &str) -> Result<(), String> {
        let status = std::process::Command::new("notify-send")
            .args(["--app-name=Flexspeak", "--icon=flexspeak", title, body])
            .status()
            .map_err(|error| error.to_string())?;
        status
            .success()
            .then_some(())
            .ok_or_else(|| format!("notify-send exited with {status}"))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn notification_text_drops_control_characters_and_is_bounded() {
        assert_eq!(super::clean("a\nb\u{7}c", 64), "abc");
        assert_eq!(super::clean(&"x".repeat(500), 240).len(), 240);
    }
}
