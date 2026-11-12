#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod alerts;
mod audio;
mod audio_processing;
mod autostart;
mod changelog;
mod chat;
mod chat_store;
mod client;
mod command_flow;
mod connection_charts;
mod hub;
mod icons;
mod install;
mod memory;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod native_audio;
mod notify;
mod omarchy;
mod onboarding;
mod permissions;
mod profiler;
mod resume;
mod server_log;
mod session_end;
mod settings;
mod shortcuts;
mod sounds;
mod streaming;
mod system_appearance;
mod themes;
mod transmission;
mod tray;
mod ts6_import;
mod ui;
mod updater;
mod usage;
mod window_icon;
mod window_place;

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Instant, SystemTime},
};

use tokio::sync::mpsc;

use alerts::{Alerts, Source};
use connection_charts::ConnectionHistory;
use icons::Icon;
use ui::Task;

const MIN_SIZE: (f32, f32) = (760.0, 560.0);
/// How long a closed window's servers get to acknowledge the goodbye before the app quits
/// regardless: each session waits up to 2 s for its server, after stopping its streams.
const QUIT_DEADLINE: std::time::Duration = std::time::Duration::from_secs(4);
/// At most one system notification this often; the rest of a burst only flashes the taskbar.
const TOAST_GAP: std::time::Duration = std::time::Duration::from_secs(10);

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "flexspeak=info,tsclientlib=warn,tsproto=warn".into()),
        )
        .init();
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--list-devices") => {
            let devices = audio::devices()?;
            println!("Microphones:");
            for device in &devices.inputs {
                println!(
                    "  {}{}",
                    device.name,
                    if Some(device) == devices.input.as_ref() {
                        " [default]"
                    } else {
                        ""
                    }
                );
            }
            println!("Playback devices:");
            for device in &devices.outputs {
                println!(
                    "  {}{}",
                    device.name,
                    if Some(device) == devices.output.as_ref() {
                        " [default]"
                    } else {
                        ""
                    }
                );
            }
            return Ok(());
        }
        Some("--probe") => {
            return client::probe(
                args.get(1)
                    .map(String::as_str)
                    .unwrap_or("ts.zirkumflex.io"),
            );
        }
        Some("--help" | "-h") => {
            println!(
                "Flexspeak\n\nNo arguments       Open the client\n--list-devices     List audio inputs and outputs\n--probe ADDRESS    Connect muted, report server state, disconnect\n--identity-path    Print the local identity file path\n--settings-path    Print the portable settings JSON path\n--uninstall        Remove the installed Windows copy (settings stay)"
            );
            return Ok(());
        }
        Some("--identity-path") => {
            println!("{}", client::identity_path()?.display());
            return Ok(());
        }
        Some("--settings-path") => {
            println!("{}", settings::path()?.display());
            return Ok(());
        }
        Some("--uninstall") => return install::uninstall(),
        // Start with Windows, with Start Minimized: the window waits in the notification area.
        Some(autostart::MINIMIZED) => {}
        // Rune's headless preview tooling (`--snap`, `--scene`, `--tree`, …) reads the rest.
        Some(other) if ui::is_tool_flag(other) => {}
        Some(other) => anyhow::bail!("Unknown argument '{other}'. Use --help."),
        None => {}
    }
    // Previews and other tools run beside the real client, so only the client itself is single.
    let tool = args.iter().any(|arg| ui::is_tool_flag(arg));
    if !tool {
        // A second launch brings the running window forward, from the tray too, and ends.
        if !rune::single_instance("io.venator.flexspeak") {
            return Ok(());
        }
        session_end::install();
        tray::allow();
    }
    ui::run(ui::Launch {
        restore_place: !tool,
        minimized: !tool && args.iter().any(|arg| arg == autostart::MINIMIZED),
    });
    Ok(())
}

struct App {
    onboarding: Option<onboarding::Step>,
    onboarding_error: Option<String>,
    permissions: permissions::Permissions,
    saved: Option<settings::Settings>,
    shortcuts: Option<shortcuts::Shortcuts>,
    /// The shortcuts another app holds, named on their rows in Settings.
    taken_shortcuts: Vec<shortcuts::Binding>,
    keybinds: shortcuts::Keybinds,
    keybind_draft: shortcuts::Keybinds,
    /// The shortcut being recorded, if Change was pressed.
    key_capture: Option<KeyCapture>,
    /// TeamSpeak 6's settings file, if setup found one to offer.
    ts6_path: Option<PathBuf>,
    /// What was read from it, once the user chose to.
    ts6: Option<ts6_import::Imported>,
    shortcut_error: Option<String>,
    settings_path: Option<PathBuf>,
    alerts: Alerts,
    settings_message: Option<String>,
    hub: Arc<hub::AudioHub>,
    playback_volume: Arc<audio::PlaybackVolume>,
    audio_processing: audio_processing::Settings,
    transmission: transmission::Settings,
    ptt: Arc<AtomicBool>,
    transmitting: bool,
    channel_health_visible: Arc<AtomicBool>,
    subscribe_all: Arc<AtomicBool>,
    /// Saved straight to settings when toggled, not held until the Settings page saves.
    footer_hidden: bool,
    /// The server log's size; saved when a resize ends or it folds, like `footer_hidden`.
    log_pane: settings::LogPane,
    /// Saved straight to settings when changed, like `footer_hidden`.
    appearance: settings::Appearance,
    user_audio: BTreeMap<String, settings::UserAudio>,
    /// `user_audio` as the voice threads read it.
    user_gains: client::UserGains,
    /// Each streamer's stream volume, by identity, as saved.
    stream_volumes: BTreeMap<String, u16>,
    /// When a stream volume last changed without being saved; saved a moment after a drag.
    stream_volumes_changed: Option<Instant>,
    sound_cues: bool,
    sounds: sounds::Settings,
    share_defaults: streaming::publishing::ShareDefaults,
    /// Settings > Streaming, applied when a share starts.
    stream_options: settings::Streaming,
    /// The streams' layout and details toggle, saved straight away and shared by every server.
    stream_view: settings::StreamView,
    /// Settings > Chat. Saved straight to settings when changed, like `footer_hidden`.
    chat_options: settings::Chat,
    /// The chat history on disk, opened the first time it is needed.
    chat_store: Option<chat_store::Store>,
    /// It couldn't be opened or written; it is left alone until the app restarts.
    chat_store_failed: bool,
    /// TeamSpeak's sounds, from its install on this computer if there is one.
    sound_bank: sounds::Bank,
    /// The sound volume moved and hasn't been saved yet.
    sounds_unsaved: bool,
    /// The playback volume, threshold or release delay moved and hasn't been saved yet.
    audio_unsaved: bool,
    /// Why the nickname typed in Settings wasn't kept, shown under its field.
    nickname_error: Option<String>,
    #[cfg(test)]
    played_sounds: Vec<sounds::Sound>,
    /// System notifications shown, by title.
    #[cfg(test)]
    shown_toasts: Vec<String>,
    /// When the last system notification went out, so a burst shows one.
    last_toast: Option<Instant>,
    /// `custom_themes` as written in the settings file.
    custom_themes: Vec<serde_json::Value>,
    theme_library: themes::Library,
    /// The desktop's theme, on Omarchy.
    omarchy: Option<omarchy::Current>,
    /// Whether the system is in dark mode, refreshed with each usage tick.
    system_dark: bool,
    window_size: (f32, f32),
    next_usage_tick: Instant,
    handle: Option<rune::AppHandle<App>>,
    ui: ui::UiState,
    address: String,
    nickname: String,
    password: String,
    devices: audio::Devices,
    input: Option<audio::DeviceChoice>,
    output: Option<audio::DeviceChoice>,
    /// Saved servers. Adding, editing or removing one saves the list straight away.
    servers: Vec<settings::Server>,
    /// Server passwords by address, kept until the app quits and never saved.
    server_passwords: HashMap<String, String>,
    /// The Add or Edit Server dialog, while it is open.
    server_editor: Option<ServerEditor>,
    loopback: bool,
    closing: bool,
    level: f32,
    /// Where this release would install, while it runs from somewhere temporary.
    install_offer: Option<install::Offer>,
    /// The install dialog shows at launch until answered.
    install_prompt: bool,
    installing: bool,
    /// The close button asked whether to hide or quit; `close_remember` is its Don't Ask Again.
    close_prompt: bool,
    close_remember: bool,
    /// Disconnect asked first, because you were sharing live.
    disconnect_prompt: bool,
    /// The window has the system's focus; false while another app is in front or it is minimized.
    window_focused: bool,
    /// The window is hidden in the tray.
    window_hidden: bool,
    /// The window's last known place, kept from before it hid, for saving at quit.
    window_place: Option<settings::WindowPlace>,
    /// Start with Windows, as the Run entry says.
    autostart: autostart::Autostart,
    /// Setup was opened from Settings over a saved profile, so it can be cancelled.
    redoing_setup: bool,
    process_monitor: usage::ProcessMonitor,
    process_usage: usage::ProcessUsage,
    cpu_history: usage::MetricHistory,
    ram_history: usage::MetricHistory,
    tab: Tab,
    /// The open Settings window's category; Settings floats over whichever page is showing.
    settings_page: Option<SettingsPage>,
    /// The category Settings reopens on.
    settings_recent: SettingsPage,
    /// What the Settings search field holds; empty shows the chosen category.
    settings_query: String,
    heap_usage: memory::HeapUsage,
    audio_buffer_bytes: usize,
    recording: Option<profiler::Recording>,
    trace_summary: Option<profiler::TraceSummary>,
    trace_path: Option<PathBuf>,
    trace_saving: bool,
    updater: updater::Updater,
    /// Where to return after an update's restart, until it has been reached.
    resume: Option<resume::Resume>,
    resume_started: bool,
    auto_connect_checked: bool,
    /// Every server connected or connecting, oldest first.
    sessions: Vec<ServerSession>,
    /// The server the window shows and the microphone goes to.
    active: Option<hub::SessionId>,
    /// Stands in for the active server while there is none: the saved-server page and setup.
    home: ServerSession,
    next_session: u64,
}

/// One server you're connected to, or connecting to, and what the window shows for it.
struct ServerSession {
    log: server_log::Log,
    /// The connection dropped and is coming back.
    reconnecting: bool,
    /// The first list of streams after joining shows who was already live, not who just went live.
    catalog_seen: bool,
    /// The channel you asked to join, to tell that apart from being moved.
    expected_channel: Option<u64>,
    quiet_channel_change: bool,
    /// You were just kicked out of your channel, which has its own sound: the change of channel
    /// that follows isn't heard as being moved as well.
    kicked: bool,
    id: hub::SessionId,
    /// The name it's saved under, if it has one.
    name: String,
    address: String,
    password: String,
    session: Option<mpsc::Sender<client::Command>>,
    connected: bool,
    stopping: bool,
    muted: bool,
    deafened: bool,
    server_controls: Option<command_flow::AudioState>,
    controls_pending: bool,
    status: String,
    detail: String,
    sent: u64,
    received: u64,
    state: client::Snapshot,
    collapsed_channels: HashSet<u64>,
    /// Members picked with ⌘/Ctrl-click, to move several at once.
    selected_members: BTreeSet<u16>,
    /// The poke being written.
    poke: Option<PokeDraft>,
    /// The kick or ban awaiting confirmation.
    moderation: Option<ModerationDraft>,
    /// The group ticks in the open member menu, shown until the server confirms them.
    group_edit: Option<GroupEditDraft>,
    /// Pokes received and not yet read, oldest first.
    pokes: Vec<Poke>,
    /// Pokes that arrived while another server was active, for the marker on the rail.
    unseen_pokes: u32,
    /// Text messages and pokes since connecting.
    chat: chat::Chat,
    expanded_client: Option<u16>,
    /// Clients heard right now, including yourself while you transmit.
    talking: BTreeSet<u16>,
    client_usage: BTreeMap<u16, Result<usage::ServerUsage, &'static str>>,
    /// Each sampled peer's health as their globe shows it.
    client_health: BTreeMap<u16, usage::SteadyHealth>,
    /// Your own health as the footer shows it.
    connection_health: usage::SteadyHealth,
    /// The server is refusing commands as flooding; Flexspeak resends what it must later.
    limited: bool,
    connection_history: ConnectionHistory,
    client_history: ConnectionHistory,
    network_usage: usage::NetworkUsage,
    connection_usage: usage::ConnectionUsage,
    channel_password: String,
    password_channel: Option<u64>,
    password_error: Option<String>,
    /// The password last used to join a channel, so an update's restart can rejoin it.
    channel_secret: Option<(u64, String)>,
    streaming: streaming::View,
    /// Bring this server to the front once it connects, since you just asked for it.
    activate_on_connect: bool,
    /// Where to return this server after an update's restart.
    resume: Option<resume::Server>,
}

impl ServerSession {
    fn new(
        id: hub::SessionId,
        name: String,
        address: String,
        password: String,
        surfaces: Arc<streaming::Surfaces>,
        visible: Arc<AtomicBool>,
    ) -> Self {
        Self {
            expected_channel: None,
            quiet_channel_change: false,
            kicked: false,
            log: server_log::Log::default(),
            reconnecting: false,
            catalog_seen: false,
            id,
            name,
            address,
            password,
            session: None,
            connected: false,
            stopping: false,
            muted: true,
            deafened: false,
            server_controls: None,
            controls_pending: false,
            status: "Ready to connect".into(),
            detail: String::new(),
            sent: 0,
            received: 0,
            state: client::Snapshot::default(),
            collapsed_channels: HashSet::new(),
            selected_members: BTreeSet::new(),
            poke: None,
            moderation: None,
            group_edit: None,
            pokes: Vec::new(),
            unseen_pokes: 0,
            chat: chat::Chat::default(),
            expanded_client: None,
            talking: BTreeSet::new(),
            client_usage: BTreeMap::new(),
            client_health: BTreeMap::new(),
            connection_health: usage::SteadyHealth::default(),
            limited: false,
            connection_history: ConnectionHistory::new(),
            client_history: ConnectionHistory::new(),
            network_usage: usage::NetworkUsage::default(),
            connection_usage: usage::ConnectionUsage::default(),
            channel_password: String::new(),
            password_channel: None,
            password_error: None,
            channel_secret: None,
            streaming: streaming::View::sharing(surfaces, visible),
            activate_on_connect: false,
            resume: None,
        }
    }

    /// What the rail calls this server: its saved name, else what the server calls itself.
    fn title(&self) -> &str {
        if !self.name.trim().is_empty() {
            self.name.trim()
        } else if !self.state.server.is_empty() {
            &self.state.server
        } else {
            self.address.trim()
        }
    }

    fn clear_client_details(&mut self) {
        self.expanded_client = None;
        self.client_history = ConnectionHistory::new();
    }

    fn can_request_join(&self, loopback: bool, id: u64) -> bool {
        self.connected
            && !loopback
            && !self.stopping
            && self
                .state
                .current
                .as_ref()
                .is_none_or(|current| current.id != id)
            && self
                .state
                .channels
                .iter()
                .any(|channel| channel.id == id && channel.join_access.can_request())
    }

    fn microphone_off(&self) -> bool {
        self.muted || self.deafened || !self.connected || !self.state.can_talk
    }
}

/// A poke being written to one client.
#[derive(Debug, Clone, Default)]
struct PokeDraft {
    id: u16,
    name: String,
    message: String,
}

/// A kick or ban, waiting for its reason and confirmation.
#[derive(Debug, Clone)]
struct ModerationDraft {
    ids: Vec<u16>,
    action: client::Moderation,
    reason: String,
}

/// Group ticks in the open member menu; each one applies to everyone in `ids` at once.
#[derive(Debug, Clone, Default)]
struct GroupEditDraft {
    ids: Vec<u16>,
    /// The server groups touched so far: added when true, removed when false.
    wanted: BTreeMap<u64, bool>,
    channel_group: Option<u64>,
}

/// Whether the people being edited hold a server group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GroupMark {
    On,
    Off,
    Mixed,
}

impl GroupMark {
    fn of(wanted: Option<bool>, held: GroupMark) -> GroupMark {
        match wanted {
            Some(true) => GroupMark::On,
            Some(false) => GroupMark::Off,
            None => held,
        }
    }
}

/// A poke someone sent you.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Poke {
    from: String,
    message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Home,
    Statistics,
}

/// The Add or Edit Server dialog's drafts. `index` is the server being edited; `None` adds one.
#[derive(Debug, Clone, Default)]
struct ServerEditor {
    index: Option<usize>,
    name: String,
    address: String,
    password: String,
    error: Option<String>,
    /// Remove was pressed once; the second press removes the server.
    remove_armed: bool,
    /// Settings opened only to hold this dialog, from Home or the rail, and closes with it.
    alone: bool,
}

/// A category of the Settings window, in sidebar order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsPage {
    Server,
    Appearance,
    Chat,
    Streaming,
    Audio,
    Sounds,
    Shortcuts,
    Permissions,
    General,
    Updates,
    Errors,
    File,
}

impl SettingsPage {
    const ALL: [Self; 12] = [
        Self::Server,
        Self::Appearance,
        Self::Chat,
        Self::Streaming,
        Self::Audio,
        Self::Sounds,
        Self::Shortcuts,
        Self::Permissions,
        Self::General,
        Self::Updates,
        Self::Errors,
        Self::File,
    ];

    fn title(self) -> &'static str {
        match self {
            Self::Server => "Server",
            Self::Appearance => "Appearance",
            Self::Chat => "Chat",
            Self::Streaming => "Streaming",
            Self::Audio => "Audio",
            Self::Sounds => "Sounds",
            Self::Shortcuts => "Shortcuts",
            Self::Permissions => "Permissions",
            Self::General => "General",
            Self::Updates => "Updates",
            Self::Errors => "Error Log",
            Self::File => "Settings File",
        }
    }
}

/// Which sound a channel change deserves. Connecting isn't a change, and neither is losing it.
fn channel_sound(session: &mut ServerSession, next: &client::Snapshot) -> Option<sounds::Sound> {
    let kicked = std::mem::take(&mut session.kicked);
    let before = session.state.current.as_ref().map(|channel| channel.id);
    let after = next.current.as_ref().map(|channel| channel.id);
    let (Some(before), Some(after)) = (before, after) else {
        return None;
    };
    if before == after {
        return None;
    }
    let expected = session.expected_channel.take();
    if std::mem::take(&mut session.quiet_channel_change) || kicked {
        return None;
    }
    Some(if expected == Some(after) {
        sounds::Sound::JoinedChannel
    } else {
        sounds::Sound::MovedChannel
    })
}

/// The sound for what someone did. Your own moves already have their channel sounds.
fn activity_sound(activity: &client::Activity) -> Option<sounds::Sound> {
    use client::ActivityKind::{
        Banned, ChannelGroupChanged, Connected, Disconnected, KickedFromChannel, KickedFromServer,
        LostConnection, Moved, RecordingStarted, RecordingStopped, ServerGroupAssigned,
        ServerGroupRevoked, TalkPowerGranted, TalkPowerRevoked, Whispered,
    };
    use sounds::Sound;
    if activity.is_self {
        return match activity.kind {
            KickedFromChannel => Some(Sound::KickedFromChannel),
            KickedFromServer => Some(Sound::Kicked),
            Banned => Some(Sound::Banned),
            TalkPowerGranted => Some(Sound::TalkPowerGranted),
            TalkPowerRevoked => Some(Sound::TalkPowerRevoked),
            ServerGroupAssigned => Some(Sound::ServerGroupAssigned),
            ServerGroupRevoked => Some(Sound::ServerGroupRevoked),
            ChannelGroupChanged => Some(Sound::ChannelGroupChanged),
            _ => None,
        };
    }
    if activity.entered_yours {
        return Some(match activity.kind {
            Connected => Sound::PersonConnected,
            Moved => Sound::PersonMovedIn,
            KickedFromChannel => Sound::PersonKickedIntoChannel,
            RecordingStarted => Sound::PersonRecordingStarted,
            RecordingStopped => Sound::PersonRecordingStopped,
            Whispered => Sound::WhisperReceived,
            _ => Sound::PersonJoinedChannel,
        });
    }
    activity.left_yours.then_some(match activity.kind {
        Disconnected => Sound::PersonDisconnected,
        LostConnection => Sound::PersonLostConnection,
        Moved => Sound::PersonMovedOut,
        KickedFromChannel => Sound::PersonKickedFromChannel,
        KickedFromServer => Sound::PersonKickedFromServer,
        Banned => Sound::PersonBanned,
        _ => Sound::PersonLeftChannel,
    })
}

fn log_activity(log: &mut server_log::Log, activity: &client::Activity) {
    use client::ActivityKind::{
        Banned, ChannelGroupChanged, Connected, Disconnected, KickedFromChannel, KickedFromServer,
        LostConnection, Moved, RecordingStarted, RecordingStopped, ServerGroupAssigned,
        ServerGroupRevoked, Switched, TalkPowerGranted, TalkPowerRevoked, Whispered,
    };
    use server_log::Tone;
    let tone = match activity.kind {
        Connected => Tone::Arrived,
        Moved | Switched | RecordingStarted | RecordingStopped | TalkPowerGranted
        | TalkPowerRevoked | Whispered | ServerGroupAssigned | ServerGroupRevoked
        | ChannelGroupChanged => Tone::Moved,
        _ => Tone::Left,
    };
    let action = match (activity.kind, activity.is_self) {
        (Switched, true) => format!("joined {}", activity.channel),
        (Switched, false) => format!("→ {}", activity.channel),
        (Moved, true) => format!("were moved to {}", activity.channel),
        (Moved, false) => format!("moved to {}", activity.channel),
        (Connected, _) => "connected".to_owned(),
        (Disconnected, _) => "disconnected".to_owned(),
        (LostConnection, _) => "lost connection".to_owned(),
        (KickedFromChannel, true) => "were kicked from the channel".to_owned(),
        (KickedFromChannel, false) => "kicked from the channel".to_owned(),
        (KickedFromServer, true) => "were kicked".to_owned(),
        (KickedFromServer, false) => "kicked".to_owned(),
        (Banned, true) => "were banned".to_owned(),
        (Banned, false) => "banned".to_owned(),
        (RecordingStarted, _) => "started recording".to_owned(),
        (RecordingStopped, _) => "stopped recording".to_owned(),
        (TalkPowerGranted, _) => "got talk power".to_owned(),
        (TalkPowerRevoked, _) => "lost talk power".to_owned(),
        (Whispered, _) => "whispered to you".to_owned(),
        (ServerGroupAssigned, _) => format!("were added to {}", activity.channel),
        (ServerGroupRevoked, _) => format!("were removed from {}", activity.channel),
        (ChannelGroupChanged, _) => format!("are now in {}", activity.channel),
    };
    let name = if activity.is_self {
        "You"
    } else {
        activity.name.as_str()
    };
    log.add(tone, name, action);
}

/// Who else in your channel went live or stopped, from its list of streams: logged, and
/// returned as the sounds for it.
fn stream_changes(
    session: &mut ServerSession,
    catalog: &[streaming::StreamInfo],
) -> Vec<sounds::Sound> {
    const STARTED: &str = "started streaming";
    if !std::mem::replace(&mut session.catalog_seen, true) {
        return Vec::new();
    }
    let member = |publisher: u16| {
        let members = session.state.current_members();
        members
            .iter()
            .find(|member| member.id == publisher)
            .cloned()
    };
    let before = &session.streaming.catalog;
    let changes: Vec<_> = catalog
        .iter()
        .filter(|stream| !before.iter().any(|old| old.id == stream.id))
        .map(|stream| (stream.publisher, STARTED))
        .chain(
            before
                .iter()
                .filter(|old| !catalog.iter().any(|stream| stream.id == old.id))
                .map(|old| (old.publisher, "stopped streaming")),
        )
        .filter_map(|(publisher, action)| {
            let person = member(publisher);
            (!person.as_ref().is_some_and(|person| person.is_self)).then(|| {
                (
                    person.map_or_else(|| "Someone".to_owned(), |person| person.name),
                    action,
                )
            })
        })
        .collect();
    changes
        .into_iter()
        .map(|(name, action)| {
            session.log.add(server_log::Tone::Stream, name, action);
            if action == STARTED {
                sounds::Sound::PersonStreamStarted
            } else {
                sounds::Sound::PersonStreamStopped
            }
        })
        .collect()
}

/// Lays a server's streams out as saved: where the others go and whether details show.
fn apply_stream_view(streaming: &mut streaming::View, view: settings::StreamView) {
    streaming.strip = if view.beside {
        streaming::Strip::Beside
    } else {
        streaming::Strip::Below
    };
    streaming.show_stats = view.details;
}

/// Your share going live or ending, then one sound for each viewer who arrived or left.
fn viewer_sounds(
    before: &streaming::publishing::Status,
    event: &streaming::Event,
) -> Vec<sounds::Sound> {
    let streaming::Event::Publishing(after) = event else {
        return Vec::new();
    };
    if !before.live || !after.live {
        return match (before.live, after.live) {
            (false, true) => vec![sounds::Sound::StreamStarted],
            (true, false) => vec![sounds::Sound::StreamStopped],
            _ => Vec::new(),
        };
    }
    let has = |status: &streaming::publishing::Status, id: u16| {
        status.viewers.iter().any(|viewer| viewer.0 == id)
    };
    let joined = after.viewers.iter().filter(|v| !has(before, v.0));
    let left = before.viewers.iter().filter(|v| !has(after, v.0));
    joined
        .map(|_| sounds::Sound::ViewerJoined)
        .chain(left.map(|_| sounds::Sound::ViewerLeft))
        .chain(
            asked_to_watch(before, event)
                .into_iter()
                .map(|_| sounds::Sound::WatchRequested),
        )
        .collect()
}

/// The names of people newly asking to watch your private share.
fn asked_to_watch(before: &streaming::publishing::Status, event: &streaming::Event) -> Vec<String> {
    let streaming::Event::Publishing(after) = event else {
        return Vec::new();
    };
    after
        .requests
        .iter()
        .filter(|(id, _)| !before.requests.iter().any(|(old, _)| old == id))
        .map(|(_, name)| name.clone())
        .collect()
}

/// A shortcut being recorded, and why the last key press wasn't accepted.
pub struct KeyCapture {
    binding: shortcuts::Binding,
    error: Option<String>,
}

#[derive(Debug, Clone)]
enum Message {
    SetupNext,
    SetupBack,
    RequestPermission(permissions::Kind),
    PermissionDone(permissions::Kind, Result<String, String>),
    OpenPermissionSettings(permissions::Kind),
    /// Closes one row of the notice strip.
    DismissAlert(u64),
    Shortcut(shortcuts::Action),
    /// Fills in setup from the TeamSpeak 6 install it found.
    ImportTs6,
    /// Leaves a server found in TeamSpeak 6 out of the saved list.
    DropTs6Server(String),
    /// Starts listening for the next key press as this shortcut.
    RecordShortcut(shortcuts::Binding),
    CancelRecording,
    Recorded(shortcuts::Recorded),
    ReloadSettings,
    CopySettingsPath,
    /// Copies the error log as text, for a bug report.
    CopyErrorLog,
    ClearErrorLog,
    PlaybackVolume(u8),
    AudioProcessing(audio_processing::Settings),
    Transmission(transmission::Settings),
    Address(String),
    Nickname(String),
    /// Enter in the nickname field, or the field losing the keyboard: keeps what was typed.
    CommitNickname,
    Password(String),
    ChannelPassword(String),
    SubmitChannelPassword,
    CancelChannelPassword,
    Input(audio::DeviceChoice),
    Output(audio::DeviceChoice),
    Refresh,
    /// Connects to the saved server at this index, alongside any others.
    Connect(usize),
    /// Shows this server: its channels, streams and voice panel, and sends your microphone there.
    SwitchServer(hub::SessionId),
    /// Ends one server's connection, or cancels it while it's still connecting.
    StopServer(hub::SessionId),
    /// The rail's + opens the list of saved servers to connect to, then closes.
    OpenServerPicker,
    CloseServerPicker,
    AddServer,
    EditServer(usize),
    ServerAutoConnect(usize, bool),
    /// Connects to the servers set to connect at launch.
    AutoConnect,
    ServerName(String),
    ServerAddress(String),
    ServerPassword(String),
    SubmitServer,
    CancelServer,
    RemoveServer,
    TestAudio,
    Stop,
    Mute,
    Deafen,
    Join(u64),
    ToggleChannel(u64),
    ToggleSubscribeAll,
    ToggleFooter,
    Appearance(settings::Appearance),
    OpenSettingsFolder,
    /// Opens the volume dialog for a person in the channel tree.
    OpenUserVolume(u16),
    UserVolume(u16),
    CloseUserVolume,
    /// Silences or restores a person for you alone.
    ToggleUserMute(u16),
    SoundCues(bool),
    SoundVolume(u8),
    SoundPack(sounds::Pack),
    SoundGroup(sounds::Group, bool),
    SoundOn(sounds::Sound, bool),
    LogSearch(bool),
    LogQuery(String),
    LogPerson(Option<String>),
    LogPeople,
    LogClear,
    /// Folds the server log down to its heading, or opens it again.
    ToggleLog,
    /// A resize of the server log ended: keep its size.
    SaveLogPane,
    SharePreview(
        streaming::publishing::ShareSource,
        Option<streaming::publishing::Still>,
    ),
    PreviewSound(sounds::Sound),
    ToggleClient(u16),
    /// A server's connection reporting; once it has ended, these are dropped.
    Backend(hub::SessionId, client::Event),
    /// A background task failed before it could report anywhere else.
    Alert(Source, String),
    /// The window's close button, or the system asking the window to close.
    Close,
    /// Leaves every server and quits.
    Quit,
    /// The close dialog's answer; `None` stays as you were.
    CloseChoice(Option<settings::CloseButton>),
    CloseRemember(bool),
    /// Settings: what the close button does while connected.
    SetCloseButton(settings::CloseButton),
    /// Settings: start with Windows, and whether in the tray.
    SetAutostart(autostart::Autostart),
    /// The tray's Disconnect: leaves the server on screen without asking.
    TrayDisconnect,
    /// The answer to Disconnect while sharing live: `true` disconnects.
    ConfirmDisconnect(bool),
    /// The window gained or lost the system's focus.
    WindowFocus(bool),
    UsageTick,
    ShowTab(Tab),
    OpenSettings(SettingsPage),
    CloseSettings,
    SettingsSearch(String),
    RefreshStreams,
    WatchStream(String),
    /// Stops one stream by ID, or all of them.
    StopWatching(Option<String>),
    /// Puts one stream on the stage, or shows every watched stream side by side.
    FocusStream(Option<String>),
    /// The grid for `None`, or a stream on the stage with the others in this strip.
    StreamLayout(Option<streaming::Strip>),
    /// Tuck the streams around the stage away, or bring them back.
    HideOtherStreams(bool),
    /// The large stream fills the screen, or comes back into the window.
    StreamFullscreen(bool),
    /// Trades two watched streams' places.
    SwapStreams(String, String),
    /// A publisher's stream volume, in percent, up to `streaming::MAX_VOLUME`.
    StreamVolume(u16, u16),
    /// Mutes a publisher's stream, or brings back the volume it had.
    ToggleStreamMute(u16),
    ToggleStreamStats,
    OpenShare,
    CloseShare,
    ShareQuality(streaming::publishing::Quality),
    ShareResolution(streaming::publishing::Resolution),
    ShareFrameRate(u32),
    ShareVideoBitrate(u32),
    ShareAudioBitrate(u32),
    ShareAccess(streaming::publishing::Access),
    /// What gives way when a share's connection or computer can't keep up.
    ShareStruggle(streaming::publishing::Struggle),
    /// The share dialog's Advanced tab, or its simple one.
    ShareAdvanced(bool),
    ShareGroup(streaming::publishing::SourceGroup),
    RefreshShareSources,
    ShareSources(Result<Vec<streaming::publishing::ShareSource>, String>),
    ShareSource(streaming::publishing::ShareSource),
    ShareAudio(bool),
    StreamingHardwareAv1(bool),
    StreamingFastCapture(bool),
    ShareAudioOutput(audio::DeviceChoice),
    StartSharing,
    /// Goes live with this source straight away, publicly.
    QuickShare(streaming::publishing::ShareSource),
    RefreshQuickSources,
    QuickSources(Result<Vec<streaming::publishing::ShareSource>, String>),
    StopSharing,
    DecideViewer(u16, bool),
    RecordTrace,
    StopTrace,
    TraceSaved(Result<PathBuf, String>),
    CopyTracePath,
    Updater(updater::Event),
    /// Returns to where the app was before an update's restart.
    Resume,
    /// A plain click on a member selects it alone; with ⌘/Ctrl it joins or leaves the selection.
    SelectMember(u16, bool),
    ClearSelection,
    /// Moves clients into a channel; your own client joins it instead.
    MoveMembers(Vec<u16>, u64),
    OpenPoke(u16),
    PokeText(String),
    SubmitPoke,
    CancelPoke,
    DismissPoke,
    /// Asks to kick or ban these clients; nothing happens until it's confirmed.
    OpenModeration(Vec<u16>, client::Moderation),
    ModerationReason(String),
    ModerationBan(Option<u64>),
    SubmitModeration,
    CancelModeration,
    OpenGroups(Vec<u16>),
    ToggleServerGroup(u64),
    PickChannelGroup(u64),
    Install,
    Installed(Result<(), String>),
    /// Closes the install dialog; `true` also stops it asking at launch.
    DeclineInstall(bool),
    RedoSetup,
    CancelSetup,
    /// Text chat: writing, sending, and what the chat column shows.
    Chat(chat::Action),
}

/// "a", "a and b", "a, b and c".
fn spoken_list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

