use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::shortcuts::Keybinds;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    /// Saved servers, in the order the home screen lists them. One is connected at a time.
    #[serde(default)]
    pub servers: Vec<Server>,
    /// The single server of files written before the list. Loading moves it to the front of
    /// `servers`, and it is never written again.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub server_address: String,
    pub nickname: String,
    pub playback_volume_percent: u8,
    pub audio_processing: crate::audio_processing::Settings,
    pub transmission: crate::transmission::Settings,
    pub keybinds: Keybinds,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_device_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_device_id: Option<String>,
    /// The footer shows only the connection status and ping, as it does unless expanded.
    #[serde(skip_serializing_if = "is_true")]
    pub footer_hidden: bool,
    /// New releases download in the background; installing still waits for Restart.
    #[serde(skip_serializing_if = "is_true")]
    pub auto_download_updates: bool,
    /// Saved straight away when changed, like `footer_hidden`.
    #[serde(skip_serializing_if = "Appearance::is_default")]
    pub appearance: Appearance,
    /// What you've set for other people, by identity. Only changes what you hear.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub user_audio: BTreeMap<String, UserAudio>,
    /// Each streamer's stream volume in percent, by identity; streams at 100% stay out.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub stream_volumes: BTreeMap<String, u16>,
    /// Whether the app plays its short event sounds at all.
    #[serde(skip_serializing_if = "is_true")]
    pub sound_cues: bool,
    #[serde(skip_serializing_if = "crate::sounds::Settings::is_default")]
    pub sounds: crate::sounds::Settings,
    /// The quality, access and sound screen sharing last used.
    #[serde(skip_serializing_if = "crate::streaming::publishing::ShareDefaults::is_default")]
    pub share: crate::streaming::publishing::ShareDefaults,
    /// Settings > Streaming. Saved straight away when changed, like `footer_hidden`.
    #[serde(skip_serializing_if = "Streaming::is_default")]
    pub streaming: Streaming,
    /// Where the other streams go and whether stream details show.
    #[serde(skip_serializing_if = "StreamView::is_default")]
    pub stream_view: StreamView,
    /// Kept as written, so one bad entry can't stop the settings from loading.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub custom_themes: Vec<serde_json::Value>,
    /// A release running from Downloads or a disk image no longer offers to install itself.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub skip_install_prompt: bool,
    /// What the window's close button does while a server is connected.
    #[serde(skip_serializing_if = "CloseButton::is_default")]
    pub close_button: CloseButton,
    /// Where the window was and how big when it last closed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<WindowPlace>,
    /// The server log's height under the channel list, and whether it is folded away.
    #[serde(skip_serializing_if = "LogPane::is_default")]
    pub log_pane: LogPane,
    /// The sidebar's width in points, when dragged away from the usual 260.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sidebar_width: Option<u16>,
    /// The channels folded away in the tree, by server address, as they were left.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub collapsed_channels: BTreeMap<String, BTreeSet<u64>>,
    /// Settings > Chat. Saved straight away when changed, like `footer_hidden`.
    #[serde(skip_serializing_if = "Chat::is_default")]
    pub chat: Chat,
    /// The Monster collection server (Settings > WIP). Without one the page keeps its cans on
    /// this computer only and never goes online.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monster_server: Option<String>,
    /// The session with that server, sealed with DPAPI (`secret::protect`). Windows only:
    /// other systems keep it in `monster-session` beside this file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monster_session: Option<String>,
}

/// What the window's close button does while connected (Settings > Appearance > Window).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CloseButton {
    /// A dialog offers to hide to the tray or quit.
    #[default]
    Ask,
    /// The window hides and every server stays connected.
    HideToTray,
    /// The app disconnects and quits, as it does when nothing is connected.
    Quit,
}

impl CloseButton {
    pub const ALL: [Self; 3] = [Self::Ask, Self::HideToTray, Self::Quit];

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Ask => "Ask",
            Self::HideToTray => "Hide to Tray",
            Self::Quit => "Quit",
        }
    }
}

/// The window's outer position and inner size in physical pixels, and whether it was maximized.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(deny_unknown_fields)]
pub struct WindowPlace {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub maximized: bool,
}

/// How text chat shows, and what happens while its column is hidden (Settings > Chat).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(default, deny_unknown_fields)]
pub struct Chat {
    pub layout: crate::chat::Layout,
    /// The column is tucked behind the window's right edge.
    pub hidden: bool,
    /// A new message pops up at the top right while the column is hidden.
    pub popups: bool,
    /// Only private messages pop up; channel and server ones just light the dot.
    pub popups_private_only: bool,
    /// The column hides itself while a stream is being watched.
    pub hide_while_watching: bool,
    /// How Tabs fits many conversations into its bar.
    pub tab_bar: crate::chat::TabBar,
    /// Messages and pokes are kept in `chat.db` beside this file and come back after a restart.
    pub history: bool,
}

