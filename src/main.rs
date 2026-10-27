#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod alerts;
mod appearance;
mod audio;
mod client;
mod command_flow;
mod connection_charts;
mod icons;
mod memory;
mod modal;
mod profiler;
mod settings;
mod shortcuts;
mod statistics_charts;
mod streaming;
mod usage;

use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use iced::{
    Element, Length, Size, Subscription, Task,
    widget::{
        canvas, checkbox, column, container, opaque, pick_list, progress_bar, row, rule, slider,
        stack, text, text_input, tooltip,
    },
    window,
};
use tokio::sync::mpsc;
use tokio_stream::wrappers::{ReceiverStream, UnboundedReceiverStream};

use alerts::{Alerts, Source};
use appearance::{
    compact_button, icon_button, label_button, primary_button as button, scroll_area as scrollable,
};
use connection_charts::{ConnectionChart, ConnectionHistory, Metric, health_color};
use icons::Icon;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "teamspeak_native=info,tsclientlib=warn,tsproto=warn".into()),
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
                "Native Voice\n\nNo arguments       Open the native client\n--list-devices     List audio inputs and outputs\n--probe ADDRESS    Connect muted, report server state, disconnect\n--identity-path    Print the local identity file path\n--settings-path    Print the portable settings JSON path"
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
        Some(other) => anyhow::bail!("Unknown argument '{other}'. Use --help."),
        None => {}
    }
    iced::application(App::new, App::update, App::view)
        // Avoid full-viewport 4x MSAA targets retained after a window shrinks.
        // Text and widgets retain their own smoothing; canvas edges may alias.
        .antialiasing(false)
        .title("Flexspeak")
        .theme(appearance::theme())
        .font(icons::FONT_BYTES)
        .window_size((900.0, 790.0))
        .centered()
        .exit_on_close_request(false)
        .subscription(App::subscription)
        .run()?;
    Ok(())
}

struct App {
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
    performance_request: u64,
    window_restore: Option<WindowLayout>,
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
}

#[derive(Debug, Clone, Copy)]
struct WindowLayout {
    // shortcut: retain current bounds; track pre-maximize bounds for exact unzoom restoration.
    size: Size,
    maximized: bool,
    mode: window::Mode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Voice,
    Streaming,
    Statistics,
    Settings,
}

#[derive(Debug, Clone)]
enum Message {
    DismissError,
    Shortcut(shortcuts::Action),
    MuteKeybind(String),
    DeafenKeybind(String),
    SaveSettings,
    ReloadSettings,
    CopySettingsPath,
    PlaybackVolume(u8),
    PerformanceMode(bool),
    PerformanceWindow(u64, window::Id, WindowLayout),
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
    ToggleClient(u16),
    Backend(client::Event),
    Close,
    UsageTick,
    CompactUsageTick,
    ShowTab(Tab),
    RefreshStreams,
    WatchStream(String),
    StopWatching,
    VideoTick,
    VideoFrameReady(
        streaming::Frame,
        Result<iced_runtime::image::Allocation, iced_runtime::image::Error>,
    ),
    RecordTrace,
    StopTrace,
    TraceSaved(Result<PathBuf, String>),
    CopyTracePath,
}