impl App {
    fn new(handle: Option<rune::AppHandle<App>>) -> Self {
        let (devices, error) = match audio::devices() {
            Ok(devices) => (devices, None),
            Err(error) => (
                audio::Devices::default(),
                Some(format!("Audio device detection failed: {error:#}")),
            ),
        };
        let mut app = Self::with_devices(devices, error);
        app.handle = handle;
        app.window_hidden = ui::STARTED_HIDDEN.load(Ordering::Relaxed);
        app.window_focused = !app.window_hidden;
        app.autostart = autostart::current();
        app.system_dark = system_appearance::is_dark().unwrap_or(true);
        match settings::path() {
            Ok(path) => app.load_settings(path),
            Err(error) => app.alerts.raise(Source::SettingsLoad, format!("{error:#}")),
        }
        app.omarchy = omarchy::poll(&app.theme_library);
        let (mut shortcuts, events) = shortcuts::Shortcuts::new(app.ptt.clone());
        if app.onboarding.is_none() {
            app.shortcut_error = shortcuts
                .configure(&app.keybinds)
                .err()
                .map(|error| format!("{error:#}"));
            if shortcuts::supported()
                && let Some(error) = &app.shortcut_error
            {
                app.alerts.raise(Source::Shortcuts, error);
            }
        }
        if !shortcuts::supported() {
            app.shortcut_error =
                Some("Global shortcuts require X11; native Wayland is not supported.".into());
        }
        let auto_download = app.updater.auto_download;
        app.updater = updater::Updater::new(
            updater::Build::running(),
            updater::Package::detect(),
            auto_download,
        );
        app.install_offer = install::offer();
        app.install_prompt = app.install_offer.is_some()
            && !app
                .saved
                .as_ref()
                .is_some_and(|saved| saved.skip_install_prompt);
        if app.updater.unsupported().is_none() {
            updater::clean_up();
            app.updater.next_check = Some(Instant::now() + updater::FIRST_CHECK_DELAY);
        }
        if app.onboarding.is_none()
            && let Some(path) = &app.settings_path
        {
            app.resume = resume::take(&resume::path(path), SystemTime::now());
        }
        app.sync_page_visibility();
        app.permissions.refresh();
        app.shortcuts = Some(shortcuts);
        if app.shortcut_error.is_none() {
            let keys = app.keybinds.clone();
            app.report_taken_shortcuts(&keys);
        }
        ui::forward_shortcuts(app.handle.clone(), events);
        app.sync_fields();
        app
    }

    fn with_devices(devices: audio::Devices, error: Option<String>) -> Self {
        let mut process_monitor = usage::ProcessMonitor::new();
        let process_usage = process_monitor.sample();
        let mut cpu_history = usage::MetricHistory::new(Instant::now());
        let mut ram_history = usage::MetricHistory::new(Instant::now());
        cpu_history.push(Instant::now(), process_usage.cpu_percent.map(f64::from));
        ram_history.push(
            Instant::now(),
            process_usage.memory_bytes.map(|bytes| bytes as f64),
        );
        let mut alerts = Alerts::default();
        if let Some(error) = error {
            alerts.raise(Source::Audio, error);
        }
        Self {
            onboarding: None,
            onboarding_error: None,
            permissions: permissions::Permissions::default(),
            saved: None,
            shortcuts: None,
            taken_shortcuts: Vec::new(),
            keybinds: shortcuts::Keybinds::default(),
            keybind_draft: shortcuts::Keybinds::default(),
            key_capture: None,
            ts6_path: None,
            ts6: None,
            shortcut_error: None,
            settings_path: None,
            alerts,
            settings_message: None,
            hub: Arc::new(hub::AudioHub::default()),
            playback_volume: Arc::new(audio::PlaybackVolume::default()),
            audio_processing: audio_processing::Settings::default(),
            transmission: transmission::Settings::default(),
            ptt: Arc::new(AtomicBool::new(false)),
            transmitting: false,
            channel_health_visible: Arc::new(AtomicBool::new(true)),
            subscribe_all: Arc::new(AtomicBool::new(true)),
            footer_hidden: false,
            log_pane: settings::LogPane::default(),
            appearance: settings::Appearance::default(),
            user_audio: BTreeMap::new(),
            user_gains: client::UserGains::default(),
            stream_volumes: BTreeMap::new(),
            stream_volumes_changed: None,
            sound_cues: true,
            sounds: sounds::Settings::default(),
            share_defaults: Default::default(),
            stream_options: settings::Streaming::default(),
            stream_view: settings::StreamView::default(),
            chat_options: settings::Chat::default(),
            chat_store: None,
            chat_store_failed: false,
            sound_bank: if cfg!(test) {
                sounds::Bank::default()
            } else {
                sounds::Bank::installed()
            },
            sounds_unsaved: false,
            audio_unsaved: false,
            nickname_error: None,
            #[cfg(test)]
            played_sounds: Vec::new(),
            #[cfg(test)]
            shown_toasts: Vec::new(),
            last_toast: None,
            custom_themes: Vec::new(),
            theme_library: themes::Library::new(&[]),
            omarchy: None,
            system_dark: true,
            window_size: (1180.0, 790.0),
            next_usage_tick: Instant::now() + usage::REFRESH_INTERVAL,
            handle: None,
            ui: ui::UiState::default(),
            input: devices.input.clone(),
            output: devices.output.clone(),
            devices,
            address: "ts.zirkumflex.io".into(),
            nickname: "Flexspeak".into(),
            password: String::new(),
            servers: Vec::new(),
            server_passwords: HashMap::new(),
            server_editor: None,
            loopback: false,
            closing: false,
            level: 0.0,
            install_offer: None,
            install_prompt: false,
            installing: false,
            close_prompt: false,
            close_remember: false,
            disconnect_prompt: false,
            window_focused: true,
            window_hidden: false,
            window_place: None,
            autostart: autostart::Autostart::default(),
            redoing_setup: false,
            process_monitor,
            process_usage,
            cpu_history,
            ram_history,
            tab: Tab::Home,
            settings_page: None,
            settings_recent: SettingsPage::Server,
            settings_query: String::new(),
            heap_usage: memory::snapshot(),
            audio_buffer_bytes: 0,
            recording: None,
            trace_summary: None,
            trace_path: None,
            trace_saving: false,
            // Previews and tests never check; `new` enables real builds.
            updater: updater::Updater::new(None, Err(String::new()), true),
            resume: None,
            resume_started: false,
            auto_connect_checked: false,
            sessions: Vec::new(),
            active: None,
            next_session: 1,
            home: ServerSession::new(
                hub::SessionId(0),
                String::new(),
                String::new(),
                String::new(),
                Arc::new(streaming::Surfaces::default()),
                Arc::new(AtomicBool::new(false)),
            ),
        }
    }

    /// What the notification-area icon shows, or `None` for no icon: switched off, or quitting.
    fn tray_status(&self) -> Option<tray::Status> {
        if self.closing || self.appearance.hide_tray_icon || !tray::SUPPORTED {
            return None;
        }
        let server = self.server();
        let connected = server.connected && !server.stopping;
        let state = if !connected {
            tray::State::Offline
        } else if server.deafened {
            tray::State::Deafened
        } else if server.muted {
            tray::State::Muted
        } else if self.own_id().is_some_and(|id| server.talking.contains(&id)) {
            tray::State::Talking
        } else {
            tray::State::Open
        };
        Some(tray::Status {
            state,
            server: if connected {
                server.title().to_owned()
            } else {
                String::new()
            },
            muted: server.muted,
            deafened: server.deafened,
        })
    }

    /// The server the window is showing, or a blank one on the saved-server page and in setup.
    fn server(&self) -> &ServerSession {
        self.active
            .and_then(|id| self.session(id))
            .unwrap_or(&self.home)
    }

    fn server_mut(&mut self) -> &mut ServerSession {
        let active = self.active;
        match self.sessions.iter_mut().find(|s| Some(s.id) == active) {
            Some(session) => session,
            None => &mut self.home,
        }
    }

    fn session(&self, id: hub::SessionId) -> Option<&ServerSession> {
        self.sessions.iter().find(|session| session.id == id)
    }

    fn session_mut(&mut self, id: hub::SessionId) -> Option<&mut ServerSession> {
        self.sessions.iter_mut().find(|session| session.id == id)
    }

    /// No server connected or connecting, and no microphone test running.
    fn idle(&self) -> bool {
        self.sessions.is_empty()
    }

    /// What the window's close button does while connected.
    fn close_button(&self) -> settings::CloseButton {
        self.saved
            .as_ref()
            .map(|saved| saved.close_button)
            .unwrap_or_default()
    }

    /// Whether the notification-area icon is there to bring a hidden window back.
    fn tray_available(&self) -> bool {
        tray::SUPPORTED && !self.appearance.hide_tray_icon
    }

    /// Hides the window and stays connected. Without a tray icon to come back from, the window
    /// is minimized instead.
    fn hide_window(&mut self) {
        self.window_focused = false;
        // Hidden, the window can't be measured; its place is kept for saving at quit.
        if let Some(place) = window_place::current() {
            self.window_place = Some(place);
        }
        let Some(handle) = &self.handle else {
            self.window_hidden = self.tray_available();
            return;
        };
        if self.tray_available() {
            self.window_hidden = true;
            handle.hide();
        } else {
            #[cfg(windows)]
            window_icon::minimize();
            #[cfg(not(windows))]
            handle.minimize();
        }
    }

    /// Remembers where the window is, for the next launch to open there.
    fn save_window_place(&mut self) {
        let Some(place) = window_place::current().or(self.window_place) else {
            return;
        };
        if self
            .saved
            .as_ref()
            .is_some_and(|saved| saved.window != Some(place))
        {
            self.save_preference(
                |saved| saved.window = Some(place),
                "The window's size and place were not saved",
            );
        }
    }

    /// Whether the window is on a server you're connected to.
    fn live(&self) -> bool {
        self.server().connected && !self.loopback && !self.server().stopping
    }

    /// How many servers are connected right now.
    fn connected_servers(&self) -> usize {
        self.sessions
            .iter()
            .filter(|s| s.connected && !s.stopping)
            .count()
    }

    /// The servers on the rail, oldest first. One being disconnected leaves the rail at once,
    /// unless it's the last one and the page is still showing it.
    fn rail_sessions(&self) -> impl Iterator<Item = &ServerSession> {
        let showing = self.active;
        self.sessions
            .iter()
            .filter(move |session| !session.stopping || Some(session.id) == showing)
    }

    /// Makes a server the one shown and spoken to.
    fn activate(&mut self, id: Option<hub::SessionId>) {
        if self.active == id {
            return;
        }
        let others_hidden = {
            let old = self.server_mut();
            // The session stops these streams itself; this clears them from the screen now.
            old.streaming.clear_picture();
            old.streaming.share_open = false;
            old.streaming.others_hidden
        };
        let view = self.stream_view;
        self.active = id;
        self.hub.set_active(id);
        self.transmitting = false;
        self.level = 0.0;
        self.ui.open = None;
        self.ui.member_menu = None;
        self.ui.move_menu = false;
        self.ui.groups_menu = None;
        self.ui.drag = None;
        self.ui.tile_drag = None;
        self.ui.server_picker = false;
        let new = self.server_mut();
        apply_stream_view(&mut new.streaming, view);
        new.streaming.others_hidden = others_hidden;
        new.unseen_pokes = 0;
        new.activate_on_connect = false;
        self.ui.chat_toast = None;
        self.ui.chat_lists = Default::default();
        self.chat_seen();
        self.sync_page_visibility();
    }

    /// Picks another server to show after this one goes, or none.
    fn next_active(&self, leaving: hub::SessionId) -> Option<hub::SessionId> {
        self.sessions
            .iter()
            .find(|session| session.id != leaving && !session.stopping)
            .map(|session| session.id)
    }

    /// Ends a server's connection and moves the window to another if there is one.
    fn stop_session(&mut self, id: hub::SessionId) {
        let Some(session) = self.session_mut(id) else {
            return;
        };
        let leaving = session.connected && !session.stopping;
        session.streaming.sharing.stop_capture();
        // While the output is still open: it closes with the last server.
        if leaving && !self.loopback && !self.closing {
            self.play_sound(sounds::Sound::Disconnected);
        }
        self.send_to(id, client::Command::Stop);
        if let Some(session) = self.session_mut(id) {
            session.stopping = true;
            session.status = "Disconnecting…".into();
        }
        if self.active == Some(id)
            && let Some(next) = self.next_active(id)
        {
            self.activate(Some(next));
        }
    }

    /// Files an alert under the active server, or the app when it isn't about a server.
    fn raise_active(&mut self, source: Source, detail: impl AsRef<str>) {
        match self.active.filter(|_| source.server_scoped()) {
            Some(id) => self.raise_for(id, source, detail),
            None => self.alerts.raise(source, detail),
        }
    }

    fn raise_for(&mut self, id: hub::SessionId, source: Source, detail: impl AsRef<str>) {
        if !source.server_scoped() {
            return self.alerts.raise(source, detail);
        }
        // Say which server, unless there is only the one you're looking at.
        let server = (self.sessions.len() > 1 || self.active != Some(id))
            .then(|| self.session(id).map(|session| session.title().to_owned()))
            .flatten();
        self.alerts.raise_in(id.0, server, source, detail);
    }

    fn begin_alert(&mut self, source: Source) {
        match self.active.filter(|_| source.server_scoped()) {
            Some(id) => self.alerts.begin_in(id.0, source),
            None => self.alerts.begin(source),
        }
    }

    fn command(&mut self, command: client::Command) -> bool {
        match self.active {
            Some(id) => self.send_to(id, command),
            None => false,
        }
    }

    fn send_to(&mut self, id: hub::SessionId, command: client::Command) -> bool {
        let Some(sender) = self.session(id).and_then(|session| session.session.clone()) else {
            return false;
        };
        if sender.try_send(command).is_err() {
            self.alerts.raise(
                Source::Worker,
                "The audio worker is busy or stopped. Wait for it to finish, then reconnect.",
            );
            false
        } else {
            self.alerts.recover(Source::Worker);
            true
        }
    }

    fn toggle_mute(&mut self) {
        if self.live()
            && let Some(id) = self.active
        {
            let value = !self.server().muted;
            self.set_muted(id, value);
            self.play_sound(if value {
                sounds::Sound::MicOff
            } else {
                sounds::Sound::MicOn
            });
        }
    }

    fn toggle_deafen(&mut self) {
        if self.live()
            && let Some(id) = self.active
        {
            let value = !self.server().deafened;
            self.set_deafened(id, value);
            self.play_sound(if value {
                sounds::Sound::Deafened
            } else {
                sounds::Sound::Undeafened
            });
        }
    }

    /// Plays an event's sound, if sounds are on and that one isn't turned off. It waits for a
    /// sound already playing, unless it answers something you just did.
    /// Asks for attention while the window is in the background: the taskbar button flashes
    /// until it comes to the front, and `toast` shows as a system notification, one per
    /// `TOAST_GAP` at most. Does nothing while the window is in front.
    fn background_attention(&mut self, toast: Option<(String, String)>) {
        if self.window_focused {
            return;
        }
        window_place::flash(true);
        let Some((title, body)) = toast else {
            return;
        };
        let now = Instant::now();
        if self
            .last_toast
            .is_some_and(|at| now.saturating_duration_since(at) < TOAST_GAP)
        {
            return;
        }
        self.last_toast = Some(now);
        #[cfg(test)]
        {
            let _ = body;
            self.shown_toasts.push(title);
        }
        #[cfg(not(test))]
        notify::show(&title, &body);
    }

    fn play_sound(&mut self, sound: sounds::Sound) {
        if self.sound_cues && self.sounds.plays(sound) {
            #[cfg(test)]
            self.played_sounds.push(sound);
            self.send_sound(sound, sound.urgent());
        }
    }

    /// Plays the sound now, for hearing it in Settings, whether or not it is switched on. With
    /// no server's devices open, it plays on its own.
    fn preview_sound(&self, sound: sounds::Sound) {
        if !self.loopback && self.sessions.iter().any(|session| session.connected) {
            self.send_sound(sound, true);
            return;
        }
        let gain = self.sounds.gain();
        let Some(output) = self.output.clone().or_else(|| self.devices.output.clone()) else {
            return;
        };
        if gain == 0.0 || cfg!(test) {
            return;
        }
        let mut samples = sounds::samples(self.sounds.pack, sound, &self.sound_bank);
        samples.iter_mut().for_each(|sample| *sample *= gain);
        audio::play_alone(output, samples, self.playback_volume.clone());
    }

    /// Hands the sound, from the chosen pack, to the output at the chosen volume.
    fn send_sound(&self, sound: sounds::Sound, urgent: bool) {
        let gain = self.sounds.gain();
        if gain == 0.0 {
            return;
        }
        let mut samples = sounds::samples(self.sounds.pack, sound, &self.sound_bank);
        samples.iter_mut().for_each(|sample| *sample *= gain);
        self.hub.play(audio::Chime {
            id: sound as u16,
            urgent,
            samples,
        });
    }

    /// Takes one picture of each screen for its tile and of the chosen source, unless they
    /// have one already.
    fn take_share_preview(&mut self) -> Task {
        if !self.server().streaming.share_open || self.server().streaming.sharing.status.active {
            return Task::none();
        }
        let sharing = &self.server().streaming.sharing;
        let mut wanted: Vec<_> = sharing
            .sources
            .iter()
            .filter(|source| matches!(source.kind, streaming::publishing::SourceKind::Screen(_)))
            .cloned()
            .collect();
        if let Some(selected) = sharing.selected.clone()
            && !wanted.contains(&selected)
        {
            wanted.push(selected);
        }
        let tasks: Vec<Task> = wanted
            .into_iter()
            .filter(|source| self.server_mut().streaming.sharing.begin_preview(source))
            .map(|source| {
                let wanted = source.clone();
                Task::perform(
                    async move { streaming::share_still(wanted).await.ok().flatten() },
                    move |still| Message::SharePreview(source.clone(), still),
                )
            })
            .collect();
        Task::batch(tasks)
    }

    /// Keeps what this share used for the next one, on every server.
    fn start_share(
        &mut self,
        source: streaming::publishing::ShareSource,
        access: streaming::publishing::Access,
        remember: bool,
    ) -> Task {
        let sharing = &self.server().streaming.sharing;
        if !self.server().connected
            || sharing.status.active
            || sharing.loading
            || self.loopback
            || self.server().stopping
            || self.tab != Tab::Home
        {
            return Task::none();
        }
        self.begin_alert(Source::StreamPublish);
        let sharing = &self.server().streaming.sharing;
        let audio = if sharing.audio {
            let Some(device) = sharing
                .audio_device
                .clone()
                .or_else(|| self.devices.output.clone())
            else {
                self.raise_active(
                    Source::StreamPublish,
                    "Choose an output device for system audio",
                );
                return Task::none();
            };
            Some(device)
        } else {
            None
        };
        let quality = sharing.quality;
        let settings::Streaming {
            fast_capture,
            hardware_av1,
        } = self.stream_options;
        if self.command(client::Command::StartSharing(
            streaming::publishing::Options {
                source,
                audio,
                voice_output: self.output.clone(),
                quality,
                access,
                fast_capture,
                hardware_av1,
            },
        )) {
            self.server_mut().streaming.share_open = false;
            self.server_mut().streaming.sharing.previews.clear();
            if remember {
                self.remember_share();
            }
            self.ui.open = None;
        }
        Task::none()
    }

    /// Sends the dialog's audio and quality to the running share, if there is one.
    fn configure_share(&mut self) {
        let sharing = &self.server().streaming.sharing;
        if !sharing.status.active {
            return;
        }
        let audio = sharing
            .audio
            .then(|| {
                sharing
                    .audio_device
                    .clone()
                    .or_else(|| self.devices.output.clone())
            })
            .flatten();
        let live = streaming::LiveShare {
            audio,
            quality: sharing.quality,
        };
        self.command(client::Command::ConfigureSharing(live));
    }

    fn remember_share(&mut self) {
        let sharing = &self.server().streaming.sharing;
        let camera = sharing
            .selected
            .as_ref()
            .is_some_and(|s| s.kind.is_camera());
        let next = streaming::publishing::ShareDefaults {
            quality: sharing.quality,
            access: sharing.access,
            // A camera share turns system sound off without that being a preference.
            audio: if camera {
                self.share_defaults.audio
            } else {
                sharing.audio
            },
        };
        if next == self.share_defaults {
            return;
        }
        self.share_defaults = next;
        for session in &mut self.sessions {
            session.streaming.sharing.apply(next);
        }
        self.save_preference(
            |saved| saved.share = next,
            "Your sharing settings apply for now but were not saved",
        );
    }

    fn save_sounds(&mut self) {
        let sounds = self.sounds.clone();
        self.save_preference(
            |saved| saved.sounds = sounds,
            "That sound choice applies for now but was not saved",
        );
    }

    /// Stores the devices, playback volume and voice settings, and hands them to the devices
    /// that are open, so a change applies during a session too.
    fn save_audio(&mut self) {
        self.audio_unsaved = false;
        let input = self.input.as_ref().map(|device| device.id.clone());
        let output = self.output.as_ref().map(|device| device.id.clone());
        let volume = self.playback_volume.percent();
        let (processing, transmission) = (self.audio_processing, self.transmission);
        self.save_preference(
            |saved| {
                saved.input_device_id = input;
                saved.output_device_id = output;
                saved.playback_volume_percent = volume;
                saved.audio_processing = processing;
                saved.transmission = transmission;
            },
            "That audio choice applies for now but was not saved",
        );
        self.retune_audio();
    }

    /// Reopens whatever audio is running, a server's or the microphone test's, with the
    /// devices and voice settings chosen now.
    fn retune_audio(&mut self) {
        let (Some(input), Some(output)) = (self.input.clone(), self.output.clone()) else {
            return;
        };
        let retune = hub::Retune {
            input,
            output,
            processing: self.audio_processing,
            transmission: self.transmission,
        };
        if self.loopback {
            if let Some(id) = self.active {
                self.send_to(id, client::Command::Retune(retune));
            }
        } else {
            self.hub.retune(retune);
        }
    }

    /// The nickname typed in Settings differs from the saved one.
    fn nickname_edited(&self) -> bool {
        self.onboarding.is_none()
            && self
                .saved
                .as_ref()
                .is_some_and(|saved| saved.nickname != self.nickname.trim())
    }

    /// Saves the nickname typed in Settings, or says under its field why it can't be kept.
    fn commit_nickname(&mut self) {
        if !self.nickname_edited() {
            self.nickname_error = None;
            return;
        }
        let nickname = self.nickname.trim().to_owned();
        if nickname.is_empty() {
            self.nickname_error = Some("Enter a nickname.".into());
            return;
        }
        self.nickname_error = None;
        self.save_preference(
            |saved| saved.nickname = nickname,
            "Your nickname applies for now but was not saved",
        );
    }

    /// Saves what sliders and the nickname field held back while they were being changed.
    fn flush_unsaved(&mut self) {
        if std::mem::take(&mut self.sounds_unsaved) {
            self.save_sounds();
        }
        if self.audio_unsaved {
            self.save_audio();
        }
        self.commit_nickname();
    }

    /// What the voice threads play each person at. Everyone unset stays out of the map.
    fn sync_user_gains(&self) {
        *self.user_gains.lock().unwrap() = self
            .user_audio
            .iter()
            .map(|(uid, audio)| (uid.clone(), audio.gain()))
            .collect();
    }

    fn user_audio(&self, uid: &str) -> settings::UserAudio {
        self.user_audio.get(uid).copied().unwrap_or_default()
    }

    /// Applies a change to one person, forgetting them again once they're back to normal.
    fn change_user_audio(&mut self, uid: String, change: impl FnOnce(&mut settings::UserAudio)) {
        let mut audio = self.user_audio(&uid);
        change(&mut audio);
        if audio == settings::UserAudio::default() {
            self.user_audio.remove(&uid);
        } else {
            self.user_audio.insert(uid, audio);
        }
        self.sync_user_gains();
    }

    /// Saves the streams' layout and details toggle as they are on screen now.
    fn save_stream_view(&mut self) {
        let view = settings::StreamView {
            beside: self.server().streaming.strip == streaming::Strip::Beside,
            details: self.server().streaming.show_stats,
        };
        if view == self.stream_view {
            return;
        }
        self.stream_view = view;
        self.save_preference(
            |saved| saved.stream_view = view,
            "That stream layout applies for now but was not saved",
        );
    }

    fn save_stream_volumes(&mut self) {
        self.stream_volumes_changed = None;
        let volumes = self.stream_volumes.clone();
        self.save_preference(
            |saved| saved.stream_volumes = volumes,
            "That stream volume applies for now but was not saved",
        );
    }

    fn save_user_audio(&mut self) {
        let audio = self.user_audio.clone();
        self.save_preference(
            |saved| saved.user_audio = audio,
            "That volume applies for now but was not saved",
        );
    }

    fn set_muted(&mut self, id: hub::SessionId, value: bool) {
        self.alerts.begin_in(id.0, Source::Controls);
        if self.send_to(id, client::Command::Mute(value))
            && let Some(session) = self.session_mut(id)
        {
            session.muted = value;
            session.controls_pending = true;
        }
    }

    fn set_deafened(&mut self, id: hub::SessionId, value: bool) {
        self.alerts.begin_in(id.0, Source::Controls);
        if self.send_to(id, client::Command::Deafen(value))
            && let Some(session) = self.session_mut(id)
        {
            session.deafened = value;
            session.controls_pending = true;
        }
    }

    fn load_settings(&mut self, path: PathBuf) {
        match settings::load(&path) {
            Ok(Some(saved)) => {
                self.apply_settings(saved);
                self.onboarding = None;
            }
            Ok(None) => self.begin_setup(),
            Err(error) => {
                self.open_settings(SettingsPage::File);
                self.alerts.raise(Source::SettingsLoad,format!("{error:#}. The file was not changed. Repair it and Reload, or Save to replace it with the values shown."));
            }
        }
        self.settings_path = Some(path);
    }

    /// Looks for a TeamSpeak 6 install to offer. Nothing is read until the user asks.
    fn find_ts6(&mut self) {
        self.ts6_path = ts6_import::path().filter(|path| path.exists());
        self.ts6 = None;
    }

    /// Fills in setup from TeamSpeak 6; every value stays editable on its step.
    fn apply_ts6(&mut self, imported: &ts6_import::Imported) {
        if self.nickname.trim().is_empty()
            && let Some(nickname) = &imported.nickname
        {
            self.nickname = nickname.clone();
        }
        if self.address.trim().is_empty()
            && let Some(first) = imported.servers.first()
        {
            self.address = first.clone();
        }
        let find = |devices: &[audio::DeviceChoice], name: &Option<String>| {
            let name = name.as_deref()?;
            devices.iter().find(|device| device.name == name).cloned()
        };
        if let Some(input) = find(&self.devices.inputs, &imported.input) {
            self.input = Some(input);
        }
        if let Some(output) = find(&self.devices.outputs, &imported.output) {
            self.output = Some(output);
        }
        if imported.push_to_talk {
            self.transmission.mode = transmission::Mode::PushToTalk;
            self.transmission.ptt_release_ms = imported.ptt_release_ms;
        }
    }

    fn begin_setup(&mut self) {
        self.find_ts6();
        self.onboarding = Some(onboarding::Step::Username);
        self.onboarding_error = None;
        self.saved = None;
        self.nickname.clear();
        self.address.clear();
        self.password.clear();
        self.servers.clear();
        self.server_editor = None;
        self.tab = Tab::Home;
        self.close_settings();
        self.sync_page_visibility();
    }

    /// Stops listening for a shortcut and gives the saved ones back to the system.
    fn end_recording(&mut self) {
        if self.key_capture.take().is_none() {
            return;
        }
        self.ui.key_capture.focused = false;
        let keys = self.keybinds.clone();
        if let Err(error) = self.configure_shortcuts(&keys) {
            self.alerts.raise(Source::Shortcuts, format!("{error:#}"));
        }
    }

    fn open_settings(&mut self, page: SettingsPage) {
        self.end_recording();
        self.settings_page = Some(page);
        self.settings_recent = page;
        self.settings_query.clear();
        self.ui.open = None;
    }

    fn close_settings(&mut self) {
        self.end_recording();
        self.flush_unsaved();
        // An empty nickname isn't kept: the saved one comes back.
        if self.nickname_error.take().is_some()
            && let Some(saved) = &self.saved
        {
            self.nickname = saved.nickname.clone();
        }
        self.settings_page = None;
        self.settings_query.clear();
        self.server_editor = None;
        self.ui.open = None;
        self.ui.chat_clear_armed = false;
        self.chat_seen();
    }

    /// The session running or starting for this saved address, if any.
    fn session_at(&self, address: &str) -> Option<&ServerSession> {
        let address = address.trim();
        self.sessions
            .iter()
            .find(|session| !self.loopback && session.address.trim() == address)
    }

    /// Whether the server at `index` is in use, so it can't be edited or removed.
    fn server_in_use(&self, index: usize) -> bool {
        self.servers
            .get(index)
            .is_some_and(|server| self.session_at(&server.address).is_some())
    }

    /// Adds or updates the dialog's server and saves the list.
    fn submit_server(&mut self) {
        let Some(editor) = self.server_editor.clone() else {
            return;
        };
        let address = editor.address.trim().to_owned();
        let error = if address.is_empty() {
            Some("Enter the server's address.")
        } else if editor.index.is_some_and(|index| self.server_in_use(index)) {
            Some("Disconnect before editing this server.")
        } else if self
            .servers
            .iter()
            .enumerate()
            .any(|(i, server)| Some(i) != editor.index && server.address.trim() == address)
        {
            Some("This server is already in the list.")
        } else {
            None
        };
        if let Some(error) = error {
            if let Some(open) = &mut self.server_editor {
                open.error = Some(error.into());
            }
            return;
        }
        let server = settings::Server {
            name: editor.name.trim().into(),
            address: address.clone(),
            auto_connect: editor
                .index
                .and_then(|index| self.servers.get(index))
                .is_some_and(|server| server.auto_connect),
        };
        let mut servers = self.servers.clone();
        match editor.index {
            Some(index) if index < servers.len() => {
                let old = std::mem::replace(&mut servers[index], server);
                if old.address.trim() != address {
                    self.server_passwords.remove(old.address.trim());
                }
            }
            _ => servers.push(server),
        }
        if editor.password.is_empty() {
            self.server_passwords.remove(&address);
        } else {
            self.server_passwords.insert(address, editor.password);
        }
        self.set_servers(servers);
        self.close_server_editor();
    }

    /// Closes the Add or Edit Server dialog, and Settings with it if it opened only for it.
    fn close_server_editor(&mut self) {
        if self.server_editor.take().is_some_and(|editor| editor.alone) {
            self.close_settings();
        }
    }

    /// Removes the dialog's server, unless it is the last one or in use.
    fn remove_server(&mut self) {
        let Some(index) = self.server_editor.as_ref().and_then(|editor| editor.index) else {
            return;
        };
        if self.servers.len() <= 1 || self.server_in_use(index) {
            return;
        }
        // The first press only asks, as Clear History does.
        if let Some(editor) = &mut self.server_editor
            && !std::mem::replace(&mut editor.remove_armed, true)
        {
            return;
        }
        let mut servers = self.servers.clone();
        let removed = servers.remove(index);
        self.server_passwords.remove(removed.address.trim());
        self.set_servers(servers);
        self.close_server_editor();
    }

    /// Connects to the servers set to connect at launch; the first one is shown.
    fn auto_connect(&mut self) -> Task {
        let mut tasks = Vec::new();
        let mut shown = None;
        for index in self.auto_connect_servers() {
            let address = self.servers[index].address.trim().to_owned();
            tasks.push(self.update(Message::Connect(index)));
            if let Some(id) = self.session_at(&address).map(|session| session.id)
                && let Some(session) = self.session_mut(id)
            {
                session.activate_on_connect = false;
                shown.get_or_insert(id);
            }
        }
        if shown.is_some() {
            self.activate(shown);
        }
        Task::batch(tasks)
    }

    fn auto_connect_servers(&self) -> Vec<usize> {
        (0..self.servers.len())
            .filter(|index| self.servers[*index].auto_connect)
            .collect()
    }

    /// Uses `servers` and saves only the list, leaving other unsaved edits alone.
    fn set_servers(&mut self, servers: Vec<settings::Server>) {
        self.servers = servers.clone();
        if self.idle()
            && let Some(first) = servers.first()
        {
            self.address = first.address.clone();
        }
        self.save_preference(
            |saved| saved.servers = servers,
            "The server list applies for now but was not saved",
        );
    }

    /// Settings and the setup step show live status; re-read it while either is open, so a
    /// change made in the OS's privacy settings appears on return without a restart.
    fn refresh_visible_permissions(&mut self) {
        let showing = match self.onboarding {
            Some(step) => step == onboarding::Step::Permissions,
            None => self.settings_page == Some(SettingsPage::Permissions),
        };
        if showing {
            self.permissions.refresh();
        }
    }

    fn sync_page_visibility(&self) {
        let normal = self.onboarding.is_none();
        // Statistics covers the server view without ending its streams: your share and the
        // ones you watch keep running and are there again on the way back.
        self.server().streaming.visible.store(
            normal && matches!(self.tab, Tab::Home | Tab::Statistics),
            Ordering::Relaxed,
        );
        self.channel_health_visible.store(
            normal && matches!(self.tab, Tab::Home | Tab::Statistics),
            Ordering::Relaxed,
        );
    }

    fn apply_settings(&mut self, saved: settings::Settings) {
        let first = saved
            .servers
            .first()
            .map(|server| server.address.clone())
            .unwrap_or_default();
        if self.idle() && self.address.trim() != first.trim() {
            self.password.clear();
        }
        self.input = match &saved.input_device_id {
            Some(id) => self
                .devices
                .inputs
                .iter()
                .find(|device| &device.id == id)
                .cloned(),
            None => self.devices.input.clone(),
        };
        self.output = match &saved.output_device_id {
            Some(id) => self
                .devices
                .outputs
                .iter()
                .find(|device| &device.id == id)
                .cloned(),
            None => self.devices.output.clone(),
        };
        self.saved = Some(saved.clone());
        // A running session keeps the address it connected to.
        if self.idle() {
            self.address = first;
        }
        self.servers = saved.servers;
        self.nickname = saved.nickname;
        self.playback_volume
            .set_percent(saved.playback_volume_percent);
        self.audio_processing = saved.audio_processing;
        self.transmission = saved.transmission;
        self.keybinds = saved.keybinds.clone();
        self.keybind_draft = saved.keybinds;
        self.footer_hidden = saved.footer_hidden;
        self.log_pane = saved.log_pane;
        self.updater.auto_download = saved.auto_download_updates;
        self.appearance = saved.appearance;
        self.user_audio = saved.user_audio;
        self.sync_user_gains();
        self.stream_volumes = saved.stream_volumes;
        self.sound_cues = saved.sound_cues;
        self.sounds = saved.sounds;
        self.stream_options = saved.streaming;
        self.stream_view = saved.stream_view;
        for session in &mut self.sessions {
            apply_stream_view(&mut session.streaming, saved.stream_view);
        }
        self.chat_options = saved.chat;
        self.share_defaults = if saved.share.quality.validate().is_ok() {
            saved.share.upgraded()
        } else {
            Default::default()
        };
        let defaults = self.share_defaults;
        for session in &mut self.sessions {
            session.streaming.sharing.apply(defaults);
        }
        self.theme_library = themes::Library::new(&saved.custom_themes);
        self.custom_themes = saved.custom_themes;
    }

