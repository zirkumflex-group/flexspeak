//! One set of audio devices shared by every server connection.
//!
//! Each server keeps its own decoders behind a [`Link`]. The hub sums everything you haven't
//! deafened into the one playback device and captures your microphone only for the active
//! server. It opens with the first connection and closes with the last.

use std::{
    collections::HashSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use anyhow::{Result, anyhow};
use tokio::sync::mpsc;
use tsclientlib::{ClientId, audio::AudioHandler};
use tsproto_packets::packets::{AudioData, CodecType, InAudioBuf};

use crate::audio::{AudioEngine, DeviceChoice, PlaybackVolume};
use crate::profiler::{self, Category};
use crate::streaming::Surfaces;

/// Half a second of 20 ms frames, so a stalled connection drops audio rather than grows memory.
const VOICE_BACKLOG: usize = 25;
const TICK: Duration = Duration::from_millis(10);

/// Identifies one server connection for as long as it runs; never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionId(pub u64);

/// One encoded microphone frame on its way to a server; empty ends an utterance.
#[derive(Debug)]
pub struct Voice {
    pub codec: CodecType,
    pub data: Vec<u8>,
}

/// What the first connection opens the devices with. Settings are locked while connected, so
/// later connections can't ask for anything else.
#[derive(Clone)]
pub struct Open {
    pub input: DeviceChoice,
    pub output: DeviceChoice,
    pub volume: Arc<PlaybackVolume>,
    pub processing: crate::audio_processing::Settings,
    pub transmission: crate::transmission::Settings,
    pub ptt: Arc<AtomicBool>,
    pub surfaces: Arc<Surfaces>,
}

/// One server's end of the hub.
struct Line {
    id: u64,
    incoming: Mutex<AudioHandler>,
    /// Deafened or not connected: its voices are not played.
    silent: AtomicBool,
    can_send: AtomicBool,
    codec: Mutex<(CodecType, u8)>,
    voice: mpsc::Sender<Voice>,
}

#[derive(Default)]
struct Shared {
    lines: Mutex<Vec<Arc<Line>>>,
    failure: Mutex<Option<String>>,
    level: AtomicU32,
    transmitting: AtomicBool,
    buffer_bytes: AtomicUsize,
    chime: AtomicBool,
}

struct Device {
    shared: Arc<Shared>,
    stop: std::sync::mpsc::Sender<()>,
    thread: JoinHandle<()>,
    description: String,
    users: usize,
}

/// Which server is active, and the devices while any server is connected.
#[derive(Default)]
pub struct AudioHub {
    active: AtomicU64,
    device: Mutex<Option<Device>>,
}

impl AudioHub {
    /// Sends the microphone to this server, or to none.
    pub fn set_active(&self, id: Option<SessionId>) {
        self.active
            .store(id.map_or(0, |id| id.0), Ordering::Release);
    }

    fn active(&self) -> u64 {
        self.active.load(Ordering::Acquire)
    }

    fn is_active(&self, id: SessionId) -> bool {
        self.active() == id.0
    }

    /// Joins a server to the hub, opening the devices if it's the first. Blocks while they
    /// open, which can include a permission dialog, so never call this on the UI thread.
    pub fn attach(self: &Arc<Self>, id: SessionId, open: Open) -> Result<Link> {
        let mut device = self.device.lock().unwrap();
        if device.is_none() {
            *device = Some(Device::open(self.clone(), open)?);
        }
        let device = device.as_mut().expect("just opened");
        device.users += 1;
        let (line, voice) = Line::new(id);
        device.shared.lines.lock().unwrap().push(line.clone());
        Ok(Link {
            hub: self.clone(),
            shared: device.shared.clone(),
            line,
            voice,
            description: device.description.clone(),
        })
    }

    fn detach(&self, line: &Arc<Line>) {
        let mut guard = self.device.lock().unwrap();
        let Some(device) = guard.as_mut() else {
            return;
        };
        device
            .shared
            .lines
            .lock()
            .unwrap()
            .retain(|other| !Arc::ptr_eq(other, line));
        device.users -= 1;
        if device.users == 0 {
            // Keep the lock until the devices have closed, so a connection arriving now opens
            // them afresh rather than racing the old ones.
            let device = guard.take().expect("checked above");
            let _ = device.stop.send(());
            let _ = device.thread.join();
        }
    }
}

impl Line {
    fn new(id: SessionId) -> (Arc<Self>, mpsc::Receiver<Voice>) {
        let (voice, receiver) = mpsc::channel(VOICE_BACKLOG);
        (
            Arc::new(Self {
                id: id.0,
                incoming: Mutex::new(AudioHandler::new()),
                silent: AtomicBool::new(true),
                can_send: AtomicBool::new(false),
                codec: Mutex::new((CodecType::OpusVoice, 7)),
                voice,
            }),
            receiver,
        )
    }
}

impl Device {
    fn open(hub: Arc<AudioHub>, open: Open) -> Result<Self> {
        let shared = Arc::new(Shared::default());
        let (stop, stopped) = std::sync::mpsc::channel();
        let (ready, opened) = std::sync::mpsc::channel();
        let thread_shared = shared.clone();
        let thread = std::thread::Builder::new()
            .name("audio-hub".into())
            .spawn(move || {
                // CPAL streams belong to the thread that made them, so they are opened here.
                let engine = AudioEngine::open(
                    &open.input,
                    &open.output,
                    open.volume.clone(),
                    open.processing,
                    open.transmission,
                );
                match engine {
                    Ok(engine) => {
                        let description = engine.description.clone();
                        let _ = ready.send(Ok(description));
                        Core::new(engine, thread_shared, hub, &open).run(&stopped);
                    }
                    Err(error) => {
                        let _ = ready.send(Err(error));
                    }
                }
            })?;
        let description = opened
            .recv()
            .map_err(|_| anyhow!("The audio worker stopped while opening devices"))??;
        Ok(Self {
            shared,
            stop,
            thread,
            description,
            users: 0,
        })
    }
}

/// Whether a session is the one the window is showing, for code that isn't audio.
#[derive(Clone)]
pub struct Foreground {
    hub: Arc<AudioHub>,
    id: SessionId,
}

impl Foreground {
    pub fn new(hub: Arc<AudioHub>, id: SessionId) -> Self {
        Self { hub, id }
    }

    pub fn is_active(&self) -> bool {
        self.hub.is_active(self.id)
    }
}

/// A server's connection to the hub. Dropping it leaves the hub.
pub struct Link {
    hub: Arc<AudioHub>,
    shared: Arc<Shared>,
    line: Arc<Line>,
    voice: mpsc::Receiver<Voice>,
    description: String,
}

impl Link {
    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn is_active(&self) -> bool {
        self.hub.active() == self.line.id
    }

    /// Why the devices failed, once they have. Every server shares this outcome.
    pub fn failure(&self) -> Option<String> {
        self.shared.failure.lock().unwrap().clone()
    }

    pub fn set_can_send(&self, can_send: bool) {
        self.line.can_send.store(can_send, Ordering::Release);
    }

    pub fn set_silent(&self, silent: bool) {
        self.line.silent.store(silent, Ordering::Release);
    }

    /// The codec to encode with while this server is active.
    pub fn set_codec(&self, codec: CodecType, quality: u8) {
        *self.line.codec.lock().unwrap() = (codec, quality);
    }

    pub fn receive(&self, packet: InAudioBuf) {
        let _trace = profiler::scope(Category::AudioProcessing, "audio.receive");
        let id = match packet.data().data() {
            AudioData::S2C { from, .. } | AudioData::S2CWhisper { from, .. } => ClientId(*from),
            _ => return,
        };
        let result = self.line.incoming.lock().unwrap().handle_packet(id, packet);
        if let Err(error) = result {
            tracing::debug!(%error, "Dropped incoming audio packet");
        }
    }

    /// Stops this server's voices only; the device keeps playing every other server.
    pub fn clear_playback(&self) {
        *self.line.incoming.lock().unwrap() = AudioHandler::new();
    }

    /// Plays the poke chime, whichever server was poked.
    pub fn chime(&self) {
        self.shared.chime.store(true, Ordering::Release);
    }

    pub fn level(&self) -> f32 {
        f32::from_bits(self.shared.level.load(Ordering::Relaxed))
    }

    /// Whether your microphone is currently open for this server.
    pub fn transmitting(&self) -> bool {
        self.is_active() && self.shared.transmitting.load(Ordering::Relaxed)
    }

    pub fn buffer_bytes(&self) -> usize {
        self.shared.buffer_bytes.load(Ordering::Relaxed)
    }

    pub async fn next_voice(&mut self) -> Option<Voice> {
        self.voice.recv().await
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        self.hub.detach(&self.line);
    }
}

/// Runs on the hub's thread: everything here is the device's, one set per app.
struct Core {
    engine: AudioEngine,
    shared: Arc<Shared>,
    hub: Arc<AudioHub>,
    ptt: Arc<AtomicBool>,
    ptt_gate: crate::transmission::PttGate,
    transmission: crate::transmission::Settings,
    surfaces: Arc<Surfaces>,
    /// The server the microphone last went to.
    routed: u64,
    /// The codec the encoder is set up for.
    applied: Option<(CodecType, u8)>,
    /// Servers still owed the empty packet that ends an utterance, because their queue was full.
    owed: HashSet<u64>,
}

impl Core {
    fn new(engine: AudioEngine, shared: Arc<Shared>, hub: Arc<AudioHub>, open: &Open) -> Self {
        Self {
            engine,
            shared,
            hub,
            ptt: open.ptt.clone(),
            ptt_gate: crate::transmission::PttGate::default(),
            transmission: open.transmission,
            surfaces: open.surfaces.clone(),
            routed: 0,
            applied: None,
            owed: HashSet::new(),
        }
    }

    fn run(mut self, stopped: &std::sync::mpsc::Receiver<()>) {
        let mut next = Instant::now();
        loop {
            next += TICK;
            let now = Instant::now();
            match stopped.recv_timeout(next.saturating_duration_since(now)) {
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                _ => return,
            }
            // After a stall, carry on rather than rush through the missed ticks.
            if next + 10 * TICK < now {
                next = now;
            }
            if let Err(error) = self.tick(now) {
                *self.shared.failure.lock().unwrap() = Some(format!("{error:#}"));
                return;
            }
        }
    }

    fn tick(&mut self, now: Instant) -> Result<()> {
        let shared = self.shared.clone();
        let lines = shared.lines.lock().unwrap();
        let find = |id: u64| lines.iter().find(|line| line.id == id);
        let active = find(self.hub.active());
        let routed = active.map_or(0, |line| line.id);
        if self.routed != routed {
            let closing = self.engine.end_utterance()?;
            if let Some(line) = find(self.routed) {
                for data in closing {
                    self.deliver(line, data);
                }
            }
            self.routed = routed;
            self.applied = None;
        }
        if let Some(line) = active {
            let codec = *line.codec.lock().unwrap();
            if self.applied != Some(codec) {
                self.engine.set_codec(codec.0, codec.1)?;
                self.applied = Some(codec);
            }
        }
        let can_send = active.is_some_and(|line| line.can_send.load(Ordering::Acquire));
        let capture = self.ptt_gate.allows(
            can_send,
            self.ptt.load(Ordering::Acquire),
            now,
            self.transmission,
        );
        self.engine.set_capture(capture)?;
        self.surfaces
            .set_audio_enabled(active.is_some_and(|line| !line.silent.load(Ordering::Acquire)));
        if shared.chime.swap(false, Ordering::AcqRel) {
            self.engine.chime();
        }
        let surfaces = self.surfaces.clone();
        let packets = self.engine.tick(false, |stereo| {
            for line in lines.iter() {
                if !line.silent.load(Ordering::Acquire) {
                    line.incoming.lock().unwrap().fill_buffer(stereo);
                }
            }
            surfaces.mix_audio(stereo);
        })?;
        if let Some(line) = active {
            for data in packets {
                self.deliver(line, data);
            }
        }
        self.settle_owed(&lines);
        shared
            .level
            .store(self.engine.level().to_bits(), Ordering::Relaxed);
        shared
            .transmitting
            .store(can_send && self.engine.transmitting(), Ordering::Relaxed);
        shared
            .buffer_bytes
            .store(self.engine.buffer_bytes(), Ordering::Relaxed);
        Ok(())
    }

    /// Queues a frame for its server. Voice may be dropped under load; the packet ending an
    /// utterance never is, or everyone there would hear you as still talking.
    fn deliver(&mut self, line: &Line, data: Vec<u8>) {
        let codec = self.engine.codec;
        if self.owed.contains(&line.id) {
            let delivered = line
                .voice
                .try_send(Voice {
                    codec,
                    data: Vec::new(),
                })
                .is_ok();
            if !delivered && !line.voice.is_closed() {
                return;
            }
            self.owed.remove(&line.id);
        }
        let end = data.is_empty();
        if line.voice.try_send(Voice { codec, data }).is_err() && end {
            self.owed.insert(line.id);
        }
    }

    fn settle_owed(&mut self, lines: &[Arc<Line>]) {
        let codec = self.engine.codec;
        self.owed.retain(|id| {
            let Some(line) = lines.iter().find(|line| line.id == *id) else {
                return false;
            };
            line.voice
                .try_send(Voice {
                    codec,
                    data: Vec::new(),
                })
                .is_err()
                && !line.voice.is_closed()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{FRAME, RATE};
    use audiopus::{Application, Channels, SampleRate, coder::Encoder};
    use tsproto_packets::packets::{Direction, OutAudio};

    struct Rig {
        core: Core,
        shared: Arc<Shared>,
        hub: Arc<AudioHub>,
    }

    fn rig(transmission: crate::transmission::Settings) -> Rig {
        let hub = Arc::new(AudioHub::default());
        let shared = Arc::new(Shared::default());
        let open = Open {
            input: DeviceChoice {
                id: String::new(),
                name: String::new(),
            },
            output: DeviceChoice {
                id: String::new(),
                name: String::new(),
            },
            volume: Arc::new(PlaybackVolume::default()),
            processing: crate::audio_processing::Settings::default(),
            transmission,
            ptt: Arc::new(AtomicBool::new(false)),
            surfaces: Arc::new(Surfaces::default()),
        };
        let engine = AudioEngine::without_devices(transmission).unwrap();
        let core = Core::new(engine, shared.clone(), hub.clone(), &open);
        Rig { core, shared, hub }
    }

    impl Rig {
        fn line(&self, id: u64) -> (Arc<Line>, mpsc::Receiver<Voice>) {
            let (line, voice) = Line::new(SessionId(id));
            line.silent.store(false, Ordering::Release);
            line.can_send.store(true, Ordering::Release);
            self.shared.lines.lock().unwrap().push(line.clone());
            (line, voice)
        }

        fn tick(&mut self) {
            self.core.tick(Instant::now()).unwrap();
        }

        /// One frame of microphone audio arriving, then the hub working through it.
        fn speak(&mut self) {
            for sample in 0..FRAME {
                let at = sample as f32 / RATE as f32;
                let value = (at * 440.0 * std::f32::consts::TAU).sin() * 0.3;
                self.core.engine.push_mic(value);
            }
            self.tick();
        }

        /// How much was played over this many ticks, with the device draining between them.
        fn heard(&mut self, ticks: usize) -> f32 {
            let mut total = 0.0;
            for _ in 0..ticks {
                self.tick();
                let played = self.core.engine.drain_playback();
                total += played.iter().flatten().map(|v| v.abs()).sum::<f32>();
            }
            total
        }
    }

    fn drain(voice: &mut mpsc::Receiver<Voice>) -> Vec<Vec<u8>> {
        std::iter::from_fn(|| voice.try_recv().ok().map(|voice| voice.data)).collect()
    }

    fn talking_packets(count: u16) -> Vec<InAudioBuf> {
        let encoder = Encoder::new(SampleRate::Hz48000, Channels::Mono, Application::Voip).unwrap();
        (0..count)
            .map(|id| {
                let input: Vec<f32> = (0..FRAME)
                    .map(|i| {
                        ((id as usize * FRAME + i) as f32 * 440.0 * std::f32::consts::TAU
                            / RATE as f32)
                            .sin()
                            * 0.25
                    })
                    .collect();
                let mut encoded = [0; 1275];
                let len = encoder.encode_float(&input, &mut encoded).unwrap();
                let packet = OutAudio::new(&AudioData::S2C {
                    id,
                    from: 42,
                    codec: CodecType::OpusVoice,
                    data: &encoded[..len],
                });
                InAudioBuf::try_new(Direction::S2C, packet.into_vec()).unwrap()
            })
            .collect()
    }

    #[test]
    fn the_handler_can_be_shared_with_the_device_thread() {
        fn sendable<T: Send + Sync>() {}
        sendable::<Mutex<AudioHandler>>();
    }

    #[test]
    fn the_microphone_goes_only_to_the_active_server() {
        let mut rig = rig(crate::transmission::Settings::default());
        let (_a, mut a) = rig.line(1);
        let (_b, mut b) = rig.line(2);
        rig.hub.set_active(Some(SessionId(1)));
        rig.tick();
        rig.speak();
        rig.speak();
        let to_a = drain(&mut a);
        assert!(to_a.iter().any(|packet| !packet.is_empty()), "{to_a:?}");
        assert!(drain(&mut b).is_empty(), "nothing reaches the other server");
        // No active server, no microphone at all.
        rig.hub.set_active(None);
        rig.tick();
        drain(&mut a);
        rig.speak();
        assert!(drain(&mut a).is_empty() && drain(&mut b).is_empty());
    }

    #[test]
    fn a_muted_active_server_sends_nothing_even_with_others_able_to() {
        let mut rig = rig(crate::transmission::Settings::default());
        let (line_a, mut a) = rig.line(1);
        let (_b, mut b) = rig.line(2);
        rig.hub.set_active(Some(SessionId(1)));
        line_a.can_send.store(false, Ordering::Release);
        rig.tick();
        rig.speak();
        assert!(drain(&mut a).is_empty() && drain(&mut b).is_empty());
    }

    #[test]
    fn switching_mid_speech_ends_the_utterance_on_the_old_server_and_starts_fresh_on_the_new() {
        let mut rig = rig(crate::transmission::Settings::default());
        let (_a, mut a) = rig.line(1);
        let (_b, mut b) = rig.line(2);
        rig.hub.set_active(Some(SessionId(1)));
        rig.tick();
        rig.speak();
        rig.speak();
        assert!(drain(&mut a).iter().all(|packet| !packet.is_empty()));
        rig.hub.set_active(Some(SessionId(2)));
        rig.tick();
        assert_eq!(
            drain(&mut a),
            [Vec::<u8>::new()],
            "the end goes to the old server"
        );
        assert!(
            drain(&mut b).is_empty(),
            "speech before the switch stays behind"
        );
        rig.speak();
        rig.speak();
        assert!(drain(&mut a).is_empty());
        let to_b = drain(&mut b);
        assert!(to_b.iter().any(|packet| !packet.is_empty()), "{to_b:?}");
        assert!(to_b.iter().all(|packet| !packet.is_empty()), "{to_b:?}");
    }

    #[test]
    fn the_end_of_an_utterance_waits_for_a_full_queue() {
        let mut rig = rig(crate::transmission::Settings::default());
        let (_a, mut a) = rig.line(1);
        let (_b, _other) = rig.line(2);
        rig.hub.set_active(Some(SessionId(1)));
        rig.tick();
        rig.speak();
        rig.speak();
        for _ in 0..VOICE_BACKLOG {
            if _a
                .voice
                .try_send(Voice {
                    codec: CodecType::OpusVoice,
                    data: vec![1],
                })
                .is_err()
            {
                break;
            }
        }
        rig.hub.set_active(Some(SessionId(2)));
        rig.tick();
        assert!(rig.core.owed.contains(&1));
        drain(&mut a);
        rig.tick();
        assert!(rig.core.owed.is_empty());
        assert_eq!(drain(&mut a), [Vec::<u8>::new()]);
    }

    #[test]
    fn a_deafened_server_stays_silent_while_others_play() {
        let mut rig = rig(crate::transmission::Settings::default());
        let (a, _voice_a) = rig.line(1);
        let (b, _voice_b) = rig.line(2);
        rig.hub.set_active(Some(SessionId(1)));
        b.silent.store(true, Ordering::Release);
        for packet in talking_packets(40) {
            b.incoming
                .lock()
                .unwrap()
                .handle_packet(ClientId(42), packet)
                .unwrap();
        }
        assert_eq!(rig.heard(30), 0.0, "a deafened server is not heard");
        // The same audio on a server you can hear.
        for packet in talking_packets(40) {
            a.incoming
                .lock()
                .unwrap()
                .handle_packet(ClientId(42), packet)
                .unwrap();
        }
        let heard = rig.heard(30);
        assert!(heard > 1.0, "the other server still plays: {heard}");
    }

    #[test]
    fn clearing_one_server_leaves_the_other_servers_voices_alone() {
        let mut rig = rig(crate::transmission::Settings::default());
        let (a, _voice_a) = rig.line(1);
        let (b, _voice_b) = rig.line(2);
        rig.hub.set_active(Some(SessionId(1)));
        for packet in talking_packets(40) {
            a.incoming
                .lock()
                .unwrap()
                .handle_packet(ClientId(42), packet)
                .unwrap();
        }
        for packet in talking_packets(40) {
            b.incoming
                .lock()
                .unwrap()
                .handle_packet(ClientId(43), packet)
                .unwrap();
        }
        // What a deafen does to one server.
        *a.incoming.lock().unwrap() = AudioHandler::new();
        assert!(rig.heard(30) > 1.0, "the other server keeps playing");
        assert!(a.incoming.lock().unwrap().get_queues().is_empty());
    }

    #[test]
    fn stream_audio_follows_the_active_server_only() {
        let mut rig = rig(crate::transmission::Settings::default());
        let (a, _voice) = rig.line(1);
        rig.hub.set_active(Some(SessionId(1)));
        rig.tick();
        let generation = rig.core.surfaces.get(0).begin();
        rig.core
            .surfaces
            .get(0)
            .publish_audio(generation, &[16384, 16384]);
        a.silent.store(true, Ordering::Release);
        rig.tick();
        rig.core
            .surfaces
            .get(0)
            .publish_audio(generation, &[16384, 16384]);
        let mut silent = [0.0; 2];
        rig.core.surfaces.mix_audio(&mut silent);
        assert_eq!(
            silent, [0.0; 2],
            "deafening the active server silences its streams"
        );
        rig.hub.set_active(None);
        a.silent.store(false, Ordering::Release);
        rig.tick();
        rig.core
            .surfaces
            .get(0)
            .publish_audio(generation, &[16384, 16384]);
        let mut silent = [0.0; 2];
        rig.core.surfaces.mix_audio(&mut silent);
        assert_eq!(silent, [0.0; 2], "no active server, no stream audio");
    }

    #[test]
    fn push_to_talk_opens_only_for_the_active_server_while_the_key_is_held() {
        let mut rig = rig(crate::transmission::Settings {
            mode: crate::transmission::Mode::PushToTalk,
            ptt_release_ms: 0,
            ..Default::default()
        });
        let (_a, mut a) = rig.line(1);
        let (_b, mut b) = rig.line(2);
        rig.hub.set_active(Some(SessionId(2)));
        rig.tick();
        rig.speak();
        assert!(
            drain(&mut a).is_empty() && drain(&mut b).is_empty(),
            "key up"
        );
        rig.core.ptt.store(true, Ordering::Release);
        rig.tick();
        rig.speak();
        rig.speak();
        assert!(
            !drain(&mut b).is_empty(),
            "held: the active server hears you"
        );
        assert!(drain(&mut a).is_empty());
    }

    #[test]
    fn a_new_server_comes_up_with_the_codec_of_its_channel() {
        let mut rig = rig(crate::transmission::Settings::default());
        let (a, _va) = rig.line(1);
        let (b, _vb) = rig.line(2);
        *a.codec.lock().unwrap() = (CodecType::OpusMusic, 10);
        *b.codec.lock().unwrap() = (CodecType::OpusVoice, 3);
        rig.hub.set_active(Some(SessionId(1)));
        rig.tick();
        assert_eq!(rig.core.engine.codec, CodecType::OpusMusic);
        // A background server's codec is not applied until it's active.
        *b.codec.lock().unwrap() = (CodecType::OpusVoice, 4);
        rig.tick();
        assert_eq!(rig.core.engine.codec, CodecType::OpusMusic);
        rig.hub.set_active(Some(SessionId(2)));
        rig.tick();
        assert_eq!(rig.core.engine.codec, CodecType::OpusVoice);
    }
}
