#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod alerts;
mod audio;
mod client;
mod command_flow;
mod connection_charts;
mod icons;
mod memory;
mod onboarding;
mod permissions;
mod profiler;
mod resume;
mod settings;
mod shortcuts;
mod streaming;
mod system_appearance;
mod ui;
mod updater;
mod usage;
mod window_icon;

use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    path::PathBuf,
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

const NORMAL_MIN_SIZE: (f32, f32) = (760.0, 560.0);
const COMPACT_MIN_SIZE: (f32, f32) = (440.0, 320.0);

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
                "Flexspeak\n\nNo arguments       Open the client\n--list-devices     List audio inputs and outputs\n--probe ADDRESS    Connect muted, report server state, disconnect\n--identity-path    Print the local identity file path\n--settings-path    Print the portable settings JSON path"
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
        // Rune's headless preview tooling (`--snap`, `--scene`, `--tree`, …) reads the rest.
        Some(other) if ui::is_tool_flag(other) => {}
        Some(other) => anyhow::bail!("Unknown argument '{other}'. Use --help."),
        None => {}
    }
    ui::run();
    Ok(())
}

struct App {
    onboarding: Option<onboarding::Step>,
    onboarding_error: Option<String>,
    permissions: permissions::Permissions,
    saved: Option<settings::Settings>,
    last_shortcut: Option<shortcuts::Action>,
    shortcuts: Option<shortcuts::Shortcuts>,
    keybinds: shortcuts::Keybinds,
    keybind_draft: shortcuts::Keybinds,
    shortcut_error: Option<String>,
    settings_path: Option<PathBuf>,
    alerts: Alerts,
    settings_message: Option<String>,
    playback_volume: Arc<audio::PlaybackVolume>,
    performance_mode: Arc<AtomicBool>,
    channel_health_visible: Arc<AtomicBool>,
    subscribe_all: Arc<AtomicBool>,
    /// Saved straight to settings when toggled, not held until the Settings page saves.
    footer_hidden: bool,
    /// Saved straight to settings when changed, like `footer_hidden`.
    appearance: settings::Appearance,
    /// Whether the system is in dark mode, refreshed with each usage tick.
    system_dark: bool,
    window_restore: Option<(f32, f32)>,
    window_size: (f32, f32),
    next_usage_tick: Instant,
    handle: Option<rune::AppHandle<App>>,
    ui: ui::UiState,
    address: String,
    nickname: String,
    password: String,
    channel_password: String,
    password_channel: Option<u64>,
    devices: audio::Devices,
    input: Option<audio::DeviceChoice>,
    output: Option<audio::DeviceChoice>,
    status: String,
    password_error: Option<String>,
    server_controls: Option<command_flow::AudioState>,
    controls_pending: bool,
    detail: String,
    session: Option<mpsc::Sender<client::Command>>,
    connected: bool,
    loopback: bool,
    stopping: bool,
    closing: bool,
    muted: bool,
    deafened: bool,
    level: f32,
    sent: u64,
    received: u64,
    state: client::Snapshot,
    collapsed_channels: HashSet<u64>,
    expanded_client: Option<u16>,
    /// Clients heard right now, including yourself while you transmit.
    talking: BTreeSet<u16>,
    client_usage: BTreeMap<u16, Result<usage::ServerUsage, &'static str>>,
    connection_history: ConnectionHistory,
    client_history: ConnectionHistory,
    process_monitor: usage::ProcessMonitor,
    process_usage: usage::ProcessUsage,
    cpu_history: usage::MetricHistory,
    ram_history: usage::MetricHistory,
    network_usage: usage::NetworkUsage,
    connection_usage: usage::ConnectionUsage,
    tab: Tab,
    streaming: streaming::View,
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
    /// The password last used to join a channel, so an update's restart can rejoin it.
    channel_secret: Option<(u64, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Home,
    Statistics,
    Settings,
}

#[derive(Debug, Clone)]
enum Message {
    SetupNext,
    SetupBack,
    TestHotkeys,
    RequestPermission(permissions::Kind),
    PermissionDone(permissions::Kind, Result<String, String>),
    OpenPermissionSettings(permissions::Kind),
    DismissError,
    Shortcut(shortcuts::Action),
    MuteKeybind(String),
    DeafenKeybind(String),
    SaveSettings,
    ReloadSettings,
    CopySettingsPath,
    PlaybackVolume(u8),
    PerformanceMode(bool),
    Address(String),
    Nickname(String),
    Password(String),
    ChannelPassword(String),
    SubmitChannelPassword,
    CancelChannelPassword,
    Input(audio::DeviceChoice),
    Output(audio::DeviceChoice),
    Refresh,
    Connect,
    TestAudio,
    Stop,
    Mute,
    Deafen,
    Join(u64),
    ToggleChannel(u64),
    ToggleSubscribeAll,
    ToggleFooter,
    Appearance(settings::Appearance),
    ToggleClient(u16),
    Backend(client::Event),
    Close,
    UsageTick,
    CompactUsageTick,
    ShowTab(Tab),
    RefreshStreams,
    WatchStream(String),
    StopWatching,
    RefreshShareSources,
    ShareSources(Result<Vec<streaming::publishing::ShareSource>, String>),
    ShareSource(streaming::publishing::ShareSource),
    ShareAudio(bool),
    ShareAudioOutput(audio::DeviceChoice),
    StartSharing,
    StopSharing,
    DecideViewer(u16, bool),
    RecordTrace,
    StopTrace,
    TraceSaved(Result<PathBuf, String>),
    CopyTracePath,
    Updater(updater::Event),
    /// Returns to where the app was before an update's restart.
    Resume,
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
        app.system_dark = system_appearance::is_dark().unwrap_or(true);
        match settings::path() {
            Ok(path) => app.load_settings(path),
            Err(error) => app.alerts.raise(Source::SettingsLoad, format!("{error:#}")),
        }
        let (mut shortcuts, events) = shortcuts::Shortcuts::new();
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
            last_shortcut: None,
            shortcuts: None,
            keybinds: shortcuts::Keybinds::default(),
            keybind_draft: shortcuts::Keybinds::default(),
            shortcut_error: None,
            settings_path: None,
            alerts,
            settings_message: None,
            playback_volume: Arc::new(audio::PlaybackVolume::default()),
            performance_mode: Arc::new(AtomicBool::new(false)),
            channel_health_visible: Arc::new(AtomicBool::new(true)),
            subscribe_all: Arc::new(AtomicBool::new(true)),
            footer_hidden: false,
            appearance: settings::Appearance::default(),
            system_dark: true,
            window_restore: None,
            window_size: (900.0, 790.0),
            next_usage_tick: Instant::now() + usage::REFRESH_INTERVAL,
            handle: None,
            ui: ui::UiState::default(),
            input: devices.input.clone(),
            output: devices.output.clone(),
            devices,
            address: "ts.zirkumflex.io".into(),
            nickname: "Flexspeak".into(),
            password: String::new(),
            channel_password: String::new(),
            password_channel: None,
            status: "Ready to connect".into(),
            password_error: None,
            server_controls: None,
            controls_pending: false,
            detail: String::new(),
            session: None,
            connected: false,
            loopback: false,
            stopping: false,
            closing: false,
            muted: true,
            deafened: false,
            level: 0.0,
            sent: 0,
            received: 0,
            state: client::Snapshot::default(),
            collapsed_channels: HashSet::new(),
            expanded_client: None,
            talking: BTreeSet::new(),
            client_usage: BTreeMap::new(),
            connection_history: ConnectionHistory::new(),
            client_history: ConnectionHistory::new(),
            process_monitor,
            process_usage,
            cpu_history,
            ram_history,
            network_usage: usage::NetworkUsage::default(),
            connection_usage: usage::ConnectionUsage::default(),
            tab: Tab::Home,
            streaming: streaming::View::default(),
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
            channel_secret: None,
        }
    }

    fn command(&mut self, command: client::Command) -> bool {
        let Some(sender) = &self.session else {
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
        if self.connected && !self.loopback && !self.stopping {
            let value = !self.muted;
            self.alerts.begin(Source::Controls);
            if self.command(client::Command::Mute(value)) {
                self.muted = value;
                self.controls_pending = true;
            }
        }
    }

    fn toggle_deafen(&mut self) {
        if self.connected && !self.loopback && !self.stopping {
            let value = !self.deafened;
            self.alerts.begin(Source::Controls);
            if self.command(client::Command::Deafen(value)) {
                self.deafened = value;
                self.controls_pending = true;
            }
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
                self.tab = Tab::Settings;
                self.alerts.raise(Source::SettingsLoad,format!("{error:#}. The file was not changed. Repair it and Reload, or Save to replace it with the values shown."));
            }
        }
        self.settings_path = Some(path);
    }

    fn begin_setup(&mut self) {
        self.onboarding = Some(onboarding::Step::Username);
        self.onboarding_error = None;
        self.saved = None;
        self.nickname.clear();
        self.address.clear();
        self.password.clear();
        self.tab = Tab::Home;
        self.sync_page_visibility();
    }

    /// Settings and the setup step show live status; re-read it while either is open, so a
    /// change made in the OS's privacy settings appears on return without a restart.
    fn refresh_visible_permissions(&mut self) {
        let showing = match self.onboarding {
            Some(step) => step == onboarding::Step::Permissions,
            None => self.tab == Tab::Settings,
        };
        if showing {
            self.permissions.refresh();
        }
    }

    fn sync_page_visibility(&self) {
        let normal = self.onboarding.is_none() && !self.performance_mode.load(Ordering::Relaxed);
        self.streaming
            .visible
            .store(normal && self.tab == Tab::Home, Ordering::Relaxed);
        self.channel_health_visible.store(
            normal && matches!(self.tab, Tab::Home | Tab::Statistics),
            Ordering::Relaxed,
        );
    }

    fn apply_settings(&mut self, saved: settings::Settings) {
        if self.address.trim() != saved.server_address.trim() {
            self.password.clear();
            self.channel_password.clear();
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
        self.address = saved.server_address;
        self.nickname = saved.nickname;
        self.playback_volume
            .set_percent(saved.playback_volume_percent);
        self.keybinds = saved.keybinds.clone();
        self.keybind_draft = saved.keybinds;
        self.footer_hidden = saved.footer_hidden;
        self.updater.auto_download = saved.auto_download_updates;
        self.appearance = saved.appearance;
    }

    fn edited_settings(&self) -> settings::Settings {
        settings::Settings {
            server_address: self.address.trim().into(),
            nickname: self.nickname.trim().into(),
            playback_volume_percent: self.playback_volume.percent(),
            keybinds: shortcuts::Keybinds {
                toggle_mute: self.keybind_draft.toggle_mute.trim().into(),
                toggle_deafen: self.keybind_draft.toggle_deafen.trim().into(),
            },
            input_device_id: self.input.as_ref().map(|device| device.id.clone()),
            output_device_id: self.output.as_ref().map(|device| device.id.clone()),
            footer_hidden: self.footer_hidden,
            auto_download_updates: self.updater.auto_download,
            appearance: self.appearance,
        }
    }

    /// Hides or shows the footer and stores only that choice, leaving unsaved edits alone.
    fn set_footer_hidden(&mut self, hidden: bool) {
        self.footer_hidden = hidden;
        self.save_preference(
            |saved| saved.footer_hidden = hidden,
            "The footer choice applies for now but was not saved",
        );
    }

    /// Changes the colour scheme or themes and stores only that choice.
    fn set_appearance(&mut self, appearance: settings::Appearance) {
        self.appearance = appearance;
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
                self.update(Message::Close)
            }
        }
    }

    /// The page, session and channel to return to after an update's restart.
    fn resume_snapshot(&self) -> resume::Resume {
        let in_session = self.connected && !self.loopback && !self.stopping;
        let channel = self
            .state
            .current
            .as_ref()
            .map(|channel| channel.id)
            .filter(|_| in_session);
        resume::Resume {
            written_at: 0,
            page: match self.tab {
                Tab::Home => resume::Page::Home,
                Tab::Statistics => resume::Page::Statistics,
                Tab::Settings => resume::Page::Settings,
            },
            performance_mode: self.performance_mode.load(Ordering::Relaxed),
            server_address: in_session.then(|| self.address.clone()),
            server_password: if in_session {
                self.password.clone()
            } else {
                String::new()
            },
            channel,
            channel_password: self
                .channel_secret
                .as_ref()
                .filter(|(id, _)| Some(*id) == channel)
                .map(|(_, password)| password.clone())
                .unwrap_or_default(),
            muted: self.muted,
            deafened: self.deafened,
        }
    }

    fn resume(&mut self) -> Task {
        let Some(resume) = self.resume.clone() else {
            return Task::none();
        };
        let mut tasks = vec![self.update(Message::ShowTab(match resume.page {
            resume::Page::Home => Tab::Home,
            resume::Page::Statistics => Tab::Statistics,
            resume::Page::Settings => Tab::Settings,
        }))];
        if resume.performance_mode {
            tasks.push(self.update(Message::PerformanceMode(true)));
        }
        // Only the saved server is reconnected; Connect always uses it.
        let saved = self.saved.as_ref().map(|saved| &saved.server_address);
        if resume.server_address.is_some() && resume.server_address.as_ref() == saved {
            self.password = resume.server_password;
            tasks.push(self.update(Message::Connect));
        }
        if self.session.is_none() {
            self.resume = None;
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
                self.settings_message = Some("Settings saved".into());
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

    fn configure_shortcuts(&mut self, keys: &shortcuts::Keybinds) -> anyhow::Result<()> {
        if !shortcuts::supported() {
            self.shortcut_error =
                Some("Global shortcuts require X11; native Wayland is not supported.".into());
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
        }
        Ok(())
    }

    fn start(&mut self, loopback: bool) -> Task {
        if self.session.is_some()
            || self.permissions.pending.is_some()
            || (!loopback && self.onboarding.is_some())
        {
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
            self.alerts
                .raise(Source::Connection, "Enter a server address and nickname.");
            return Task::none();
        }
        let config = client::Config {
            playback_volume: self.playback_volume.clone(),
            performance_mode: self.performance_mode.clone(),
            channel_health_visible: self.channel_health_visible.clone(),
            subscribe_all: self.subscribe_all.clone(),
            streaming_visible: self.streaming.visible.clone(),
            video_surface: self.streaming.surface.clone(),
            address: self.address.trim().into(),
            nickname: self.nickname.trim().into(),
            password: self.password.clone(),
            input,
            output,
        };
        for source in [
            Source::Connection,
            Source::Audio,
            Source::Worker,
            Source::Join,
            Source::Controls,
            Source::Subscription,
            Source::Flood,
        ] {
            self.alerts.begin(source);
        }
        self.server_controls = None;
        self.controls_pending = !loopback;
        self.loopback = loopback;
        self.muted = true;
        self.deafened = false;
        self.sent = 0;
        self.received = 0;
        self.state = client::Snapshot::default();
        self.streaming.reset();
        self.password_channel = None;
        self.channel_password.clear();
        self.collapsed_channels.clear();
        self.clear_client_details();
        self.client_usage.clear();
        self.talking.clear();
        self.connection_history = ConnectionHistory::new();
        self.network_usage = usage::NetworkUsage::default();
        self.connection_usage = usage::ConnectionUsage::default();
        self.status = if loopback {
            "Opening local microphone test…"
        } else {
            "Connecting…"
        }
        .into();
        match client::start(config, loopback) {
            Ok((sender, events)) => {
                self.session = Some(sender);
                Task::Events(events)
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
        let _trace = profiler::scope(profiler::Category::Ui, "ui.update");
        match message {
            Message::SetupNext => {
                if self.session.is_some() || self.permissions.pending.is_some() {
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
                        self.onboarding = None;
                        self.tab = Tab::Home;
                        self.sync_page_visibility();
                    }
                }
            }
            Message::SetupBack => {
                if self.session.is_none()
                    && self.permissions.pending.is_none()
                    && let Some(previous) = self.onboarding.and_then(onboarding::Step::previous)
                {
                    self.onboarding = Some(previous);
                    self.onboarding_error = None;
                    self.refresh_visible_permissions();
                }
            }
            Message::TestHotkeys => {
                self.alerts.begin(Source::Shortcuts);
                let keys = self.edited_settings().keybinds;
                match keys.parse().and_then(|_| self.configure_shortcuts(&keys)) {
                    Ok(()) => {
                        self.keybinds = keys;
                        self.last_shortcut = None;
                    }
                    Err(error) => self.alerts.raise(Source::Shortcuts, format!("{error:#}")),
                }
            }
            Message::RequestPermission(kind) => {
                if self.session.is_none() && self.permissions.pending.is_none() {
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
                if self.permissions.pending.is_none() && self.session.is_none() {
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
            Message::DismissError => self.alerts.dismiss(),
            Message::Shortcut(action) => {
                self.last_shortcut = Some(action);
                match action {
                    shortcuts::Action::Mute => self.toggle_mute(),
                    shortcuts::Action::Deafen => self.toggle_deafen(),
                }
            }
            Message::PlaybackVolume(percent) => {
                self.playback_volume.set_percent(percent);
                self.settings_message = None;
            }
            Message::MuteKeybind(value) => {
                self.keybind_draft.toggle_mute = value;
                self.settings_message = None;
            }
            Message::DeafenKeybind(value) => {
                self.keybind_draft.toggle_deafen = value;
                self.settings_message = None;
            }
            Message::SaveSettings => {
                if self.onboarding.is_none() {
                    self.save_settings();
                }
            }
            Message::ReloadSettings => {
                if self.session.is_none()
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
            Message::CopySettingsPath => {
                if let Some(path) = &self.settings_path {
                    return Task::Copy(path.display().to_string());
                }
            }
            Message::PerformanceMode(enabled) => {
                if self.onboarding.is_some() {
                    return Task::none();
                }
                if self.performance_mode.swap(enabled, Ordering::Relaxed) == enabled {
                    return Task::none();
                }
                self.sync_page_visibility();
                if enabled && self.expanded_client.is_some() {
                    self.select_client(None);
                }
                if enabled {
                    self.streaming.sharing.stop_capture();
                    self.streaming.clear_picture();
                    self.client_usage.clear();
                    self.connection_history = ConnectionHistory::new();
                    self.client_history = ConnectionHistory::new();
                    self.connection_usage = usage::ConnectionUsage::default();
                }
                // Prime CPU timing for the new sampling interval in either mode.
                self.process_monitor = usage::ProcessMonitor::new();
                self.process_usage = self.process_monitor.sample();
                self.next_usage_tick = Instant::now()
                    + if enabled {
                        usage::COMPACT_REFRESH_INTERVAL
                    } else {
                        usage::REFRESH_INTERVAL
                    };
                if enabled {
                    // Keep the first restore size if a switch is repeated before it lands.
                    self.window_restore.get_or_insert(self.window_size);
                    return Task::batch([self.stop_trace(), Task::Resize(COMPACT_MIN_SIZE)]);
                }
                self.cpu_history.push(Instant::now(), None);
                self.ram_history.push(Instant::now(), None);
                if let Some(size) = self.window_restore.take() {
                    return Task::Resize((
                        size.0.max(NORMAL_MIN_SIZE.0),
                        size.1.max(NORMAL_MIN_SIZE.1),
                    ));
                }
            }
            Message::ShowTab(tab) => {
                if self.onboarding.is_some() {
                    return Task::none();
                }
                if tab != Tab::Statistics && self.expanded_client.is_some() {
                    self.select_client(None);
                }
                self.tab = tab;
                self.sync_page_visibility();
                self.refresh_visible_permissions();
                if tab != Tab::Home {
                    self.streaming.sharing.stop_capture();
                    self.streaming.clear_picture();
                }
            }
            Message::RefreshStreams => {
                self.alerts.begin(Source::StreamDiscovery);
                self.command(client::Command::RefreshStreams);
            }
            Message::WatchStream(id) => {
                if self.connected && !self.loopback && !self.stopping && self.tab == Tab::Home {
                    self.alerts.begin(Source::StreamWatch);
                    self.alerts.begin(Source::StreamFrame);
                    self.command(client::Command::WatchStream(id));
                }
            }
            Message::StopWatching => {
                if self.command(client::Command::StopWatching) {
                    self.streaming.clear_picture();
                }
            }
            Message::RefreshShareSources => {
                if !self.streaming.sharing.status.active && !self.streaming.sharing.loading {
                    self.streaming.sharing.loading = true;
                    self.alerts.begin(Source::StreamPublish);
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
                self.streaming.sharing.loading = false;
                match result {
                    Ok(sources) => {
                        if self
                            .streaming
                            .sharing
                            .selected
                            .as_ref()
                            .is_some_and(|selected| !sources.contains(selected))
                        {
                            self.streaming.sharing.selected = None;
                        }
                        if sources.len() == 1 {
                            self.streaming.sharing.selected = sources.first().cloned();
                        }
                        self.streaming.sharing.sources = sources;
                    }
                    Err(error) => self.alerts.raise(Source::StreamPublish, error),
                }
            }
            Message::ShareSource(source) => {
                if !self.streaming.sharing.status.active {
                    self.streaming.sharing.selected = Some(source);
                }
            }
            Message::ShareAudio(audio) => {
                if !self.streaming.sharing.status.active {
                    self.streaming.sharing.audio = audio;
                }
            }
            Message::ShareAudioOutput(device) => {
                if !self.streaming.sharing.status.active {
                    self.streaming.sharing.audio_device = Some(device);
                }
            }
            Message::StartSharing => {
                if self.connected
                    && !self.loopback
                    && !self.stopping
                    && self.tab == Tab::Home
                    && let Some(source) = self.streaming.sharing.selected.clone()
                {
                    self.alerts.begin(Source::StreamPublish);
                    let audio = if self.streaming.sharing.audio {
                        let Some(device) = self
                            .streaming
                            .sharing
                            .audio_device
                            .clone()
                            .or_else(|| self.devices.output.clone())
                        else {
                            self.alerts.raise(
                                Source::StreamPublish,
                                "Choose an output device for system audio",
                            );
                            return Task::none();
                        };
                        Some(device)
                    } else {
                        None
                    };
                    self.command(client::Command::StartSharing(
                        streaming::publishing::Options { source, audio },
                    ));
                }
            }
            Message::StopSharing => {
                self.streaming.sharing.stop_capture();
                self.command(client::Command::StopSharing);
            }
            Message::DecideViewer(id, allow) => {
                self.command(client::Command::DecideViewer(id, allow));
            }
            Message::RecordTrace => {
                if self.recording.is_none()
                    && !self.trace_saving
                    && !self.performance_mode.load(Ordering::Relaxed)
                {
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
            Message::CompactUsageTick => {
                if self.performance_mode.load(Ordering::Relaxed) {
                    self.process_usage = self.process_monitor.sample();
                }
            }
            Message::UsageTick => {
                self.refresh_visible_permissions();
                if self.performance_mode.load(Ordering::Relaxed) {
                    return Task::none();
                }
                self.process_usage = self.process_monitor.sample();
                let now = Instant::now();
                if self.connected {
                    self.connection_history.advance(now);
                    self.client_history.advance(now);
                }
                self.cpu_history
                    .push(now, self.process_usage.cpu_percent.map(f64::from));
                self.ram_history.push(
                    now,
                    self.process_usage.memory_bytes.map(|bytes| bytes as f64),
                );
                self.heap_usage = memory::snapshot();
                profiler::record_counters(
                    self.process_usage.memory_bytes,
                    self.audio_buffer_bytes,
                    self.network_usage.upload_per_second,
                    self.network_usage.download_per_second,
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
                if self.session.is_none() {
                    if self.address != value {
                        self.password.clear();
                        self.channel_password.clear();
                    }
                    self.address = value;
                    self.settings_message = None;
                }
            }
            Message::Nickname(value) => {
                if self.session.is_none() {
                    self.nickname = value;
                    self.settings_message = None;
                }
            }
            Message::Password(value) => self.password = value,
            Message::ChannelPassword(value) => self.channel_password = value,
            Message::Input(value) => {
                if self.session.is_none() {
                    self.input = Some(value);
                }
            }
            Message::Output(value) => {
                if self.session.is_none() {
                    self.output = Some(value);
                }
            }
            Message::Refresh => {
                self.alerts.begin(Source::Audio);
                if self.session.is_none() {
                    match audio::devices() {
                        Ok(devices) => {
                            if !self
                                .input
                                .as_ref()
                                .is_some_and(|d| devices.inputs.contains(d))
                            {
                                self.input = devices.input.clone();
                            }
                            if !self
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
            }
            Message::Connect => {
                if self.onboarding.is_some() {
                    return Task::none();
                }
                if let Some(saved) = &self.saved {
                    if self.address != saved.server_address {
                        self.password.clear();
                    }
                    self.address = saved.server_address.clone();
                    self.nickname = saved.nickname.clone();
                }
                self.alerts.begin(Source::Connection);
                self.alerts.begin(Source::Audio);
                return self.start(false);
            }
            Message::TestAudio => {
                self.alerts.begin(Source::Audio);
                return self.start(true);
            }
            Message::Stop => {
                self.streaming.sharing.stop_capture();
                self.command(client::Command::Stop);
                self.stopping = true;
                self.status = "Disconnecting…".into();
            }
            Message::Mute => self.toggle_mute(),
            Message::Deafen => self.toggle_deafen(),
            Message::Join(id) => {
                if self.can_request_join(id) {
                    self.alerts.begin(Source::Join);
                    self.password_error = None;
                    self.channel_password.clear();
                    if self
                        .state
                        .channels
                        .iter()
                        .any(|channel| channel.id == id && channel.has_password)
                    {
                        self.password_channel = Some(id);
                        return Task::Focus(ui::Field::ChannelPassword);
                    }
                    self.command(client::Command::Join(id, String::new()));
                }
            }
            Message::SubmitChannelPassword => {
                if let Some(id) = self.password_channel
                    && self.can_request_join(id)
                    && self.command(client::Command::Join(id, self.channel_password.clone()))
                {
                    self.channel_secret = Some((id, self.channel_password.clone()));
                    self.password_error = None;
                    self.password_channel = None;
                    self.channel_password.clear();
                }
            }
            Message::CancelChannelPassword => {
                self.password_channel = None;
                self.password_error = None;
                self.channel_password.clear();
            }
            Message::ToggleFooter => self.set_footer_hidden(!self.footer_hidden),
            Message::Updater(event) => return self.update_software(event),
            Message::Resume => return self.resume(),
            Message::Appearance(appearance) => self.set_appearance(appearance),
            Message::ToggleSubscribeAll => {
                self.subscribe_all.fetch_xor(true, Ordering::Relaxed);
            }
            Message::ToggleChannel(id) => {
                if !self.collapsed_channels.remove(&id) {
                    self.collapsed_channels.insert(id);
                }
            }
            Message::ToggleClient(id) => {
                if self.connected
                    && !self.loopback
                    && !self.stopping
                    && self
                        .state
                        .current_members()
                        .iter()
                        .any(|member| member.id == id)
                {
                    self.select_client((self.expanded_client != Some(id)).then_some(id));
                }
            }
            Message::Close => {
                self.streaming.sharing.stop_capture();
                if self.session.is_none() {
                    return Task::Exit;
                }
                self.closing = true;
                self.command(client::Command::Stop);
                self.stopping = true;
            }
            Message::Backend(event) => match event {
                client::Event::Status(status) => {
                    self.streaming.reset();
                    self.clear_client_details();
                    self.client_usage.clear();
                    self.talking.clear();
                    self.connection_history = ConnectionHistory::new();
                    self.password_channel = None;
                    self.channel_password.clear();
                    self.status = status;
                    self.connected = false;
                    self.network_usage.stop();
                    self.connection_usage = usage::ConnectionUsage::default();
                }
                client::Event::Connected(detail) => {
                    self.connected = true;
                    // A new session starts muted; restore the microphone as it was.
                    if let Some(resume) = self.resume.as_mut() {
                        let (muted, deafened) = (resume.muted, resume.deafened);
                        (resume.muted, resume.deafened) = (true, false);
                        if !self.loopback && !muted {
                            self.toggle_mute();
                        }
                        if !self.loopback && deafened {
                            self.toggle_deafen();
                        }
                    }
                    self.alerts.recover(Source::Connection);
                    self.alerts.recover(Source::Audio);
                    self.detail = detail;
                    self.status = if self.loopback {
                        "Local microphone test — only you can hear this"
                    } else {
                        "Connected — microphone starts muted"
                    }
                    .into();
                }
                client::Event::State(state) => {
                    let channel_changed = self.state.current.as_ref().map(|channel| channel.id)
                        != state.current.as_ref().map(|channel| channel.id);
                    if channel_changed {
                        self.client_usage.clear();
                    } else {
                        self.client_usage.retain(|id, _| {
                            state
                                .current_members()
                                .iter()
                                .any(|member| member.id == *id && !member.is_self)
                        });
                    }
                    if self.expanded_client.is_some_and(|id| {
                        channel_changed
                            || !state.current_members().iter().any(|member| member.id == id)
                    }) {
                        self.clear_client_details();
                    }
                    self.collapsed_channels
                        .retain(|id| state.channels.iter().any(|channel| channel.id == *id));
                    self.state = state;
                    if self.connected
                        && !self.state.channels.is_empty()
                        && let Some(resume) = self.resume.take()
                        && let Some(id) = resume.channel
                        && self.state.current.as_ref().map(|channel| channel.id) != Some(id)
                        && self.can_request_join(id)
                    {
                        self.alerts.begin(Source::Join);
                        if !resume.channel_password.is_empty() {
                            self.channel_secret = Some((id, resume.channel_password.clone()));
                        }
                        self.command(client::Command::Join(id, resume.channel_password));
                    }
                    if self
                        .password_channel
                        .is_some_and(|id| !self.can_request_join(id))
                    {
                        self.password_channel = None;
                        self.channel_password.clear();
                    }
                }
                client::Event::Talking(talking) => self.talking = talking,
                client::Event::Streaming(event) => {
                    match &event {
                        streaming::Event::State {
                            generation,
                            phase,
                            detail,
                            ..
                        } if *generation >= self.streaming.generation => match phase {
                            streaming::Phase::Failed => {
                                self.alerts.raise(Source::StreamWatch, detail)
                            }
                            streaming::Phase::Receiving => self.alerts.recover(Source::StreamWatch),
                            _ => {}
                        },
                        streaming::Event::Notice(detail) if !detail.is_empty() => {
                            self.alerts.raise(Source::StreamDiscovery, detail)
                        }
                        _ => {}
                    }
                    self.streaming.update(event);
                }
                client::Event::Meter {
                    level,
                    sent,
                    received,
                    audio_buffer_bytes,
                } => {
                    self.level = level;
                    self.sent = sent;
                    self.received = received;
                    self.audio_buffer_bytes = audio_buffer_bytes;
                }
                client::Event::Warning { source, detail } => self.alerts.raise(source, detail),
                client::Event::Recovered(source) => self.alerts.recover(source),
                client::Event::Controls { confirmed, pending } => {
                    self.server_controls = confirmed;
                    self.controls_pending = pending;
                }
                client::Event::ChannelPasswordRequired(id) => {
                    if self.can_request_join(id) {
                        self.password_channel = Some(id);
                        self.channel_password.clear();
                        self.password_error =
                            Some("The channel password was missing or incorrect.".into());
                        return Task::Focus(ui::Field::ChannelPassword);
                    }
                }
                client::Event::Usage {
                    network,
                    connection,
                } => {
                    self.network_usage = network;
                    self.connection_usage = connection;
                    if self.connected
                        && !self.loopback
                        && !self.performance_mode.load(Ordering::Relaxed)
                    {
                        self.connection_history.push(Instant::now(), connection);
                    }
                }
                client::Event::ClientUsage { id, result } => {
                    if self.connected
                        && self
                            .state
                            .current_members()
                            .iter()
                            .any(|member| member.id == id && !member.is_self)
                    {
                        if self.expanded_client == Some(id)
                            && let Ok(sample) = result
                        {
                            let now = Instant::now();
                            self.client_history.push(now, sample.sample(now));
                        }
                        self.client_usage.insert(id, result);
                    }
                }
                client::Event::Stopped(error) => {
                    self.streaming.reset();
                    self.clear_client_details();
                    self.client_usage.clear();
                    self.talking.clear();
                    self.connection_history = ConnectionHistory::new();
                    self.password_channel = None;
                    self.channel_password.clear();
                    self.session = None;
                    self.resume = None;
                    self.connected = false;
                    self.stopping = false;
                    self.level = 0.0;
                    self.audio_buffer_bytes = 0;
                    self.detail.clear();
                    self.status = "Disconnected".into();
                    self.network_usage.stop();
                    self.connection_usage = usage::ConnectionUsage::default();
                    self.server_controls = None;
                    self.controls_pending = false;
                    for source in [
                        Source::Controls,
                        Source::Subscription,
                        Source::Flood,
                        Source::Join,
                    ] {
                        self.alerts.recover(source);
                    }
                    if let Some((source, detail)) = error {
                        self.alerts.raise(source, detail);
                    }
                    if self.closing {
                        return Task::Exit;
                    }
                }
            },
        }
        Task::none()
    }

    // ---- view-independent helpers ----
    fn clear_client_details(&mut self) {
        self.expanded_client = None;
        self.client_history = ConnectionHistory::new();
    }

    fn select_client(&mut self, id: Option<u16>) {
        if id.is_some_and(|id| {
            !self
                .state
                .current_members()
                .iter()
                .any(|member| member.id == id)
        }) {
            return;
        }
        self.clear_client_details();
        self.expanded_client = id;
    }
    fn footer_connection_status(&self) -> (usage::Health, &'static str, String) {
        if self.connected && !self.loopback && !self.stopping && self.state.current.is_some() {
            if self.performance_mode.load(Ordering::Relaxed) {
                return (
                    usage::Health::Unknown,
                    "Unknown",
                    "Connection-health sampling is paused in performance mode.".into(),
                );
            }
            let health = self.connection_usage.health();
            let label = if health == usage::Health::Unknown {
                "Unknown"
            } else {
                health.label()
            };
            (health, label, self.connection_usage.health_tooltip())
        } else {
            let label = if self.stopping {
                "Stopping"
            } else if self.loopback && self.connected {
                "Local test"
            } else if self.session.is_some() {
                "Connecting"
            } else {
                "Offline"
            };
            (usage::Health::Unknown, label, self.status.clone())
        }
    }
    fn microphone_status(&self) -> &'static str {
        if !self.connected || self.stopping {
            "Inactive"
        } else if self.loopback {
            "Local test only"
        } else if self.deafened {
            "Muted (deafened)"
        } else if self.muted {
            "Muted"
        } else if !self.state.can_talk {
            "Blocked by server"
        } else {
            "Unmuted"
        }
    }

    fn microphone_icon(&self) -> Icon {
        if self.connected
            && !self.stopping
            && (self.loopback || (!self.muted && !self.deafened && self.state.can_talk))
        {
            Icon::Mic
        } else {
            Icon::MicOff
        }
    }

    fn playback_icon(&self) -> Icon {
        if self.deafened {
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
        self.connected
            && !self.loopback
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
    /// Whether the member is sharing a stream; the catalog only covers your current channel.
    fn is_streaming(&self, member: &client::Member) -> bool {
        (member.is_self && self.streaming.sharing.status.live)
            || self
                .streaming
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
        } else if let Some(Err(error)) = self.client_usage.get(&member.id) {
            (usage::Health::Unknown, (*error).into())
        } else {
            let quality = self.client_quality(member.id, Instant::now());
            (quality.health(), quality.health_tooltip())
        }
    }
    fn client_quality(&self, id: u16, now: Instant) -> usage::ConnectionUsage {
        self.client_usage
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
        if let Some(sender) = &self.session {
            let _ = sender.try_send(client::Command::Stop);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn first_run_only_saves_after_five_steps_and_restores_devices_without_connecting() {
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
        app.session = Some(sender);
        app.loopback = true;
        let _ = app.update(Message::SetupNext);
        assert_eq!(app.onboarding, Some(onboarding::Step::Devices));
        app.session = None;
        app.loopback = false;
        let _ = app.update(Message::SetupNext);
        assert_eq!(app.onboarding, Some(onboarding::Step::Hotkeys));
        assert!(!path.exists());
        let _ = app.update(Message::MuteKeybind("F8".into()));
        let _ = app.update(Message::DeafenKeybind("F9".into()));
        let _ = app.update(Message::SetupNext);
        assert!(app.onboarding.is_none() && app.session.is_none());
        assert_eq!(app.password, "temporary-test-password");
        let saved = settings::load(&path).unwrap().unwrap();
        assert_eq!(saved.nickname, "Alex");
        assert_eq!(saved.server_address, "voice.example:9987");
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
        assert_eq!(app.tab, Tab::Settings);
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
        app.nickname = "Unsaved draft".into();
        let _ = app.update(Message::ToggleFooter);
        assert!(app.footer_hidden);
        let stored = settings::load(&path).unwrap().unwrap();
        assert!(stored.footer_hidden);
        assert_eq!(stored.nickname, "Saved name", "drafts stay unsaved");
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
        app.nickname = "Unsaved draft".into();
        let _ = app.update(Message::Updater(updater::Event::AutoDownload(false)));
        assert!(!app.updater.auto_download);
        let stored = settings::load(&path).unwrap().unwrap();
        assert!(!stored.auto_download_updates);
        assert_eq!(stored.nickname, "Flexspeak", "drafts stay unsaved");
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
        app.nickname = "Unsaved draft".into();
        let chosen = settings::Appearance {
            scheme: settings::ColorScheme::Light,
            light_theme: settings::ThemeName::Ocean,
            dark_theme: settings::ThemeName::Ember,
        };
        let _ = app.update(Message::Appearance(chosen));
        let stored = settings::load(&path).unwrap().unwrap();
        assert_eq!(stored.appearance, chosen);
        assert_eq!(stored.nickname, settings::Settings::default().nickname);
        let (mut restarted, _) = setup_fixture("appearance-restart");
        restarted.load_settings(path);
        assert_eq!(restarted.appearance, chosen);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_finished_update_install_records_the_session_then_closes() {
        let (mut app, root) = setup_fixture("resume-write");
        let path = root.join("settings.json");
        settings::save(&path, &settings::Settings::default()).unwrap();
        app.load_settings(path.clone());
        let (sender, _commands) = mpsc::channel(8);
        app.session = Some(sender);
        app.connected = true;
        app.muted = false;
        app.password = "server secret".into();
        app.state.current = Some(client::Channel {
            id: 9,
            ..client::Channel::default()
        });
        app.channel_secret = Some((9, "channel secret".into()));
        app.tab = Tab::Statistics;
        app.updater = updater::Updater::new(
            updater::Build::parse_tag("v0.1.0-build.6"),
            Ok(updater::Package::MacApp),
            true,
        );
        app.updater.phase = updater::Phase::Installing;
        let _ = app.update(Message::Updater(updater::Event::Installed(Ok(()))));
        assert!(app.closing && app.stopping, "the session ends before exit");
        let saved = resume::take(&resume::path(&path), SystemTime::now()).unwrap();
        assert_eq!(saved.server_address.as_deref(), Some("ts.zirkumflex.io"));
        assert_eq!(saved.server_password, "server secret");
        assert_eq!(saved.channel, Some(9));
        assert_eq!(saved.channel_password, "channel secret");
        assert_eq!((saved.muted, saved.deafened), (false, false));
        assert_eq!(saved.page, resume::Page::Statistics);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn after_an_update_the_app_rejoins_its_channel_with_the_same_microphone() {
        let mut app = App::with_devices(audio::Devices::default(), None);
        app.saved = Some(settings::Settings::default());
        app.resume = Some(resume::Resume {
            page: resume::Page::Settings,
            server_address: Some("ts.zirkumflex.io".into()),
            server_password: "server secret".into(),
            channel: Some(4),
            channel_password: "channel secret".into(),
            muted: false,
            deafened: true,
            ..resume::Resume::default()
        });
        // Without devices the reconnect can't start, so the resume ends there, on its page.
        let _ = app.update(Message::Resume);
        assert_eq!(app.tab, Tab::Settings);
        assert_eq!(app.password, "server secret");
        assert!(app.resume.is_none());

        // With a session under way: controls on connect, then one join once channels arrive.
        app.resume = Some(resume::Resume {
            channel: Some(4),
            channel_password: "channel secret".into(),
            muted: false,
            deafened: true,
            ..resume::Resume::default()
        });
        let (sender, mut commands) = mpsc::channel(8);
        app.session = Some(sender);
        let _ = app.update(Message::Backend(client::Event::Connected(String::new())));
        assert!(!app.muted && app.deafened);
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
        let _ = app.update(Message::Backend(client::Event::State(client::Snapshot {
            channels: vec![lobby.clone(), room],
            current: Some(lobby),
            ..client::Snapshot::default()
        })));
        match commands.try_recv() {
            Ok(client::Command::Join(4, password)) => assert_eq!(password, "channel secret"),
            other => panic!("expected a join, got {other:?}"),
        }
        assert!(app.resume.is_none(), "only once");
        assert_eq!(app.channel_secret, Some((4, "channel secret".into())));
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
        assert!(app.streaming.visible.load(Ordering::Relaxed));
        assert!(app.channel_health_visible.load(Ordering::Relaxed));
        let _ = app.update(Message::ShowTab(Tab::Statistics));
        assert!(!app.streaming.visible.load(Ordering::Relaxed));
        assert!(app.channel_health_visible.load(Ordering::Relaxed));
        let _ = app.update(Message::ShowTab(Tab::Settings));
        assert!(!app.channel_health_visible.load(Ordering::Relaxed));
        app.begin_setup();
        let _ = app.update(Message::ShowTab(Tab::Home));
        assert!(!app.streaming.visible.load(Ordering::Relaxed));
        assert!(!app.channel_health_visible.load(Ordering::Relaxed));
    }

    #[test]
    fn modal_dismissal_retry_and_recovery_follow_the_specific_operation() {
        let mut app = App::with_devices(audio::Devices::default(), None);
        let warning = || {
            Message::Backend(client::Event::Warning {
                source: Source::Flood,
                detail: "Cooldown".into(),
            })
        };
        let _ = app.update(warning());
        let _ = app.update(warning());
        assert_eq!(app.alerts.current().unwrap().reports, 2);
        let _ = app.update(Message::Backend(client::Event::Recovered(Source::Controls)));
        assert_eq!(app.alerts.current().unwrap().source, Source::Flood);
        let _ = app.update(Message::DismissError);
        let _ = app.update(warning());
        assert!(app.alerts.current().is_none());
        let _ = app.update(Message::Backend(client::Event::Recovered(Source::Flood)));
        let _ = app.update(warning());
        assert_eq!(app.alerts.current().unwrap().reports, 1);
        let _ = app.update(Message::DismissError);
        app.keybind_draft.toggle_mute.clear();
        let _ = app.update(Message::SaveSettings);
        assert_eq!(app.alerts.current().unwrap().source, Source::SettingsSave);
        let _ = app.update(Message::DismissError);
        let _ = app.update(Message::SaveSettings);
        assert_eq!(app.alerts.current().unwrap().source, Source::SettingsSave);
    }

    #[test]
    fn footer_health_only_reports_live_channel_metrics_and_marks_paused_sampling() {
        let mut app = App::with_devices(audio::Devices::default(), None);
        app.state.current = Some(client::Channel::default());
        app.connection_usage = usage::ConnectionUsage {
            packet_loss_in: Some(6.0),
            packet_loss_out: Some(0.0),
            ping_ms: Some(23.0),
            ping_deviation_ms: Some(5.0),
            ..Default::default()
        };
        assert_eq!(app.footer_connection_status().1, "Offline");
        app.connected = true;
        let (health, label, details) = app.footer_connection_status();
        assert_eq!(health, usage::Health::Poor);
        assert_eq!(label, "Poor");
        assert_eq!(details, "Incoming packet loss: 6.0%");
        app.performance_mode.store(true, Ordering::Relaxed);
        assert_eq!(app.footer_connection_status().0, usage::Health::Unknown);
        assert!(app.footer_connection_status().2.contains("paused"));
        app.performance_mode.store(false, Ordering::Relaxed);
        app.loopback = true;
        assert_eq!(app.footer_connection_status().1, "Local test");
        app.stopping = true;
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
                        name: format!("Client {id}"),
                        is_self: false,
                        mic_muted: true,
                        sound_muted: false,
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
        app.connected = true;
        app.state = health_test_state();
        let now = Instant::now();
        let mut member = app.state.current.as_ref().unwrap().members[0].clone();
        app.client_usage
            .insert(member.id, Ok(peer_sample(0.02, now)));
        let (health, detail) = app.member_health(&member, true);
        assert_eq!(health, usage::Health::Fair);
        assert_eq!(detail, "Incoming packet loss: 2.0%");
        let (health, detail) = app.member_health(&member, false);
        assert_eq!(health, usage::Health::Unknown);
        assert!(detail.contains("current channel"));
        app.client_usage.insert(member.id, Err("Permission denied"));
        assert_eq!(
            app.member_health(&member, true),
            (usage::Health::Unknown, "Permission denied".into())
        );
        app.client_usage.insert(
            member.id,
            Ok(peer_sample(0.0, now - std::time::Duration::from_secs(16))),
        );
        assert_eq!(app.member_health(&member, true).0, usage::Health::Unknown);
        member.is_self = true;
        app.connection_usage = peer_sample(0.06, now).sample(now);
        let (health, _, detail) = app.footer_connection_status();
        assert_eq!(app.member_health(&member, true), (health, detail));
    }

    #[test]
    fn collapsed_health_updates_without_expansion_and_expires_without_false_good() {
        let mut app = App::with_devices(audio::Devices::default(), None);
        app.connected = true;
        app.state = health_test_state();
        let now = Instant::now();
        assert_eq!(app.client_quality(11, now).health(), usage::Health::Unknown);
        for (loss, health) in [
            (0.0, usage::Health::Good),
            (0.02, usage::Health::Fair),
            (0.06, usage::Health::Poor),
        ] {
            let _ = app.update(Message::Backend(client::Event::ClientUsage {
                id: 11,
                result: Ok(peer_sample(loss, now)),
            }));
            assert_eq!(app.expanded_client, None);
            assert_eq!(app.client_quality(11, now).health(), health);
        }
        let _ = app.update(Message::ToggleClient(11));
        assert_eq!(app.expanded_client, Some(11));
        let _ = app.update(Message::ToggleClient(11));
        assert_eq!(app.expanded_client, None);
        assert_eq!(app.client_quality(11, now).health(), usage::Health::Poor);
        assert_eq!(
            app.client_quality(11, now + std::time::Duration::from_secs(16))
                .health(),
            usage::Health::Unknown
        );
        let _ = app.update(Message::Backend(client::Event::ClientUsage {
            id: 11,
            result: Err("Denied"),
        }));
        assert_eq!(app.client_quality(11, now).health(), usage::Health::Unknown);
        let _ = app.update(Message::ShowTab(Tab::Settings));
        assert!(!app.channel_health_visible.load(Ordering::Relaxed));
        let _ = app.update(Message::ShowTab(Tab::Home));
        assert!(app.channel_health_visible.load(Ordering::Relaxed));
    }

    #[test]
    fn client_health_cache_is_scoped_to_current_members_and_clears_on_channel_moves() {
        let mut app = App::with_devices(audio::Devices::default(), None);
        app.connected = true;
        let mut state = health_test_state();
        app.state = state.clone();
        let now = Instant::now();
        for id in [12, 99] {
            let _ = app.update(Message::ToggleClient(id));
            let _ = app.update(Message::Backend(client::Event::ClientUsage {
                id,
                result: Ok(peer_sample(0.0, now)),
            }));
        }
        assert!(app.client_usage.is_empty());
        assert_eq!(app.expanded_client, None);
        let _ = app.update(Message::Backend(client::Event::ClientUsage {
            id: 11,
            result: Ok(peer_sample(0.0, now)),
        }));
        let _ = app.update(Message::ToggleClient(11));
        let peer = state.channels[0].members.pop().unwrap();
        state.channels[1].members.push(peer);
        state.current = Some(state.channels[0].clone());
        let _ = app.update(Message::Backend(client::Event::State(state.clone())));
        assert!(app.client_usage.is_empty());
        assert_eq!(app.expanded_client, None);
        state.current = Some(state.channels[1].clone());
        let _ = app.update(Message::Backend(client::Event::State(state.clone())));
        let _ = app.update(Message::ToggleClient(12));
        let _ = app.update(Message::Backend(client::Event::ClientUsage {
            id: 12,
            result: Ok(peer_sample(0.0, now)),
        }));
        assert_eq!(app.expanded_client, Some(12));
        state.current = Some(state.channels[0].clone());
        let _ = app.update(Message::Backend(client::Event::State(state)));
        assert!(app.client_usage.is_empty());
        assert_eq!(app.expanded_client, None);
        let _ = app.update(Message::Backend(client::Event::ClientUsage {
            id: 12,
            result: Err("Late response"),
        }));
        assert!(app.client_usage.is_empty());
    }

    #[test]
    fn joining_blocks_known_denials_and_lets_the_server_check_unknown_access() {
        let mut app = App::with_devices(audio::Devices::default(), None);
        let (sender, mut commands) = mpsc::channel(16);
        app.session = Some(sender);
        app.connected = true;
        app.channel_password = "channel secret".into();
        app.state.channels = [
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
        app.state.current = Some(app.state.channels[0].clone());
        let _ = app.update(Message::Join(1));
        assert!(commands.try_recv().is_err());
        app.state.current = None;
        app.stopping = true;
        let _ = app.update(Message::Join(1));
        assert!(commands.try_recv().is_err());
        app.stopping = false;
        app.connected = false;
        let _ = app.update(Message::Join(1));
        assert!(commands.try_recv().is_err());
    }

    #[test]
    fn channel_password_is_prompted_on_join_and_cleared_after_submit_or_cancel() {
        let mut app = App::with_devices(audio::Devices::default(), None);
        let (sender, mut commands) = mpsc::channel(16);
        app.session = Some(sender);
        app.connected = true;
        app.state.channels = vec![client::Channel {
            id: 2,
            has_password: true,
            join_access: client::JoinAccess::Unknown,
            ..client::Channel::default()
        }];
        let _ = app.update(Message::Join(2));
        assert_eq!(app.password_channel, Some(2));
        assert!(commands.try_recv().is_err());
        let _ = app.update(Message::ChannelPassword("secret".into()));
        let _ = app.update(Message::SubmitChannelPassword);
        assert!(
            matches!(commands.try_recv(), Ok(client::Command::Join(2, password)) if password == "secret")
        );
        assert!(app.password_channel.is_none() && app.channel_password.is_empty());
        let _ = app.update(Message::Backend(client::Event::ChannelPasswordRequired(2)));
        assert_eq!(app.password_channel, Some(2));
        assert!(app.password_error.is_some());
        let _ = app.update(Message::ChannelPassword("discard me".into()));
        let _ = app.update(Message::CancelChannelPassword);
        assert!(app.password_channel.is_none() && app.channel_password.is_empty());
        assert!(commands.try_recv().is_err());
    }

    #[test]
    fn mute_and_deafen_toggles_preserve_state_and_ignore_inactive_sessions() {
        let mut app = App::with_devices(audio::Devices::default(), None);
        let (sender, mut commands) = mpsc::channel(16);
        app.session = Some(sender);
        let _ = app.update(Message::Shortcut(shortcuts::Action::Mute));
        assert_eq!(app.last_shortcut, Some(shortcuts::Action::Mute));
        assert!(commands.try_recv().is_err());
        app.connected = true;
        let _ = app.update(Message::Shortcut(shortcuts::Action::Deafen));
        assert!(app.deafened && app.muted);
        assert!(matches!(
            commands.try_recv(),
            Ok(client::Command::Deafen(true))
        ));
        let _ = app.update(Message::Mute);
        assert!(!app.muted && app.deafened);
        assert_eq!(app.microphone_status(), "Muted (deafened)");
        assert!(matches!(
            commands.try_recv(),
            Ok(client::Command::Mute(false))
        ));
        let _ = app.update(Message::Mute);
        let _ = app.update(Message::Deafen);
        assert!(app.muted && !app.deafened);
        assert!(matches!(
            commands.try_recv(),
            Ok(client::Command::Mute(true))
        ));
        assert!(matches!(
            commands.try_recv(),
            Ok(client::Command::Deafen(false))
        ));
        app.stopping = true;
        let _ = app.update(Message::Mute);
        let _ = app.update(Message::Deafen);
        assert!(app.muted && !app.deafened);
        assert!(commands.try_recv().is_err());
    }

    #[test]
    fn failed_control_delivery_does_not_report_a_false_mute_state() {
        let mut app = App::with_devices(audio::Devices::default(), None);
        let (sender, _commands) = mpsc::channel(1);
        sender.try_send(client::Command::Mute(true)).unwrap();
        app.session = Some(sender);
        app.connected = true;
        let _ = app.update(Message::Mute);
        let _ = app.update(Message::Deafen);
        assert!(app.muted && !app.deafened);
        assert_eq!(app.alerts.current().unwrap().source, Source::Worker);
    }

    #[test]
    fn volume_and_portable_settings_do_not_change_microphone_state() {
        let mut app = App::with_devices(audio::Devices::default(), None);
        app.connected = true;
        app.muted = false;
        let worker_volume = app.playback_volume.clone();
        let _ = app.update(Message::PlaybackVolume(0));
        assert_eq!(worker_volume.percent(), 0);
        assert!(!app.muted && !app.deafened);
        assert_eq!(app.edited_settings().playback_volume_percent, 0);
        app.connected = false;
        app.password = "old-server-password".into();
        app.channel_password = "old-channel-password".into();
        app.apply_settings(settings::Settings {
            server_address: "voice.example:9987".into(),
            nickname: "Portable name".into(),
            playback_volume_percent: 42,
            ..settings::Settings::default()
        });
        assert_eq!(app.address, "voice.example:9987");
        assert_eq!(app.nickname, "Portable name");
        assert_eq!(worker_volume.percent(), 42);
        assert!(app.password.is_empty() && app.channel_password.is_empty());
    }

    #[test]
    fn performance_mode_samples_slowly_without_history_or_voice_changes() {
        let mut app = App::with_devices(audio::Devices::default(), None);
        let worker_flag = app.performance_mode.clone();
        let (sender, mut commands) = mpsc::channel(16);
        app.session = Some(sender);
        app.connected = true;
        app.muted = false;
        app.state.can_talk = true;
        app.state.current = Some(client::Channel {
            id: 1,
            name: "Lobby".into(),
            ..client::Channel::default()
        });
        app.tab = Tab::Statistics;
        let samples = app.ram_history.len();

        let _ = app.update(Message::PerformanceMode(true));
        let _ = app.update(Message::UsageTick); // A timer tick may already be queued.
        let _ = app.update(Message::RecordTrace);
        assert!(worker_flag.load(Ordering::Relaxed));
        assert!(app.process_usage.cpu_percent.is_none());
        assert_eq!(app.ram_history.len(), samples);
        assert!(app.recording.is_none());
        assert!(app.connected);
        assert_eq!(app.microphone_status(), "Unmuted");
        assert_eq!(app.state.current.as_ref().unwrap().name, "Lobby");
        assert!(commands.try_recv().is_err()); // No stop/mute command on a UI switch.
        app.process_usage.memory_bytes = None;
        app.process_usage.memory_detail_bytes = None;
        let _ = app.update(Message::CompactUsageTick);
        assert!(app.process_usage.memory_bytes.is_some());
        #[cfg(target_os = "macos")]
        assert!(
            app.process_usage
                .memory_detail_bytes
                .is_some_and(|bytes| bytes > 0)
        );
        assert_eq!(app.ram_history.len(), samples);
        assert!(app.cpu_history.len() == 0);

        let _ = app.update(Message::Backend(client::Event::State(client::Snapshot {
            current: Some(client::Channel {
                id: 2,
                name: "Moved by server".into(),
                ..client::Channel::default()
            }),
            ..client::Snapshot::default()
        })));
        assert_eq!(app.state.current.as_ref().unwrap().id, 2);
        assert_eq!(app.microphone_status(), "Blocked by server");

        let _ = app.update(Message::PerformanceMode(false));
        assert!(!worker_flag.load(Ordering::Relaxed));
        assert_eq!(app.tab, Tab::Statistics);
        assert!(app.process_usage.cpu_percent.is_none());
        let _ = app.update(Message::CompactUsageTick); // Ignore a queued slow tick after exit.
        assert!(app.process_usage.cpu_percent.is_none());
        let _ = app.update(Message::UsageTick);
        assert_eq!(app.ram_history.len(), samples + 1);
        assert!(commands.try_recv().is_err());
    }

    #[test]
    fn microphone_status_and_meter_reflect_effective_capture_state() {
        let mut app = App::with_devices(audio::Devices::default(), None);
        app.level = 0.25; // A queued meter event must not show activity after capture stops.
        assert_eq!(app.microphone_status(), "Inactive");
        assert_eq!(app.microphone_icon(), Icon::MicOff);
        assert_eq!(app.microphone_meter_level(), 0.0);
        assert_eq!(app.playback_icon(), Icon::Headphones);
        app.connected = true;
        assert_eq!(app.microphone_status(), "Muted");
        assert_eq!(app.microphone_icon(), Icon::MicOff);
        assert_eq!(app.microphone_meter_level(), 0.0);
        app.muted = false;
        assert_eq!(app.microphone_status(), "Blocked by server");
        assert_eq!(app.microphone_icon(), Icon::MicOff);
        assert_eq!(app.microphone_meter_level(), 0.0);
        app.state.can_talk = true;
        assert_eq!(app.microphone_status(), "Unmuted");
        assert_eq!(app.microphone_icon(), Icon::Mic);
        assert_eq!(app.microphone_meter_level(), 0.5);
        app.deafened = true;
        assert_eq!(app.microphone_status(), "Muted (deafened)");
        assert_eq!(app.microphone_icon(), Icon::MicOff);
        assert_eq!(app.microphone_meter_level(), 0.0);
        assert_eq!(app.playback_icon(), Icon::HeadphoneOff);
        app.loopback = true;
        assert_eq!(app.microphone_status(), "Local test only");
        assert_eq!(app.microphone_icon(), Icon::Mic);
        assert_eq!(app.microphone_meter_level(), 0.5);
        app.stopping = true;
        assert_eq!(app.microphone_status(), "Inactive");
        assert_eq!(app.microphone_icon(), Icon::MicOff);
        assert_eq!(app.microphone_meter_level(), 0.0);
        let _ = app.update(Message::Backend(client::Event::Stopped(None)));
        assert_eq!(app.microphone_status(), "Inactive");
        assert_eq!(app.microphone_meter_level(), 0.0);
    }

    #[test]
    fn performance_mode_restores_the_previous_window_size_once() {
        let mut app = App::with_devices(audio::Devices::default(), None);
        app.window_size = (1200.0, 900.0);
        assert!(matches!(
            app.update(Message::PerformanceMode(true)),
            Task::Batch(tasks) if matches!(tasks.last(), Some(Task::Resize(COMPACT_MIN_SIZE)))
        ));
        app.window_size = COMPACT_MIN_SIZE;
        assert!(matches!(
            app.update(Message::PerformanceMode(false)),
            Task::Resize((1200.0, 900.0))
        ));
        assert!(app.window_restore.is_none());
        app.window_size = (500.0, 400.0);
        let _ = app.update(Message::PerformanceMode(true));
        assert!(matches!(
            app.update(Message::PerformanceMode(false)),
            Task::Resize(NORMAL_MIN_SIZE)
        ));
    }
}
