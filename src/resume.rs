//! Where the app was when Restart installed an update: the relaunched build reads it once and
//! reconnects to the same server and channel with the same microphone state and page.
//!
//! The file sits beside `settings.json`, readable only by its owner, and holds the session's
//! server and channel passwords so a protected server or channel needs no re-entry. It's deleted
//! as soon as it's read and ignored once it's [`MAX_AGE`] old, so a later ordinary launch never
//! reconnects on its own.

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

pub const MAX_AGE: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Page {
    #[default]
    Home,
    Statistics,
    Settings,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Resume {
    /// Seconds since the Unix epoch.
    pub written_at: u64,
    pub page: Page,
    pub performance_mode: bool,
    /// The saved server the session was on; `None` when there was no session.
    pub server_address: Option<String>,
    pub server_password: String,
    pub channel: Option<u64>,
    pub channel_password: String,
    pub muted: bool,
    pub deafened: bool,
}

pub fn path(settings: &Path) -> PathBuf {
    settings.with_file_name("resume.json")
}

fn seconds(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

pub fn save(path: &Path, resume: &Resume, now: SystemTime) -> std::io::Result<()> {
    let resume = Resume {
        written_at: seconds(now),
        ..resume.clone()
    };
    let _ = fs::remove_file(path);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(&serde_json::to_vec(&resume)?)?;
    file.sync_all()
}

/// Reads and deletes the file. Only a fresh one is returned.
pub fn take(path: &Path, now: SystemTime) -> Option<Resume> {
    let json = fs::read(path).ok()?;
    let _ = fs::remove_file(path);
    let resume: Resume = serde_json::from_slice(&json).ok()?;
    let age = seconds(now).checked_sub(resume.written_at)?;
    (age <= MAX_AGE.as_secs()).then_some(resume)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_file_is_read_once_and_a_stale_one_is_ignored() {
        let dir = std::env::temp_dir().join(format!(
            "flexspeak-resume-{}-{}",
            std::process::id(),
            seconds(SystemTime::now())
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = path(&dir.join("settings.json"));
        assert_eq!(path, dir.join("resume.json"));
        let resume = Resume {
            page: Page::Statistics,
            server_address: Some("ts.example".into()),
            server_password: "secret".into(),
            channel: Some(4),
            muted: false,
            deafened: true,
            ..Resume::default()
        };
        let now = SystemTime::now();
        save(&path, &resume, now).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let read = take(&path, now + Duration::from_secs(5)).unwrap();
        assert_eq!(
            (read.page, read.channel, read.deafened),
            (Page::Statistics, Some(4), true)
        );
        assert_eq!(read.server_password, "secret");
        assert!(!path.exists(), "read once");
        assert_eq!(take(&path, now), None);

        save(&path, &resume, now).unwrap();
        assert_eq!(take(&path, now + MAX_AGE + Duration::from_secs(1)), None);
        assert!(!path.exists(), "a stale file is removed too");
        save(&path, &resume, now).unwrap();
        assert_eq!(
            take(&path, now - Duration::from_secs(60)),
            None,
            "from the future"
        );
        fs::write(&path, "{ not json").unwrap();
        assert_eq!(take(&path, now), None);
        fs::remove_dir_all(dir).unwrap();
    }
}
