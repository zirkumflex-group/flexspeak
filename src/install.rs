//! Installing Flexspeak where it stays: a release opened from Downloads, a mounted disk image or
//! an unpacked ZIP copies itself to the platform's usual place for apps, and runs from there.
//! In-app updates then replace that installed copy, so they survive the download folder being
//! cleaned out, and the app appears in Start, Launchpad or the application launcher.
//!
//! | Platform | Installed copy | Also |
//! | --- | --- | --- |
//! | Windows | `%LOCALAPPDATA%\Programs\Flexspeak\Flexspeak.exe` | Start menu shortcut, an entry in Installed apps (uninstall runs `--uninstall`) |
//! | macOS | `/Applications/Flexspeak.app`, or `~/Applications` when that isn't writable | The copy loses the download quarantine, as when dragged there |
//! | Linux tarball | `~/.local/bin/flexspeak` | A launcher entry in `~/.local/share/applications` |
//!
//! The Arch package and Homebrew already install system-wide and are left alone. Settings and
//! the identity live in the per-user data directory either way, so nothing is lost.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

/// Where this build runs from, and where installing would put it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    /// The executable, or on macOS the `.app`, that is running.
    pub from: PathBuf,
    /// What installing creates or replaces.
    pub to: PathBuf,
}

/// Directories the plan depends on, read from the environment for the running process.
#[derive(Debug, Clone, Default)]
pub struct Places {
    pub home: Option<PathBuf>,
    /// `%LOCALAPPDATA%` on Windows.
    pub local_data: Option<PathBuf>,
    /// Whether `/Applications` accepts new apps from this user.
    pub applications_writable: bool,
}

impl Places {
    fn current() -> Self {
        let home =
            std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
        Self {
            home,
            local_data: std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
            applications_writable: writable(Path::new("/Applications")),
        }
    }
}

fn writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".flexspeak-write-test-{}", std::process::id()));
    let ok = fs::write(&probe, b"").is_ok();
    let _ = fs::remove_file(&probe);
    ok
}

/// The install offer for this process, if it runs a release from somewhere temporary.
/// Development builds (no build number) never offer to install.
pub fn offer() -> Option<Offer> {
    crate::updater::Build::running()?;
    let exe = std::env::current_exe().ok()?;
    plan(std::env::consts::OS, &exe, &Places::current())
}

/// Decides whether `exe` is installed, and where it would go otherwise.
pub fn plan(os: &str, exe: &Path, places: &Places) -> Option<Offer> {
    match os {
        "windows" => {
            let to = places
                .local_data
                .as_ref()?
                .join("Programs")
                .join("Flexspeak")
                .join("Flexspeak.exe");
            let same = exe.to_string_lossy().to_lowercase() == to.to_string_lossy().to_lowercase();
            (!same).then(|| Offer {
                from: exe.to_path_buf(),
                to,
            })
        }
        "macos" => {
            let bundle = exe
                .ancestors()
                .find(|path| path.extension().is_some_and(|extension| extension == "app"))?;
            let user_apps = places.home.as_ref().map(|home| home.join("Applications"));
            let parent = bundle.parent()?;
            let translocated = bundle
                .components()
                .any(|part| part.as_os_str() == "AppTranslocation");
            let installed = !translocated
                && (parent == Path::new("/Applications")
                    || user_apps.as_deref() == Some(parent)
                    // Homebrew and admins may use a subfolder such as /Applications/Utilities.
                    || parent.starts_with("/Applications/"));
            if installed {
                return None;
            }
            let folder = if places.applications_writable {
                PathBuf::from("/Applications")
            } else {
                user_apps?
            };
            Some(Offer {
                from: bundle.to_path_buf(),
                to: folder.join("Flexspeak.app"),
            })
        }
        "linux" => {
            let to = places.home.as_ref()?.join(".local/bin/flexspeak");
            let system = exe.starts_with("/usr/") || exe.starts_with("/opt/");
            (!system && exe != to).then(|| Offer {
                from: exe.to_path_buf(),
                to,
            })
        }
        _ => None,
    }
}

