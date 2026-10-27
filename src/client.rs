use crate::audio::{AudioEngine, DeviceChoice, PlaybackVolume};
use crate::profiler::{self, Category};
use crate::usage::{self, ConnectionUsage, NetworkUsage, ServerUsage, TrafficMeter};
use anyhow::{Context, Result, bail};
use futures::StreamExt;
use std::{
    fmt,
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::mpsc;
use tsclientlib::{
    ChannelId, Codec, Connection, DisconnectOptions, Identity, MessageHandle, StreamItem,
    events::{Event as BookEvent, PropertyId},
    prelude::*,
};
use tsproto_packets::packets::{AudioData, CodecType, OutAudio};

pub struct Config {
    pub performance_mode: Arc<AtomicBool>,
    pub playback_volume: Arc<PlaybackVolume>,
    pub address: String,
    pub nickname: String,
    pub password: String,
    pub input: DeviceChoice,
    pub output: DeviceChoice,
}

#[derive(Debug)]
pub enum Command {
    Stop,
    Mute(bool),
    Deafen(bool),
    Join(u64, String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Channel {
    pub id: u64,
    pub name: String,
}
impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}  ·  #{}", self.name, self.id)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub server: String,
    pub channels: Vec<Channel>,
    pub current: Option<Channel>,
    pub members: Vec<String>,
    pub can_talk: bool,
}

#[derive(Debug, Clone)]
pub enum Event {
    Status(String),
    Connected(String),
    State(Snapshot),
    Meter {
        level: f32,
        sent: u64,
        received: u64,
        audio_buffer_bytes: usize,
    },
    Usage {
        network: NetworkUsage,
        connection: ConnectionUsage,
    },
    Warning(String),
    Stopped(Option<String>),
}

pub fn start(
    config: Config,
    loopback: bool,
) -> Result<(mpsc::Sender<Command>, mpsc::Receiver<Event>)> {
    let (commands, receiver) = mpsc::channel(16);
    let (events, output) = mpsc::channel(64);
    std::thread::Builder::new()
        .name("voice-session".into())
        .spawn(move || {
            let result = (|| -> Result<()> {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?;
                runtime.block_on(async {
                    if loopback {
                        run_loopback(config, receiver, &events).await
                    } else {
                        run_connection(config, receiver, &events).await
                    }
                })
            })();
            let _ = events.blocking_send(Event::Stopped(
                result.err().map(|error| format!("{error:#}")),
            ));
        })?;
    Ok((commands, output))
}

pub fn identity_path() -> Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("io", "Venator", "TeamSpeakNative")
        .context("Cannot locate your application data directory")?;
    Ok(dirs.data_local_dir().join("identity.txt"))
}

