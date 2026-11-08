//! Where the app was when Restart installed an update: the relaunched build reads it once and
//! reconnects to the same servers and channels with the same microphone state and page.
//!
//! The file sits beside `settings.json`, readable only by its owner, and holds the sessions'
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

/// One server to reconnect to.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Server {
    pub address: String,
    pub password: String,
    pub channel: Option<u64>,
    pub channel_password: String,
    pub muted: bool,
    pub deafened: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Resume {
    /// Seconds since the Unix epoch.
    pub written_at: u64,
    pub page: Page,
    /// Every server that was connected, oldest first.
    pub servers: Vec<Server>,
    /// The address of the one that was active.
    pub active: Option<String>,
    // Older builds wrote one server's fields straight on the file: still read, no longer written.
    #[serde(skip_serializing)]
    server_address: Option<String>,
    #[serde(skip_serializing)]
    server_password: String,
    #[serde(skip_serializing)]
    channel: Option<u64>,
    #[serde(skip_serializing)]
    channel_password: String,
    #[serde(skip_serializing)]
    muted: bool,
    #[serde(skip_serializing)]
    deafened: bool,
}

impl Resume {
    pub fn new(page: Page, servers: Vec<Server>, active: Option<String>) -> Self {
        Self {
            page,
            servers,
            active,
            ..Self::default()
        }
    }

    /// Moves a file from an older build, with its single server, onto the list.
    fn upgraded(mut self) -> Self {
        if self.servers.is_empty()
            && let Some(address) = self.server_address.take()
        {
            self.active = Some(address.clone());
            self.servers.push(Server {
                address,
                password: std::mem::take(&mut self.server_password),
                channel: self.channel.take(),
                channel_password: std::mem::take(&mut self.channel_password),
                muted: self.muted,
                deafened: self.deafened,
            });
        }
        self
    }
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
    (age <= MAX_AGE.as_secs()).then(|| resume.upgraded())
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
            servers: vec![
                Server {
                    address: "one.example".into(),
                    password: "secret".into(),
                    channel: Some(4),
                    muted: false,
                    deafened: true,
                    ..Server::default()
                },
                Server {
                    address: "two.example".into(),
                    channel: Some(9),
                    channel_password: "lock".into(),
                    muted: true,
                    ..Server::default()
                },
            ],
            active: Some("two.example".into()),
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
        assert_eq!(read.page, Page::Statistics);
        assert_eq!(read.servers, resume.servers);
        assert_eq!(read.active.as_deref(), Some("two.example"));
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

    #[test]
    fn a_file_from_an_older_build_reconnects_its_one_server() {
        let dir = std::env::temp_dir().join(format!(
            "flexspeak-resume-old-{}-{}",
            std::process::id(),
            seconds(SystemTime::now())
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = path(&dir.join("settings.json"));
        let now = SystemTime::now();
        let old = serde_json::json!({
            "written_at": seconds(now),
            "page": "Home",
            "server_address": "ts.example",
            "server_password": "secret",
            "channel": 4,
            "channel_password": "lock",
            "muted": false,
            "deafened": true,
        });
        fs::write(&path, old.to_string()).unwrap();
        let read = take(&path, now).unwrap();
        assert_eq!(read.active.as_deref(), Some("ts.example"));
        assert_eq!(
            read.servers,
            [Server {
                address: "ts.example".into(),
                password: "secret".into(),
                channel: Some(4),
                channel_password: "lock".into(),
                muted: false,
                deafened: true,
            }]
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_old_single_server_fields_are_not_written_any_more() {
        let json = serde_json::to_string(&Resume {
            server_address: Some("ts.example".into()),
            ..Resume::default()
        })
        .unwrap();
        assert!(!json.contains("server_address"), "{json}");
    }
}