impl App {
    fn new() -> (Self, Task<Message>) {
        let (devices, error) = match audio::devices() {
            Ok(devices) => (devices, None),
            Err(error) => (
                audio::Devices::default(),
                Some(format!("Audio device detection failed: {error:#}")),
            ),
        };
        let mut app = Self::with_devices(devices, error);
        match settings::path() {
            Ok(path) => {
                match settings::load_or_create(&path) {
                    Ok(saved) => app.apply_settings(saved),
                    Err(error) => {
                        app.alerts.raise(Source::SettingsLoad, format!(
                            "{error:#}. Defaults are shown; fix the file and Reload, or Save to replace it with the displayed settings."
                        ))
                    }
                }
                app.settings_path = Some(path);
            }
            Err(error) => app.alerts.raise(Source::SettingsLoad, format!("{error:#}")),
        }
        let (mut shortcuts, events) = shortcuts::Shortcuts::new();
        app.shortcut_error = shortcuts
            .configure(&app.keybinds)
            .err()
            .map(|error| format!("{error:#}"));
        if shortcuts::supported()
            && let Some(error) = &app.shortcut_error
        {
            app.alerts.raise(Source::Shortcuts, error);
        }
        app.shortcuts = Some(shortcuts);
        let task = Task::run(UnboundedReceiverStream::new(events), Message::Shortcut);
        (app, task)
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
            performance_request: 0,
            window_restore: None,
            input: devices.input.clone(),
            output: devices.output.clone(),
            devices,
            address: "ts.zirkumflex.io".into(),
            nickname: "Native Voice".into(),
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
            client_usage: BTreeMap::new(),
            connection_history: ConnectionHistory::new(),
            client_history: ConnectionHistory::new(),
            process_monitor,
            process_usage,
            cpu_history,
            ram_history,
            network_usage: usage::NetworkUsage::default(),
            connection_usage: usage::ConnectionUsage::default(),
            tab: Tab::Voice,
            streaming: streaming::View::default(),
            heap_usage: memory::snapshot(),
            audio_buffer_bytes: 0,
            recording: None,
            trace_summary: None,
            trace_path: None,
            trace_saving: false,
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            iced::window::close_requests().map(|_| Message::Close),
            if self.password_channel.is_some() {
                iced::event::listen_with(|event, _, _| {
                    modal::escape_released(&event).then_some(Message::CancelChannelPassword)
                })
            } else if self.alerts.current().is_some() {
                iced::event::listen_with(|event, _, _| {
                    modal::dismiss_released(&event).then_some(Message::DismissError)
                })
            } else {
                Subscription::none()
            },
            if self.performance_mode.load(Ordering::Relaxed) {
                iced::time::every(usage::COMPACT_REFRESH_INTERVAL)
                    .map(|_| Message::CompactUsageTick)
            } else {
                iced::time::every(usage::REFRESH_INTERVAL).map(|_| Message::UsageTick)
            },
            if self.tab == Tab::Streaming
                && self.streaming.active()
                && !self.performance_mode.load(Ordering::Relaxed)
            {
                iced::time::every(std::time::Duration::from_millis(33)).map(|_| Message::VideoTick)
            } else {
                Subscription::none()
            },
        ])
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

    fn apply_settings(&mut self, saved: settings::Settings) {
        if self.address != saved.server_address {
            self.password.clear();
            self.channel_password.clear();
        }
        self.address = saved.server_address;
        self.nickname = saved.nickname;
        self.playback_volume
            .set_percent(saved.playback_volume_percent);
        self.keybinds = saved.keybinds.clone();
        self.keybind_draft = saved.keybinds;
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
        }
    }

    fn configure_shortcuts(&mut self, keys: &shortcuts::Keybinds) -> anyhow::Result<()> {
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

    fn start(&mut self, loopback: bool) -> Task<Message> {
        if self.session.is_some() {
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
                Task::run(ReceiverStream::new(events), Message::Backend)
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

    fn update(&mut self, message: Message) -> Task<Message> {
        let _trace = profiler::scope(profiler::Category::Ui, "ui.update");
        match message {
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
                self.settings_message = None;
                self.alerts.begin(Source::SettingsSave);
                self.alerts.begin(Source::Shortcuts);
                let saved = self.edited_settings();
                if let Err(error) = saved.validate() {
                    self.alerts
                        .raise(Source::SettingsSave, format!("{error:#}"));
                    return Task::none();
                }
                if let Err(error) = self.configure_shortcuts(&saved.keybinds) {
                    self.alerts.raise(Source::Shortcuts, format!("{error:#}"));
                    return Task::none();
                }
                self.apply_settings(saved.clone());
                let result = self
                    .settings_path
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("Settings path is unavailable"))
                    .and_then(|path| settings::save(path, &saved));
                match result {
                    Ok(()) => {
                        self.alerts.recover(Source::SettingsSave);
                        self.alerts.recover(Source::SettingsLoad);
                        self.settings_message = Some("Saved settings.json".into());
                    }
                    Err(error) => self.alerts.raise(
                        Source::SettingsSave,
                        format!("Applied for this session, but not saved: {error:#}"),
                    ),
                }
            }
            Message::ReloadSettings => {
                if self.session.is_some() {
                    return Task::none();
                }
                self.settings_message = None;
                self.alerts.begin(Source::SettingsLoad);
                self.alerts.begin(Source::Shortcuts);
                let result = self
                    .settings_path
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("Settings path is unavailable"))
                    .and_then(settings::load_or_create);
                match result {
                    Ok(saved) => match self.configure_shortcuts(&saved.keybinds) {
                        Ok(()) => {
                            self.apply_settings(saved);
                            self.alerts.recover(Source::SettingsLoad);
                            self.settings_message = Some("Reloaded settings.json".into());
                        }
                        Err(error) => self.alerts.raise(Source::Shortcuts, format!("{error:#}")),
                    },
                    Err(error) => self
                        .alerts
                        .raise(Source::SettingsLoad, format!("{error:#}")),
                }
            }
            Message::CopySettingsPath => {
                if let Some(path) = &self.settings_path {
                    return iced::clipboard::write(path.display().to_string());
                }
            }
            Message::PerformanceMode(enabled) => {
                if self.performance_mode.swap(enabled, Ordering::Relaxed) == enabled {
                    return Task::none();
                }
                self.performance_request = self.performance_request.wrapping_add(1);
                if enabled && self.expanded_client.is_some() {
                    self.select_client(None);
                }
                if enabled {
                    self.streaming.clear_picture();
                    self.client_usage.clear();
                    self.connection_history = ConnectionHistory::new();
                    self.client_history = ConnectionHistory::new();
                    self.connection_usage = usage::ConnectionUsage::default();
                }
                let request = self.performance_request;
                // Prime CPU timing for the new sampling interval in either mode.
                self.process_monitor = usage::ProcessMonitor::new();
                self.process_usage = self.process_monitor.sample();
                let trace = if enabled {
                    self.cpu_history.release_graphics();
                    self.ram_history.release_graphics();
                    self.stop_trace()
                } else {
                    self.cpu_history.push(Instant::now(), None);
                    self.ram_history.push(Instant::now(), None);
                    Task::none()
                };
                return Task::batch([
                    trace,
                    window::latest().and_then(move |id| {
                        window::size(id).then(move |size| {
                            window::is_maximized(id).then(move |maximized| {
                                window::mode(id).map(move |mode| {
                                    Message::PerformanceWindow(
                                        request,
                                        id,
                                        WindowLayout {
                                            size,
                                            maximized,
                                            mode,
                                        },
                                    )
                                })
                            })
                        })
                    }),
                ]);
            }
            Message::PerformanceWindow(request, id, layout) => {
                // A newer switch action supersedes an outstanding window query.
                if self.performance_request != request {
                    return Task::none();
                }
                if self.performance_mode.load(Ordering::Relaxed) {
                    self.window_restore.get_or_insert(layout);
                    return window::set_mode(id, window::Mode::Windowed)
                        .chain(window::maximize(id, false))
                        .chain(window::resize(id, Size::new(440.0, 320.0)));
                }
                if let Some(layout) = self.window_restore.take() {
                    return window::resize(id, layout.size)
                        .chain(window::maximize(id, layout.maximized))
                        .chain(window::set_mode(id, layout.mode));
                }
            }
            Message::ShowTab(tab) => {
                if tab != Tab::Voice && self.expanded_client.is_some() {
                    self.select_client(None);
                }
                self.tab = tab;
                self.streaming
                    .visible
                    .store(tab == Tab::Streaming, Ordering::Relaxed);
                if tab != Tab::Streaming {
                    self.streaming.clear_picture();
                }
                self.channel_health_visible
                    .store(tab == Tab::Voice, Ordering::Relaxed);
            }
            Message::VideoTick => {
                if let Some(frame) = self.streaming.take_frame() {
                    return iced_runtime::image::allocate(frame.handle.clone())
                        .map(move |result| Message::VideoFrameReady(frame.clone(), result));
                }
            }
            Message::VideoFrameReady(frame, result) => {
                let detail = result.as_ref().err().map(ToString::to_string);
                match self.streaming.frame_uploaded(frame, result) {
                    Some(true) => self.alerts.recover(Source::StreamFrame),
                    Some(false) => self.alerts.raise(
                        Source::StreamFrame,
                        detail.unwrap_or_else(|| "Could not display the video frame".into()),
                    ),
                    None => {}
                }
            }
            Message::RefreshStreams => {
                self.alerts.begin(Source::StreamDiscovery);
                self.command(client::Command::RefreshStreams);
            }
            Message::WatchStream(id) => {
                if self.connected && !self.loopback && !self.stopping && self.tab == Tab::Streaming
                {
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
                    return iced::clipboard::write(path.display().to_string());
                }
            }
            Message::CompactUsageTick => {
                if self.performance_mode.load(Ordering::Relaxed) {
                    self.process_usage = self.process_monitor.sample();
                }
            }
            Message::UsageTick => {
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
                self.alerts.begin(Source::Connection);
                self.alerts.begin(Source::Audio);
                return self.start(false);
            }
            Message::TestAudio => {
                self.alerts.begin(Source::Audio);
                return self.start(true);
            }
            Message::Stop => {
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
                        return iced::widget::operation::focus("channel-password");
                    }
                    self.command(client::Command::Join(id, String::new()));
                }
            }
            Message::SubmitChannelPassword => {
                if let Some(id) = self.password_channel
                    && self.can_request_join(id)
                    && self.command(client::Command::Join(id, self.channel_password.clone()))
                {
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
                if self.session.is_none() {
                    return iced::exit();
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
                    if self
                        .password_channel
                        .is_some_and(|id| !self.can_request_join(id))
                    {
                        self.password_channel = None;
                        self.channel_password.clear();
                    }
                }
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
                        return iced::widget::operation::focus("channel-password");
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
                    self.connection_history = ConnectionHistory::new();
                    self.password_channel = None;
                    self.channel_password.clear();
                    self.session = None;
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
                        return iced::exit();
                    }
                }
            },
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let _trace = profiler::scope(profiler::Category::Ui, "ui.view");
        let content = if self.performance_mode.load(Ordering::Relaxed) {
            self.performance_view()
        } else {
            self.normal_view()
        };
        let blocked = self.password_channel.is_some() || self.alerts.current().is_some();
        let content = modal::shield(content, blocked);
        if let Some(channel) = self
            .password_channel
            .and_then(|id| self.state.channels.iter().find(|channel| channel.id == id))
        {
            let mut fields = column![
                text(format!("Join {}", channel.name)).size(22),
                text("This channel is password protected.").size(13),
                text_input("Channel password", &self.channel_password)
                    .style(appearance::input)
                    .id("channel-password")
                    .secure(true)
                    .on_input(Message::ChannelPassword)
                    .on_submit(Message::SubmitChannelPassword)
                    .padding(12),
            ]
            .spacing(14);
            if let Some(error) = &self.password_error {
                fields = fields.push(
                    text(error)
                        .size(12)
                        .color(iced::Color::from_rgb8(255, 130, 150)),
                );
            }
            let dialog = container(
                fields.push(
                    row![
                        label_button("Cancel")
                            .on_press(Message::CancelChannelPassword)
                            .style(appearance::secondary),
                        label_button("Join").on_press(Message::SubmitChannelPassword),
                    ]
                    .spacing(appearance::CONTROL_GAP),
                ),
            )
            .padding(24)
            .width(Length::Fill)
            .max_width(400)
            .style(appearance::card);
            stack![
                content,
                opaque(
                    container(dialog)
                        .center_x(Length::Fill)
                        .center_y(Length::Fill)
                        .padding(20)
                        .style(|_| container::Style {
                            background: Some(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.65).into()),
                            ..container::Style::default()
                        })
                )
            ]
            .into()
        } else if let Some(incident) = self.alerts.current() {
            let overlay = iced::widget::responsive(|size| {
                let dialog = container(
                    column![
                        text(incident.source.title()).size(20),
                        container(
                            scrollable(text(&incident.detail).size(13)).height(Length::Shrink)
                        )
                        .max_height((size.height - 180.0).clamp(48.0, 240.0)),
                        text(if incident.reports > 1 {
                            format!("{} reports of this issue", incident.reports)
                        } else {
                            String::new()
                        })
                        .size(11)
                        .color(appearance::MUTED),
                        label_button("Close · Enter / Esc").on_press(Message::DismissError),
                    ]
                    .spacing(12),
                )
                .padding(20)
                .width(Length::Fill)
                .max_width(520)
                .style(appearance::card);
                opaque(
                    container(dialog)
                        .center_x(Length::Fill)
                        .center_y(Length::Fill)
                        .padding(16)
                        .style(|_| container::Style {
                            background: Some(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.65).into()),
                            ..Default::default()
                        }),
                )
            });
            stack![content, overlay].into()
        } else {
            stack![content].into()
        }
    }

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

    fn normal_view(&self) -> Element<'_, Message> {
        let sections = iced::widget::responsive(|size| {
            let mut tabs = row![].spacing(appearance::CONTROL_GAP);
            for (tab, icon, label) in [
                (Tab::Voice, Icon::Headphones, "Voice"),
                (Tab::Streaming, Icon::MonitorPlay, "Streaming"),
            ] {
                let control = if size.width >= 220.0 {
                    icon_button(icon, label)
                } else {
                    appearance::icon_only_button(icon)
                };
                tabs = tabs.push(
                    tooltip(
                        control
                            .on_press(Message::ShowTab(tab))
                            .style(if self.tab == tab {
                                appearance::active_nav
                            } else {
                                appearance::nav
                            }),
                        text(label).size(12),
                        tooltip::Position::Bottom,
                    )
                    .padding(8)
                    .gap(8)
                    .style(appearance::floating),
                );
            }
            tabs.into()
        })
        .height(appearance::CONTROL_HEIGHT);
        let mut utilities = row![].spacing(appearance::CONTROL_GAP);
        for (tab, icon, label) in [
            (Tab::Statistics, Icon::ChartLine, "Statistics"),
            (Tab::Settings, Icon::Settings, "Settings"),
        ] {
            utilities = utilities.push(
                tooltip(
                    appearance::icon_only_button(icon)
                        .on_press(Message::ShowTab(tab))
                        .style(if self.tab == tab {
                            appearance::active_nav
                        } else {
                            appearance::nav
                        }),
                    text(label).size(12),
                    tooltip::Position::Bottom,
                )
                .padding(8)
                .gap(8)
                .style(appearance::floating),
            );
        }
        column![
            container(
                row![
                    sections,
                    self.performance_switch(false),
                    container(rule::vertical(1)).height(24),
                    utilities,
                ]
                .spacing(20)
                .align_y(iced::Alignment::Center)
            )
            .padding([14, 24])
            .style(appearance::chrome),
            rule::horizontal(1),
            match self.tab {
                Tab::Voice => self.voice_view(),
                Tab::Streaming => self.streaming_view(),
                Tab::Statistics => self.statistics_view(),
                Tab::Settings => self.settings_view(),
            },
            self.resource_bar(),
        ]
        .height(Length::Fill)
        .width(Length::Fill)
        .into()
    }

    fn streaming_view(&self) -> Element<'_, Message> {
        let connected = self.connected && !self.loopback && !self.stopping;
        let supported = connected && self.state.server_version.starts_with("6.");
        let mut content = column![
            text("Streaming").size(28),
            text("Experimental TeamSpeak screen-share viewer · video only")
                .size(13)
                .color(appearance::MUTED),
        ]
        .spacing(18);
        if !connected {
            content = content.push(
                container(
                    column![
                        text("Connect to a server first").size(18),
                        text("Join the publisher’s channel in Voice, then return here to watch.")
                            .size(14)
                    ]
                    .spacing(10),
                )
                .padding(20)
                .width(Length::Fill)
                .style(appearance::card),
            );
        } else {
            content = content.push(
                row![
                    column![
                        text(&self.state.server).size(16),
                        text(format!(
                            "Server {} · {}",
                            self.state.server_version,
                            self.state
                                .current
                                .as_ref()
                                .map_or("No channel", |channel| channel.name.as_str())
                        ))
                        .size(12)
                        .color(appearance::MUTED)
                    ]
                    .width(Length::Fill)
                    .spacing(4),
                    label_button("Refresh streams")
                        .on_press_maybe(supported.then_some(Message::RefreshStreams))
                        .style(appearance::secondary),
                ]
                .spacing(12)
                .align_y(iced::Alignment::Center),
            );
            if !supported {
                content = content.push(text("This server does not advertise TeamSpeak 6. Server-channel streaming is unavailable.").size(14));
            } else if !streaming::AVAILABLE {
                content = content.push(
                    text("The experimental native viewer is currently available on macOS.")
                        .size(14),
                );
            }
            let active_stream = self
                .streaming
                .stream
                .as_ref()
                .filter(|_| self.streaming.active());
            if let Some(stream) = active_stream {
                let phase = self.streaming.phase.unwrap_or(streaming::Phase::Stopped);
                let mut player = column![
                    row![
                        text(&stream.name).size(18).width(Length::Fill),
                        label_button("Stop watching")
                            .on_press(Message::StopWatching)
                            .style(appearance::danger)
                    ]
                    .spacing(12)
                    .align_y(iced::Alignment::Center),
                    text(phase.label()).size(14),
                    text(&self.streaming.detail)
                        .size(12)
                        .color(appearance::MUTED),
                ]
                .spacing(10);
                if let Some(frame) = &self.streaming.frame {
                    player = player.push(
                        iced::widget::image(frame.handle.clone())
                            .rotation(iced::Radians(frame.rotation))
                            .content_fit(iced::ContentFit::Contain)
                            .width(Length::Fill)
                            .height(400),
                    );
                } else if self.streaming.active() {
                    player = player.push(
                        container(
                            text("Waiting for decoded video…")
                                .size(14)
                                .color(appearance::MUTED),
                        )
                        .center_x(Length::Fill)
                        .center_y(240)
                        .style(appearance::inset),
                    );
                }
                let stats = &self.streaming.stats;
                if stats.packets > 0 || self.streaming.ui_updates > 0 {
                    player = player.push(
                        text(format!(
                            "{} · {}×{} · {:.1} decoded FPS",
                            if stats.codec.is_empty() {
                                "Negotiating codec"
                            } else {
                                &stats.codec
                            },
                            stats.width,
                            stats.height,
                            stats.fps
                        ))
                        .size(13),
                    );
                    player = player.push(
                        text(format!(
                            "{} RTP packets · {} decoded frames · {} UI updates · {} received",
                            stats.packets,
                            stats.decoded_frames,
                            self.streaming.ui_updates,
                            usage::format_bytes(stats.bytes as f64)
                        ))
                        .size(12),
                    );
                    player = player.push(
                        text(format!(
                            "Decoder: {} · ICE: {}",
                            stats.decoder, stats.ice_path
                        ))
                        .size(11)
                        .color(appearance::MUTED),
                    );
                    if !stats.codec_parameters.is_empty() {
                        player = player.push(
                            text(&stats.codec_parameters)
                                .size(10)
                                .color(appearance::MUTED),
                        );
                    }
                    if !stats.publisher_version.is_empty() {
                        player = player.push(
                            text(format!("Publisher: {}", stats.publisher_version))
                                .size(11)
                                .color(appearance::MUTED),
                        );
                    }
                }
                content = content.push(
                    container(player)
                        .padding(20)
                        .width(Length::Fill)
                        .style(appearance::card),
                );
            }
            let mut available_streams = self
                .streaming
                .catalog
                .iter()
                .filter(|stream| active_stream.is_none_or(|active| active.id != stream.id))
                .peekable();
            if available_streams.peek().is_some() || active_stream.is_none() {
                content = content.push(text("Streams in your channel").size(18));
            }
            if available_streams.peek().is_none() && active_stream.is_none() {
                content = content.push(text("No active streams found. Start sharing from an official TeamSpeak 6 client, then refresh.").size(13).color(appearance::MUTED));
            }
            for stream in available_streams {
                let publisher = self
                    .state
                    .current_members()
                    .iter()
                    .find(|member| member.id == stream.publisher)
                    .map_or("Publisher", |member| member.name.as_str());
                content = content.push(
                    container(
                        row![
                            column![
                                text(&stream.name).size(15),
                                text(format!(
                                    "{publisher} · {} viewers{}",
                                    stream.viewers,
                                    if stream.has_audio {
                                        " · stream audio is not played"
                                    } else {
                                        ""
                                    }
                                ))
                                .size(12)
                                .color(appearance::MUTED)
                            ]
                            .spacing(5)
                            .width(Length::Fill),
                            label_button(if stream.server_relayed {
                                "Server relay unavailable"
                            } else {
                                "Watch"
                            })
                            .on_press_maybe(
                                (supported
                                    && streaming::AVAILABLE
                                    && !stream.server_relayed
                                    && !self.streaming.active())
                                .then(|| Message::WatchStream(stream.id.clone()))
                            )
                        ]
                        .spacing(12)
                        .align_y(iced::Alignment::Center),
                    )
                    .padding(16)
                    .width(Length::Fill)
                    .style(appearance::card),
                );
            }
        }
        content = content.push(text("Receive-only proof · preview up to 1280×720 at 30 FPS. No screen, camera or microphone capture is started here. Leaving Streaming, switching channels or entering performance mode stops video viewing.").size(11).color(appearance::MUTED));
        scrollable(content.padding(28).max_width(1200))
            .height(Length::Fill)
            .into()
    }

    fn performance_switch(&self, enabled: bool) -> Element<'_, Message> {
        checkbox(enabled)
            .label("Performance mode")
            .size(16)
            .text_size(if enabled { 14 } else { 12 })
            .on_toggle(Message::PerformanceMode)
            .style(appearance::check)
            .into()
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

    fn resource_bar(&self) -> Element<'_, Message> {
        let compact = self.performance_mode.load(Ordering::Relaxed);
        let show_connection = !compact
            && self.connected
            && !self.loopback
            && !self.stopping
            && self.state.current.is_some();
        let cpu = self.process_usage.cpu_percent.map_or_else(
            || "measuring…".into(),
            |value| usage::format_cpu_percent(f64::from(value)),
        );
        let (health, label, detail) = self.footer_connection_status();
        let status = tooltip(
            container(
                row![text("●").size(10), text(label).size(12)]
                    .spacing(7)
                    .align_y(iced::Alignment::Center),
            )
            .padding([5, 9])
            .style(move |theme| container::Style {
                text_color: Some(health_color(health)),
                ..appearance::inset(theme)
            }),
            container(text(detail).size(12)).max_width(340),
            tooltip::Position::Top,
        )
        .padding(10)
        .gap(8)
        .style(appearance::floating);
        let size = if compact { 11 } else { 12 };
        let ram = format!("RAM {}", usage::format_mib(self.process_usage.memory_bytes));
        let footprint = format!(
            "{} {}",
            usage::MEMORY_DETAIL_LABEL,
            usage::format_mib(self.process_usage.memory_detail_bytes)
        );
        let readings: Element<'_, Message> = if show_connection {
            column![
                icons::label(Icon::Cpu, format!("CPU {cpu}"), 11),
                icons::label(Icon::Memory, ram, 11),
                text(footprint).size(10).color(appearance::MUTED),
            ]
            .spacing(2)
            .width(150)
            .into()
        } else {
            row![
                icons::label(Icon::Cpu, format!("CPU {cpu}"), size),
                icons::label(Icon::Memory, ram, size),
                text(footprint).size(size),
            ]
            .spacing(if compact { 12 } else { 24 })
            .align_y(iced::Alignment::Center)
            .wrap()
            .into()
        };
        let content: Element<'_, Message> = if compact {
            column![readings, status].spacing(7).into()
        } else if show_connection {
            row![
                status,
                iced::widget::responsive(|size| {
                    let minimum = 4.0 * appearance::CONNECTION_CHART_MIN_WIDTH
                        + 3.0 * appearance::CONNECTION_CHART_GAP as f32;
                    scrollable(
                        container(connection_health_view(
                            self.connection_usage,
                            &self.connection_history,
                            true,
                        ))
                        .width(size.width.max(minimum)),
                    )
                    .direction(iced::widget::scrollable::Direction::Horizontal(
                        iced::widget::scrollable::Scrollbar::new()
                            .width(3)
                            .scroller_width(3)
                            .spacing(3),
                    ))
                    .width(Length::Fill)
                    .height(44)
                    .into()
                })
                .height(44),
                container(rule::vertical(1)).height(38),
                readings,
            ]
            .spacing(16)
            .align_y(iced::Alignment::Center)
            .into()
        } else {
            row![status, iced::widget::space::horizontal(), readings]
                .spacing(20)
                .align_y(iced::Alignment::Center)
                .into()
        };
        column![
            rule::horizontal(1),
            container(content)
                .padding(if show_connection {
                    iced::Padding::from([8, 14])
                } else {
                    iced::Padding::from(if compact { 12 } else { 14 })
                })
                .width(Length::Fill)
                .style(appearance::chrome)
        ]
        .width(Length::Fill)
        .into()
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

    fn shortcut_hint(&self) -> Element<'_, Message> {
        if let Some(error) = &self.shortcut_error {
            text(if shortcuts::supported() {
                "Global shortcuts unavailable. Configure them in Settings.".into()
            } else {
                format!("Global shortcuts unavailable: {error}")
            })
            .size(11)
            .color(iced::Color::from_rgb8(255, 130, 150))
            .into()
        } else {
            text(format!(
                "Global: {} · mute/unmute   |   {} · deafen/undeafen",
                self.keybinds.toggle_mute, self.keybinds.toggle_deafen
            ))
            .size(11)
            .color(appearance::MUTED)
            .into()
        }
    }

    fn settings_view(&self) -> Element<'_, Message> {
        let idle = self.session.is_none();
        let connection = column![
            text("Connection").size(18),
            row![
                column![
                    icons::label(Icon::Server, "Server address", 12),
                    text_input("hostname:9987 or IP address", &self.address)
                        .style(appearance::input)
                        .on_input_maybe(idle.then_some(Message::Address))
                        .padding(11)
                ]
                .spacing(8)
                .width(Length::Fill),
                column![
                    text("Nickname").size(12),
                    text_input("Nickname", &self.nickname)
                        .style(appearance::input)
                        .on_input_maybe(idle.then_some(Message::Nickname))
                        .padding(11)
                ]
                .spacing(8)
                .width(Length::Fill),
            ]
            .spacing(16),
        ]
        .spacing(14);
        let shortcuts = column![
            text("Global keybinds").size(18),
            text("Use your controls from any app. Save to apply changes.")
                .size(12)
                .color(appearance::MUTED),
            row![
                column![
                    text("Toggle microphone mute").size(12),
                    text_input("Ctrl+Shift+M", &self.keybind_draft.toggle_mute)
                        .style(appearance::input)
                        .on_input(Message::MuteKeybind)
                        .padding(11)
                ]
                .spacing(8)
                .width(Length::Fill),
                column![
                    text("Toggle deafen").size(12),
                    text_input("Ctrl+Shift+D", &self.keybind_draft.toggle_deafen)
                        .style(appearance::input)
                        .on_input(Message::DeafenKeybind)
                        .padding(11)
                ]
                .spacing(8)
                .width(Length::Fill),
            ]
            .spacing(16),
            self.shortcut_hint(),
        ]
        .spacing(14);
        let mut content = column![
            text("Settings").size(28),
            text("Your connection, audio and controls.").size(13).color(appearance::MUTED),
            container(connection).padding(20).width(Length::Fill).style(appearance::card),
            container(column![text("Audio").size(18), self.audio_settings_view()].spacing(16)).padding(20).width(Length::Fill).style(appearance::card),
            container(shortcuts).padding(20).width(Length::Fill).style(appearance::card),
            row![
                label_button("Save settings").on_press(Message::SaveSettings),
                label_button("Reload JSON").on_press_maybe(idle.then_some(Message::ReloadSettings)).style(appearance::secondary),
                label_button("Copy settings path").on_press_maybe(self.settings_path.as_ref().map(|_| Message::CopySettingsPath)).style(appearance::secondary),
            ].spacing(appearance::CONTROL_GAP).align_y(iced::Alignment::Center).wrap(),
            text("Save writes your portable settings. After editing the JSON externally, disconnect and Reload JSON.").size(12).color(appearance::MUTED),
        ].spacing(18);
        if let Some(path) = &self.settings_path {
            content = content.push(text(path.display().to_string()).size(11));
        }
        if let Some(message) = &self.settings_message {
            content = content.push(text(message).size(13));
        }
        content = content.push(text(match self.last_shortcut {
            Some(shortcuts::Action::Mute) => "Last shortcut received: mute/unmute",
            Some(shortcuts::Action::Deafen) => "Last shortcut received: deafen/undeafen",
            None => "You can test shortcuts while disconnected; the last detected action appears here.",
        }).size(12));
        scrollable(content.padding(28).max_width(920))
            .height(Length::Fill)
            .into()
    }

    fn performance_view(&self) -> Element<'_, Message> {
        let in_channel = self.connected && !self.loopback && !self.stopping;
        let connection = if self.stopping {
            "Disconnecting…"
        } else if self.connected {
            if self.loopback {
                "Local microphone test"
            } else {
                "Connected"
            }
        } else {
            &self.status
        };
        let channel = self
            .state
            .current
            .as_ref()
            .filter(|_| in_channel)
            .map_or("—", |channel| channel.name.as_str());
        let playback = if !self.connected || self.stopping {
            "Inactive"
        } else if self.deafened {
            "Deafened"
        } else {
            "On"
        };
        let info = column![
            text(format!("Connection: {connection}")).size(14),
            icons::label(
                Icon::Server,
                format!(
                    "Server: {}",
                    if in_channel {
                        &self.state.server
                    } else {
                        "—"
                    }
                ),
                14
            ),
            icons::label(Icon::Channel, format!("Channel: {channel}"), 14),
            icons::label(
                self.microphone_icon(),
                format!("Microphone: {}", self.microphone_status()),
                14
            ),
            icons::label(self.playback_icon(), format!("Playback: {playback}"), 14),
        ]
        .spacing(6);
        column![
            container(
                column![
                    self.performance_switch(true),
                    scrollable(info.width(Length::Fill))
                        .height(Length::Fill)
                        .width(Length::Fill),
                    self.shortcut_hint(),
                    text(format!(
                        "Updates every {} s · CPU: 100% = one core",
                        usage::COMPACT_REFRESH_INTERVAL.as_secs()
                    ))
                    .size(11),
                ]
                .spacing(8)
                .width(Length::Fill)
                .height(Length::Fill)
            )
            .padding(16)
            .height(Length::Fill)
            .width(Length::Fill),
            self.resource_bar(),
        ]
        .height(Length::Fill)
        .into()
    }

    fn audio_settings_view(&self) -> Element<'_, Message> {
        let busy = self.session.is_some();
        let volume = self.playback_volume.percent();
        column![
            row![
                column![
                    icons::label(Icon::Mic, "Microphone", 13),
                    pick_list(self.devices.inputs.clone(), self.input.as_ref(), Message::Input).style(appearance::device_picker).menu_style(appearance::device_menu)
                        .placeholder("No microphone detected").width(Length::Fill).padding(10),
                    text(if busy {
                        format!("Input activity · {}", self.microphone_status())
                    } else {
                        "Input activity · start a local test".into()
                    }).size(12),
                    progress_bar(0.0..=1.0, self.microphone_meter_level()).girth(8).style(appearance::meter),
                ].spacing(6).width(Length::Fill),
                column![
                    icons::label(self.playback_icon(), "Playback", 13),
                    pick_list(self.devices.outputs.clone(), self.output.as_ref(), Message::Output).style(appearance::device_picker).menu_style(appearance::device_menu)
                        .placeholder("No playback device detected").width(Length::Fill).padding(10),
                    icons::label(if volume == 0 { Icon::VolumeOff } else { Icon::Volume }, format!("Volume · {volume}%"), 12),
                    slider(0..=100, volume, Message::PlaybackVolume).step(1).style(appearance::volume),
                ].spacing(6).width(Length::Fill),
            ].spacing(16),
            row![
                label_button("Refresh devices").on_press_maybe((!busy).then_some(Message::Refresh)).style(appearance::secondary),
                icon_button(Icon::Mic, if busy && self.loopback { "Stop microphone test" } else { "Test microphone locally" })
                    .on_press_maybe(if self.stopping { None } else if busy && self.loopback { Some(Message::Stop) } else if !busy { Some(Message::TestAudio) } else { None })
                    .style(if busy && self.loopback { appearance::danger } else { appearance::secondary }),
            ].spacing(appearance::CONTROL_GAP).align_y(iced::Alignment::Center).wrap(),
            text("Use headphones for the local test. Disconnect to change audio devices; volume updates live.").size(12),
        ].spacing(12).into()
    }

    fn voice_view(&self) -> Element<'_, Message> {
        let busy = self.session.is_some();
        let on_server = self.connected && !self.loopback;
        let mut content = column![].spacing(if on_server { 10 } else { 16 });
        if on_server {
            content = content
                .push(text("VOICE CHANNEL").size(10).color(appearance::MUTED))
                .push(icons::label(
                    Icon::Channel,
                    self.state
                        .current
                        .as_ref()
                        .map_or("Connecting…", |channel| channel.name.as_str()),
                    24,
                ));
        } else {
            content = content
                .push(text("Join your server").size(30))
                .push(
                    text("Choose a server and a name. Your microphone starts muted.")
                        .size(13)
                        .color(appearance::MUTED),
                )
                .push(icons::label(Icon::Server, "Server", 13))
                .push(
                    text_input("hostname:9987", &self.address)
                        .style(appearance::input)
                        .on_input_maybe((!busy).then_some(Message::Address))
                        .padding(10),
                )
                .push(text("Nickname").size(13))
                .push(
                    text_input("Your name", &self.nickname)
                        .style(appearance::input)
                        .on_input_maybe((!busy).then_some(Message::Nickname))
                        .padding(10),
                )
                .push(text("Server password (optional)").size(13))
                .push(
                    text_input("Password", &self.password)
                        .style(appearance::input)
                        .secure(true)
                        .on_input_maybe((!busy).then_some(Message::Password))
                        .padding(10),
                );
        }
        content = content.push(text(&self.status).size(13).color(appearance::MUTED));
        if busy && !on_server {
            content = content.push(
                label_button(if self.stopping {
                    "Stopping…"
                } else if self.loopback {
                    "Stop audio test"
                } else {
                    "Cancel connection"
                })
                .on_press_maybe((!self.stopping).then_some(Message::Stop))
                .style(appearance::danger),
            );
        } else if !busy {
            content = content
                .push(icon_button(Icon::Server, "Connect to server").on_press(Message::Connect));
        }
        content = content.push(self.shortcut_hint());
        if self.connected {
            content = content.push(rule::horizontal(1)).push(
                row![
                    icons::label(
                        self.microphone_icon(),
                        format!("Microphone: {}", self.microphone_status()),
                        12
                    ),
                    icons::label(
                        self.playback_icon(),
                        if self.deafened {
                            "Playback: Deafened"
                        } else {
                            "Playback: On"
                        },
                        12
                    ),
                ]
                .spacing(20)
                .wrap(),
            );
            if !self.detail.is_empty() {
                content = content.push(text(&self.detail).size(11).color(appearance::MUTED));
            }
            if !self.loopback
                && (self.controls_pending
                    || self.server_controls.is_none_or(|state| {
                        state.muted != self.muted || state.deafened != self.deafened
                    }))
            {
                let confirmed = self.server_controls.map_or_else(
                    || "not yet confirmed".into(),
                    |state| {
                        format!(
                            "microphone {} · sound {}",
                            if state.muted { "muted" } else { "unmuted" },
                            if state.deafened { "deafened" } else { "on" }
                        )
                    },
                );
                content = content.push(
                    text(format!(
                        "Server state: {confirmed}{}",
                        if self.controls_pending {
                            " · update pending"
                        } else {
                            " · update not confirmed"
                        }
                    ))
                    .size(11)
                    .color(appearance::MUTED),
                );
            }
            if !self.loopback {
                content = content.push(
                    text(format!(
                        "Audio packets  ↑ {}  ↓ {}",
                        self.sent, self.received
                    ))
                    .size(12),
                );
                content = column![
                    container(content)
                        .padding(20)
                        .width(Length::Fill)
                        .style(appearance::card)
                ]
                .spacing(18);
                content = content.push(self.client_connections_view());
                if !self.muted && !self.deafened && !self.state.can_talk {
                    content = content
                        .push(text("The server is not allowing speech in this channel.").size(13));
                }
            }
        }
        let body: Element<'_, Message> = if on_server {
            container(content).padding(24).width(Length::Fill).into()
        } else {
            container(
                container(content)
                    .padding(28)
                    .width(Length::Fill)
                    .max_width(600)
                    .style(appearance::card),
            )
            .padding(32)
            .center_x(Length::Fill)
            .into()
        };
        let main = scrollable(body).height(Length::Fill).width(Length::Fill);
        if on_server {
            row![
                container(
                    column![
                        self.sidebar_controls(),
                        rule::horizontal(1),
                        row![
                            Icon::Server.view(18),
                            text(&self.state.server).size(16).width(Length::Fill)
                        ]
                        .spacing(8),
                        text("CHANNELS").size(10).color(appearance::MUTED),
                        scrollable(self.channel_tree_view())
                            .height(Length::Fill)
                            .width(Length::Fill),
                    ]
                    .spacing(12)
                    .height(Length::Fill)
                )
                .padding(12)
                .width(280)
                .height(Length::Fill)
                .style(appearance::chrome),
                rule::vertical(1),
                main,
            ]
            .height(Length::Fill)
            .width(Length::Fill)
            .into()
        } else {
            main.into()
        }
    }

    fn sidebar_controls(&self) -> Element<'_, Message> {
        let enabled = self.connected && !self.loopback && !self.stopping;
        let mut controls = row![].spacing(appearance::CONTROL_GAP);
        for (icon, label, message, active, enabled) in [
            (
                Icon::Disconnect,
                "Disconnect",
                Message::Stop,
                false,
                !self.stopping,
            ),
            (
                self.microphone_icon(),
                if self.muted {
                    "Unmute microphone"
                } else {
                    "Mute microphone"
                },
                Message::Mute,
                self.muted || self.deafened,
                enabled,
            ),
            (
                self.playback_icon(),
                if self.deafened { "Undeafen" } else { "Deafen" },
                Message::Deafen,
                self.deafened,
                enabled,
            ),
        ] {
            controls = controls.push(
                tooltip(
                    appearance::icon_only_button(icon)
                        .on_press_maybe(enabled.then_some(message))
                        .style(if icon == Icon::Disconnect {
                            appearance::danger
                        } else if active {
                            appearance::active_nav
                        } else {
                            appearance::secondary
                        }),
                    text(label).size(12),
                    tooltip::Position::Bottom,
                )
                .padding(8)
                .gap(6)
                .style(appearance::floating),
            );
        }
        controls.into()
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

    fn client_connections_view(&self) -> Element<'_, Message> {
        let mut clients = column![text("Channel connections").size(16)].spacing(8);
        let mut count = 0;
        if let Some(channel) = &self.state.current {
            for member in channel.members.iter().filter(|member| !member.is_self) {
                count += 1;
                let expanded = self.expanded_client == Some(member.id);
                let quality = self.client_quality(member.id, Instant::now());
                let (health, detail) = self.member_health(member, true);
                let header = row![
                    connection_indicator(health, detail),
                    text(&member.name).size(14).width(Length::Fill),
                    text(if expanded { "▾" } else { "▸" }).size(13),
                ]
                .spacing(10)
                .align_y(iced::Alignment::Center);
                clients = clients.push(
                    button(header)
                        .width(Length::Fill)
                        .style(appearance::secondary)
                        .on_press_maybe(
                            (!self.stopping).then_some(Message::ToggleClient(member.id)),
                        ),
                );
                if expanded {
                    clients =
                        clients.push(connection_health_view(quality, &self.client_history, false));
                    clients = clients.push(
                        text(match self.client_usage.get(&member.id) {
                            Some(Err(error)) => *error,
                            Some(Ok(_)) => {
                                "Server snapshot · refreshes are staggered across your channel"
                            }
                            None => "Waiting for a connection-health sample…",
                        })
                        .size(11),
                    );
                }
            }
        }
        if count == 0 {
            clients = clients.push(text("No other clients in your channel.").size(12));
        }
        clients.into()
    }

    fn client_quality(&self, id: u16, now: Instant) -> usage::ConnectionUsage {
        self.client_usage
            .get(&id)
            .and_then(|result| result.as_ref().ok())
            .map_or_else(usage::ConnectionUsage::default, |sample| sample.sample(now))
    }

    fn channel_tree_view(&self) -> Element<'_, Message> {
        let mut tree = column![].spacing(12);
        let mut hidden_below = None;
        for channel in &self.state.channels {
            if hidden_below.is_some_and(|depth| channel.depth > depth) {
                continue;
            }
            hidden_below = None;
            let collapsed = self.collapsed_channels.contains(&channel.id);
            if collapsed {
                hidden_below = Some(channel.depth);
            }
            let current = self
                .state
                .current
                .as_ref()
                .is_some_and(|value| value.id == channel.id);
            let hint = if current {
                "Current channel"
            } else {
                match channel.join_access {
                    client::JoinAccess::Denied => "You do not have permission to join this channel",
                    client::JoinAccess::UnsupportedCodec => {
                        "This channel uses an unsupported audio codec"
                    }
                    _ if channel.has_password => "Click to join · a password will be requested",
                    _ => "Click to join this channel",
                }
            };
            let name = button(
                row![
                    Icon::Volume.view(16),
                    text(&channel.name)
                        .size(13)
                        .font(iced::font::Font {
                            weight: iced::font::Weight::Bold,
                            ..Default::default()
                        })
                        .width(Length::Fill),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            )
            .height(appearance::CHANNEL_ROW_HEIGHT)
            .width(Length::Fill)
            .padding([0, 8])
            .on_press_maybe(
                ((!self.stopping && current) || self.can_request_join(channel.id))
                    .then_some(Message::Join(channel.id)),
            )
            .style(move |theme, status| appearance::channel_row(theme, status, current));
            let header = row![
                compact_button(if collapsed { "▸" } else { "▾" })
                    .on_press(Message::ToggleChannel(channel.id))
                    .style(appearance::nav)
                    .width(24)
                    .height(appearance::CHANNEL_ROW_HEIGHT),
                tooltip(name, text(hint).size(12), tooltip::Position::Top)
                    .padding(8)
                    .gap(6)
                    .style(appearance::floating),
            ]
            .spacing(2)
            .align_y(iced::Alignment::Center);
            let mut node = column![header].spacing(2);
            if !collapsed {
                for member in &channel.members {
                    let mic_muted = member.mic_muted
                        || (member.is_self && self.microphone_icon() == Icon::MicOff);
                    let sound_muted = member.sound_muted || (member.is_self && self.deafened);
                    let name = if member.is_self {
                        format!("{} (you)", member.name)
                    } else {
                        member.name.clone()
                    };
                    let (health, detail) = self.member_health(member, current);
                    let mut entry = row![
                        connection_indicator(health, detail),
                        text(name).size(13).width(Length::Fill),
                    ]
                    .spacing(8)
                    .align_y(iced::Alignment::Center);
                    for (muted, icon, label) in [
                        (mic_muted, Icon::MicOff, "Microphone muted"),
                        (sound_muted, Icon::HeadphoneOff, "Sound muted"),
                    ] {
                        if muted {
                            entry = entry.push(
                                tooltip(
                                    icon.view(14).color(appearance::MUTED),
                                    text(label).size(12),
                                    tooltip::Position::Top,
                                )
                                .padding(8)
                                .style(appearance::floating),
                            );
                        }
                    }
                    node = node.push(
                        container(container(entry).padding([4, 8]).width(Length::Fill).style(
                            move |_| container::Style {
                                background: member.is_self.then_some(appearance::SURFACE.into()),
                                ..Default::default()
                            },
                        ))
                        .padding(iced::Padding {
                            left: 26.0,
                            ..Default::default()
                        }),
                    );
                }
                if !channel.members_visible {
                    node = node.push(
                        container(
                            text("Members not visible")
                                .size(10)
                                .color(appearance::MUTED),
                        )
                        .padding(iced::Padding {
                            left: 34.0,
                            ..Default::default()
                        }),
                    );
                }
            }
            tree = tree.push(container(node).padding(iced::Padding {
                left: channel.depth.min(8) as f32 * 12.0,
                ..iced::Padding::default()
            }));
        }
        tree.into()
    }

    fn stop_trace(&mut self) -> Task<Message> {
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

    fn statistics_view(&self) -> Element<'_, Message> {
        let mut content = column![
            text("Usage & Statistics").size(28),
            text(&self.status).size(13),
            self.usage_view(),
            rule::horizontal(1),
            text("Traces & CPU breakdown").size(22),
            text("Record up to 30 seconds of timings and resource counters. Audio, messages and passwords are excluded.").size(12),
        ].spacing(10);
        content = content.push(if let Some(recording) = &self.recording {
            label_button(format!(
                "Stop & save trace · {} s",
                recording.started.elapsed().as_secs()
            ))
            .on_press(Message::StopTrace)
            .style(appearance::danger)
        } else {
            label_button(if self.trace_saving {
                "Saving trace…"
            } else {
                "Record 30 s trace"
            })
            .on_press_maybe((!self.trace_saving).then_some(Message::RecordTrace))
        });
        if let Some(path) = &self.trace_path {
            content = content
                .push(
                    row![
                        text(format!(
                            "Saved {}",
                            path.file_name().unwrap_or_default().to_string_lossy()
                        ))
                        .size(12),
                        label_button("Copy trace path")
                            .on_press(Message::CopyTracePath)
                            .style(appearance::secondary)
                    ]
                    .spacing(12)
                    .align_y(iced::Alignment::Center),
                )
                .push(text("The JSON trace opens in Perfetto or a Chrome trace viewer.").size(12));
        }
        if let Some(summary) = &self.trace_summary {
            content = content.push(statistics_charts::trace(summary));
        } else if self.recording.is_none() {
            content = content.push(text("Record a trace to measure CPU time in UI, networking, audio processing and audio callbacks.").size(13));
        }
        content = content
            .push(rule::horizontal(1))
            .push(statistics_charts::memory(
                self.process_usage.memory_bytes,
                self.heap_usage,
                self.audio_buffer_bytes,
                self.cpu_history.buffer_bytes()
                    + self.ram_history.buffer_bytes()
                    + self.connection_history.buffer_bytes()
                    + self.client_history.buffer_bytes(),
                profiler::buffer_bytes(),
            ));
        scrollable(content.padding(28).max_width(1200))
            .height(Length::Fill)
            .into()
    }

    fn usage_view(&self) -> Element<'_, Message> {
        let quality = self.connection_usage;
        let network = self.network_usage;
        let percent =
            |value: Option<f32>| value.map_or_else(|| "—".into(), |value| format!("{value:.1} %"));
        let rate = |value: f64| format!("{}/s", usage::format_bytes(value));
        let file_rate =
            |value: Option<u64>| value.map_or_else(|| "—".into(), |value| rate(value as f64));
        let ping = quality.ping_text();
        let histories = row![
            history_card(
                "App CPU",
                self.process_usage.cpu_percent.map_or_else(
                    || "—".into(),
                    |value| usage::format_cpu_percent(f64::from(value))
                ),
                &self.cpu_history,
                true,
                None,
            ),
            history_card(
                "RAM (RSS)",
                usage::format_mib(self.process_usage.memory_bytes),
                &self.ram_history,
                false,
                Some((
                    usage::MEMORY_DETAIL_LABEL,
                    usage::format_mib(self.process_usage.memory_detail_bytes)
                )),
            ),
        ]
        .spacing(24);
        column![
            histories,
            row![
                metric("Upload", rate(network.upload_per_second)),
                metric("Download", rate(network.download_per_second)),
                metric("Packet loss (in)", percent(quality.packet_loss_in)),
                metric("Packet loss (out)", percent(quality.packet_loss_out)),
                metric("Ping", ping),
                metric("Idle time", quality.idle_seconds.map_or_else(|| "—".into(), usage::format_idle)),
            ].spacing(12),
            row![
                metric("File transfer (in)", file_rate(quality.file_download_per_second)),
                metric("File transfer (out)", file_rate(quality.file_upload_per_second)),
                metric("Session sent", usage::format_bytes(network.uploaded as f64)),
                metric("Session received", usage::format_bytes(network.downloaded as f64)),
            ].spacing(12),
            text("CPU: 100% = one core · Memory counters overlap; do not add them · Traffic: TeamSpeak estimate · Local: 1 s / server: 5 s · — unavailable").size(10),
        ].spacing(8).max_width(1200).into()
    }
}

