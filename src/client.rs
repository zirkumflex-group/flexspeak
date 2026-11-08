use crate::audio::{AudioEngine, DeviceChoice, PlaybackVolume};
use crate::hub::{self, AudioHub, Link, SessionId, Voice};
use crate::profiler::{self, Category};
use crate::usage::{self, ConnectionUsage, NetworkUsage, ServerUsage, TrafficMeter};
use crate::{
    alerts::Source,
    command_flow::{AudioState, Controls, Flow, Operation, Ticket},
    streaming,
};
use anyhow::{Context, Result, bail};
use futures::StreamExt;
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::mpsc;
use tsclientlib::{
    ChannelGroupId, ChannelId, ChannelPermissionHint, ClientDbId, ClientId, ClientPermissionHint,
    Codec, Connection, DisconnectOptions, GroupType, Identity, ServerGroupId, StreamItem,
    events::{Event as BookEvent, PropertyId, PropertyValue},
    messages::c2s,
    prelude::*,
};
use tsproto_packets::packets::{AudioData, CodecType, InAudioBuf, OutAudio, OutCommand};

/// How loud to play each person, by identity. Anyone absent plays at full volume.
pub type UserGains = Arc<Mutex<HashMap<String, f32>>>;

/// A TeamSpeak identity as text, for settings and lookups.
pub fn uid_key(uid: &[u8]) -> String {
    uid.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub struct Config {
    pub user_gains: UserGains,
    pub id: SessionId,
    pub hub: Arc<AudioHub>,
    pub channel_health_visible: Arc<AtomicBool>,
    /// See every channel's members (subscribe to all), or only your own channel's.
    pub subscribe_all: Arc<AtomicBool>,
    pub streaming_visible: Arc<AtomicBool>,
    pub video_surfaces: Arc<streaming::Surfaces>,
    pub playback_volume: Arc<PlaybackVolume>,
    pub audio_processing: crate::audio_processing::Settings,
    pub transmission: crate::transmission::Settings,
    pub ptt: Arc<AtomicBool>,
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
    RefreshStreams,
    WatchStream(String),
    StartSharing(streaming::publishing::Options),
    StopSharing,
    DecideViewer(u16, bool),
    /// Stops the stream with this ID, or every watched stream.
    StopWatching(Option<String>),
    /// Pokes a client with a message.
    Poke(u16, String),
    /// Moves these clients into a channel.
    Move(Vec<u16>, u64),
    /// Kicks or bans these clients, with an optional reason.
    Moderate(Vec<u16>, Moderation, String),
    /// Applies one set of group changes to each of these clients.
    SetGroups(Vec<u16>, GroupEdit),
    /// Asks for the group lists, for servers that haven't sent them.
    RequestGroups,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Moderation {
    KickChannel,
    KickServer,
    /// For this many seconds, or forever.
    Ban(Option<u64>),
}

/// Server groups to add and remove, and the channel group to set in each client's channel.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupEdit {
    pub add: Vec<u64>,
    pub remove: Vec<u64>,
    pub channel_group: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum JoinAccess {
    Allowed,
    Denied,
    #[default]
    Unknown,
    UnsupportedCodec,
}

impl JoinAccess {
    pub fn can_request(self) -> bool {
        matches!(self, Self::Allowed | Self::Unknown)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Member {
    pub id: u16,
    /// Their identity as text; empty until the server has sent it.
    pub uid: String,
    pub name: String,
    pub is_self: bool,
    pub mic_muted: bool,
    pub sound_muted: bool,
    /// Whether the server says you may poke and move this client; `None` when it hasn't said.
    pub can_poke: Option<bool>,
    pub can_move: Option<bool>,
    pub can_kick_channel: Option<bool>,
    pub can_kick_server: Option<bool>,
    pub can_ban: Option<bool>,
    /// Sorted server group ids.
    pub server_groups: Vec<u64>,
    pub channel_group: u64,
}

/// A server or channel group someone can be put in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub id: u64,
    pub name: String,
    /// Shown, but nobody can be added to or removed from it.
    pub fixed: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Channel {
    pub id: u64,
    pub name: String,
    pub parent: u64,
    pub order: u64,
    pub depth: usize,
    pub join_access: JoinAccess,
    pub has_password: bool,
    pub members_visible: bool,
    pub members: Vec<Member>,
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub server: String,
    pub server_version: String,
    pub channels: Vec<Channel>,
    pub current: Option<Channel>,
    pub can_talk: bool,
    pub server_groups: Vec<Group>,
    pub channel_groups: Vec<Group>,
}

impl Snapshot {
    pub fn current_members(&self) -> &[Member] {
        self.current
            .as_ref()
            .map_or(&[], |channel| &channel.members)
    }
}

/// A quiet gap longer than this makes the next whisper a new one.
const WHISPER_GAP: Duration = Duration::from_secs(3);

/// What the status says when the connection dropped and is being re-established.
pub const CONNECTION_LOST: &str = "Connection lost. Reconnecting…";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityKind {
    Connected,
    Disconnected,
    LostConnection,
    /// Moved by someone else.
    Moved,
    /// Changed channel themselves.
    Switched,
    KickedFromChannel,
    KickedFromServer,
    Banned,
    RecordingStarted,
    RecordingStopped,
    TalkPowerGranted,
    TalkPowerRevoked,
    Whispered,
    ServerGroupAssigned,
    ServerGroupRevoked,
    ChannelGroupChanged,
}

/// Something a person did on the server, for the log and its sounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Activity {
    pub kind: ActivityKind,
    pub name: String,
    /// The channel it happened in, or the group for a group change.
    pub channel: String,
    pub is_self: bool,
    pub entered_yours: bool,
    pub left_yours: bool,
}

use tsclientlib::Reason;

/// How someone left, or `None` when they only went out of view.
fn leaving(reason: Option<Reason>) -> Option<ActivityKind> {
    match reason {
        Some(Reason::LostConnection) => Some(ActivityKind::LostConnection),
        Some(Reason::KickChannel) => Some(ActivityKind::KickedFromChannel),
        Some(Reason::KickServer) => Some(ActivityKind::KickedFromServer),
        Some(Reason::KickServerBan) => Some(ActivityKind::Banned),
        Some(
            Reason::Moved | Reason::Subscription | Reason::Channelupdate | Reason::Channeledit,
        ) => None,
        _ => Some(ActivityKind::Disconnected),
    }
}

/// How someone arrived, or `None` when they only came into view.
fn arriving(reason: Option<Reason>) -> Option<ActivityKind> {
    match reason {
        None | Some(Reason::None) => Some(ActivityKind::Connected),
        Some(Reason::Moved) => Some(ActivityKind::Moved),
        _ => None,
    }
}

fn activity(state: &tsclientlib::data::Connection, change: &BookEvent) -> Option<Activity> {
    let clean = |text: &str| {
        text.chars()
            .filter(|c| !c.is_control())
            .take(64)
            .collect::<String>()
    };
    let channel_name = |id| {
        state
            .channels
            .get(&id)
            .map_or_else(String::new, |c: &tsclientlib::data::Channel| clean(&c.name))
    };
    let yours = state
        .clients
        .get(&state.own_client)
        .map(|client| client.channel);
    let server_group_name = |group: &ServerGroupId| {
        state
            .server_groups
            .get(group)
            .map_or_else(String::new, |group| clean(&group.name))
    };
    let own_change = |kind, channel| Activity {
        kind,
        name: String::new(),
        channel,
        is_self: true,
        entered_yours: false,
        left_yours: false,
    };
    match change {
        BookEvent::PropertyAdded {
            id: PropertyId::Client(id),
            extra,
            ..
        } if *id != state.own_client => {
            let client = state.clients.get(id)?;
            Some(Activity {
                kind: arriving(extra.reason)?,
                name: clean(&client.name),
                channel: channel_name(client.channel),
                is_self: false,
                entered_yours: Some(client.channel) == yours,
                left_yours: false,
            })
        }
        BookEvent::PropertyChanged {
            id: PropertyId::ClientChannel(id),
            old: PropertyValue::ChannelId(old),
            extra,
            ..
        } => {
            let client = state.clients.get(id)?;
            Some(Activity {
                kind: match extra.reason {
                    Some(Reason::KickChannel) => ActivityKind::KickedFromChannel,
                    Some(Reason::Moved) => ActivityKind::Moved,
                    _ => ActivityKind::Switched,
                },
                name: clean(&client.name),
                channel: channel_name(client.channel),
                is_self: *id == state.own_client,
                entered_yours: Some(client.channel) == yours,
                left_yours: Some(*old) == yours,
            })
        }
        BookEvent::PropertyRemoved {
            id: PropertyId::Client(id),
            old: PropertyValue::Client(client),
            extra,
            ..
        } => Some(Activity {
            kind: leaving(extra.reason)?,
            name: clean(&client.name),
            channel: channel_name(client.channel),
            is_self: *id == state.own_client,
            entered_yours: false,
            left_yours: Some(client.channel) == yours,
        }),
        BookEvent::PropertyChanged {
            id: PropertyId::ClientIsRecording(id),
            old: PropertyValue::Bool(was),
            ..
        } if *id != state.own_client => {
            let client = state.clients.get(id)?;
            (Some(client.channel) == yours).then(|| Activity {
                kind: if *was {
                    ActivityKind::RecordingStopped
                } else {
                    ActivityKind::RecordingStarted
                },
                name: clean(&client.name),
                channel: channel_name(client.channel),
                is_self: false,
                entered_yours: true,
                left_yours: false,
            })
        }
        BookEvent::PropertyChanged {
            id: PropertyId::ClientTalkPowerGranted(id),
            old: PropertyValue::Bool(was),
            ..
        } if *id == state.own_client => Some(own_change(
            if *was {
                ActivityKind::TalkPowerRevoked
            } else {
                ActivityKind::TalkPowerGranted
            },
            String::new(),
        )),
        BookEvent::PropertyAdded {
            id: PropertyId::ClientServerGroup(id, group),
            ..
        } if *id == state.own_client && *group != state.server.default_server_group => Some(
            own_change(ActivityKind::ServerGroupAssigned, server_group_name(group)),
        ),
        BookEvent::PropertyRemoved {
            id: PropertyId::ClientServerGroup(id, group),
            ..
        } if *id == state.own_client && *group != state.server.default_server_group => Some(
            own_change(ActivityKind::ServerGroupRevoked, server_group_name(group)),
        ),
        // Joining a channel also reports your group there, from the server rather than a person.
        BookEvent::PropertyChanged {
            id: PropertyId::ClientChannelGroup(id),
            invoker: Some(invoker),
            ..
        } if *id == state.own_client && invoker.id != ClientId(0) => {
            let group = state.clients.get(id)?.channel_group;
            Some(own_change(
                ActivityKind::ChannelGroupChanged,
                state
                    .channel_groups
                    .get(&group)
                    .map_or_else(String::new, |group| clean(&group.name)),
            ))
        }
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub enum Event {
    /// People arriving, moving and leaving since the last update.
    Activity(Vec<Activity>),
    Status(String),
    Connected(String),
    State(Snapshot),
    /// The clients heard right now, sent whenever the set changes.
    Talking(BTreeSet<u16>),
    /// Whether the microphone gate (push-to-talk or voice activation) is open.
    Transmitting(bool),
    Streaming(streaming::Event),
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
    ClientUsage {
        id: u16,
        result: Result<ServerUsage, &'static str>,
    },
    Warning {
        source: Source,
        detail: String,
    },
    Recovered(Source),
    /// The server refused a command as flooding; `Recovered(Source::Flood)` ends it.
    Limited,
    Controls {
        confirmed: Option<AudioState>,
        pending: bool,
    },
    ChannelPasswordRequired(u64),
    /// The server refused something for lack of permission.
    PermissionDenied,
    /// Someone poked you.
    Poked {
        from: String,
        message: String,
    },
    /// The server accepted a poke or move you asked for.
    Done(Source),
    Stopped(Option<(Source, String)>),
}

#[derive(Debug)]
struct AudioFailure(anyhow::Error);
impl std::fmt::Display for AudioFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for AudioFailure {}

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
            let _ = events.blocking_send(Event::Stopped(result.err().map(|error| {
                let source = if error.downcast_ref::<AudioFailure>().is_some() {
                    Source::Audio
                } else {
                    Source::Connection
                };
                (source, format!("{error:#}"))
            })));
        })?;
    Ok((commands, output))
}

fn report_transmission(events: &mpsc::Sender<Event>, reported: &mut bool, transmitting: bool) {
    if *reported != transmitting && events.try_send(Event::Transmitting(transmitting)).is_ok() {
        *reported = transmitting;
    }
}

pub fn identity_path() -> Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("io", "Venator", "TeamSpeakNative")
        .context("Cannot locate your application data directory")?;
    Ok(dirs.data_local_dir().join("identity.txt"))
}

/// One identity file: two connections starting together must not race to create it.
static IDENTITY_FILE: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn load_identity() -> Result<Identity> {
    let _guard = IDENTITY_FILE.lock().unwrap();
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

/// Who is heard right now. A client talks from its first voice packet until it sends the
/// empty end-of-speech packet, or until no packet arrived for [`Speakers::HOLD`].
#[derive(Debug, Default)]
struct Speakers {
    heard: BTreeMap<u16, Instant>,
    reported: BTreeSet<u16>,
}

impl Speakers {
    /// Voice packets arrive every 20 ms; this rides out jitter without lingering after speech.
    const HOLD: Duration = Duration::from_millis(300);

    fn packet(&mut self, id: u16, end_of_speech: bool, now: Instant) {
        if end_of_speech {
            self.heard.remove(&id);
        } else {
            self.heard.insert(id, now);
        }
    }

    fn clear(&mut self) {
        self.heard.clear();
    }

    /// Hands the talking set to `send` when it differs from the last one delivered; a failed
    /// send is retried on the next call.
    fn report(&mut self, now: Instant, send: impl FnOnce(BTreeSet<u16>) -> bool) {
        self.heard
            .retain(|_, at| now.saturating_duration_since(*at) < Self::HOLD);
        if self.heard.len() == self.reported.len() && self.heard.keys().eq(self.reported.iter()) {
            return;
        }
        let talking: BTreeSet<u16> = self.heard.keys().copied().collect();
        if send(talking.clone()) {
            self.reported = talking;
        }
    }
}

/// How loud the sender of this packet should play.
fn sender_gain(connection: &Connection, gains: &UserGains, packet: &InAudioBuf) -> f32 {
    let gains = gains.lock().unwrap();
    if gains.is_empty() {
        return 1.0;
    }
    let (AudioData::S2C { from, .. } | AudioData::S2CWhisper { from, .. }) = packet.data().data()
    else {
        return 1.0;
    };
    connection
        .get_state()
        .ok()
        .and_then(|state| {
            state
                .clients
                .get(&ClientId(*from))?
                .uid
                .as_ref()
                .map(|uid| uid_key(&uid.0))
        })
        .and_then(|key| gains.get(&key).copied())
        .unwrap_or(1.0)
}

fn snapshot(connection: &Connection) -> Result<Snapshot> {
    let state = connection.get_state()?;
    let current_id = state
        .clients
        .get(&state.own_client)
        .map(|client| client.channel);
    let mut members: BTreeMap<u64, Vec<Member>> = BTreeMap::new();
    for client in state.clients.values() {
        let hint = |flag| client.permission_hints.map(|hints| hints.contains(flag));
        let mut server_groups: Vec<u64> = client.server_groups.iter().map(|id| id.0).collect();
        server_groups.sort_unstable();
        members.entry(client.channel.0).or_default().push(Member {
            id: client.id.0,
            uid: client
                .uid
                .as_ref()
                .map_or_else(String::new, |uid| uid_key(&uid.0)),
            name: client.name.clone(),
            is_self: client.id == state.own_client,
            mic_muted: client.input_muted || client.output_muted || !client.input_hardware_enabled,
            sound_muted: client.output_muted
                || client.output_only_muted
                || !client.output_hardware_enabled,
            can_poke: hint(ClientPermissionHint::POKE),
            can_move: hint(ClientPermissionHint::MOVE_CLIENT),
            can_kick_channel: hint(ClientPermissionHint::KICK_CHANNEL),
            can_kick_server: hint(ClientPermissionHint::KICK_SERVER),
            can_ban: hint(ClientPermissionHint::BAN),
            server_groups,
            channel_group: client.channel_group.0,
        });
    }
    for clients in members.values_mut() {
        clients
            .sort_by_cached_key(|client| (!client.is_self, client.name.to_lowercase(), client.id));
    }
    let channels = order_channels(
        state
            .channels
            .values()
            .map(|c| {
                let members = members.remove(&c.id.0).unwrap_or_default();
                Channel {
                    id: c.id.0,
                    name: c.name.clone(),
                    parent: c.parent.0,
                    order: c.order.0,
                    join_access: channel_join_access(c),
                    has_password: c.has_password.unwrap_or(false),
                    // The channel you just left can still be sent to you while its subscribed
                    // flag reads false; anyone listed is someone you can see.
                    members_visible: c.subscribed
                        || Some(c.id) == current_id
                        || !members.is_empty(),
                    members,
                    depth: 0,
                }
            })
            .collect(),
    );
    let current = channels
        .iter()
        .find(|c| Some(ChannelId(c.id)) == current_id)
        .cloned();
    Ok(Snapshot {
        server: state.server.name.clone(),
        server_version: state.server.version.clone(),
        channels,
        current,
        can_talk: connection.can_send_audio(),
        server_groups: sorted_groups(state.server_groups.values().filter_map(|group| {
            (group.group_type == GroupType::Regular).then_some((
                group.sort_id,
                group.id.0,
                group.name.as_str(),
                // Everyone without another group is in this one; the server won't change that.
                group.id == state.server.default_server_group,
            ))
        })),
        channel_groups: sorted_groups(state.channel_groups.values().filter_map(|group| {
            (group.group_type == GroupType::Regular).then_some((
                group.sort_id,
                group.id.0,
                group.name.as_str(),
                false,
            ))
        })),
    })
}

fn sorted_groups<'a>(groups: impl Iterator<Item = (i32, u64, &'a str, bool)>) -> Vec<Group> {
    let mut groups: Vec<_> = groups.collect();
    groups.sort_unstable();
    groups
        .into_iter()
        .map(|(_, id, name, fixed)| Group {
            id,
            name: name.to_owned(),
            fixed,
        })
        .collect()
}

