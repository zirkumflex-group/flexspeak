use super::{
    AVAILABLE, Event, MAX_CANDIDATES, Phase, StreamInfo, Surface,
    protocol::{self, Input},
    rtc,
};
use crate::{
    alerts::Source,
    client,
    command_flow::{Flow, Operation, Ticket},
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

struct Watch {
    info: StreamInfo,
    generation: u64,
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
    events: mpsc::Sender<client::Event>,
    surface: Arc<Surface>,
    visible: Arc<AtomicBool>,
    performance: Arc<AtomicBool>,
    supported: bool,
    channel: Option<ChannelId>,
    members: HashSet<u16>,
    catalog: BTreeMap<String, StreamInfo>,
    discovery: VecDeque<u16>,
    flow: Rc<RefCell<Flow>>,
    pending: HashMap<Ticket, (Pending, Instant)>,
    last_request: Instant,
    was_visible: bool,
    discovery_failed: bool,
    watch: Option<Watch>,
}

impl Session {
    pub fn new(
        events: mpsc::Sender<client::Event>,
        surface: Arc<Surface>,
        visible: Arc<AtomicBool>,
        performance: Arc<AtomicBool>,
        flow: Rc<RefCell<Flow>>,
    ) -> Self {
        Self {
            events,
            surface,
            visible,
            performance,
            supported: false,
            channel: None,
            members: HashSet::new(),
            catalog: BTreeMap::new(),
            discovery: VecDeque::new(),
            flow,
            pending: HashMap::new(),
            last_request: Instant::now() - Duration::from_secs(1),
            was_visible: false,
            discovery_failed: false,
            watch: None,
        }
    }

    fn emit(&self, event: Event) {
        let _ = self.events.try_send(client::Event::Streaming(event));
    }
    fn publish_catalog(&self) {
        self.emit(Event::Catalog(self.catalog.values().cloned().collect()));
    }
    fn state(&self, phase: Phase, detail: impl Into<String>) {
        if let Some(watch) = &self.watch {
            self.emit(Event::State {
                generation: watch.generation,
                stream: watch.info.clone(),
                phase,
                detail: detail.into(),
            });
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
        self.supported = state.server.version.starts_with("6.");
        let members: HashSet<_> = state
            .clients
            .values()
            .filter(|client| Some(client.channel) == channel && client.id != state.own_client)
            .map(|client| client.id.0)
            .collect();
        if channel != self.channel {
            self.stop(
                connection,
                Phase::Stopped,
                "Stopped after changing channels",
            );
            self.channel = channel;
            self.catalog.clear();
            self.discovery.clear();
            self.members.clear();
            self.publish_catalog();
        }
        if self
            .watch
            .as_ref()
            .is_some_and(|watch| !members.contains(&watch.info.publisher))
        {
            self.stop(
                connection,
                Phase::Stopped,
                "The publisher left your channel",
            );
        }
        let previous = self.catalog.len();
        self.catalog
            .retain(|_, info| members.contains(&info.publisher));
        if self.catalog.len() != previous {
            self.publish_catalog();
        }
        self.discovery.retain(|id| members.contains(id));
        if self.visible.load(Ordering::Relaxed) && self.supported {
            for id in members.difference(&self.members) {
                if self.discovery.len() < 128 {
                    self.discovery.push_back(*id);
                }
            }
        }
        self.members = members;
    }

    pub fn refresh(&mut self, connection: &mut Connection) {
        self.sync(connection);
        self.emit(Event::Notice(String::new()));
        self.discovery_failed = false;
        self.discovery.clear();
        if self.supported {
            self.discovery
                .extend(self.members.iter().copied().take(128));
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
            "The native stream receiver is currently available on macOS"
        );
        ensure!(
            self.supported
                && self.visible.load(Ordering::Relaxed)
                && !self.performance.load(Ordering::Relaxed),
            "Open Streaming in normal mode on a TeamSpeak 6 server"
        );
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
        self.stop(connection, Phase::Stopped, "Stopped watching");
        let generation = self.surface.begin();
        self.watch = Some(Watch {
            info: info.clone(),
            generation,
            started: Instant::now(),
            phase: Phase::Requesting,
            candidates: Vec::new(),
            receiver: None,
        });
        self.state(Phase::Requesting, "The publisher may need to accept your request. Video only; stream audio is not played.");
        if let Err(error) = self.send(
            protocol::join(&info, false),
            Pending::Control(generation),
            Operation::StreamJoin,
        ) {
            self.stop(connection, Phase::Failed, "Could not request the stream");
            return Err(error);
        }
        let _ = self.send(
            protocol::client_version(info.publisher),
            Pending::Metadata,
            Operation::PublisherInfo,
        );
        Ok(())
    }

    pub fn stop(&mut self, _connection: &mut Connection, phase: Phase, detail: &str) {
        self.state(phase, detail);
        if let Some(watch) = self.watch.take() {
            self.surface.end(watch.generation);
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
        self.state(Phase::Stopped, "The server connection ended");
        if let Some(watch) = self.watch.take() {
            self.surface.end(watch.generation);
        }
        self.catalog.clear();
        self.members.clear();
        self.discovery.clear();
        for ticket in self.pending.keys() {
            self.flow.borrow_mut().cancel(*ticket);
        }
        self.pending.clear();
        self.channel = None;
        self.publish_catalog();
    }

    pub fn tick(&mut self, connection: &mut Connection) {
        let visible =
            self.visible.load(Ordering::Relaxed) && !self.performance.load(Ordering::Relaxed);
        if !visible {
            self.stop(
                connection,
                Phase::Stopped,
                "Stopped when leaving the Streaming view",
            );
        }
        if visible && !self.was_visible {
            self.refresh(connection);
        }
        self.was_visible = visible;
        if self.watch.as_ref().is_some_and(|watch| {
            watch.receiver.is_none() && watch.started.elapsed() > Duration::from_secs(45)
        }) {
            self.stop(
                connection,
                Phase::Failed,
                "The publisher did not send an offer within 45 seconds",
            );
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
                && self
                    .watch
                    .as_ref()
                    .is_some_and(|watch| watch.generation == generation)
            {
                self.stop(
                    connection,
                    Phase::Failed,
                    "The server did not acknowledge stream signaling",
                );
            }
        }
        if visible
            && self.supported
            && self.flow.borrow().optional_ready(Instant::now())
            && self.last_request.elapsed() >= Duration::from_secs(1)
            && let Some(id) = self.discovery.pop_front()
        {
            self.last_request = Instant::now();
            if self.members.contains(&id) {
                let _ = self.send(
                    protocol::request_info(id),
                    Pending::Discovery {
                        client: id,
                        channel: self.channel,
                    },
                    Operation::Discovery,
                );
            }
        }
    }

    pub fn message_result(
        &mut self,
        connection: &mut Connection,
        handle: Ticket,
        result: &std::result::Result<(), CommandError>,
    ) -> bool {
        let Some((kind, _)) = self.pending.remove(&handle) else {
            return false;
        };
        if let Err(error) = result {
            if error.error == tsclientlib::TsError::ClientIsFlooding {
                if let Pending::Control(generation) = kind
                    && self
                        .watch
                        .as_ref()
                        .is_some_and(|watch| watch.generation == generation)
                {
                    self.stop(connection, Phase::Stopped, "Viewing stopped while the server limits requests. Try Watch after the cooldown.");
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
                Pending::Control(generation)
                    if self
                        .watch
                        .as_ref()
                        .is_some_and(|watch| watch.generation == generation) =>
                {
                    self.stop(
                        connection,
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
        let Some((kind, _)) = self.pending.remove(&ticket) else {
            return false;
        };
        if let Pending::Control(generation) = kind
            && self
                .watch
                .as_ref()
                .is_some_and(|watch| watch.generation == generation)
        {
            self.stop(
                connection,
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
            match name {
                "notifycliententerview" | "notifyclientupdated" => {
                    if let Some(id) = args
                        .get("clid")
                        .and_then(|id| id.parse().ok())
                        .filter(|id| self.members.contains(id))
                    {
                        if args
                            .get("client_is_streaming")
                            .is_some_and(|value| value == "1")
                        {
                            if self.discovery.len() < 128 && !self.discovery.contains(&id) {
                                self.discovery.push_back(id);
                            }
                        } else if args
                            .get("client_is_streaming")
                            .is_some_and(|value| value == "0")
                        {
                            self.catalog.retain(|_, info| info.publisher != id);
                            self.publish_catalog();
                            if self
                                .watch
                                .as_ref()
                                .is_some_and(|watch| watch.info.publisher == id)
                            {
                                self.stop(
                                    connection,
                                    Phase::Stopped,
                                    "The publisher stopped sharing",
                                );
                            }
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
                        if self.watch.as_ref().is_some_and(|watch| {
                            watch.info.publisher == info.publisher && watch.info.id != info.id
                        }) {
                            self.stop(
                                connection,
                                Phase::Stopped,
                                "The publisher started a different stream",
                            );
                        }
                        self.catalog
                            .retain(|id, old| old.publisher != info.publisher || *id == info.id);
                        self.catalog.insert(info.id.clone(), info);
                        self.publish_catalog();
                    }
                }
                "notifystreamstopped" => {
                    if let Some(id) = args.get("id") {
                        self.catalog.remove(id);
                        self.publish_catalog();
                        if self
                            .watch
                            .as_ref()
                            .is_some_and(|watch| watch.info.id == *id)
                        {
                            self.stop(connection, Phase::Stopped, "The publisher stopped sharing");
                        }
                    }
                }
                "notifystreamclientleft" => {
                    let own = connection.get_state().ok().map(|state| state.own_client.0);
                    let left = args.get("clid").and_then(|id| id.parse::<u16>().ok());
                    if self.watch.as_ref().is_some_and(|watch| {
                        args.get("id") == Some(&watch.info.id)
                            && left.is_some_and(|id| Some(id) == own || id == watch.info.publisher)
                    }) {
                        self.stop(
                            connection,
                            Phase::Stopped,
                            "You or the publisher left the stream",
                        );
                    }
                }
                "notifyrespondjoinstreamrequest" | "notifystreamsignaling" => {
                    let Some(watch) = &self.watch else {
                        continue;
                    };
                    if args.get("id") != Some(&watch.info.id) {
                        continue;
                    }
                    if args
                        .get("invokerid")
                        .is_some_and(|id| id.parse::<u16>().ok() != Some(watch.info.publisher))
                    {
                        continue;
                    }
                    let signal = if name == "notifyrespondjoinstreamrequest" {
                        if args.get("decision").map(String::as_str) != Some("1") {
                            self.stop(
                                connection,
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
                            if let Err(error) = self.media_input(signal) {
                                self.stop(connection, Phase::Failed, &error.to_string());
                            }
                        }
                        Ok(None) => {}
                        Err(_) => self.stop(
                            connection,
                            Phase::Failed,
                            "Invalid stream offer or connection candidate",
                        ),
                    }
                }
                _ => {}
            }
        }
    }

    fn media_input(&mut self, input: Input) -> Result<()> {
        let Some(watch) = &mut self.watch else {
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
                self.surface.clone(),
            )?);
            watch.phase = Phase::Negotiating;
            self.state(
                Phase::Negotiating,
                "Negotiating a receive-only video connection",
            );
        } else {
            ensure!(
                watch.candidates.len() < MAX_CANDIDATES,
                "Too many pending connection candidates"
            );
            watch.candidates.push(input);
        }
        Ok(())
    }

    pub async fn next_media(&mut self) -> Option<rtc::Output> {
        match self
            .watch
            .as_mut()
            .and_then(|watch| watch.receiver.as_mut())
        {
            Some(receiver) => receiver.output.recv().await,
            None => std::future::pending().await,
        }
    }

    pub fn media_event(&mut self, connection: &mut Connection, event: Option<rtc::Output>) {
        let Some(watch) = &self.watch else {
            return;
        };
        let generation = watch.generation;
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
                    self.stop(
                        connection,
                        Phase::Failed,
                        "Could not send video connection signaling",
                    );
                }
            }
            Some(rtc::Output::TransportConnected) => self.state(
                Phase::Negotiating,
                "Video transport connected; waiting for decoded frames",
            ),
            Some(rtc::Output::Playing) => {
                if let Some(watch) = &mut self.watch {
                    watch.phase = Phase::Receiving;
                }
                self.state(
                    Phase::Receiving,
                    "Video only · stop watching before leaving this channel",
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
            Some(rtc::Output::Failed(error)) => self.stop(connection, Phase::Failed, &error),
            None => self.stop(connection, Phase::Stopped, "The video receiver stopped"),
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(watch) = self.watch.take() {
            self.surface.end(watch.generation);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (Session, Connection, mpsc::Receiver<client::Event>) {
        let (events, output) = mpsc::channel(32);
        let mut session = Session::new(
            events,
            Arc::new(Surface::default()),
            Arc::new(AtomicBool::new(true)),
            Arc::new(AtomicBool::new(false)),
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
        (session, connection, output)
    }

    fn active(session: &mut Session) {
        let info = session.catalog.get("stream-a").unwrap().clone();
        let generation = session.surface.begin();
        session.watch = Some(Watch {
            info,
            generation,
            started: Instant::now(),
            phase: Phase::Requesting,
            candidates: vec![],
            receiver: None,
        });
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
        assert!(session.watch.is_some());
        session.notification(
            &mut connection,
            "notifyrespondjoinstreamrequest",
            "notifyrespondjoinstreamrequest id=stream-a invokerid=8 decision=0",
        );
        assert!(session.watch.is_some());
        session.notification(
            &mut connection,
            "notifyrespondjoinstreamrequest",
            "notifyrespondjoinstreamrequest id=stream-a invokerid=7 decision=0",
        );
        assert!(session.watch.is_none());
    }

    #[test]
    fn early_ice_is_bounded_and_stop_flags_clear_receiver_state_without_stopping_voice() {
        let (mut session, mut connection, mut output) = fixture();
        session.notification(
            &mut connection,
            "notifystreaminfo",
            "notifystreaminfo id=stream-a clid=7 name=Window",
        );
        active(&mut session);
        for _ in 0..MAX_CANDIDATES {
            session
                .media_input(Input::Candidate {
                    sdp: "candidate:1 1 udp 1 127.0.0.1 5000 typ host".into(),
                    mid: "0".into(),
                    mline: 0,
                })
                .unwrap();
        }
        assert!(
            session
                .media_input(Input::Candidate {
                    sdp: "x".into(),
                    mid: "0".into(),
                    mline: 0
                })
                .is_err()
        );
        session.notification(
            &mut connection,
            "notifyclientupdated",
            "notifyclientupdated clid=7 client_is_streaming=0",
        );
        assert!(session.watch.is_none());
        assert!(session.catalog.is_empty());
        while let Ok(event) = output.try_recv() {
            assert!(matches!(event, client::Event::Streaming(_)));
        }
    }
}
