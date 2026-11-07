mod protocol;
mod publish_media;
pub mod publishing;
mod rtc;
mod session;
#[cfg(target_os = "macos")]
pub use publish_media::request_system_audio_access;
pub use publish_media::sources as share_sources;

pub(crate) use rtc::Output as MediaEvent;
pub use session::Session;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

pub const AVAILABLE: bool = cfg!(any(target_os = "macos", target_os = "windows"));
pub const MAX_SDP_BYTES: usize = 64 * 1024;
pub const MAX_CANDIDATES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamInfo {
    pub id: String,
    pub publisher: u16,
    pub name: String,
    pub viewers: u32,
    pub has_audio: bool,
    pub server_relayed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Requesting,
    Negotiating,
    Receiving,
    Stopped,
    Failed,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Requesting => "Waiting for the publisher",
            Self::Negotiating => "Connecting video",
            Self::Receiving => "Receiving video",
            Self::Stopped => "Not watching",
            Self::Failed => "Stream unavailable",
        }
    }
    pub fn active(self) -> bool {
        matches!(self, Self::Requesting | Self::Negotiating | Self::Receiving)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub publisher_version: String,
    pub codec: String,
    pub codec_parameters: String,
    pub decoder: String,
    pub ice_path: String,
    pub packets: u64,
    pub bytes: u64,
    pub decoded_frames: u64,
    pub fps: f64,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone)]
pub enum Event {
    Publishing(publishing::Status),
    Catalog(Vec<StreamInfo>),
    Notice(String),
    /// One watched stream's progress. `slot` is the [`Surfaces`] entry its picture goes to.
    State {
        generation: u64,
        slot: usize,
        stream: StreamInfo,
        phase: Phase,
        detail: String,
    },
    Stats {
        generation: u64,
        stats: Stats,
    },
}

/// The most streams watched at once; each has its own picture and audio.
pub const MAX_WATCHES: usize = 4;

/// One stream being watched, as the view knows it.
#[derive(Debug, Clone)]
pub struct Watching {
    pub stream: StreamInfo,
    pub slot: usize,
    pub generation: u64,
    pub phase: Phase,
    pub detail: String,
    pub stats: Stats,
    pub frame: Option<Frame>,
    pub ui_updates: u64,
}

pub struct View {
    pub sharing: publishing::Ui,
    pub catalog: Vec<StreamInfo>,
    /// Watched streams, oldest first.
    pub watching: Vec<Watching>,
    /// The stream filling the stage, by ID; the others shrink to a strip beside it.
    pub focused: Option<String>,
    /// Codec, resolution and transport details over each picture.
    pub show_stats: bool,
    /// The share dialog is open.
    pub share_open: bool,
    /// Each publisher's stream volume in percent, kept for the session.
    pub volumes: std::collections::HashMap<u16, u8>,
    /// Generations stopped from this window, so a late state from them can't bring them back.
    stopped: u64,
    pub notice: String,
    pub surfaces: Arc<Surfaces>,
    pub visible: Arc<AtomicBool>,
}