fn moderation_packet(id: ClientId, action: Moderation, reason: &str) -> OutCommand {
    let message = (!reason.is_empty()).then(|| reason.into());
    let kick = |reason| {
        c2s::OutClientKickMessage::new(&mut std::iter::once(c2s::OutClientKickPart {
            client_id: id,
            reason,
            reason_message: message.clone(),
        }))
    };
    match action {
        Moderation::KickChannel => kick(Reason::KickChannel),
        Moderation::KickServer => kick(Reason::KickServer),
        Moderation::Ban(seconds) => {
            c2s::OutBanClientMessage::new(&mut std::iter::once(c2s::OutBanClientPart {
                client_id: id,
                time: seconds.map(|seconds| time::Duration::seconds(seconds as i64)),
                ban_reason: message.clone(),
            }))
        }
    }
}

/// What it takes to bring this client in line with the edit; already-applied changes are skipped.
fn group_packets(client: &tsclientlib::data::Client, edit: &GroupEdit) -> Vec<OutCommand> {
    let db = ClientDbId(client.database_id.0);
    let held = |id: &u64| client.server_groups.contains(&ServerGroupId(*id));
    let added = edit.add.iter().filter(|id| !held(id)).map(|id| {
        c2s::OutServerGroupAddClientMessage::new(&mut std::iter::once(
            c2s::OutServerGroupAddClientPart {
                server_group_id: ServerGroupId(*id),
                client_db_id: db,
            },
        ))
    });
    let removed = edit.remove.iter().filter(|id| held(id)).map(|id| {
        c2s::OutServerGroupDelClientMessage::new(&mut std::iter::once(
            c2s::OutServerGroupDelClientPart {
                server_group_id: ServerGroupId(*id),
                client_db_id: db,
            },
        ))
    });
    let channel = edit
        .channel_group
        .filter(|id| *id != client.channel_group.0)
        .map(|id| {
            c2s::OutSetClientChannelGroupMessage::new(&mut std::iter::once(
                c2s::OutSetClientChannelGroupPart {
                    channel_group: ChannelGroupId(id),
                    channel_id: client.channel,
                    client_db_id: db,
                },
            ))
        });
    added.chain(removed).chain(channel).collect()
}

