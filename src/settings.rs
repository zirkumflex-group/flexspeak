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
    /// The footer shows only the connection status and ping; it starts expanded.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
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
    /// The people whose streams are joined as soon as they go live in your channel, by identity.
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub auto_watch: BTreeSet<String>,
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
    /// The collection server typed in Settings before the address was built into the app
    /// (`FLEXSPEAK_COLLECTION_SERVER`). Read so older files still load, and so a session from
    /// another server is dropped; never written again.
    #[serde(skip_serializing)]
    pub monster_server: Option<String>,
    /// The session with that server, sealed with DPAPI (`secret::protect`). Windows only:
    /// other systems keep it in `monster-session` beside this file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monster_session: Option<String>,
    /// The old switch for settings sync, which is now always on while signed in. Read so older
    /// files still load; never written again.
    #[serde(rename = "settings_sync", skip_serializing)]
    pub retired_settings_sync: Option<bool>,
    /// The version of the account's settings this computer last saw; 0 for none. Forgotten on
    /// signing out, so the next account starts by comparing.
    #[serde(skip_serializing_if = "is_zero")]
    pub settings_sync_version: i64,
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
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Streaming {
    /// Offer AV1 from the NVIDIA hardware encoder before VP8.
    pub hardware_av1: bool,
    /// Capture that never waits for the GPU (`streaming/wgc.rs`). On unless turned off; a
    /// share it cannot serve uses the standard capture.
    pub fast_capture: bool,
}

impl Default for Streaming {
    fn default() -> Self {
        Self {
            hardware_av1: false,
            fast_capture: true,
        }
    }
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
    /// The channel to join on connecting, instead of the one the server puts you in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_channel: Option<DefaultChannel>,
}

/// A saved server's channel to join on connecting: the server's ID for it, and the name it
/// had when it was chosen, to show while disconnected.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct DefaultChannel {
    pub id: u64,
    pub name: String,
}

impl Server {
    pub fn new(address: impl Into<String>) -> Self {
        Self {
            name: String::new(),
            address: address.into(),
            auto_connect: false,
            password: String::new(),
            default_channel: None,
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

fn is_zero(value: &i64) -> bool {
    *value == 0
}

/// The settings that travel between computers through the collection server: an allow-list,
/// so a setting added to [`Settings`] later stays on its computer until someone adds it here.
///
/// Every field is optional and read on its own: one this version doesn't know is ignored, one
/// missing or unreadable keeps this computer's value. So older and newer versions of the app
/// can share one copy. Devices, shortcuts, the window and everything bound to one computer stay
/// out, and so does every server password.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Synced {
    #[serde(
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub servers: Option<Vec<SyncedServer>>,
    #[serde(
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub nickname: Option<String>,
    #[serde(
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub audio_processing: Option<crate::audio_processing::Settings>,
    #[serde(
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub transmission: Option<crate::transmission::Settings>,
    #[serde(
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub appearance: Option<Appearance>,
    #[serde(
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub custom_themes: Option<Vec<serde_json::Value>>,
    #[serde(
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub chat: Option<Chat>,
    #[serde(
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub sound_cues: Option<bool>,
    #[serde(
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub sounds: Option<crate::sounds::Settings>,
    #[serde(
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub share: Option<crate::streaming::publishing::ShareDefaults>,
    #[serde(
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub stream_view: Option<StreamView>,
    #[serde(
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub close_button: Option<CloseButton>,
    #[serde(
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub footer_hidden: Option<bool>,
    #[serde(
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub auto_download_updates: Option<bool>,
    #[serde(
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub user_audio: Option<BTreeMap<String, UserAudio>>,
    #[serde(
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub stream_volumes: Option<BTreeMap<String, u16>>,
}

/// A saved server as it travels: never its password, which stays sealed on its computer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SyncedServer {
    pub name: String,
    pub address: String,
    pub auto_connect: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_channel: Option<DefaultChannel>,
}

/// Reads a value that may be from another version of the app: one that doesn't fit is
/// dropped, so the computer keeps its own.
fn lenient<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value)
        .map_err(|error| tracing::debug!("A synced setting was left out: {error}"))
        .ok())
}

