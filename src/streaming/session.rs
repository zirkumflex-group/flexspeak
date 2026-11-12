use super::{
    AVAILABLE, Event, MAX_CANDIDATES, MAX_WATCHES, Phase, StreamInfo, Surfaces,
    protocol::{self, Input},
    rtc,
};
use crate::{
    alerts::Source,
    client,
    command_flow::{Flow, Operation, Ticket},
    hub::Foreground,
};
use anyhow::{Result, anyhow, ensure};
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::mpsc;
use tsclientlib::{ChannelId, ClientId, CommandError, Connection};
use tsproto_packets::packets::OutCommand;

/// Why watches end that need no notice: you, or the publisher, ended them, or the connection
/// went, which has its own.
const PUBLISHER_STOPPED: &str = "The publisher stopped sharing";
const LEFT_STREAM: &str = "You or the publisher left the stream";
const CHANGED_CHANNELS: &str = "Stopped after changing channels";
const LEFT_VIEW: &str = "Stopped when switching servers or leaving the server view";
const CONNECTION_ENDED: &str = "The server connection ended";

/// Whether a watch that stopped this way ended as expected, so it may end without a word.
pub fn ended_as_expected(detail: &str) -> bool {
    [
        PUBLISHER_STOPPED,
        LEFT_STREAM,
        CHANGED_CHANNELS,
        LEFT_VIEW,
        CONNECTION_ENDED,
    ]
    .contains(&detail)
}

struct Watch {
    info: StreamInfo,
    generation: u64,
    slot: usize,
    started: Instant,
    phase: Phase,
    candidates: Vec<Input>,
    receiver: Option<rtc::Receiver>,
}

#[derive(Clone, Copy)]
enum Pending {
    Metadata,
    Discovery {
        client: u16,
        channel: Option<ChannelId>,
    },
    Control(u64),
}

pub struct Session {
    pub publisher: super::publishing::Publisher,
    events: mpsc::Sender<client::Event>,
    surfaces: Arc<Surfaces>,
    /// The server page is showing, rather than Statistics.
    visible: Arc<AtomicBool>,
    /// Whether this is the server the window shows; only that one watches streams.
    active: Foreground,
    supported: bool,
    channel: Option<ChannelId>,
    members: HashSet<u16>,
    /// Remember flags from subscribed channels before we join them.
    streaming_flags: HashMap<u16, bool>,
    /// The streamers last sent to the window, so it only hears about changes.
    shown_streamers: Vec<u16>,
    catalog: BTreeMap<String, StreamInfo>,
    discovery: VecDeque<u16>,
    flow: Rc<RefCell<Flow>>,
    pending: HashMap<Ticket, (Pending, Instant)>,
    last_request: Instant,
    was_visible: bool,
    discovery_failed: bool,
    watches: Vec<Watch>,
    /// Which receiver `next_media` polls first, so a busy stream can't starve the others.
    turn: usize,
}

impl Session {
    pub fn new(
        events: mpsc::Sender<client::Event>,
        surfaces: Arc<Surfaces>,
        visible: Arc<AtomicBool>,
        active: Foreground,
        flow: Rc<RefCell<Flow>>,
    ) -> Self {
        Self {
            publisher: super::publishing::Publisher::new(events.clone(), flow.clone()),
            events,
            surfaces,
            visible,
            active,
            supported: false,
            channel: None,
            members: HashSet::new(),
            streaming_flags: HashMap::new(),
            shown_streamers: Vec::new(),
            catalog: BTreeMap::new(),
            discovery: VecDeque::new(),
            flow,
            pending: HashMap::new(),
            last_request: Instant::now() - Duration::from_secs(1),
            was_visible: false,
            discovery_failed: false,
            watches: Vec::new(),
            turn: 0,
        }
    }

    /// Whether streams from this server are on screen.
    pub fn foreground(&self) -> bool {
        self.visible.load(Ordering::Relaxed) && self.active.is_active()
    }