fn channel_join_access(channel: &tsclientlib::data::Channel) -> JoinAccess {
    if !matches!(channel.codec, Codec::OpusVoice | Codec::OpusMusic) {
        return JoinAccess::UnsupportedCodec;
    }
    match channel.permission_hints {
        Some(hints) if hints.contains(ChannelPermissionHint::JOIN) => JoinAccess::Allowed,
        Some(_) => JoinAccess::Denied,
        None => JoinAccess::Unknown,
    }
}

#[derive(Default)]
struct PeerPoll {
    last_request: Option<Instant>,
    disabled: bool,
}

#[derive(Default)]
struct ChannelPolls {
    channel: Option<ChannelId>,
    peers: HashMap<ClientId, PeerPoll>,
}

impl ChannelPolls {
    fn sync(&mut self, channel: Option<ChannelId>, ids: impl Iterator<Item = ClientId>) {
        if self.channel != channel {
            self.peers.clear();
            self.channel = channel;
        }
        let ids: HashSet<_> = ids.collect();
        self.peers.retain(|id, _| ids.contains(id));
        for id in ids {
            self.peers.entry(id).or_default();
        }
    }

    fn next_due(&self, now: Instant) -> Option<ClientId> {
        self.peers
            .iter()
            .filter(|(_, peer)| {
                !peer.disabled
                    && peer.last_request.is_none_or(|at| {
                        now.saturating_duration_since(at) >= usage::SERVER_REFRESH_INTERVAL
                    })
            })
            .min_by_key(|(id, peer)| (peer.last_request, id.0))
            .map(|(id, _)| *id)
    }

    fn requested(&mut self, id: ClientId, now: Instant) {
        if let Some(peer) = self.peers.get_mut(&id) {
            peer.last_request = Some(now);
        }
    }

    fn disable(&mut self, id: ClientId, channel: Option<ChannelId>) -> bool {
        if self.channel == channel
            && let Some(peer) = self.peers.get_mut(&id)
        {
            peer.disabled = true;
            true
        } else {
            false
        }
    }
}

fn order_channels(channels: Vec<Channel>) -> Vec<Channel> {
    let mut groups: BTreeMap<u64, Vec<Channel>> = BTreeMap::new();
    for channel in channels {
        groups.entry(channel.parent).or_default().push(channel);
    }
    for siblings in groups.values_mut() {
        let mut remaining: BTreeMap<_, _> = siblings
            .drain(..)
            .map(|channel| (channel.id, channel))
            .collect();
        let mut successors: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
        for channel in remaining.values() {
            successors
                .entry(channel.order)
                .or_default()
                .push(channel.id);
        }
        let mut previous = 0;
        while !remaining.is_empty() {
            let id = successors
                .get(&previous)
                .and_then(|ids| ids.iter().find(|id| remaining.contains_key(id)))
                .copied()
                .unwrap_or_else(|| *remaining.first_key_value().unwrap().0);
            siblings.push(remaining.remove(&id).unwrap());
            previous = id;
        }
    }
    let mut ordered = Vec::new();
    let mut stack: Vec<_> = groups
        .remove(&0)
        .unwrap_or_default()
        .into_iter()
        .rev()
        .map(|channel| (channel, 0))
        .collect();
    while !stack.is_empty() || !groups.is_empty() {
        // Missing parents and malformed cycles still appear once, without recursion.
        if stack.is_empty() {
            let (_, orphaned) = groups.pop_first().unwrap();
            stack.extend(orphaned.into_iter().rev().map(|channel| (channel, 0)));
        }
        let (mut channel, depth) = stack.pop().unwrap();
        if let Some(children) = groups.remove(&channel.id) {
            stack.extend(children.into_iter().rev().map(|child| (child, depth + 1)));
        }
        channel.depth = depth;
        ordered.push(channel);
    }
    ordered
}