impl Synced {
    /// What travels from these settings.
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            servers: Some(
                settings
                    .servers
                    .iter()
                    .map(|server| SyncedServer {
                        name: server.name.clone(),
                        address: server.address.clone(),
                        auto_connect: server.auto_connect,
                        default_channel: server.default_channel.clone(),
                    })
                    .collect(),
            ),
            nickname: Some(settings.nickname.clone()),
            audio_processing: Some(settings.audio_processing),
            transmission: Some(settings.transmission),
            appearance: Some(settings.appearance.clone()),
            custom_themes: Some(settings.custom_themes.clone()),
            chat: Some(settings.chat),
            sound_cues: Some(settings.sound_cues),
            sounds: Some(settings.sounds.clone()),
            share: Some(settings.share),
            stream_view: Some(settings.stream_view),
            close_button: Some(settings.close_button),
            footer_hidden: Some(settings.footer_hidden),
            auto_download_updates: Some(settings.auto_download_updates),
            user_audio: Some(settings.user_audio.clone()),
            stream_volumes: Some(settings.stream_volumes.clone()),
        }
    }

    /// Reads a copy fetched from the server; anything unreadable is left out.
    pub fn read(data: &serde_json::Value) -> Self {
        serde_json::from_value(data.clone()).unwrap_or_default()
    }

    /// Puts what travelled onto this computer's settings. A field that is missing, or would
    /// make the settings invalid, keeps this computer's value. A server already saved here
    /// keeps its remembered password.
    pub fn apply(self, settings: &mut Settings) {
        if let Some(servers) = self.servers.filter(|servers| {
            !servers.is_empty()
                && servers
                    .iter()
                    .all(|server| !server.address.trim().is_empty())
        }) {
            let passwords: BTreeMap<String, String> = settings
                .servers
                .iter()
                .filter(|server| !server.password.is_empty())
                .map(|server| (server.address.trim().to_owned(), server.password.clone()))
                .collect();
            settings.servers = servers
                .into_iter()
                .map(|server| Server {
                    password: passwords
                        .get(server.address.trim())
                        .cloned()
                        .unwrap_or_default(),
                    name: server.name,
                    address: server.address,
                    auto_connect: server.auto_connect,
                    default_channel: server.default_channel,
                })
                .collect();
        }
        if let Some(nickname) = self
            .nickname
            .filter(|nickname| crate::onboarding::nickname_problem(nickname).is_none())
        {
            settings.nickname = nickname.trim().to_owned();
        }
        if let Some(processing) = self.audio_processing {
            settings.audio_processing = processing;
        }
        if let Some(transmission) = self
            .transmission
            .filter(|transmission| transmission.validate().is_ok())
        {
            settings.transmission = transmission;
        }
        if let Some(appearance) = self.appearance {
            settings.appearance = appearance;
        }
        if let Some(themes) = self.custom_themes {
            settings.custom_themes = themes;
        }
        if let Some(chat) = self.chat {
            settings.chat = chat;
        }
        if let Some(on) = self.sound_cues {
            settings.sound_cues = on;
        }
        if let Some(sounds) = self.sounds {
            settings.sounds = sounds;
        }
        if let Some(share) = self.share {
            settings.share = share;
        }
        if let Some(view) = self.stream_view {
            settings.stream_view = view;
        }
        if let Some(close) = self.close_button {
            settings.close_button = close;
        }
        if let Some(hidden) = self.footer_hidden {
            settings.footer_hidden = hidden;
        }
        if let Some(on) = self.auto_download_updates {
            settings.auto_download_updates = on;
        }
        if let Some(audio) = self.user_audio {
            settings.user_audio = audio;
        }
        if let Some(volumes) = self.stream_volumes {
            settings.stream_volumes = volumes;
        }
    }

    /// Whether taking this copy would change nothing on a computer with `settings`.
    pub fn matches(&self, settings: &Settings) -> bool {
        let mut taken = settings.clone();
        self.clone().apply(&mut taken);
        taken == *settings
    }
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
            footer_hidden: false,
            log_pane: LogPane::default(),
            sidebar_width: None,
            collapsed_channels: BTreeMap::new(),
            auto_download_updates: true,
            appearance: Appearance::default(),
            user_audio: BTreeMap::new(),
            stream_volumes: BTreeMap::new(),
            auto_watch: BTreeSet::new(),
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
            retired_settings_sync: None,
            settings_sync_version: 0,
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
    fn streaming_options_round_trip_and_stay_out_of_the_file_when_untouched() {
        let untouched = serde_json::to_string(&Settings::default()).unwrap();
        assert!(!untouched.contains("streaming"), "{untouched}");
        // Fast capture is on until someone turns it off.
        assert!(Streaming::default().fast_capture && !Streaming::default().hardware_av1);
        let settings = Settings {
            streaming: Streaming {
                hardware_av1: true,
                fast_capture: false,
            },
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        assert!(json.contains("\"hardware_av1\":true"), "{json}");
        assert!(json.contains("\"fast_capture\":false"), "{json}");
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.streaming, settings.streaming);
        // A file from before the default changed that only names the other option.
        let older: Streaming = serde_json::from_str(r#"{"hardware_av1":true}"#).unwrap();
        assert!(older.fast_capture);
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

    fn travelling() -> Settings {
        Settings {
            servers: vec![
                Server {
                    name: "Friends".into(),
                    address: "voice.example".into(),
                    auto_connect: true,
                    password: "sealed-secret".into(),
                    default_channel: Some(DefaultChannel {
                        id: 7,
                        name: "Raid night".into(),
                    }),
                },
                Server::new("other.example:9988"),
            ],
            nickname: "Sam".into(),
            transmission: crate::transmission::Settings {
                mode: crate::transmission::Mode::VoiceActivation,
                voice_threshold_db: -30,
                ptt_release_ms: 200,
            },
            appearance: Appearance {
                scheme: ColorScheme::Light,
                light_theme: ThemeName::Ocean,
                ..Appearance::default()
            },
            footer_hidden: true,
            close_button: CloseButton::Quit,
            sound_cues: false,
            user_audio: BTreeMap::from([(
                "uid-1".into(),
                UserAudio {
                    volume_percent: 150,
                    muted: true,
                },
            )]),
            stream_volumes: BTreeMap::from([("uid-2".into(), 60)]),
            ..Settings::default()
        }
    }

    #[test]
    fn synced_settings_round_trip_what_travels() {
        let from = travelling();
        let json = serde_json::to_value(Synced::from_settings(&from)).unwrap();
        let mut onto = Settings::default();
        Synced::read(&json).apply(&mut onto);
        assert_eq!(onto.nickname, "Sam");
        assert_eq!(onto.transmission, from.transmission);
        assert_eq!(onto.appearance, from.appearance);
        assert!(onto.footer_hidden);
        assert_eq!(onto.close_button, CloseButton::Quit);
        assert!(!onto.sound_cues);
        assert_eq!(onto.user_audio, from.user_audio);
        assert_eq!(onto.stream_volumes, from.stream_volumes);
        assert_eq!(onto.servers.len(), 2);
        assert!(onto.servers[0].auto_connect);
        assert_eq!(onto.servers[0].name, "Friends");
        assert!(Synced::read(&json).matches(&onto));
    }

    #[test]
    fn passwords_never_travel_and_a_local_one_survives() {
        let from = travelling();
        let text = serde_json::to_string(&Synced::from_settings(&from)).unwrap();
        assert!(!text.contains("password"), "{text}");
        assert!(!text.contains("sealed-secret"), "{text}");
        // This computer remembers a password for voice.example; the fetched list keeps it.
        let mut here = Settings {
            servers: vec![Server {
                password: "kept-here".into(),
                ..Server::new(" voice.example ")
            }],
            ..Settings::default()
        };
        let fetched = Synced {
            servers: Some(vec![
                SyncedServer {
                    name: "Renamed".into(),
                    address: "voice.example".into(),
                    auto_connect: false,
                    default_channel: None,
                },
                SyncedServer {
                    address: "new.example".into(),
                    ..SyncedServer::default()
                },
            ]),
            ..Synced::default()
        };
        fetched.apply(&mut here);
        assert_eq!(here.servers[0].password, "kept-here");
        assert_eq!(here.servers[0].name, "Renamed");
        assert_eq!(here.servers[1].password, "");
    }

    #[test]
    fn applying_leaves_what_stays_on_this_computer_alone() {
        let mut here = Settings {
            playback_volume_percent: 30,
            input_device_id: Some("input-id".into()),
            output_device_id: Some("output-id".into()),
            keybinds: Keybinds {
                toggle_mute: "F8".into(),
                ..Keybinds::default()
            },
            window: Some(WindowPlace {
                x: 1,
                y: 2,
                width: 800,
                height: 600,
                maximized: false,
            }),
            sidebar_width: Some(300),
            log_pane: LogPane {
                height: 200,
                collapsed: true,
            },
            collapsed_channels: BTreeMap::from([("voice.example".into(), BTreeSet::from([4]))]),
            streaming: Streaming {
                hardware_av1: true,
                fast_capture: false,
            },
            skip_install_prompt: true,
            monster_session: Some("sealed".into()),
            settings_sync_version: 7,
            ..Settings::default()
        };
        let before = here.clone();
        let text = serde_json::to_string(&Synced::from_settings(&here)).unwrap();
        for local in [
            "playback_volume",
            "input_device",
            "output_device",
            "keybinds",
            "window",
            "sidebar",
            "log_pane",
            "collapsed",
            "streaming",
            "skip_install",
            "monster",
            "settings_sync",
        ] {
            assert!(!text.contains(local), "{local} travelled: {text}");
        }
        Synced::from_settings(&travelling()).apply(&mut here);
        assert_eq!(here.nickname, "Sam");
        here.servers = before.servers.clone();
        here.nickname = before.nickname.clone();
        assert_eq!(here.playback_volume_percent, 30);
        assert_eq!(here.input_device_id, before.input_device_id);
        assert_eq!(here.output_device_id, before.output_device_id);
        assert_eq!(here.keybinds, before.keybinds);
        assert_eq!(here.window, before.window);
        assert_eq!(here.sidebar_width, before.sidebar_width);
        assert_eq!(here.log_pane, before.log_pane);
        assert_eq!(here.collapsed_channels, before.collapsed_channels);
        assert_eq!(here.streaming, before.streaming);
        assert!(here.skip_install_prompt);
        assert_eq!(here.monster_session, before.monster_session);
        assert_eq!(here.settings_sync_version, 7);
    }

    #[test]
    fn unknown_missing_and_unreadable_synced_fields_are_tolerated() {
        let mut here = Settings {
            nickname: "Here".into(),
            footer_hidden: true,
            ..Settings::default()
        };
        // From a newer version: a field this one doesn't know, a nickname, and a footer value
        // of a shape it can't read. Everything else is missing.
        let fetched = Synced::read(&serde_json::json!({
            "laser_mode": true,
            "nickname": "Sam",
            "footer_hidden": { "new": "shape" },
            "transmission": { "mode": "push_to_talk", "voice_threshold_db": 5, "ptt_release_ms": 9999 },
        }));
        assert_eq!(fetched.nickname.as_deref(), Some("Sam"));
        assert_eq!(fetched.footer_hidden, None);
        let before = here.clone();
        fetched.apply(&mut here);
        assert_eq!(here.nickname, "Sam");
        assert!(here.footer_hidden, "unreadable keeps this computer's");
        assert_eq!(
            here.transmission, before.transmission,
            "invalid keeps this computer's"
        );
        assert_eq!(
            here.servers, before.servers,
            "missing keeps this computer's"
        );
        assert_eq!(here.appearance, before.appearance);
        // Something that isn't an object at all changes nothing.
        let mut same = before.clone();
        Synced::read(&serde_json::json!([1, 2])).apply(&mut same);
        assert_eq!(same, before);
        // An empty server list or a blank nickname would make the settings invalid.
        let mut kept = before.clone();
        Synced::read(&serde_json::json!({ "servers": [], "nickname": "  " })).apply(&mut kept);
        assert_eq!(kept, before);
    }
}