    fn emit(&self, event: Event) {
        let _ = self.events.try_send(client::Event::Streaming(event));
    }
    fn publish_catalog(&self) {
        self.emit(Event::Catalog(self.catalog.values().cloned().collect()));
    }
    /// Tells the window who is streaming anywhere it can see, when that changes.
    fn publish_streamers(&mut self) {
        let mut streamers: Vec<u16> = self
            .streaming_flags
            .iter()
            .filter(|(_, streaming)| **streaming)
            .map(|(id, _)| *id)
            .collect();
        streamers.sort_unstable();
        if streamers != self.shown_streamers {
            self.shown_streamers = streamers.clone();
            self.emit(Event::Streamers(streamers));
        }
    }
    fn state(&self, generation: u64, phase: Phase, detail: impl Into<String>) {
        if let Some(watch) = self.find(generation) {
            self.emit(Event::State {
                generation,
                slot: watch.slot,
                stream: watch.info.clone(),
                phase,
                detail: detail.into(),
            });
        }
    }
    fn find(&self, generation: u64) -> Option<&Watch> {
        self.watches
            .iter()
            .find(|watch| watch.generation == generation)
    }
    /// The generations of the watches `select` picks.
    fn matching(&self, select: impl Fn(&Watch) -> bool) -> Vec<u64> {
        self.watches
            .iter()
            .filter(|watch| select(watch))
            .map(|watch| watch.generation)
            .collect()
    }
    /// Stops every watch `select` picks.
    fn stop_where(
        &mut self,
        connection: &mut Connection,
        select: impl Fn(&Watch) -> bool,
        phase: Phase,
        detail: &str,
    ) {
        for generation in self.matching(select) {
            self.stop_one(connection, generation, phase, detail);
        }
    }

    pub fn sync(&mut self, connection: &mut Connection) {
        let Ok(state) = connection.get_state() else {
            return;
        };
        let channel = state
            .clients
            .get(&state.own_client)
            .map(|client| client.channel);
        let own = state.own_client;
        let names = state
            .clients
            .values()
            .filter(|client| Some(client.channel) == channel && client.id != own)
            .map(|client| {
                (
                    client.id.0,
                    client
                        .name
                        .chars()
                        .filter(|c| !c.is_control())
                        .take(160)
                        .collect(),
                )
            })
            .collect();
        self.supported = state.server.version.starts_with("6.");
        self.streaming_flags
            .retain(|id, _| state.clients.contains_key(&ClientId(*id)));
        self.publish_streamers();
        let members: HashSet<_> = state
            .clients
            .values()
            .filter(|client| Some(client.channel) == channel && client.id != state.own_client)
            .map(|client| client.id.0)
            .collect();
        if channel != self.channel {
            if self.publisher.active() {
                self.publisher
                    .stop("Sharing stopped after changing channels");
            }
            self.stop(connection, None, Phase::Stopped, CHANGED_CHANNELS);
            self.channel = channel;
            self.catalog.clear();
            self.discovery.clear();
            self.members.clear();
            self.publish_catalog();
        }
        self.stop_where(
            connection,
            |watch| !members.contains(&watch.info.publisher),
            Phase::Stopped,
            "The publisher left your channel",
        );
        let previous = self.catalog.len();
        self.catalog
            .retain(|_, info| members.contains(&info.publisher));
        if self.catalog.len() != previous {
            self.publish_catalog();
        }
        self.sync_members(members);
        self.publisher.sync(own, names);
    }

    fn sync_members(&mut self, members: HashSet<u16>) {
        self.discovery.retain(|id| members.contains(id));
        self.pending.retain(|ticket, (kind, _)| {
            if matches!(kind, Pending::Discovery { client, channel }
                if *channel != self.channel || !members.contains(client))
            {
                self.flow.borrow_mut().cancel(*ticket);
                false
            } else {
                true
            }
        });
        if self.foreground() && self.supported {
            let added: Vec<_> = members.difference(&self.members).copied().collect();
            for id in added {
                self.queue_discovery(id);
            }
        }
        self.members = members;
    }

    fn queue_discovery(&mut self, id: u16) {
        if self.streaming_flags.get(&id) == Some(&false)
            || self.pending.values().any(|(kind, _)| {
                matches!(kind, Pending::Discovery { client, channel }
                    if *client == id && *channel == self.channel)
            })
        {
            return;
        }
        self.discovery.retain(|queued| *queued != id);
        if self.streaming_flags.get(&id) == Some(&true) {
            self.discovery.push_front(id);
            self.discovery.truncate(128);
        } else if self.discovery.len() < 128 {
            self.discovery.push_back(id);
        }
    }

    fn cancel_discovery(&mut self, id: u16) {
        self.discovery.retain(|queued| *queued != id);
        // Keep sent requests until their acknowledgement, including recovery reporting.
        self.pending.retain(|ticket, (kind, _)| {
            !matches!(kind, Pending::Discovery { client, .. } if *client == id)
                || !self.flow.borrow_mut().cancel(*ticket)
        });
    }