    fn edited_settings(&self) -> settings::Settings {
        settings::Settings {
            servers: self.setup_servers(),
            server_address: String::new(),
            nickname: self.nickname.trim().into(),
            playback_volume_percent: self.playback_volume.percent(),
            audio_processing: self.audio_processing,
            transmission: self.transmission,
            keybinds: shortcuts::Keybinds {
                toggle_mute: self.keybind_draft.toggle_mute.trim().into(),
                toggle_deafen: self.keybind_draft.toggle_deafen.trim().into(),
                push_to_talk: self.keybind_draft.push_to_talk.trim().into(),
            },
            input_device_id: self.input.as_ref().map(|device| device.id.clone()),
            output_device_id: self.output.as_ref().map(|device| device.id.clone()),
            footer_hidden: self.footer_hidden,
            log_pane: self.log_pane,
            auto_download_updates: self.updater.auto_download,
            appearance: self.appearance.clone(),
            custom_themes: self.custom_themes.clone(),
            user_audio: self.user_audio.clone(),
            stream_volumes: self.stream_volumes.clone(),
            sound_cues: self.sound_cues,
            sounds: self.sounds.clone(),
            share: self.share_defaults,
            streaming: self.stream_options,
            stream_view: self.stream_view,
            chat: self.chat_options,
            skip_install_prompt: self
                .saved
                .as_ref()
                .is_some_and(|saved| saved.skip_install_prompt),
            close_button: self.close_button(),
            window: self.saved.as_ref().and_then(|saved| saved.window),
        }
    }

    /// The server list to save. First-run setup has no list yet, so its one server becomes the
    /// first; setup run again from Settings puts a new address at the front of the list.
    fn setup_servers(&self) -> Vec<settings::Server> {
        let mut servers = self.chosen_servers();
        if self.onboarding.is_some()
            && let Some(imported) = &self.ts6
        {
            for address in &imported.servers {
                if !servers
                    .iter()
                    .any(|server| server.address.trim() == address)
                {
                    servers.push(settings::Server::new(address));
                }
            }
        }
        servers
    }

    fn chosen_servers(&self) -> Vec<settings::Server> {
        let address = self.address.trim();
        if self.servers.is_empty() {
            return vec![settings::Server::new(address)];
        }
        let mut servers = self.servers.clone();
        if self.onboarding.is_some()
            && !address.is_empty()
            && !servers
                .iter()
                .any(|server| server.address.trim() == address)
        {
            servers.insert(0, settings::Server::new(address));
        }
        servers
    }

    /// Hides or shows the footer and stores only that choice, leaving unsaved edits alone.
    fn set_footer_hidden(&mut self, hidden: bool) {
        self.footer_hidden = hidden;
        self.save_preference(
            |saved| saved.footer_hidden = hidden,
            "The footer choice applies for now but was not saved",
        );
    }

    /// Stores the server log's size and fold, leaving unsaved edits alone.
    fn save_log_pane(&mut self) {
        let pane = self.log_pane;
        self.save_preference(
            |saved| saved.log_pane = pane,
            "The server log's size applies for now but was not saved",
        );
    }

    /// Shows the folder holding the settings file, where custom themes are added.
    fn open_settings_folder(&mut self) {
        let Some(folder) = self.settings_path.as_deref().and_then(Path::parent) else {
            return;
        };
        let opener = if cfg!(target_os = "macos") {
            "open"
        } else if cfg!(windows) {
            "explorer"
        } else {
            "xdg-open"
        };
        match Command::new(opener).arg(folder).spawn() {
            // Waited on elsewhere so it can't linger as a zombie.
            Ok(mut child) => {
                std::thread::spawn(move || child.wait());
            }
            Err(error) => self.alerts.raise(
                Source::SettingsLoad,
                format!("Couldn't open the settings folder: {error}"),
            ),
        }
    }

    /// Changes the colour scheme or themes and stores only that choice.
    fn set_appearance(&mut self, appearance: settings::Appearance) {
        self.appearance = appearance.clone();
        self.save_preference(
            |saved| saved.appearance = appearance,
            "The appearance choice applies for now but was not saved",
        );
    }

    /// Stores one choice that applies at once in the saved settings, leaving other unsaved edits
    /// alone. Before setup has saved anything, the choice is kept for the first save.
    fn save_preference(&mut self, change: impl FnOnce(&mut settings::Settings), failed: &str) {
        let Some(mut saved) = self.saved.clone() else {
            return;
        };
        change(&mut saved);
        let result = self
            .settings_path
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Settings path is unavailable"))
            .and_then(|path| settings::save(path, &saved));
        match result {
            Ok(()) => {
                self.saved = Some(saved);
                self.alerts.recover(Source::SettingsSave);
            }
            Err(error) => self
                .alerts
                .raise(Source::SettingsSave, format!("{failed}: {error:#}")),
        }
    }

    fn update_software(&mut self, event: updater::Event) -> Task {
        if let updater::Event::AutoDownload(on) = event {
            self.save_preference(
                |saved| saved.auto_download_updates = on,
                "The update choice applies for now but was not saved",
            );
        }
        let wrap = |event| Message::Updater(event);
        match self.updater.handle(event, Instant::now()) {
            updater::Effect::None => Task::none(),
            updater::Effect::Check { running, package } => {
                Task::perform(updater::check(running, package), move |result| {
                    wrap(updater::Event::Checked(result))
                })
            }
            updater::Effect::Download {
                generation,
                release,
                package,
                cancel,
            } => Task::stream(move |emit| {
                let result =
                    updater::block_on(updater::download(&release, package, &cancel, |progress| {
                        emit(wrap(updater::Event::Progress {
                            generation,
                            progress,
                        }))
                    }));
                wrap(updater::Event::Downloaded { generation, result })
            }),
            updater::Effect::Install { staged, package } => Task::perform(
                async move { updater::install(&staged, package) },
                move |result| wrap(updater::Event::Installed(result)),
            ),
            // Leave as the window's close button does, so a session ends cleanly first.
            updater::Effect::Exit => {
                if let Some(path) = self.settings_path.as_deref().map(resume::path)
                    && let Err(error) =
                        resume::save(&path, &self.resume_snapshot(), SystemTime::now())
                {
                    tracing::warn!(
                        "The update restarts without returning to this session: {error}"
                    );
                }
                self.update(Message::Quit)
            }
        }
    }

    /// The page, servers and channels to return to after an update's restart.
    fn resume_snapshot(&self) -> resume::Resume {
        let servers = self
            .sessions
            .iter()
            .filter(|session| session.connected && !self.loopback && !session.stopping)
            .map(|session| {
                let channel = session.state.current.as_ref().map(|channel| channel.id);
                resume::Server {
                    address: session.address.clone(),
                    password: session.password.clone(),
                    channel,
                    channel_password: session
                        .channel_secret
                        .as_ref()
                        .filter(|(id, _)| Some(*id) == channel)
                        .map(|(_, password)| password.clone())
                        .unwrap_or_default(),
                    muted: session.muted,
                    deafened: session.deafened,
                }
            })
            .collect();
        resume::Resume::new(
            match self.tab {
                _ if self.settings_page.is_some() => resume::Page::Settings,
                Tab::Home => resume::Page::Home,
                Tab::Statistics => resume::Page::Statistics,
            },
            servers,
            self.active
                .and_then(|id| self.session(id))
                .filter(|session| session.connected && !session.stopping)
                .map(|session| session.address.clone()),
        )
    }

    fn resume(&mut self) -> Task {
        let Some(resume) = self.resume.take() else {
            return Task::none();
        };
        let mut tasks = vec![self.update(Message::ShowTab(match resume.page {
            resume::Page::Statistics => Tab::Statistics,
            resume::Page::Home | resume::Page::Settings => Tab::Home,
        }))];
        if resume.page == resume::Page::Settings {
            self.open_settings(SettingsPage::Updates);
        }
        let active = resume.active.as_deref().map(str::trim).map(str::to_owned);
        for saved in resume.servers {
            // Only a saved server is reconnected, with the password it was joined with.
            let Some(index) = self
                .servers
                .iter()
                .position(|server| server.address.trim() == saved.address.trim())
            else {
                continue;
            };
            let address = self.servers[index].address.trim().to_owned();
            if saved.password.is_empty() {
                self.server_passwords.remove(&address);
            } else {
                self.server_passwords
                    .insert(address.clone(), saved.password.clone());
            }
            tasks.push(self.update(Message::Connect(index)));
            if let Some(id) = self.session_at(&address).map(|session| session.id)
                && let Some(session) = self.session_mut(id)
            {
                session.activate_on_connect = false;
                session.resume = Some(saved);
            }
        }
        if let Some(id) = active
            .as_deref()
            .and_then(|address| self.session_at(address))
            .map(|session| session.id)
        {
            self.activate(Some(id));
        }
        Task::batch(tasks)
    }

    fn save_settings(&mut self) -> bool {
        self.settings_message = None;
        self.alerts.begin(Source::SettingsSave);
        self.alerts.begin(Source::Shortcuts);
        let saved = self.edited_settings();
        if let Err(error) = saved.validate() {
            self.alerts
                .raise(Source::SettingsSave, format!("{error:#}"));
            return false;
        }
        if let Err(error) = self.configure_shortcuts(&saved.keybinds) {
            self.alerts.raise(Source::Shortcuts, format!("{error:#}"));
            return false;
        }
        self.keybinds = saved.keybinds.clone();
        let result = self
            .settings_path
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Settings path is unavailable"))
            .and_then(|path| settings::save(path, &saved));
        match result {
            Ok(()) => {
                self.apply_settings(saved);
                self.alerts.recover(Source::SettingsSave);
                self.alerts.recover(Source::SettingsLoad);
                true
            }
            Err(error) => {
                self.alerts.raise(
                    Source::SettingsSave,
                    format!("Shortcuts applied for this session, but settings could not be saved: {error:#}"),
                );
                false
            }
        }
    }

    /// Applies the shortcuts as recorded and stores only them.
    fn save_keybinds(&mut self) {
        self.alerts.begin(Source::Shortcuts);
        let keys = shortcuts::Keybinds {
            toggle_mute: self.keybind_draft.toggle_mute.trim().into(),
            toggle_deafen: self.keybind_draft.toggle_deafen.trim().into(),
            push_to_talk: self.keybind_draft.push_to_talk.trim().into(),
        };
        if let Err(error) = keys.parse().and_then(|_| self.configure_shortcuts(&keys)) {
            self.alerts.raise(Source::Shortcuts, format!("{error:#}"));
            return;
        }
        self.keybinds = keys.clone();
        self.save_preference(
            |saved| saved.keybinds = keys,
            "That shortcut applies for now but was not saved",
        );
    }

    fn configure_shortcuts(&mut self, keys: &shortcuts::Keybinds) -> anyhow::Result<()> {
        if !shortcuts::supported() {
            self.shortcut_error =
                Some("Global shortcuts need X11; native Wayland is not supported.".into());
        }
        if let Some(shortcuts) = &mut self.shortcuts
            && shortcuts::supported()
        {
            if let Err(error) = shortcuts.configure(keys) {
                if !shortcuts.is_active() {
                    self.shortcut_error = Some(format!("{error:#}"));
                }
                return Err(error);
            }
            self.shortcut_error = None;
            self.alerts.recover(Source::Shortcuts);
            self.report_taken_shortcuts(keys);
        }
        Ok(())
    }

    /// Says which shortcuts another app holds, or clears that once none is.
    fn report_taken_shortcuts(&mut self, keys: &shortcuts::Keybinds) {
        let taken = self
            .shortcuts
            .as_ref()
            .map(shortcuts::Shortcuts::taken)
            .unwrap_or_default();
        self.taken_shortcuts = taken.clone();
        if taken.is_empty() {
            self.alerts.recover(Source::Shortcuts);
            return;
        }
        let one = taken.len() == 1;
        let shortcuts: Vec<String> = taken
            .iter()
            .map(|binding| keys.get(*binding).trim().to_owned())
            .collect();
        let actions: Vec<String> = taken
            .iter()
            .map(|binding| {
                match binding {
                    shortcuts::Binding::Mute => "mute",
                    shortcuts::Binding::Deafen => "deafen",
                    shortcuts::Binding::PushToTalk => "push-to-talk",
                }
                .to_owned()
            })
            .collect();
        self.alerts.begin(Source::Shortcuts);
        self.alerts.raise(
            Source::Shortcuts,
            format!(
                "Another app is using {}, so the {} {} off. Flexspeak takes {} as soon as {} free.",
                spoken_list(&shortcuts),
                spoken_list(&actions),
                if one { "shortcut is" } else { "shortcuts are" },
                if one { "it" } else { "them" },
                if one { "it's" } else { "they're" },
            ),
        );
    }

    fn start(&mut self, loopback: bool) -> Task {
        // A microphone test is exclusive with server connections, in both directions.
        if (loopback && !self.idle())
            || (!loopback && self.loopback && !self.idle())
            || self.permissions.pending.is_some()
            || (!loopback && self.onboarding.is_some())
        {
            return Task::none();
        }
        if self.transmission.mode == transmission::Mode::PushToTalk
            && (!shortcuts::supported()
                || !self
                    .shortcuts
                    .as_ref()
                    .is_some_and(shortcuts::Shortcuts::has_ptt))
        {
            self.alerts.raise(
                Source::Shortcuts,
                if shortcuts::supported() {
                    "Push-to-talk needs a working global shortcut. Set one in Settings, or choose another transmission mode."
                } else {
                    "Push-to-talk needs a global shortcut, which native Wayland doesn't allow. Choose another transmission mode."
                },
            );
            return Task::none();
        }
        if self.transmission.mode == transmission::Mode::VoiceActivation
            && !audio_processing::AVAILABLE
        {
            self.alerts.raise(Source::Audio, "Speech detection is available on Mac and Windows. Choose continuous transmission or push-to-talk.");
            return Task::none();
        }
        let (Some(input), Some(output)) = (self.input.clone(), self.output.clone()) else {
            self.alerts.raise(
                Source::Audio,
                "Choose an available microphone and playback device first.",
            );
            return Task::none();
        };
        if !loopback && (self.address.trim().is_empty() || self.nickname.trim().is_empty()) {
            self.raise_active(Source::Connection, "Enter a server address and nickname.");
            return Task::none();
        }
        let id = hub::SessionId(self.next_session);
        self.next_session += 1;
        let address = self.address.trim().to_owned();
        let name = self
            .servers
            .iter()
            .find(|server| server.address.trim() == address)
            .map(|server| server.name.clone())
            .unwrap_or_default();
        let surfaces = self.home.streaming.surfaces.clone();
        let visible = self.home.streaming.visible.clone();
        let config = client::Config {
            id,
            user_gains: self.user_gains.clone(),
            hub: self.hub.clone(),
            playback_volume: self.playback_volume.clone(),
            audio_processing: self.audio_processing,
            transmission: self.transmission,
            ptt: self.ptt.clone(),
            channel_health_visible: self.channel_health_visible.clone(),
            subscribe_all: self.subscribe_all.clone(),
            streaming_visible: visible.clone(),
            video_surfaces: surfaces.clone(),
            address: address.clone(),
            nickname: self.nickname.trim().into(),
            password: self.password.clone(),
            input,
            output,
        };
        if self.idle() {
            self.ptt.store(false, Ordering::Release);
            self.transmitting = false;
        }
        self.alerts.begin(Source::Audio);
        self.alerts.begin(Source::Worker);
        match client::start(config, loopback) {
            Ok((sender, events)) => {
                let mut session =
                    ServerSession::new(id, name, address, self.password.clone(), surfaces, visible);
                session.session = Some(sender);
                session.controls_pending = !loopback;
                session.status = if loopback {
                    "Opening local microphone test…"
                } else {
                    "Connecting…"
                }
                .into();
                // With a server on screen, it stays there until this one has connected.
                session.activate_on_connect = self.active.is_some();
                self.loopback = loopback;
                session.streaming.sharing.apply(self.share_defaults);
                apply_stream_view(&mut session.streaming, self.stream_view);
                self.sessions.push(session);
                if self.active.is_none() {
                    self.activate(Some(id));
                }
                Task::Events(id, events)
            }
            Err(error) => {
                self.alerts.raise(
                    Source::Audio,
                    format!("Cannot start audio worker: {error:#}"),
                );
                Task::none()
            }
        }
    }

    fn update(&mut self, message: Message) -> Task {
        // The nickname field keeps its text when it loses the keyboard.
        if !self.ui.nickname.focused && self.nickname_edited() && self.nickname_error.is_none() {
            self.commit_nickname();
        }
        let task = self.update_inner(message);
        // Whatever the message added to a conversation goes to the history on disk.
        self.chat_persist();
        task
    }