/// Copies this build into place and registers it, then starts a helper that opens the
/// installed copy once this process has exited. The running copy is left where it was.
pub fn install(offer: &Offer) -> Result<(), String> {
    let pid = std::process::id();
    let parent = offer
        .to
        .parent()
        .ok_or("The install location has no parent folder.")?;
    let denied = |error: String| {
        format!(
            "Cannot install to {}: {error}. Check that you can write there.",
            parent.display()
        )
    };
    fs::create_dir_all(parent).map_err(|error| denied(error.to_string()))?;
    if cfg!(target_os = "macos") {
        install_app(offer, parent).map_err(denied)?;
        crate::updater::relaunch_unix(
            "while kill -0 \"$1\" 2>/dev/null; do sleep 0.2; done; exec /usr/bin/open \"$2\"",
            &[pid.to_string().as_ref(), offer.to.as_os_str()],
        )
    } else {
        replace_file(&offer.from, &offer.to).map_err(denied)?;
        if cfg!(windows) {
            if let Err(error) = register_windows(&offer.to) {
                tracing::warn!("Installed, but the Start menu entry is missing: {error}");
            }
            crate::updater::relaunch_windows(pid, &offer.to)
        } else {
            if let Err(error) = register_linux(&offer.to) {
                tracing::warn!("Installed, but the launcher entry is missing: {error}");
            }
            crate::updater::relaunch_unix(
                "while kill -0 \"$1\" 2>/dev/null; do sleep 0.2; done; exec \"$2\"",
                &[pid.to_string().as_ref(), offer.to.as_os_str()],
            )
        }
    }
}

/// Copies `from` beside `to` and renames it over, so a failed copy leaves `to` as it was.
fn replace_file(from: &Path, to: &Path) -> Result<(), String> {
    let incoming = to.with_extension("install");
    fs::copy(from, &incoming).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&incoming, fs::Permissions::from_mode(0o755))
            .map_err(|error| error.to_string())?;
    }
    if cfg!(windows) && to.exists() {
        // A running copy can't be overwritten on Windows, but it can be renamed aside.
        let old = to.with_extension("exe.old");
        let _ = fs::remove_file(&old);
        if let Err(error) = fs::rename(to, &old) {
            let _ = fs::remove_file(&incoming);
            return Err(format!("{error}. Quit the installed Flexspeak first"));
        }
    }
    fs::rename(&incoming, to).map_err(|error| {
        let _ = fs::remove_file(&incoming);
        error.to_string()
    })
}

/// Copies the bundle with `ditto`, checks it, and swaps it in for any older installed copy.
fn install_app(offer: &Offer, parent: &Path) -> Result<(), String> {
    let incoming = parent.join(".Flexspeak-install.app");
    let old = parent.join(format!(".Flexspeak-replaced-{}.app", std::process::id()));
    let _ = fs::remove_dir_all(&incoming);
    crate::updater::run(
        Command::new("/usr/bin/ditto")
            .arg(&offer.from)
            .arg(&incoming),
        "copy failed",
    )?;
    // The user already opened this build; the installed copy shouldn't be translocated again.
    let _ = Command::new("/usr/bin/xattr")
        .args(["-dr", "com.apple.quarantine"])
        .arg(&incoming)
        .status();
    if let Err(error) = crate::updater::verify_app(&incoming) {
        let _ = fs::remove_dir_all(&incoming);
        return Err(error);
    }
    let replacing = offer.to.exists();
    if replacing && let Err(error) = fs::rename(&offer.to, &old) {
        let _ = fs::remove_dir_all(&incoming);
        return Err(error.to_string());
    }
    if let Err(error) = fs::rename(&incoming, &offer.to) {
        if replacing {
            let _ = fs::rename(&old, &offer.to);
        }
        let _ = fs::remove_dir_all(&incoming);
        return Err(error.to_string());
    }
    if replacing {
        let _ = fs::remove_dir_all(&old);
    }
    Ok(())
}