    pub fn refresh(&mut self, connection: &mut Connection) {
        self.sync(connection);
        self.emit(Event::Notice(String::new()));
        self.discovery_failed = false;
        self.discovery.clear();
        if self.supported {
            for id in self.members.clone() {
                self.queue_discovery(id);
            }
        }
    }

    fn send(&mut self, command: OutCommand, pending: Pending, operation: Operation) -> Result<()> {
        ensure!(
            self.pending.len() < 128,
            "Too many outstanding streaming commands"
        );
        let handle = self.flow.borrow_mut().enqueue(command, operation)?;
        self.pending.insert(handle, (pending, Instant::now()));
        Ok(())
    }

    pub fn watch(&mut self, connection: &mut Connection, id: &str) -> Result<()> {
        self.sync(connection);
        ensure!(
            AVAILABLE,
            "The native stream receiver is currently available on Mac and Windows"
        );
        ensure!(
            self.supported && self.foreground(),
            "Open your server view on a TeamSpeak 6 server"
        );
        if self.watches.iter().any(|watch| watch.info.id == id) {
            return Ok(());
        }
        let Some(slot) = (0..MAX_WATCHES).find(|slot| self.watches.iter().all(|w| w.slot != *slot))
        else {
            anyhow::bail!("You can watch up to {MAX_WATCHES} streams at once. Stop one first.");
        };
        let info = self
            .catalog
            .get(id)
            .filter(|info| self.members.contains(&info.publisher))
            .cloned()
            .ok_or_else(|| anyhow!("This stream is no longer in your channel"))?;
        ensure!(
            !info.server_relayed,
            "This proof supports peer-to-peer streams only"
        );
        let generation = self.surfaces.get(slot).begin();
        self.watches.push(Watch {
            info: info.clone(),
            generation,
            slot,
            started: Instant::now(),
            phase: Phase::Requesting,
            candidates: Vec::new(),
            receiver: None,
        });
        self.state(generation, Phase::Requesting, "The publisher may need to accept your request. Stream audio follows playback volume and deafen.");
        if let Err(error) = self.send(
            protocol::join(&info, false),
            Pending::Control(generation),
            Operation::StreamJoin,
        ) {
            self.stop_one(
                connection,
                generation,
                Phase::Failed,
                "Could not request the stream",
            );
            return Err(error);
        }
        let _ = self.send(
            protocol::client_version(info.publisher),
            Pending::Metadata,
            Operation::PublisherInfo,
        );
        Ok(())
    }

    /// Stops the stream with this ID, or every watched stream for `None`.
    pub fn stop(
        &mut self,
        connection: &mut Connection,
        id: Option<&str>,
        phase: Phase,
        detail: &str,
    ) {
        self.stop_where(
            connection,
            |watch| id.is_none_or(|id| watch.info.id == id),
            phase,
            detail,
        );
    }

    fn stop_one(
        &mut self,
        _connection: &mut Connection,
        generation: u64,
        phase: Phase,
        detail: &str,
    ) {
        self.state(generation, phase, detail);
        if let Some(index) = self
            .watches
            .iter()
            .position(|watch| watch.generation == generation)
        {
            let watch = self.watches.remove(index);
            self.surfaces.get(watch.slot).end(watch.generation);
            self.pending.retain(|ticket, (kind, _)| {
                if matches!(kind, Pending::Control(generation) if *generation == watch.generation) {
                    self.flow.borrow_mut().cancel(*ticket);
                    false
                } else {
                    true
                }
            });
            let _ = self.send(
                protocol::join(&watch.info, true),
                Pending::Control(watch.generation),
                Operation::StreamLeave,
            );
            drop(watch);
        }
    }

    pub fn reset(&mut self) {
        self.publisher.reset();
        for watch in std::mem::take(&mut self.watches) {
            self.emit(Event::State {
                generation: watch.generation,
                slot: watch.slot,
                stream: watch.info.clone(),
                phase: Phase::Stopped,
                detail: CONNECTION_ENDED.into(),
            });
            self.surfaces.get(watch.slot).end(watch.generation);
        }
        self.catalog.clear();
        self.members.clear();
        self.streaming_flags.clear();
        self.publish_streamers();
        self.discovery.clear();
        for ticket in self.pending.keys() {
            self.flow.borrow_mut().cancel(*ticket);
        }
        self.pending.clear();
        self.channel = None;
        self.publish_catalog();
    }