impl Default for Chat {
    fn default() -> Self {
        Self {
            layout: crate::chat::Layout::Tabs,
            hidden: false,
            popups: true,
            popups_private_only: false,
            hide_while_watching: false,
            tab_bar: crate::chat::TabBar::Scroll,
            history: true,
        }
    }
}

impl Chat {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// How screen sharing encodes and captures (Settings > Streaming). Both apply to the next
/// share, and both are Windows only.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Streaming {
    /// Offer AV1 from the NVIDIA hardware encoder before VP8.
    pub hardware_av1: bool,
    /// Experimental capture that never waits for the GPU (`streaming/wgc.rs`).
    pub fast_capture: bool,
}

impl Streaming {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// How watched streams are laid out. Saved straight away when changed, like `footer_hidden`.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct StreamView {
    /// The other streams line up beside the large one rather than below it.
    pub beside: bool,
    /// Codec, resolution, frame rate and transport show over each stream.
    pub details: bool,
}

impl StreamView {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// How much of the sidebar the server log takes. Saved straight away, like `footer_hidden`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct LogPane {
    /// The height of its list, in points. The sidebar caps it to leave the channels room.
    pub height: u16,
    /// Only its heading shows.
    pub collapsed: bool,
}

impl Default for LogPane {
    fn default() -> Self {
        Self {
            height: 144,
            collapsed: false,
        }
    }
}

impl LogPane {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// A saved server: an optional name to show, and the address to connect to.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Server {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub address: String,
    /// Connect when Flexspeak opens.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub auto_connect: bool,
    /// The password, when you chose to remember it, sealed by `crate::secret` (DPAPI on
    /// Windows); empty when it isn't remembered.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub password: String,
}

impl Server {
    pub fn new(address: impl Into<String>) -> Self {
        Self {
            name: String::new(),
            address: address.into(),
            auto_connect: false,
            password: String::new(),
        }
    }

    /// The name, or the address for a server without one.
    pub fn title(&self) -> &str {
        if self.name.trim().is_empty() {
            &self.address
        } else {
            &self.name
        }
    }
}

/// Whether the window follows the system's light or dark appearance or keeps one.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ColorScheme {
    #[default]
    System,
    Light,
    Dark,
}

impl ColorScheme {
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    pub fn title(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }
}

/// The supplied colour themes. Each has a light and a dark palette.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeName {
    #[default]
    Flexspeak,
    Grove,
    Ocean,
    Ember,
    Iris,
}

impl ThemeName {
    pub const ALL: [Self; 5] = [
        Self::Flexspeak,
        Self::Grove,
        Self::Ocean,
        Self::Ember,
        Self::Iris,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Flexspeak => "Flexspeak",
            Self::Grove => "Grove",
            Self::Ocean => "Ocean",
            Self::Ember => "Ember",
            Self::Iris => "Iris",
        }
    }
}

/// The colour scheme, and the theme used while the window is light and while it is dark.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Appearance {
    pub scheme: ColorScheme,
    pub light_theme: ThemeName,
    pub dark_theme: ThemeName,
    /// A theme from the More Themes section, used instead of the built-in one above.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub light_theme_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dark_theme_id: Option<String>,
    /// Keeps the theme chosen here on an Omarchy desktop, instead of following Omarchy's.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub ignore_omarchy: bool,
    /// Written only when on, so settings stay readable by builds without this option.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub disable_animations: bool,
    /// The notification-area or menu-bar icon is on unless this is set; written only when set.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub hide_tray_icon: bool,
}

impl Appearance {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Whether the window is dark, given whether the system currently is.
    pub fn is_dark(&self, system_dark: bool) -> bool {
        match self.scheme {
            ColorScheme::System => system_dark,
            ColorScheme::Light => false,
            ColorScheme::Dark => true,
        }
    }

    /// The More Themes entry chosen for that appearance, if any.
    pub fn extra_theme(&self, dark: bool) -> Option<&str> {
        if dark {
            self.dark_theme_id.as_deref()
        } else {
            self.light_theme_id.as_deref()
        }
    }

    /// The theme in use for that appearance.
    pub fn theme(&self, dark: bool) -> ThemeName {
        if dark {
            self.dark_theme
        } else {
            self.light_theme
        }
    }
}

/// One person's playback on this computer.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct UserAudio {
    pub volume_percent: u16,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub muted: bool,
}

