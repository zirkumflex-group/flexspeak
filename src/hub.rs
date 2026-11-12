//! One set of audio devices shared by every server connection.
//!
//! Each server keeps its own decoders behind a [`Link`]. The hub sums everything you haven't
//! deafened into the one playback device and captures your microphone only for the active
//! server. It opens with the first connection and closes with the last.
//!
//! Losing a device never ends a connection. The hub plays through the system's default device
//! while a chosen one is unplugged and goes back to it once it returns; with nothing usable at
//! all it keeps everyone connected and tries again until a device works. Each server reads the
//! outcome from [`Link::trouble`].

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

use crate::audio::{AudioEngine, Chime, DeviceChoice, PlaybackVolume};
use crate::profiler::{self, Category};
use crate::streaming::Surfaces;

/// Half a second of 20 ms frames, so a stalled connection drops audio rather than grows memory.
const VOICE_BACKLOG: usize = 25;
const TICK: Duration = Duration::from_millis(10);
/// How often to try again with no working device.
const RETRY: Duration = Duration::from_millis(1500);
/// Devices that fail again this soon after opening wait before the next try, so a fault that
/// opening can't fix doesn't spin.
const SETTLE: Duration = Duration::from_secs(3);
/// How often to look for a chosen device to come back while another stands in.
const LOOK_BACK: Duration = Duration::from_secs(3);
const BACKOFF_MAX: Duration = Duration::from_secs(15);
/// How long the devices stay open after the last server has gone, for a sound still playing.
const LAST_SOUND: Duration = Duration::from_secs(4);
/// What the device still holds once the last sample has been handed to it.
const SOUND_TAIL: Duration = Duration::from_millis(60);

/// Something wrong with the devices that every server shares and none disconnects over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trouble {
    /// A chosen device is gone and the system's default stands in until it's back.
    Fallback(String),
    /// Nothing could be opened. Flexspeak keeps trying.
    Unavailable(String),
}

/// Identifies one server connection for as long as it runs; never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionId(pub u64);

/// One encoded microphone frame on its way to a server; empty ends an utterance.
#[derive(Debug)]
pub struct Voice {
    pub codec: CodecType,
    pub data: Vec<u8>,
}

/// What the first connection opens the devices with. Later connections share them; a change in
/// Settings reaches the open devices through [`AudioHub::retune`].
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

/// The devices and voice settings chosen in Settings, for devices that are already open.
#[derive(Debug, Clone, PartialEq)]
pub struct Retune {
    pub input: DeviceChoice,
    pub output: DeviceChoice,
    pub processing: crate::audio_processing::Settings,
    pub transmission: crate::transmission::Settings,
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

impl Line {
    fn hear(
        &self,
        id: ClientId,
        packet: InAudioBuf,
        gain: f32,
    ) -> Result<Option<ClientId>, tsclientlib::audio::Error> {
        let mut incoming = self.incoming.lock().unwrap();
        let result = incoming.handle_packet(id, packet);
        // Set on every packet, because a pause in speech drops the queue and its volume.
        if let Some(queue) = incoming.get_mut_queues().get_mut(&id) {
            queue.volume = gain;
        }
        result
    }
}

#[derive(Default)]
struct Shared {
    lines: Mutex<Vec<Arc<Line>>>,
    trouble: Mutex<Option<Trouble>>,
    level: AtomicU32,
    transmitting: AtomicBool,
    buffer_bytes: AtomicUsize,
    /// Sounds on their way to the device, already at their final volume.
    sounds: Mutex<Vec<Chime>>,
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
    /// Connections waiting for the devices, which a sound playing out must not hold up.
    opening: AtomicUsize,
    /// Set as the app quits: nothing is left to play, and nothing is waited for.
    hushed: AtomicBool,
    /// Settings chosen while the devices are open, for the hub's thread to reopen them with.
    retune: Mutex<Option<Retune>>,
}

impl AudioHub {
    /// Sends the microphone to this server, or to none.
    pub fn set_active(&self, id: Option<SessionId>) {
        self.active
            .store(id.map_or(0, |id| id.0), Ordering::Release);
    }

