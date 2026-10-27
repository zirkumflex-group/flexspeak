#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod audio;
mod client;
mod icons;
mod memory;
mod profiler;
mod settings;
mod shortcuts;
mod statistics_charts;
mod usage;

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use iced::{
    Element, Length, Size, Subscription, Task, Theme,
    widget::{
        button, canvas, checkbox, column, container, pick_list, progress_bar, row, rule,
        scrollable, slider, text, text_input,
    },
    window,
};
use tokio::sync::mpsc;
use tokio_stream::wrappers::{ReceiverStream, UnboundedReceiverStream};

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
        .title("Native Voice — TeamSpeak client")
        .theme(Theme::TokyoNight)
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
    settings_error: Option<String>,
    settings_message: Option<String>,
    playback_volume: Arc<audio::PlaybackVolume>,
    performance_mode: Arc<AtomicBool>,
    performance_request: u64,
    window_restore: Option<WindowLayout>,
    address: String,
    nickname: String,
    password: String,
    channel_password: String,
    devices: audio::Devices,
    input: Option<audio::DeviceChoice>,
    output: Option<audio::DeviceChoice>,
    status: String,
    error: Option<String>,
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
    process_monitor: usage::ProcessMonitor,
    process_usage: usage::ProcessUsage,
    cpu_history: usage::MetricHistory,
    ram_history: usage::MetricHistory,
    network_usage: usage::NetworkUsage,
    connection_usage: usage::ConnectionUsage,
    tab: Tab,
    heap_usage: memory::HeapUsage,
    audio_buffer_bytes: usize,
    recording: Option<profiler::Recording>,
    trace_summary: Option<profiler::TraceSummary>,
    trace_path: Option<PathBuf>,
    trace_error: Option<String>,
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
    Statistics,
    Settings,
}