    pub fn tick(&mut self, connection: &mut Connection) {
        let visible = self.foreground();
        if !self.visible.load(Ordering::Relaxed) && self.publisher.active() {
            self.publisher
                .stop("Sharing stopped when leaving the server view");
        }
        // Your own share carries on in the background; only watching is tied to the screen.
        if !visible {
            self.stop(connection, None, Phase::Stopped, LEFT_VIEW);
        }
        if visible && !self.was_visible {
            self.publish_catalog();
            self.refresh(connection);
        }
        self.was_visible = visible;
        self.publisher.tick();
        // A request to watch that the publisher's client never answered, by name.
        let unanswered = self.matching(|watch| {
            watch.receiver.is_none() && watch.started.elapsed() > Duration::from_secs(45)
        });
        for generation in unanswered {
            let name = self.find(generation).and_then(|watch| {
                let state = connection.get_state().ok()?;
                let client = state.clients.get(&ClientId(watch.info.publisher))?;
                Some(client.name.to_string())
            });
            let detail = match name.filter(|name| !name.is_empty()) {
                Some(name) => format!("{name} didn't answer your request."),
                None => "The streamer didn't answer your request.".to_owned(),
            };
            self.stop_one(connection, generation, Phase::Failed, &detail);
        }
        let expired: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, (_, at))| at.elapsed() > Duration::from_secs(15))
            .map(|(handle, (kind, _))| (*handle, *kind))
            .collect();
        for (handle, kind) in expired {
            self.pending.remove(&handle);
            self.flow.borrow_mut().cancel(handle);
            if let Pending::Control(generation) = kind
                && self.find(generation).is_some()
            {
                self.stop_one(
                    connection,
                    generation,
                    Phase::Failed,
                    "The server did not acknowledge stream signaling",
                );
            }
        }
        if visible
            && self.supported
            && let Some(&id) = self.discovery.front()
        {
            // The server has said this member is streaming, so the user is waiting on the
            // answer: send it at the normal command pace. Members without a flag are only a
            // fallback probe and stay paced behind other background requests.
            let flagged = self.streaming_flags.get(&id) == Some(&true);
            let due = flagged
                || (self
                    .flow
                    .borrow()
                    .optional_ready(Operation::Discovery, Instant::now())
                    && self.last_request.elapsed() >= Duration::from_secs(1));
            if due {
                self.discovery.pop_front();
                if !flagged {
                    self.last_request = Instant::now();
                }
                if self.members.contains(&id) {
                    let _ = self.send(
                        protocol::request_info(id),
                        Pending::Discovery {
                            client: id,
                            channel: self.channel,
                        },
                        if flagged {
                            Operation::StreamInfo
                        } else {
                            Operation::Discovery
                        },
                    );
                }
            }
        }
    }

    pub fn message_result(
        &mut self,
        connection: &mut Connection,
        handle: Ticket,
        result: &std::result::Result<(), CommandError>,
    ) -> bool {
        if self.publisher.result(handle, result) {
            return true;
        }
        let Some((kind, _)) = self.pending.remove(&handle) else {
            return false;
        };
        if let Err(error) = result {
            if error.error == tsclientlib::TsError::ClientIsFlooding {
                if let Pending::Control(generation) = kind
                    && self.find(generation).is_some()
                {
                    self.stop_one(connection, generation, Phase::Stopped, "Viewing stopped while the server limits requests. Try Watch after the cooldown.");
                }
                return true;
            }
            match kind {
                Pending::Discovery {
                    client: id,
                    channel,
                } if channel == self.channel => {
                    if error.missing_permission.is_some() {
                        self.discovery_failed = true;
                        self.emit(Event::Notice(format!(
                            "The server denied stream discovery: {}",
                            error.error
                        )));
                    }
                    let old_len = self.catalog.len();
                    self.catalog.retain(|_, info| info.publisher != id);
                    if self.catalog.len() != old_len {
                        self.publish_catalog();
                    }
                }
                Pending::Discovery { .. } => {}
                Pending::Control(generation) if self.find(generation).is_some() => {
                    self.stop_one(
                        connection,
                        generation,
                        Phase::Failed,
                        &format!("The server rejected stream signaling: {}", error.error),
                    );
                }
                Pending::Control(_) | Pending::Metadata => {}
            }
        } else if matches!(kind, Pending::Discovery { .. })
            && !self.discovery_failed
            && self.discovery.is_empty()
            && !self
                .pending
                .values()
                .any(|(kind, _)| matches!(kind, Pending::Discovery { .. }))
        {
            let _ = self
                .events
                .try_send(client::Event::Recovered(Source::StreamDiscovery));
        }
        true
    }

    pub fn timeout(&mut self, connection: &mut Connection, ticket: Ticket) -> bool {
        if self.publisher.timeout(ticket) {
            return true;
        }
        let Some((kind, _)) = self.pending.remove(&ticket) else {
            return false;
        };
        if let Pending::Control(generation) = kind
            && self.find(generation).is_some()
        {
            self.stop_one(
                connection,
                generation,
                Phase::Failed,
                "The server did not acknowledge video negotiation",
            );
        } else if matches!(kind, Pending::Discovery { .. }) {
            self.discovery_failed = true;
            self.emit(Event::Notice(
                "Stream discovery timed out. Refresh streams to try again.".into(),
            ));
        }
        true
    }

    pub fn notification(&mut self, connection: &mut Connection, name: &str, content: &str) {
        self.sync(connection);
        let Ok(parts) = protocol::parse(name, content) else {
            // Without a valid stream ID, an unrelated malformed command cannot stop our viewer.
            return;
        };
        for args in parts {
            self.publisher.notification(name, &args);
            match name {
                "notifycliententerview" | "notifyclientupdated" => {
                    if let Some(id) = args
                        .get("clid")
                        .and_then(|id| id.parse().ok())
                        .filter(|id| *id != 0)
                    {
                        let streaming = match args.get("client_is_streaming").map(String::as_str) {
                            Some("1") => true,
                            Some("0") => false,
                            _ => continue,
                        };
                        self.streaming_flags.insert(id, streaming);
                        if !self.members.contains(&id) {
                            continue;
                        }
                        if streaming {
                            self.queue_discovery(id);
                        } else {
                            self.cancel_discovery(id);
                            self.catalog.retain(|_, info| info.publisher != id);
                            self.publish_catalog();
                            self.stop_where(
                                connection,
                                |watch| watch.info.publisher == id,
                                Phase::Stopped,
                                PUBLISHER_STOPPED,
                            );
                        }
                    }
                }
                "notifystreaminfo" | "notifystreamstarted" => {
                    if let Some(info) = protocol::stream_info(&args)
                        .filter(|info| self.members.contains(&info.publisher))
                    {
                        if self.catalog.len() >= 128 && !self.catalog.contains_key(&info.id) {
                            continue;
                        }
                        // One publisher may share several streams at once: each is its own entry.
                        self.streaming_flags.insert(info.publisher, true);
                        self.cancel_discovery(info.publisher);
                        self.catalog.insert(info.id.clone(), info);
                        self.publish_catalog();
                    }
                }
                "notifystreamstopped" => {
                    if let Some(id) = args.get("id") {
                        if let Some(info) = self.catalog.remove(id) {
                            // Still streaming while any of their other streams runs.
                            let streaming = self
                                .catalog
                                .values()
                                .any(|other| other.publisher == info.publisher);
                            self.streaming_flags.insert(info.publisher, streaming);
                            self.cancel_discovery(info.publisher);
                        }
                        self.publish_catalog();
                        self.stop_where(
                            connection,
                            |watch| watch.info.id == *id,
                            Phase::Stopped,
                            PUBLISHER_STOPPED,
                        );
                    }
                }
                "notifystreamclientleft" => {
                    let own = connection.get_state().ok().map(|state| state.own_client.0);
                    let left = args.get("clid").and_then(|id| id.parse::<u16>().ok());
                    self.stop_where(
                        connection,
                        |watch| {
                            args.get("id") == Some(&watch.info.id)
                                && left
                                    .is_some_and(|id| Some(id) == own || id == watch.info.publisher)
                        },
                        Phase::Stopped,
                        LEFT_STREAM,
                    );
                }
                "notifyrespondjoinstreamrequest" | "notifystreamsignaling" => {
                    let Some(watch) = self
                        .watches
                        .iter()
                        .find(|watch| args.get("id") == Some(&watch.info.id))
                    else {
                        continue;
                    };
                    let generation = watch.generation;
                    if args
                        .get("invokerid")
                        .is_some_and(|id| id.parse::<u16>().ok() != Some(watch.info.publisher))
                    {
                        continue;
                    }
                    let signal = if name == "notifyrespondjoinstreamrequest" {
                        if args.get("decision").map(String::as_str) != Some("1") {
                            self.stop_one(
                                connection,
                                generation,
                                Phase::Failed,
                                "The publisher declined the viewing request",
                            );
                            continue;
                        }
                        protocol::offer(args.get("offer").map_or("", String::as_str)).map(Some)
                    } else {
                        protocol::incoming_signal(args.get("json").map_or("", String::as_str))
                    };
                    match signal {
                        Ok(Some(signal)) => {
                            if let Err(error) = self.media_input(generation, signal) {
                                self.stop_one(
                                    connection,
                                    generation,
                                    Phase::Failed,
                                    &error.to_string(),
                                );
                            }
                        }
                        Ok(None) => {}
                        Err(_) => self.stop_one(
                            connection,
                            generation,
                            Phase::Failed,
                            "Invalid stream offer or connection candidate",
                        ),
                    }
                }
                _ => {}
            }
        }
        self.publish_streamers();
    }

    fn media_input(&mut self, generation: u64, input: Input) -> Result<()> {
        let Some(watch) = self
            .watches
            .iter_mut()
            .find(|watch| watch.generation == generation)
        else {
            return Ok(());
        };
        if let Some(receiver) = &watch.receiver {
            receiver
                .input
                .try_send(input)
                .map_err(|_| anyhow!("The video receiver is busy or stopped"))?;
        } else if let Input::Offer(sdp) = input {
            watch.receiver = Some(rtc::start(
                sdp,
                std::mem::take(&mut watch.candidates),
                watch.generation,
                self.surfaces.get(watch.slot).clone(),
            )?);
            watch.phase = Phase::Negotiating;
            self.state(
                generation,
                Phase::Negotiating,
                "Negotiating a receive-only media connection",
            );
        } else if matches!(input, Input::Candidate { .. }) {
            ensure!(
                watch.candidates.len() < MAX_CANDIDATES,
                "Too many pending connection candidates"
            );
            watch.candidates.push(input);
        }
        Ok(())
    }

    /// The next output of any watched stream's receiver, with that stream's generation.
    pub async fn next_media(&mut self) -> (u64, Option<rtc::Output>) {
        let count = self.watches.len();
        if !self.watches.iter().any(|watch| watch.receiver.is_some()) {
            return std::future::pending().await;
        }
        self.turn = self.turn.wrapping_add(1);
        let first = self.turn;
        let watches = &mut self.watches;
        std::future::poll_fn(|context| {
            for offset in 0..count {
                let watch = &mut watches[(first + offset) % count];
                if let Some(receiver) = &mut watch.receiver
                    && let std::task::Poll::Ready(output) = receiver.output.poll_recv(context)
                {
                    return std::task::Poll::Ready((watch.generation, output));
                }
            }
            std::task::Poll::Pending
        })
        .await
    }

    pub fn media_event(
        &mut self,
        connection: &mut Connection,
        generation: u64,
        event: Option<rtc::Output>,
    ) {
        let Some(watch) = self.find(generation) else {
            return;
        };
        match event {
            Some(rtc::Output::Signal(value)) => {
                let packet = protocol::signal(&watch.info, &value);
                if packet
                    .and_then(|packet| {
                        self.send(
                            packet,
                            Pending::Control(generation),
                            Operation::StreamSignal,
                        )
                    })
                    .is_err()
                {
                    self.stop_one(
                        connection,
                        generation,
                        Phase::Failed,
                        "Could not send video connection signaling",
                    );
                }
            }
            Some(rtc::Output::TransportConnected) => self.state(
                generation,
                Phase::Negotiating,
                "Video transport connected; waiting for decoded frames",
            ),
            Some(rtc::Output::Playing) => {
                if let Some(watch) = self
                    .watches
                    .iter_mut()
                    .find(|watch| watch.generation == generation)
                {
                    watch.phase = Phase::Receiving;
                }
                self.state(
                    generation,
                    Phase::Receiving,
                    "Receiving stream · audio follows playback volume and deafen",
                );
            }
            Some(rtc::Output::Stats(mut stats)) => {
                if let Some(data) = connection
                    .get_state()
                    .ok()
                    .and_then(|state| state.clients.get(&ClientId(watch.info.publisher)))
                    .and_then(|client| client.optional_data.as_ref())
                {
                    stats.publisher_version = format!("{} · {}", data.version, data.platform)
                        .chars()
                        .take(256)
                        .collect();
                }
                self.emit(Event::Stats { generation, stats });
            }
            Some(rtc::Output::Failed(error)) => {
                self.stop_one(connection, generation, Phase::Failed, &error)
            }
            None => self.stop_one(
                connection,
                generation,
                Phase::Stopped,
                "The video receiver stopped",
            ),
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        for watch in self.watches.drain(..) {
            self.surfaces.get(watch.slot).end(watch.generation);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (Session, Connection, mpsc::Receiver<client::Event>) {
        let (session, connection, output, _hub) = fixture_on_hub();
        (session, connection, output)
    }

    /// A session that is the hub's active server, with the hub to switch away from it.
    fn fixture_on_hub() -> (
        Session,
        Connection,
        mpsc::Receiver<client::Event>,
        Arc<crate::hub::AudioHub>,
    ) {
        let (events, output) = mpsc::channel(32);
        let hub = Arc::new(crate::hub::AudioHub::default());
        hub.set_active(Some(crate::hub::SessionId(1)));
        let mut session = Session::new(
            events,
            Arc::new(Surfaces::default()),
            Arc::new(AtomicBool::new(true)),
            Foreground::new(hub.clone(), crate::hub::SessionId(1)),
            Rc::new(RefCell::new(Flow::default())),
        );
        session.channel = Some(ChannelId(1));
        session.supported = true;
        session.members.insert(7);
        // The connection future is never polled: these tests send no network packets.
        let connection = Connection::build("127.0.0.1:0")
            .identity(tsclientlib::Identity::create())
            .connect()
            .unwrap();
        (session, connection, output, hub)
    }

    fn active(session: &mut Session) -> u64 {
        let info = session.catalog.get("stream-a").unwrap().clone();
        let generation = session.surfaces.get(0).begin();
        session.watches.push(Watch {
            info,
            generation,
            slot: 0,
            started: Instant::now(),
            phase: Phase::Requesting,
            candidates: vec![],
            receiver: None,
        });
        generation
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn you_can_watch_a_stream_while_sharing_your_own() {
        let (mut session, mut connection, _output) = fixture();
        let _share = session.publisher.pretend_sharing();
        session.notification(
            &mut connection,
            "notifystreaminfo",
            "notifystreaminfo id=stream-a clid=7 name=Window",
        );
        session.watch(&mut connection, "stream-a").unwrap();
        assert_eq!(session.watches.len(), 1);
        assert!(session.publisher.active());
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn five_streams_can_be_watched_each_in_its_own_slot_but_not_a_sixth() {
        let (mut session, mut connection, _output) = fixture();
        for publisher in 10..16 {
            session.members.insert(publisher);
            session.notification(
                &mut connection,
                "notifystreaminfo",
                &format!("notifystreaminfo id=s{publisher} clid={publisher} name=Window"),
            );
        }
        for publisher in 10..15 {
            session
                .watch(&mut connection, &format!("s{publisher}"))
                .unwrap();
        }
        let slots: HashSet<_> = session.watches.iter().map(|watch| watch.slot).collect();
        assert_eq!(slots.len(), MAX_WATCHES);
        assert!(session.watch(&mut connection, "s15").is_err());
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn switching_servers_stops_watches_but_keeps_your_own_share() {
        let (mut session, mut connection, _output, hub) = fixture_on_hub();
        let _share = session.publisher.pretend_sharing();
        session.notification(
            &mut connection,
            "notifystreaminfo",
            "notifystreaminfo id=stream-a clid=7 name=Window",
        );
        session.watch(&mut connection, "stream-a").unwrap();
        hub.set_active(Some(crate::hub::SessionId(2)));
        session.tick(&mut connection);
        assert!(
            session.watches.is_empty(),
            "watching follows the active server"
        );
        assert!(session.publisher.active(), "your own share keeps running");
        assert!(session.watch(&mut connection, "stream-a").is_err());
        // Leaving the server view altogether still ends both, as it always has.
        hub.set_active(Some(crate::hub::SessionId(1)));
        session.visible.store(false, Ordering::Relaxed);
        session.tick(&mut connection);
        assert!(!session.publisher.active());
    }

    #[test]
    fn one_publisher_can_share_several_streams_at_once() {
        let (mut session, mut connection, _) = fixture();
        session.notification(
            &mut connection,
            "notifystreaminfo",
            "notifystreaminfo id=stream-a clid=7 name=Screen|id=stream-b clid=7 name=Game",
        );
        assert_eq!(
            session.catalog.len(),
            2,
            "a second stream doesn't replace the first"
        );
        active(&mut session);
        session.notification(
            &mut connection,
            "notifystreamstarted",
            "notifystreamstarted id=stream-c clid=7 name=Camera",
        );
        assert_eq!(session.catalog.len(), 3);
        assert_eq!(
            session.watches.len(),
            1,
            "watching one isn't ended by another starting"
        );
        session.notification(
            &mut connection,
            "notifystreamstopped",
            "notifystreamstopped id=stream-b",
        );
        assert_eq!(session.catalog.len(), 2);
        assert_eq!(
            session.streaming_flags.get(&7),
            Some(&true),
            "still streaming the others"
        );
    }

    #[test]
    fn discovery_is_scoped_and_unrelated_or_malformed_signals_cannot_end_a_view() {
        let (mut session, mut connection, _) = fixture();
        session.notification(
            &mut connection,
            "notifystreaminfo",
            "notifystreaminfo id=outside clid=8 name=Other",
        );
        assert!(session.catalog.is_empty());
        session.notification(
            &mut connection,
            "notifystreaminfo",
            "notifystreaminfo id=stream-a clid=7 name=Window",
        );
        assert_eq!(session.catalog.len(), 1);
        active(&mut session);
        session.notification(
            &mut connection,
            "notifystreamstopped",
            "notifystreamstopped id=other",
        );
        session.notification(
            &mut connection,
            "notifystreamsignaling",
            "notifystreamsignaling id=stream-a id=other",
        );
        session.notification(
            &mut connection,
            "notifyrespondjoinstreamrequest",
            "notifyrespondjoinstreamrequest id=other decision=0",
        );
        assert_eq!(session.watches.len(), 1);
        session.notification(
            &mut connection,
            "notifyrespondjoinstreamrequest",
            "notifyrespondjoinstreamrequest id=stream-a invokerid=8 decision=0",
        );
        assert_eq!(session.watches.len(), 1);
        session.notification(
            &mut connection,
            "notifyrespondjoinstreamrequest",
            "notifyrespondjoinstreamrequest id=stream-a invokerid=7 decision=0",
        );
        assert!(session.watches.is_empty());
    }

    #[test]
    fn streamers_in_other_channels_reach_the_window_without_discovery() {
        let (mut session, mut connection, mut output) = fixture();
        let streamers = |output: &mut mpsc::Receiver<client::Event>| {
            let mut sent = Vec::new();
            while let Ok(event) = output.try_recv() {
                if let client::Event::Streaming(Event::Streamers(ids)) = event {
                    sent.push(ids);
                }
            }
            sent
        };
        // Client 9 is in another channel: only the server's flag says it streams.
        session.notification(
            &mut connection,
            "notifycliententerview",
            "notifycliententerview clid=9 client_is_streaming=1",
        );
        assert_eq!(streamers(&mut output), vec![vec![9]]);
        assert!(session.discovery.is_empty() && session.catalog.is_empty());
        // The same flag again changes nothing, so the window hears nothing.
        session.notification(
            &mut connection,
            "notifyclientupdated",
            "notifyclientupdated clid=9 client_is_streaming=1",
        );
        assert!(streamers(&mut output).is_empty());
        session.notification(
            &mut connection,
            "notifyclientupdated",
            "notifyclientupdated clid=9 client_is_streaming=0",
        );
        assert_eq!(streamers(&mut output), vec![Vec::<u16>::new()]);
    }

    #[test]
    fn early_ice_is_bounded_and_stop_flags_clear_receiver_state_without_stopping_voice() {
        let (mut session, mut connection, mut output) = fixture();
        session.notification(
            &mut connection,
            "notifystreaminfo",
            "notifystreaminfo id=stream-a clid=7 name=Window",
        );
        let generation = active(&mut session);
        for _ in 0..MAX_CANDIDATES {
            session
                .media_input(
                    generation,
                    Input::Candidate {
                        sdp: "candidate:1 1 udp 1 127.0.0.1 5000 typ host".into(),
                        mid: "0".into(),
                        mline: 0,
                    },
                )
                .unwrap();
        }
        assert!(
            session
                .media_input(
                    generation,
                    Input::Candidate {
                        sdp: "x".into(),
                        mid: "0".into(),
                        mline: 0
                    }
                )
                .is_err()
        );
        session.notification(
            &mut connection,
            "notifyclientupdated",
            "notifyclientupdated clid=7 client_is_streaming=0",
        );
        assert!(session.watches.is_empty());
        assert!(session.catalog.is_empty());
        while let Ok(event) = output.try_recv() {
            assert!(matches!(event, client::Event::Streaming(_)));
        }
    }
}