impl Default for UserAudio {
    fn default() -> Self {
        Self {
            volume_percent: 100,
            muted: false,
        }
    }
}

impl UserAudio {
    pub const MAX_PERCENT: u16 = 200;

    pub fn gain(&self) -> f32 {
        if self.muted {
            0.0
        } else {
            f32::from(self.volume_percent.min(Self::MAX_PERCENT)) / 100.0
        }
    }
}

fn is_true(value: &bool) -> bool {
    *value
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            servers: vec![Server::new("ts.zirkumflex.io")],
            server_address: String::new(),
            nickname: "Flexspeak".into(),
            playback_volume_percent: 100,
            audio_processing: crate::audio_processing::Settings::default(),
            transmission: crate::transmission::Settings::default(),
            keybinds: Keybinds::default(),
            input_device_id: None,
            output_device_id: None,
            footer_hidden: true,
            log_pane: LogPane::default(),
            sidebar_width: None,
            collapsed_channels: BTreeMap::new(),
            auto_download_updates: true,
            appearance: Appearance::default(),
            user_audio: BTreeMap::new(),
            stream_volumes: BTreeMap::new(),
            sound_cues: true,
            sounds: crate::sounds::Settings::default(),
            share: Default::default(),
            streaming: Streaming::default(),
            stream_view: StreamView::default(),
            custom_themes: Vec::new(),
            skip_install_prompt: false,
            close_button: CloseButton::default(),
            window: None,
            chat: Chat::default(),
            monster_server: None,
            monster_session: None,
        }
    }
}

impl Settings {
    /// Moves a pre-list file's `server_address` to the front of `servers`.
    pub fn migrate(mut self) -> Self {
        let legacy = std::mem::take(&mut self.server_address);
        let legacy = legacy.trim();
        if !legacy.is_empty() && !self.servers.iter().any(|server| server.address == legacy) {
            self.servers.insert(0, Server::new(legacy));
        }
        self
    }