fn connection_indicator(health: usage::Health, detail: String) -> Element<'static, Message> {
    tooltip(
        container(text("●").size(10).color(health_color(health)))
            .width(16)
            .center_y(20),
        container(
            column![
                text(if health == usage::Health::Unknown {
                    "Unknown"
                } else {
                    health.label()
                })
                .size(12),
                text(detail).size(12),
            ]
            .spacing(4),
        )
        .max_width(300),
        tooltip::Position::Top,
    )
    .padding(8)
    .gap(6)
    .style(appearance::floating)
    .into()
}

fn connection_health_view(
    quality: usage::ConnectionUsage,
    history: &ConnectionHistory,
    compact: bool,
) -> Element<'_, Message> {
    let percent =
        |value: Option<f32>| value.map_or_else(|| "—".into(), |value| format!("{value:.1}%"));
    let mut content = column![].spacing(8);
    let mut strip = row![]
        .spacing(appearance::CONNECTION_CHART_GAP)
        .width(Length::Fill);
    for (metric, label, value, health, status, explanation) in [
        (
            Metric::LossIn,
            "Packet loss (in)",
            percent(quality.packet_loss_in),
            usage::Health::loss(quality.packet_loss_in),
            None,
            "Server → this client. Good: below 1%; Fair: 1–5%; Poor: 5% or more.",
        ),
        (
            Metric::LossOut,
            "Packet loss (out)",
            percent(quality.packet_loss_out),
            usage::Health::loss(quality.packet_loss_out),
            None,
            "This client → server. Good: below 1%; Fair: 1–5%; Poor: 5% or more.",
        ),
        (
            Metric::Ping,
            "Ping",
            quality.ping_text(),
            usage::Health::ping(quality.ping_ms, quality.ping_deviation_ms),
            None,
            "Round-trip latency ± variation. Fair: 100 ms or variation 20 ms; Poor: 200 ms or variation 50 ms.",
        ),
        (
            Metric::Idle,
            "Idle time",
            quality
                .idle_seconds
                .map_or_else(|| "—".into(), usage::format_idle),
            usage::Health::Unknown,
            quality
                .idle_seconds
                .map(|seconds| if seconds < 60 { "Active" } else { "Idle" }),
            "Time since voice activity. Being idle is not a connection problem.",
        ),
    ] {
        let color = health_color(health);
        let status = status.unwrap_or(health.label());
        let heading = if compact {
            row![
                text(match metric {
                    Metric::LossIn => "Loss ↓",
                    Metric::LossOut => "Loss ↑",
                    Metric::Ping => "Ping",
                    Metric::Idle => "Idle",
                })
                .size(10)
                .color(appearance::MUTED)
                .width(Length::Fill),
                text(value).size(11).color(color),
            ]
            .spacing(4)
        } else {
            row![
                text(label).size(12).width(Length::Fill),
                text(status).size(10).color(color),
                text(value).size(13),
            ]
            .spacing(8)
        };
        let reading = column![
            heading.align_y(iced::Alignment::Center),
            canvas(ConnectionChart { history, metric })
                .width(Length::Fill)
                .height(if compact { 20 } else { 32 }),
        ]
        .spacing(3);
        let chart = tooltip(
            container(reading)
                .padding(if compact { 0 } else { 7 })
                .width(Length::Fill)
                .style(appearance::inset),
            container(
                column![
                    text(format!("{label} · {status}")).size(12),
                    text(explanation).size(12),
                    text("Last 60 s · auto-scaled · shaded band: ping variation").size(10),
                ]
                .spacing(4),
            )
            .max_width(300),
            if compact {
                tooltip::Position::Top
            } else {
                tooltip::Position::Bottom
            },
        )
        .padding(8)
        .gap(8)
        .style(appearance::floating);
        if compact {
            strip = strip.push(chart);
        } else {
            content = content.push(chart);
        }
    }
    if compact {
        return strip.into();
    }
    content =
        content.push(text("Last 60 s → · auto-scaled · shaded band: ping variation").size(10));
    container(content)
        .padding(12)
        .width(Length::Fill)
        .style(appearance::card)
        .into()
}