fn update_codec(connection: &Connection, link: &Link) -> Result<()> {
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
        link.set_codec(codec, channel.codec_quality.unwrap_or(7));
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
    let mut audio = config
        .hub
        .attach(
            config.id,
            hub::Open {
                input: config.input.clone(),
                output: config.output.clone(),
                volume: config.playback_volume.clone(),
                processing: config.audio_processing.for_session(false),
                transmission: config.transmission,
                ptt: config.ptt.clone(),
                surfaces: config.video_surfaces.clone(),
            },
        )
        .map_err(AudioFailure)?;
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
    let mut reported_transmission = false;
    let flow = Rc::new(RefCell::new(Flow::default()));
    let mut streams = streaming::Session::new(
        events.clone(),
        config.video_surfaces.clone(),
        config.streaming_visible.clone(),
        hub::Foreground::new(config.hub.clone(), config.id),
        flow.clone(),
    );
    let result = async {
        let mut controls = Controls::new(Instant::now());
        // The request in flight, and the subscription the server last settled on.
        let mut subscription: Option<(Ticket, bool)> = None;
        let mut subscribed: Option<bool> = None;
        let mut connected = false;
        let mut deadline = Instant::now();
        let mut sent = 0;
        let mut received = 0;
        let mut last_meter = Instant::now();
        let mut meter = MeterDelivery::default();
        let mut last_usage = Instant::now();
        let mut last_server_request = Instant::now() - usage::SERVER_REFRESH_INTERVAL;
        let mut server_usage: Option<ServerUsage> = None;
        let mut pending_info: Option<(Ticket, ClientId, Option<ChannelId>, Instant)> = None;
        let mut channel_polls = ChannelPolls::default();
        let mut last_info_check = Instant::now() - usage::REFRESH_INTERVAL;
        let mut pending_join: Option<(Ticket, u64, Instant)> = None;
        let mut server_info_enabled = true;
        let mut last_voice = None;
        let mut last_whisper: Option<Instant> = None;
        let mut speakers = Speakers::default();
        let mut tick = tokio::time::interval(Duration::from_millis(10));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        enum Step { Network(Option<Result<StreamItem, tsclientlib::Error>>), Command(Option<Command>), Media(u64, Option<streaming::MediaEvent>), Voice(Option<Voice>), Tick }
        // Pokes and moves in flight, so their replies can be reported.
        let mut requests: HashMap<Ticket, Source> = HashMap::new();
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
                    (generation, event) = streams.next_media() => Step::Media(generation, event),
                    voice = audio.next_voice() => Step::Voice(voice),
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
                            controls.desired.muted = value;
                            if value { audio.set_can_send(false); }
                            controls.changed(&mut flow.borrow_mut(), Instant::now());
                            let _ = events.try_send(Event::Controls { confirmed: controls.confirmed, pending: controls.pending.is_some() || controls.confirmed != Some(controls.desired) });
                        },
                        Command::Deafen(value) => {
                            controls.desired.deafened = value;
                            audio.set_silent(value || !connected);
                            audio.clear_playback();
                            audio.set_can_send(false);
                            controls.changed(&mut flow.borrow_mut(), Instant::now());
                            let _ = events.try_send(Event::Controls { confirmed: controls.confirmed, pending: controls.pending.is_some() || controls.confirmed != Some(controls.desired) });
                        },
                        Command::Join(id, password) => {
                            if flow.borrow().cooling_down(Instant::now()) {
                                let _ = events.try_send(Event::Warning { source: Source::Flood, detail: flow.borrow().flood_detail(Operation::Join, Instant::now()) });
                                continue;
                            }
                            if pending_join.is_some() {
                                let _ = events.try_send(Event::Warning { source: Source::Join, detail: "A channel join is already in progress.".into() });
                                continue;
                            }
                            let access = state.channels.get(&ChannelId(id)).map(channel_join_access);
                            if !access.is_some_and(JoinAccess::can_request) {
                                let _ = events.try_send(Event::Warning { source: Source::Join, detail: "Cannot join: channel access is unavailable or denied.".into() });
                                continue;
                            }
                            if let Some(client) = state.clients.get(&state.own_client) {
                                let handle = flow.borrow_mut().enqueue(client.client_move(ChannelId(id)).set_password(&password).to_packet(), Operation::Join)?;
                                pending_join = Some((handle, id, Instant::now()));
                            }
                        },
                        Command::RefreshStreams => streams.refresh(&mut connection),
                        Command::WatchStream(id) => {
                            if flow.borrow().cooling_down(Instant::now()) {
                                let _ = events.try_send(Event::Warning { source: Source::Flood, detail: flow.borrow().flood_detail(Operation::StreamJoin, Instant::now()) });
                                continue;
                            }
                            if let Err(error) = streams.watch(&mut connection, &id) {
                                let _ = events.try_send(Event::Warning { source: Source::StreamWatch, detail: error.to_string() });
                            }
                        },
                        Command::StopWatching(id) => streams.stop(&mut connection, id.as_deref(), streaming::Phase::Stopped, "Stopped watching"),
                        Command::Poke(id, message) => {
                            let Some(client) = state.clients.get(&ClientId(id)) else {
                                let _ = events.try_send(Event::Warning { source: Source::Poke, detail: "That client is no longer on the server.".into() });
                                continue;
                            };
                            let message: String = message.chars().filter(|c| !c.is_control()).take(100).collect();
                            match flow.borrow_mut().enqueue(client.poke(&message), Operation::Poke) {
                                Ok(ticket) => { requests.insert(ticket, Source::Poke); },
                                Err(error) => { let _ = events.try_send(Event::Warning { source: Source::Poke, detail: error.to_string() }); },
                            }
                        },
                        Command::Move(ids, channel) => {
                            if !state.channels.contains_key(&ChannelId(channel)) {
                                let _ = events.try_send(Event::Warning { source: Source::Move, detail: "That channel no longer exists.".into() });
                                continue;
                            }
                            for id in ids {
                                let Some(client) = state.clients.get(&ClientId(id)) else { continue; };
                                if client.channel == ChannelId(channel) { continue; }
                                match flow.borrow_mut().enqueue(client.client_move(ChannelId(channel)).to_packet(), Operation::Move) {
                                    Ok(ticket) => { requests.insert(ticket, Source::Move); },
                                    Err(error) => { let _ = events.try_send(Event::Warning { source: Source::Move, detail: error.to_string() }); },
                                }
                            }
                        },
                        Command::Moderate(ids, action, reason) => {
                            let reason: String = reason.chars().filter(|c| !c.is_control()).take(80).collect();
                            for id in ids.into_iter().map(ClientId).filter(|id| *id != state.own_client && state.clients.contains_key(id)) {
                                match flow.borrow_mut().enqueue(moderation_packet(id, action, &reason), Operation::Moderate) {
                                    Ok(ticket) => { requests.insert(ticket, Source::Moderate); },
                                    Err(error) => { let _ = events.try_send(Event::Warning { source: Source::Moderate, detail: error.to_string() }); },
                                }
                            }
                        },
                        Command::SetGroups(ids, edit) => {
                            for client in ids.into_iter().filter_map(|id| state.clients.get(&ClientId(id))) {
                                for packet in group_packets(client, &edit) {
                                    match flow.borrow_mut().enqueue(packet, Operation::Groups) {
                                        Ok(ticket) => { requests.insert(ticket, Source::Groups); },
                                        Err(error) => { let _ = events.try_send(Event::Warning { source: Source::Groups, detail: error.to_string() }); },
                                    }
                                }
                            }
                        },
                        Command::RequestGroups => {
                            for packet in [c2s::OutServerGroupListRequestMessage::new(), c2s::OutChannelGroupListRequestMessage::new()] {
                                if let Err(error) = flow.borrow_mut().enqueue(packet, Operation::Groups) {
                                    let _ = events.try_send(Event::Warning { source: Source::Groups, detail: error.to_string() });
                                }
                            }
                        },
                        Command::StartSharing(options) => {
                            streams.sync(&mut connection);
                            let result = if !connected || !streams.foreground() {
                                Err(anyhow::anyhow!("Open your connected server view to share"))
                            } else if !connection.get_state()?.server.version.starts_with("6.") {
                                Err(anyhow::anyhow!("Sharing requires a TeamSpeak 6 server"))
                            } else {
                                streams.publisher.start(options)
                            };
                            if let Err(error) = result { let _ = events.try_send(Event::Warning { source: Source::StreamPublish, detail: error.to_string() }); }
                        },
                        Command::StopSharing => streams.publisher.stop("Stopped sharing"),
                        Command::DecideViewer(id, allow) => if let Err(error) = streams.publisher.decide(id,allow) {
                            let _ = events.try_send(Event::Warning { source: Source::StreamPublish, detail: error.to_string() });
                        },
                        Command::Stop => break,
                    }
                    let _ = events.try_send(Event::State(snapshot(&connection)?));
                },
                Step::Network(Some(Ok(event))) => match event {
                    StreamItem::BookEvents(changes) => {
                        // The first batch is everyone already here, not people arriving.
                        let first_batch = !connected;
                        if !connected {
                            connected = true;
                            traffic.reset_transport(Instant::now());
                            last_usage = Instant::now();
                            server_info_enabled = true;
                            last_server_request = Instant::now() - usage::SERVER_REFRESH_INTERVAL;
                            let _ = events.try_send(Event::Connected(audio.description().to_owned()));
                            controls.reconnect(Instant::now());
                            subscribed = None;
                            subscription = None;
                        }
                        let state = connection.get_state()?;
                        let own_channel = state.clients.get(&state.own_client).map(|client| client.channel);
                        channel_polls.sync(own_channel, state.clients.values()
                            .filter(|client| client.id != state.own_client && Some(client.channel) == own_channel)
                            .map(|client| client.id));
                        // Deliver membership first so new-channel samples cannot populate the old view.
                        let _ = events.try_send(Event::State(snapshot(&connection)?));
                        if !first_batch {
                            let happened: Vec<_> = changes.iter().filter_map(|change| activity(state, change)).collect();
                            if !happened.is_empty() {
                                let _ = events.try_send(Event::Activity(happened));
                            }
                        }
                        for change in &changes {
                            if let BookEvent::Message { target: tsclientlib::MessageTarget::Poke(_), invoker, message } = change {
                                let clean = |text: &str, limit: usize| text.chars().filter(|c| !c.is_control()).take(limit).collect::<String>();
                                let _ = events.try_send(Event::Poked { from: clean(&invoker.name, 64), message: clean(message, 200) });
                                continue;
                            }
                            if let BookEvent::PropertyAdded { id: PropertyId::ConnectionClientData(id), .. }
                                | BookEvent::PropertyChanged { id: PropertyId::ConnectionClientData(id), .. } = change
                                && let Some(data) = state.clients.get(id).and_then(|client| client.connection_data.as_ref()) {
                                let sample = ServerUsage::new(data, Instant::now());
                                if *id == state.own_client {
                                    server_usage = Some(sample);
                                } else if channel_polls.peers.contains_key(id) {
                                    let _ = events.try_send(Event::ClientUsage { id: id.0, result: Ok(sample) });
                                }
                            }
                        }
                        update_codec(&connection, &audio)?;
                        streams.sync(&mut connection);
                    },
                    StreamItem::Audio(packet) => {
                        received += 1;
                        if let AudioData::S2CWhisper { from, data, .. } = packet.data().data()
                            && !data.is_empty()
                            && !controls.desired.deafened
                        {
                            let now = Instant::now();
                            // One cue for a whisper, however many packets it takes.
                            if last_whisper.is_none_or(|at| now.saturating_duration_since(at) >= WHISPER_GAP) {
                                let name = connection.get_state()?.clients.get(&ClientId(*from)).map_or_else(String::new, |client| client.name.chars().filter(|c| !c.is_control()).take(64).collect());
                                let _ = events.try_send(Event::Activity(vec![Activity { kind: ActivityKind::Whispered, name, channel: String::new(), is_self: false, entered_yours: true, left_yours: false }]));
                            }
                            last_whisper = Some(now);
                        }
                        if let AudioData::S2C { from, data, .. } | AudioData::S2CWhisper { from, data, .. } = packet.data().data() {
                            speakers.packet(*from, data.is_empty(), Instant::now());
                        }
                        if !controls.desired.deafened && connection.can_receive_audio() {
                            let gain = sender_gain(&connection, &config.user_gains, &packet);
                            audio.receive(packet, gain);
                        }
                    },
                    StreamItem::IdentityLevelIncreasing(level) => { let _ = events.try_send(Event::Status(format!("Preparing server identity (security level {level})…"))); },
                    StreamItem::DisconnectedTemporarily(_) => {
                        streams.reset();
                        *flow.borrow_mut() = Flow::default();
                        controls.reconnect(Instant::now());
                        subscription = None;
                        subscribed = None;
                        connected = false;
                        deadline = Instant::now();
                        audio.set_can_send(false);
                        audio.set_silent(true);
                        audio.clear_playback();
                        server_usage = None;
                        pending_info = None;
                        channel_polls = ChannelPolls::default();
                        pending_join = None;
                        last_voice = None;
                        speakers.clear();
                        let network = traffic.reset_transport(Instant::now());
                        let _ = events.try_send(Event::Usage { network, connection: ConnectionUsage::default() });
                        let _ = events.try_send(Event::Status(CONNECTION_LOST.into()));
                    },
                    StreamItem::MessageResult(handle, result) => {
                        let reply = flow.borrow_mut().result(handle, result, Instant::now());
                        if let Some(reply) = reply {
                            let ticket = reply.ticket;
                            if reply.flooded {
                                let _ = events.try_send(Event::Limited);
                                // What Flexspeak resends by itself only shows in the footer;
                                // something you asked for that didn't happen gets a message.
                                if !reply.operation.retried() {
                                    let seconds = flow.borrow().wait_seconds(Instant::now());
                                    let _ = events.try_send(Event::Warning { source: Source::Flood, detail: format!(
                                        "The server asked Flexspeak to slow down, so the {} didn't go through. Try again in about {seconds} seconds.", reply.operation.label()) });
                                }
                            }
                            if reply.recovered { let _ = events.try_send(Event::Recovered(Source::Flood)); }
                            if controls.reply(ticket, reply.result.is_ok(), reply.flooded) {
                                let pending = controls.confirmed != Some(controls.desired) && (reply.flooded || reply.result.is_ok());
                                let _ = events.try_send(Event::Controls { confirmed: controls.confirmed, pending });
                                if reply.result.is_ok() && !pending {
                                    let _ = events.try_send(Event::Recovered(Source::Controls));
                                } else if !reply.flooded && let Err(error) = &reply.result {
                                    let _ = events.try_send(Event::Warning { source: Source::Controls, detail: format!("Server rejected the mute/deafen update: {error}. Your local audio choice is still applied.") });
                                }
                            } else if let Some((_, wanted)) = subscription.filter(|(pending, _)| *pending == ticket) {
                                subscription = None;
                                if !reply.flooded { subscribed = Some(wanted); }
                                if reply.result.is_ok() { let _ = events.try_send(Event::Recovered(Source::Subscription)); }
                                else if !reply.flooded && let Err(error) = &reply.result {
                                    let _ = events.try_send(Event::Warning { source: Source::Subscription, detail: format!("Channel subscription: {error}") });
                                }
                            } else if let Some(source) = requests.remove(&ticket) {
                                match &reply.result {
                                    Ok(()) => { let _ = events.try_send(Event::Done(source)); },
                                    Err(error) if !reply.flooded => {
                                        let detail = if error.missing_permission.is_some() || error.error == tsclientlib::TsError::PermissionsClientInsufficient {
                                            let _ = events.try_send(Event::PermissionDenied);
                                            format!("The server denied it: you lack the permission ({}).", error.error)
                                        } else {
                                            format!("The server rejected it: {error}")
                                        };
                                        let _ = events.try_send(Event::Warning { source, detail });
                                    },
                                    Err(_) => {},
                                }
                            } else if streams.message_result(&mut connection, ticket, &reply.result) {
                            } else if pending_join.is_some_and(|(pending, _, _)| pending == ticket) {
                                let (_, id, _) = pending_join.take().unwrap();
                                if let Err(error) = reply.result {
                                    if error.error == tsclientlib::TsError::ChannelAlreadyIn {
                                        // Being there is what the join was for.
                                        let _ = events.try_send(Event::Recovered(Source::Join));
                                    } else if error.error == tsclientlib::TsError::ChannelInvalidPassword {
                                        let _ = events.try_send(Event::ChannelPasswordRequired(id));
                                    } else if !reply.flooded {
                                        if error.missing_permission.is_some() || error.error == tsclientlib::TsError::PermissionsClientInsufficient {
                                            let _ = events.try_send(Event::PermissionDenied);
                                        }
                                        let _ = events.try_send(Event::Warning { source: Source::Join, detail: format!("Could not join channel: {error}") });
                                    }
                                } else { let _ = events.try_send(Event::Recovered(Source::Join)); }
                            } else if pending_info.is_some_and(|(pending, _, _, _)| pending == ticket) {
                                let (_, id, channel, _) = pending_info.take().unwrap();
                                if reply.result.is_err() && !reply.flooded {
                                    if id == connection.get_state()?.own_client {
                                        server_info_enabled = false;
                                        server_usage = None;
                                    } else if channel_polls.disable(id, channel) {
                                        let _ = events.try_send(Event::ClientUsage { id: id.0, result: Err("Server denied connection details.") });
                                    }
                                }
                            }
                        }
                    },
                    StreamItem::AudioChange(_) => { let _ = events.try_send(Event::State(snapshot(&connection)?)); },
                    StreamItem::StreamCommand { name, content } => streams.notification(&mut connection, &name, &content),
                    _ => {},
                },
                Step::Network(Some(Err(error))) => return Err(connection_error(error)).context("TeamSpeak connection failed"),
                Step::Network(None) => bail!("The server closed the connection."),
                Step::Media(generation, event) => streams.media_event(&mut connection, generation, event),
                Step::Voice(None) => {}
                Step::Voice(Some(voice)) => {
                    let can_send = connected && controls.capture_allowed() && connection.can_send_audio() && audio.is_active();
                    // Empty packets end an utterance when push-to-talk or voice activation closes.
                    if voice.data.is_empty() {
                        if connected {
                            connection.send_audio(OutAudio::new(&AudioData::C2S { id: 0, codec: voice.codec, data: &voice.data }))?;
                        }
                    } else if can_send {
                        if audio.level().sqrt() > crate::audio::SPEAKING_METER {
                            speakers.packet(connection.get_state()?.own_client.0, false, Instant::now());
                        }
                        let _trace = profiler::scope(Category::Network, "network.send_voice");
                        connection.send_audio(OutAudio::new(&AudioData::C2S { id: 0, codec: voice.codec, data: &voice.data }))?;
                        sent += 1;
                        last_voice = Some(Instant::now());
                    }
                },
                Step::Tick => {
                    if events.is_closed() { break; }
                    if !connected && deadline.elapsed() > Duration::from_secs(40) { bail!("Connection timed out. Check the address, UDP port, password, and server availability."); }
                    {
                        if let Some(failure) = audio.failure() { return Err(AudioFailure(anyhow::anyhow!(failure)).into()); }
                        let can_send = connected && controls.capture_allowed() && connection.can_send_audio() && audio.is_active();
                        audio.set_can_send(can_send);
                        audio.set_silent(controls.desired.deafened || !connected);
                        report_transmission(events, &mut reported_transmission, can_send && audio.transmitting());
                        let now = Instant::now();
                        speakers.report(now, |talking| events.try_send(Event::Talking(talking)).is_ok());
                        let expired = flow.borrow_mut().expired(now);
                        for (ticket, operation) in expired {
                            if controls.reply(ticket, false, false) {
                                let _ = events.try_send(Event::Controls { confirmed: controls.confirmed, pending: false });
                                let _ = events.try_send(Event::Warning { source: Source::Controls, detail: "The server did not acknowledge the mute/deafen update. Local audio protection remains applied.".into() });
                            } else if let Some((_, wanted)) = subscription.filter(|(pending, _)| *pending == ticket) {
                                subscription = None;
                                subscribed = Some(wanted);
                                let _ = events.try_send(Event::Warning { source: Source::Subscription, detail: "Channel subscription timed out.".into() });
                            } else if let Some(source) = requests.remove(&ticket) {
                                let _ = events.try_send(Event::Warning { source, detail: format!("The server did not answer the {}.", operation.label()) });
                            } else if streams.timeout(&mut connection, ticket) {
                            } else if pending_join.is_some_and(|(pending, _, _)| pending == ticket) {
                                pending_join = None;
                                let _ = events.try_send(Event::Warning { source: Source::Join, detail: "Channel join timed out. Try joining again.".into() });
                            } else if pending_info.is_some_and(|(pending, _, _, _)| pending == ticket) {
                                let (_, id, channel, _) = pending_info.take().unwrap();
                                if id == connection.get_state()?.own_client { server_info_enabled = false; server_usage = None; }
                                else if channel_polls.disable(id, channel) { let _ = events.try_send(Event::ClientUsage { id: id.0, result: Err("Connection details timed out.") }); }
                            } else {
                                tracing::debug!(operation = operation.label(), "Expired obsolete command");
                            }
                        }
                        if connected && controls.needs_send(now) {
                            let desired = controls.desired;
                            let command = connection.get_state()?.client_update().set_input_muted(desired.muted).set_output_muted(desired.deafened);
                            let ticket = flow.borrow_mut().enqueue(command.to_packet(), Operation::Controls)?;
                            controls.pending = Some((ticket, desired));
                            let _ = events.try_send(Event::Controls { confirmed: controls.confirmed, pending: true });
                        }
                        let wanted = config.subscribe_all.load(Ordering::Relaxed);
                        if connected && subscribed != Some(wanted) && subscription.is_none() && !flow.borrow().cooling_down(now) {
                            let command = connection.get_state()?.server.set_subscribed(wanted);
                            subscription = Some((flow.borrow_mut().enqueue(command, Operation::Subscribe)?, wanted));
                        }
                        if pending_join.is_some_and(|(_, _, at)| now.duration_since(at) > Duration::from_secs(10)) {
                            if let Some((ticket, _, _)) = pending_join.take() { flow.borrow_mut().cancel(ticket); }
                            let _ = events.try_send(Event::Warning { source: Source::Join, detail: "Channel join timed out. You can try again.".into() });
                        }
                        if now.duration_since(last_usage) >= usage::REFRESH_INTERVAL {
                            if let Ok(stats) = connection.get_network_stats() {
                                let network = traffic.sample(&stats.total_bytes, now);
                                let quality = usage::connection_usage(stats, server_usage.as_ref(), last_voice, now);
                                let _ = events.try_send(Event::Usage { network, connection: quality });
                            }
                            last_usage = now;
                        }
                        // One request per second at most, shared by ourselves and current-channel peers.
                        if pending_info.is_some_and(|(_, _, _, at)| now.duration_since(at) > Duration::from_secs(10)) {
                            let (ticket, id, channel, _) = pending_info.take().unwrap();
                            flow.borrow_mut().cancel(ticket);
                            if id == connection.get_state()?.own_client {
                                server_info_enabled = false;
                                server_usage = None;
                            } else if channel_polls.disable(id, channel) {
                                let _ = events.try_send(Event::ClientUsage { id: id.0, result: Err("Connection details timed out.") });
                            }
                        }
                        if connected && pending_info.is_none()
                            && flow.borrow().optional_ready(Operation::Statistics, now)
                            && now.duration_since(last_info_check) >= usage::REFRESH_INTERVAL {
                            last_info_check = now;
                            let state = connection.get_state()?;
                            let own = state.own_client;
                            let target = if server_info_enabled && now.duration_since(last_server_request) >= usage::SERVER_REFRESH_INTERVAL {
                                Some(own)
                            } else if config.channel_health_visible.load(Ordering::Relaxed) {
                                channel_polls.next_due(now)
                            } else { None };
                            if let Some(id) = target && let Some(client) = state.clients.get(&id) {
                                match flow.borrow_mut().enqueue(client.connection_info().to_packet(), Operation::Statistics) {
                                    Ok(handle) => pending_info = Some((handle, id, channel_polls.channel, now)),
                                    Err(_) if id == own => { server_info_enabled = false; server_usage = None; },
                                    Err(_) => {
                                        channel_polls.disable(id, channel_polls.channel);
                                        let _ = events.try_send(Event::ClientUsage { id: id.0, result: Err("Cannot request connection details.") });
                                    },
                                }
                                if id == own { last_server_request = now; } else { channel_polls.requested(id, now); }
                            }
                        }
                        if last_meter.elapsed() >= Duration::from_millis(100) {
                            meter.send(events, audio.level(), sent, received, audio.buffer_bytes());
                            last_meter = Instant::now();
                        }
                        if connected {
                            streams.tick(&mut connection);
                            flow.borrow_mut().flush(&mut connection, now)?;
                        }
                    }
                },
            }
        }
        Ok(())
    }.await;
    streams.publisher.stop("Disconnected");
    streams.stop(
        &mut connection,
        None,
        streaming::Phase::Stopped,
        "The voice session ended",
    );
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
        config.audio_processing.for_session(true),
        config.transmission,
    )
    .map_err(AudioFailure)?;
    let mut ptt_gate = crate::transmission::PttGate::default();
    let mut reported_transmission = false;
    let _ = events.try_send(Event::Connected(audio.description.clone()));
    let mut tick = tokio::time::interval(Duration::from_millis(10));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_meter = Instant::now();
    let mut meter = MeterDelivery::default();
    loop {
        tokio::select! {
            command = commands.recv() => if matches!(command, None | Some(Command::Stop)) { break; },
            _ = tick.tick() => {
                if events.is_closed() { break; }
                let capture = ptt_gate.allows(true, config.ptt.load(Ordering::Acquire), Instant::now(), config.transmission);
                audio.set_capture(capture).map_err(AudioFailure)?;
                audio.tick(true, |_| {}).map_err(AudioFailure)?;
                report_transmission(events, &mut reported_transmission, audio.transmitting());
                if last_meter.elapsed() >= Duration::from_millis(100) {
                    meter.send(events, audio.level(), 0, 0, audio.buffer_bytes());
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
                .name("Flexspeak Check")
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

    #[test]
    fn speakers_start_on_voice_and_stop_on_end_packet_or_silence() {
        fn report(
            speakers: &mut Speakers,
            sent: &mut Vec<BTreeSet<u16>>,
            at: Instant,
            delivered: bool,
        ) {
            speakers.report(at, |talking| {
                sent.push(talking);
                delivered
            });
        }
        let now = Instant::now();
        let ms = Duration::from_millis;
        let mut speakers = Speakers::default();
        let mut sent = Vec::new();
        report(&mut speakers, &mut sent, now, true);
        assert!(sent.is_empty(), "nothing changed, nothing to send");
        speakers.packet(2, false, now);
        speakers.packet(3, false, now);
        report(&mut speakers, &mut sent, now, true);
        assert_eq!(sent.pop(), Some(BTreeSet::from([2, 3])));
        report(&mut speakers, &mut sent, now + ms(20), true);
        assert!(sent.is_empty(), "an unchanged set is not resent");
        speakers.packet(2, true, now + ms(40));
        // A full queue keeps the change pending until it is delivered.
        report(&mut speakers, &mut sent, now + ms(40), false);
        report(&mut speakers, &mut sent, now + ms(50), true);
        assert_eq!(sent, [BTreeSet::from([3]), BTreeSet::from([3])]);
        sent.clear();
        report(&mut speakers, &mut sent, now + Speakers::HOLD, true);
        assert_eq!(sent.pop(), Some(BTreeSet::new()), "silence ends speech");
    }

    #[test]
    fn leaving_tells_kicks_and_bans_from_going_out_of_view() {
        use ActivityKind::*;
        assert_eq!(leaving(Some(Reason::KickServer)), Some(KickedFromServer));
        assert_eq!(leaving(Some(Reason::KickServerBan)), Some(Banned));
        assert_eq!(leaving(Some(Reason::LostConnection)), Some(LostConnection));
        assert_eq!(leaving(Some(Reason::Clientdisconnect)), Some(Disconnected));
        assert_eq!(
            leaving(Some(Reason::Subscription)),
            None,
            "only out of view"
        );
        assert_eq!(leaving(Some(Reason::Moved)), None, "moved out of view");
        assert_eq!(arriving(Some(Reason::None)), Some(Connected));
        assert_eq!(arriving(Some(Reason::Moved)), Some(Moved));
        assert_eq!(
            arriving(Some(Reason::Subscription)),
            None,
            "only came into view"
        );
    }

    #[test]
    fn transmit_status_retries_a_full_ui_queue_and_does_not_need_meter_sampling() {
        let (events, mut receiver) = mpsc::channel(1);
        events.try_send(Event::Status("busy".into())).unwrap();
        let mut reported = false;
        report_transmission(&events, &mut reported, true);
        assert!(!reported);
        receiver.try_recv().unwrap();
        report_transmission(&events, &mut reported, true);
        assert!(matches!(receiver.try_recv(), Ok(Event::Transmitting(true))));
        report_transmission(&events, &mut reported, true);
        assert!(receiver.try_recv().is_err());
        report_transmission(&events, &mut reported, false);
        assert!(matches!(
            receiver.try_recv(),
            Ok(Event::Transmitting(false))
        ));
    }

    #[test]
    fn channel_health_polling_rotates_fairly_and_stops_retrying_denied_peers() {
        let now = Instant::now();
        let mut polls = ChannelPolls::default();
        polls.sync(
            Some(ChannelId(1)),
            [ClientId(2), ClientId(3), ClientId(4)].into_iter(),
        );
        for (second, id) in [(0, 2), (1, 3), (2, 4)] {
            let at = now + Duration::from_secs(second);
            assert_eq!(polls.next_due(at), Some(ClientId(id)));
            polls.requested(ClientId(id), at);
        }
        assert_eq!(polls.next_due(now + Duration::from_secs(4)), None);
        assert_eq!(
            polls.next_due(now + Duration::from_secs(5)),
            Some(ClientId(2))
        );
        assert!(!polls.disable(ClientId(2), Some(ChannelId(9))));
        assert!(polls.disable(ClientId(2), Some(ChannelId(1))));
        assert_eq!(
            polls.next_due(now + Duration::from_secs(6)),
            Some(ClientId(3))
        );
        // A departed client is removed; a new channel resets denials and schedules fresh samples.
        polls.sync(Some(ChannelId(1)), [ClientId(2), ClientId(4)].into_iter());
        assert_eq!(
            polls.next_due(now + Duration::from_secs(20)),
            Some(ClientId(4))
        );
        polls.sync(Some(ChannelId(2)), [ClientId(2), ClientId(5)].into_iter());
        assert_eq!(
            polls.next_due(now + Duration::from_secs(20)),
            Some(ClientId(2))
        );
        assert!(!polls.disable(ClientId(2), Some(ChannelId(1))));
        polls.sync(None, std::iter::empty());
        assert!(polls.peers.is_empty());
        assert_eq!(polls.next_due(now), None);
    }

    #[tokio::test]
    #[ignore = "Explicit network-only QA; set NATIVE_VOICE_TEST_SERVER"]
    async fn server_tree_probe_stays_muted_in_lobby() -> Result<()> {
        let address = std::env::var("NATIVE_VOICE_TEST_SERVER")?;
        let mut connection = Connection::build(address)
            .name("Flexspeak Tree Check")
            .identity(Identity::create())
            .channel_id(ChannelId(1))
            .input_muted(true)
            .output_muted(true)
            .connect()?;
        let result = tokio::time::timeout(Duration::from_secs(20), async {
            let mut flow = Flow::default();
            let mut pending = HashSet::new();
            let mut initialized = false;
            let mut tick = tokio::time::interval(Duration::from_millis(25));
            loop {
                let event = {
                    let mut stream = connection.events();
                    tokio::select! {
                        event = stream.next() => Some(event),
                        _ = tick.tick() => None,
                    }
                };
                let Some(event) = event else {
                    flow.flush(&mut connection, Instant::now())?;
                    continue;
                };
                match event {
                    Some(Ok(StreamItem::BookEvents(_))) if !initialized => {
                        let state = connection.get_state()?;
                        if let Some(own) = state.clients.get(&state.own_client) {
                            assert_eq!(own.channel, ChannelId(1));
                            pending.insert(
                                flow.enqueue(
                                    state
                                        .client_update()
                                        .set_input_muted(true)
                                        .set_output_muted(true)
                                        .to_packet(),
                                    Operation::Controls,
                                )?,
                            );
                            pending.insert(flow.enqueue(
                                state.server.set_subscribed(true),
                                Operation::Subscribe,
                            )?);
                            pending.insert(flow.enqueue(
                                own.connection_info().to_packet(),
                                Operation::Statistics,
                            )?);
                            initialized = true;
                        }
                    }
                    Some(Ok(StreamItem::MessageResult(handle, result))) => {
                        if let Some(reply) = flow.result(handle, result, Instant::now()) {
                            reply.result?;
                            assert!(pending.remove(&reply.ticket));
                            println!("Paced acknowledgement: {}", reply.operation.label());
                            if pending.is_empty() {
                                let server = &connection.get_state()?.server;
                                println!(
                                    "Server version: {} · platform: {} · protocol: {}",
                                    server.version, server.platform, server.protocol_version
                                );
                                return snapshot(&connection);
                            }
                        }
                    }
                    Some(Err(error)) => return Err(error.into()),
                    None => bail!("Disconnected before receiving paced command acknowledgements"),
                    _ => {}
                }
            }
        })
        .await
        .context("Server tree probe timed out")
        .and_then(|result| result);
        close(&mut connection).await;
        let state = result?;
        assert_eq!(state.current.as_ref().map(|channel| channel.id), Some(1));
        let own = state
            .channels
            .iter()
            .flat_map(|channel| &channel.members)
            .find(|member| member.is_self)
            .context("Own client missing")?;
        assert!(own.mic_muted && own.sound_muted);
        println!(
            "Tree probe: {} channels, {} visible clients, {} subscribed channels; join hints: {} allowed / {} denied / {} unavailable. Muted/deafened in Lobby; no audio devices opened.",
            state.channels.len(),
            state
                .channels
                .iter()
                .map(|channel| channel.members.len())
                .sum::<usize>(),
            state
                .channels
                .iter()
                .filter(|channel| channel.members_visible)
                .count(),
            state
                .channels
                .iter()
                .filter(|channel| channel.join_access == JoinAccess::Allowed)
                .count(),
            state
                .channels
                .iter()
                .filter(|channel| channel.join_access == JoinAccess::Denied)
                .count(),
            state
                .channels
                .iter()
                .filter(|channel| channel.join_access == JoinAccess::Unknown)
                .count()
        );
        Ok(())
    }

    #[test]
    fn tree_preserves_parent_hierarchy_and_linked_sibling_order() {
        let channels = [
            (3, 0, 7),
            (2, 10, 8),
            (4, 2, 0),
            (7, 0, 10),
            (8, 10, 0),
            (10, 0, 0),
        ]
        .into_iter()
        .map(|(id, parent, order)| Channel {
            id,
            parent,
            order,
            ..Channel::default()
        })
        .collect();
        let ordered = order_channels(channels);
        assert_eq!(
            ordered
                .iter()
                .map(|channel| (channel.id, channel.depth))
                .collect::<Vec<_>>(),
            [(10, 0), (8, 1), (2, 1), (4, 2), (7, 0), (3, 0)]
        );
    }

    #[test]
    fn tree_keeps_orphans_and_cycles_once_without_recursing() {
        let channels = [(1, 2, 2), (2, 1, 1), (3, 999, 777), (4, 0, 4)]
            .into_iter()
            .map(|(id, parent, order)| Channel {
                id,
                parent,
                order,
                ..Channel::default()
            })
            .collect();
        let ordered = order_channels(channels);
        let mut ids: Vec<_> = ordered.iter().map(|channel| channel.id).collect();
        ids.sort_unstable();
        assert_eq!(ids, [1, 2, 3, 4]);
    }

    #[test]
    fn join_access_requires_a_positive_server_hint_and_supported_codec() {
        let mut channel: tsclientlib::data::Channel = serde_json::from_value(serde_json::json!({
            "id": 1, "parent": 0, "name": "Lobby", "order": 0, "codec": "OpusVoice",
            "channel_type": "Permanent", "forced_silence": false, "subscribed": true
        }))
        .unwrap();
        assert_eq!(channel_join_access(&channel), JoinAccess::Unknown);
        channel.permission_hints = Some(ChannelPermissionHint::SUBSCRIBE);
        assert_eq!(channel_join_access(&channel), JoinAccess::Denied);
        channel.permission_hints = Some(ChannelPermissionHint::JOIN);
        assert_eq!(channel_join_access(&channel), JoinAccess::Allowed);
        channel.codec = Codec::SpeexNarrowband;
        assert_eq!(channel_join_access(&channel), JoinAccess::UnsupportedCodec);
    }

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

// Cache only delivered samples: a full channel must not swallow a final zero.
#[derive(Default)]
struct MeterDelivery(Option<(f32, u64, u64, usize)>);
impl MeterDelivery {
    fn send(
        &mut self,
        events: &tokio::sync::mpsc::Sender<Event>,
        level: f32,
        sent: u64,
        received: u64,
        audio_buffer_bytes: usize,
    ) {
        let sample = (level, sent, received, audio_buffer_bytes);
        if self.0 != Some(sample)
            && events
                .try_send(Event::Meter {
                    level,
                    sent,
                    received,
                    audio_buffer_bytes,
                })
                .is_ok()
        {
            self.0 = Some(sample);
        }
    }
}
#[cfg(test)]
mod meter_delivery_tests {
    use super::*;
    #[test]
    fn unchanged_samples_are_skipped_and_final_zero_is_retried() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let mut meter = MeterDelivery::default();
        meter.send(&tx, 1.0, 0, 0, 0);
        meter.send(&tx, 0.0, 0, 0, 0);
        assert!(matches!(rx.try_recv(), Ok(Event::Meter { level: 1.0, .. })));
        meter.send(&tx, 0.0, 0, 0, 0);
        assert!(matches!(rx.try_recv(), Ok(Event::Meter { level: 0.0, .. })));
        meter.send(&tx, 0.0, 0, 0, 0);
        assert!(rx.try_recv().is_err());
    }
}