    /// Plays a short sound on the shared device. Nothing plays until a server has opened it,
    /// nor while the devices are opening or closing: this is called from the window's thread,
    /// which must not wait for them.
    pub fn play(&self, chime: Chime) {
        /// More than the device will line up anyway, should it stop taking them.
        const PENDING: usize = 8;
        if let Ok(device) = self.device.try_lock()
            && let Some(device) = device.as_ref()
        {
            let mut sounds = device.shared.sounds.lock().unwrap();
            if sounds.len() < PENDING {
                sounds.push(chime);
            }
        }
    }

    /// Reopens the shared devices with these choices, if any are open, without waiting for
    /// them: the hub's thread picks the change up on its next tick.
    pub fn retune(&self, retune: Retune) {
        *self.retune.lock().unwrap() = Some(retune);
    }

    #[cfg(test)]
    pub fn pending_retune(&self) -> Option<Retune> {
        self.retune.lock().unwrap().clone()
    }

    /// Stops every sound and stops waiting for one to finish. For quitting.
    pub fn hush(&self) {
        self.hushed.store(true, Ordering::Release);
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
        self.opening.fetch_add(1, Ordering::AcqRel);
        let mut device = self.device.lock().unwrap();
        self.opening.fetch_sub(1, Ordering::AcqRel);
        if device.is_none() {
            // Fresh devices open with the current choices, so an earlier change is spent.
            self.retune.lock().unwrap().take();
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
                // Without a working device the connection goes ahead, and the hub keeps trying.
                let mut core = Core::closed(thread_shared, hub, &open);
                core.reopen(Instant::now());
                let description = core.engine.as_ref().map_or_else(
                    || "No audio device".to_owned(),
                    |engine| engine.description.clone(),
                );
                let _ = ready.send(Ok::<_, anyhow::Error>(description));
                core.run(&stopped);
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

/// An error as the start of a sentence that goes on.
fn sentence(error: &anyhow::Error) -> String {
    format!("{error:#}")
        .trim_end()
        .trim_end_matches('.')
        .to_owned()
}

/// What to say when a stand-in replaced a chosen device, if one did.
fn stand_in_notice(open: &Open, input: &DeviceChoice, output: &DeviceChoice) -> Option<Trouble> {
    let output_gone = *output != open.output;
    let input_gone = *input != open.input;
    let text = match (output_gone, input_gone) {
        (false, false) => return None,
        (true, false) => format!(
            "“{}” isn't connected, so you hear everyone through “{}” until it's back.",
            open.output.name, output.name
        ),
        (false, true) => format!(
            "“{}” isn't connected, so you talk through “{}” until it's back.",
            open.input.name, input.name
        ),
        (true, true) => format!(
            "“{}” and “{}” aren't connected, so Flexspeak uses “{}” and “{}” until they're back.",
            open.output.name, open.input.name, output.name, input.name
        ),
    };
    Some(Trouble::Fallback(text))
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

    /// What's wrong with the devices right now, if anything. Every server shares this.
    pub fn trouble(&self) -> Option<Trouble> {
        self.shared.trouble.lock().unwrap().clone()
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

    pub fn receive(&self, packet: InAudioBuf, gain: f32) {
        let _trace = profiler::scope(Category::AudioProcessing, "audio.receive");
        let id = match packet.data().data() {
            AudioData::S2C { from, .. } | AudioData::S2CWhisper { from, .. } => ClientId(*from),
            _ => return,
        };
        let result = self.line.hear(id, packet, gain);
        if let Err(error) = result {
            tracing::debug!(%error, "Dropped incoming audio packet");
        }
    }

    /// Stops this server's voices only; the device keeps playing every other server.
    pub fn clear_playback(&self) {
        *self.line.incoming.lock().unwrap() = AudioHandler::new();
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
    /// `None` while no device works.
    engine: Option<AudioEngine>,
    /// What the first connection asked for, to open again after a device goes.
    open: Open,
    /// When to next try to open devices, or to look for a chosen one to come back.
    next_attempt: Instant,
    opened_at: Instant,
    backoff: Duration,
    /// The codec the last engine encoded with, for packets owed while there is none.
    codec: CodecType,
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
    #[cfg(test)]
    fn new(engine: AudioEngine, shared: Arc<Shared>, hub: Arc<AudioHub>, open: &Open) -> Self {
        let mut core = Self::closed(shared, hub, open);
        core.engine = Some(engine);
        core
    }

    /// A hub with no devices open yet.
    fn closed(shared: Arc<Shared>, hub: Arc<AudioHub>, open: &Open) -> Self {
        let now = Instant::now();
        Self {
            engine: None,
            open: open.clone(),
            next_attempt: now,
            opened_at: now,
            backoff: RETRY,
            codec: CodecType::OpusVoice,
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
                _ => return self.finish_sounds(),
            }
            // After a stall, carry on rather than rush through the missed ticks.
            if next + 10 * TICK < now {
                next = now;
            }
            let retune = self.hub.retune.lock().unwrap().take();
            if let Some(retune) = retune {
                self.retune(retune, now);
            }
            if now >= self.next_attempt {
                self.maintain(now);
            }
            if self.engine.is_none() {
                self.silence();
            } else if let Err(error) = self.tick(now) {
                self.lose(&error, now);
            }
        }
    }

    /// Whether a sound is on its way to the device or still coming out of it.
    fn sounding(&self) -> bool {
        self.engine.as_ref().is_some_and(|engine| {
            engine.sounding() || !self.shared.sounds.lock().unwrap().is_empty()
        })
    }

    /// Plays out the sounds already due before the devices go, so the last server leaving
    /// doesn't cut off "Disconnected" or "Kicked". Gives way to a new connection and to quitting.
    fn finish_sounds(&mut self) {
        if !self.sounding() {
            return;
        }
        let started = Instant::now();
        let mut quiet_since = None;
        while started.elapsed() < LAST_SOUND
            && self.hub.opening.load(Ordering::Acquire) == 0
            && !self.hub.hushed.load(Ordering::Acquire)
        {
            let now = Instant::now();
            if self.sounding() {
                quiet_since = None;
            } else if now.duration_since(*quiet_since.get_or_insert(now)) >= SOUND_TAIL {
                break;
            }
            if self.tick(now).is_err() {
                break;
            }
            std::thread::sleep(TICK);
        }
    }

    fn set_trouble(&self, trouble: Option<Trouble>) {
        *self.shared.trouble.lock().unwrap() = trouble;
    }

    /// With no device, tries again. With a stand-in, goes back to the chosen devices once
    /// they're both plugged in again.
    fn maintain(&mut self, now: Instant) {
        if self.engine.is_none() {
            self.reopen(now);
            return;
        }
        let standing_in = matches!(
            *self.shared.trouble.lock().unwrap(),
            Some(Trouble::Fallback(_))
        );
        self.next_attempt = now + if standing_in { LOOK_BACK } else { RETRY };
        if standing_in
            && crate::audio::stand_in(&self.open.input, true).as_ref() == Some(&self.open.input)
            && crate::audio::stand_in(&self.open.output, false).as_ref() == Some(&self.open.output)
        {
            self.close();
            self.reopen(now);
        }
    }

    /// Opens the chosen devices, or the system's defaults in place of any that are gone.
    fn reopen(&mut self, now: Instant) {
        let input = crate::audio::stand_in(&self.open.input, true);
        let output = crate::audio::stand_in(&self.open.output, false);
        let opened = match (&input, &output) {
            (None, _) => Err(anyhow!("No microphone is connected")),
            (_, None) => Err(anyhow!("No playback device is connected")),
            (Some(input), Some(output)) => AudioEngine::open(
                input,
                output,
                self.open.volume.clone(),
                self.open.processing,
                self.open.transmission,
            ),
        };
        match opened {
            Ok(engine) => {
                self.engine = Some(engine);
                self.opened_at = now;
                self.next_attempt = now + RETRY;
                self.set_trouble(stand_in_notice(
                    &self.open,
                    input.as_ref().expect("opened"),
                    output.as_ref().expect("opened"),
                ));
            }
            // Nothing to open, or it wouldn't open: cheap to ask again soon, so a device
            // plugged back in is heard within a moment.
            Err(error) => {
                self.next_attempt = now + RETRY;
                self.set_trouble(Some(Trouble::Unavailable(format!(
                    "{}. You stay connected, and Flexspeak picks your audio up again as soon as \
                     a device works.",
                    sentence(&error)
                ))));
            }
        }
    }

    /// A device stopped. Ends what you were saying, then opens whatever is there now, unless
    /// the devices just opened and failed straight away, which waits a little first.
    fn lose(&mut self, error: &anyhow::Error, now: Instant) {
        tracing::warn!(%error, "Audio device failed; reopening");
        self.close();
        if now.saturating_duration_since(self.opened_at) < SETTLE {
            self.next_attempt = now + self.backoff;
            self.backoff = (self.backoff * 2).min(BACKOFF_MAX);
            self.set_trouble(Some(Trouble::Unavailable(format!(
                "{}. You stay connected, and Flexspeak tries again in a moment.",
                sentence(error)
            ))));
        } else {
            self.backoff = RETRY;
            self.reopen(now);
        }
    }

    /// Opens the devices again with new choices from Settings; nothing happens if they match.
    fn retune(&mut self, retune: Retune, now: Instant) {
        let processing = retune.processing.for_session(false);
        if retune.input == self.open.input
            && retune.output == self.open.output
            && processing == self.open.processing
            && retune.transmission == self.open.transmission
        {
            return;
        }
        let same_devices = retune.input == self.open.input
            && retune.output == self.open.output
            && processing == self.open.processing;
        self.open.input = retune.input;
        self.open.output = retune.output;
        self.open.processing = processing;
        self.open.transmission = retune.transmission;
        self.transmission = retune.transmission;
        // A new threshold or delay goes to the running engine; anything else reopens it.
        if same_devices
            && self
                .engine
                .as_mut()
                .is_some_and(|engine| engine.tune_transmission(retune.transmission))
        {
            return;
        }
        self.close();

        self.backoff = RETRY;
        self.reopen(now);
    }

    /// Drops the devices. The server you were talking to is owed the packet that ends an
    /// utterance, so nobody hears you as still talking.
    fn close(&mut self) {
        if let Some(engine) = self.engine.take() {
            self.codec = engine.codec;
        }
        if self.routed != 0 {
            self.owed.insert(self.routed);
        }
        self.routed = 0;
        self.applied = None;
    }

    /// While no device works: voices are dropped rather than queued, and nothing is sent.
    fn silence(&mut self) {
        let shared = self.shared.clone();
        let lines = shared.lines.lock().unwrap();
        for line in lines.iter() {
            *line.incoming.lock().unwrap() = AudioHandler::new();
        }
        self.settle_owed(&lines);
        shared.sounds.lock().unwrap().clear();
        shared.level.store(0.0_f32.to_bits(), Ordering::Relaxed);
        shared.transmitting.store(false, Ordering::Relaxed);
        shared.buffer_bytes.store(0, Ordering::Relaxed);
    }

    fn engine(&mut self) -> &mut AudioEngine {
        self.engine.as_mut().expect("ticked with devices open")
    }

    fn tick(&mut self, now: Instant) -> Result<()> {
        let shared = self.shared.clone();
        let lines = shared.lines.lock().unwrap();
        let find = |id: u64| lines.iter().find(|line| line.id == id);
        let active = find(self.hub.active());
        let routed = active.map_or(0, |line| line.id);
        if self.routed != routed {
            let closing = self.engine().end_utterance()?;
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
                self.engine().set_codec(codec.0, codec.1)?;
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
        self.engine().set_capture(capture)?;
        self.surfaces
            .set_audio_enabled(active.is_some_and(|line| !line.silent.load(Ordering::Acquire)));
        if self.hub.hushed.load(Ordering::Acquire) {
            shared.sounds.lock().unwrap().clear();
            self.engine().hush();
        }
        for chime in std::mem::take(&mut *shared.sounds.lock().unwrap()) {
            self.engine().play(chime);
        }
        let surfaces = self.surfaces.clone();
        let packets = self.engine().tick(false, |stereo| {
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
            .store(self.engine().level().to_bits(), Ordering::Relaxed);
        shared
            .transmitting
            .store(can_send && self.engine().transmitting(), Ordering::Relaxed);
        shared
            .buffer_bytes
            .store(self.engine().buffer_bytes(), Ordering::Relaxed);
        Ok(())
    }

    /// Queues a frame for its server. Voice may be dropped under load; the packet ending an
    /// utterance never is, or everyone there would hear you as still talking.
    fn deliver(&mut self, line: &Line, data: Vec<u8>) {
        let codec = self
            .engine
            .as_ref()
            .map_or(self.codec, |engine| engine.codec);
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
        let codec = self
            .engine
            .as_ref()
            .map_or(self.codec, |engine| engine.codec);
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

    #[test]
    fn a_stand_in_is_named_only_when_a_chosen_device_is_gone() {
        let device = |id: &str, name: &str| DeviceChoice {
            id: id.into(),
            name: name.into(),
        };
        let headset = device("headset", "Arctis");
        let mic = device("mic", "Arctis Mic");
        let open = Open {
            input: mic.clone(),
            output: headset.clone(),
            volume: Arc::new(PlaybackVolume::default()),
            processing: crate::audio_processing::Settings::default(),
            transmission: crate::transmission::Settings::default(),
            ptt: Arc::new(AtomicBool::new(false)),
            surfaces: Arc::new(Surfaces::default()),
        };
        assert_eq!(stand_in_notice(&open, &mic, &headset), None);
        let speakers = device("speakers", "Speakers");
        let Some(Trouble::Fallback(text)) = stand_in_notice(&open, &mic, &speakers) else {
            panic!("expected a stand-in");
        };
        assert!(
            text.contains("\u{201c}Arctis\u{201d}") && text.contains("\u{201c}Speakers\u{201d}")
        );
        assert!(matches!(
            stand_in_notice(&open, &device("laptop", "Laptop Mic"), &speakers),
            Some(Trouble::Fallback(text)) if text.contains("aren't connected")
        ));
    }
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
                self.core.engine().push_mic(value);
            }
            self.tick();
        }

        /// How much was played over this many ticks, with the device draining between them.
        fn heard(&mut self, ticks: usize) -> f32 {
            let mut total = 0.0;
            for _ in 0..ticks {
                self.tick();
                let played = self.core.engine().drain_playback();
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
    fn a_persons_volume_sets_how_loud_they_are_mixed() {
        let mut rig = rig(crate::transmission::Settings::default());
        let (a, _voice) = rig.line(1);
        rig.hub.set_active(Some(SessionId(1)));
        let mut heard_at = |gain: f32| {
            for packet in talking_packets(40) {
                a.hear(ClientId(42), packet, gain).unwrap();
            }
            let heard = rig.heard(30);
            *a.incoming.lock().unwrap() = AudioHandler::new();
            heard
        };
        let full = heard_at(1.0);
        assert!(full > 0.0);
        assert_eq!(heard_at(0.0), 0.0, "muted for you");
        let half = heard_at(0.5);
        assert!(half > 0.0 && half < full, "{half} against {full}");
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
        assert_eq!(rig.core.engine().codec, CodecType::OpusMusic);
        // A background server's codec is not applied until it's active.
        *b.codec.lock().unwrap() = (CodecType::OpusVoice, 4);
        rig.tick();
        assert_eq!(rig.core.engine().codec, CodecType::OpusMusic);
        rig.hub.set_active(Some(SessionId(2)));
        rig.tick();
        assert_eq!(rig.core.engine().codec, CodecType::OpusVoice);
    }
}