#[derive(Debug, Clone)]
enum Message {
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
    Input(audio::DeviceChoice),
    Output(audio::DeviceChoice),
    Refresh,
    Connect,
    TestAudio,
    Stop,
    Mute,
    Deafen,
    Join(client::Channel),
    Backend(client::Event),
    Close,
    UsageTick,
    CompactUsageTick,
    ShowTab(Tab),
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
                        app.settings_error = Some(format!(
                            "{error:#}. Defaults are shown; fix the file and Reload, or Save to replace it with the displayed settings."
                        ))
                    }
                }
                app.settings_path = Some(path);
            }
            Err(error) => app.settings_error = Some(format!("{error:#}")),
        }
        let (mut shortcuts, events) = shortcuts::Shortcuts::new();
        app.shortcut_error = shortcuts
            .configure(&app.keybinds)
            .err()
            .map(|error| format!("{error:#}"));
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
        Self {
            last_shortcut: None,
            shortcuts: None,
            keybinds: shortcuts::Keybinds::default(),
            keybind_draft: shortcuts::Keybinds::default(),
            shortcut_error: None,
            settings_path: None,
            settings_error: None,
            settings_message: None,
            playback_volume: Arc::new(audio::PlaybackVolume::default()),
            performance_mode: Arc::new(AtomicBool::new(false)),
            performance_request: 0,
            window_restore: None,
            input: devices.input.clone(),
            output: devices.output.clone(),
            devices,
            address: "ts.zirkumflex.io".into(),
            nickname: "Native Voice".into(),
            password: String::new(),
            channel_password: String::new(),
            status: "Ready to connect".into(),
            error,
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
            process_monitor,
            process_usage,
            cpu_history,
            ram_history,
            network_usage: usage::NetworkUsage::default(),
            connection_usage: usage::ConnectionUsage::default(),
            tab: Tab::Voice,
            heap_usage: memory::snapshot(),
            audio_buffer_bytes: 0,
            recording: None,
            trace_summary: None,
            trace_path: None,
            trace_error: None,
            trace_saving: false,
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            iced::window::close_requests().map(|_| Message::Close),
            if self.performance_mode.load(Ordering::Relaxed) {
                iced::time::every(usage::COMPACT_REFRESH_INTERVAL)
                    .map(|_| Message::CompactUsageTick)
            } else {
                iced::time::every(usage::REFRESH_INTERVAL).map(|_| Message::UsageTick)
            },
        ])
    }

    fn command(&mut self, command: client::Command) -> bool {
        let Some(sender) = &self.session else {
            return false;
        };
        if sender.try_send(command).is_err() {
            self.error = Some(
                "The audio worker is busy or stopped. Wait for it to finish, then reconnect."
                    .into(),
            );
            false
        } else {
            true
        }
    }

    fn toggle_mute(&mut self) {
        if self.connected && !self.loopback && !self.stopping {
            let value = !self.muted;
            if self.command(client::Command::Mute(value)) {
                self.muted = value;
            }
        }
    }

    fn toggle_deafen(&mut self) {
        if self.connected && !self.loopback && !self.stopping {
            let value = !self.deafened;
            if self.command(client::Command::Deafen(value)) {
                self.deafened = value;
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
        }
        Ok(())
    }

    fn start(&mut self, loopback: bool) -> Task<Message> {
        if self.session.is_some() {
            return Task::none();
        }
        let (Some(input), Some(output)) = (self.input.clone(), self.output.clone()) else {
            self.error = Some("Choose an available microphone and playback device first.".into());
            return Task::none();
        };
        if !loopback && (self.address.trim().is_empty() || self.nickname.trim().is_empty()) {
            self.error = Some("Enter a server address and nickname.".into());
            return Task::none();
        }
        let config = client::Config {
            playback_volume: self.playback_volume.clone(),
            performance_mode: self.performance_mode.clone(),
            address: self.address.trim().into(),
            nickname: self.nickname.trim().into(),
            password: self.password.clone(),
            input,
            output,
        };
        self.error = None;
        self.loopback = loopback;
        self.muted = true;
        self.deafened = false;
        self.sent = 0;
        self.received = 0;
        self.state = client::Snapshot::default();
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
                self.error = Some(format!("Cannot start audio worker: {error:#}"));
                Task::none()
            }
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        let _trace = profiler::scope(profiler::Category::Ui, "ui.update");
        match message {
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
                let saved = self.edited_settings();
                let result = saved
                    .validate()
                    .and_then(|_| self.configure_shortcuts(&saved.keybinds));
                if let Err(error) = result {
                    self.settings_error = Some(format!("{error:#}"));
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
                        self.settings_error = None;
                        self.settings_message = Some("Saved settings.json".into());
                    }
                    Err(error) => {
                        self.settings_error = Some(format!(
                            "Applied for this session, but not saved: {error:#}"
                        ))
                    }
                }
            }
            Message::ReloadSettings => {
                if self.session.is_some() {
                    return Task::none();
                }
                self.settings_message = None;
                let result = self
                    .settings_path
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("Settings path is unavailable"))
                    .and_then(settings::load_or_create);
                match result {
                    Ok(saved) => match self.configure_shortcuts(&saved.keybinds) {
                        Ok(()) => {
                            self.apply_settings(saved);
                            self.settings_error = None;
                            self.settings_message = Some("Reloaded settings.json".into());
                        }
                        Err(error) => self.settings_error = Some(format!("{error:#}")),
                    },
                    Err(error) => self.settings_error = Some(format!("{error:#}")),
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
            Message::ShowTab(tab) => self.tab = tab,
            Message::RecordTrace => {
                if self.recording.is_none()
                    && !self.trace_saving
                    && !self.performance_mode.load(Ordering::Relaxed)
                {
                    self.trace_error = None;
                    self.trace_path = None;
                    self.trace_summary = None;
                    match profiler::Recording::start() {
                        Ok(recording) => self.recording = Some(recording),
                        Err(error) => self.trace_error = Some(error.to_string()),
                    }
                }
            }
            Message::StopTrace => return self.stop_trace(),
            Message::TraceSaved(result) => {
                self.trace_saving = false;
                match result {
                    Ok(path) => self.trace_path = Some(path),
                    Err(error) => self.trace_error = Some(error),
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
                            self.error = None;
                        }
                        Err(error) => {
                            self.error = Some(format!("Device detection failed: {error:#}"))
                        }
                    }
                }
            }
            Message::Connect => return self.start(false),
            Message::TestAudio => return self.start(true),
            Message::Stop => {
                self.command(client::Command::Stop);
                self.stopping = true;
                self.status = "Disconnecting…".into();
            }
            Message::Mute => self.toggle_mute(),
            Message::Deafen => self.toggle_deafen(),
            Message::Join(channel) => {
                self.error = None;
                self.command(client::Command::Join(
                    channel.id,
                    self.channel_password.clone(),
                ));
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
                    self.status = status;
                    self.connected = false;
                    self.network_usage.stop();
                    self.connection_usage = usage::ConnectionUsage::default();
                }
                client::Event::Connected(detail) => {
                    self.connected = true;
                    self.detail = detail;
                    self.status = if self.loopback {
                        "Local microphone test — only you can hear this"
                    } else {
                        "Connected — microphone starts muted"
                    }
                    .into();
                }
                client::Event::State(state) => self.state = state,
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
                client::Event::Warning(error) => self.error = Some(error),
                client::Event::Usage {
                    network,
                    connection,
                } => {
                    self.network_usage = network;
                    self.connection_usage = connection;
                }
                client::Event::Stopped(error) => {
                    self.session = None;
                    self.connected = false;
                    self.stopping = false;
                    self.level = 0.0;
                    self.audio_buffer_bytes = 0;
                    self.detail.clear();
                    self.status = "Disconnected".into();
                    self.network_usage.stop();
                    self.connection_usage = usage::ConnectionUsage::default();
                    if error.is_some() {
                        self.error = error;
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
        if self.performance_mode.load(Ordering::Relaxed) {
            return self.performance_view();
        }
        let tabs = row![
            button("Voice")
                .on_press(Message::ShowTab(Tab::Voice))
                .style(if self.tab == Tab::Voice {
                    button::primary
                } else {
                    button::secondary
                }),
            button("Statistics")
                .on_press(Message::ShowTab(Tab::Statistics))
                .style(if self.tab == Tab::Statistics {
                    button::primary
                } else {
                    button::secondary
                }),
            button("Settings")
                .on_press(Message::ShowTab(Tab::Settings))
                .style(if self.tab == Tab::Settings {
                    button::primary
                } else {
                    button::secondary
                }),
        ]
        .spacing(10);
        column![
            container(
                row![
                    tabs,
                    iced::widget::space::horizontal(),
                    self.performance_switch(false)
                ]
                .align_y(iced::Alignment::Center)
            )
            .padding([12, 28]),
            match self.tab {
                Tab::Voice => self.voice_view(),
                Tab::Statistics => self.statistics_view(),
                Tab::Settings => self.settings_view(),
            }
        ]
        .height(Length::Fill)
        .width(Length::Fill)
        .into()
    }

    fn performance_switch(&self, enabled: bool) -> Element<'_, Message> {
        checkbox(enabled)
            .label("Performance mode")
            .on_toggle(Message::PerformanceMode)
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

    fn shortcut_hint(&self) -> Element<'_, Message> {
        if let Some(error) = &self.shortcut_error {
            text(format!("Global shortcuts unavailable: {error}"))
                .size(11)
                .color(iced::Color::from_rgb8(255, 130, 150))
                .into()
        } else {
            text(format!(
                "Global: {} · mute/unmute   |   {} · deafen/undeafen",
                self.keybinds.toggle_mute, self.keybinds.toggle_deafen
            ))
            .size(11)
            .into()
        }
    }

    fn settings_view(&self) -> Element<'_, Message> {
        let idle = self.session.is_none();
        let mut content = column![
            text("Settings").size(28),
            text("Save server, nickname, playback volume and global keybinds in one portable JSON file.").size(13),
            icons::label(Icon::Server, "Server address", 13),
            text_input("hostname:9987 or IP address", &self.address).on_input_maybe(idle.then_some(Message::Address)).padding(10),
            text("Nickname").size(13),
            text_input("Nickname", &self.nickname).on_input_maybe(idle.then_some(Message::Nickname)).padding(10),
            text(format!("Playback volume: {}% · adjust below Playback on the Voice tab", self.playback_volume.percent())).size(13),
            rule::horizontal(1),
            text("Global keybinds").size(20),
            text("Type a combination such as Ctrl+Shift+M, Ctrl+Alt+D or F8, then Save. These work while another app is focused.").size(13),
            text("Toggle microphone mute").size(13),
            text_input("Ctrl+Shift+M", &self.keybind_draft.toggle_mute).on_input(Message::MuteKeybind).padding(10),
            text("Toggle deafen").size(13),
            text_input("Ctrl+Shift+D", &self.keybind_draft.toggle_deafen).on_input(Message::DeafenKeybind).padding(10),
            self.shortcut_hint(),
            row![
                button("Save settings").on_press(Message::SaveSettings),
                button("Reload JSON").on_press_maybe(idle.then_some(Message::ReloadSettings)).style(button::secondary),
                button("Copy settings path").on_press_maybe(self.settings_path.as_ref().map(|_| Message::CopySettingsPath)).style(button::secondary),
            ].spacing(10),
            text("Save applies the keybinds and writes the current values. After editing or copying the file externally, disconnect and Reload JSON, or restart the app.").size(12),
        ].spacing(10);
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
        if let Some(error) = &self.settings_error {
            content = content.push(
                text(error)
                    .size(13)
                    .color(iced::Color::from_rgb8(255, 130, 150)),
            );
        }
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
        let mut info = column![
            text(format!("Connection: {connection}")),
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
                16
            ),
            icons::label(Icon::Channel, format!("Channel: {channel}"), 16),
            icons::label(
                self.microphone_icon(),
                format!("Microphone: {}", self.microphone_status()),
                16
            ),
            icons::label(self.playback_icon(), format!("Playback: {playback}"), 16),
        ]
        .spacing(8);
        if let Some(error) = &self.error {
            info = info.push(text(error).color(iced::Color::from_rgb8(255, 130, 150)));
        }
        if let Some(error) = &self.trace_error {
            info = info.push(text(format!("Trace: {error}")));
        }
        let cpu = self.process_usage.cpu_percent.map_or_else(
            || "measuring…".into(),
            |value| usage::format_cpu_percent(f64::from(value)),
        );
        let ram = usage::format_mib(self.process_usage.memory_bytes);
        let memory_detail = usage::format_mib(self.process_usage.memory_detail_bytes);
        column![
            self.performance_switch(true),
            scrollable(info).height(Length::Fill),
            column![
                icons::label(Icon::Cpu, format!("CPU: {cpu}"), 14),
                row![
                    icons::label(Icon::Memory, format!("RAM (RSS): {ram}"), 14).width(Length::Fill),
                    text(format!("{}: {memory_detail}", usage::MEMORY_DETAIL_LABEL))
                        .size(14)
                        .width(Length::Fill),
                ]
                .spacing(12),
                text(format!(
                    "Updates every {} s · CPU: 100% = one core",
                    usage::COMPACT_REFRESH_INTERVAL.as_secs()
                ))
                .size(11),
            ]
            .spacing(4),
            self.shortcut_hint(),
        ]
        .spacing(16)
        .padding(20)
        .height(Length::Fill)
        .into()
    }

    fn voice_view(&self) -> Element<'_, Message> {
        let busy = self.session.is_some();
        let volume = self.playback_volume.percent();
        let mut content = column![
            text("NATIVE VOICE")
                .size(13)
                .color(iced::Color::from_rgb8(124, 161, 255)),
            text("A place to talk.").size(32),
            text("TeamSpeak voice · native Rust client").size(15),
            rule::horizontal(1),
            row![
                column![
                    icons::label(Icon::Server, "Server", 13),
                    text_input("hostname:9987", &self.address)
                        .on_input_maybe((!busy).then_some(Message::Address))
                        .padding(10)
                ]
                .spacing(6)
                .width(Length::FillPortion(2)),
                column![
                    text("Nickname").size(13),
                    text_input("Your name", &self.nickname)
                        .on_input_maybe((!busy).then_some(Message::Nickname))
                        .padding(10)
                ]
                .spacing(6)
                .width(Length::Fill),
            ]
            .spacing(16),
            column![
                text("Server password (optional)").size(13),
                text_input("Password", &self.password)
                    .secure(true)
                    .on_input_maybe((!busy).then_some(Message::Password))
                    .padding(10)
            ]
            .spacing(6),
            row![
                column![
                    icons::label(Icon::Mic, "Microphone", 13),
                    pick_list(
                        self.devices.inputs.clone(),
                        self.input.as_ref(),
                        Message::Input
                    )
                    .placeholder("No microphone detected")
                    .width(Length::Fill)
                    .padding(10)
                ]
                .spacing(6)
                .width(Length::Fill),
                column![
                    icons::label(self.playback_icon(), "Playback", 13),
                    pick_list(
                        self.devices.outputs.clone(),
                        self.output.as_ref(),
                        Message::Output
                    )
                    .placeholder("No playback device detected")
                    .width(Length::Fill)
                    .padding(10),
                    icons::label(if volume == 0 { Icon::VolumeOff } else { Icon::Volume }, format!("Volume · {volume}% · save in Settings"), 12),
                    slider(0..=100, volume, Message::PlaybackVolume).step(1),
                ]
                .spacing(6)
                .width(Length::Fill),
            ]
            .spacing(16),
            row![
                button("Refresh devices")
                    .on_press_maybe((!busy).then_some(Message::Refresh))
                    .style(button::secondary),
                button(icons::label(Icon::Mic, "Test microphone locally", 16))
                    .on_press_maybe((!busy).then_some(Message::TestAudio))
                    .style(button::secondary)
            ]
            .spacing(10),
            text("Headphones recommended: echo cancellation is not included yet. Reconnect to change devices.")
                .size(12),
            rule::horizontal(1),
        ]
        .spacing(14);
        if busy {
            content = content.push(
                row![
                    button(if self.stopping {
                        "Stopping…"
                    } else if self.loopback {
                        "Stop audio test"
                    } else {
                        "Disconnect"
                    })
                    .on_press_maybe((!self.stopping).then_some(Message::Stop))
                    .style(button::danger),
                    button(icons::label(
                        self.microphone_icon(),
                        if self.muted {
                            "Unmute microphone"
                        } else {
                            "Mute microphone"
                        },
                        16
                    ))
                    .on_press_maybe(
                        (self.connected && !self.loopback && !self.stopping)
                            .then_some(Message::Mute)
                    ),
                    button(icons::label(
                        self.playback_icon(),
                        if self.deafened {
                            "Enable playback"
                        } else {
                            "Deafen"
                        },
                        16
                    ))
                    .on_press_maybe(
                        (self.connected && !self.loopback && !self.stopping)
                            .then_some(Message::Deafen)
                    )
                    .style(button::secondary),
                ]
                .spacing(10),
            );
        } else {
            content = content.push(
                button(icons::label(Icon::Server, "Connect to server", 16))
                    .on_press(Message::Connect)
                    .padding([12, 22]),
            );
        }
        content = content
            .push(self.shortcut_hint())
            .push(text(&self.status).size(15));
        if let Some(error) = &self.error {
            content = content.push(
                container(text(error).size(14))
                    .padding(12)
                    .style(container::danger),
            );
        }
        if self.connected {
            content = content.push(text(&self.detail).size(12)).push(
                row![
                    icons::label(self.microphone_icon(), "Microphone", 13),
                    progress_bar(0.0..=1.0, self.level.sqrt().min(1.0)).girth(8)
                ]
                .spacing(12)
                .align_y(iced::Alignment::Center),
            );
            if !self.loopback {
                content = content
                    .push(
                        text(format!(
                            "Audio packets  ↑ {}  ↓ {}",
                            self.sent, self.received
                        ))
                        .size(12),
                    )
                    .push(rule::horizontal(1))
                    .push(icons::label(Icon::Server, &self.state.server, 20))
                    .push(
                        row![
                            Icon::Channel.view(18),
                            pick_list(
                                self.state.channels.clone(),
                                self.state.current.as_ref(),
                                Message::Join
                            )
                            .placeholder("Choose a channel")
                            .width(Length::FillPortion(2))
                            .padding(10),
                            text_input("Channel password (optional)", &self.channel_password)
                                .secure(true)
                                .on_input(Message::ChannelPassword)
                                .width(Length::Fill)
                                .padding(10)
                        ]
                        .spacing(12)
                        .align_y(iced::Alignment::Center),
                    );
                if !self.muted && !self.deafened && !self.state.can_talk {
                    content = content.push(text("The server is not allowing speech in this channel. Try another channel or check talk permissions.").size(13));
                }
                let members = self.state.members.iter().fold(
                    column![icons::label(Icon::Channel, "IN THIS CHANNEL", 12)].spacing(7),
                    |list, member| list.push(text(member).size(14)),
                );
                content = content.push(members);
            }
        }
        container(scrollable(content.padding(28).max_width(920)).height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
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
            button(text(format!(
                "Stop & save trace · {} s",
                recording.started.elapsed().as_secs()
            )))
            .on_press(Message::StopTrace)
            .style(button::danger)
        } else {
            button(if self.trace_saving {
                "Saving trace…"
            } else {
                "Record 30 s trace"
            })
            .on_press_maybe((!self.trace_saving).then_some(Message::RecordTrace))
        });
        if let Some(error) = &self.trace_error {
            content = content.push(text(error).color(iced::Color::from_rgb8(255, 130, 150)));
        }
        if let Some(path) = &self.trace_path {
            content = content
                .push(
                    row![
                        text(format!(
                            "Saved {}",
                            path.file_name().unwrap_or_default().to_string_lossy()
                        ))
                        .size(12),
                        button("Copy trace path")
                            .on_press(Message::CopyTracePath)
                            .style(button::secondary)
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
                self.cpu_history.buffer_bytes() + self.ram_history.buffer_bytes(),
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
        let ping = match (quality.ping_ms, quality.ping_deviation_ms) {
            (Some(ping), Some(variation)) => format!("{ping:.0} ms ± {variation:.1}"),
            _ => "—".into(),
        };
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
    .width(Length::Fill)
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
        assert!(app.error.is_some());
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
    fn compact_microphone_status_accounts_for_deafen_permissions_and_disconnect() {
        let mut app = App::with_devices(audio::Devices::default(), None);
        assert_eq!(app.microphone_status(), "Inactive");
        assert_eq!(app.microphone_icon(), Icon::MicOff);
        assert_eq!(app.playback_icon(), Icon::Headphones);
        app.connected = true;
        assert_eq!(app.microphone_status(), "Muted");
        assert_eq!(app.microphone_icon(), Icon::MicOff);
        app.muted = false;
        assert_eq!(app.microphone_status(), "Blocked by server");
        assert_eq!(app.microphone_icon(), Icon::MicOff);
        app.state.can_talk = true;
        assert_eq!(app.microphone_status(), "Unmuted");
        assert_eq!(app.microphone_icon(), Icon::Mic);
        app.deafened = true;
        assert_eq!(app.microphone_status(), "Muted (deafened)");
        assert_eq!(app.microphone_icon(), Icon::MicOff);
        assert_eq!(app.playback_icon(), Icon::HeadphoneOff);
        app.loopback = true;
        assert_eq!(app.microphone_status(), "Local test only");
        assert_eq!(app.microphone_icon(), Icon::Mic);
        app.stopping = true;
        assert_eq!(app.microphone_status(), "Inactive");
        assert_eq!(app.microphone_icon(), Icon::MicOff);
        let _ = app.update(Message::Backend(client::Event::Stopped(None)));
        assert_eq!(app.microphone_status(), "Inactive");
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