    fn update_inner(&mut self, message: Message) -> Task {
        let _trace = profiler::scope(profiler::Category::Ui, "ui.update");
        match message {
            Message::SetupNext => {
                self.end_recording();
                if !self.idle() || self.permissions.pending.is_some() {
                    return Task::none();
                }
                if let Some(step) = self.onboarding {
                    if let Err(error) = step.validate(&self.nickname, &self.address) {
                        self.onboarding_error = Some(error.into());
                        return Task::none();
                    }
                    self.onboarding_error = None;
                    if let Some(next) = step.next() {
                        self.onboarding = Some(next);
                        self.refresh_visible_permissions();
                    } else if self.save_settings() {
                        if !self.password.is_empty() {
                            self.server_passwords
                                .insert(self.address.trim().into(), self.password.clone());
                        }
                        self.onboarding = None;
                        self.redoing_setup = false;
                        self.tab = Tab::Home;
                        self.sync_page_visibility();
                    }
                }
            }
            Message::ImportTs6 => {
                let Some(path) = self.ts6_path.clone() else {
                    return Task::none();
                };
                match ts6_import::read(&path) {
                    Ok(imported) => {
                        self.apply_ts6(&imported);
                        self.ts6 = Some(imported);
                        self.onboarding_error = None;
                    }
                    // TeamSpeak's format isn't documented, so this can happen after its updates.
                    Err(_) => {
                        self.onboarding_error = Some(
                            "Couldn't read your TeamSpeak 6 settings. You can set things up here instead."
                                .into(),
                        );
                    }
                }
            }
            Message::DropTs6Server(address) => {
                if let Some(imported) = &mut self.ts6 {
                    imported.servers.retain(|server| *server != address);
                }
            }
            Message::SetupBack => {
                self.end_recording();
                if self.idle()
                    && self.permissions.pending.is_none()
                    && let Some(previous) = self.onboarding.and_then(onboarding::Step::previous)
                {
                    self.onboarding = Some(previous);
                    self.onboarding_error = None;
                    self.refresh_visible_permissions();
                }
            }
            Message::RequestPermission(kind) => {
                if self.idle() && self.permissions.pending.is_none() {
                    self.permissions.pending = Some(kind);
                    self.permissions.messages[kind.index()] =
                        Some("Waiting for the operating system…".into());
                    return Task::perform(
                        permissions::request(kind, self.output.clone()),
                        move |result| {
                            Message::PermissionDone(
                                kind,
                                result.map_err(|error| format!("{error:#}")),
                            )
                        },
                    );
                }
            }
            Message::PermissionDone(kind, result) => {
                if self.permissions.pending == Some(kind) {
                    self.permissions.pending = None;
                    self.permissions.messages[kind.index()] = Some(match result {
                        Ok(message) => message,
                        Err(error) => error,
                    });
                    self.permissions.refresh();
                }
            }
            Message::OpenPermissionSettings(kind) => {
                if self.permissions.pending.is_none() && self.idle() {
                    self.permissions.pending = Some(kind);
                    return Task::perform(
                        async move {
                            tokio::task::spawn_blocking(move || permissions::open_settings(kind))
                                .await
                                .map_err(|error| error.to_string())?
                                .map(|_| {
                                    "Privacy settings opened. Return here after reviewing access."
                                        .to_string()
                                })
                                .map_err(|error| error.to_string())
                        },
                        move |result| Message::PermissionDone(kind, result),
                    );
                }
            }
            Message::DismissAlert(id) => self.alerts.dismiss(id),
            Message::Shortcut(action) => {
                match action {
                    shortcuts::Action::Mute => self.toggle_mute(),
                    shortcuts::Action::Deafen => self.toggle_deafen(),
                    shortcuts::Action::PushToTalk(_) => {}
                }
            }
            Message::PlaybackVolume(percent) => {
                self.playback_volume.set_percent(percent);
                // Saved as Settings closes or the app quits, not at every step of the slider.
                self.audio_unsaved = true;
            }
            Message::AudioProcessing(options) => {
                // Without WebRTC only typing suppression applies; refuse the other effects.
                if options.for_session(false) == options {
                    self.audio_processing = options;
                    self.save_audio();
                }
            }
            Message::Transmission(settings) => {
                if settings.validate().is_ok() {
                    let mode_changed = settings.mode != self.transmission.mode;
                    self.transmission = settings;
                    if mode_changed {
                        self.save_audio();
                    } else {
                        // A slider: the running audio follows it, the file waits for the end.
                        self.audio_unsaved = true;
                        self.retune_audio();
                    }
                }
            }
            Message::RecordShortcut(binding) => {
                if self.key_capture.is_none()
                    && let Some(shortcuts) = &mut self.shortcuts
                {
                    shortcuts.suspend();
                }
                self.key_capture = Some(KeyCapture {
                    binding,
                    error: None,
                });
                self.ui.key_capture.focused = true;
                self.settings_message = None;
            }
            Message::CancelRecording => self.end_recording(),
            Message::Recorded(recorded) => {
                let Some(capture) = &mut self.key_capture else {
                    return Task::none();
                };
                let binding = capture.binding;
                let shortcut = match recorded {
                    shortcuts::Recorded::Ignored => return Task::none(),
                    shortcuts::Recorded::Cancel => {
                        self.end_recording();
                        return Task::none();
                    }
                    // Any shortcut may be left unset.
                    shortcuts::Recorded::Clear => String::new(),
                    shortcuts::Recorded::Shortcut(shortcut) => shortcut,
                };
                let next = self.keybind_draft.with(binding, shortcut);
                if let Err(error) = next.parse() {
                    capture.error = Some(format!("{error:#}"));
                    return Task::none();
                }
                self.keybind_draft = next;
                self.end_recording();
                // Setup saves everything together at its last step.
                if self.onboarding.is_none() {
                    self.save_keybinds();
                }
            }
            Message::ReloadSettings => {
                if self.idle()
                    && let Some(path) = self.settings_path.clone()
                {
                    self.alerts.begin(Source::SettingsLoad);
                    match settings::load(&path) {
                        Ok(Some(saved)) => match self.configure_shortcuts(&saved.keybinds) {
                            Ok(()) => {
                                self.apply_settings(saved);
                                self.alerts.recover(Source::SettingsLoad);
                                self.settings_message = Some("Settings reloaded".into());
                            }
                            Err(error) => {
                                self.alerts.raise(Source::Shortcuts, format!("{error:#}"))
                            }
                        },
                        Ok(None) => self.begin_setup(),
                        Err(error) => self
                            .alerts
                            .raise(Source::SettingsLoad, format!("{error:#}")),
                    }
                }
            }
            Message::CopyErrorLog => return Task::Copy(self.alerts.history_text()),
            Message::ClearErrorLog => self.alerts.clear_history(),
            Message::CopySettingsPath => {
                if let Some(path) = &self.settings_path {
                    return Task::Copy(path.display().to_string());
                }
            }
            Message::ShowTab(tab) => {
                if self.onboarding.is_some() {
                    return Task::none();
                }
                if tab != Tab::Statistics && self.server().expanded_client.is_some() {
                    self.select_client(None);
                }
                self.tab = tab;
                self.chat_seen();
                self.sync_page_visibility();
                self.refresh_visible_permissions();
            }
            Message::OpenSettings(page) => {
                if self.onboarding.is_none() {
                    self.open_settings(page);
                }
            }
            Message::CloseSettings => self.close_settings(),
            Message::SettingsSearch(query) => {
                if self.settings_page.is_some() {
                    self.settings_query = query;
                }
            }
            Message::RefreshStreams => {
                self.begin_alert(Source::StreamDiscovery);
                self.command(client::Command::RefreshStreams);
            }
            Message::WatchStream(id) => {
                if self.live() && self.tab == Tab::Home {
                    self.begin_alert(Source::StreamWatch);
                    self.begin_alert(Source::StreamFrame);
                    self.command(client::Command::WatchStream(id));
                }
            }
            Message::StopWatching(id) => {
                if self.command(client::Command::StopWatching(id.clone())) {
                    match id {
                        Some(id) => self.server_mut().streaming.forget(&id),
                        None => self.server_mut().streaming.clear_picture(),
                    }
                }
            }
            Message::FocusStream(id) => {
                let focused = id.filter(|id| self.server().streaming.watching(id).is_some());
                let streaming = &mut self.server_mut().streaming;
                // Back to the grid shows every stream, so nothing stays tucked away.
                streaming.others_hidden &= focused.is_some();
                streaming.focused = focused;
            }
            Message::HideOtherStreams(hidden) => {
                self.server_mut().streaming.others_hidden = hidden;
            }
            // The large stream, or the first one, takes the whole screen with the others
            // tucked away; leaving puts them back as they were.
            Message::StreamFullscreen(true) => {
                if self.ui.fullscreen.is_none() && ui::fullscreen_allowed(self) {
                    let streaming = &mut self.server_mut().streaming;
                    if streaming.stage().is_none() {
                        streaming.focused = streaming
                            .watching
                            .first()
                            .map(|watch| watch.stream.id.clone());
                    }
                    let hidden = std::mem::replace(&mut streaming.others_hidden, true);
                    self.ui.fullscreen = Some(hidden);
                }
            }
            Message::StreamFullscreen(false) => {
                if let Some(hidden) = self.ui.fullscreen.take() {
                    self.server_mut().streaming.others_hidden = hidden;
                }
            }
            Message::StreamLayout(None) => {
                // The grid shows every stream, so nothing stays tucked away.
                let streaming = &mut self.server_mut().streaming;
                streaming.focused = None;
                streaming.others_hidden = false;
            }
            Message::StreamLayout(Some(strip)) => {
                // Choosing where the others go means showing them.
                self.server_mut().streaming.others_hidden = false;
                self.server_mut().streaming.strip = strip;
                self.save_stream_view();
                if self.server().streaming.stage().is_none() {
                    self.server_mut().streaming.focused = self
                        .server()
                        .streaming
                        .watching
                        .first()
                        .map(|watch| watch.stream.id.clone());
                }
            }
            Message::SwapStreams(from, to) => self.server_mut().streaming.swap(&from, &to),
            Message::StreamVolume(publisher, percent) => {
                let percent = percent.min(streaming::MAX_VOLUME);
                self.server_mut().streaming.set_volume(publisher, percent);
                // Kept for this streamer by identity, so their stream starts there next time.
                if let Some(uid) = self
                    .find_member(publisher)
                    .filter(|member| !member.uid.is_empty())
                    .map(|member| member.uid.clone())
                {
                    if percent == 100 {
                        self.stream_volumes.remove(&uid);
                    } else {
                        self.stream_volumes.insert(uid, percent);
                    }
                    self.stream_volumes_changed = Some(Instant::now());
                }
            }
            // Muting is for now; the saved volume stays what it was.
            Message::ToggleStreamMute(publisher) => {
                self.server_mut().streaming.toggle_mute(publisher)
            }
            Message::ToggleStreamStats => {
                self.server_mut().streaming.show_stats = !self.server().streaming.show_stats;
                self.save_stream_view();
            }
            Message::OpenShare => {
                if self.live() && self.tab == Tab::Home {
                    self.server_mut().streaming.share_open = true;
                    // How the last share ended is old news in a dialog starting a new one.
                    let status = &mut self.server_mut().streaming.sharing.status;
                    if !status.active {
                        status.detail.clear();
                    }
                    if self.server().streaming.sharing.sources.is_empty() {
                        return self.update(Message::RefreshShareSources);
                    }
                    return self.take_share_preview();
                }
            }
            Message::CloseShare => {
                self.server_mut().streaming.share_open = false;
                self.server_mut().streaming.sharing.previews.clear();
                self.remember_share();
                self.ui.open = None;
                self.ui.share_group = None;
            }
            // While sharing, the quality changes at once, for viewers already watching too.
            Message::ShareQuality(quality) => {
                if quality.validate().is_ok() {
                    self.server_mut().streaming.sharing.quality = quality;
                    self.configure_share();
                }
            }
            Message::ShareResolution(resolution) => {
                let quality = streaming::publishing::Quality {
                    resolution,
                    ..self.server().streaming.sharing.quality
                };
                return self.update(Message::ShareQuality(quality));
            }
            Message::ShareFrameRate(frame_rate) => {
                let quality = streaming::publishing::Quality {
                    frame_rate,
                    ..self.server().streaming.sharing.quality
                };
                return self.update(Message::ShareQuality(quality));
            }
            Message::ShareVideoBitrate(video_kbps) => {
                let quality = streaming::publishing::Quality {
                    video_kbps,
                    ..self.server().streaming.sharing.quality
                };
                return self.update(Message::ShareQuality(quality));
            }
            Message::ShareAudioBitrate(audio_kbps) => {
                let quality = streaming::publishing::Quality {
                    audio_kbps,
                    ..self.server().streaming.sharing.quality
                };
                return self.update(Message::ShareQuality(quality));
            }
            Message::ShareStruggle(struggle) => {
                let quality = streaming::publishing::Quality {
                    struggle,
                    ..self.server().streaming.sharing.quality
                };
                return self.update(Message::ShareQuality(quality));
            }
            Message::ShareAdvanced(advanced) => self.ui.share_advanced = advanced,
            // Switching to Screen, Window or Camera picks the likely one of that kind: the main
            // screen, the game being played, the camera. Windows without a game wait for a pick.
            Message::ShareGroup(group) => {
                use streaming::publishing::{SourceGroup, default_source};
                if self.server().streaming.sharing.status.active {
                    return Task::none();
                }
                self.ui.share_group = Some(group);
                let sharing = &self.server().streaming.sharing;
                let kind =
                    |source: &streaming::publishing::ShareSource| SourceGroup::of(&source.kind);
                if sharing.selected.as_ref().map(kind) == Some(group) {
                    return Task::none();
                }
                let next = match group {
                    SourceGroup::Screen => default_source(&sharing.sources, &self.ui.quick_sources),
                    SourceGroup::Window => sharing
                        .sources
                        .iter()
                        .find(|source| {
                            kind(source) == SourceGroup::Window
                                && self.ui.quick_sources.iter().any(|q| q.kind == source.kind)
                        })
                        .cloned(),
                    SourceGroup::Camera => sharing
                        .sources
                        .iter()
                        .find(|source| kind(source) == SourceGroup::Camera)
                        .cloned(),
                };
                match next {
                    Some(source) => return self.update(Message::ShareSource(source)),
                    None => self.server_mut().streaming.sharing.selected = None,
                }
            }
            Message::ShareAccess(access) => {
                if !self.server().streaming.sharing.status.active {
                    self.server_mut().streaming.sharing.access = access;
                }
            }
            Message::RefreshShareSources => {
                if !self.server().streaming.sharing.status.active
                    && !self.server().streaming.sharing.loading
                {
                    self.server_mut().streaming.sharing.loading = true;
                    self.server_mut().streaming.sharing.previews.clear();
                    self.begin_alert(Source::StreamPublish);
                    return Task::perform(
                        async {
                            tokio::task::spawn_blocking(streaming::share_sources)
                                .await
                                .map_err(|error| error.to_string())?
                                .map_err(|error| error.to_string())
                        },
                        Message::ShareSources,
                    );
                }
            }
            Message::ShareSources(result) => {
                self.server_mut().streaming.sharing.loading = false;
                match result {
                    Ok(sources) => {
                        if self
                            .server()
                            .streaming
                            .sharing
                            .selected
                            .as_ref()
                            .is_some_and(|selected| !sources.contains(selected))
                        {
                            self.server_mut().streaming.sharing.selected = None;
                        }
                        if self.server().streaming.sharing.selected.is_none() {
                            self.server_mut().streaming.sharing.selected =
                                streaming::publishing::default_source(
                                    &sources,
                                    &self.ui.quick_sources,
                                );
                        }
                        self.server_mut().streaming.sharing.sources = sources;
                        return self.take_share_preview();
                    }
                    Err(error) => self.raise_active(Source::StreamPublish, error),
                }
            }
            Message::ShareSource(source) => {
                if !self.server().streaming.sharing.status.active {
                    // A camera share starts without system sound; voice chat remains separate.
                    if source.kind.is_camera() {
                        self.server_mut().streaming.sharing.audio = false;
                    }
                    self.server_mut().streaming.sharing.selected = Some(source);
                    return self.take_share_preview();
                }
            }
            Message::SharePreview(source, still) => {
                let sharing = &mut self.server_mut().streaming.sharing;
                sharing.finish_preview(&source, still);
            }
            Message::ShareAudio(audio) => {
                self.server_mut().streaming.sharing.audio = audio;
                self.configure_share();
            }
            Message::StreamingHardwareAv1(on) => {
                self.stream_options.hardware_av1 = on;
                self.save_preference(
                    |saved| saved.streaming.hardware_av1 = on,
                    "The AV1 choice applies for now but was not saved",
                );
            }
            Message::StreamingFastCapture(on) => {
                self.stream_options.fast_capture = on;
                self.save_preference(
                    |saved| saved.streaming.fast_capture = on,
                    "The fast capture choice applies for now but was not saved",
                );
            }
            Message::ShareAudioOutput(device) => {
                self.server_mut().streaming.sharing.audio_device = Some(device);
                self.configure_share();
            }
            Message::StartSharing => {
                if let Some(source) = self.server().streaming.sharing.selected.clone() {
                    let access = self.server().streaming.sharing.access;
                    return self.start_share(source, access, true);
                }
            }
            // Always public, and it leaves the dialog's choices as they were.
            Message::QuickShare(source) => {
                return self.start_share(source, streaming::publishing::Access::Public, false);
            }
            Message::RefreshQuickSources => {
                let session = self.server();
                let allowed = matches!(
                    permissions::status(permissions::Kind::Screen),
                    permissions::Status::Granted | permissions::Status::NotRequired
                );
                if self.live()
                    && self.tab == Tab::Home
                    && streaming::AVAILABLE
                    && session.state.server_version.starts_with("6.")
                    && !session.streaming.sharing.status.active
                    && !self.ui.quick_checking
                    && allowed
                {
                    self.ui.quick_checking = true;
                    return Task::perform(
                        async {
                            tokio::task::spawn_blocking(streaming::quick_share_sources)
                                .await
                                .map_err(|error| error.to_string())?
                                .map_err(|error| error.to_string())
                        },
                        Message::QuickSources,
                    );
                }
            }
            Message::QuickSources(result) => {
                self.ui.quick_checking = false;
                self.ui.quick_sources = result.unwrap_or_default();
            }
            Message::StopSharing => {
                self.server().streaming.sharing.stop_capture();
                self.command(client::Command::StopSharing);
            }
            Message::DecideViewer(id, allow) => {
                self.command(client::Command::DecideViewer(id, allow));
            }
            Message::RecordTrace => {
                if self.recording.is_none() && !self.trace_saving {
                    self.alerts.begin(Source::Trace);
                    self.trace_path = None;
                    self.trace_summary = None;
                    match profiler::Recording::start() {
                        Ok(recording) => self.recording = Some(recording),
                        Err(error) => self.alerts.raise(Source::Trace, error.to_string()),
                    }
                }
            }
            Message::StopTrace => return self.stop_trace(),
            Message::TraceSaved(result) => {
                self.trace_saving = false;
                match result {
                    Ok(path) => {
                        self.trace_path = Some(path);
                        self.alerts.recover(Source::Trace);
                    }
                    Err(error) => self.alerts.raise(Source::Trace, error),
                }
            }
            Message::CopyTracePath => {
                if let Some(path) = &self.trace_path {
                    return Task::Copy(path.display().to_string());
                }
            }
            Message::UsageTick => {
                // A shortcut another app held is taken as soon as it lets go.
                if self.key_capture.is_none()
                    && self
                        .shortcuts
                        .as_mut()
                        .is_some_and(shortcuts::Shortcuts::retry)
                {
                    let keys = self.keybinds.clone();
                    self.report_taken_shortcuts(&keys);
                }
                self.refresh_visible_permissions();
                if self
                    .stream_volumes_changed
                    .is_some_and(|at| at.elapsed() >= std::time::Duration::from_secs(1))
                {
                    self.save_stream_volumes();
                }
                self.process_usage = self.process_monitor.sample();
                let now = Instant::now();
                let nickname = self.nickname.trim().to_owned();
                for session in &mut self.sessions {
                    if session.connected {
                        session.connection_history.advance(now);
                        session.client_history.advance(now);
                    }
                    // A message the server never sent back is shown after a short wait.
                    let (own, channel) = match &session.state.current {
                        Some(channel) => (
                            channel
                                .members
                                .iter()
                                .find(|member| member.is_self)
                                .map_or(nickname.as_str(), |member| member.name.as_str()),
                            channel.name.as_str(),
                        ),
                        None => (nickname.as_str(), ""),
                    };
                    let (own, channel) = (own.to_owned(), channel.to_owned());
                    session.chat.overdue(now, &own, &channel);
                }
                self.cpu_history
                    .push(now, self.process_usage.cpu_percent.map(f64::from));
                self.ram_history.push(
                    now,
                    self.process_usage.memory_bytes.map(|bytes| bytes as f64),
                );
                self.heap_usage = memory::snapshot();
                let (upload, download) = self.sessions.iter().fold((0.0, 0.0), |sum, session| {
                    (
                        sum.0 + session.network_usage.upload_per_second,
                        sum.1 + session.network_usage.download_per_second,
                    )
                });
                profiler::record_counters(
                    self.process_usage.memory_bytes,
                    self.audio_buffer_bytes,
                    upload,
                    download,
                );
                if self
                    .recording
                    .as_ref()
                    .is_some_and(|r| r.started.elapsed() >= profiler::MAX_DURATION)
                {
                    return self.stop_trace();
                }
            }
            Message::Address(value) => {
                if self.idle() {
                    if self.address != value {
                        self.password.clear();
                    }
                    self.address = value;
                    self.settings_message = None;
                }
            }
            Message::Nickname(value) => {
                if self.idle() {
                    self.nickname = value;
                    self.nickname_error = None;
                }
            }
            Message::CommitNickname => self.commit_nickname(),
            Message::Password(value) => self.password = value,
            Message::ChannelPassword(value) => self.server_mut().channel_password = value,
            Message::Input(value) => {
                self.input = Some(value);
                self.save_audio();
            }
            Message::Output(value) => {
                self.output = Some(value);
                self.save_audio();
            }
            Message::Refresh => {
                self.alerts.begin(Source::Audio);
                match audio::devices() {
                    Ok(devices) => {
                        // A chosen device that's unplugged stays chosen while connected: the
                        // hub stands in for it and goes back to it once it returns.
                        let keep = !self.idle();
                        if !keep
                            && !self
                                .input
                                .as_ref()
                                .is_some_and(|d| devices.inputs.contains(d))
                        {
                            self.input = devices.input.clone();
                        }
                        if !keep
                            && !self
                                .output
                                .as_ref()
                                .is_some_and(|d| devices.outputs.contains(d))
                        {
                            self.output = devices.output.clone();
                        }
                        self.devices = devices;
                        self.alerts.recover(Source::Audio);
                    }
                    Err(error) => self
                        .alerts
                        .raise(Source::Audio, format!("Device detection failed: {error:#}")),
                }
            }
            Message::Connect(index) => {
                if self.onboarding.is_some() || self.loopback {
                    return Task::none();
                }
                let Some(server) = self.servers.get(index) else {
                    return Task::none();
                };
                let address = server.address.trim().to_owned();
                if self.session_at(&address).is_some() {
                    return Task::none();
                }
                self.ui.server_picker = false;
                self.password = self
                    .server_passwords
                    .get(&address)
                    .cloned()
                    .unwrap_or_default();
                self.address = address;
                // An edited nickname is kept; one that can't be falls back to the saved one.
                self.commit_nickname();
                if self.nickname_error.take().is_some()
                    && let Some(saved) = &self.saved
                {
                    self.nickname = saved.nickname.clone();
                }
                self.alerts.begin(Source::Connection);
                self.alerts.begin(Source::Audio);
                self.alerts.recover_at(&self.address.clone());
                return self.start(false);
            }
            Message::AddServer => {
                self.ui.server_picker = false;
                if self.onboarding.is_none() {
                    let alone = self.settings_page.is_none();
                    if alone {
                        self.open_settings(SettingsPage::Server);
                    }
                    self.server_editor = Some(ServerEditor {
                        alone,
                        ..ServerEditor::default()
                    });
                }
            }
            Message::EditServer(index) => {
                if let Some(server) = self.servers.get(index) {
                    self.server_editor = Some(ServerEditor {
                        index: Some(index),
                        name: server.name.clone(),
                        address: server.address.clone(),
                        password: self
                            .server_passwords
                            .get(server.address.trim())
                            .cloned()
                            .unwrap_or_default(),
                        ..ServerEditor::default()
                    });
                }
            }
            Message::ServerName(value) => {
                if let Some(editor) = &mut self.server_editor {
                    editor.name = value;
                }
            }
            Message::ServerAddress(value) => {
                if let Some(editor) = &mut self.server_editor {
                    editor.address = value;
                    editor.error = None;
                }
            }
            Message::ServerPassword(value) => {
                if let Some(editor) = &mut self.server_editor {
                    editor.password = value;
                }
            }
            Message::ServerAutoConnect(index, on) => {
                let mut servers = self.servers.clone();
                if let Some(server) = servers.get_mut(index) {
                    server.auto_connect = on;
                    self.set_servers(servers);
                }
            }
            Message::AutoConnect => return self.auto_connect(),
            Message::SubmitServer => self.submit_server(),
            Message::CancelServer => self.close_server_editor(),
            Message::RemoveServer => self.remove_server(),
            Message::TestAudio => {
                self.alerts.begin(Source::Audio);
                return self.start(true);
            }
            Message::Stop => {
                // Leaving cuts a live share off for its viewers, so that asks first.
                if self.server().streaming.sharing.status.active {
                    self.disconnect_prompt = true;
                } else if let Some(id) = self.active {
                    self.stop_session(id);
                }
            }
            Message::ConfirmDisconnect(leave) => {
                self.disconnect_prompt = false;
                if leave && let Some(id) = self.active {
                    self.stop_session(id);
                }
            }
            Message::TrayDisconnect => {
                if let Some(id) = self.active {
                    self.stop_session(id);
                }
            }
            Message::StopServer(id) => self.stop_session(id),
            Message::SwitchServer(id) => {
                if self.session(id).is_some_and(|session| !session.stopping) {
                    for session in &mut self.sessions {
                        session.activate_on_connect = false;
                    }
                    self.activate(Some(id));
                }
            }
            Message::OpenServerPicker => {
                if self.onboarding.is_none() && !self.loopback {
                    self.ui.rail_peek = ui::RailPeek::default();
                    self.ui.server_picker = true;
                }
            }
            Message::CloseServerPicker => self.ui.server_picker = false,
            Message::Mute => self.toggle_mute(),
            Message::Deafen => self.toggle_deafen(),
            Message::Join(id) => {
                if self.can_request_join(id) {
                    self.begin_alert(Source::Join);
                    self.server_mut().password_error = None;
                    self.server_mut().channel_password.clear();
                    if self
                        .server()
                        .state
                        .channels
                        .iter()
                        .any(|channel| channel.id == id && channel.has_password)
                    {
                        self.server_mut().password_channel = Some(id);
                        return Task::Focus(ui::Field::ChannelPassword);
                    }
                    self.server_mut().expected_channel = Some(id);
                    self.command(client::Command::Join(id, String::new()));
                }
            }
            Message::SubmitChannelPassword => {
                if let Some(id) = self.server().password_channel
                    && self.can_request_join(id)
                    && self.command(client::Command::Join(
                        id,
                        self.server().channel_password.clone(),
                    ))
                {
                    self.server_mut().expected_channel = Some(id);
                    self.server_mut().channel_secret =
                        Some((id, self.server().channel_password.clone()));
                    self.server_mut().password_error = None;
                    self.server_mut().password_channel = None;
                    self.server_mut().channel_password.clear();
                }
            }
            Message::CancelChannelPassword => {
                self.server_mut().password_channel = None;
                self.server_mut().password_error = None;
                self.server_mut().channel_password.clear();
            }
            Message::ToggleFooter => self.set_footer_hidden(!self.footer_hidden),
            Message::Updater(event) => {
                if matches!(event, updater::Event::Dismiss) {
                    self.ui.update_hover = false;
                }
                return self.update_software(event);
            }
            Message::Resume => return self.resume(),
            Message::SelectMember(id, toggle) => {
                if toggle {
                    if !self.server_mut().selected_members.remove(&id) {
                        self.server_mut().selected_members.insert(id);
                    }
                } else if self.server().selected_members.len() == 1
                    && self.server().selected_members.contains(&id)
                {
                    self.server_mut().selected_members.clear();
                } else {
                    self.server_mut().selected_members = BTreeSet::from([id]);
                }
            }
            Message::ClearSelection => self.server_mut().selected_members.clear(),
            Message::MoveMembers(ids, channel) => {
                if !(self.live()) {
                    return Task::none();
                }
                self.server_mut().selected_members.clear();
                let own = self.own_id();
                let others: Vec<u16> = ids
                    .iter()
                    .copied()
                    .filter(|id| Some(*id) != own && self.member_channel(*id) != Some(channel))
                    .collect();
                if !others.is_empty() {
                    self.begin_alert(Source::Move);
                    self.command(client::Command::Move(others, channel));
                }
                if own.is_some_and(|own| ids.contains(&own)) {
                    return self.update(Message::Join(channel));
                }
            }
            Message::OpenPoke(id) => {
                if let Some(member) = self.find_member(id).filter(|member| !member.is_self)
                    && self.server().connected
                    && !self.loopback
                {
                    self.server_mut().poke = Some(PokeDraft {
                        id,
                        name: member.name.clone(),
                        message: String::new(),
                    });
                    return Task::Focus(ui::Field::PokeMessage);
                }
            }
            Message::PokeText(value) => {
                if let Some(draft) = &mut self.server_mut().poke {
                    draft.message = value.chars().take(100).collect();
                }
            }
            Message::SubmitPoke => {
                if let Some(draft) = self.server_mut().poke.take() {
                    self.begin_alert(Source::Poke);
                    let key = self
                        .find_member(draft.id)
                        .map(|member| client::chat_key(&member.uid, member.id));
                    if self.command(client::Command::Poke(draft.id, draft.message.clone()))
                        && let Some(key) = key
                    {
                        self.server_mut()
                            .chat
                            .poke(key, &draft.name, true, &draft.message);
                    }
                }
            }
            Message::CancelPoke => self.server_mut().poke = None,
            Message::DismissPoke => {
                if !self.server().pokes.is_empty() {
                    self.server_mut().pokes.remove(0);
                }
            }
            Message::OpenModeration(ids, action) => {
                let ids: Vec<u16> = ids
                    .into_iter()
                    .filter(|id| self.find_member(*id).is_some_and(|member| !member.is_self))
                    .collect();
                if self.live() && !ids.is_empty() {
                    self.server_mut().moderation = Some(ModerationDraft {
                        ids,
                        action,
                        reason: String::new(),
                    });
                    return Task::Focus(ui::Field::ModerationReason);
                }
            }
            Message::ModerationReason(value) => {
                if let Some(draft) = &mut self.server_mut().moderation {
                    draft.reason = value.chars().take(80).collect();
                }
            }
            Message::ModerationBan(seconds) => {
                if let Some(draft) = &mut self.server_mut().moderation
                    && matches!(draft.action, client::Moderation::Ban(_))
                {
                    draft.action = client::Moderation::Ban(seconds);
                }
            }
            Message::SubmitModeration => {
                if let Some(draft) = self.server_mut().moderation.take() {
                    self.server_mut().selected_members.clear();
                    self.begin_alert(Source::Moderate);
                    self.command(client::Command::Moderate(
                        draft.ids,
                        draft.action,
                        draft.reason,
                    ));
                }
            }
            Message::CancelModeration => self.server_mut().moderation = None,
            Message::OpenGroups(ids) => {
                let unchanged = self
                    .server()
                    .group_edit
                    .as_ref()
                    .is_some_and(|draft| draft.ids == ids);
                if self.live() && !ids.is_empty() && !unchanged {
                    self.server_mut().group_edit = Some(GroupEditDraft {
                        ids,
                        ..Default::default()
                    });
                    let state = &self.server().state;
                    if state.server_groups.is_empty() || state.channel_groups.is_empty() {
                        self.command(client::Command::RequestGroups);
                    }
                }
            }
            Message::ToggleServerGroup(group) => {
                if let Some(draft) = &self.server().group_edit {
                    let ids = draft.ids.clone();
                    let held = self.server_group_mark(&ids, group);
                    let waiting = draft
                        .wanted
                        .get(&group)
                        .is_some_and(|wanted| GroupMark::of(Some(*wanted), held) != held);
                    if !waiting {
                        let add = held != GroupMark::On;
                        if let Some(draft) = &mut self.server_mut().group_edit {
                            draft.wanted.insert(group, add);
                        }
                        let (added, removed) = if add {
                            (vec![group], Vec::new())
                        } else {
                            (Vec::new(), vec![group])
                        };
                        self.send_group_edit(
                            ids,
                            client::GroupEdit {
                                add: added,
                                remove: removed,
                                channel_group: None,
                            },
                        );
                    }
                }
            }
            Message::PickChannelGroup(group) => {
                if let Some(draft) = &self.server().group_edit {
                    let ids = draft.ids.clone();
                    let shared = self.shared_channel_group(&ids);
                    let waiting = draft
                        .channel_group
                        .is_some_and(|picked| Some(picked) != shared);
                    if !waiting && shared != Some(group) {
                        if let Some(draft) = &mut self.server_mut().group_edit {
                            draft.channel_group = Some(group);
                        }
                        self.send_group_edit(
                            ids,
                            client::GroupEdit {
                                channel_group: Some(group),
                                ..Default::default()
                            },
                        );
                    }
                }
            }
            Message::Install => {
                if let Some(offer) = self.install_offer.clone()
                    && !self.installing
                {
                    self.installing = true;
                    self.alerts.begin(Source::Install);
                    return Task::perform(
                        async move { install::install(&offer) },
                        Message::Installed,
                    );
                }
            }
            Message::Installed(result) => match result {
                Ok(()) => {
                    // Return to the same session in the installed copy, as an update does.
                    if let Some(path) = self.settings_path.as_deref().map(resume::path)
                        && let Err(error) =
                            resume::save(&path, &self.resume_snapshot(), SystemTime::now())
                    {
                        tracing::warn!("The installed copy opens without this session: {error}");
                    }
                    return self.update(Message::Quit);
                }
                Err(error) => {
                    self.installing = false;
                    self.alerts.raise(Source::Install, error);
                }
            },
            Message::DeclineInstall(forever) => {
                self.install_prompt = false;
                if forever {
                    self.save_preference(
                        |saved| saved.skip_install_prompt = true,
                        "The choice applies for now but was not saved",
                    );
                }
            }
            Message::RedoSetup => {
                if self.idle() && self.onboarding.is_none() {
                    self.close_settings();
                    self.tab = Tab::Home;
                    self.redoing_setup = true;
                    self.find_ts6();
                    self.onboarding = Some(onboarding::Step::Username);
                    self.onboarding_error = None;
                    if let Some(first) = self.servers.first() {
                        self.address = first.address.clone();
                        self.password = self
                            .server_passwords
                            .get(first.address.trim())
                            .cloned()
                            .unwrap_or_default();
                    }
                    self.sync_page_visibility();
                }
            }
            Message::CancelSetup => {
                if self.redoing_setup
                    && self.idle()
                    && self.permissions.pending.is_none()
                    && let Some(saved) = self.saved.clone()
                {
                    self.apply_settings(saved);
                    self.password.clear();
                    self.onboarding = None;
                    self.onboarding_error = None;
                    self.redoing_setup = false;
                    self.sync_page_visibility();
                }
            }
            Message::Appearance(appearance) => self.set_appearance(appearance),
            Message::Chat(action) => return self.chat_action(action),
            Message::OpenSettingsFolder => self.open_settings_folder(),
            Message::OpenUserVolume(id) => {
                if let Some(member) = self.find_member(id).filter(|m| !m.uid.is_empty()) {
                    self.ui.user_volume = Some(ui::UserVolumeTarget {
                        uid: member.uid.clone(),
                        name: member.name.clone(),
                    });
                }
            }
            Message::UserVolume(percent) => {
                if let Some(uid) = self.ui.user_volume.as_ref().map(|t| t.uid.clone()) {
                    self.change_user_audio(uid, |audio| {
                        audio.volume_percent = percent.min(settings::UserAudio::MAX_PERCENT);
                    });
                }
            }
            // Saved here rather than on every step of the slider.
            Message::CloseUserVolume => {
                if self.ui.user_volume.take().is_some() {
                    self.save_user_audio();
                }
            }
            Message::ToggleUserMute(id) => {
                if let Some(uid) = self
                    .find_member(id)
                    .filter(|m| !m.uid.is_empty())
                    .map(|m| m.uid.clone())
                {
                    self.change_user_audio(uid, |audio| audio.muted = !audio.muted);
                    self.save_user_audio();
                }
            }
            Message::SoundVolume(percent) => {
                self.sounds.volume = percent.min(100);
                self.sounds_unsaved = true;
            }
            Message::SoundPack(pack) => {
                self.sounds.pack = pack;
                self.save_sounds();
            }
            Message::SoundGroup(group, on) => {
                self.sounds.set_group(group, on);
                self.save_sounds();
            }
            Message::SoundOn(sound, on) => {
                self.sounds.set_sound(sound, on);
                self.save_sounds();
            }
            Message::LogSearch(open) => {
                let log = &mut self.server_mut().log;
                log.searching = open;
                if !open {
                    log.clear_filter();
                }
                self.ui.log_people = false;
                self.ui.log_search.focused = open;
            }
            Message::LogQuery(query) => self.server_mut().log.set_query(query),
            Message::LogPerson(person) => {
                self.server_mut().log.set_person(person);
                self.ui.log_people = false;
            }
            Message::LogPeople => self.ui.log_people = !self.ui.log_people,
            Message::LogClear => self.server_mut().log.clear_filter(),
            Message::ToggleLog => {
                self.log_pane.collapsed = !self.log_pane.collapsed;
                self.save_log_pane();
            }
            Message::SaveLogPane => self.save_log_pane(),
            Message::PreviewSound(sound) => self.preview_sound(sound),
            Message::SoundCues(on) => {
                self.sound_cues = on;
                self.save_preference(
                    |saved| saved.sound_cues = on,
                    "The sound cue choice applies for now but was not saved",
                );
            }
            Message::ToggleSubscribeAll => {
                self.subscribe_all.fetch_xor(true, Ordering::Relaxed);
            }
            Message::ToggleChannel(id) => {
                if !self.server_mut().collapsed_channels.remove(&id) {
                    self.server_mut().collapsed_channels.insert(id);
                }
            }
            Message::ToggleClient(id) => {
                if self.server().connected
                    && !self.loopback
                    && !self.server().stopping
                    && self
                        .server()
                        .state
                        .current_members()
                        .iter()
                        .any(|member| member.id == id)
                {
                    self.select_client((self.server().expanded_client != Some(id)).then_some(id));
                }
            }
            Message::Close => {
                if self.idle() {
                    return self.update(Message::Quit);
                }
                match self.close_button() {
                    settings::CloseButton::Ask => {
                        self.close_prompt = true;
                        self.close_remember = false;
                    }
                    settings::CloseButton::HideToTray => self.hide_window(),
                    settings::CloseButton::Quit => return self.update(Message::Quit),
                }
            }
            Message::CloseRemember(remember) => self.close_remember = remember,
            Message::CloseChoice(choice) => {
                self.close_prompt = false;
                let Some(choice) = choice else {
                    return Task::none();
                };
                if self.close_remember {
                    self.save_preference(
                        |saved| saved.close_button = choice,
                        "The close choice applies for now but was not saved",
                    );
                }
                if choice == settings::CloseButton::HideToTray {
                    self.hide_window();
                } else {
                    return self.update(Message::Quit);
                }
            }
            Message::SetCloseButton(choice) => self.save_preference(
                |saved| saved.close_button = choice,
                "The close choice applies for now but was not saved",
            ),
            Message::SetAutostart(wanted) => match autostart::set(wanted) {
                Ok(()) => self.autostart = wanted,
                Err(error) => self.alerts.raise(
                    Source::SettingsSave,
                    format!("Start with Windows was not changed: {error}"),
                ),
            },
            Message::WindowFocus(focused) => {
                self.window_focused = focused;
                if focused {
                    self.window_hidden = false;
                    self.chat_seen();
                }
            }
            Message::Quit => {
                self.close_prompt = false;
                self.save_window_place();
                if self.stream_volumes_changed.is_some() {
                    self.save_stream_volumes();
                }
                self.flush_unsaved();
                if self.idle() {
                    return Task::Exit;
                }
                self.closing = true;
                // Quitting doesn't wait for a sound to finish.
                self.hub.hush();
                if let Some(handle) = &self.handle {
                    // The window goes at once, so closing feels instant; each server is told
                    // goodbye behind it. Should one not answer, the app quits anyway.
                    handle.hide();
                    let handle = handle.clone();
                    let _ = std::thread::Builder::new()
                        .name("quit-deadline".into())
                        .spawn(move || {
                            std::thread::sleep(QUIT_DEADLINE);
                            handle.quit();
                        });
                }
                let ids: Vec<_> = self.sessions.iter().map(|session| session.id).collect();
                for id in ids {
                    if let Some(session) = self.session(id) {
                        session.streaming.sharing.stop_capture();
                    }
                    self.send_to(id, client::Command::Stop);
                    if let Some(session) = self.session_mut(id) {
                        session.stopping = true;
                    }
                }
            }
            Message::Backend(id, event) => return self.backend(id, event),
            Message::Alert(source, detail) => self.alerts.raise(source, detail),
        }
        Task::none()
    }

    /// One server's event. Everything here belongs to that server, active or not; only the
    /// marked places also touch what the window is showing.
    fn backend(&mut self, id: hub::SessionId, event: client::Event) -> Task {
        let Some(index) = self.sessions.iter().position(|session| session.id == id) else {
            return Task::none();
        };
        let loopback = self.loopback;
        let active = self.active == Some(id);
        let session = &mut self.sessions[index];
        let mut sounds_due = Vec::new();
        let mut arrived = None;
        // Shown only while the window is in the background.
        let mut toast = None;
        // A first connection brings the server's saved conversations back.
        let restore =
            matches!(event, client::Event::Connected(_)) && session.chat.needs_restoring();
        match event {
            client::Event::Activity(activities) => {
                for activity in &activities {
                    sounds_due.extend(activity_sound(activity));
                    log_activity(&mut session.log, activity);
                    session.kicked |= activity.is_self
                        && matches!(activity.kind, client::ActivityKind::KickedFromChannel);
                }
            }
            client::Event::Status(status) => {
                if status == client::CONNECTION_LOST && !session.reconnecting {
                    session.reconnecting = true;
                    session
                        .log
                        .add(server_log::Tone::Left, "", "Connection lost");
                    sounds_due.push(sounds::Sound::ConnectionLost);
                }
                session.catalog_seen = false;
                session.streaming.reset();
                session.clear_client_details();
                session.client_usage.clear();
                session.client_health.clear();
                session.connection_health = usage::SteadyHealth::default();
                session.limited = false;
                session.talking.clear();
                session.connection_history = ConnectionHistory::new();
                session.password_channel = None;
                session.channel_password.clear();
                session.status = status;
                session.connected = false;
                session.network_usage.stop();
                session.connection_usage = usage::ConnectionUsage::default();
            }
            client::Event::Connected(detail) => {
                if !loopback {
                    let back = std::mem::take(&mut session.reconnecting);
                    let (sound, words) = if back {
                        (sounds::Sound::Reconnected, "Reconnected")
                    } else {
                        (sounds::Sound::Connected, "Connected")
                    };
                    session.log.add(server_log::Tone::Arrived, "", words);
                    sounds_due.push(sound);
                }
                session.connected = true;
                session.detail = detail;
                session.status = if loopback {
                    "Local microphone test — only you can hear this"
                } else {
                    "Connected — microphone starts muted"
                }
                .into();
                let take_over = session.activate_on_connect;
                // A new session starts muted; restore the microphone as it was.
                let restored = session.resume.as_mut().map(|resume| {
                    let state = (resume.muted, resume.deafened);
                    (resume.muted, resume.deafened) = (true, false);
                    state
                });
                self.alerts.recover_in(id.0, Source::Connection);
                let address = session.address.trim().to_owned();
                self.alerts.recover_at(&address);
                if let Some((muted, deafened)) = restored
                    && !loopback
                {
                    if !muted {
                        self.set_muted(id, false);
                    }
                    if deafened {
                        self.set_deafened(id, true);
                    }
                }
                if take_over {
                    self.activate(Some(id));
                }
            }
            client::Event::State(state) => {
                let channel_changed = session.state.current.as_ref().map(|channel| channel.id)
                    != state.current.as_ref().map(|channel| channel.id);
                sounds_due.extend(channel_sound(session, &state));
                if channel_changed {
                    session.catalog_seen = false;
                    session.client_usage.clear();
                    session.client_health.clear();
                } else {
                    let sampled = |id: &u16| {
                        state
                            .current_members()
                            .iter()
                            .any(|member| member.id == *id && !member.is_self)
                    };
                    session.client_usage.retain(|id, _| sampled(id));
                    session.client_health.retain(|id, _| sampled(id));
                }
                if session.expanded_client.is_some_and(|id| {
                    channel_changed || !state.current_members().iter().any(|member| member.id == id)
                }) {
                    session.clear_client_details();
                }
                session
                    .collapsed_channels
                    .retain(|id| state.channels.iter().any(|channel| channel.id == *id));
                let present = |id: &u16| {
                    state
                        .channels
                        .iter()
                        .any(|channel| channel.members.iter().any(|member| member.id == *id))
                };
                session.selected_members.retain(present);
                if session
                    .poke
                    .as_ref()
                    .is_some_and(|draft| !present(&draft.id))
                {
                    session.poke = None;
                }
                if session.moderation.as_mut().is_some_and(|draft| {
                    draft.ids.retain(present);
                    draft.ids.is_empty()
                }) {
                    session.moderation = None;
                }
                if session.group_edit.as_mut().is_some_and(|draft| {
                    draft.ids.retain(present);
                    draft.ids.is_empty()
                }) {
                    session.group_edit = None;
                }
                session.state = state;
                if let Some(channel) = &session.state.current {
                    session.chat.enter(channel.id, &channel.name);
                }
                let mut join = None;
                if session.connected
                    && !session.state.channels.is_empty()
                    // The channel list arrives before the server says where you are, and
                    // asking for the channel you were put in is refused with an error.
                    && session.state.current.is_some()
                    && let Some(resume) = session.resume.take()
                    && let Some(channel) = resume.channel
                    && session.state.current.as_ref().map(|channel| channel.id) != Some(channel)
                    && session.can_request_join(loopback, channel)
                {
                    if !resume.channel_password.is_empty() {
                        session.channel_secret = Some((channel, resume.channel_password.clone()));
                    }
                    join = Some((channel, resume.channel_password));
                }
                if session
                    .password_channel
                    .is_some_and(|channel| !session.can_request_join(loopback, channel))
                {
                    session.password_channel = None;
                    session.channel_password.clear();
                }
                if let Some((channel, password)) = join {
                    // Going back after an update isn't something to announce.
                    if let Some(session) = self.session_mut(id) {
                        session.quiet_channel_change = true;
                    }
                    self.alerts.begin_in(id.0, Source::Join);
                    self.send_to(id, client::Command::Join(channel, password));
                }
            }
            client::Event::Talking(talking) => session.talking = talking,
            client::Event::PermissionDenied => {
                sounds_due.push(sounds::Sound::InsufficientPermissions);
            }
            client::Event::Chat(message) => {
                let channel = session
                    .state
                    .current
                    .as_ref()
                    .map_or_else(String::new, |channel| channel.name.clone());
                arrived = session.chat.receive(message, &channel).cloned();
            }
            client::Event::Poked { from, key, message } => {
                session.chat.poke(key, &from, false, &message);
                sounds_due.push(sounds::Sound::Poke);
                toast = Some((
                    format!("{from} poked you"),
                    if message.is_empty() {
                        "Open Flexspeak to reply.".to_owned()
                    } else {
                        message.clone()
                    },
                ));
                if session.pokes.len() >= 10 {
                    session.pokes.remove(0);
                }
                session.pokes.push(Poke { from, message });
                if !active {
                    session.unseen_pokes = session.unseen_pokes.saturating_add(1);
                }
            }
            client::Event::Done(source) => self.alerts.recover_in(id.0, source),
            client::Event::Streaming(event) => {
                let share_sounds = viewer_sounds(&session.streaming.sharing.status, &event);
                if let Some(name) =
                    asked_to_watch(&session.streaming.sharing.status, &event).first()
                {
                    toast = Some((
                        format!("{name} asked to watch your stream"),
                        "Open Flexspeak to let them in.".to_owned(),
                    ));
                }
                for sound in &share_sounds {
                    let words = match sound {
                        sounds::Sound::StreamStarted => "started streaming",
                        sounds::Sound::StreamStopped => "stopped streaming",
                        _ => continue,
                    };
                    session.log.add(server_log::Tone::Stream, "You", words);
                }
                sounds_due.extend(share_sounds);
                if let streaming::Event::Catalog(catalog) = &event {
                    sounds_due.extend(stream_changes(session, catalog));
                }
                // A streamer's stream starts at the volume saved for them.
                if let streaming::Event::State { stream, .. } = &event
                    && !session.streaming.volumes.contains_key(&stream.publisher)
                    && let Some(volume) = session
                        .state
                        .channels
                        .iter()
                        .flat_map(|channel| &channel.members)
                        .find(|member| member.id == stream.publisher && !member.uid.is_empty())
                        .and_then(|member| self.stream_volumes.get(&member.uid))
                {
                    session.streaming.volumes.insert(stream.publisher, *volume);
                }
                // Your own Stop forgets the stream first, so only an end you didn't ask for
                // finds it still on screen.
                let on_screen = matches!(&event, streaming::Event::State { generation, .. }
                    if session.streaming.watching.iter().any(|w| w.generation == *generation));
                session.streaming.update(event.clone());
                match &event {
                    streaming::Event::State { phase, detail, .. } => match phase {
                        streaming::Phase::Failed => self.raise_for(id, Source::StreamWatch, detail),
                        streaming::Phase::Stopped
                            if on_screen && !streaming::ended_as_expected(detail) =>
                        {
                            self.raise_for(id, Source::StreamEnded, detail)
                        }
                        streaming::Phase::Receiving => {
                            self.alerts.recover_in(id.0, Source::StreamWatch)
                        }
                        _ => {}
                    },
                    streaming::Event::Notice(detail) if !detail.is_empty() => {
                        self.raise_for(id, Source::StreamDiscovery, detail)
                    }
                    _ => {}
                }
            }
            client::Event::Transmitting(value) => {
                if active {
                    self.transmitting = value;
                }
            }
            client::Event::Meter {
                level,
                sent,
                received,
                audio_buffer_bytes,
            } => {
                session.sent = sent;
                session.received = received;
                if active {
                    self.level = level;
                    self.audio_buffer_bytes = audio_buffer_bytes;
                }
            }
            client::Event::Warning { source, detail } => {
                if source == Source::Chat {
                    session.chat.refused();
                }
                // A refused change must not keep showing as ticked.
                if matches!(source, Source::Groups | Source::Flood)
                    && let Some(draft) = &mut session.group_edit
                {
                    draft.wanted.clear();
                    draft.channel_group = None;
                }
                self.raise_for(id, source, detail)
            }
            client::Event::Limited => session.limited = true,
            client::Event::Recovered(source) => {
                if source == Source::Flood {
                    session.limited = false;
                }
                match source.server_scoped() {
                    true => self.alerts.recover_in(id.0, source),
                    false => self.alerts.recover(source),
                }
            }
            client::Event::Controls { confirmed, pending } => {
                session.server_controls = confirmed;
                session.controls_pending = pending;
            }
            client::Event::ChannelPasswordRequired(channel) => {
                if session.can_request_join(loopback, channel) {
                    session.password_channel = Some(channel);
                    session.channel_password.clear();
                    session.password_error =
                        Some("The channel password was missing or incorrect.".into());
                    if active {
                        return Task::Focus(ui::Field::ChannelPassword);
                    }
                }
            }
            client::Event::Usage {
                network,
                connection,
            } => {
                session.network_usage = network;
                session.connection_usage = connection;
                session
                    .connection_health
                    .update(connection.health(), Instant::now());
                if session.connected && !loopback {
                    session.connection_history.push(Instant::now(), connection);
                }
            }
            client::Event::ClientUsage { id: client, result } => {
                if session.connected
                    && session
                        .state
                        .current_members()
                        .iter()
                        .any(|member| member.id == client && !member.is_self)
                {
                    if session.expanded_client == Some(client)
                        && let Ok(sample) = result
                    {
                        let now = Instant::now();
                        session.client_history.push(now, sample.sample(now));
                    }
                    match &result {
                        Ok(sample) => {
                            let now = Instant::now();
                            session
                                .client_health
                                .entry(client)
                                .or_default()
                                .update(sample.sample(now).health(), now);
                        }
                        Err(_) => {
                            session.client_health.remove(&client);
                        }
                    }
                    session.client_usage.insert(client, result);
                }
            }
            client::Event::Stopped(error) => {
                // Name the server before it goes, so its alert isn't taken for the active one's.
                // Filed under its address, so connecting there again clears it.
                let (address, name, was_connected) = (
                    session.address.trim().to_owned(),
                    session.title().to_owned(),
                    session.connected,
                );
                self.alerts.close_server(id.0);
                let audio_failed = matches!(error, Some((Source::Audio, _)));
                match error {
                    Some((Source::Connection, detail)) if !loopback => {
                        let source = if was_connected {
                            Source::Dropped
                        } else {
                            Source::Connection
                        };
                        self.alerts.raise_at(&address, Some(name), source, detail);
                    }
                    Some((source, detail)) => self.alerts.raise(source, detail),
                    None => {}
                }
                if active {
                    let next = self.next_active(id);
                    self.activate(next);
                }
                self.sessions.remove(index);
                if self.sessions.is_empty() {
                    // The devices close with the last server, and with them what was wrong.
                    self.alerts.recover(Source::AudioFallback);
                    if !audio_failed {
                        self.alerts.recover(Source::Audio);
                    }
                    self.ptt.store(false, Ordering::Release);
                    self.transmitting = false;
                    self.level = 0.0;
                    self.audio_buffer_bytes = 0;
                    self.loopback = false;
                    self.resume = None;
                    self.home.status = "Disconnected".into();
                    if self.closing {
                        return Task::Exit;
                    }
                }
            }
        }
        for sound in sounds_due {
            self.play_sound(sound);
        }
        if toast.is_some() {
            self.background_attention(toast);
        }
        if restore && !self.loopback {
            self.chat_restore(id);
        }
        if let Some(entry) = arrived {
            self.chat_arrived(id, entry);
        }
        Task::none()
    }