/// The Start menu shortcut and the Installed apps entry, through Windows PowerShell. Paths
/// travel in environment variables, so no quoting can break the script.
fn register_windows(exe: &Path) -> Result<(), String> {
    const SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$exe = $env:FLEXSPEAK_EXE
$programs = [Environment]::GetFolderPath('Programs')
$shell = New-Object -ComObject WScript.Shell
$link = $shell.CreateShortcut((Join-Path $programs 'Flexspeak.lnk'))
$link.TargetPath = $exe
$link.WorkingDirectory = Split-Path $exe
$link.IconLocation = "$exe,0"
$link.Save()
$key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Flexspeak'
New-Item -Path $key -Force | Out-Null
$values = @{
  DisplayName = 'Flexspeak'
  DisplayIcon = "$exe,0"
  Publisher = 'Zirkumflex'
  InstallLocation = (Split-Path $exe)
  UninstallString = "`"$exe`" --uninstall"
  QuietUninstallString = "`"$exe`" --uninstall"
}
foreach ($name in $values.Keys) { New-ItemProperty -Path $key -Name $name -Value $values[$name] -PropertyType String -Force | Out-Null }
New-ItemProperty -Path $key -Name NoModify -Value 1 -PropertyType DWord -Force | Out-Null
New-ItemProperty -Path $key -Name NoRepair -Value 1 -PropertyType DWord -Force | Out-Null
"#;
    powershell(SCRIPT, exe)
}

fn powershell(script: &str, exe: &Path) -> Result<(), String> {
    let mut command = Command::new("powershell.exe");
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .env("FLEXSPEAK_EXE", exe)
        .env("FLEXSPEAK_PID", std::process::id().to_string());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    crate::updater::run(&mut command, "PowerShell failed")
}

/// A launcher entry for desktops that follow the XDG layout.
fn register_linux(exe: &Path) -> Result<(), String> {
    let home = Places::current().home.ok_or("No home directory")?;
    let applications = home.join(".local/share/applications");
    fs::create_dir_all(&applications).map_err(|error| error.to_string())?;
    fs::write(applications.join("flexspeak.desktop"), desktop_entry(exe))
        .map_err(|error| error.to_string())
}

pub fn desktop_entry(exe: &Path) -> String {
    // Spaces in a path need quoting in Exec; quotes and backslashes are escaped inside.
    let path = exe
        .display()
        .to_string()
        .replace('\\', "\\\\\\\\")
        .replace('"', "\\\\\"");
    format!(
        "[Desktop Entry]\nType=Application\nName=Flexspeak\nComment=TeamSpeak voice client\nExec=\"{path}\"\nIcon=flexspeak\nTerminal=false\nCategories=Network;Chat;AudioVideo;\nStartupWMClass=flexspeak\n"
    )
}