    pub fn validate(&self) -> Result<()> {
        if self.servers.is_empty() {
            bail!("Add a server.");
        }
        if self
            .servers
            .iter()
            .any(|server| server.address.trim().is_empty())
        {
            bail!("Enter an address for every server.");
        }
        if self.nickname.trim().is_empty() {
            bail!("Enter a nickname.");
        }
        if self.playback_volume_percent > 100 {
            bail!("Playback volume must be between 0 and 100.");
        }
        self.keybinds.parse()?;
        self.transmission.validate()?;
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

pub fn load(path: &Path) -> Result<Option<Settings>> {
    match fs::read_to_string(path) {
        Ok(json) => {
            let settings = serde_json::from_str::<Settings>(&json)
                .context("Invalid settings.json; the file was not changed")?
                .migrate();
            settings.validate()?;
            Ok(Some(settings))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
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
    fn a_remembered_password_round_trips_sealed_and_is_left_out_when_not_remembered() {
        let plain = serde_json::to_string(&Server::new("one.example")).unwrap();
        assert!(!plain.contains("password"), "{plain}");
        let Some(sealed) = crate::secret::protect("server-secret") else {
            return;
        };
        let server = Server {
            password: sealed,
            ..Server::new("one.example")
        };
        let json = serde_json::to_string(&server).unwrap();
        assert!(!json.contains("server-secret"), "{json}");
        let back: Server = serde_json::from_str(&json).unwrap();
        assert_eq!(back, server);
        assert_eq!(
            crate::secret::unprotect(&back.password).as_deref(),
            Some("server-secret")
        );
    }

    #[test]
    fn streaming_options_round_trip_and_stay_out_of_the_file_when_off() {
        let off = serde_json::to_string(&Settings::default()).unwrap();
        assert!(!off.contains("streaming"), "{off}");
        let settings = Settings {
            streaming: Streaming {
                hardware_av1: true,
                fast_capture: true,
            },
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        assert!(json.contains("\"hardware_av1\":true"), "{json}");
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.streaming, settings.streaming);
    }

    #[test]
    fn animations_default_on_and_the_option_is_only_written_when_used() {
        let appearance = Appearance {
            scheme: ColorScheme::Light,
            ..Appearance::default()
        };
        let json = serde_json::to_string(&appearance).unwrap();
        assert!(!json.contains("disable_animations"));
        let read: Appearance = serde_json::from_str(&json).unwrap();
        assert!(!read.disable_animations);
    }

    #[test]
    fn settings_are_portable_and_missing_fields_have_defaults() -> Result<()> {
        assert_eq!(
            serde_json::from_str::<Settings>(include_str!("../settings.example.json"))?,
            Settings::default()
        );
        let settings = Settings {
            servers: vec![
                Server::new("[::1]:9987"),
                Server {
                    name: "Friends".into(),
                    address: "voice.example".into(),
                    ..Default::default()
                },
            ],
            nickname: "Älex".into(),
            playback_volume_percent: 37,
            audio_processing: crate::audio_processing::Settings {
                noise_suppression: true,
                automatic_gain: true,
                echo_cancellation: true,
                typing_suppression: true,
                ..crate::audio_processing::Settings::default()
            },
            keybinds: Keybinds {
                toggle_mute: "F8".into(),
                toggle_deafen: "Ctrl+Alt+D".into(),
                ..Keybinds::default()
            },
            transmission: crate::transmission::Settings {
                mode: crate::transmission::Mode::VoiceActivation,
                voice_threshold_db: -35,
                ptt_release_ms: 100,
            },
            input_device_id: Some("input-id".into()),
            output_device_id: Some("output-id".into()),
            footer_hidden: true,
            auto_download_updates: false,
            appearance: Appearance {
                scheme: ColorScheme::Light,
                light_theme: ThemeName::Grove,
                dark_theme: ThemeName::Iris,
                disable_animations: true,
                ..Appearance::default()
            },
            ..Settings::default()
        };
        settings.validate()?;
        assert_eq!(
            serde_json::from_str::<Settings>(&serde_json::to_string(&settings)?)?,
            settings
        );
        // A file from before the server list keeps its server, now the only one in the list.
        let minimal = serde_json::from_str::<Settings>(
            r#"{"server_address":"voice.example:9987","nickname":"Alex"}"#,
        )?
        .migrate();
        assert_eq!(minimal.servers, [Server::new("voice.example:9987")]);
        assert!(minimal.server_address.is_empty());
        assert!(!serde_json::to_string(&minimal)?.contains("server_address"));
        minimal.validate()?;
        assert!(
            Settings {
                servers: Vec::new(),
                ..Settings::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            Settings {
                servers: vec![Server::new(" ")],
                ..Settings::default()
            }
            .validate()
            .is_err()
        );
        assert_eq!(minimal.playback_volume_percent, 100);
        assert_eq!(
            minimal.transmission,
            crate::transmission::Settings::default()
        );
        let old_processing: Settings =
            serde_json::from_str(r#"{"audio_processing":{"noise_suppression":true}}"#)?;
        assert_eq!(
            old_processing.audio_processing.suppression_level,
            crate::audio_processing::SuppressionLevel::Moderate
        );
        let invalid: Settings = serde_json::from_str(r#"{"transmission":{"ptt_release_ms":501}}"#)?;
        assert!(invalid.validate().is_err());
        assert_eq!(
            minimal.audio_processing,
            crate::audio_processing::Settings::default()
        );
        assert!(
            serde_json::from_str::<Settings>(r#"{"audio_processing":{"noise_suppresion":true}}"#)
                .is_err()
        );
        assert_eq!(minimal.keybinds, Keybinds::default());
        assert!(
            minimal.auto_download_updates,
            "updates download unless turned off"
        );
        assert!(
            !serde_json::to_string(&Settings::default())?.contains("auto_download_updates"),
            "the default stays out of the file"
        );
        assert!(serde_json::to_string(&settings)?.contains(r#""auto_download_updates":false"#));
        assert!(serde_json::from_str::<Settings>(r#"{"nicknmae":"Alex"}"#).is_err());
        let themed: Settings =
            serde_json::from_str(r#"{"appearance":{"scheme":"dark","dark_theme":"ember"}}"#)?;
        assert_eq!(themed.appearance.light_theme, ThemeName::Flexspeak);
        assert_eq!(themed.appearance.theme(true), ThemeName::Ember);
        assert!(
            !serde_json::to_string(&Settings::default())?.contains("appearance"),
            "the default appearance stays out of the file"
        );
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
            "flexspeak-settings-test-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        let path = root.join("settings.json");
        assert_eq!(load(&path)?, None);
        assert!(
            !path.exists(),
            "Starting setup must not create a completed config"
        );
        let mut settings = Settings {
            nickname: "Saved name".into(),
            ..Settings::default()
        };
        save(&path, &settings)?;
        assert_eq!(load(&path)?, Some(settings.clone()));
        let old = fs::read(&path)?;
        settings.playback_volume_percent = 200;
        assert!(save(&path, &settings).is_err());
        assert_eq!(fs::read(&path)?, old);
        fs::write(&path, "{ malformed")?;
        assert!(load(&path).is_err());
        assert_eq!(fs::read_to_string(&path)?, "{ malformed");
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