    // ---- chat history ----
    /// Runs `work` on the history file, opening it on first use. A failure is reported once
    /// and the file is then left alone until the app restarts. `forced` works on it even
    /// while keeping history is switched off, as clearing it must.
    fn with_history<T>(
        &mut self,
        forced: bool,
        work: impl FnOnce(&mut chat_store::Store) -> anyhow::Result<T>,
    ) -> Option<T> {
        if self.chat_store_failed || !(forced || self.chat_options.history) {
            return None;
        }
        let result = match &mut self.chat_store {
            Some(store) => work(store),
            None => {
                let path = self
                    .settings_path
                    .as_deref()?
                    .parent()?
                    .join(chat_store::FILE);
                chat_store::Store::open(&path).and_then(|mut store| {
                    let value = work(&mut store);
                    self.chat_store = Some(store);
                    value
                })
            }
        };
        match result {
            Ok(value) => Some(value),
            Err(error) => {
                self.chat_store = None;
                self.chat_store_failed = true;
                self.alerts.raise(
                    Source::ChatHistory,
                    format!("{error:#}. Messages are kept until you quit."),
                );
                None
            }
        }
    }

    /// What a server's history is filed under: the address it is connected by.
    fn history_key(session: &ServerSession) -> String {
        session.address.trim().to_ascii_lowercase()
    }

    /// Writes the lines added since the last time to the history on disk.
    fn chat_persist(&mut self) {
        for index in 0..self.sessions.len() {
            if !self.sessions[index].chat.has_unsaved() {
                continue;
            }
            let lines = self.sessions[index].chat.unsaved();
            if self.loopback {
                continue;
            }
            let server = Self::history_key(&self.sessions[index]);
            self.with_history(false, |store| store.append(&server, &lines));
        }
    }

    /// Brings a server's saved conversations back when it first connects.
    fn chat_restore(&mut self, id: hub::SessionId) {
        let Some(server) = self.session(id).map(Self::history_key) else {
            return;
        };
        if let Some(history) = self.with_history(false, |store| store.load(&server))
            && let Some(session) = self.session_mut(id)
        {
            session.chat.restore(history);
        }
    }

    /// Remembers that a conversation's tab was closed, or opened again.
    fn chat_tab(&mut self, scope: &chat::Scope, closed: bool) {
        let server = Self::history_key(self.server());
        let scope = scope.clone();
        self.with_history(false, |store| match closed {
            true => store.close(&server, &scope),
            false => store.reopen(&server, &scope),
        });
    }

    // ---- text chat ----
    /// Whether the chat column is docked beside the stage, rather than behind the window's edge.
    fn chat_docked(&self) -> bool {
        !self.chat_options.hidden
            && self.window_size.0 >= ui::CHAT_MIN_DOCK_WINDOW
            && !(self.chat_options.hide_while_watching
                && self.server().streaming.active()
                && !self.ui.chat_over_stream)
    }

    /// Whether the chat is on screen right now: docked, or peeking over the stage.
    fn chat_showing(&self, now: Instant) -> bool {
        self.live()
            // In the background, a message waits as unread for you to come back.
            && self.window_focused
            && !self.window_hidden
            && self.tab == Tab::Home
            && self.onboarding.is_none()
            // Settings covers it.
            && self.settings_page.is_none()
            && (self.chat_docked() || self.ui.chat_peek_open(now))
    }

    /// Clears the unread counts of what the chat has on screen, and its pop-up.
    fn chat_seen(&mut self) {
        // Read back, the lines below wait under the "new messages" control.
        if self.chat_showing(Instant::now()) && !self.ui.chat_back {
            let layout = self.chat_options.layout;
            self.server_mut().chat.mark_seen(layout);
            self.ui.chat_toast = None;
        }
    }

    /// Counts a message you haven't seen, and pops it up while the chat is hidden.
    fn chat_arrived(&mut self, id: hub::SessionId, entry: chat::Entry) {
        if entry.own {
            return;
        }
        let now = Instant::now();
        let (layout, options) = (self.chat_options.layout, self.chat_options);
        let active = self.active == Some(id);
        let showing = active && self.chat_showing(now);
        let back = self.ui.chat_back;
        // A private message is for you alone: in the background it sounds and flashes the
        // taskbar. Channel and server messages only wait as unread.
        if entry.scope.is_private() && !self.window_focused {
            self.play_sound(sounds::Sound::PrivateMessage);
            self.background_attention(None);
        }
        let Some(session) = self.session_mut(id) else {
            return;
        };
        if showing && !back && session.chat.shows(layout, &entry.scope) {
            return;
        }
        session.chat.mark_unread(&entry.scope);
        if active
            && !showing
            && options.popups
            && (entry.scope.is_private() || !options.popups_private_only)
            && self.onboarding.is_none()
        {
            self.ui.chat_toast = Some(ui::ChatToast {
                entry,
                until: now + ui::ChatToast::SHOWN,
            });
            self.ui.chat_toast_hover = false;
        }
    }

    /// The person a private conversation is with, while they're on the server and visible.
    fn chat_member(&self, key: &str) -> Option<&client::Member> {
        self.server()
            .state
            .channels
            .iter()
            .flat_map(|channel| &channel.members)
            .find(|member| !member.is_self && client::chat_key(&member.uid, member.id) == key)
    }

    /// Shows a conversation and points the message box at it.
    fn chat_open(&mut self, scope: chat::Scope) {
        let name = match &scope {
            chat::Scope::Private(key) => self
                .server()
                .chat
                .peer(key)
                .map(|peer| peer.name.clone())
                .or_else(|| self.chat_member(key).map(|member| member.name.clone()))
                .unwrap_or_default(),
            _ => String::new(),
        };
        if matches!(scope, chat::Scope::Private(_) | chat::Scope::Past(_)) {
            self.chat_tab(&scope, false);
        }
        self.server_mut().chat.open(scope, &name);
    }

    /// Docks the chat column if it was hidden.
    fn chat_reveal(&mut self) {
        if self.chat_options.hidden {
            self.set_chat_options(settings::Chat {
                hidden: false,
                ..self.chat_options
            });
        }
        if self.chat_options.hide_while_watching && self.server().streaming.active() {
            self.ui.chat_over_stream = true;
        }
    }

    /// Changes a chat setting and stores only that. Setup keeps it for its own save.
    fn set_chat_options(&mut self, options: settings::Chat) {
        self.chat_options = options;
        if self.onboarding.is_none() {
            self.save_preference(
                |saved| saved.chat = options,
                "The chat choice applies for now but was not saved",
            );
        }
        self.chat_seen();
    }

    fn chat_action(&mut self, action: chat::Action) -> Task {
        use chat::Action;
        match action {
            Action::Draft(text) => {
                let mut end = text.len().min(chat::MAX_BYTES);
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                let chat = &mut self.server_mut().chat;
                // More than a character over: a paste was cut, which the box says.
                chat.draft_cut = text.len() > chat::MAX_BYTES + 4;
                chat.draft = text[..end].to_owned();
            }
            Action::Send => {
                if !self.live() {
                    return Task::none();
                }
                let chat = &self.server().chat;
                let Some(text) = chat::outgoing(&chat.draft) else {
                    return Task::none();
                };
                let scope = chat.target.clone();
                let target = match &chat.target {
                    chat::Scope::Channel => client::ChatTarget::Channel,
                    chat::Scope::Server => client::ChatTarget::Server,
                    // The box is locked while they're away, so this is a race with them leaving.
                    chat::Scope::Private(key) => match self.chat_member(key) {
                        Some(member) => client::ChatTarget::Client(member.id),
                        None => return Task::none(),
                    },
                    // History of a channel you left: there is nobody to write to.
                    chat::Scope::Past(_) => return Task::none(),
                };
                self.begin_alert(Source::Chat);
                if self.command(client::Command::Chat(target, text.clone())) {
                    let chat = &mut self.server_mut().chat;
                    chat.sent(scope, &text, Instant::now());
                    chat.draft.clear();
                    chat.draft_cut = false;
                }
            }
            Action::Target(scope) => {
                self.chat_open(scope);
                self.chat_seen();
                self.ui.chat_input.focused = self.chat_showing(Instant::now());
            }
            Action::Filter(filter) => {
                self.server_mut().chat.filter = filter;
                self.chat_seen();
            }
            Action::Narrow(scope) => {
                self.server_mut().chat.narrow(scope);
                self.chat_seen();
            }
            Action::Everything => {
                self.server_mut().chat.everything = true;
                self.chat_seen();
            }
            Action::SendTo(scope) => {
                let name = match &scope {
                    chat::Scope::Private(key) => self
                        .chat_member(key)
                        .map(|member| member.name.clone())
                        .unwrap_or_default(),
                    _ => String::new(),
                };
                self.server_mut().chat.aim(scope, &name);
                self.ui.chat_input.focused = self.chat_showing(Instant::now());
            }
            Action::Message(id) => {
                if self.live()
                    && let Some(member) = self.find_member(id).filter(|member| !member.is_self)
                {
                    let key = client::chat_key(&member.uid, member.id);
                    let name = member.name.clone();
                    self.server_mut()
                        .chat
                        .open(chat::Scope::Private(key), &name);
                    self.chat_reveal();
                    self.chat_seen();
                    self.ui.chat_input.focused = true;
                }
            }
            Action::ClosePeer(key) => {
                self.chat_tab(&chat::Scope::Private(key.clone()), true);
                self.server_mut().chat.close_peer(&key);
            }
            Action::ClosePast(id) => {
                self.chat_tab(&chat::Scope::Past(id), true);
                self.server_mut().chat.close_past(id);
            }
            Action::ClearHistory => {
                // The first press asks; the second, while Settings is still open, clears.
                if !std::mem::replace(&mut self.ui.chat_clear_armed, true) {
                    return Task::none();
                }
                self.ui.chat_clear_armed = false;
                self.with_history(true, |store| store.clear());
                for session in &mut self.sessions {
                    session.chat.clear();
                }
                self.ui.chat_toast = None;
                self.settings_message = Some("Chat history cleared".into());
            }
            Action::People => {
                let chat = &self.server().chat;
                let waiting =
                    |peer: &&chat::Peer| chat.unread(&chat::Scope::Private(peer.key.clone())) > 0;
                let pick = chat
                    .peers
                    .iter()
                    .find(waiting)
                    .or(chat.peers.last())
                    .map(|peer| chat::Scope::Private(peer.key.clone()));
                if let Some(scope) = pick {
                    return self.chat_action(Action::Target(scope));
                }
            }
            Action::Toggle => {
                if !self.live() {
                    return Task::none();
                }
                if self.chat_docked() {
                    if self.chat_options.hide_while_watching && self.server().streaming.active() {
                        self.ui.chat_over_stream = false;
                    } else {
                        self.set_chat_options(settings::Chat {
                            hidden: true,
                            ..self.chat_options
                        });
                    }
                    // Or the message box would hold the column open as a peek.
                    self.ui.chat_input.focused = false;
                    self.ui.chat_peek = ui::RailPeek::default();
                    self.ui.chat_lists = Default::default();
                } else if self.window_size.0 < ui::CHAT_MIN_DOCK_WINDOW {
                    // No room to dock: hold the peek open instead, or let it go.
                    self.ui.chat_peek.toggle_pin();
                } else {
                    self.chat_reveal();
                }
                self.chat_seen();
            }
            Action::Open(scope) => {
                self.chat_open(scope);
                self.chat_reveal();
                self.chat_seen();
                self.ui.chat_input.focused = true;
            }
            Action::Reply(scope) => {
                // The focused message box holds the column open over the stage.
                self.chat_open(scope);
                self.ui.chat_input.focused = true;
                self.chat_seen();
            }
            Action::DismissToast => self.ui.chat_toast = None,
            Action::Layout(layout) => self.set_chat_options(settings::Chat {
                layout,
                ..self.chat_options
            }),
            Action::Options(options) => self.set_chat_options(options),
        }
        Task::none()
    }

    // ---- view-independent helpers ----
    fn select_client(&mut self, id: Option<u16>) {
        if id.is_some_and(|id| {
            !self
                .server()
                .state
                .current_members()
                .iter()
                .any(|member| member.id == id)
        }) {
            return;
        }
        self.server_mut().clear_client_details();
        self.server_mut().expanded_client = id;
    }
    fn footer_connection_status(&self) -> (usage::Health, &'static str, String) {
        if self.live() && self.server().state.current.is_some() {
            let health = self
                .server()
                .connection_health
                .show(self.server().connection_usage.health());
            let label = if health == usage::Health::Unknown {
                "Unknown"
            } else {
                health.label()
            };
            (
                health,
                label,
                self.server().connection_usage.health_tooltip(),
            )
        } else {
            let label = if self.server().stopping {
                "Stopping"
            } else if self.loopback && self.server().connected {
                "Local test"
            } else if !self.idle() {
                "Connecting"
            } else {
                "Offline"
            };
            (usage::Health::Unknown, label, self.server().status.clone())
        }
    }
    fn microphone_status(&self) -> &'static str {
        if !self.server().connected || self.server().stopping {
            "Inactive"
        } else if self.loopback {
            match (self.transmission.mode, self.transmitting) {
                (transmission::Mode::PushToTalk, false) => "Local test: hold push-to-talk",
                (transmission::Mode::VoiceActivation, false) => "Local test: waiting for speech",
                _ => "Local test only",
            }
        } else if self.server().deafened {
            "Muted (deafened)"
        } else if self.server().muted {
            "Muted"
        } else if !self.server().state.can_talk {
            "Blocked by server"
        } else {
            match (self.transmission.mode, self.transmitting) {
                (transmission::Mode::PushToTalk, false) => "Waiting for push-to-talk",
                (transmission::Mode::VoiceActivation, false) => "Waiting for speech",
                (transmission::Mode::Continuous, _) => "Unmuted",
                _ => "Transmitting",
            }
        }
    }

    fn microphone_icon(&self) -> Icon {
        if self.server().connected
            && !self.server().stopping
            && (self.loopback
                || (!self.server().muted
                    && !self.server().deafened
                    && self.server().state.can_talk))
        {
            Icon::Mic
        } else {
            Icon::MicOff
        }
    }

    fn playback_icon(&self) -> Icon {
        if self.server().deafened {
            Icon::HeadphoneOff
        } else {
            Icon::Headphones
        }
    }

    fn microphone_meter_level(&self) -> f32 {
        if self.microphone_icon() == Icon::Mic {
            self.level.sqrt().min(1.0)
        } else {
            0.0
        }
    }

    fn can_request_join(&self, id: u64) -> bool {
        self.server().can_request_join(self.loopback, id)
    }
    fn find_member(&self, id: u16) -> Option<&client::Member> {
        self.server()
            .state
            .channels
            .iter()
            .flat_map(|channel| &channel.members)
            .find(|member| member.id == id)
    }
    fn member_channel(&self, id: u16) -> Option<u64> {
        self.server()
            .state
            .channels
            .iter()
            .find(|channel| channel.members.iter().any(|member| member.id == id))
            .map(|channel| channel.id)
    }
    fn own_id(&self) -> Option<u16> {
        self.server()
            .state
            .channels
            .iter()
            .flat_map(|channel| &channel.members)
            .find(|member| member.is_self)
            .map(|member| member.id)
    }
    /// Whether these clients can be moved: the server hasn't denied it for any of them.
    fn can_move(&self, ids: &[u16]) -> bool {
        self.server().connected
            && !self.loopback
            && !self.server().stopping
            && !ids.is_empty()
            && ids.iter().all(|id| {
                self.find_member(*id)
                    .is_some_and(|member| member.is_self || member.can_move != Some(false))
            })
    }
    /// One person's name, or how many people these are.
    fn people(&self, ids: &[u16]) -> String {
        match ids {
            [one] => self
                .find_member(*one)
                .map_or_else(|| "1 person".to_owned(), |member| member.name.clone()),
            many => format!("{} people", many.len()),
        }
    }
    /// Whether all, none or some of these clients are in a server group.
    fn server_group_mark(&self, ids: &[u16], group: u64) -> GroupMark {
        let holds = |id: &u16| {
            self.find_member(*id)
                .is_some_and(|member| member.server_groups.contains(&group))
        };
        if ids.iter().all(holds) {
            GroupMark::On
        } else if ids.iter().any(holds) {
            GroupMark::Mixed
        } else {
            GroupMark::Off
        }
    }
    /// The channel group these clients share, if they all have the same one.
    fn shared_channel_group(&self, ids: &[u16]) -> Option<u64> {
        let mut groups = ids
            .iter()
            .map(|id| self.find_member(*id).map(|member| member.channel_group));
        let first = groups.next()??;
        groups.all(|group| group == Some(first)).then_some(first)
    }
    fn send_group_edit(&mut self, ids: Vec<u16>, edit: client::GroupEdit) {
        self.begin_alert(Source::Groups);
        self.command(client::Command::SetGroups(ids, edit));
    }
    /// Whether the member is sharing a stream, in any channel. The server flags streamers
    /// everywhere you can see; the catalog adds your own channel's streams as they are found.
    fn is_streaming(&self, member: &client::Member) -> bool {
        let streaming = &self.server().streaming;
        if member.is_self {
            return streaming.sharing.status.live;
        }
        streaming.streamers.contains(&member.id)
            || streaming
                .catalog
                .iter()
                .any(|stream| stream.publisher == member.id)
    }
    fn member_health(
        &self,
        member: &client::Member,
        current_channel: bool,
    ) -> (usage::Health, String) {
        if member.is_self {
            let (health, _, detail) = self.footer_connection_status();
            (health, detail)
        } else if !current_channel {
            (
                usage::Health::Unknown,
                "Connection health is only sampled for your current channel.".into(),
            )
        } else if let Some(Err(error)) = self.server().client_usage.get(&member.id) {
            (usage::Health::Unknown, (*error).into())
        } else {
            let quality = self.client_quality(member.id, Instant::now());
            let reading = quality.health();
            let health = self
                .server()
                .client_health
                .get(&member.id)
                .map_or(reading, |steady| steady.show(reading));
            (health, quality.health_tooltip())
        }
    }
    /// A playing stream's trouble, with the side it is most likely on.
    fn stream_problem(&self, watch: &streaming::Watching) -> Option<streaming::health::Problem> {
        if watch.phase != streaming::Phase::Receiving {
            return None;
        }
        let (ours, _, _) = self.footer_connection_status();
        let theirs = self
            .find_member(watch.stream.publisher)
            .map_or(usage::Health::Unknown, |member| {
                self.member_health(member, true).0
            });
        streaming::health::attribute(watch.stats.media, ours, theirs)
    }
    fn client_quality(&self, id: u16, now: Instant) -> usage::ConnectionUsage {
        self.server()
            .client_usage
            .get(&id)
            .and_then(|result| result.as_ref().ok())
            .map_or_else(usage::ConnectionUsage::default, |sample| sample.sample(now))
    }
    fn stop_trace(&mut self) -> Task {
        let Some(recording) = self.recording.take() else {
            return Task::none();
        };
        let report = recording.finish();
        self.trace_summary = Some(report.summary.clone());
        self.trace_saving = true;
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || report.save().map_err(|e| format!("{e:#}")))
                    .await
                    .map_err(|e| e.to_string())
                    .and_then(|result| result)
            },
            Message::TraceSaved,
        )
    }
}

