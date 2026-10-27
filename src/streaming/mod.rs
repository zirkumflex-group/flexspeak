mod protocol;
mod rtc;
mod session;

use iced::widget::image::Handle;
pub(crate) use rtc::Output as MediaEvent;
pub use session::Session;
use std::sync::{Arc, Mutex, atomic::AtomicBool};

pub const AVAILABLE: bool = cfg!(target_os = "macos");
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
    pub catalog: Vec<StreamInfo>,
    pub stream: Option<StreamInfo>,
    pub generation: u64,
    pub phase: Option<Phase>,
    pub detail: String,
    pub notice: String,
    pub stats: Stats,
    pub frame: Option<Frame>,
    allocation: Option<iced_runtime::image::Allocation>,
    uploading: Option<(u64, u64)>,
    pub ui_updates: u64,
    pub surface: Arc<Surface>,
    pub visible: Arc<AtomicBool>,
}

impl View {
    pub fn active(&self) -> bool {
        self.phase.is_some_and(Phase::active)
    }
    pub fn reset(&mut self) {
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
        self.allocation = None;
        // Keep the in-flight slot occupied until its callback arrives, even across a restart.
    }
    pub fn update(&mut self, event: Event) {
        match event {
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
    pub fn take_frame(&mut self) -> Option<Frame> {
        if self.uploading.is_none()
            && self.active()
            && let Some(frame) = self.surface.take()
            && frame.generation == self.generation
            && self
                .frame
                .as_ref()
                .is_none_or(|previous| frame.sequence > previous.sequence)
        {
            self.uploading = Some((frame.generation, frame.sequence));
            return Some(frame);
        }
        None
    }
    pub fn frame_uploaded(
        &mut self,
        frame: Frame,
        result: Result<iced_runtime::image::Allocation, iced_runtime::image::Error>,
    ) -> Option<bool> {
        if self.uploading != Some((frame.generation, frame.sequence)) {
            return None;
        }
        self.uploading = None;
        if frame.generation != self.generation
            || !self.active()
            || !self.surface.is_active(frame.generation)
        {
            return None;
        }
        match result {
            Ok(allocation) => {
                // Retain the previous image until the renderer guarantees the next is drawable.
                self.allocation = Some(allocation);
                self.frame = Some(frame);
                self.ui_updates += 1;
                Some(true)
            }
            Err(_) => Some(false),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub handle: Handle,
    pub generation: u64,
    pub sequence: u64,
    pub rotation: f32,
}

#[derive(Default)]
struct SurfaceState {
    generation: u64,
    active: bool,
    sequence: u64,
    frame: Option<Frame>,
}

/// One latest frame, never a queue of image buffers. Old sessions cannot publish into new ones.
#[derive(Default)]
pub struct Surface(Mutex<SurfaceState>);

impl Surface {
    fn is_active(&self, generation: u64) -> bool {
        let state = self.0.lock().unwrap();
        state.active && state.generation == generation
    }
    pub fn begin(&self) -> u64 {
        let mut state = self.0.lock().unwrap();
        state.generation += 1;
        state.active = true;
        state.sequence = 0;
        state.frame = None;
        state.generation
    }
    pub fn end(&self, generation: u64) {
        let mut state = self.0.lock().unwrap();
        if state.generation == generation {
            state.active = false;
            state.frame = None;
        }
    }
    #[cfg(any(target_os = "macos", test))]
    pub fn publish(&self, generation: u64, handle: Handle, rotation: f32) {
        let mut state = self.0.lock().unwrap();
        if state.active && state.generation == generation {
            state.sequence += 1;
            state.frame = Some(Frame {
                handle,
                generation,
                sequence: state.sequence,
                rotation,
            });
        }
    }
    pub fn take(&self) -> Option<Frame> {
        self.0.lock().unwrap().frame.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uploading_a_frame_keeps_the_previous_picture_visible() {
        let mut view = View::default();
        view.generation = view.surface.begin();
        view.phase = Some(Phase::Receiving);
        let previous = Frame {
            handle: Handle::from_rgba(1, 1, vec![255, 0, 0, 255]),
            generation: view.generation,
            sequence: 0,
            rotation: 0.0,
        };
        view.frame = Some(previous.clone());
        view.surface.publish(
            view.generation,
            Handle::from_rgba(1280, 720, vec![255; 1280 * 720 * 4]),
            0.0,
        );
        let uploading = view.take_frame().unwrap();
        assert_eq!(view.frame.as_ref().unwrap().handle, previous.handle);
        assert_eq!(view.ui_updates, 0);
        for _ in 0..3 {
            view.surface
                .publish(view.generation, previous.handle.clone(), 0.0);
        }
        assert!(
            view.take_frame().is_none(),
            "Only one upload may be pending"
        );
        view.frame_uploaded(uploading, Err(iced_runtime::image::Error::OutOfMemory));
        assert_eq!(view.frame.as_ref().unwrap().handle, previous.handle);
        let newest = view.take_frame().unwrap();
        assert_eq!(newest.sequence, 4, "Skipped frames must not form a backlog");
        view.clear_picture();
        view.frame_uploaded(newest, Err(iced_runtime::image::Error::OutOfMemory));
        assert!(view.frame.is_none() && view.allocation.is_none());
        assert!(view.uploading.is_none());
    }

    #[test]
    fn latest_frame_replaces_backlog_and_rejects_closed_or_old_sessions() {
        let surface = Surface::default();
        let frame = || Handle::from_rgba(1, 1, vec![0, 0, 0, 255]);
        let first = surface.begin();
        surface.publish(first, frame(), 0.0);
        surface.publish(first, frame(), 0.0);
        assert_eq!(surface.take().unwrap().sequence, 2);
        assert!(surface.take().is_none());
        let second = surface.begin();
        surface.publish(first, frame(), 0.0);
        surface.end(first);
        surface.publish(second, frame(), 0.0);
        assert_eq!(surface.take().unwrap().generation, second);
        surface.end(second);
        surface.publish(second, frame(), 0.0);
        assert!(surface.take().is_none());
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
