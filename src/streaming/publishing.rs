use super::{Event, protocol, publish_media};
use crate::{
    alerts::Source,
    audio::DeviceChoice,
    client,
    command_flow::{Flow, Operation, Ticket},
};
use anyhow::{Result, anyhow, ensure};
use publish_media::PeerKey;
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet},
    fmt,
    rc::Rc,
    time::{Duration, Instant},
};
use tokio::sync::mpsc;
use tsclientlib::{ClientId, CommandError};

pub const MAX_VIEWERS: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareSource {
    pub kind: SourceKind,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
pub enum SourceKind {
    Screen(Option<u64>),
    Window(u64),
    Camera(String),
}
impl SourceKind {
    pub fn wire_type(&self) -> u8 {
        match self {
            Self::Camera(_) => 1,
            Self::Screen(_) => 2,
            Self::Window(_) => 3,
        }
    }
    pub fn is_camera(&self) -> bool {
        matches!(self, Self::Camera(_))
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Access {
    Public,
    #[default]
    Private,
}
impl Access {
    pub const ALL: [Self; 2] = [Self::Public, Self::Private];
    pub fn label(self) -> &'static str {
        match self {
            Self::Public => "Public · join automatically",
            Self::Private => "Private · approve each viewer",
        }
    }
    pub fn wire_value(self) -> u8 {
        match self {
            Self::Public => 1,
            Self::Private => 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    fn options() -> Options {
        Options {
            source: ShareSource {
                kind: SourceKind::Screen(None),
                name: "Test screen".into(),
            },
            audio: None,
            quality: Quality::default(),
            access: Access::default(),
        }
    }
    fn fixture() -> (
        Publisher,
        mpsc::Receiver<publish_media::Input>,
        mpsc::Sender<publish_media::Output>,
    ) {
        let (events, _) = mpsc::channel(128);
        let mut publisher = Publisher::new(events, Rc::new(RefCell::new(Flow::default())));
        let (worker, commands, output, _) = publish_media::test_worker();
        publisher.worker = Some(worker);
        publisher.options = Some(options());
        publisher.id = Some("ours".into());
        publisher.sync(ClientId(1), HashMap::from([(2, "Viewer".into())]));
        (publisher, commands, output)
    }
    fn notify(publisher: &mut Publisher, name: &str, fields: &str) {
        let args = protocol::parse(name, &format!("{name} {fields}")).unwrap();
        publisher.notification(name, &args[0]);
    }
    #[test]
    fn viewers_must_be_in_channel_and_explicitly_approved() {
        let (mut publisher, mut commands, _output) = fixture();
        notify(&mut publisher, "notifyjoinstreamrequest", "id=other clid=2");
        notify(&mut publisher, "notifyjoinstreamrequest", "id=ours clid=3");
        assert!(publisher.requests.is_empty());
        notify(&mut publisher, "notifyjoinstreamrequest", "id=ours clid=2");
        assert!(commands.try_recv().is_err());
        assert!(publisher.viewers.is_empty());
        publisher.decide(2, true).unwrap();
        let publish_media::Input::Add(key) = commands.try_recv().unwrap() else {
            panic!("Expected admitted viewer")
        };
        assert_eq!(key.client, 2);
        assert_eq!(publisher.viewers.len(), 1);
        publisher.sync(ClientId(1), HashMap::new());
        assert!(publisher.viewers.is_empty());
        assert!(
            matches!(commands.try_recv(),Ok(publish_media::Input::Remove(removed)) if removed == key)
        );
    }
    #[test]
    fn public_shares_admit_channel_members_and_fill_vacancies_without_approval() {
        let (mut publisher, mut commands, _output) = fixture();
        publisher.options.as_mut().unwrap().access = Access::Public;
        publisher.members = (1..=6).map(|id| (id, format!("Viewer {id}"))).collect();
        notify(&mut publisher, "notifyjoinstreamrequest", "id=ours clid=1");
        notify(&mut publisher, "notifyjoinstreamrequest", "id=ours clid=99");
        assert!(commands.try_recv().is_err());
        for id in 2..=6 {
            notify(
                &mut publisher,
                "notifyjoinstreamrequest",
                &format!("id=ours clid={id}"),
            );
        }
        assert_eq!(publisher.viewers.len(), MAX_VIEWERS);
        assert_eq!(
            publisher.requests.keys().copied().collect::<Vec<_>>(),
            vec![6]
        );
        for _ in 0..MAX_VIEWERS {
            assert!(matches!(
                commands.try_recv(),
                Ok(publish_media::Input::Add(_))
            ));
        }
        notify(&mut publisher, "notifyjoinstreamrequest", "id=ours clid=2");
        assert!(
            commands.try_recv().is_err(),
            "Repeated requests must not create duplicate peers"
        );
        notify(&mut publisher, "notifystreamclientleft", "id=ours clid=2");
        assert!(matches!(
            commands.try_recv(),
            Ok(publish_media::Input::Remove(_))
        ));
        assert!(
            matches!(commands.try_recv(), Ok(publish_media::Input::Add(key)) if key.client == 6)
        );
        assert_eq!(publisher.viewers.len(), MAX_VIEWERS);
        assert!(publisher.requests.is_empty());
    }

    #[test]
    fn quality_presets_are_valid_and_custom_settings_are_independent() {
        for preset in Preset::ALL {
            let quality = preset.quality();
            quality.validate().unwrap();
            assert_eq!(quality.preset(), Some(preset));
        }
        let mut quality = Quality {
            frame_rate: 24,
            resolution: Resolution::P2160,
            video_kbps: 12_000,
            audio_kbps: 256,
        };
        assert_eq!(quality.preset(), None);
        assert_eq!(quality.max_size(), (3840, 2160));
        assert_eq!(quality.fps(), 24);
        assert_eq!(quality.bitrate(), 12_000_000);
        assert_eq!(quality.audio_bitrate(), 256_000);
        quality.validate().unwrap();
        quality.frame_rate = 0;
        assert!(quality.validate().is_err());
        quality.frame_rate = 60;
        quality.audio_kbps = 321;
        assert!(quality.validate().is_err());
        quality.audio_kbps = 128;
        quality.video_kbps = 20_001;
        assert!(quality.validate().is_err());
    }

    #[test]
    fn stale_peer_events_cannot_reopen_or_remove_a_new_attempt() {
        let (mut publisher, mut commands, _output) = fixture();
        notify(&mut publisher, "notifyjoinstreamrequest", "id=ours clid=2");
        publisher.decide(2, true).unwrap();
        let publish_media::Input::Add(first) = commands.try_recv().unwrap() else {
            panic!()
        };
        publisher.remove_viewer(2);
        commands.try_recv().unwrap();
        notify(&mut publisher, "notifyjoinstreamrequest", "id=ours clid=2");
        publisher.decide(2, true).unwrap();
        let publish_media::Input::Add(second) = commands.try_recv().unwrap() else {
            panic!()
        };
        assert_ne!(first, second);
        publisher
            .media_event(publish_media::Output::Offer(first, "v=0\r\n".into()))
            .unwrap();
        publisher
            .media_event(publish_media::Output::Connected(first))
            .unwrap();
        publisher
            .media_event(publish_media::Output::Left(first))
            .unwrap();
        assert_eq!(publisher.viewers[&2].key, second);
        assert!(!publisher.viewers[&2].ready);
        assert!(publisher.pending.is_empty());
    }
    #[test]
    fn denied_requests_stay_denied_and_requests_are_bounded() {
        let (mut publisher, mut commands, _output) = fixture();
        notify(&mut publisher, "notifyjoinstreamrequest", "id=ours clid=2");
        publisher.decide(2, false).unwrap();
        notify(&mut publisher, "notifyjoinstreamrequest", "id=ours clid=2");
        assert!(publisher.requests.is_empty());
        assert!(commands.try_recv().is_err());
        for id in 3..80 {
            publisher.members.insert(id, format!("Viewer {id}"));
            notify(
                &mut publisher,
                "notifyjoinstreamrequest",
                &format!("id=ours clid={id}"),
            );
        }
        assert_eq!(publisher.requests.len(), 16);
        assert!(publisher.viewers.is_empty());
    }
    #[test]
    fn stopping_cancels_capture_and_late_start_is_taken_down() {
        let (mut publisher, _commands, _output) = fixture();
        publisher.id = None;
        publisher.media_event(publish_media::Output::Ready).unwrap();
        let ticket = publisher.setup.unwrap();
        publisher.stop("Stopped");
        assert!(!publisher.active());
        assert!(publisher.setup.is_none());
        assert!(!publisher.pending.contains_key(&ticket));
        notify(&mut publisher, "notifystreamstarted", "id=late clid=1");
        assert!(publisher.id.is_none());
        assert!(
            publisher
                .pending
                .values()
                .any(|kind| matches!(kind, Pending::Stop))
        );
        let count = publisher.pending.len();
        publisher.own = None;
        notify(&mut publisher, "notifystreamstarted", "id=unknown");
        assert_eq!(publisher.pending.len(), count);
        let (worker, _, _, active) = publish_media::test_worker();
        drop(worker);
        assert!(!active.load(Ordering::Acquire));
    }
    #[test]
    fn ui_stop_bypasses_the_voice_command_queue() {
        let (mut publisher, _commands, _output) = fixture();
        publisher.emit();
        let ui = Ui {
            status: publisher.status.clone(),
            ..Ui::default()
        };
        ui.stop_capture();
        assert!(publisher.worker.as_ref().unwrap().stop.stopped());
        publisher.tick();
        assert!(!publisher.active());
        assert!(publisher.status.detail.contains("Stopped"));
    }
    #[test]
    fn unconfirmed_start_blocks_retry_and_old_timeouts_cannot_stop_new_share() {
        let (mut publisher, _commands, _output) = fixture();
        publisher.id = None;
        publisher.media_event(publish_media::Output::Ready).unwrap();
        let ticket = publisher.setup.unwrap();
        // A sent command can no longer be cancelled by removing it from the queue.
        publisher.flow.borrow_mut().cancel(ticket);
        publisher.timeout(ticket);
        assert!(!publisher.active());
        assert!(publisher.start(options()).is_err());
        publisher.reset();
        assert!(!publisher.uncertain_start);
        let old = publisher
            .send(
                protocol::setup(
                    "old",
                    &SourceKind::Screen(None),
                    false,
                    Quality::default(),
                    Access::Private,
                ),
                Pending::Setup(0),
                Operation::StreamStart,
            )
            .unwrap();
        publisher.generation = 1;
        publisher.timeout(old);
        assert!(!publisher.uncertain_start);
    }
}
impl fmt::Display for ShareSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}

#[derive(Debug, Clone)]
pub struct Options {
    pub source: ShareSource,
    pub audio: Option<DeviceChoice>,
    pub quality: Quality,
    pub access: Access,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Resolution {
    P480,
    #[default]
    P720,
    P1080,
    P1440,
    P2160,
}
impl Resolution {
    pub const ALL: [Self; 5] = [
        Self::P480,
        Self::P720,
        Self::P1080,
        Self::P1440,
        Self::P2160,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::P480 => "480p",
            Self::P720 => "720p",
            Self::P1080 => "1080p",
            Self::P1440 => "1440p",
            Self::P2160 => "2160p (4K)",
        }
    }
    #[cfg(any(target_os = "macos", target_os = "windows", test))]
    pub fn max_size(self) -> (u32, u32) {
        match self {
            Self::P480 => (854, 480),
            Self::P720 => (1280, 720),
            Self::P1080 => (1920, 1080),
            Self::P1440 => (2560, 1440),
            Self::P2160 => (3840, 2160),
        }
    }
}

/// Independent encoder ceilings. Presets just fill these values; changing one leaves the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quality {
    pub resolution: Resolution,
    pub frame_rate: u32,
    pub video_kbps: u32,
    pub audio_kbps: u32,
}
impl Default for Quality {
    fn default() -> Self {
        Preset::Balanced.quality()
    }
}
impl Quality {
    pub fn label(self) -> String {
        format!(
            "{} · {} fps · {} kbps",
            self.resolution.label(),
            self.frame_rate,
            self.video_kbps
        )
    }
    #[cfg(any(target_os = "macos", target_os = "windows", test))]
    pub fn max_size(self) -> (u32, u32) {
        self.resolution.max_size()
    }
    #[cfg(any(target_os = "macos", target_os = "windows", test))]
    pub fn fps(self) -> u32 {
        self.frame_rate
    }
    pub fn bitrate(self) -> u64 {
        u64::from(self.video_kbps) * 1000
    }
    #[cfg(any(target_os = "macos", target_os = "windows", test))]
    pub fn audio_bitrate(self) -> u64 {
        u64::from(self.audio_kbps) * 1000
    }
    pub fn validate(self) -> Result<()> {
        ensure!(
            (5..=60).contains(&self.frame_rate),
            "Frame rate must be 5–60 fps"
        );
        ensure!(
            (250..=20_000).contains(&self.video_kbps),
            "Video bitrate must be 250–20,000 kbps"
        );
        ensure!(
            (32..=320).contains(&self.audio_kbps),
            "Audio bitrate must be 32–320 kbps"
        );
        Ok(())
    }
    pub fn preset(self) -> Option<Preset> {
        Preset::ALL.into_iter().find(|p| p.quality() == self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    Saver,
    Balanced,
    Sharp,
    Smooth,
}
impl Preset {
    pub const ALL: [Self; 4] = [Self::Saver, Self::Balanced, Self::Sharp, Self::Smooth];
    pub fn label(self) -> &'static str {
        match self {
            Self::Saver => "Data saver",
            Self::Balanced => "Balanced",
            Self::Sharp => "Sharp text",
            Self::Smooth => "Smooth motion",
        }
    }
    pub fn quality(self) -> Quality {
        let (resolution, frame_rate, video_kbps, audio_kbps) = match self {
            Self::Saver => (Resolution::P480, 30, 1200, 64),
            Self::Balanced => (Resolution::P720, 30, 2500, 128),
            Self::Sharp => (Resolution::P1080, 30, 4500, 128),
            Self::Smooth => (Resolution::P720, 60, 4000, 128),
        };
        Quality {
            resolution,
            frame_rate,
            video_kbps,
            audio_kbps,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Status {
    pub stop: Option<publish_media::Stop>,
    pub active: bool,
    pub live: bool,
    pub detail: String,
    pub requests: Vec<(u16, String)>,
    pub viewers: Vec<(u16, String, bool)>,
    pub video_frames: u64,
    pub audio_frames: u64,
    pub audio_level: f32,
}

pub struct Ui {
    pub sources: Vec<ShareSource>,
    pub selected: Option<ShareSource>,
    pub loading: bool,
    pub audio: bool,
    pub audio_device: Option<DeviceChoice>,
    pub quality: Quality,
    pub access: Access,
    pub status: Status,
}
impl Default for Ui {
    fn default() -> Self {
        Self {
            sources: vec![],
            selected: None,
            loading: false,
            audio: true,
            audio_device: None,
            quality: Quality::default(),
            access: Access::default(),
            status: Status::default(),
        }
    }
}
impl Ui {
    pub fn stop_capture(&self) {
        if let Some(stop) = &self.status.stop {
            stop.stop();
        }
    }
}

#[derive(Clone, Copy)]
enum Pending {
    Setup(u64),
    Stop,
    Peer(PeerKey),
}

struct Viewer {
    name: String,
    ready: bool,
    key: PeerKey,
}

pub struct Publisher {
    generation: u64,
    next_peer: u64,
    events: mpsc::Sender<client::Event>,
    flow: Rc<RefCell<Flow>>,
    worker: Option<publish_media::Publisher>,
    options: Option<Options>,
    id: Option<String>,
    setup: Option<Ticket>,
    uncertain_start: bool,
    pending: HashMap<Ticket, Pending>,
    requests: BTreeMap<u16, String>,
    viewers: BTreeMap<u16, Viewer>,
    denied: HashSet<u16>,
    members: HashMap<u16, String>,
    own: Option<u16>,
    started: Instant,
    status: Status,
}

impl Publisher {
    pub fn new(events: mpsc::Sender<client::Event>, flow: Rc<RefCell<Flow>>) -> Self {
        Self {
            generation: 0,
            next_peer: 0,
            events,
            flow,
            worker: None,
            options: None,
            id: None,
            setup: None,
            uncertain_start: false,
            pending: HashMap::new(),
            requests: BTreeMap::new(),
            viewers: BTreeMap::new(),
            denied: HashSet::new(),
            members: HashMap::new(),
            own: None,
            started: Instant::now(),
            status: Status::default(),
        }
    }
    pub fn active(&self) -> bool {
        self.worker.is_some()
    }
    fn emit(&mut self) {
        self.status.stop = self.worker.as_ref().map(|worker| worker.stop.clone());
        self.status.active = self.active();
        self.status.live = self.id.is_some() && self.active();
        self.status.requests = self
            .requests
            .iter()
            .map(|(id, name)| (*id, name.clone()))
            .collect();
        self.status.viewers = self
            .viewers
            .iter()
            .map(|(id, viewer)| (*id, viewer.name.clone(), viewer.ready))
            .collect();
        let _ = self
            .events
            .try_send(client::Event::Streaming(Event::Publishing(
                self.status.clone(),
            )));
    }
    fn send(
        &mut self,
        packet: tsproto_packets::packets::OutCommand,
        kind: Pending,
        operation: Operation,
    ) -> Result<Ticket> {
        ensure!(
            self.pending.len() < 64,
            "Too many pending publishing commands"
        );
        let ticket = self.flow.borrow_mut().enqueue(packet, operation)?;
        self.pending.insert(ticket, kind);
        Ok(ticket)
    }
    pub fn sync(&mut self, own: ClientId, members: HashMap<u16, String>) {
        self.own = Some(own.0);
        let gone: Vec<_> = self
            .viewers
            .keys()
            .chain(self.requests.keys())
            .copied()
            .filter(|id| !members.contains_key(id))
            .collect();
        self.members = members;
        for id in gone {
            self.remove_viewer(id);
        }
    }
    pub fn start(&mut self, options: Options) -> Result<()> {
        ensure!(!self.active(), "Already sharing");
        ensure!(
            self.setup.is_none() && !self.uncertain_start,
            "The previous start is still unconfirmed. Reconnect before starting another share."
        );
        ensure!(
            !self.flow.borrow().cooling_down(Instant::now()),
            "Wait for the server cooldown before sharing"
        );
        options.quality.validate()?;
        self.generation += 1;
        self.status = Status::default();
        self.denied.clear();
        self.started = Instant::now();
        self.worker = Some(publish_media::start(options.clone())?);
        self.options = Some(options);
        self.status.detail = "Starting capture; allow access if prompted…".into();
        self.emit();
        Ok(())
    }
    pub fn stop(&mut self, detail: &str) {
        self.worker = None;
        self.options = None;
        self.requests.clear();
        self.viewers.clear();
        self.denied.clear();
        let tickets: Vec<_> = self
            .pending
            .iter()
            .filter_map(|(ticket, kind)| matches!(kind, Pending::Peer(_)).then_some(*ticket))
            .collect();
        for ticket in tickets {
            self.pending.remove(&ticket);
            self.flow.borrow_mut().cancel(ticket);
        }
        if let Some(ticket) = self.setup {
            if self.flow.borrow_mut().cancel(ticket) {
                self.pending.remove(&ticket);
                self.setup = None;
            } else {
                self.uncertain_start = true;
            }
        }
        if let Some(id) = self.id.take()
            && let Err(error) = self.send(protocol::stop(&id), Pending::Stop, Operation::StreamStop)
        {
            self.warn(format!(
                "Capture stopped, but the server could not be notified: {error}"
            ));
        }
        self.status.detail = detail.into();
        self.status.audio_level = 0.0;
        self.emit();
    }
    pub fn reset(&mut self) {
        self.worker = None;
        self.options = None;
        self.id = None;
        self.setup = None;
        self.uncertain_start = false;
        self.requests.clear();
        self.viewers.clear();
        self.denied.clear();
        for ticket in self.pending.keys() {
            self.flow.borrow_mut().cancel(*ticket);
        }
        self.pending.clear();
        self.status = Status::default();
        self.emit();
    }
    fn warn(&self, detail: String) {
        let _ = self.events.try_send(client::Event::Warning {
            source: Source::StreamPublish,
            detail,
        });
    }
    fn fail(&mut self, detail: &str) {
        self.stop(detail);
        self.warn(detail.into());
    }
    fn command(&self, command: publish_media::Input) -> Result<()> {
        self.worker
            .as_ref()
            .ok_or_else(|| anyhow!("Sharing has stopped"))?
            .input
            .try_send(command)
            .map_err(|_| anyhow!("The publisher is busy or stopped"))
    }
    pub fn decide(&mut self, peer: u16, allow: bool) -> Result<()> {
        let name = self
            .requests
            .get(&peer)
            .cloned()
            .ok_or_else(|| anyhow!("This viewer is no longer waiting"))?;
        ensure!(
            self.members.contains_key(&peer),
            "The viewer left your channel"
        );
        let id = self
            .id
            .clone()
            .ok_or_else(|| anyhow!("Sharing has stopped"))?;
        if allow {
            ensure!(
                self.viewers.len() < MAX_VIEWERS,
                "This share already has four viewers"
            );
            self.next_peer += 1;
            let key = PeerKey {
                client: peer,
                serial: self.next_peer,
            };
            self.command(publish_media::Input::Add(key))?;
            self.viewers.insert(
                peer,
                Viewer {
                    name,
                    ready: false,
                    key,
                },
            );
        } else {
            // Remember a refusal locally; repeated server requests must not reopen it.
            ensure!(
                self.denied.len() < 128,
                "Too many denied requests; restart sharing to clear them"
            );
            self.denied.insert(peer);
            self.send(
                protocol::admit(&id, peer, None)?,
                Pending::Peer(PeerKey {
                    client: peer,
                    serial: 0,
                }),
                Operation::StreamAdmit,
            )?;
        }
        self.requests.remove(&peer);
        self.emit();
        Ok(())
    }
    fn remove_viewer(&mut self, peer: u16) {
        self.requests.remove(&peer);
        if let Some(viewer) = self.viewers.remove(&peer)
            && self
                .command(publish_media::Input::Remove(viewer.key))
                .is_err()
            && self.active()
        {
            self.fail("Could not stop a departing viewer; sharing stopped");
            return;
        }
        let tickets: Vec<_> = self
            .pending
            .iter()
            .filter_map(|(ticket, kind)| {
                matches!(kind, Pending::Peer(key) if key.client == peer).then_some(*ticket)
            })
            .collect();
        for ticket in tickets {
            self.pending.remove(&ticket);
            self.flow.borrow_mut().cancel(ticket);
        }
        self.admit_waiting();
        self.emit();
    }
    /// Public shares also fill newly freed slots, so a full share never needs manual approval.
    fn admit_waiting(&mut self) {
        if !self
            .options
            .as_ref()
            .is_some_and(|o| o.access == Access::Public)
        {
            return;
        }
        let available = MAX_VIEWERS.saturating_sub(self.viewers.len());
        let waiting: Vec<_> = self.requests.keys().copied().take(available).collect();
        for peer in waiting {
            if let Err(error) = self.decide(peer, true) {
                self.warn(format!("Could not admit viewer: {error}"));
                break;
            }
        }
    }
    pub fn notification(&mut self, name: &str, args: &protocol::Args) {
        if name == "notifystreamstarted"
            && self.own.is_some()
            && args.get("clid").and_then(|v| v.parse().ok()) == self.own
        {
            let Some(id) = args.get("id").filter(|id| {
                !id.is_empty() && id.len() <= 128 && !id.chars().any(char::is_control)
            }) else {
                return;
            };
            if !self.active()
                || self.uncertain_start
                || self.options.is_none()
                || (self.id.is_none() && self.setup.is_none())
            {
                self.setup = None;
                self.uncertain_start = false;
                if let Err(error) =
                    self.send(protocol::stop(id), Pending::Stop, Operation::StreamStop)
                {
                    self.warn(error.to_string());
                }
                return;
            }
            if self.id.as_ref().is_some_and(|current| current != id) {
                return;
            }
            self.id = Some(id.clone());
            self.setup = None;
            self.status.detail = if self
                .options
                .as_ref()
                .is_some_and(|o| o.access == Access::Public)
            {
                "Public stream · viewers in your channel join automatically."
            } else {
                "Private stream · approve viewers below."
            }
            .into();
            let _ = self
                .events
                .try_send(client::Event::Recovered(Source::StreamPublish));
            self.emit();
            return;
        }
        if self.id.as_ref().is_none_or(|id| args.get("id") != Some(id)) {
            return;
        }
        let peer = args.get("clid").and_then(|v| v.parse::<u16>().ok());
        match name {
            "notifystreamstopped" => {
                self.id = None;
                self.stop("The server stopped sharing");
            }
            "notifystreamclientleft" => {
                if let Some(peer) = peer {
                    self.remove_viewer(peer);
                }
            }
            "notifyjoinstreamrequest" => {
                if let Some(peer) =
                    peer.filter(|id| self.members.contains_key(id) && Some(*id) != self.own)
                {
                    if args.get("is_remove").is_some_and(|v| v == "1") {
                        self.remove_viewer(peer);
                    } else if !self.denied.contains(&peer)
                        && !self.viewers.contains_key(&peer)
                        && self.requests.len() < 16
                    {
                        self.requests.insert(peer, self.members[&peer].clone());
                        self.admit_waiting();
                        self.emit();
                    }
                }
            }
            "notifystreamsignaling" => {
                if let Some(peer) =
                    peer.filter(|id| self.viewers.contains_key(id) && self.members.contains_key(id))
                {
                    let result =
                        protocol::incoming_signal(args.get("json").map_or("", String::as_str))
                            .and_then(|signal| match signal {
                                Some(
                                    input @ (protocol::Input::Answer(_)
                                    | protocol::Input::Candidate { .. }),
                                ) => self.command(publish_media::Input::Signal(
                                    self.viewers[&peer].key,
                                    input,
                                )),
                                _ => Ok(()),
                            });
                    if let Err(error) = result {
                        self.remove_viewer(peer);
                        self.warn(format!("Viewer negotiation failed: {error}"));
                    }
                }
            }
            _ => {}
        }
    }
    fn current(&self, kind: Pending) -> bool {
        match kind {
            Pending::Setup(generation) => generation == self.generation,
            Pending::Peer(key) => {
                key.serial == 0 || self.viewers.get(&key.client).is_some_and(|v| v.key == key)
            }
            Pending::Stop => true,
        }
    }
    pub fn result(
        &mut self,
        ticket: Ticket,
        result: &std::result::Result<(), CommandError>,
    ) -> bool {
        let Some(kind) = self.pending.remove(&ticket) else {
            return false;
        };
        if !self.current(kind) {
            return true;
        }
        if let Err(error) = result {
            if matches!(kind, Pending::Setup(_)) {
                self.setup = None;
                self.uncertain_start = false;
            }
            if error.error == tsclientlib::TsError::ClientIsFlooding {
                self.stop("Sharing stopped while the server limits requests");
            } else {
                match kind {
                    Pending::Setup(_) => {
                        self.fail(&format!("The server rejected sharing: {}", error.error))
                    }
                    Pending::Peer(peer) => {
                        self.remove_viewer(peer.client);
                        self.warn(format!("Viewer signaling rejected: {}", error.error));
                    }
                    Pending::Stop => self.warn(format!(
                        "Capture stopped; the server rejected stop sharing: {}",
                        error.error
                    )),
                }
            }
        }
        true
    }
    pub fn timeout(&mut self, ticket: Ticket) -> bool {
        let Some(kind) = self.pending.remove(&ticket) else {
            return false;
        };
        if !self.current(kind) {
            return true;
        }
        match kind {
            Pending::Setup(_) => {
                self.uncertain_start = true;
                self.fail("The server did not confirm sharing. Reconnect before retrying.");
            }
            Pending::Peer(peer) => {
                self.remove_viewer(peer.client);
                self.warn("Viewer signaling timed out".into());
            }
            Pending::Stop => {
                self.warn("Capture stopped; the server did not confirm stop sharing".into())
            }
        }
        true
    }
    pub fn tick(&mut self) {
        if self
            .worker
            .as_ref()
            .is_some_and(|worker| worker.stop.stopped())
        {
            self.stop("Stopped sharing");
            return;
        }
        if self.active() && self.id.is_none() && self.started.elapsed() > Duration::from_secs(60) {
            self.fail("Screen sharing did not start. Check capture permission and try again.");
        }
        for _ in 0..32 {
            let event = match self.worker.as_mut().map(|w| w.output.try_recv()) {
                Some(Ok(event)) => event,
                Some(Err(mpsc::error::TryRecvError::Disconnected)) => {
                    self.fail("The screen publisher stopped");
                    break;
                }
                _ => break,
            };
            if let Err(error) = self.media_event(event) {
                self.fail(&error.to_string());
                break;
            }
        }
    }
    fn media_event(&mut self, event: publish_media::Output) -> Result<()> {
        match event {
            publish_media::Output::Ready => {
                let options = self
                    .options
                    .as_ref()
                    .ok_or_else(|| anyhow!("Sharing was cancelled"))?;
                let packet = protocol::setup(
                    if options.source.kind.is_camera() {
                        "Camera"
                    } else {
                        "Screen share"
                    },
                    &options.source.kind,
                    options.audio.is_some(),
                    options.quality,
                    options.access,
                );
                self.setup = Some(self.send(
                    packet,
                    Pending::Setup(self.generation),
                    Operation::StreamStart,
                )?);
                self.status.detail = "Capture ready; waiting for the server…".into();
                self.emit();
            }
            publish_media::Output::Offer(peer, sdp) => {
                if self.current(Pending::Peer(peer)) {
                    let id = self
                        .id
                        .as_deref()
                        .ok_or_else(|| anyhow!("Sharing has stopped"))?;
                    self.send(
                        protocol::admit(id, peer.client, Some(&sdp))?,
                        Pending::Peer(peer),
                        Operation::StreamAdmit,
                    )?;
                }
            }
            publish_media::Output::Signal(peer, value) => {
                if self.current(Pending::Peer(peer)) {
                    let id = self
                        .id
                        .as_deref()
                        .ok_or_else(|| anyhow!("Sharing has stopped"))?;
                    self.send(
                        protocol::signal_to(id, peer.client, &value)?,
                        Pending::Peer(peer),
                        Operation::StreamSignal,
                    )?;
                }
            }
            publish_media::Output::Connected(peer) => {
                if let Some(viewer) = self.viewers.get_mut(&peer.client).filter(|v| v.key == peer) {
                    viewer.ready = true;
                }
                self.emit();
            }
            publish_media::Output::Left(peer) => {
                if self.current(Pending::Peer(peer)) {
                    self.remove_viewer(peer.client);
                }
            }
            publish_media::Output::Stats {
                video,
                audio,
                level,
            } => {
                self.status.video_frames = video;
                self.status.audio_frames = audio;
                self.status.audio_level = level;
                self.emit();
            }
            publish_media::Output::Failed(error) => return Err(anyhow!(error)),
        }
        Ok(())
    }
}