impl Drop for App {
    fn drop(&mut self) {
        for session in &self.sessions {
            if let Some(sender) = &session.session {
                let _ = sender.try_send(client::Command::Stop);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Starts a pretend connection on `sender` and shows it, as `start` would.
    fn live(app: &mut App, sender: mpsc::Sender<client::Command>) -> hub::SessionId {
        let id = hub::SessionId(app.next_session);
        app.next_session += 1;
        let mut session = ServerSession::new(
            id,
            String::new(),
            app.address.trim().to_owned(),
            app.password.clone(),
            app.home.streaming.surfaces.clone(),
            app.home.streaming.visible.clone(),
        );
        session.session = Some(sender);
        app.sessions.push(session);
        app.activate(Some(id));
        id
    }

    /// A pretend connection with its commands to read.
    fn connection(app: &mut App) -> (hub::SessionId, mpsc::Receiver<client::Command>) {
        let (sender, commands) = mpsc::channel(32);
        (live(app, sender), commands)
    }

    fn setup_fixture(name: &str) -> (App, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "flexspeak-setup-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let input = audio::DeviceChoice {
            id: "mic-a".into(),
            name: "Microphone A".into(),
        };
        let other = audio::DeviceChoice {
            id: "mic-b".into(),
            name: "Microphone B".into(),
        };
        let output = audio::DeviceChoice {
            id: "speaker".into(),
            name: "Speakers".into(),
        };
        let mut app = App::with_devices(
            audio::Devices {
                inputs: vec![input.clone(), other],
                outputs: vec![output.clone()],
                input: Some(input),
                output: Some(output),
            },
            None,
        );
        app.load_settings(root.join("settings.json"));
        (app, root)
    }

    /// A saved profile with one server, written to a fresh file.
    fn saved_fixture(name: &str) -> (App, PathBuf) {
        let (mut app, root) = setup_fixture(name);
        let path = root.join("settings.json");
        settings::save(
            &path,
            &settings::Settings {
                servers: vec![settings::Server::new("one.example")],
                nickname: "Alex".into(),
                ..settings::Settings::default()
            },
        )
        .unwrap();
        app.load_settings(path);
        assert!(app.onboarding.is_none());
        (app, root)
    }

    fn add_server(app: &mut App, name: &str, address: &str, password: &str) {
        let _ = app.update(Message::AddServer);
        let _ = app.update(Message::ServerName(name.into()));
        let _ = app.update(Message::ServerAddress(address.into()));
        let _ = app.update(Message::ServerPassword(password.into()));
        let _ = app.update(Message::SubmitServer);
    }

    #[test]
    fn servers_are_added_edited_and_removed_and_save_at_once() {
        let (mut app, root) = saved_fixture("servers");
        let path = root.join("settings.json");
        add_server(&mut app, "Friends", " two.example:9988 ", "secret");
        assert!(app.server_editor.is_none());
        let stored = settings::load(&path).unwrap().unwrap();
        assert_eq!(
            stored.servers,
            [
                settings::Server::new("one.example"),
                settings::Server {
                    name: "Friends".into(),
                    address: "two.example:9988".into(),
                    ..Default::default()
                },
            ]
        );
        assert_eq!(stored.nickname, "Alex", "the rest of the file is untouched");
        assert!(!std::fs::read_to_string(&path).unwrap().contains("secret"));
        // Empty and repeated addresses keep the dialog open with a reason.
        add_server(&mut app, "", "  ", "");
        assert!(app.server_editor.as_ref().unwrap().error.is_some());
        let _ = app.update(Message::CancelServer);
        add_server(&mut app, "Again", "two.example:9988", "");
        assert!(app.server_editor.as_ref().unwrap().error.is_some());
        let _ = app.update(Message::CancelServer);
        // Editing keeps the place in the list and the session password.
        let _ = app.update(Message::EditServer(1));
        assert_eq!(app.server_editor.as_ref().unwrap().password, "secret");
        let _ = app.update(Message::ServerName("Squad".into()));
        let _ = app.update(Message::SubmitServer);
        assert_eq!(app.servers[1].name, "Squad");
        assert_eq!(
            settings::load(&path).unwrap().unwrap().servers[1].name,
            "Squad"
        );
        let _ = app.update(Message::EditServer(0));
        let _ = app.update(Message::RemoveServer);
        assert_eq!(app.servers.len(), 2, "the first press only asks");
        assert!(app.server_editor.as_ref().unwrap().remove_armed);
        let _ = app.update(Message::RemoveServer);
        assert_eq!(app.servers.len(), 1);
        assert_eq!(app.servers[0].address, "two.example:9988");
        // The last server stays.
        let _ = app.update(Message::EditServer(0));
        let _ = app.update(Message::RemoveServer);
        assert_eq!(app.servers.len(), 1);
        assert_eq!(settings::load(&path).unwrap().unwrap().servers.len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn auto_connect_is_set_per_server_and_survives_editing_it() {
        let (mut app, _root) = saved_fixture("auto-connect");
        add_server(&mut app, "", "two.example", "");
        assert!(app.auto_connect_servers().is_empty());
        let _ = app.update(Message::ServerAutoConnect(1, true));
        assert_eq!(app.auto_connect_servers(), [1]);
        let saved = settings::load(app.settings_path.as_deref().unwrap())
            .unwrap()
            .unwrap();
        assert!(saved.servers[1].auto_connect && !saved.servers[0].auto_connect);

        let _ = app.update(Message::EditServer(1));
        let _ = app.update(Message::ServerName("Two".into()));
        let _ = app.update(Message::SubmitServer);
        assert_eq!(app.auto_connect_servers(), [1], "editing keeps it");

        // Without a microphone nothing starts, but the field shows which server was tried.
        app.input = None;
        let _ = app.update(Message::AutoConnect);
        assert_eq!(app.address, "two.example");

        let _ = app.update(Message::ServerAutoConnect(1, false));
        assert!(app.auto_connect_servers().is_empty());
    }

    #[test]
    fn each_server_connects_once_and_the_ones_in_use_are_locked() {
        let (mut app, root) = saved_fixture("connect");
        add_server(&mut app, "", "two.example", "two-password");
        // Without a microphone the connection can't start, so the fields say what it would use.
        app.input = None;
        let _ = app.update(Message::Connect(1));
        assert_eq!(app.address, "two.example");
        assert_eq!(app.password, "two-password");
        let _ = app.update(Message::Connect(0));
        assert_eq!(app.nickname, "Alex");
        assert_eq!(app.address, "one.example");
        assert!(app.password.is_empty());
        // With a session on the second server, it can't be connected again or edited.
        app.address = "two.example".into();
        let (_id, mut commands) = connection(&mut app);
        let _ = app.update(Message::Connect(1));
        assert_eq!(app.sessions.len(), 1, "one session per server");
        assert!(app.server_in_use(1) && !app.server_in_use(0));
        let _ = app.update(Message::EditServer(1));
        let _ = app.update(Message::ServerAddress("elsewhere.example".into()));
        let _ = app.update(Message::SubmitServer);
        assert_eq!(app.servers[1].address, "two.example");
        let _ = app.update(Message::RemoveServer);
        assert_eq!(app.servers.len(), 2);
        let _ = app.update(Message::CancelServer);
        assert!(commands.try_recv().is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    type Connection = (hub::SessionId, mpsc::Receiver<client::Command>);

    fn two_servers() -> (App, Connection, Connection) {
        let mut app = App::with_devices(audio::Devices::default(), None);
        let first = connection(&mut app);
        app.server_mut().connected = true;
        app.server_mut().name = "First".into();
        let second = connection(&mut app);
        app.server_mut().connected = true;
        app.server_mut().name = "Second".into();
        let _ = app.update(Message::SwitchServer(first.0));
        assert_eq!(app.active, Some(first.0));
        (app, first, second)
    }

    #[test]
    fn mute_deafen_and_shortcuts_act_on_the_active_server_only() {
        let (mut app, (first, mut first_commands), (second, mut second_commands)) = two_servers();
        let _ = app.update(Message::Shortcut(shortcuts::Action::Mute));
        assert!(matches!(
            first_commands.try_recv(),
            Ok(client::Command::Mute(false))
        ));
        assert!(second_commands.try_recv().is_err());
        assert!(!app.session(first).unwrap().muted);
        assert!(app.session(second).unwrap().muted);
        let _ = app.update(Message::SwitchServer(second));
        assert!(app.server().muted, "each server keeps its own mute");
        let _ = app.update(Message::Deafen);
        assert!(matches!(
            second_commands.try_recv(),
            Ok(client::Command::Deafen(true))
        ));
        assert!(!app.session(first).unwrap().deafened);
    }

    #[test]
    fn a_background_error_names_its_server_and_leaves_the_active_view_alone() {
        let (mut app, (first, _a), (second, _b)) = two_servers();
        app.server_mut().talking = BTreeSet::from([2]);
        let _ = app.update(Message::Backend(
            second,
            client::Event::Warning {
                source: Source::Join,
                detail: "No room".into(),
            },
        ));
        let alert = app.alerts.current().unwrap();
        assert_eq!(alert.server.as_deref(), Some("Second"));
        assert_eq!(alert.source, Source::Join);
        let _ = app.update(Message::DismissAlert(app.alerts.current().unwrap().id));
        // Its connection dropping takes it off the rail and nothing else.
        let _ = app.update(Message::Backend(
            second,
            client::Event::Status("Lost".into()),
        ));
        let _ = app.update(Message::Backend(
            second,
            client::Event::Stopped(Some((Source::Connection, "Kicked".into()))),
        ));
        assert_eq!(app.active, Some(first));
        assert_eq!(app.server().talking, BTreeSet::from([2]));
        assert!(app.server().connected);
        assert_eq!(
            app.alerts.current().unwrap().server.as_deref(),
            Some("Second")
        );
        assert!(app.session(second).is_none());
        // An event from a server that has gone is dropped.
        let _ = app.update(Message::Backend(
            second,
            client::Event::Status("Late".into()),
        ));
        assert_eq!(app.sessions.len(), 1);
    }

    #[test]
    fn a_poke_on_a_background_server_counts_and_clears_on_switch() {
        let (mut app, (first, _a), (second, _b)) = two_servers();
        for from in ["Mara", "Theo"] {
            let _ = app.update(Message::Backend(
                second,
                client::Event::Poked {
                    key: "uid-9".into(),
                    from: from.into(),
                    message: String::new(),
                },
            ));
        }
        assert_eq!(app.session(second).unwrap().unseen_pokes, 2);
        assert!(
            app.server().pokes.is_empty(),
            "no dialog over the active view"
        );
        let _ = app.update(Message::SwitchServer(second));
        assert_eq!(app.session(second).unwrap().unseen_pokes, 0);
        assert_eq!(app.server().pokes.len(), 2, "still there to read");
        // On the active server it's the usual notification, with no marker.
        let _ = app.update(Message::Backend(
            second,
            client::Event::Poked {
                key: "uid-9".into(),
                from: "Noor".into(),
                message: String::new(),
            },
        ));
        assert_eq!(app.session(second).unwrap().unseen_pokes, 0);
        let _ = app.update(Message::SwitchServer(first));
        assert_eq!(app.session(first).unwrap().unseen_pokes, 0);
    }

    #[test]
    fn disconnecting_activates_another_server_or_returns_home() {
        let (mut app, (first, mut first_commands), (second, _b)) = two_servers();
        let _ = app.update(Message::Stop);
        assert!(matches!(
            first_commands.try_recv(),
            Ok(client::Command::Stop)
        ));
        assert_eq!(
            app.active,
            Some(second),
            "the other server takes over at once"
        );
        assert!(
            app.rail_sessions().all(|session| session.id != first),
            "one being disconnected leaves the rail"
        );
        let _ = app.update(Message::Backend(first, client::Event::Stopped(None)));
        let _ = app.update(Message::Stop);
        assert_eq!(
            app.active,
            Some(second),
            "the last one stays on screen as it stops"
        );
        let _ = app.update(Message::Backend(second, client::Event::Stopped(None)));
        assert_eq!(app.active, None);
        assert!(app.idle());
        assert_eq!(app.server().status, "Disconnected");
    }

    #[test]
    fn quitting_waits_for_every_server_to_stop() {
        let (mut app, (first, mut a), (second, mut b)) = two_servers();
        assert!(matches!(app.update(Message::Quit), Task::None));
        assert!(matches!(a.try_recv(), Ok(client::Command::Stop)));
        assert!(matches!(b.try_recv(), Ok(client::Command::Stop)));
        assert!(matches!(
            app.update(Message::Backend(first, client::Event::Stopped(None))),
            Task::None
        ));
        assert!(matches!(
            app.update(Message::Backend(second, client::Event::Stopped(None))),
            Task::Exit
        ));
    }

    #[test]
    fn only_the_active_server_sets_what_the_mic_meter_shows() {
        let (mut app, (first, _a), (second, _b)) = two_servers();
        let meter = |level| client::Event::Meter {
            level,
            sent: 3,
            received: 4,
            audio_buffer_bytes: 9,
        };
        let _ = app.update(Message::Backend(second, meter(0.9)));
        assert_eq!(app.level, 0.0);
        assert_eq!(app.session(second).unwrap().sent, 3);
        let _ = app.update(Message::Backend(first, meter(0.4)));
        assert_eq!(app.level, 0.4);
        let _ = app.update(Message::Backend(second, client::Event::Transmitting(true)));
        assert!(!app.transmitting);
    }

    #[test]
    fn a_microphone_test_and_server_connections_exclude_each_other() {
        let (mut app, _root) = setup_fixture("exclusive");
        let (sender, _commands) = mpsc::channel(4);
        live(&mut app, sender);
        app.loopback = true;
        assert!(!app.idle());
        // The running test holds the fields and blocks connecting.
        let _ = app.update(Message::Connect(0));
        assert_eq!(app.sessions.len(), 1);
    }

    #[test]
    fn tucked_away_streams_come_back_with_the_grid_or_a_layout_choice() {
        let (mut app, _first, _second) = two_servers();
        let _ = app.update(Message::HideOtherStreams(true));
        assert!(app.server().streaming.others_hidden);
        let _ = app.update(Message::StreamLayout(None));
        assert!(
            !app.server().streaming.others_hidden,
            "the grid shows every stream"
        );
        let _ = app.update(Message::HideOtherStreams(true));
        let _ = app.update(Message::StreamLayout(Some(streaming::Strip::Beside)));
        assert!(
            !app.server().streaming.others_hidden,
            "choosing where the others go shows them"
        );
        let _ = app.update(Message::HideOtherStreams(true));
        let _ = app.update(Message::FocusStream(None));
        assert!(!app.server().streaming.others_hidden);
    }

    #[test]
    fn switching_servers_reports_streaming_from_the_new_one() {
        let (mut app, (first, _a), (second, _b)) = two_servers();
        app.server_mut()
            .streaming
            .watching
            .push(streaming::Watching {
                stream: streaming::StreamInfo {
                    id: "a".into(),
                    publisher: 7,
                    name: "Window".into(),
                    viewers: 1,
                    has_audio: false,
                    server_relayed: false,
                },
                slot: 0,
                generation: 1,
                phase: streaming::Phase::Receiving,
                detail: String::new(),
                stats: streaming::Stats::default(),
                frame: None,
            });
        app.server_mut().streaming.sharing.status.active = true;
        let _ = app.update(Message::StreamLayout(Some(streaming::Strip::Beside)));
        let _ = app.update(Message::SwitchServer(second));
        assert!(app.server().streaming.watching.is_empty());
        assert_eq!(app.server().streaming.strip, streaming::Strip::Beside);
        assert!(app.session(first).unwrap().streaming.watching.is_empty());
        assert!(
            app.session(first).unwrap().streaming.sharing.status.active,
            "a share in the background is left running"
        );
    }

    #[test]
    fn first_run_only_saves_after_six_steps_and_restores_devices_without_connecting() {
        let (mut app, root) = setup_fixture("complete");
        let path = root.join("settings.json");
        assert_eq!(app.onboarding, Some(onboarding::Step::Username));
        assert!(app.nickname.is_empty() && app.address.is_empty() && !path.exists());
        let _ = app.update(Message::SetupNext);
        assert_eq!(app.onboarding, Some(onboarding::Step::Username));
        assert!(app.onboarding_error.is_some());
        let _ = app.update(Message::Nickname("Alex".into()));
        let _ = app.update(Message::SetupNext);
        assert_eq!(app.onboarding, Some(onboarding::Step::Server));
        let _ = app.update(Message::SetupNext);
        assert_eq!(app.onboarding, Some(onboarding::Step::Server));
        let _ = app.update(Message::Address(" voice.example:9987 ".into()));
        let _ = app.update(Message::Password("temporary-test-password".into()));
        let _ = app.update(Message::SetupNext);
        assert_eq!(app.onboarding, Some(onboarding::Step::Permissions));
        // Permission decisions are made by the OS; denied/deferred access never becomes a fake grant.
        app.permissions.pending = Some(permissions::Kind::Microphone);
        let _ = app.update(Message::SetupNext);
        assert_eq!(app.onboarding, Some(onboarding::Step::Permissions));
        let _ = app.update(Message::PermissionDone(
            permissions::Kind::Microphone,
            Ok("Access denied".into()),
        ));
        let _ = app.update(Message::SetupNext);
        assert_eq!(app.onboarding, Some(onboarding::Step::Devices));
        let _ = app.update(Message::Input(app.devices.inputs[1].clone()));
        let (sender, _) = mpsc::channel(4);
        live(&mut app, sender);
        app.loopback = true;
        let _ = app.update(Message::SetupNext);
        assert_eq!(app.onboarding, Some(onboarding::Step::Devices));
        app.sessions.clear();
        app.active = None;
        app.loopback = false;
        let _ = app.update(Message::SetupNext);
        assert_eq!(app.onboarding, Some(onboarding::Step::Chat));
        // The layout picked here is kept for setup's own save.
        let _ = app.update(Message::Chat(chat::Action::Layout(chat::Layout::Feed)));
        let _ = app.update(Message::SetupNext);
        assert_eq!(app.onboarding, Some(onboarding::Step::Hotkeys));
        assert!(!path.exists());
        app.keybind_draft.toggle_mute = "F8".into();
        app.keybind_draft.toggle_deafen = "F9".into();
        let _ = app.update(Message::SetupNext);
        assert!(app.onboarding.is_none() && app.server().session.is_none());
        assert_eq!(app.password, "temporary-test-password");
        let saved = settings::load(&path).unwrap().unwrap();
        assert_eq!(saved.nickname, "Alex");
        assert_eq!(saved.servers, [settings::Server::new("voice.example:9987")]);
        assert_eq!(saved.chat.layout, chat::Layout::Feed);
        assert_eq!(
            app.server_passwords
                .get("voice.example:9987")
                .map(String::as_str),
            Some("temporary-test-password"),
            "the setup password is remembered for the session"
        );
        assert_eq!(saved.input_device_id.as_deref(), Some("mic-b"));
        assert_eq!(saved.output_device_id.as_deref(), Some("speaker"));
        assert_eq!(saved.keybinds.toggle_mute, "F8");
        assert!(
            !std::fs::read_to_string(&path)
                .unwrap()
                .contains("temporary-test-password")
        );
        let mut restarted = App::with_devices(app.devices.clone(), None);
        restarted.load_settings(path);
        assert!(restarted.onboarding.is_none());
        assert_eq!(restarted.input.as_ref().unwrap().id, "mic-b");
        assert!(restarted.password.is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn interrupted_setup_restarts_and_bad_hotkeys_or_failed_writes_do_not_finish_it() {
        let (mut app, root) = setup_fixture("incomplete");
        let _ = app.update(Message::Nickname("Draft".into()));
        let _ = app.update(Message::SetupNext);
        assert!(!root.exists());
        let mut restarted = App::with_devices(app.devices.clone(), None);
        restarted.load_settings(root.join("settings.json"));
        assert_eq!(restarted.onboarding, Some(onboarding::Step::Username));
        assert!(restarted.nickname.is_empty());
        app.address = "voice.example".into();
        app.onboarding = Some(onboarding::Step::Hotkeys);
        app.keybind_draft.toggle_deafen = app.keybind_draft.toggle_mute.clone();
        let _ = app.update(Message::SetupNext);
        assert!(app.onboarding.is_some() && !root.exists());
        app.keybind_draft = shortcuts::Keybinds {
            toggle_mute: "F8".into(),
            toggle_deafen: "F9".into(),
            ..shortcuts::Keybinds::default()
        };
        std::fs::write(&root, "not a directory").unwrap();
        let _ = app.update(Message::SetupNext);
        assert!(app.onboarding.is_some() && app.saved.is_none());
        assert_eq!(
            app.keybinds, app.keybind_draft,
            "The UI must describe the applied shortcuts even if saving fails"
        );
        std::fs::remove_file(root).unwrap();
    }

    #[test]
    fn corrupt_existing_settings_are_preserved_and_missing_saved_devices_are_not_replaced() {
        let (mut app, root) = setup_fixture("preserve");
        let path = root.join("settings.json");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&path, "{ broken").unwrap();
        app.onboarding = None;
        app.load_settings(path.clone());
        assert!(app.onboarding.is_none());
        assert_eq!(app.settings_page, Some(SettingsPage::File));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ broken");
        app.apply_settings(settings::Settings {
            input_device_id: Some("unplugged-microphone".into()),
            ..Default::default()
        });
        assert!(app.input.is_none());
        assert!(app.output.is_some());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn hiding_the_footer_saves_only_that_choice_and_survives_a_restart() {
        let (mut app, root) = setup_fixture("footer");
        let path = root.join("settings.json");
        let saved = settings::Settings {
            nickname: "Saved name".into(),
            ..Default::default()
        };
        settings::save(&path, &saved).unwrap();
        app.load_settings(path.clone());
        let _ = app.update(Message::ToggleFooter);
        assert!(app.footer_hidden);
        let stored = settings::load(&path).unwrap().unwrap();
        assert!(stored.footer_hidden);
        assert_eq!(stored.nickname, "Saved name", "the rest of the file is untouched");
        let (mut restarted, _) = setup_fixture("footer-restart");
        restarted.load_settings(path.clone());
        assert!(restarted.footer_hidden);
        let _ = restarted.update(Message::ToggleFooter);
        assert!(!settings::load(&path).unwrap().unwrap().footer_hidden);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn automatic_update_downloads_save_at_once_and_survive_a_restart() {
        let (mut app, root) = setup_fixture("auto-update");
        let path = root.join("settings.json");
        settings::save(&path, &settings::Settings::default()).unwrap();
        app.load_settings(path.clone());
        assert!(app.updater.auto_download, "on by default");
        let _ = app.update(Message::Updater(updater::Event::AutoDownload(false)));
        assert!(!app.updater.auto_download);
        let stored = settings::load(&path).unwrap().unwrap();
        assert!(!stored.auto_download_updates);
        assert_eq!(stored.nickname, "Flexspeak", "the rest of the file is untouched");
        let (mut restarted, _) = setup_fixture("auto-update-restart");
        restarted.load_settings(path.clone());
        assert!(!restarted.updater.auto_download);
        assert!(!restarted.edited_settings().auto_download_updates);
        let _ = restarted.update(Message::Updater(updater::Event::AutoDownload(true)));
        assert!(
            settings::load(&path)
                .unwrap()
                .unwrap()
                .auto_download_updates
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn appearance_choices_save_alone_and_survive_a_restart() {
        let (mut app, root) = setup_fixture("appearance");
        let path = root.join("settings.json");
        settings::save(&path, &settings::Settings::default()).unwrap();
        app.load_settings(path.clone());
        let chosen = settings::Appearance {
            scheme: settings::ColorScheme::Light,
            light_theme: settings::ThemeName::Ocean,
            dark_theme: settings::ThemeName::Ember,
            disable_animations: true,
            dark_theme_id: Some("catppuccin-mocha".into()),
            ..settings::Appearance::default()
        };
        let _ = app.update(Message::Appearance(chosen.clone()));
        let stored = settings::load(&path).unwrap().unwrap();
        assert_eq!(stored.appearance, chosen);
        assert_eq!(stored.nickname, settings::Settings::default().nickname);
        let (mut restarted, _) = setup_fixture("appearance-restart");
        restarted.load_settings(path);
        assert_eq!(restarted.appearance, chosen);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_finished_update_install_records_every_server_then_closes() {
        let (mut app, root) = setup_fixture("resume-write");
        let path = root.join("settings.json");
        settings::save(&path, &settings::Settings::default()).unwrap();
        app.load_settings(path.clone());
        let (first, _a) = connection(&mut app);
        let session = app.session_mut(first).unwrap();
        session.connected = true;
        session.muted = false;
        session.password = "server secret".into();
        session.address = "ts.zirkumflex.io".into();
        session.state.current = Some(client::Channel {
            id: 9,
            ..client::Channel::default()
        });
        session.channel_secret = Some((9, "channel secret".into()));
        let (second, _b) = connection(&mut app);
        let session = app.session_mut(second).unwrap();
        session.connected = true;
        session.deafened = true;
        session.address = "two.example".into();
        app.tab = Tab::Statistics;
        app.updater = updater::Updater::new(
            updater::Build::parse_tag("v0.1.0-build.6"),
            Ok(updater::Package::MacApp),
            true,
        );
        app.updater.phase = updater::Phase::Installing;
        let _ = app.update(Message::Updater(updater::Event::Installed(Ok(()))));
        assert!(
            app.closing && app.sessions.iter().all(|session| session.stopping),
            "every session ends before exit"
        );
        let saved = resume::take(&resume::path(&path), SystemTime::now()).unwrap();
        assert_eq!(saved.page, resume::Page::Statistics);
        assert_eq!(saved.active.as_deref(), Some("two.example"));
        assert_eq!(saved.servers.len(), 2);
        assert_eq!(saved.servers[0].address, "ts.zirkumflex.io");
        assert_eq!(saved.servers[0].password, "server secret");
        assert_eq!(saved.servers[0].channel, Some(9));
        assert_eq!(saved.servers[0].channel_password, "channel secret");
        assert_eq!(
            (saved.servers[0].muted, saved.servers[0].deafened),
            (false, false)
        );
        assert!(saved.servers[1].deafened);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn after_an_update_each_server_rejoins_its_channel_with_the_same_microphone() {
        let mut app = App::with_devices(audio::Devices::default(), None);
        app.saved = Some(settings::Settings::default());
        app.servers = vec![settings::Server::new("ts.zirkumflex.io")];
        app.resume = Some(resume::Resume::new(
            resume::Page::Settings,
            vec![resume::Server {
                address: "ts.zirkumflex.io".into(),
                password: "server secret".into(),
                channel: Some(4),
                channel_password: "channel secret".into(),
                muted: false,
                deafened: true,
            }],
            None,
        ));
        // Without devices the reconnect can't start, so the resume ends there, on its page.
        let _ = app.update(Message::Resume);
        assert_eq!(app.settings_page, Some(SettingsPage::Updates));
        assert_eq!(app.password, "server secret");
        assert!(app.resume.is_none());

        // With a session under way: controls on connect, then one join once channels arrive.
        let (id, mut commands) = connection(&mut app);
        app.server_mut().resume = Some(resume::Server {
            address: "ts.zirkumflex.io".into(),
            channel: Some(4),
            channel_password: "channel secret".into(),
            muted: false,
            deafened: true,
            ..resume::Server::default()
        });
        let _ = app.update(Message::Backend(
            id,
            client::Event::Connected(String::new()),
        ));
        assert!(!app.server().muted && app.server().deafened);
        assert!(matches!(
            commands.try_recv(),
            Ok(client::Command::Mute(false))
        ));
        assert!(matches!(
            commands.try_recv(),
            Ok(client::Command::Deafen(true))
        ));
        let lobby = client::Channel {
            id: 1,
            join_access: client::JoinAccess::Allowed,
            ..client::Channel::default()
        };
        let room = client::Channel {
            id: 4,
            has_password: true,
            join_access: client::JoinAccess::Allowed,
            ..client::Channel::default()
        };
        let _ = app.update(Message::Backend(
            id,
            client::Event::State(client::Snapshot {
                channels: vec![lobby.clone(), room],
                current: Some(lobby),
                ..client::Snapshot::default()
            }),
        ));
        match commands.try_recv() {
            Ok(client::Command::Join(4, password)) => assert_eq!(password, "channel secret"),
            other => panic!("expected a join, got {other:?}"),
        }
        assert!(app.server().resume.is_none(), "only once");
        assert_eq!(
            app.server().channel_secret,
            Some((4, "channel secret".into()))
        );
    }

    #[test]
    fn teamspeak_6_settings_fill_in_setup_and_stay_changeable() {
        let mic = |id: &str, name: &str| audio::DeviceChoice {
            id: id.into(),
            name: name.into(),
        };
        let devices = audio::Devices {
            inputs: vec![mic("a", "Laptop Mic"), mic("b", "Desk Mic")],
            outputs: vec![mic("c", "Speakers")],
            input: Some(mic("a", "Laptop Mic")),
            output: Some(mic("c", "Speakers")),
        };
        let mut app = App::with_devices(devices, None);
        app.begin_setup();
        app.input = Some(mic("a", "Laptop Mic"));
        let imported = ts6_import::Imported {
            nickname: Some("Alex".into()),
            servers: vec!["one.example".into(), "two.example".into()],
            input: Some("Desk Mic".into()),
            output: Some("Unplugged Headset".into()),
            push_to_talk: true,
            ptt_release_ms: 200,
        };
        app.apply_ts6(&imported);
        app.ts6 = Some(imported);
        assert_eq!(app.address, "one.example");
        assert_eq!(app.input.as_ref().unwrap().name, "Desk Mic");
        assert_eq!(
            app.output.as_ref().unwrap().name,
            "Speakers",
            "not on this computer"
        );
        assert_eq!(app.transmission.mode, transmission::Mode::PushToTalk);
        assert_eq!(app.transmission.ptt_release_ms, 200);

        // Your own choice comes first, and the rest of TeamSpeak's servers are kept.
        let _ = app.update(Message::Address("mine.example".into()));
        let saved = |app: &App| {
            app.edited_settings()
                .servers
                .iter()
                .map(|server| server.address.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(saved(&app), ["mine.example", "one.example", "two.example"]);
        let _ = app.update(Message::DropTs6Server("one.example".into()));
        assert_eq!(saved(&app), ["mine.example", "two.example"]);
    }

    #[test]
    fn an_unreadable_teamspeak_6_file_leaves_setup_usable() {
        let mut app = App::with_devices(audio::Devices::default(), None);
        app.begin_setup();
        app.ts6_path = Some(std::env::temp_dir().join("flexspeak-no-such-folder/settings.db"));
        let _ = app.update(Message::ImportTs6);
        assert!(app.ts6.is_none());
        assert!(app.onboarding_error.is_some());
        assert_eq!(app.onboarding, Some(onboarding::Step::Username));
    }

    #[test]
    fn each_person_has_their_own_volume_and_mute_for_you_alone() {
        let (mut app, _commands) = session_fixture();
        let other = app
            .server()
            .state
            .current_members()
            .iter()
            .find(|member| !member.is_self)
            .unwrap()
            .clone();
        let gain = |app: &App| app.user_gains.lock().unwrap().get(&other.uid).copied();
        assert_eq!(gain(&app), None, "full volume by default");

        let _ = app.update(Message::OpenUserVolume(other.id));
        let _ = app.update(Message::UserVolume(40));
        assert_eq!(gain(&app), Some(0.4));
        let _ = app.update(Message::CloseUserVolume);
        assert!(app.ui.user_volume.is_none());

        let _ = app.update(Message::ToggleUserMute(other.id));
        assert_eq!(gain(&app), Some(0.0));
        assert_eq!(
            app.user_audio(&other.uid).volume_percent,
            40,
            "kept while muted"
        );
        let _ = app.update(Message::ToggleUserMute(other.id));
        assert_eq!(gain(&app), Some(0.4));

        let _ = app.update(Message::OpenUserVolume(other.id));
        let _ = app.update(Message::UserVolume(100));
        assert_eq!(gain(&app), None, "back to normal is forgotten");
        assert!(app.edited_settings().user_audio.is_empty());
    }

    #[test]
    fn share_settings_are_remembered_when_the_dialog_closes() {
        use streaming::publishing::{Access, Preset};
        let (mut app, _commands) = session_fixture();
        let _ = app.update(Message::ShareQuality(Preset::Performance.quality()));
        let _ = app.update(Message::ShareAccess(Access::Public));
        assert_ne!(
            app.share_defaults.quality,
            Preset::Performance.quality(),
            "not while dragging"
        );
        let _ = app.update(Message::CloseShare);
        assert_eq!(app.share_defaults.quality, Preset::Performance.quality());
        assert_eq!(app.share_defaults.access, Access::Public);
        assert!(app.share_defaults.audio);
    }

    #[test]
    fn muting_and_deafening_play_their_sounds_unless_turned_off() {
        use sounds::Sound::{Deafened, MicOff, MicOn, Undeafened};
        let (mut app, _commands) = session_fixture();
        let _ = app.update(Message::Mute);
        let _ = app.update(Message::Mute);
        let _ = app.update(Message::Deafen);
        let _ = app.update(Message::Deafen);
        let mut played = std::mem::take(&mut app.played_sounds);
        played.sort();
        let mut expected = vec![MicOff, MicOn, Deafened, Undeafened];
        expected.sort();
        assert_eq!(played, expected);

        let _ = app.update(Message::SoundOn(MicOff, false));
        app.server_mut().muted = false;
        let _ = app.update(Message::Mute);
        assert!(app.played_sounds.is_empty(), "this sound is off");
        let _ = app.update(Message::Mute);
        assert_eq!(
            app.played_sounds,
            [MicOn],
            "the others in its group still play"
        );
        let _ = app.update(Message::SoundGroup(sounds::Group::Controls, false));
        let _ = app.update(Message::Deafen);
        assert_eq!(app.played_sounds, [MicOn], "its whole group is off");
        let _ = app.update(Message::SoundGroup(sounds::Group::Controls, true));
        let _ = app.update(Message::Deafen);
        assert_eq!(app.played_sounds.len(), 2, "and back on");
        app.played_sounds.clear();
        let _ = app.update(Message::SoundCues(false));
        let _ = app.update(Message::Deafen);
        assert!(app.played_sounds.is_empty(), "all sounds are off");
    }

    #[test]
    fn joining_and_being_moved_sound_different_and_connecting_is_silent() {
        use sounds::Sound::{JoinedChannel, MovedChannel};
        let (mut app, _commands) = session_fixture();
        let id = app.active.unwrap();
        let channels = app.server().state.channels.clone();
        let home = app.server().state.current.clone().unwrap();
        let other = channels.iter().find(|c| c.id != home.id).unwrap().clone();
        let arrive = |app: &mut App, channel: Option<client::Channel>| {
            let mut state = app.server().state.clone();
            state.current = channel;
            let _ = app.update(Message::Backend(id, client::Event::State(state)));
        };

        arrive(&mut app, Some(other.clone()));
        assert_eq!(app.played_sounds, [MovedChannel], "nobody asked for this");
        app.played_sounds.clear();

        app.server_mut().expected_channel = Some(home.id);
        arrive(&mut app, Some(home.clone()));
        assert_eq!(app.played_sounds, [JoinedChannel]);
        app.played_sounds.clear();

        arrive(&mut app, Some(home.clone()));
        assert!(app.played_sounds.is_empty(), "no change");
        arrive(&mut app, None);
        arrive(&mut app, Some(other.clone()));
        assert!(app.played_sounds.is_empty(), "connecting isn't a move");

        app.server_mut().quiet_channel_change = true;
        arrive(&mut app, Some(home));
        assert!(app.played_sounds.is_empty(), "returning after an update");
    }

    #[test]
    fn being_kicked_from_your_channel_is_not_heard_as_a_move_as_well() {
        use sounds::Sound::{KickedFromChannel, MovedChannel};
        let (mut app, _commands) = session_fixture();
        let id = app.active.unwrap();
        let home = app.server().state.current.clone().unwrap();
        let channels = app.server().state.channels.clone();
        let other = channels.iter().find(|c| c.id != home.id).unwrap().clone();
        let arrive = |app: &mut App, channel: client::Channel| {
            let mut state = app.server().state.clone();
            state.current = Some(channel);
            let _ = app.update(Message::Backend(id, client::Event::State(state)));
        };
        let kicked = client::Activity {
            kind: client::ActivityKind::KickedFromChannel,
            name: "Jan".into(),
            channel: other.name.clone(),
            is_self: true,
            entered_yours: false,
            left_yours: true,
        };
        let _ = app.update(Message::Backend(id, client::Event::Activity(vec![kicked])));
        arrive(&mut app, other);
        assert_eq!(app.played_sounds, [KickedFromChannel]);
        arrive(&mut app, home);
        assert_eq!(
            app.played_sounds,
            [KickedFromChannel, MovedChannel],
            "the next move is heard again"
        );
    }

    #[test]
    fn someone_else_going_live_or_stopping_has_a_sound() {
        use sounds::Sound::{PersonStreamStarted, PersonStreamStopped};
        let (mut app, _commands) = session_fixture();
        let id = app.active.unwrap();
        let stream = |publisher: u16| streaming::StreamInfo {
            id: format!("stream-{publisher}"),
            publisher,
            name: "Screen".into(),
            viewers: 0,
            has_audio: true,
            server_relayed: false,
        };
        let list = |app: &mut App, streams: Vec<streaming::StreamInfo>| {
            let event = streaming::Event::Catalog(streams);
            let _ = app.update(Message::Backend(id, client::Event::Streaming(event)));
            std::mem::take(&mut app.played_sounds)
        };
        assert!(
            list(&mut app, vec![stream(2)]).is_empty(),
            "already live when you arrived"
        );
        assert_eq!(
            list(&mut app, vec![stream(2), stream(3)]),
            [PersonStreamStarted]
        );
        assert!(list(&mut app, vec![stream(2), stream(3)]).is_empty());
        assert_eq!(list(&mut app, vec![stream(3)]), [PersonStreamStopped]);
        assert!(
            list(&mut app, vec![stream(3), stream(1)]).is_empty(),
            "your own stream has its own sounds"
        );
        let _ = app.update(Message::SoundOn(PersonStreamStarted, false));
        assert!(list(&mut app, vec![stream(1), stream(2), stream(3)]).is_empty());
        assert_eq!(
            list(&mut app, vec![stream(1), stream(2)]),
            [PersonStreamStopped]
        );
    }

    #[test]
    fn the_tray_follows_the_server_on_screen() {
        let mut app = App::with_devices(audio::Devices::default(), None);
        assert_eq!(
            app.tray_status().map(|status| status.state),
            tray::SUPPORTED.then_some(tray::State::Offline)
        );
        if !tray::SUPPORTED {
            return;
        }
        let _connection = connection(&mut app);
        let server = app.server_mut();
        server.connected = true;
        server.muted = false;
        server.state.channels = vec![client::Channel {
            id: 1,
            members: vec![client::Member {
                id: 7,
                is_self: true,
                ..Default::default()
            }],
            ..Default::default()
        }];
        assert_eq!(app.tray_status().unwrap().state, tray::State::Open);
        app.server_mut().talking = BTreeSet::from([7]);
        assert_eq!(app.tray_status().unwrap().state, tray::State::Talking);
        app.server_mut().talking = BTreeSet::from([8]);
        assert_eq!(
            app.tray_status().unwrap().state,
            tray::State::Open,
            "someone else talking"
        );
        app.server_mut().muted = true;
        assert_eq!(app.tray_status().unwrap().state, tray::State::Muted);
        app.server_mut().deafened = true;
        let status = app.tray_status().unwrap();
        assert_eq!(status.state, tray::State::Deafened);
        assert!(status.muted && status.deafened);
        app.closing = true;
        assert!(
            app.tray_status().is_none(),
            "the icon goes as the window does"
        );
    }

    #[test]
    fn viewers_arriving_and_leaving_your_stream_have_sounds() {
        use sounds::Sound::{ViewerJoined, ViewerLeft};
        let (mut app, _commands) = session_fixture();
        let id = app.active.unwrap();
        let publish = |app: &mut App, live: bool, viewers: &[u16]| {
            let status = streaming::publishing::Status {
                live,
                viewers: viewers
                    .iter()
                    .map(|v| (*v, format!("V{v}"), true))
                    .collect(),
                ..Default::default()
            };
            let event = streaming::Event::Publishing(status);
            let _ = app.update(Message::Backend(id, client::Event::Streaming(event)));
        };
        publish(&mut app, true, &[]);
        assert_eq!(app.played_sounds, [sounds::Sound::StreamStarted]);
        app.played_sounds.clear();
        publish(&mut app, true, &[7]);
        assert_eq!(app.played_sounds, [ViewerJoined]);
        app.played_sounds.clear();
        publish(&mut app, true, &[]);
        assert_eq!(app.played_sounds, [ViewerLeft]);
        app.played_sounds.clear();
        publish(&mut app, true, &[7]);
        app.played_sounds.clear();
        publish(&mut app, false, &[]);
        assert_eq!(
            app.played_sounds,
            [sounds::Sound::StreamStopped],
            "not a viewer leaving as well"
        );
    }

    #[test]
    fn a_request_to_watch_sounds_and_in_the_background_flashes_and_toasts() {
        let (mut app, _commands) = session_fixture();
        let id = app.active.unwrap();
        let publish = |app: &mut App, requests: &[u16]| {
            let status = streaming::publishing::Status {
                live: true,
                requests: requests.iter().map(|r| (*r, format!("R{r}"))).collect(),
                ..Default::default()
            };
            let event = streaming::Event::Publishing(status);
            let _ = app.update(Message::Backend(id, client::Event::Streaming(event)));
        };
        publish(&mut app, &[]);
        app.played_sounds.clear();
        publish(&mut app, &[4]);
        assert_eq!(app.played_sounds, [sounds::Sound::WatchRequested]);
        assert!(app.shown_toasts.is_empty(), "The window is in front");
        app.played_sounds.clear();
        publish(&mut app, &[4]);
        assert!(app.played_sounds.is_empty(), "Only a new request sounds");
        let _ = app.update(Message::WindowFocus(false));
        publish(&mut app, &[4, 5]);
        assert_eq!(app.shown_toasts, ["R5 asked to watch your stream"]);
    }

    #[test]
    fn pokes_toast_only_in_the_background_and_one_per_ten_seconds() {
        let (mut app, _commands) = session_fixture();
        let id = app.active.unwrap();
        let poke = |app: &mut App| {
            let _ = app.update(Message::Backend(
                id,
                client::Event::Poked {
                    from: "Mara".into(),
                    key: "uid-2".into(),
                    message: "look".into(),
                },
            ));
        };
        poke(&mut app);
        assert!(app.shown_toasts.is_empty());
        let _ = app.update(Message::WindowFocus(false));
        poke(&mut app);
        poke(&mut app);
        assert_eq!(app.shown_toasts, ["Mara poked you"]);
        app.last_toast = app
            .last_toast
            .map(|at| at - TOAST_GAP - std::time::Duration::from_millis(1));
        poke(&mut app);
        assert_eq!(app.shown_toasts.len(), 2);
    }

    #[test]
    fn people_arriving_and_leaving_are_logged_with_their_sounds() {
        use client::{Activity, ActivityKind};
        let (mut app, _commands) = session_fixture();
        let id = app.active.unwrap();
        let person = |kind, entered_yours, left_yours| Activity {
            kind,
            name: "Mara".into(),
            channel: "Lobby".into(),
            is_self: false,
            entered_yours,
            left_yours,
        };
        let list = vec![
            person(ActivityKind::Moved, true, false),
            person(ActivityKind::Disconnected, false, true),
            person(ActivityKind::Connected, false, false),
        ];
        let _ = app.update(Message::Backend(id, client::Event::Activity(list)));
        assert_eq!(
            app.played_sounds,
            [
                sounds::Sound::PersonMovedIn,
                sounds::Sound::PersonDisconnected
            ],
            "nothing for someone elsewhere on the server"
        );
        let log = &app.server().log;
        assert_eq!(log.len(), 3);
        assert_eq!(log.get(0).unwrap().action, "moved to Lobby");
        assert_eq!(log.get(1).unwrap().tone, server_log::Tone::Left);
        assert_eq!(log.get(2).unwrap().action, "connected");
    }

    fn moderation_app() -> (App, mpsc::Receiver<client::Command>) {
        let mut app = App::with_devices(audio::Devices::default(), None);
        let (_, commands) = connection(&mut app);
        let person = |id: u16, is_self: bool, groups: &[u64], channel_group: u64| client::Member {
            id,
            name: format!("Person {id}"),
            is_self,
            server_groups: groups.to_vec(),
            channel_group,
            ..Default::default()
        };
        app.server_mut().connected = true;
        app.server_mut().state.channels = vec![client::Channel {
            id: 1,
            members: vec![
                person(1, true, &[], 5),
                person(2, false, &[7, 8], 5),
                person(3, false, &[8], 6),
            ],
            ..Default::default()
        }];
        (app, commands)
    }

    #[test]
    fn a_kick_waits_for_confirmation_and_skips_yourself() {
        let (mut app, mut commands) = moderation_app();
        let action = client::Moderation::KickServer;
        assert!(matches!(
            app.update(Message::OpenModeration(vec![1, 2, 3], action)),
            Task::Focus(ui::Field::ModerationReason)
        ));
        assert_eq!(app.server().moderation.as_ref().unwrap().ids, vec![2, 3]);
        assert!(commands.try_recv().is_err(), "Opening sends nothing");
        let _ = app.update(Message::ModerationReason("x".repeat(100)));
        assert_eq!(app.server().moderation.as_ref().unwrap().reason.len(), 80);
        let _ = app.update(Message::ModerationReason("Spam".into()));
        let _ = app.update(Message::SubmitModeration);
        match commands.try_recv().unwrap() {
            client::Command::Moderate(ids, sent, reason) => {
                assert_eq!((ids, sent, reason.as_str()), (vec![2, 3], action, "Spam"));
            }
            other => panic!("Unexpected command: {other:?}"),
        }
    }

    #[test]
    fn a_ban_length_only_changes_a_ban() {
        let (mut app, _commands) = moderation_app();
        let _ = app.update(Message::OpenModeration(
            vec![2],
            client::Moderation::KickChannel,
        ));
        let _ = app.update(Message::ModerationBan(None));
        assert_eq!(
            app.server().moderation.as_ref().unwrap().action,
            client::Moderation::KickChannel
        );
        let _ = app.update(Message::OpenModeration(
            vec![2],
            client::Moderation::Ban(Some(60)),
        ));
        let _ = app.update(Message::ModerationBan(None));
        assert_eq!(
            app.server().moderation.as_ref().unwrap().action,
            client::Moderation::Ban(None)
        );
    }

    fn sent_edit(commands: &mut mpsc::Receiver<client::Command>) -> client::GroupEdit {
        match commands.try_recv().unwrap() {
            client::Command::SetGroups(ids, edit) => {
                assert_eq!(ids, vec![2, 3]);
                edit
            }
            other => panic!("Unexpected command: {other:?}"),
        }
    }

    #[test]
    fn each_group_tick_applies_to_everyone_right_away() {
        let (mut app, mut commands) = moderation_app();
        let _ = app.update(Message::OpenGroups(vec![2, 3]));
        assert!(commands.try_recv().is_ok(), "Asks for the missing lists");
        assert_eq!(app.server_group_mark(&[2, 3], 8), GroupMark::On);
        assert_eq!(app.server_group_mark(&[2, 3], 7), GroupMark::Mixed);
        assert_eq!(app.server_group_mark(&[2, 3], 9), GroupMark::Off);
        assert_eq!(app.shared_channel_group(&[2, 3]), None);

        let _ = app.update(Message::ToggleServerGroup(9));
        assert_eq!(sent_edit(&mut commands).add, vec![9]);
        let _ = app.update(Message::OpenGroups(vec![2, 3]));
        assert_eq!(
            app.server().group_edit.as_ref().unwrap().wanted.len(),
            1,
            "Switching submenus keeps your ticks"
        );
        let _ = app.update(Message::ToggleServerGroup(8));
        assert_eq!(sent_edit(&mut commands).remove, vec![8]);
        let _ = app.update(Message::ToggleServerGroup(7));
        assert_eq!(sent_edit(&mut commands).add, vec![7], "Mixed adds to all");
        let _ = app.update(Message::PickChannelGroup(6));
        assert_eq!(sent_edit(&mut commands).channel_group, Some(6));
        assert!(app.server().group_edit.is_some(), "The menu stays usable");
    }

    #[test]
    fn a_group_waits_for_the_server_before_it_can_flip_back() {
        let (mut app, mut commands) = moderation_app();
        let _ = app.update(Message::OpenGroups(vec![2, 3]));
        let _ = commands.try_recv();
        let _ = app.update(Message::ToggleServerGroup(9));
        let _ = sent_edit(&mut commands);
        let _ = app.update(Message::ToggleServerGroup(9));
        assert!(commands.try_recv().is_err(), "Still waiting for the server");

        for member in &mut app.server_mut().state.channels[0].members {
            member.server_groups.push(9);
        }
        let _ = app.update(Message::ToggleServerGroup(9));
        assert_eq!(sent_edit(&mut commands).remove, vec![9]);
    }

    #[test]
    fn a_refused_group_change_stops_showing_as_ticked() {
        let (mut app, mut commands) = moderation_app();
        let _ = app.update(Message::OpenGroups(vec![2, 3]));
        let _ = commands.try_recv();
        let _ = app.update(Message::ToggleServerGroup(9));
        let id = app.active.unwrap();
        let _ = app.update(Message::Backend(
            id,
            client::Event::Warning {
                source: Source::Groups,
                detail: "Denied".into(),
            },
        ));
        assert!(app.server().group_edit.as_ref().unwrap().wanted.is_empty());
    }

    #[test]
    fn someone_leaving_drops_them_from_open_dialogs() {
        let (mut app, _commands) = moderation_app();
        let _ = app.update(Message::OpenGroups(vec![2]));
        let _ = app.update(Message::OpenModeration(
            vec![2],
            client::Moderation::KickChannel,
        ));
        let id = app.active.unwrap();
        let mut state = app.server().state.clone();
        state.channels[0].members.retain(|member| member.id != 2);
        let _ = app.update(Message::Backend(id, client::Event::State(state)));
        assert!(app.server().moderation.is_none());
        assert!(app.server().group_edit.is_none());
    }

    #[test]
    fn one_click_goes_live_publicly_and_leaves_the_dialogs_choices() {
        use streaming::publishing::{Access, ShareSource, SourceKind};
        let (mut app, mut commands) = moderation_app();
        app.server_mut().streaming.sharing.audio = false;
        app.server_mut().streaming.sharing.access = Access::Private;
        let defaults = app.share_defaults;
        let source = ShareSource {
            kind: SourceKind::Screen(Some(0)),
            name: "Main screen".into(),
        };
        let _ = app.update(Message::QuickShare(source.clone()));
        match commands.try_recv().unwrap() {
            client::Command::StartSharing(options) => {
                assert_eq!(options.source, source);
                assert_eq!(options.access, Access::Public);
            }
            other => panic!("Unexpected command: {other:?}"),
        }
        assert_eq!(app.server().streaming.sharing.access, Access::Private);
        assert!(app.server().streaming.sharing.selected.is_none());
        assert_eq!(app.share_defaults, defaults);
    }

    #[test]
    fn each_thing_that_happens_in_your_channel_has_its_own_sound() {
        use client::ActivityKind::*;
        use sounds::Sound;
        let heard = |kind, is_self, entered_yours, left_yours| {
            activity_sound(&client::Activity {
                kind,
                name: "Mara".into(),
                channel: "Lobby".into(),
                is_self,
                entered_yours,
                left_yours,
            })
        };
        for (kind, sound) in [
            (Connected, Sound::PersonConnected),
            (Moved, Sound::PersonMovedIn),
            (Switched, Sound::PersonJoinedChannel),
            (KickedFromChannel, Sound::PersonKickedIntoChannel),
        ] {
            assert_eq!(heard(kind, false, true, false), Some(sound));
        }
        for (kind, sound) in [
            (Disconnected, Sound::PersonDisconnected),
            (LostConnection, Sound::PersonLostConnection),
            (Moved, Sound::PersonMovedOut),
            (Switched, Sound::PersonLeftChannel),
            (KickedFromChannel, Sound::PersonKickedFromChannel),
            (KickedFromServer, Sound::PersonKickedFromServer),
            (Banned, Sound::PersonBanned),
        ] {
            assert_eq!(heard(kind, false, false, true), Some(sound));
        }
        for (kind, sound) in [
            (RecordingStarted, Sound::PersonRecordingStarted),
            (RecordingStopped, Sound::PersonRecordingStopped),
            (Whispered, Sound::WhisperReceived),
        ] {
            assert_eq!(heard(kind, false, true, false), Some(sound));
        }
        for (kind, sound) in [
            (TalkPowerGranted, Sound::TalkPowerGranted),
            (TalkPowerRevoked, Sound::TalkPowerRevoked),
            (ServerGroupAssigned, Sound::ServerGroupAssigned),
            (ServerGroupRevoked, Sound::ServerGroupRevoked),
            (ChannelGroupChanged, Sound::ChannelGroupChanged),
        ] {
            assert_eq!(heard(kind, true, false, false), Some(sound));
        }
        assert_eq!(heard(Banned, false, false, false), None, "somewhere else");
        assert_eq!(
            heard(KickedFromChannel, true, false, true),
            Some(Sound::KickedFromChannel)
        );
        assert_eq!(
            heard(KickedFromServer, true, false, true),
            Some(Sound::Kicked)
        );
        assert_eq!(
            heard(Moved, true, true, false),
            None,
            "has its channel sound"
        );
    }

    #[test]
    fn being_kicked_and_losing_the_connection_are_logged_and_heard() {
        let (mut app, _commands) = session_fixture();
        let id = app.active.unwrap();
        let kicked = client::Activity {
            kind: client::ActivityKind::KickedFromServer,
            name: "Jan".into(),
            channel: "Lobby".into(),
            is_self: true,
            entered_yours: false,
            left_yours: true,
        };
        let _ = app.update(Message::Backend(id, client::Event::Activity(vec![kicked])));
        assert_eq!(app.played_sounds, [sounds::Sound::Kicked]);
        app.played_sounds.clear();
        let lost = || client::Event::Status(client::CONNECTION_LOST.into());
        let _ = app.update(Message::Backend(id, lost()));
        let _ = app.update(Message::Backend(id, lost()));
        assert_eq!(app.played_sounds, [sounds::Sound::ConnectionLost], "once");
        app.played_sounds.clear();
        let _ = app.update(Message::Backend(
            id,
            client::Event::Connected(String::new()),
        ));
        assert_eq!(app.played_sounds, [sounds::Sound::Reconnected]);
        let log = &app.server().log;
        assert_eq!(log.get(0).unwrap().name, "You");
        assert_eq!(log.get(1).unwrap().action, "Connection lost");
        assert_eq!(log.get(2).unwrap().action, "Reconnected");
    }

    #[test]
    fn a_poke_on_any_server_plays_its_sound() {
        let (mut app, _commands) = session_fixture();
        let id = app.active.unwrap();
        let poke = client::Event::Poked {
            key: "uid-9".into(),
            from: "Mara".into(),
            message: String::new(),
        };
        let _ = app.update(Message::Backend(id, poke));
        assert_eq!(app.played_sounds, [sounds::Sound::Poke]);
    }

    #[test]
    fn teamspeak_sounds_are_available_without_loading_settings_or_an_install() {
        let (mut app, _commands) = session_fixture();
        let _ = app.update(Message::SoundPack(sounds::Pack::TeamSpeak));
        app.sound_bank = sounds::Bank::with(sounds::Sound::Poke, vec![0.5]);
        app.played_sounds.clear();
        app.play_sound(sounds::Sound::Poke);
        assert_eq!(app.played_sounds, [sounds::Sound::Poke]);
    }

    #[test]
    fn resuming_waits_to_learn_your_channel_and_skips_the_join_if_you_are_there() {
        let mut app = App::with_devices(audio::Devices::default(), None);
        let (sender, mut commands) = mpsc::channel(8);
        let id = live(&mut app, sender);
        app.server_mut().resume = Some(resume::Server {
            channel: Some(1),
            ..resume::Server::default()
        });
        app.server_mut().connected = true;
        let lobby = client::Channel {
            id: 1,
            join_access: client::JoinAccess::Allowed,
            ..client::Channel::default()
        };
        let _ = app.update(Message::Backend(
            id,
            client::Event::State(client::Snapshot {
                channels: vec![lobby.clone()],
                current: None,
                ..client::Snapshot::default()
            }),
        ));
        assert!(
            app.server().resume.is_some(),
            "still waiting to learn the channel"
        );
        let _ = app.update(Message::Backend(
            id,
            client::Event::State(client::Snapshot {
                channels: vec![lobby.clone()],
                current: Some(lobby),
                ..client::Snapshot::default()
            }),
        ));
        assert!(app.server().resume.is_none());
        assert!(commands.try_recv().is_err(), "already there, so no join");
    }

    /// A live session in Lobby with Mara and Theo, and an empty Games channel.
    fn session_fixture() -> (App, mpsc::Receiver<client::Command>) {
        let mut app = App::with_devices(audio::Devices::default(), None);
        let (sender, commands) = mpsc::channel(16);
        live(&mut app, sender);
        app.server_mut().connected = true;
        let person = |id: u16, name: &str, is_self: bool, can_move: Option<bool>| client::Member {
            id,
            uid: format!("uid-{id}"),
            name: name.into(),
            is_self,
            mic_muted: false,
            sound_muted: false,
            can_poke: Some(true),
            can_move,
            ..Default::default()
        };
        let lobby = client::Channel {
            id: 1,
            name: "Lobby".into(),
            join_access: client::JoinAccess::Allowed,
            members: vec![
                person(1, "Me", true, None),
                person(2, "Mara", false, Some(true)),
                person(3, "Theo", false, Some(false)),
            ],
            ..Default::default()
        };
        let games = client::Channel {
            id: 2,
            name: "Games".into(),
            join_access: client::JoinAccess::Allowed,
            ..Default::default()
        };
        app.server_mut().state.channels = vec![lobby.clone(), games];
        app.server_mut().state.current = Some(lobby);
        (app, commands)
    }

    #[test]
    fn clicks_select_one_member_and_command_clicks_add_to_the_selection() {
        let (mut app, _commands) = session_fixture();
        let id = app.active.unwrap();
        let _ = app.update(Message::SelectMember(2, false));
        assert_eq!(app.server().selected_members, BTreeSet::from([2]));
        let _ = app.update(Message::SelectMember(3, true));
        assert_eq!(app.server().selected_members, BTreeSet::from([2, 3]));
        let _ = app.update(Message::SelectMember(3, false));
        assert_eq!(app.server().selected_members, BTreeSet::from([3]));
        let _ = app.update(Message::SelectMember(3, false));
        assert!(
            app.server().selected_members.is_empty(),
            "A second click unselects"
        );
        let _ = app.update(Message::SelectMember(2, true));
        let mut state = app.server().state.clone();
        state.channels[0].members.retain(|member| member.id != 2);
        let _ = app.update(Message::Backend(id, client::Event::State(state)));
        assert!(
            app.server().selected_members.is_empty(),
            "Leavers drop out of the selection"
        );
    }

    #[test]
    fn moving_sends_others_to_the_server_and_joins_for_yourself() {
        let (mut app, mut commands) = session_fixture();
        assert!(app.can_move(&[2]));
        assert!(!app.can_move(&[2, 3]), "The server denied moving Theo");
        assert!(app.can_move(&[1]), "You can always try to join");
        app.server_mut().selected_members = BTreeSet::from([1, 2]);
        let _ = app.update(Message::MoveMembers(vec![1, 2], 2));
        assert!(matches!(
            commands.try_recv(),
            Ok(client::Command::Move(ids, 2)) if ids == vec![2]
        ));
        assert!(matches!(
            commands.try_recv(),
            Ok(client::Command::Join(2, password)) if password.is_empty()
        ));
        assert!(app.server().selected_members.is_empty());
        // Nobody to move when everyone is already there.
        let _ = app.update(Message::MoveMembers(vec![2], 1));
        assert!(commands.try_recv().is_err());
    }

    #[test]
    fn a_poke_is_written_sent_and_received_in_order() {
        let (mut app, mut commands) = session_fixture();
        let id = app.active.unwrap();
        let _ = app.update(Message::OpenPoke(1));
        assert!(app.server().poke.is_none(), "You can't poke yourself");
        assert!(matches!(
            app.update(Message::OpenPoke(2)),
            Task::Focus(ui::Field::PokeMessage)
        ));
        let _ = app.update(Message::PokeText("x".repeat(150)));
        assert_eq!(app.server().poke.as_ref().unwrap().message.len(), 100);
        let _ = app.update(Message::PokeText("Raid now".into()));
        let _ = app.update(Message::SubmitPoke);
        assert!(app.server().poke.is_none());
        assert!(matches!(
            commands.try_recv(),
            Ok(client::Command::Poke(2, message)) if message == "Raid now"
        ));
        for from in ["Mara", "Theo"] {
            let _ = app.update(Message::Backend(
                id,
                client::Event::Poked {
                    key: "uid-9".into(),
                    from: from.into(),
                    message: String::new(),
                },
            ));
        }
        assert_eq!(app.server().pokes.len(), 2);
        let _ = app.update(Message::DismissPoke);
        assert_eq!(app.server().pokes[0].from, "Theo");
        let _ = app.update(Message::OpenPoke(2));
        let _ = app.update(Message::Backend(id, client::Event::Stopped(None)));
        assert!(
            app.server().poke.is_none(),
            "Disconnecting drops an unsent poke"
        );
    }

    fn watch_event(publisher: u16, generation: u64) -> client::Event {
        client::Event::Streaming(streaming::Event::State {
            generation,
            slot: 0,
            stream: streaming::StreamInfo {
                id: format!("stream-{publisher}"),
                publisher,
                name: "Screen".into(),
                viewers: 1,
                has_audio: true,
                server_relayed: false,
            },
            phase: streaming::Phase::Receiving,
            detail: String::new(),
        })
    }

    #[test]
    fn stream_volume_goes_to_300_percent_and_is_remembered_per_streamer() {
        let (mut app, _commands) = session_fixture();
        let id = app.active.unwrap();
        let _ = app.update(Message::Backend(id, watch_event(2, 900)));
        let _ = app.update(Message::StreamVolume(2, 1000));
        assert_eq!(app.server().streaming.volume(2), streaming::MAX_VOLUME);
        assert!((app.server().streaming.surfaces.get(0).volume() - 3.0).abs() < 1e-6);
        let _ = app.update(Message::StreamVolume(2, 250));
        assert_eq!(app.stream_volumes.get("uid-2"), Some(&250));
        assert!(app.stream_volumes_changed.is_some(), "saved a moment later");

        // Muting is for now: unmuting brings 250% back, and the saved volume stays.
        let _ = app.update(Message::ToggleStreamMute(2));
        assert_eq!(app.server().streaming.volume(2), 0);
        assert_eq!(app.stream_volumes.get("uid-2"), Some(&250));
        let _ = app.update(Message::ToggleStreamMute(2));
        assert_eq!(app.server().streaming.volume(2), 250);

        // A later session starts Mara's stream at her saved volume, even under a new client id.
        let (mut later, _commands) = session_fixture();
        later.stream_volumes = app.stream_volumes.clone();
        let id = later.active.unwrap();
        let _ = later.update(Message::Backend(id, watch_event(2, 901)));
        assert_eq!(later.server().streaming.volume(2), 250);
        assert!((later.server().streaming.surfaces.get(0).volume() - 2.5).abs() < 1e-6);
        // Back at 100% the streamer is forgotten again.
        let _ = later.update(Message::StreamVolume(2, 100));
        assert!(later.stream_volumes.is_empty());
    }

    #[test]
    fn changing_quality_or_audio_while_sharing_reaches_the_running_share() {
        let (mut app, mut commands) = session_fixture();
        let _ = app.update(Message::ShareFrameRate(24));
        assert!(
            commands.try_recv().is_err(),
            "nothing runs before going live"
        );
        app.server_mut().streaming.sharing.status.active = true;
        app.server_mut().streaming.sharing.audio = false;
        let _ = app.update(Message::ShareFrameRate(48));
        match commands.try_recv() {
            Ok(client::Command::ConfigureSharing(live)) => {
                assert_eq!(live.quality.frame_rate, 48);
                assert!(live.audio.is_none());
            }
            other => panic!("Unexpected command: {other:?}"),
        }
        let output = audio::DeviceChoice {
            id: "speakers".into(),
            name: "Speakers".into(),
        };
        let _ = app.update(Message::ShareAudioOutput(output.clone()));
        let _ = commands.try_recv();
        let _ = app.update(Message::ShareAudio(true));
        match commands.try_recv() {
            Ok(client::Command::ConfigureSharing(live)) => assert_eq!(live.audio, Some(output)),
            other => panic!("Unexpected command: {other:?}"),
        }
    }

    #[test]
    fn several_streams_are_watched_focused_and_stopped_one_at_a_time() {
        let (mut app, mut commands) = session_fixture();
        let id = app.active.unwrap();
        let session = id;
        for (slot, id) in ["a", "b"].into_iter().enumerate() {
            let _ = app.update(Message::Backend(
                session,
                client::Event::Streaming(streaming::Event::State {
                    generation: 900 + slot as u64,
                    slot,
                    stream: streaming::StreamInfo {
                        id: id.into(),
                        publisher: 2 + slot as u16,
                        name: id.into(),
                        viewers: 1,
                        has_audio: true,
                        server_relayed: false,
                    },
                    phase: streaming::Phase::Receiving,
                    detail: String::new(),
                }),
            ));
        }
        assert_eq!(app.server().streaming.watching.len(), 2);
        let _ = app.update(Message::FocusStream(Some("missing".into())));
        assert!(app.server().streaming.focused.is_none());
        let _ = app.update(Message::FocusStream(Some("b".into())));
        assert_eq!(app.server().streaming.stage().unwrap().stream.id, "b");
        let _ = app.update(Message::StreamVolume(3, 30));
        assert_eq!(app.server().streaming.volume(3), 30);
        assert!((app.server().streaming.surfaces.get(1).volume() - 0.3).abs() < 1e-6);
        let _ = app.update(Message::StopWatching(Some("b".into())));
        assert!(matches!(
            commands.try_recv(),
            Ok(client::Command::StopWatching(Some(id))) if id == "b"
        ));
        assert_eq!(app.server().streaming.stage().unwrap().stream.id, "a");
        let _ = app.update(Message::ToggleStreamStats);
        assert!(app.server().streaming.show_stats);
    }

    #[test]
    fn setup_runs_again_over_saved_settings_and_cancel_restores_them() {
        let (mut app, root) = saved_fixture("redo");
        let _ = app.update(Message::RedoSetup);
        assert_eq!(app.onboarding, Some(onboarding::Step::Username));
        assert_eq!(app.nickname, "Alex", "Setup starts from what was saved");
        assert_eq!(app.address, "one.example");
        let _ = app.update(Message::Nickname("Blake".into()));
        let _ = app.update(Message::CancelSetup);
        assert!(app.onboarding.is_none());
        assert_eq!(app.nickname, "Alex");
        let _ = app.update(Message::RedoSetup);
        let _ = app.update(Message::Nickname("Blake".into()));
        let _ = app.update(Message::SetupNext);
        let _ = app.update(Message::Address("two.example".into()));
        for _ in 0..5 {
            let _ = app.update(Message::SetupNext);
        }
        assert!(app.onboarding.is_none() && !app.redoing_setup);
        let saved = settings::load(&root.join("settings.json"))
            .unwrap()
            .unwrap();
        assert_eq!(saved.nickname, "Blake");
        assert_eq!(
            saved
                .servers
                .iter()
                .map(|server| server.address.as_str())
                .collect::<Vec<_>>(),
            ["two.example", "one.example"],
            "A new address joins the list instead of replacing it"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_install_offer_can_be_declined_for_good() {
        let (mut app, root) = saved_fixture("install");
        app.install_offer = Some(install::Offer {
            from: "/tmp/Flexspeak.exe".into(),
            to: "/tmp/installed/Flexspeak.exe".into(),
        });
        app.install_prompt = true;
        let _ = app.update(Message::DeclineInstall(false));
        assert!(!app.install_prompt);
        assert!(!app.saved.as_ref().unwrap().skip_install_prompt);
        app.install_prompt = true;
        let _ = app.update(Message::DeclineInstall(true));
        let saved = settings::load(&root.join("settings.json"))
            .unwrap()
            .unwrap();
        assert!(saved.skip_install_prompt);
        let _ = app.update(Message::Installed(Err("Disk full".into())));
        assert!(!app.installing);
        assert_eq!(app.alerts.current().unwrap().source, Source::Install);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_eye_toggles_between_every_channel_and_your_own() {
        let mut app = App::with_devices(audio::Devices::default(), None);
        assert!(
            app.subscribe_all.load(Ordering::Relaxed),
            "TeamSpeak's default"
        );
        let _ = app.update(Message::ToggleSubscribeAll);
        assert!(!app.subscribe_all.load(Ordering::Relaxed));
        let _ = app.update(Message::ToggleSubscribeAll);
        assert!(app.subscribe_all.load(Ordering::Relaxed));
    }

    #[test]
    fn unified_home_drives_streaming_and_statistics_keep_peer_details() {
        let mut app = App::with_devices(audio::Devices::default(), None);
        let _ = app.update(Message::ShowTab(Tab::Home));
        assert!(app.server().streaming.visible.load(Ordering::Relaxed));
        assert!(app.channel_health_visible.load(Ordering::Relaxed));
        let _ = app.update(Message::ShowTab(Tab::Statistics));
        assert!(app.server().streaming.visible.load(Ordering::Relaxed));
        assert!(app.channel_health_visible.load(Ordering::Relaxed));
        // Settings floats over the page, so it leaves what that page shows running.
        let _ = app.update(Message::OpenSettings(SettingsPage::Audio));
        assert!(app.channel_health_visible.load(Ordering::Relaxed));
        let _ = app.update(Message::CloseSettings);
        assert_eq!(app.settings_page, None);
        app.begin_setup();
        let _ = app.update(Message::ShowTab(Tab::Home));
        assert!(!app.server().streaming.visible.load(Ordering::Relaxed));
        assert!(!app.channel_health_visible.load(Ordering::Relaxed));
    }

    #[test]
    fn a_dropped_server_is_named_and_cleared_by_connecting_there_again() {
        let mut app = App::with_devices(audio::Devices::default(), None);
        app.address = "ts.example.org".into();
        let (id, _commands) = connection(&mut app);
        app.session_mut(id).unwrap().connected = true;
        let _ = app.update(Message::Backend(
            id,
            client::Event::Stopped(Some((Source::Connection, "Kicked: Take a break".into()))),
        ));
        let alert = app.alerts.current().unwrap();
        assert_eq!(alert.source, Source::Dropped);
        assert_eq!(alert.heading(), "Disconnected from ts.example.org");
        assert_eq!(alert.address.as_deref(), Some("ts.example.org"));
        // A device problem the last server reported closes with it.
        app.alerts.raise(Source::AudioFallback, "Speakers stand in");
        let (other, _more) = connection(&mut app);
        let _ = app.update(Message::Backend(other, client::Event::Stopped(None)));
        assert!(
            app.alerts
                .visible()
                .iter()
                .all(|alert| alert.source != Source::AudioFallback)
        );
        // Coming back clears why it ended.
        let (again, _commands) = connection(&mut app);
        let _ = app.update(Message::Backend(
            again,
            client::Event::Connected(String::new()),
        ));
        assert!(app.alerts.current().is_none());
    }

    #[test]
    fn a_device_problem_never_ends_the_connection() {
        let mut app = App::with_devices(audio::Devices::default(), None);
        let (id, _commands) = connection(&mut app);
        let _ = app.update(Message::Backend(
            id,
            client::Event::Warning {
                source: Source::AudioFallback,
                detail: "Headphones are gone".into(),
            },
        ));
        assert_eq!(app.alerts.current().unwrap().source, Source::AudioFallback);
        assert!(app.session(id).is_some());
        let _ = app.update(Message::Backend(
            id,
            client::Event::Recovered(Source::AudioFallback),
        ));
        assert!(app.alerts.current().is_none());
    }

    #[test]
    fn lists_read_as_sentences() {
        let words = |items: &[&str]| -> Vec<String> {
            items.iter().map(|item| (*item).to_owned()).collect()
        };
        assert_eq!(spoken_list(&words(&["mute"])), "mute");
        assert_eq!(spoken_list(&words(&["mute", "deafen"])), "mute and deafen");
        assert_eq!(
            spoken_list(&words(&["mute", "deafen", "push-to-talk"])),
            "mute, deafen and push-to-talk"
        );
    }

    #[test]
    fn background_flooding_shows_in_the_footer_without_a_dialog() {
        let mut app = App::with_devices(audio::Devices::default(), None);
        let (id, _commands) = connection(&mut app);
        let _ = app.update(Message::Backend(id, client::Event::Limited));
        assert!(app.server().limited);
        assert!(app.alerts.current().is_none());
        let _ = app.update(Message::Backend(
            id,
            client::Event::Recovered(Source::Flood),
        ));
        assert!(!app.server().limited);
    }

    #[test]
    fn modal_dismissal_retry_and_recovery_follow_the_specific_operation() {
        let mut app = App::with_devices(audio::Devices::default(), None);
        let (id, _commands) = connection(&mut app);
        let warning = || {
            Message::Backend(
                id,
                client::Event::Warning {
                    source: Source::Flood,
                    detail: "Cooldown".into(),
                },
            )
        };
        let _ = app.update(warning());
        let _ = app.update(warning());
        assert_eq!(app.alerts.current().unwrap().reports, 2);
        let _ = app.update(Message::Backend(
            id,
            client::Event::Recovered(Source::Controls),
        ));
        assert_eq!(app.alerts.current().unwrap().source, Source::Flood);
        let _ = app.update(Message::DismissAlert(app.alerts.current().unwrap().id));
        let _ = app.update(warning());
        assert!(app.alerts.current().is_none());
        let _ = app.update(Message::Backend(
            id,
            client::Event::Recovered(Source::Flood),
        ));
        let _ = app.update(warning());
        assert_eq!(app.alerts.current().unwrap().reports, 1);
        let _ = app.update(Message::DismissAlert(app.alerts.current().unwrap().id));
        app.keybind_draft.toggle_mute = "Nonsense+Key".into();
        app.save_keybinds();
        assert_eq!(app.alerts.current().unwrap().source, Source::Shortcuts);
        let _ = app.update(Message::DismissAlert(app.alerts.current().unwrap().id));
        app.save_keybinds();
        assert_eq!(app.alerts.current().unwrap().source, Source::Shortcuts);
    }

    /// An app with a valid saved profile written to a temporary settings file.
    fn saved_app(name: &str) -> (App, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "flexspeak-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let mut app = App::with_devices(audio::Devices::default(), None);
        let saved = settings::Settings {
            servers: vec![settings::Server::new("ts.example")],
            nickname: "Ada".into(),
            ..settings::Settings::default()
        };
        app.settings_path = Some(root.join("settings.json"));
        settings::save(app.settings_path.as_deref().unwrap(), &saved).unwrap();
        app.apply_settings(saved);
        (app, root)
    }

    fn on_disk(app: &App) -> settings::Settings {
        settings::load(app.settings_path.as_deref().unwrap())
            .unwrap()
            .unwrap()
    }

    #[test]
    fn settings_save_as_they_change_without_a_save_button() {
        let (mut app, root) = saved_app("save-on-change");
        let _ = app.update(Message::Transmission(transmission::Settings {
            mode: transmission::Mode::PushToTalk,
            ..app.transmission
        }));
        assert_eq!(on_disk(&app).transmission.mode, transmission::Mode::PushToTalk);
        // Sliders wait for Settings to close, or the app to quit, before writing the file.
        let _ = app.update(Message::PlaybackVolume(40));
        let _ = app.update(Message::SoundVolume(30));
        assert_ne!(on_disk(&app).playback_volume_percent, 40);
        let _ = app.update(Message::Close);
        let saved = on_disk(&app);
        assert_eq!(saved.playback_volume_percent, 40);
        assert_eq!(saved.sounds.volume, 30);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_nickname_saves_on_enter_and_an_empty_one_is_explained_under_its_field() {
        let (mut app, root) = saved_app("nickname");
        app.open_settings(SettingsPage::Server);
        let _ = app.update(Message::Nickname("  ".into()));
        let _ = app.update(Message::CommitNickname);
        assert_eq!(app.nickname_error.as_deref(), Some("Enter a nickname."));
        assert!(app.alerts.current().is_none(), "no corner notice for it");
        assert_eq!(on_disk(&app).nickname, "Ada");
        // Closing Settings brings the saved name back.
        app.close_settings();
        assert_eq!((app.nickname.as_str(), app.nickname_error.as_deref()), ("Ada", None));

        app.open_settings(SettingsPage::Server);
        let _ = app.update(Message::Nickname("Grace".into()));
        let _ = app.update(Message::CommitNickname);
        assert_eq!(on_disk(&app).nickname, "Grace");
        // An edit not yet committed is kept when Settings closes, too.
        let _ = app.update(Message::Nickname("Hopper".into()));
        app.close_settings();
        assert_eq!(on_disk(&app).nickname, "Hopper");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn connecting_keeps_an_edited_nickname() {
        let (mut app, root) = saved_app("nickname-connect");
        let _ = app.update(Message::Nickname("Grace".into()));
        let _ = app.update(Message::Connect(0));
        assert_eq!(app.nickname, "Grace");
        assert_eq!(on_disk(&app).nickname, "Grace");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn footer_health_only_reports_live_channel_metrics_and_marks_paused_sampling() {
        let mut app = App::with_devices(audio::Devices::default(), None);
        app.server_mut().state.current = Some(client::Channel::default());
        app.server_mut().connection_usage = usage::ConnectionUsage {
            packet_loss_in: Some(6.0),
            packet_loss_out: Some(0.0),
            ping_ms: Some(23.0),
            ping_deviation_ms: Some(5.0),
            ..Default::default()
        };
        assert_eq!(app.footer_connection_status().1, "Offline");
        app.server_mut().connected = true;
        // A problem shows once it has held, as readings arriving each second make it do.
        let reading = app.server().connection_usage.health();
        let start = Instant::now();
        app.server_mut().connection_health.update(reading, start);
        assert_eq!(app.footer_connection_status().0, usage::Health::Good);
        app.server_mut()
            .connection_health
            .update(reading, start + usage::HEALTH_HOLD);
        let (health, label, details) = app.footer_connection_status();
        assert_eq!(health, usage::Health::Poor);
        assert_eq!(label, "Poor");
        assert_eq!(details, "Incoming packet loss: 6.0%");
        app.loopback = true;
        assert_eq!(app.footer_connection_status().1, "Local test");
        app.server_mut().stopping = true;
        assert_eq!(app.footer_connection_status().1, "Stopping");
    }

    fn peer_sample(loss: f32, now: Instant) -> usage::ServerUsage {
        let data = serde_json::from_value(serde_json::json!({
            "client_id": 11, "ping": [0, 23_000_000], "ping_deviation": [0, 11_200_000],
            "idle_time": [60, 0], "server_to_client_packetloss_total": loss,
            "client_to_server_packetloss_speech": 0.0, "client_to_server_packetloss_keepalive": 0.0,
            "client_to_server_packetloss_control": 0.0, "client_to_server_packetloss_total": 0.0
        }))
        .unwrap();
        usage::ServerUsage::new(&data, now)
    }

    fn health_test_state() -> client::Snapshot {
        let mut state = client::Snapshot {
            channels: [1, 2]
                .into_iter()
                .map(|id| client::Channel {
                    id,
                    members: vec![client::Member {
                        id: id as u16 + 10,
                        uid: String::new(),
                        name: format!("Client {id}"),
                        is_self: false,
                        mic_muted: true,
                        sound_muted: false,
                        can_poke: None,
                        can_move: None,
                        ..Default::default()
                    }],
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        state.current = Some(state.channels[0].clone());
        state
    }

    #[test]
    fn member_indicators_share_own_health_and_do_not_invent_remote_health() {
        let mut app = App::with_devices(audio::Devices::default(), None);
        app.server_mut().connected = true;
        app.server_mut().state = health_test_state();
        let now = Instant::now();
        let mut member = app.server().state.current.as_ref().unwrap().members[0].clone();
        app.server_mut()
            .client_usage
            .insert(member.id, Ok(peer_sample(0.02, now)));
        let (health, detail) = app.member_health(&member, true);
        assert_eq!(health, usage::Health::Fair);
        assert_eq!(detail, "Incoming packet loss: 2.0%");
        let (health, detail) = app.member_health(&member, false);
        assert_eq!(health, usage::Health::Unknown);
        assert!(detail.contains("current channel"));
        app.server_mut()
            .client_usage
            .insert(member.id, Err("Permission denied"));
        assert_eq!(
            app.member_health(&member, true),
            (usage::Health::Unknown, "Permission denied".into())
        );
        app.server_mut().client_usage.insert(
            member.id,
            Ok(peer_sample(0.0, now - std::time::Duration::from_secs(16))),
        );
        assert_eq!(app.member_health(&member, true).0, usage::Health::Unknown);
        member.is_self = true;
        app.server_mut().connection_usage = peer_sample(0.06, now).sample(now);
        let (health, _, detail) = app.footer_connection_status();
        assert_eq!(app.member_health(&member, true), (health, detail));
    }

    #[test]
    fn collapsed_health_updates_without_expansion_and_expires_without_false_good() {
        let mut app = App::with_devices(audio::Devices::default(), None);
        let (id, _commands) = connection(&mut app);
        app.server_mut().connected = true;
        app.server_mut().state = health_test_state();
        let now = Instant::now();
        assert_eq!(app.client_quality(11, now).health(), usage::Health::Unknown);
        for (loss, health) in [
            (0.0, usage::Health::Good),
            (0.02, usage::Health::Fair),
            (0.06, usage::Health::Poor),
        ] {
            let _ = app.update(Message::Backend(
                id,
                client::Event::ClientUsage {
                    id: 11,
                    result: Ok(peer_sample(loss, now)),
                },
            ));
            assert_eq!(app.server().expanded_client, None);
            assert_eq!(app.client_quality(11, now).health(), health);
        }
        let _ = app.update(Message::ToggleClient(11));
        assert_eq!(app.server().expanded_client, Some(11));
        let _ = app.update(Message::ToggleClient(11));
        assert_eq!(app.server().expanded_client, None);
        assert_eq!(app.client_quality(11, now).health(), usage::Health::Poor);
        assert_eq!(
            app.client_quality(11, now + std::time::Duration::from_secs(16))
                .health(),
            usage::Health::Unknown
        );
        let _ = app.update(Message::Backend(
            id,
            client::Event::ClientUsage {
                id: 11,
                result: Err("Denied"),
            },
        ));
        assert_eq!(app.client_quality(11, now).health(), usage::Health::Unknown);
        let _ = app.update(Message::ShowTab(Tab::Statistics));
        assert!(app.channel_health_visible.load(Ordering::Relaxed));
        let _ = app.update(Message::ShowTab(Tab::Home));
        assert!(app.channel_health_visible.load(Ordering::Relaxed));
    }

    #[test]
    fn client_health_cache_is_scoped_to_current_members_and_clears_on_channel_moves() {
        let mut app = App::with_devices(audio::Devices::default(), None);
        let (session, _commands) = connection(&mut app);
        app.server_mut().connected = true;
        let mut state = health_test_state();
        app.server_mut().state = state.clone();
        let now = Instant::now();
        for id in [12, 99] {
            let _ = app.update(Message::ToggleClient(id));
            let _ = app.update(Message::Backend(
                session,
                client::Event::ClientUsage {
                    id,
                    result: Ok(peer_sample(0.0, now)),
                },
            ));
        }
        assert!(app.server().client_usage.is_empty());
        assert_eq!(app.server().expanded_client, None);
        let _ = app.update(Message::Backend(
            session,
            client::Event::ClientUsage {
                id: 11,
                result: Ok(peer_sample(0.0, now)),
            },
        ));
        let _ = app.update(Message::ToggleClient(11));
        let peer = state.channels[0].members.pop().unwrap();
        state.channels[1].members.push(peer);
        state.current = Some(state.channels[0].clone());
        let _ = app.update(Message::Backend(
            session,
            client::Event::State(state.clone()),
        ));
        assert!(app.server().client_usage.is_empty());
        assert_eq!(app.server().expanded_client, None);
        state.current = Some(state.channels[1].clone());
        let _ = app.update(Message::Backend(
            session,
            client::Event::State(state.clone()),
        ));
        let _ = app.update(Message::ToggleClient(12));
        let _ = app.update(Message::Backend(
            session,
            client::Event::ClientUsage {
                id: 12,
                result: Ok(peer_sample(0.0, now)),
            },
        ));
        assert_eq!(app.server().expanded_client, Some(12));
        state.current = Some(state.channels[0].clone());
        let _ = app.update(Message::Backend(session, client::Event::State(state)));
        assert!(app.server().client_usage.is_empty());
        assert_eq!(app.server().expanded_client, None);
        let _ = app.update(Message::Backend(
            session,
            client::Event::ClientUsage {
                id: 12,
                result: Err("Late response"),
            },
        ));
        assert!(app.server().client_usage.is_empty());
    }

    #[test]
    fn joining_blocks_known_denials_and_lets_the_server_check_unknown_access() {
        let mut app = App::with_devices(audio::Devices::default(), None);
        let (sender, mut commands) = mpsc::channel(16);
        live(&mut app, sender);
        app.server_mut().connected = true;
        app.server_mut().channel_password = "channel secret".into();
        app.server_mut().state.channels = [
            client::JoinAccess::Allowed,
            client::JoinAccess::Denied,
            client::JoinAccess::Unknown,
            client::JoinAccess::UnsupportedCodec,
        ]
        .into_iter()
        .enumerate()
        .map(|(index, join_access)| client::Channel {
            id: index as u64 + 1,
            join_access,
            ..client::Channel::default()
        })
        .collect();
        for id in [2, 4, 999] {
            let _ = app.update(Message::Join(id));
            assert!(commands.try_recv().is_err());
        }
        for id in [1, 3] {
            let _ = app.update(Message::Join(id));
            assert!(
                matches!(commands.try_recv(), Ok(client::Command::Join(sent_id, password)) if sent_id == id && password.is_empty())
            );
        }
        app.server_mut().state.current = Some(app.server().state.channels[0].clone());
        let _ = app.update(Message::Join(1));
        assert!(commands.try_recv().is_err());
        app.server_mut().state.current = None;
        app.server_mut().stopping = true;
        let _ = app.update(Message::Join(1));
        assert!(commands.try_recv().is_err());
        app.server_mut().stopping = false;
        app.server_mut().connected = false;
        let _ = app.update(Message::Join(1));
        assert!(commands.try_recv().is_err());
    }

    #[test]
    fn channel_password_is_prompted_on_join_and_cleared_after_submit_or_cancel() {
        let mut app = App::with_devices(audio::Devices::default(), None);
        let (sender, mut commands) = mpsc::channel(16);
        let id = live(&mut app, sender);
        app.server_mut().connected = true;
        app.server_mut().state.channels = vec![client::Channel {
            id: 2,
            has_password: true,
            join_access: client::JoinAccess::Unknown,
            ..client::Channel::default()
        }];
        let _ = app.update(Message::Join(2));
        assert_eq!(app.server().password_channel, Some(2));
        assert!(commands.try_recv().is_err());
        let _ = app.update(Message::ChannelPassword("secret".into()));
        let _ = app.update(Message::SubmitChannelPassword);
        assert!(
            matches!(commands.try_recv(), Ok(client::Command::Join(2, password)) if password == "secret")
        );
        assert!(
            app.server().password_channel.is_none() && app.server().channel_password.is_empty()
        );
        let _ = app.update(Message::Backend(
            id,
            client::Event::ChannelPasswordRequired(2),
        ));
        assert_eq!(app.server().password_channel, Some(2));
        assert!(app.server().password_error.is_some());
        let _ = app.update(Message::ChannelPassword("discard me".into()));
        let _ = app.update(Message::CancelChannelPassword);
        assert!(
            app.server().password_channel.is_none() && app.server().channel_password.is_empty()
        );
        assert!(commands.try_recv().is_err());
    }

    #[test]
    fn mute_and_deafen_toggles_preserve_state_and_ignore_inactive_sessions() {
        let mut app = App::with_devices(audio::Devices::default(), None);
        app.sound_cues = false;
        let (sender, mut commands) = mpsc::channel(16);
        live(&mut app, sender);
        let _ = app.update(Message::Shortcut(shortcuts::Action::Mute));
        assert!(commands.try_recv().is_err());
        app.server_mut().connected = true;
        let _ = app.update(Message::Shortcut(shortcuts::Action::Deafen));
        assert!(app.server().deafened && app.server().muted);
        assert!(matches!(
            commands.try_recv(),
            Ok(client::Command::Deafen(true))
        ));
        let _ = app.update(Message::Mute);
        assert!(!app.server().muted && app.server().deafened);
        assert_eq!(app.microphone_status(), "Muted (deafened)");
        assert!(matches!(
            commands.try_recv(),
            Ok(client::Command::Mute(false))
        ));
        let _ = app.update(Message::Mute);
        let _ = app.update(Message::Deafen);
        assert!(app.server().muted && !app.server().deafened);
        assert!(matches!(
            commands.try_recv(),
            Ok(client::Command::Mute(true))
        ));
        assert!(matches!(
            commands.try_recv(),
            Ok(client::Command::Deafen(false))
        ));
        app.server_mut().stopping = true;
        let _ = app.update(Message::Mute);
        let _ = app.update(Message::Deafen);
        assert!(app.server().muted && !app.server().deafened);
        assert!(commands.try_recv().is_err());
    }

    #[test]
    fn failed_control_delivery_does_not_report_a_false_mute_state() {
        let mut app = App::with_devices(audio::Devices::default(), None);
        let (sender, _commands) = mpsc::channel(1);
        sender.try_send(client::Command::Mute(true)).unwrap();
        live(&mut app, sender);
        app.server_mut().connected = true;
        let _ = app.update(Message::Mute);
        let _ = app.update(Message::Deafen);
        assert!(app.server().muted && !app.server().deafened);
        assert_eq!(app.alerts.current().unwrap().source, Source::Worker);
    }

    #[test]
    fn volume_and_portable_settings_do_not_change_microphone_state() {
        let mut app = App::with_devices(audio::Devices::default(), None);
        app.server_mut().connected = true;
        app.server_mut().muted = false;
        let worker_volume = app.playback_volume.clone();
        let _ = app.update(Message::PlaybackVolume(0));
        assert_eq!(worker_volume.percent(), 0);
        assert!(!app.server().muted && !app.server().deafened);
        assert_eq!(app.edited_settings().playback_volume_percent, 0);
        app.server_mut().connected = false;
        app.password = "old-server-password".into();
        app.server_mut().channel_password = "old-channel-password".into();
        app.apply_settings(settings::Settings {
            servers: vec![settings::Server::new("voice.example:9987")],
            nickname: "Portable name".into(),
            playback_volume_percent: 42,
            ..settings::Settings::default()
        });
        assert_eq!(app.address, "voice.example:9987");
        assert_eq!(app.nickname, "Portable name");
        assert_eq!(worker_volume.percent(), 42);
        assert!(app.password.is_empty());
    }

    #[test]
    fn processing_settings_are_saved_and_reach_the_devices_of_an_active_session() {
        let mut app = App::with_devices(audio::Devices::default(), None);
        let chosen = audio_processing::Settings {
            noise_suppression: true,
            automatic_gain: true,
            echo_cancellation: true,
            ..crate::audio_processing::Settings::default()
        };
        let _ = app.update(Message::AudioProcessing(chosen));
        assert_eq!(
            app.audio_processing,
            if audio_processing::AVAILABLE {
                chosen
            } else {
                audio_processing::Settings::default()
            }
        );
        app.apply_settings(settings::Settings {
            audio_processing: chosen,
            ..settings::Settings::default()
        });
        assert_eq!(app.edited_settings().audio_processing, chosen);
        let (sender, _commands) = mpsc::channel(1);
        live(&mut app, sender);
        app.server_mut().muted = true;
        app.server_mut().deafened = true;
        let device = audio::DeviceChoice {
            id: "speakers".into(),
            name: "Speakers".into(),
        };
        app.input = Some(device.clone());
        app.output = Some(device);
        let _ = app.update(Message::AudioProcessing(
            audio_processing::Settings::default(),
        ));
        assert_eq!(app.audio_processing, audio_processing::Settings::default());
        assert_eq!(
            app.hub.pending_retune().map(|retune| retune.processing),
            Some(audio_processing::Settings::default()),
            "the open devices take the change"
        );
        assert!(app.server().muted && app.server().deafened);
    }

    #[test]
    fn transmission_ui_preserves_safety_gates_and_requires_a_ptt_binding() {
        let mut app = App::with_devices(audio::Devices::default(), None);
        let settings = transmission::Settings {
            mode: transmission::Mode::PushToTalk,
            ptt_release_ms: 75,
            voice_threshold_db: -35,
        };
        let _ = app.update(Message::Transmission(settings));
        assert_eq!(app.edited_settings().transmission, settings);
        let _ = app.start(false);
        assert!(app.server().session.is_none());
        assert_eq!(app.alerts.current().unwrap().source, Source::Shortcuts);
        app.ptt.store(false, Ordering::Release);
        let _ = app.update(Message::Shortcut(shortcuts::Action::PushToTalk(true)));
        assert!(
            !app.ptt.load(Ordering::Acquire),
            "Delayed UI messages must not reopen a released key"
        );
        let (sender, _commands) = mpsc::channel(1);
        let id = live(&mut app, sender);
        app.server_mut().connected = true;
        app.server_mut().state.can_talk = true;
        app.server_mut().muted = false;
        assert_eq!(app.microphone_status(), "Waiting for push-to-talk");
        app.transmitting = true;
        assert_eq!(app.microphone_status(), "Transmitting");
        let _ = app.update(Message::Backend(id, client::Event::Transmitting(false)));
        assert_eq!(app.microphone_status(), "Waiting for push-to-talk");
        app.server_mut().muted = true;
        assert_eq!(app.microphone_status(), "Muted");
        // The mode can change during a session too.
        let _ = app.update(Message::Transmission(transmission::Settings::default()));
        assert_eq!(app.transmission, transmission::Settings::default());
    }

    #[test]
    fn microphone_status_and_meter_reflect_effective_capture_state() {
        let mut app = App::with_devices(audio::Devices::default(), None);
        let (id, _commands) = connection(&mut app);
        app.level = 0.25; // A queued meter event must not show activity after capture stops.
        assert_eq!(app.microphone_status(), "Inactive");
        assert_eq!(app.microphone_icon(), Icon::MicOff);
        assert_eq!(app.microphone_meter_level(), 0.0);
        assert_eq!(app.playback_icon(), Icon::Headphones);
        app.server_mut().connected = true;
        assert_eq!(app.microphone_status(), "Muted");
        assert_eq!(app.microphone_icon(), Icon::MicOff);
        assert_eq!(app.microphone_meter_level(), 0.0);
        app.server_mut().muted = false;
        assert_eq!(app.microphone_status(), "Blocked by server");
        assert_eq!(app.microphone_icon(), Icon::MicOff);
        assert_eq!(app.microphone_meter_level(), 0.0);
        app.server_mut().state.can_talk = true;
        assert_eq!(app.microphone_status(), "Unmuted");
        assert_eq!(app.microphone_icon(), Icon::Mic);
        assert_eq!(app.microphone_meter_level(), 0.5);
        app.server_mut().deafened = true;
        assert_eq!(app.microphone_status(), "Muted (deafened)");
        assert_eq!(app.microphone_icon(), Icon::MicOff);
        assert_eq!(app.microphone_meter_level(), 0.0);
        assert_eq!(app.playback_icon(), Icon::HeadphoneOff);
        app.loopback = true;
        assert_eq!(app.microphone_status(), "Local test only");
        assert_eq!(app.microphone_icon(), Icon::Mic);
        assert_eq!(app.microphone_meter_level(), 0.5);
        app.server_mut().stopping = true;
        assert_eq!(app.microphone_status(), "Inactive");
        assert_eq!(app.microphone_icon(), Icon::MicOff);
        assert_eq!(app.microphone_meter_level(), 0.0);
        let _ = app.update(Message::Backend(id, client::Event::Stopped(None)));
        assert_eq!(app.microphone_status(), "Inactive");
        assert_eq!(app.microphone_meter_level(), 0.0);
    }

    fn chat_event(scope: chat::Scope, from: &str, text: &str, own: bool) -> client::Event {
        client::Event::Chat(chat::Incoming {
            scope,
            own,
            from: from.into(),
            peer: "Mara".into(),
            text: text.into(),
        })
    }

    #[test]
    fn a_message_goes_where_the_box_points_and_links_are_marked_for_teamspeak() {
        let (mut app, mut commands) = session_fixture();
        let _ = app.update(Message::Chat(chat::Action::Send));
        assert!(commands.try_recv().is_err(), "Nothing to send yet");
        let _ = app.update(Message::Chat(chat::Action::Draft(
            " notes: https://example.org/vault ".into(),
        )));
        let _ = app.update(Message::Chat(chat::Action::Send));
        assert!(matches!(
            commands.try_recv(),
            Ok(client::Command::Chat(client::ChatTarget::Channel, text))
                if text == "notes: [URL]https://example.org/vault[/URL]"
        ));
        assert!(app.server().chat.draft.is_empty());
        // The server sends your own line back; only then does it show.
        assert!(app.server().chat.shown(chat::Layout::Tabs).is_empty());

        let _ = app.update(Message::Chat(chat::Action::Message(1)));
        assert_eq!(
            app.server().chat.target,
            chat::Scope::Channel,
            "You can't message yourself"
        );
        let _ = app.update(Message::Chat(chat::Action::Message(2)));
        assert_eq!(
            app.server().chat.target,
            chat::Scope::Private("uid-2".into())
        );
        let _ = app.update(Message::Chat(chat::Action::Draft("psst".into())));
        let _ = app.update(Message::Chat(chat::Action::Send));
        assert!(matches!(
            commands.try_recv(),
            Ok(client::Command::Chat(client::ChatTarget::Client(2), text)) if text == "psst"
        ));
        // Once they've left there is nobody to send to, and the draft stays.
        let mut state = app.server().state.clone();
        state.channels[0].members.retain(|member| member.id != 2);
        let id = app.active.unwrap();
        let _ = app.update(Message::Backend(id, client::Event::State(state)));
        let _ = app.update(Message::Chat(chat::Action::Draft("still there?".into())));
        let _ = app.update(Message::Chat(chat::Action::Send));
        assert!(commands.try_recv().is_err());
        assert_eq!(app.server().chat.draft, "still there?");
        let _ = app.update(Message::Chat(chat::Action::Target(chat::Scope::Server)));
        let _ = app.update(Message::Chat(chat::Action::Send));
        assert!(matches!(
            commands.try_recv(),
            Ok(client::Command::Chat(client::ChatTarget::Server, _))
        ));
    }

    #[test]
    fn unread_counts_follow_what_the_chat_column_shows() {
        let (mut app, _commands) = session_fixture();
        let id = app.active.unwrap();
        let mara = chat::Scope::Private("uid-2".into());
        assert!(app.chat_docked());
        // Tabs is on the channel: its lines are read as they arrive, a private one is not.
        let _ = app.update(Message::Backend(
            id,
            chat_event(chat::Scope::Channel, "Theo", "back", false),
        ));
        let _ = app.update(Message::Backend(
            id,
            chat_event(mara.clone(), "Mara", "psst", false),
        ));
        let chat = &app.server().chat;
        assert_eq!(
            (chat.unread(&chat::Scope::Channel), chat.unread(&mara)),
            (0, 1)
        );
        assert_eq!(chat.shown(chat::Layout::Tabs)[0].place, "Lobby");
        assert!(app.ui.chat_toast.is_none(), "The column is showing");
        // Opening her tab reads it.
        let _ = app.update(Message::Chat(chat::Action::Target(mara.clone())));
        assert_eq!(app.server().chat.unread(&mara), 0);
        // One Feed shows everything, so nothing arrives unread.
        let _ = app.update(Message::Chat(chat::Action::Layout(chat::Layout::Feed)));
        let _ = app.update(Message::Backend(
            id,
            chat_event(chat::Scope::Server, "Lukas", "restart at 4", false),
        ));
        assert_eq!(app.server().chat.unread_public(), 0);
        // Narrowed to the channel, a server line waits.
        let _ = app.update(Message::Chat(chat::Action::Filter(chat::Filter::Channel)));
        let _ = app.update(Message::Backend(
            id,
            chat_event(chat::Scope::Server, "Lukas", "really", false),
        ));
        assert_eq!(app.server().chat.unread(&chat::Scope::Server), 1);
    }

    #[test]
    fn messages_in_the_background_wait_unread_and_private_ones_sound() {
        let (mut app, _commands) = session_fixture();
        let id = app.active.unwrap();
        let mara = chat::Scope::Private("uid-2".into());
        let _ = app.update(Message::WindowFocus(false));
        assert!(!app.chat_showing(Instant::now()));
        let _ = app.update(Message::Backend(
            id,
            chat_event(chat::Scope::Channel, "Theo", "back", false),
        ));
        assert_eq!(app.server().chat.unread(&chat::Scope::Channel), 1);
        assert!(
            !app.played_sounds.contains(&sounds::Sound::PrivateMessage),
            "Channel messages only wait"
        );
        let _ = app.update(Message::Backend(
            id,
            chat_event(mara.clone(), "Mara", "psst", false),
        ));
        assert_eq!(app.server().chat.unread(&mara), 1);
        assert!(app.played_sounds.contains(&sounds::Sound::PrivateMessage));
        // Back in front, the conversation on screen is read.
        let _ = app.update(Message::WindowFocus(true));
        assert!(app.chat_showing(Instant::now()));
        assert_eq!(app.server().chat.unread(&chat::Scope::Channel), 0);
        app.played_sounds.clear();
        let _ = app.update(Message::Backend(
            id,
            chat_event(mara.clone(), "Mara", "again", false),
        ));
        assert!(app.played_sounds.is_empty(), "In front, the chat shows it");
        // Hidden in the tray counts as the background too.
        app.window_hidden = true;
        assert!(!app.chat_showing(Instant::now()));
    }

    #[test]
    fn closing_while_connected_asks_and_can_hide_and_stay() {
        let (mut app, mut commands) = session_fixture();
        let _ = app.update(Message::Close);
        assert!(app.close_prompt);
        assert!(commands.try_recv().is_err(), "Nothing ends while it asks");
        let _ = app.update(Message::CloseChoice(None));
        assert!(!app.close_prompt && !app.closing);
        let _ = app.update(Message::Close);
        let _ = app.update(Message::CloseChoice(Some(
            settings::CloseButton::HideToTray,
        )));
        assert!(!app.close_prompt && !app.closing && !app.window_focused);
        assert!(commands.try_recv().is_err(), "Still connected");
        let _ = app.update(Message::WindowFocus(true));
        assert!(!app.window_hidden);
        let _ = app.update(Message::Close);
        let _ = app.update(Message::CloseChoice(Some(settings::CloseButton::Quit)));
        assert!(app.closing);
        assert!(matches!(commands.try_recv(), Ok(client::Command::Stop)));
        let mut idle = App::with_devices(audio::Devices::default(), None);
        assert!(matches!(idle.update(Message::Close), Task::Exit));
    }

    #[test]
    fn disconnect_asks_first_only_while_sharing() {
        let (mut app, mut commands) = session_fixture();
        app.server_mut().streaming.sharing.status.active = true;
        let _ = app.update(Message::Stop);
        assert!(app.disconnect_prompt);
        assert!(commands.try_recv().is_err());
        let _ = app.update(Message::ConfirmDisconnect(false));
        assert!(!app.disconnect_prompt && !app.server().stopping);
        let _ = app.update(Message::Stop);
        let _ = app.update(Message::ConfirmDisconnect(true));
        assert!(app.server().stopping);
        let (mut app, mut commands) = session_fixture();
        let _ = app.update(Message::Stop);
        assert!(!app.disconnect_prompt && app.server().stopping);
        assert!(matches!(commands.try_recv(), Ok(client::Command::Stop)));
    }

    #[test]
    fn a_hidden_chat_pops_messages_up_and_lights_its_handle() {
        let (mut app, _commands) = session_fixture();
        let id = app.active.unwrap();
        let mara = chat::Scope::Private("uid-2".into());
        let _ = app.update(Message::Chat(chat::Action::Toggle));
        assert!(app.chat_options.hidden && !app.chat_docked());
        let _ = app.update(Message::Backend(
            id,
            chat_event(chat::Scope::Channel, "Me", "my own line", true),
        ));
        assert!(app.ui.chat_toast.is_none(), "Your own lines never pop up");
        assert_eq!(app.server().chat.unread_public(), 0);
        let _ = app.update(Message::Backend(
            id,
            chat_event(chat::Scope::Channel, "Theo", "anyone?", false),
        ));
        assert_eq!(app.ui.chat_toast.as_ref().unwrap().entry.text, "anyone?");
        assert_eq!(app.server().chat.unread_public(), 1);
        // Only private ones pop up when asked; the rest still light the dot.
        let _ = app.update(Message::Chat(chat::Action::DismissToast));
        let _ = app.update(Message::Chat(chat::Action::Options(settings::Chat {
            popups_private_only: true,
            ..app.chat_options
        })));
        let _ = app.update(Message::Backend(
            id,
            chat_event(chat::Scope::Channel, "Theo", "hello?", false),
        ));
        assert!(app.ui.chat_toast.is_none());
        assert_eq!(app.server().chat.unread_public(), 2);
        let _ = app.update(Message::Backend(
            id,
            chat_event(mara.clone(), "Mara", "psst", false),
        ));
        assert_eq!(app.ui.chat_toast.as_ref().unwrap().entry.scope, mara);
        // Reply peeks the column on that conversation with the box ready.
        let _ = app.update(Message::Chat(chat::Action::Reply(mara.clone())));
        assert!(app.ui.chat_toast.is_none() && app.ui.chat_input.focused);
        assert!(app.chat_options.hidden, "A reply doesn't dock the column");
        assert_eq!(app.server().chat.target, mara);
        assert_eq!(app.server().chat.unread(&mara), 0);
        assert_eq!(
            app.server().chat.unread_public(),
            2,
            "The channel tab isn't open"
        );
        // Open Chat docks it.
        let _ = app.update(Message::Chat(chat::Action::Open(chat::Scope::Channel)));
        assert!(app.chat_docked());
        assert_eq!(app.server().chat.unread_public(), 0);
    }

    #[test]
    fn a_message_on_another_server_waits_without_a_pop_up() {
        let (mut app, _commands) = session_fixture();
        let first = app.active.unwrap();
        let (sender, _other) = mpsc::channel(4);
        let second = live(&mut app, sender);
        app.server_mut().connected = true;
        let _ = app.update(Message::Chat(chat::Action::Toggle));
        let mara = chat::Scope::Private("uid-2".into());
        let _ = app.update(Message::Backend(
            first,
            chat_event(mara.clone(), "Mara", "hi", false),
        ));
        assert!(app.ui.chat_toast.is_none());
        assert_eq!(app.session(first).unwrap().chat.unread_private(), 1);
        assert_eq!(app.session(second).unwrap().chat.unread_private(), 0);
    }

    #[test]
    fn changing_channel_leaves_the_old_one_as_history() {
        let (mut app, mut commands) = session_fixture();
        let id = app.active.unwrap();
        // The fixture is already in the Lobby; tell the chat as a State event would.
        let state = app.server().state.clone();
        let _ = app.update(Message::Backend(id, client::Event::State(state.clone())));
        let _ = app.update(Message::Backend(
            id,
            chat_event(chat::Scope::Channel, "Theo", "in the lobby", false),
        ));
        let mut moved = state.clone();
        moved.current = Some(moved.channels[1].clone());
        let _ = app.update(Message::Backend(id, client::Event::State(moved)));
        let chat = &app.server().chat;
        assert!(
            chat.shown(chat::Layout::Tabs).is_empty(),
            "Games starts empty"
        );
        assert_eq!(chat.past().map(|room| room.id).collect::<Vec<_>>(), [1]);
        // The Lobby's lines can be read but not added to.
        let _ = app.update(Message::Chat(chat::Action::Target(chat::Scope::Past(1))));
        assert_eq!(app.server().chat.shown(chat::Layout::Tabs).len(), 1);
        let _ = app.update(Message::Chat(chat::Action::Draft("hello".into())));
        let _ = app.update(Message::Chat(chat::Action::Send));
        assert!(commands.try_recv().is_err());
        // Back in the Lobby its tab is the channel tab again.
        let _ = app.update(Message::Backend(id, client::Event::State(state)));
        assert_eq!(app.server().chat.target, chat::Scope::Channel);
        assert_eq!(app.server().chat.shown(chat::Layout::Tabs).len(), 1);
        let _ = app.update(Message::Chat(chat::Action::Send));
        assert!(matches!(
            commands.try_recv(),
            Ok(client::Command::Chat(client::ChatTarget::Channel, _))
        ));
    }

    #[test]
    fn conversations_come_back_after_a_restart_until_history_is_off_or_cleared() {
        let (mut app, root) = saved_fixture("chat-history");
        let settings_file = root.join("settings.json");
        let mara = chat::Scope::Private("uid-2".into());
        let connect = |app: &mut App| {
            let (sender, commands) = mpsc::channel(16);
            let id = live(app, sender);
            let _ = app.update(Message::Backend(
                id,
                client::Event::Connected("test".into()),
            ));
            (id, commands)
        };
        let restart = || {
            let mut app = App::with_devices(audio::Devices::default(), None);
            app.load_settings(settings_file.clone());
            app
        };
        let (id, _commands) = connect(&mut app);
        assert!(app.server().chat.shown(chat::Layout::Feed).is_empty());
        let _ = app.update(Message::Backend(
            id,
            chat_event(chat::Scope::Channel, "Theo", "see you tomorrow", false),
        ));
        let _ = app.update(Message::Backend(
            id,
            chat_event(mara.clone(), "Mara", "psst", false),
        ));
        assert!(root.join(chat_store::FILE).exists());

        // The next launch has both lines again, and Mara's tab, with nothing unread.
        let mut app = restart();
        let (id, _commands) = connect(&mut app);
        let texts = |app: &App| -> Vec<String> {
            let lines = app.server().chat.shown(chat::Layout::Feed);
            lines.iter().map(|line| line.text.clone()).collect()
        };
        assert_eq!(texts(&app), ["see you tomorrow", "psst"]);
        assert_eq!(app.server().chat.peers[0].name, "Mara");
        assert_eq!(app.server().chat.unread_private(), 0);
        // A reconnect in the same run doesn't bring them a second time.
        let _ = app.update(Message::Backend(
            id,
            client::Event::Connected("test".into()),
        ));
        assert_eq!(texts(&app).len(), 2);
        // A closed tab stays closed after a restart.
        let _ = app.update(Message::Chat(chat::Action::ClosePeer("uid-2".into())));
        // With history off, what is said now is gone after a restart.
        let _ = app.update(Message::Chat(chat::Action::Options(settings::Chat {
            history: false,
            ..app.chat_options
        })));
        let _ = app.update(Message::Backend(
            id,
            chat_event(chat::Scope::Channel, "Theo", "off the record", false),
        ));
        let mut app = restart();
        assert!(!app.chat_options.history);
        let (_id, _commands) = connect(&mut app);
        assert!(
            texts(&app).is_empty(),
            "Nothing is read while it is off either"
        );
        let _ = app.update(Message::Chat(chat::Action::Options(settings::Chat {
            history: true,
            ..app.chat_options
        })));
        let mut app = restart();
        let (_id, _commands) = connect(&mut app);
        assert_eq!(texts(&app), ["see you tomorrow", "psst"]);
        assert!(app.server().chat.peers.is_empty(), "Mara's tab was closed");

        // Clearing asks twice, then empties the file and the window.
        let _ = app.update(Message::Chat(chat::Action::ClearHistory));
        assert_eq!(texts(&app).len(), 2);
        let _ = app.update(Message::Chat(chat::Action::ClearHistory));
        assert!(texts(&app).is_empty());
        let mut app = restart();
        let (_id, _commands) = connect(&mut app);
        assert!(texts(&app).is_empty());
        assert!(app.alerts.visible().is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn pokes_are_kept_in_the_conversation_both_ways() {
        let (mut app, mut commands) = session_fixture();
        let id = app.active.unwrap();
        let _ = app.update(Message::OpenPoke(2));
        let _ = app.update(Message::PokeText("Raid now".into()));
        let _ = app.update(Message::SubmitPoke);
        assert!(matches!(
            commands.try_recv(),
            Ok(client::Command::Poke(2, _))
        ));
        let _ = app.update(Message::Backend(
            id,
            client::Event::Poked {
                from: "Mara".into(),
                key: "uid-2".into(),
                message: "coming".into(),
            },
        ));
        let _ = app.update(Message::Chat(chat::Action::Message(2)));
        let lines = app.server().chat.shown(chat::Layout::Tabs);
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().all(|line| line.kind == chat::Kind::Poke));
        assert_eq!((lines[0].own, lines[0].text.as_str()), (true, "Raid now"));
        assert_eq!((lines[1].own, lines[1].from.as_str()), (false, "Mara"));
        assert_eq!(
            app.server().chat.unread_private(),
            0,
            "A poke has its own dialog and chime"
        );
    }

    #[test]
    fn the_chat_layout_and_hiding_are_saved_straight_away() {
        let (mut app, root) = saved_fixture("chat");
        let path = root.join("settings.json");
        let _ = app.update(Message::Chat(chat::Action::Layout(chat::Layout::Feed)));
        let (sender, _commands) = mpsc::channel(4);
        live(&mut app, sender);
        app.server_mut().connected = true;
        let _ = app.update(Message::Chat(chat::Action::Toggle));
        let saved = settings::load(&path).unwrap().unwrap();
        assert_eq!(saved.chat.layout, chat::Layout::Feed);
        assert!(saved.chat.hidden);
        let _ = app.update(Message::Chat(chat::Action::Toggle));
        assert!(!settings::load(&path).unwrap().unwrap().chat.hidden);
        // Too narrow to dock: the column peeks instead, and the choice stays as saved.
        app.window_size = (crate::MIN_SIZE.0, 600.0);
        assert!(!app.chat_docked());
        let _ = app.update(Message::Chat(chat::Action::Toggle));
        assert!(app.ui.chat_peek_open(Instant::now()));
        assert!(!settings::load(&path).unwrap().unwrap().chat.hidden);
        let _ = std::fs::remove_dir_all(root);
    }
}
