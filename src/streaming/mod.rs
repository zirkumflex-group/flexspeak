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
use std::sync::{Arc, Mutex, atomic::AtomicBool};

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
    State {
        generation: u64,
        stream: StreamInfo,
        phase: Phase,
        detail: String,
    },
    Stats {
        generation: u64,
        stats: Stats,
    },
}

#[derive(Default)]
pub struct View {
    pub sharing: publishing::Ui,
    pub catalog: Vec<StreamInfo>,
    pub stream: Option<StreamInfo>,
    pub generation: u64,
    pub phase: Option<Phase>,
    pub detail: String,
    pub notice: String,
    pub stats: Stats,
    pub frame: Option<Frame>,
    pub ui_updates: u64,
    pub surface: Arc<Surface>,
    pub visible: Arc<AtomicBool>,
}

impl View {
    pub fn active(&self) -> bool {
        self.phase.is_some_and(Phase::active)
    }
    pub fn reset(&mut self) {
        self.sharing.status = publishing::Status::default();
        self.clear_picture();
        self.catalog.clear();
        self.stream = None;
        self.phase = None;
        self.detail.clear();
        self.notice.clear();
        self.stats = Stats::default();
        self.ui_updates = 0;
    }
    pub fn clear_picture(&mut self) {
        self.surface.end(self.generation);
        self.frame = None;
    }
    pub fn update(&mut self, event: Event) {
        match event {
            Event::Publishing(status) => self.sharing.status = status,
            Event::Catalog(catalog) => self.catalog = catalog,
            Event::Notice(notice) => self.notice = notice,
            Event::State {
                generation,
                stream,
                phase,
                detail,
            } if generation >= self.generation => {
                if generation != self.generation {
                    self.clear_picture();
                    self.ui_updates = 0;
                    self.stats = Stats::default();
                }
                if !phase.active() {
                    self.clear_picture();
                }
                self.generation = generation;
                self.stream = Some(stream);
                self.phase = Some(phase);
                self.detail = detail;
            }
            Event::Stats { generation, stats }
                if generation == self.generation && self.active() =>
            {
                self.stats = stats
            }
            _ => {}
        }
    }
    /// Notes the newest frame the decoder showed. Pixels reach the GPU without the UI thread;
    /// this only tracks what the view describes. True when the picture appeared or changed shape.
    pub fn sync_frame(&mut self) -> bool {
        if !self.active() {
            return false;
        }
        let Some(frame) = self.surface.latest() else {
            return false;
        };
        if frame.generation != self.generation {
            return false;
        }
        let previous = self.frame;
        if previous.is_some_and(|previous| frame.sequence <= previous.sequence) {
            return false;
        }
        self.ui_updates += frame.sequence - previous.map_or(0, |previous| previous.sequence);
        self.frame = Some(frame);
        previous.is_none_or(|previous| {
            (previous.width, previous.height, previous.rotation)
                != (frame.width, frame.height, frame.rotation)
        })
    }
}

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

/// One latest frame, never a queue of image buffers. Old sessions cannot publish into new ones.
#[derive(Default)]
pub struct Surface {
    state: Mutex<SurfaceState>,
    pixels: rune::Pixels,
    waker: Mutex<Option<rune::Waker>>,
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
    pub fn begin(&self) -> u64 {
        let mut state = self.state.lock().unwrap();
        state.generation += 1;
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
        for frame in stereo.as_chunks_mut::<2>().0 {
            let Some([left, right]) = state.audio.pop_front() else {
                break;
            };
            frame[0] = (frame[0] + left).clamp(-1.0, 1.0);
            frame[1] = (frame[1] + right).clamp(-1.0, 1.0);
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
    #[test]
    fn the_view_tracks_the_newest_frame_and_only_wakes_for_shape_changes() {
        let mut view = View::default();
        let wakes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = wakes.clone();
        view.surface.set_waker(Arc::new(move || {
            counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }));
        view.generation = view.surface.begin();
        view.phase = Some(Phase::Receiving);
        let image = |w: u32, h: u32| Image {
            width: w,
            height: h,
            pixels: vec![255; (w * h * 4) as usize],
        };
        assert!(!view.sync_frame(), "No picture before the first frame");
        view.surface.publish(view.generation, image(4, 2), 0.0);
        assert!(view.sync_frame(), "The first frame shows the picture");
        assert_eq!(view.surface.pixels().size(), (4, 2));
        for _ in 0..3 {
            view.surface.publish(view.generation, image(4, 2), 0.0);
        }
        assert!(
            !view.sync_frame(),
            "Same-shape frames redraw without the view"
        );
        assert_eq!(view.ui_updates, 4);
        assert_eq!(wakes.load(std::sync::atomic::Ordering::Relaxed), 1);
        view.surface.publish(view.generation, image(2, 4), 0.0);
        assert!(view.sync_frame());
        assert_eq!(wakes.load(std::sync::atomic::Ordering::Relaxed), 2);
        view.clear_picture();
        assert!(view.frame.is_none());
        assert_eq!(view.surface.pixels().size(), (0, 0));
        view.surface.publish(view.generation, image(4, 2), 0.0);
        assert!(
            !view.sync_frame(),
            "A closed session cannot bring the picture back"
        );
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
        let info = StreamInfo {
            id: "stream".into(),
            publisher: 7,
            name: "Window".into(),
            viewers: 1,
            has_audio: false,
            server_relayed: false,
        };
        let mut view = View::default();
        view.update(Event::State {
            generation: 2,
            stream: info.clone(),
            phase: Phase::Receiving,
            detail: String::new(),
        });
        view.update(Event::Stats {
            generation: 1,
            stats: Stats {
                packets: 99,
                ..Default::default()
            },
        });
        view.update(Event::State {
            generation: 1,
            stream: info.clone(),
            phase: Phase::Failed,
            detail: "Old session".into(),
        });
        assert_eq!(view.phase, Some(Phase::Receiving));
        assert_eq!(view.stats.packets, 0);
        view.update(Event::State {
            generation: 2,
            stream: info,
            phase: Phase::Stopped,
            detail: "Stopped".into(),
        });
        view.update(Event::Stats {
            generation: 2,
            stats: Stats {
                packets: 99,
                ..Default::default()
            },
        });
        assert_eq!(view.stats.packets, 0);
        assert!(!view.active() && view.frame.is_none());
    }
}
