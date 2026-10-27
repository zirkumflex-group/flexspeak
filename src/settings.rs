use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::shortcuts::Keybinds;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub server_address: String,
    pub nickname: String,
    pub playback_volume_percent: u8,
    pub keybinds: Keybinds,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            server_address: "ts.zirkumflex.io".into(),
            nickname: "Native Voice".into(),
            playback_volume_percent: 100,
            keybinds: Keybinds::default(),
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<()> {
        if self.server_address.trim().is_empty() {
            bail!("Enter a server address.");
        }
        if self.nickname.trim().is_empty() {
            bail!("Enter a nickname.");
        }
        if self.playback_volume_percent > 100 {
            bail!("Playback volume must be between 0 and 100.");
        }
        self.keybinds.parse()?;
        Ok(())
    }
}

pub fn path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("NATIVE_VOICE_SETTINGS") {
        return Ok(PathBuf::from(path));
    }
    let dirs = directories::ProjectDirs::from("io", "Venator", "TeamSpeakNative")
        .context("Cannot locate the settings directory")?;
    Ok(dirs.data_local_dir().join("settings.json"))
}

pub fn load_or_create(path: &Path) -> Result<Settings> {
    match fs::read_to_string(path) {
        Ok(json) => {
            let settings: Settings = serde_json::from_str(&json)
                .context("Invalid settings.json; the file was not changed")?;
            settings.validate()?;
            Ok(settings)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let settings = Settings::default();
            save(path, &settings)?;
            Ok(settings)
        }
        Err(error) => Err(error).context("Cannot read settings.json"),
    }
}

pub fn save(path: &Path, settings: &Settings) -> Result<()> {
    settings.validate()?;
    let parent = path
        .parent()
        .context("Settings path has no parent directory")?;
    fs::create_dir_all(parent)?;
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let temporary = parent.join(format!(".settings-{}-{nonce}.tmp", std::process::id()));
    let result = (|| -> Result<()> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        let mut json = serde_json::to_vec_pretty(settings)?;
        json.push(b'\n');
        file.write_all(&json)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.context("Cannot save settings.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_are_portable_and_missing_fields_have_defaults() -> Result<()> {
        assert_eq!(
            serde_json::from_str::<Settings>(include_str!("../settings.example.json"))?,
            Settings::default()
        );
        let settings = Settings {
            server_address: "[::1]:9987".into(),
            nickname: "Älex".into(),
            playback_volume_percent: 37,
            keybinds: Keybinds {
                toggle_mute: "F8".into(),
                toggle_deafen: "Ctrl+Alt+D".into(),
            },
        };
        settings.validate()?;
        assert_eq!(
            serde_json::from_str::<Settings>(&serde_json::to_string(&settings)?)?,
            settings
        );
        let minimal: Settings =
            serde_json::from_str(r#"{"server_address":"voice.example:9987","nickname":"Alex"}"#)?;
        assert_eq!(minimal.playback_volume_percent, 100);
        assert_eq!(minimal.keybinds, Keybinds::default());
        assert!(serde_json::from_str::<Settings>(r#"{"nicknmae":"Alex"}"#).is_err());
        assert!(
            Settings {
                playback_volume_percent: 101,
                ..Settings::default()
            }
            .validate()
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn save_replaces_complete_settings_and_failed_load_preserves_the_file() -> Result<()> {
        let root = std::env::temp_dir().join(format!(
            "nativevoice-settings-test-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        let path = root.join("settings.json");
        let mut settings = load_or_create(&path)?;
        settings.nickname = "Saved name".into();
        save(&path, &settings)?;
        assert_eq!(load_or_create(&path)?, settings);
        let old = fs::read(&path)?;
        settings.playback_volume_percent = 200;
        assert!(save(&path, &settings).is_err());
        assert_eq!(fs::read(&path)?, old);
        fs::write(&path, "{ malformed")?;
        assert!(load_or_create(&path).is_err());
        assert_eq!(fs::read_to_string(&path)?, "{ malformed");
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