fn load_identity() -> Result<Identity> {
    let path = identity_path()?;
    match fs::read_to_string(&path) {
        Ok(value) => {
            return Identity::new_from_ts_str(value.trim())
                .context("Saved TeamSpeak identity is invalid; back it up before replacing it");
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("Cannot read saved TeamSpeak identity"),
    }
    fs::create_dir_all(path.parent().context("Identity path has no parent")?)?;
    let identity = Identity::create();
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(&path) {
        Ok(mut file) => {
            write!(file, "{}V{}", identity.counter(), identity.key().to_ts())?;
            file.sync_all()?;
            Ok(identity)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            Identity::new_from_ts_str(fs::read_to_string(&path)?.trim()).map_err(Into::into)
        }
        Err(error) => Err(error).context("Cannot save TeamSpeak identity"),
    }
}

fn snapshot(connection: &Connection) -> Result<Snapshot> {
    let state = connection.get_state()?;
    let current_id = state
        .clients
        .get(&state.own_client)
        .map(|client| client.channel);
    let mut channels: Vec<_> = state
        .channels
        .values()
        .map(|c| Channel {
            id: c.id.0,
            name: c.name.clone(),
        })
        .collect();
    channels.sort_by_key(|c| c.id);
    let current = channels
        .iter()
        .find(|c| Some(ChannelId(c.id)) == current_id)
        .cloned();
    let mut members: Vec<_> = state
        .clients
        .values()
        .filter(|c| Some(c.channel) == current_id)
        .map(|c| {
            format!(
                "{}{}{}",
                c.name,
                if c.id == state.own_client {
                    " (you)"
                } else {
                    ""
                },
                if c.input_muted { " · muted" } else { "" }
            )
        })
        .collect();
    members.sort();
    Ok(Snapshot {
        server: state.server.name.clone(),
        channels,
        current,
        members,
        can_talk: connection.can_send_audio(),
    })
}

fn update_codec(connection: &Connection, audio: &mut AudioEngine) -> Result<()> {
    let state = connection.get_state()?;
    if let Some(client) = state.clients.get(&state.own_client)
        && let Some(channel) = state.channels.get(&client.channel)
    {
        let codec = match channel.codec {
            Codec::OpusVoice => CodecType::OpusVoice,
            Codec::OpusMusic => CodecType::OpusMusic,
            _ => bail!(
                "The current channel uses a legacy codec. This prototype requires an Opus channel."
            ),
        };
        audio.set_codec(codec, channel.codec_quality.unwrap_or(7))?;
    }
    Ok(())
}

async fn close(connection: &mut Connection) {
    // A failed connection future must not be polled again during cleanup.
    if connection.get_state().is_err() {
        return;
    }
    let _ = connection.disconnect(DisconnectOptions::new());
    let _ = tokio::time::timeout(Duration::from_secs(2), async {
        while connection.events().next().await.is_some() {}
    })
    .await;
}

fn connection_error(error: tsclientlib::Error) -> anyhow::Error {
    if let tsclientlib::Error::ConnectFailed { address, errors } = &error
        && errors.is_empty()
    {
        return anyhow::anyhow!(
            "Could not resolve '{address}'. Check the hostname or enter an IP address and UDP port (usually 9987)."
        );
    }
    error.into()
}

async fn run_connection(
    config: Config,
    mut commands: mpsc::Receiver<Command>,
    events: &mpsc::Sender<Event>,
) -> Result<()> {
    let _ = events.try_send(Event::Status(
        "Opening audio devices — allow microphone access if prompted…".into(),
    ));
    // Native permission dialogs may block device initialization. Finish them
    // before connecting so they cannot stall TeamSpeak's network event loop.
    let mut audio = AudioEngine::open(
        &config.input,
        &config.output,
        config.playback_volume.clone(),
    )?;
    if commands.is_closed()
        || events.is_closed()
        || matches!(commands.try_recv(), Ok(Command::Stop))
    {
        return Ok(());
    }
    let _ = events.try_send(Event::Status("Resolving server and connecting…".into()));
    let mut connection = Connection::build(config.address)
        .name(config.nickname)
        .identity(load_identity()?)
        .password(config.password)
        .input_muted(true)
        .connect()?;
    let mut traffic = TrafficMeter::new(Instant::now());
    let result = async {
        let mut muted = true;
        let mut deafened = false;
        let mut connected = false;
        let mut deadline = Instant::now();
        let mut sent = 0;
        let mut received = 0;
        let mut last_meter = Instant::now();
        let mut last_usage = Instant::now();
        let mut last_server_request = Instant::now() - usage::SERVER_REFRESH_INTERVAL;
        let mut server_usage: Option<ServerUsage> = None;
        let mut pending_info: Option<(MessageHandle, Instant)> = None;
        let mut server_info_enabled = true;
        let mut last_voice = None;
        let mut tick = tokio::time::interval(Duration::from_millis(10));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        enum Step { Network(Option<Result<StreamItem, tsclientlib::Error>>), Command(Option<Command>), Tick }
        loop {
            let step = {
                let mut stream = connection.events();
                let next_event = futures::future::poll_fn(|context| {
                    // Time individual polls, not the wait between network events.
                    let _trace = profiler::scope(Category::Network, "network.poll");
                    stream.poll_next_unpin(context)
                });
                tokio::select! {
                    command = commands.recv() => Step::Command(command),
                    event = next_event => Step::Network(event),
                    _ = tick.tick() => Step::Tick,
                }
            };
            match step {
                Step::Command(None | Some(Command::Stop)) => break,
                Step::Command(Some(command)) => {
                    if !connected { continue; }
                    let state = connection.get_state()?;
                    match command {
                        Command::Mute(value) => {
                            muted = value;
                            if value { audio.set_capture(false); }
                            state.client_update().set_input_muted(value).send_with_result(&mut connection)?;
                        },
                        Command::Deafen(value) => {
                            deafened = value;
                            audio.clear_playback();
                            audio.set_capture(false);
                            state.client_update().set_output_muted(value).send_with_result(&mut connection)?;
                        },
                        Command::Join(id, password) => {
                            if let Some(client) = state.clients.get(&state.own_client) {
                                client.client_move(ChannelId(id)).set_password(&password).send_with_result(&mut connection)?;
                            }
                        },
                        Command::Stop => break,
                    }
                    let _ = events.try_send(Event::State(snapshot(&connection)?));
                },
                Step::Network(Some(Ok(event))) => match event {
                    StreamItem::BookEvents(changes) => {
                        if !connected {
                            connected = true;
                            traffic.reset_transport(Instant::now());
                            last_usage = Instant::now();
                            server_info_enabled = true;
                            last_server_request = Instant::now() - usage::SERVER_REFRESH_INTERVAL;
                            let _ = events.try_send(Event::Connected(audio.description.clone()));
                            connection.get_state()?.client_update().set_input_muted(muted).set_output_muted(deafened).send_with_result(&mut connection)?;
                        }
                        let state = connection.get_state()?;
                        if changes.iter().any(|change| matches!(change,
                            BookEvent::PropertyAdded { id: PropertyId::ConnectionClientData(id), .. }
                            | BookEvent::PropertyChanged { id: PropertyId::ConnectionClientData(id), .. }
                            if *id == state.own_client))
                            && let Some(data) = state.clients.get(&state.own_client).and_then(|client| client.connection_data.as_ref()) {
                            server_usage = Some(ServerUsage::new(data, Instant::now()));
                        }
                        update_codec(&connection, &mut audio)?;
                        let _ = events.try_send(Event::State(snapshot(&connection)?));
                    },
                    StreamItem::Audio(packet) => {
                        received += 1;
                        if !deafened && connection.can_receive_audio() { audio.receive(packet); }
                    },
                    StreamItem::IdentityLevelIncreasing(level) => { let _ = events.try_send(Event::Status(format!("Preparing server identity (security level {level})…"))); },
                    StreamItem::DisconnectedTemporarily(_) => {
                        connected = false;
                        deadline = Instant::now();
                        audio.set_capture(false);
                        audio.clear_playback();
                        server_usage = None;
                        pending_info = None;
                        last_voice = None;
                        let network = traffic.reset_transport(Instant::now());
                        let _ = events.try_send(Event::Usage { network, connection: ConnectionUsage::default() });
                        let _ = events.try_send(Event::Status("Connection lost. Reconnecting…".into()));
                    },
                    StreamItem::MessageResult(handle, result) => {
                        if pending_info.is_some_and(|(pending, _)| pending == handle) {
                            pending_info = None;
                            if result.is_err() { server_info_enabled = false; server_usage = None; }
                        } else if let Err(error) = result { let _ = events.try_send(Event::Warning(format!("Server: {error}"))); }
                    },
                    StreamItem::AudioChange(_) => { let _ = events.try_send(Event::State(snapshot(&connection)?)); },
                    _ => {},
                },
                Step::Network(Some(Err(error))) => return Err(connection_error(error)).context("TeamSpeak connection failed"),
                Step::Network(None) => bail!("The server closed the connection."),
                Step::Tick => {
                    if events.is_closed() { break; }
                    if !connected && deadline.elapsed() > Duration::from_secs(40) { bail!("Connection timed out. Check the address, UDP port, password, and server availability."); }
                    {
                        let can_send = connected && !muted && !deafened && connection.can_send_audio();
                        audio.set_capture(can_send);
                        let packets = audio.tick(false, deafened || !connected)?;
                        if can_send {
                            for packet in packets {
                                let _trace = profiler::scope(Category::Network, "network.send_voice");
                                connection.send_audio(OutAudio::new(&AudioData::C2S { id: 0, codec: audio.codec, data: &packet }))?;
                                sent += 1;
                                last_voice = Some(Instant::now());
                            }
                        }
                        let now = Instant::now();
                        let show_telemetry = !config.performance_mode.load(Ordering::Relaxed);
                        if show_telemetry && now.duration_since(last_usage) >= usage::REFRESH_INTERVAL {
                            if let Ok(stats) = connection.get_network_stats() {
                                let network = traffic.sample(&stats.total_bytes, now);
                                let quality = usage::connection_usage(stats, server_usage.as_ref(), last_voice, now);
                                let _ = events.try_send(Event::Usage { network, connection: quality });
                            }
                            last_usage = now;
                        }
                        // Ask only for our own stats, and stop polling if the server
                        // denies or times out rather than repeatedly triggering errors.
                        if pending_info.is_some_and(|(_, at)| now.duration_since(at) > Duration::from_secs(10)) {
                            pending_info = None;
                            server_info_enabled = false;
                            server_usage = None;
                        }
                        if show_telemetry && connected && server_info_enabled && pending_info.is_none()
                            && now.duration_since(last_server_request) >= usage::SERVER_REFRESH_INTERVAL
                            && let Ok(state) = connection.get_state()
                            && let Some(client) = state.clients.get(&state.own_client) {
                            match client.connection_info().send_with_result(&mut connection) {
                                Ok(handle) => pending_info = Some((handle, now)),
                                Err(_) => server_info_enabled = false,
                            }
                            last_server_request = now;
                        }
                        if show_telemetry && last_meter.elapsed() >= Duration::from_millis(100) {
                            let _ = events.try_send(Event::Meter { level: audio.level(), sent, received, audio_buffer_bytes: audio.buffer_bytes() });
                            last_meter = Instant::now();
                        }
                    }
                },
            }
        }
        Ok(())
    }.await;
    close(&mut connection).await;
    if let Ok(stats) = connection.get_network_stats() {
        traffic.sample(&stats.total_bytes, Instant::now());
    }
    let _ = events.try_send(Event::Usage {
        network: traffic.reset_transport(Instant::now()),
        connection: ConnectionUsage::default(),
    });
    result
}

async fn run_loopback(
    config: Config,
    mut commands: mpsc::Receiver<Command>,
    events: &mpsc::Sender<Event>,
) -> Result<()> {
    let mut audio = AudioEngine::open(
        &config.input,
        &config.output,
        config.playback_volume.clone(),
    )?;
    audio.set_capture(true);
    let _ = events.try_send(Event::Connected(audio.description.clone()));
    let mut tick = tokio::time::interval(Duration::from_millis(10));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_meter = Instant::now();
    loop {
        tokio::select! {
            command = commands.recv() => if matches!(command, None | Some(Command::Stop)) { break; },
            _ = tick.tick() => {
                if events.is_closed() { break; }
                audio.tick(true, false)?;
                if !config.performance_mode.load(Ordering::Relaxed)
                    && last_meter.elapsed() >= Duration::from_millis(100) {
                    let _ = events.try_send(Event::Meter { level: audio.level(), sent: 0, received: 0, audio_buffer_bytes: audio.buffer_bytes() });
                    last_meter = Instant::now();
                }
            }
        }
    }
    Ok(())
}

/// Network-only check: never opens audio devices or transmits microphone audio.
pub fn probe(address: &str) -> Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let mut connection = Connection::build(address.to_owned())
                .name("Native Voice Check")
                .identity(load_identity()?)
                .input_muted(true)
                .output_muted(true)
                .connect()?;
            let result = tokio::time::timeout(Duration::from_secs(40), async {
                loop {
                    let event = connection.events().next().await;
                    match event {
                        Some(Ok(StreamItem::BookEvents(_))) => {
                            let state = snapshot(&connection)?;
                            let Some(current) = state.current else {
                                continue;
                            };
                            println!("Connected to: {}", state.server);
                            println!("Visible channels: {}", state.channels.len());
                            println!("Channel: {}", current.name);
                            println!("Probe is muted and deafened. No microphone was opened.");
                            return Ok::<_, anyhow::Error>(());
                        }
                        Some(Ok(_)) => {}
                        Some(Err(error)) => return Err(connection_error(error)),
                        None => bail!("Connection closed before initialization"),
                    }
                }
            })
            .await
            .context("Connection probe timed out")?;
            close(&mut connection).await;
            result
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancelling_an_unconnected_session_does_not_drive_the_handshake() -> Result<()> {
        let address: std::net::SocketAddr = "127.0.0.1:0".parse()?;
        let mut connection = Connection::build(address)
            .identity(Identity::create())
            .connect()?;
        tokio::time::timeout(Duration::from_millis(50), close(&mut connection)).await?;
        Ok(())
    }

    #[test]
    fn identity_roundtrip_keeps_the_same_public_identity() -> Result<()> {
        let identity = Identity::create();
        let encoded = format!("{}V{}", identity.counter(), identity.key().to_ts());
        let restored = Identity::new_from_ts_str(&encoded)?;
        assert_eq!(
            identity.key().to_pub().to_ts(),
            restored.key().to_pub().to_ts()
        );
        assert_eq!(identity.counter(), restored.counter());
        Ok(())
    }
}