impl Default for View {
    fn default() -> Self {
        Self {
            sharing: publishing::Ui::default(),
            catalog: Vec::new(),
            watching: Vec::new(),
            focused: None,
            show_stats: false,
            share_open: false,
            volumes: std::collections::HashMap::new(),
            stopped: 0,
            notice: String::new(),
            surfaces: Arc::new(Surfaces::default()),
            visible: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl View {
    pub fn active(&self) -> bool {
        !self.watching.is_empty()
    }
    pub fn watching(&self, id: &str) -> Option<&Watching> {
        self.watching.iter().find(|watch| watch.stream.id == id)
    }
    /// The stream on the stage: the focused one, or the only one.
    pub fn stage(&self) -> Option<&Watching> {
        match &self.focused {
            Some(id) => self.watching(id),
            None if self.watching.len() == 1 => self.watching.first(),
            None => None,
        }
    }
    /// A watched stream's volume in percent; streams start at full volume.
    pub fn volume(&self, publisher: u16) -> u8 {
        self.volumes.get(&publisher).copied().unwrap_or(100)
    }
    /// Sets a publisher's stream volume, and remembers it for the session.
    pub fn set_volume(&mut self, publisher: u16, percent: u8) {
        let percent = percent.min(100);
        self.volumes.insert(publisher, percent);
        for watch in &self.watching {
            if watch.stream.publisher == publisher {
                self.surfaces
                    .get(watch.slot)
                    .set_volume(f32::from(percent) / 100.0);
            }
        }
    }
    pub fn reset(&mut self) {
        self.sharing.status = publishing::Status::default();
        self.clear_picture();
        self.catalog.clear();
        self.notice.clear();
        self.share_open = false;
    }
    /// Ends every picture and forgets the watched streams; the session stops them too.
    pub fn clear_picture(&mut self) {
        for watch in self.watching.drain(..) {
            self.surfaces.get(watch.slot).end(watch.generation);
            self.stopped = self.stopped.max(watch.generation);
        }
        self.focused = None;
    }
    /// Forgets one watched stream at once, before the session confirms it stopped.
    pub fn forget(&mut self, id: &str) {
        if let Some(index) = self.watching.iter().position(|watch| watch.stream.id == id) {
            let watch = self.watching.remove(index);
            self.surfaces.get(watch.slot).end(watch.generation);
            self.stopped = self.stopped.max(watch.generation);
        }
        if self.focused.as_deref() == Some(id) {
            self.focused = None;
        }
    }
    pub fn update(&mut self, event: Event) {
        match event {
            Event::Publishing(status) => self.sharing.status = status,
            Event::Catalog(catalog) => self.catalog = catalog,
            Event::Notice(notice) => self.notice = notice,
            Event::State {
                generation,
                slot,
                stream,
                phase,
                detail,
            } => {
                let index = self.watching.iter().position(|watch| {
                    watch.generation == generation || watch.stream.id == stream.id
                });
                if let Some(index) = index
                    && self.watching[index].generation > generation
                {
                    return;
                }
                if !phase.active() {
                    if let Some(index) = index {
                        let watch = self.watching.remove(index);
                        if self.focused.as_deref() == Some(watch.stream.id.as_str()) {
                            self.focused = None;
                        }
                    }
                    return;
                }
                if index.is_none() && generation <= self.stopped {
                    return;
                }
                let publisher = stream.publisher;
                let entry = Watching {
                    stream,
                    slot,
                    generation,
                    phase,
                    detail,
                    stats: Stats::default(),
                    frame: None,
                    ui_updates: 0,
                };
                match index {
                    Some(index) if self.watching[index].generation == generation => {
                        let watch = &mut self.watching[index];
                        watch.stream = entry.stream;
                        watch.phase = phase;
                        watch.detail = entry.detail;
                    }
                    Some(index) => self.watching[index] = entry,
                    None => self.watching.push(entry),
                }
                let volume = self.volume(publisher);
                self.surfaces
                    .get(slot)
                    .set_volume(f32::from(volume) / 100.0);
            }
            Event::Stats { generation, stats } => {
                if let Some(watch) = self
                    .watching
                    .iter_mut()
                    .find(|watch| watch.generation == generation)
                {
                    watch.stats = stats;
                }
            }
        }
    }
    /// Notes the newest frame each decoder showed. Pixels reach the GPU without the UI thread;
    /// this only tracks what the view describes. True when a picture appeared or changed shape.
    pub fn sync_frame(&mut self) -> bool {
        let mut changed = false;
        for watch in &mut self.watching {
            let Some(frame) = self.surfaces.get(watch.slot).latest() else {
                continue;
            };
            if frame.generation != watch.generation {
                continue;
            }
            let previous = watch.frame;
            if previous.is_some_and(|previous| frame.sequence <= previous.sequence) {
                continue;
            }
            watch.ui_updates += frame.sequence - previous.map_or(0, |previous| previous.sequence);
            watch.frame = Some(frame);
            changed |= previous.is_none_or(|previous| {
                (previous.width, previous.height, previous.rotation)
                    != (frame.width, frame.height, frame.rotation)
            });
        }
        changed
    }
}

/// One surface per watched stream, shared with the session and the audio callback.
pub struct Surfaces {
    slots: [Arc<Surface>; MAX_WATCHES],
}

impl Default for Surfaces {
    fn default() -> Self {
        Self {
            slots: std::array::from_fn(|_| Arc::new(Surface::default())),
        }
    }
}

impl Surfaces {
    pub fn get(&self, slot: usize) -> &Arc<Surface> {
        &self.slots[slot.min(MAX_WATCHES - 1)]
    }
    pub fn set_waker(&self, waker: rune::Waker) {
        for surface in &self.slots {
            surface.set_waker(waker.clone());
        }
    }
    pub fn set_audio_enabled(&self, enabled: bool) {
        for surface in &self.slots {
            surface.set_audio_enabled(enabled);
        }
    }
    /// Adds every watched stream's audio, each at its own volume.
    pub fn mix_audio(&self, stereo: &mut [f32]) {
        for surface in &self.slots {
            surface.mix_audio(stereo);
        }
    }
}

/// Generations are unique across surfaces, so a stream's events can't reach another's view.
static NEXT_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Decoded RGBA, rows top first. Only the native receivers (macOS, Windows) produce one.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// What the view knows about the picture; the pixels themselves live in [`Surface::pixels`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub generation: u64,
    pub sequence: u64,
    pub rotation: f32,
    pub width: u32,
    pub height: u32,
}

#[derive(Default)]
struct SurfaceState {
    generation: u64,
    active: bool,
    sequence: u64,
    frame: Option<Frame>,
    #[cfg(test)]
    image: Option<Image>,
    audio: std::collections::VecDeque<[f32; 2]>,
    audio_disabled: bool,
}

/// A stream's own volume, 0–100%, as `f32` bits.
struct Volume(std::sync::atomic::AtomicU32);

impl Default for Volume {
    fn default() -> Self {
        Self(std::sync::atomic::AtomicU32::new(1.0_f32.to_bits()))
    }
}

/// One latest frame, never a queue of image buffers. Old sessions cannot publish into new ones.
#[derive(Default)]
pub struct Surface {
    state: Mutex<SurfaceState>,
    pixels: rune::Pixels,
    waker: Mutex<Option<rune::Waker>>,
    volume: Volume,
}

impl Surface {
    /// The image the stream view draws. Publishing replaces it in place.
    pub fn pixels(&self) -> &rune::Pixels {
        &self.pixels
    }
    /// Wakes the window when the picture first appears or changes shape.
    pub fn set_waker(&self, waker: rune::Waker) {
        *self.waker.lock().unwrap() = Some(waker);
    }
    fn release_pixels(&self) {
        if self.pixels.size() != (0, 0) {
            self.pixels.set(0, 0, Vec::new());
        }
    }
    /// This stream's playback volume, 0–1, on top of the app's playback volume.
    pub fn set_volume(&self, volume: f32) {
        self.volume
            .0
            .store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }
    pub fn volume(&self) -> f32 {
        f32::from_bits(self.volume.0.load(Ordering::Relaxed))
    }
    pub fn begin(&self) -> u64 {
        let mut state = self.state.lock().unwrap();
        state.generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
        state.active = true;
        state.sequence = 0;
        state.frame = None;
        state.audio.clear();
        let generation = state.generation;
        drop(state);
        self.release_pixels();
        generation
    }
    pub fn end(&self, generation: u64) {
        let mut state = self.state.lock().unwrap();
        if state.generation == generation {
            state.active = false;
            state.frame = None;
            state.audio.clear();
            drop(state);
            self.release_pixels();
        }
    }
    #[cfg(any(target_os = "macos", target_os = "windows", test))]
    pub fn publish(&self, generation: u64, image: Image, rotation: f32) {
        let mut state = self.state.lock().unwrap();
        if !state.active || state.generation != generation {
            return;
        }
        state.sequence += 1;
        let frame = Frame {
            generation,
            sequence: state.sequence,
            rotation,
            width: image.width,
            height: image.height,
        };
        let reshaped = state.frame.is_none_or(|previous| {
            (previous.width, previous.height, previous.rotation)
                != (frame.width, frame.height, frame.rotation)
        });
        state.frame = Some(frame);
        #[cfg(test)]
        {
            state.image = Some(image.clone());
        }
        // Hold the lock while replacing the pixels, so a concurrent `end` cannot be overtaken.
        self.pixels.set(image.width, image.height, image.pixels);
        drop(state);
        if reshaped && let Some(waker) = self.waker.lock().unwrap().as_ref() {
            waker();
        }
    }
    /// The newest frame's description, if a session is showing one.
    pub fn latest(&self) -> Option<Frame> {
        self.state.lock().unwrap().frame
    }
    /// The newest decoded image, once. Test builds keep a copy for checking decoded pixels.
    #[cfg(test)]
    pub fn take(&self) -> Option<Image> {
        self.state.lock().unwrap().image.take()
    }
    pub fn set_audio_enabled(&self, enabled: bool) {
        let mut state = self.state.lock().unwrap();
        state.audio_disabled = !enabled;
        if !enabled {
            state.audio.clear();
        }
    }
    #[cfg(any(target_os = "macos", target_os = "windows", test))]
    pub fn publish_audio(&self, generation: u64, pcm: &[i16]) {
        let mut state = self.state.lock().unwrap();
        if !state.active || state.generation != generation || state.audio_disabled {
            return;
        }
        for frame in pcm.as_chunks::<2>().0 {
            if state.audio.len() == 3840 {
                state.audio.pop_front();
            }
            state
                .audio
                .push_back([f32::from(frame[0]) / 32768.0, f32::from(frame[1]) / 32768.0]);
        }
    }
    pub fn mix_audio(&self, stereo: &mut [f32]) {
        let mut state = self.state.lock().unwrap();
        if !state.active || state.audio_disabled {
            return;
        }
        let volume = self.volume();
        for frame in stereo.as_chunks_mut::<2>().0 {
            let Some([left, right]) = state.audio.pop_front() else {
                break;
            };
            frame[0] = (frame[0] + left * volume).clamp(-1.0, 1.0);
            frame[1] = (frame[1] + right * volume).clamp(-1.0, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stream_audio_is_bounded_scoped_and_deafen_discards_backlog() {
        let surface = Surface::default();
        let first = surface.begin();
        surface.publish_audio(first, &vec![16384; 20000]);
        assert_eq!(surface.state.lock().unwrap().audio.len(), 3840);
        let mut mixed = [0.25, -0.25];
        surface.mix_audio(&mut mixed);
        assert_eq!(mixed, [0.75, 0.25]);
        surface.set_audio_enabled(false);
        surface.publish_audio(first, &[16384; 4]);
        surface.set_audio_enabled(true);
        let mut silent = [0.0; 2];
        surface.mix_audio(&mut silent);
        assert_eq!(silent, [0.0; 2]);
        let next = surface.begin();
        surface.publish_audio(first, &[16384; 4]);
        surface.mix_audio(&mut silent);
        assert_eq!(silent, [0.0; 2]);
        surface.publish_audio(next, &[16384; 4]);
        surface.end(next);
        surface.mix_audio(&mut silent);
        assert_eq!(silent, [0.0; 2]);
    }
    fn info(id: &str, publisher: u16) -> StreamInfo {
        StreamInfo {
            id: id.into(),
            publisher,
            name: "Window".into(),
            viewers: 1,
            has_audio: false,
            server_relayed: false,
        }
    }

    fn receiving(view: &mut View, id: &str, slot: usize) -> u64 {
        let generation = view.surfaces.get(slot).begin();
        view.update(Event::State {
            generation,
            slot,
            stream: info(id, 7),
            phase: Phase::Receiving,
            detail: String::new(),
        });
        generation
    }

    #[test]
    fn the_view_tracks_the_newest_frame_and_only_wakes_for_shape_changes() {
        let mut view = View::default();
        let wakes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = wakes.clone();
        view.surfaces.set_waker(Arc::new(move || {
            counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }));
        let generation = receiving(&mut view, "a", 0);
        let surface = view.surfaces.get(0).clone();
        let image = |w: u32, h: u32| Image {
            width: w,
            height: h,
            pixels: vec![255; (w * h * 4) as usize],
        };
        assert!(!view.sync_frame(), "No picture before the first frame");
        surface.publish(generation, image(4, 2), 0.0);
        assert!(view.sync_frame(), "The first frame shows the picture");
        assert_eq!(surface.pixels().size(), (4, 2));
        for _ in 0..3 {
            surface.publish(generation, image(4, 2), 0.0);
        }
        assert!(
            !view.sync_frame(),
            "Same-shape frames redraw without the view"
        );
        assert_eq!(view.watching[0].ui_updates, 4);
        assert_eq!(wakes.load(std::sync::atomic::Ordering::Relaxed), 1);
        surface.publish(generation, image(2, 4), 0.0);
        assert!(view.sync_frame());
        assert_eq!(wakes.load(std::sync::atomic::Ordering::Relaxed), 2);
        view.clear_picture();
        assert!(view.watching.is_empty());
        assert_eq!(surface.pixels().size(), (0, 0));
        surface.publish(generation, image(4, 2), 0.0);
        assert!(
            !view.sync_frame(),
            "A closed session cannot bring the picture back"
        );
    }

    #[test]
    fn several_streams_play_side_by_side_and_one_can_take_the_stage() {
        let mut view = View::default();
        let a = receiving(&mut view, "a", 0);
        let b = receiving(&mut view, "b", 1);
        assert_ne!(a, b, "Generations are unique across surfaces");
        assert_eq!(view.watching.len(), 2);
        assert!(view.stage().is_none(), "Two streams share the stage");
        view.focused = Some("b".into());
        assert_eq!(view.stage().map(|w| w.generation), Some(b));
        view.surfaces.get(0).publish(
            a,
            Image {
                width: 2,
                height: 2,
                pixels: vec![0; 16],
            },
            0.0,
        );
        assert!(view.sync_frame());
        assert!(view.watching[0].frame.is_some() && view.watching[1].frame.is_none());
        view.forget("b");
        assert!(view.focused.is_none());
        assert_eq!(view.stage().map(|w| w.generation), Some(a));
        // A late state from the forgotten stream can't bring it back.
        view.update(Event::State {
            generation: b,
            slot: 1,
            stream: info("b", 7),
            phase: Phase::Receiving,
            detail: String::new(),
        });
        assert_eq!(view.watching.len(), 1);
    }

    #[test]
    fn each_stream_mixes_at_its_own_volume() {
        let surfaces = Surfaces::default();
        let a = surfaces.get(0).begin();
        let b = surfaces.get(1).begin();
        surfaces.get(0).publish_audio(a, &[16384, 16384]);
        surfaces.get(1).publish_audio(b, &[16384, 16384]);
        surfaces.get(1).set_volume(0.5);
        let mut mixed = [0.0; 2];
        surfaces.mix_audio(&mut mixed);
        assert_eq!(mixed, [0.75, 0.75]);
    }

    #[test]
    fn latest_frame_replaces_backlog_and_rejects_closed_or_old_sessions() {
        let surface = Surface::default();
        let frame = || Image {
            width: 1,
            height: 1,
            pixels: vec![0, 0, 0, 255],
        };
        let first = surface.begin();
        surface.publish(first, frame(), 0.0);
        surface.publish(first, frame(), 0.0);
        assert_eq!(surface.latest().unwrap().sequence, 2);
        assert!(surface.take().is_some());
        assert!(surface.take().is_none());
        let second = surface.begin();
        surface.publish(first, frame(), 0.0);
        surface.end(first);
        surface.publish(second, frame(), 0.0);
        assert_eq!(surface.latest().unwrap().generation, second);
        surface.end(second);
        surface.publish(second, frame(), 0.0);
        assert!(surface.latest().is_none());
    }

    #[test]
    fn stopped_and_previous_receivers_cannot_replace_the_current_picture_or_stats() {
        let mut view = View::default();
        view.update(Event::State {
            generation: 20,
            slot: 0,
            stream: info("stream", 7),
            phase: Phase::Receiving,
            detail: String::new(),
        });
        view.update(Event::Stats {
            generation: 10,
            stats: Stats {
                packets: 99,
                ..Default::default()
            },
        });
        view.update(Event::State {
            generation: 10,
            slot: 0,
            stream: info("stream", 7),
            phase: Phase::Failed,
            detail: "Old session".into(),
        });
        assert_eq!(view.watching[0].phase, Phase::Receiving);
        assert_eq!(view.watching[0].stats.packets, 0);
        view.update(Event::State {
            generation: 20,
            slot: 0,
            stream: info("stream", 7),
            phase: Phase::Stopped,
            detail: "Stopped".into(),
        });
        view.update(Event::Stats {
            generation: 20,
            stats: Stats {
                packets: 99,
                ..Default::default()
            },
        });
        assert!(!view.active());
    }
}