/// `--uninstall`, as Windows' Installed apps runs it: removes the shortcut and the entry, then
/// the program folder once this process has exited. Settings and the identity stay.
pub fn uninstall() -> anyhow::Result<()> {
    if !cfg!(windows) {
        anyhow::bail!(
            "Uninstall Flexspeak with your package manager, or delete the app; settings stay."
        );
    }
    const SCRIPT: &str = r#"
$ErrorActionPreference = 'SilentlyContinue'
Remove-Item -Path (Join-Path ([Environment]::GetFolderPath('Programs')) 'Flexspeak.lnk') -Force
Remove-Item -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Flexspeak' -Recurse -Force
Remove-Item -Path 'HKCU:\Software\Classes\AppUserModelId\io.venator.flexspeak' -Recurse -Force
$folder = Split-Path $env:FLEXSPEAK_EXE
Start-Process -WindowStyle Hidden -FilePath powershell.exe -ArgumentList @('-NoProfile', '-Command', "Wait-Process -Id $env:FLEXSPEAK_PID; Remove-Item -LiteralPath '$($folder -replace "'", "''")' -Recurse -Force")
"#;
    let exe = std::env::current_exe()?;
    powershell(SCRIPT, &exe).map_err(anyhow::Error::msg)?;
    println!("Flexspeak was removed. Your settings and identity were kept.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn places() -> Places {
        Places {
            home: Some(PathBuf::from("/Users/alex")),
            local_data: Some(PathBuf::from(r"C:\Users\alex\AppData\Local")),
            applications_writable: true,
        }
    }

    #[test]
    fn windows_installs_per_user_and_recognises_its_copy_in_any_case() {
        let installed = PathBuf::from(r"C:\Users\alex\AppData\Local")
            .join("Programs")
            .join("Flexspeak")
            .join("Flexspeak.exe");
        let downloaded = PathBuf::from(r"C:\Users\alex\Downloads\Flexspeak.exe");
        let offer = plan("windows", &downloaded, &places()).unwrap();
        assert_eq!(offer.to, installed);
        assert_eq!(offer.from, downloaded);
        assert!(plan("windows", &installed, &places()).is_none());
        let shouting = PathBuf::from(installed.to_string_lossy().to_uppercase());
        assert!(plan("windows", &shouting, &places()).is_none());
    }

    #[test]
    fn macos_installs_apps_outside_applications_and_falls_back_to_the_home_folder() {
        let exe = |bundle: &str| PathBuf::from(bundle).join("Contents/MacOS/flexspeak");
        assert!(plan("macos", &exe("/Applications/Flexspeak.app"), &places()).is_none());
        assert!(
            plan(
                "macos",
                &exe("/Users/alex/Applications/Flexspeak.app"),
                &places()
            )
            .is_none()
        );
        let mounted = plan("macos", &exe("/Volumes/Flexspeak/Flexspeak.app"), &places()).unwrap();
        assert_eq!(
            mounted.from,
            PathBuf::from("/Volumes/Flexspeak/Flexspeak.app")
        );
        assert_eq!(mounted.to, PathBuf::from("/Applications/Flexspeak.app"));
        let translocated = exe("/private/var/folders/x/AppTranslocation/ABC/d/Flexspeak.app");
        assert!(plan("macos", &translocated, &places()).is_some());
        let locked = Places {
            applications_writable: false,
            ..places()
        };
        let offer = plan(
            "macos",
            &exe("/Users/alex/Downloads/Flexspeak.app"),
            &locked,
        )
        .unwrap();
        assert_eq!(
            offer.to,
            PathBuf::from("/Users/alex/Applications/Flexspeak.app")
        );
        assert!(plan("macos", Path::new("/tmp/flexspeak"), &places()).is_none());
    }

    #[test]
    fn linux_leaves_packages_alone_and_installs_tarballs_for_the_user() {
        assert!(plan("linux", Path::new("/usr/bin/flexspeak"), &places()).is_none());
        assert!(plan("linux", Path::new("/opt/flexspeak/flexspeak"), &places()).is_none());
        let offer = plan(
            "linux",
            Path::new("/Users/alex/Downloads/flexspeak"),
            &places(),
        )
        .unwrap();
        assert_eq!(offer.to, PathBuf::from("/Users/alex/.local/bin/flexspeak"));
        assert!(plan("linux", &offer.to, &places()).is_none());
    }

    #[test]
    fn the_launcher_entry_quotes_its_path() {
        let entry = desktop_entry(Path::new("/home/a b/.local/bin/flexspeak"));
        assert!(entry.contains("Exec=\"/home/a b/.local/bin/flexspeak\"\n"));
        assert!(entry.starts_with("[Desktop Entry]\n"));
    }

    #[test]
    fn replacing_keeps_the_old_file_until_the_copy_is_complete() {
        let dir = std::env::temp_dir().join(format!("flexspeak-install-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let (from, to) = (dir.join("download"), dir.join("installed"));
        fs::write(&from, b"new").unwrap();
        fs::write(&to, b"old").unwrap();
        replace_file(&from, &to).unwrap();
        assert_eq!(fs::read(&to).unwrap(), b"new");
        assert!(replace_file(&dir.join("missing"), &to).is_err());
        assert_eq!(
            fs::read(&to).unwrap(),
            b"new",
            "A failed copy changes nothing"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