fn metric(label: &'static str, value: String) -> Element<'static, Message> {
    column![text(label).size(11), text(value).size(16)]
        .spacing(2)
        .width(Length::Fill)
        .into()
}

fn history_card<'a>(
    label: &'static str,
    current: String,
    history: &'a usage::MetricHistory,
    cpu: bool,
    memory_detail: Option<(&'static str, String)>,
) -> Element<'a, Message> {
    let format = |value: f64| {
        if cpu {
            usage::format_cpu_percent(value)
        } else {
            usage::format_bytes(value)
        }
    };
    let values = history.percentiles.map_or_else(
        || "P50 —   ·   P90 —   ·   P99 —".into(),
        |p| {
            format!(
                "P50 {}   ·   P90 {}   ·   P99 {}",
                format(p.p50),
                format(p.p90),
                format(p.p99)
            )
        },
    );
    let color = if cpu {
        iced::Color::from_rgb8(50, 193, 215)
    } else {
        iced::Color::from_rgb8(163, 142, 246)
    };
    let mut heading = row![
        if cpu { Icon::Cpu } else { Icon::Memory }.view(16),
        text(format!("{label}   {current}")).size(14)
    ]
    .spacing(12)
    .align_y(iced::Alignment::Center);
    if let Some((detail_label, value)) = memory_detail {
        heading = heading
            .push(iced::widget::space::horizontal())
            .push(text(format!("{detail_label}   {value}")).size(14));
    }
    container(
        column![
            heading,
            text(format!("Scale 0–{}", format(history.upper_bound))).size(10),
            canvas(usage::HistoryChart { history, color })
                .width(Length::Fill)
                .height(86),
            text(values).size(11),
            text(format!(
                "{}Last 5 min · {} samples",
                if cpu { "" } else { "RSS history · " },
                history.len()
            ))
            .size(10),
        ]
        .spacing(3)
        .width(Length::Fill),
    )
    .padding(16)
    .width(Length::Fill)
    .style(appearance::card)
    .into()
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
        let _ = app.update(Message::ShowTab(Tab::Voice));
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
    fn outdated_window_queries_cannot_overwrite_the_restore_size() {
        let mut app = App::with_devices(audio::Devices::default(), None);
        let id = window::Id::unique();
        let layout = WindowLayout {
            size: Size::new(1200.0, 900.0),
            maximized: true,
            mode: window::Mode::Windowed,
        };
        let _ = app.update(Message::PerformanceMode(true));
        let stale_request = app.performance_request;
        let _ = app.update(Message::PerformanceMode(false));
        let _ = app.update(Message::PerformanceMode(true));
        let _ = app.update(Message::PerformanceWindow(stale_request, id, layout));
        assert!(app.window_restore.is_none());
        let _ = app.update(Message::PerformanceWindow(
            app.performance_request,
            id,
            layout,
        ));
        assert_eq!(app.window_restore.unwrap().size, layout.size);
        let _ = app.update(Message::PerformanceMode(false));
        let _ = app.update(Message::PerformanceWindow(
            app.performance_request,
            id,
            layout,
        ));
        assert!(app.window_restore.is_none());
    }
}
