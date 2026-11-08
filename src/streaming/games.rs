//! Which open window is a game, so sharing can offer it. There is no system flag for "a game":
//! this reads where the program is installed and, on Mac, what kind of app it declares itself.

/// Installed through a game store. The stores' own launchers live nearby and don't count.
fn in_game_folder(path: &str) -> bool {
    let path = path.replace('\\', "/").to_lowercase();
    if path.contains("/epic games/launcher/") || path.contains("/riot games/riot client/") {
        return false;
    }
    [
        "/steamapps/common/",
        "/epic games/",
        "/riot games/",
        "/gog galaxy/games/",
        "/gog games/",
        "/ubisoft game launcher/games/",
        "/ea games/",
        "/xboxgames/",
        "/rockstar games/",
    ]
    .iter()
    .any(|folder| path.contains(folder))
}

#[cfg(target_os = "macos")]
fn app_is_game(executable: &str) -> bool {
    use objc2_foundation::{NSBundle, NSString};
    if in_game_folder(executable) {
        return true;
    }
    let Some(end) = executable.find(".app/") else {
        return false;
    };
    let path = NSString::from_str(&executable[..end + ".app".len()]);
    NSBundle::bundleWithPath(&path)
        .and_then(|bundle| {
            bundle.objectForInfoDictionaryKey(&NSString::from_str("LSApplicationCategoryType"))
        })
        .and_then(|category| category.downcast::<NSString>().ok())
        // public.app-category.games, and its kinds like public.app-category.action-games.
        .is_some_and(|category| category.to_string().ends_with("games"))
}

/// A test for "is this window id a game", from one look at what's on screen.
#[cfg(target_os = "macos")]
pub fn detector() -> impl Fn(u64) -> bool {
    use objc2_core_foundation::{CFDictionary, CFNumber, CFString};
    use objc2_core_graphics::{
        CGWindowListCopyWindowInfo, CGWindowListOption, kCGWindowNumber, kCGWindowOwnerPID,
    };
    use std::collections::HashSet;
    let number = |window: &CFDictionary, key: &CFString| -> Option<i64> {
        // SAFETY: the window list's values under these keys are CFNumbers, kept alive by the list.
        unsafe {
            let value = window
                .value((key as *const CFString).cast())
                .cast::<CFNumber>();
            value.as_ref()?.as_i64()
        }
    };
    let mut games = HashSet::new();
    let options =
        CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements;
    if let Some(windows) = CGWindowListCopyWindowInfo(options, 0) {
        for index in 0..windows.count() {
            // SAFETY: the index is in range, and each entry of the list is a CFDictionary.
            let window = unsafe { &*windows.value_at_index(index).cast::<CFDictionary>() };
            // SAFETY: Core Graphics exports these keys for the life of the process.
            let (id, pid) = unsafe {
                (
                    number(window, kCGWindowNumber),
                    number(window, kCGWindowOwnerPID),
                )
            };
            let (Some(id), Some(pid)) = (id, pid) else {
                continue;
            };
            let mut path = [0_u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
            // SAFETY: the buffer is writable for the length given.
            let length = unsafe {
                libc::proc_pidpath(pid as i32, path.as_mut_ptr().cast(), path.len() as u32)
            };
            if length > 0 && app_is_game(&String::from_utf8_lossy(&path[..length as usize])) {
                games.insert(id as u64);
            }
        }
    }
    move |id| games.contains(&id)
}

/// A test for "is this window id a game".
#[cfg(target_os = "windows")]
pub fn detector() -> impl Fn(u64) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
    |id| {
        let mut pid = 0_u32;
        // SAFETY: a stale window handle only makes these calls fail; each pointer is valid.
        unsafe {
            GetWindowThreadProcessId(id as _, &mut pid);
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                return false;
            }
            let mut path = [0_u16; 1024];
            let mut length = path.len() as u32;
            let read = QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                path.as_mut_ptr(),
                &mut length,
            );
            CloseHandle(process);
            read != 0 && in_game_folder(&String::from_utf16_lossy(&path[..length as usize]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::in_game_folder;

    #[test]
    fn games_are_told_from_their_launchers_by_where_they_live() {
        assert!(in_game_folder(
            r"C:\Program Files (x86)\Steam\steamapps\common\Counter-Strike Global Offensive\cs2.exe"
        ));
        assert!(in_game_folder(r"D:\Epic Games\Fortnite\FortniteClient.exe"));
        assert!(in_game_folder(r"C:\Riot Games\VALORANT\live\VALORANT.exe"));
        assert!(in_game_folder(
            "/Users/me/Library/Application Support/Steam/steamapps/common/Hades/Hades.app/Hades"
        ));
        assert!(!in_game_folder(
            r"C:\Program Files (x86)\Epic Games\Launcher\Portal\EpicGamesLauncher.exe"
        ));
        assert!(!in_game_folder(r"C:\Riot Games\Riot Client\RiotClient.exe"));
        assert!(!in_game_folder(
            r"C:\Program Files\Mozilla Firefox\firefox.exe"
        ));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_mac_app_says_when_it_is_a_game() {
        assert!(super::app_is_game(
            "/System/Applications/Chess.app/Contents/MacOS/Chess"
        ));
        assert!(!super::app_is_game(
            "/System/Applications/Calculator.app/Contents/MacOS/Calculator"
        ));
    }
}
