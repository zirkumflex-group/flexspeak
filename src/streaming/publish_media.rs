use super::{
    protocol,
    publishing::{Options, ShareSource, Still},
};
use anyhow::Result;
use serde_json::Value;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::{mpsc, watch};

#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PeerKey {
    pub client: u16,
    pub serial: u64,
}

#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
pub enum Input {
    Add(PeerKey),
    Remove(PeerKey),
    Signal(PeerKey, protocol::Input),
}
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
pub enum Output {
    /// The first frame was sent. Says whether the experimental fast capture produced it and
    /// whether hardware AV1 is offered to viewers.
    Ready {
        fast_capture: bool,
        hardware_av1: bool,
    },
    Offer(PeerKey, String),
    Signal(PeerKey, Value),
    Connected(PeerKey),
    Left(PeerKey),
    Stats {
        video: u64,
        audio: u64,
        level: f32,
    },
    Failed(String),
}
pub struct Publisher {
    pub input: mpsc::Sender<Input>,
    pub output: mpsc::Receiver<Output>,
    pub stop: Stop,
}
#[derive(Debug, Clone)]
pub struct Stop {
    cancel: watch::Sender<bool>,
    active: Arc<AtomicBool>,
}
impl Stop {
    pub fn stop(&self) {
        self.active.store(false, Ordering::Release);
        self.cancel.send_replace(true);
    }
    pub fn stopped(&self) -> bool {
        *self.cancel.borrow()
    }
}
impl Drop for Publisher {
    fn drop(&mut self) {
        self.stop.stop();
    }
}

#[cfg(test)]
pub fn test_worker() -> (
    Publisher,
    mpsc::Receiver<Input>,
    mpsc::Sender<Output>,
    Arc<AtomicBool>,
) {
    let (input, commands) = mpsc::channel(64);
    let (events, output) = mpsc::channel(128);
    let (cancel, _) = watch::channel(false);
    let active = Arc::new(AtomicBool::new(true));
    (
        Publisher {
            input,
            output,
            stop: Stop {
                cancel,
                active: active.clone(),
            },
        },
        commands,
        events,
        active,
    )
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn sources() -> Result<Vec<ShareSource>> {
    anyhow::bail!("Screen sharing is available on Mac and Windows")
}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn quick_sources() -> Result<Vec<ShareSource>> {
    Ok(Vec::new())
}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub async fn still(_: ShareSource) -> Result<Option<Still>> {
    Ok(None)
}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn start(_: Options) -> Result<Publisher> {
    anyhow::bail!("Screen sharing is available on Mac and Windows")
}

/// One picture of a source for the share dialog. `None` when it can't be known in advance.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub async fn still(source: ShareSource) -> Result<Option<Still>> {
    if source.kind.is_camera() {
        native::camera_access().await?;
    }
    tokio::task::spawn_blocking(move || native::still(&source)).await?
}

/// What one click can share: the main screen, then the game being played.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub fn quick_sources() -> Result<Vec<ShareSource>> {
    native::quick_sources()
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub fn sources() -> Result<Vec<ShareSource>> {
    let mut sources = native::sources()?;
    // The picker resolves display labels; disambiguate identical camera/window names.
    let mut counts = std::collections::HashMap::new();
    for source in &sources {
        *counts.entry(source.name.clone()).or_insert(0) += 1;
    }
    for (index, source) in sources.iter_mut().enumerate() {
        if counts[&source.name] > 1 {
            source.name = format!("{} ({})", source.name, index + 1);
        }
    }
    Ok(sources)
}
#[cfg(target_os = "macos")]
pub fn request_system_audio_access(device: crate::audio::DeviceChoice) -> Result<()> {
    native::request_system_audio_access(device)
}
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub fn start(options: Options) -> Result<Publisher> {
    let (input, commands) = mpsc::channel(64);
    let (events, output) = mpsc::channel(128);
    let (cancel, cancelled) = watch::channel(false);
    let active = Arc::new(AtomicBool::new(true));
    let running = active.clone();
    std::thread::Builder::new()
        .name("stream-publisher".into())
        .spawn(move || {
            let result = (|| -> Result<()> {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?;
                runtime.block_on(native::run(options, commands, cancelled, running, &events))
            })();
            if let Err(error) = result {
                let _ = events.try_send(Output::Failed(error.to_string()));
            }
        })?;
    Ok(Publisher {
        input,
        output,
        stop: Stop { cancel, active },
    })
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod native {
    use super::super::{
        MAX_CANDIDATES, camera,
        publishing::{MAX_VIEWERS, Quality, SourceKind},
    };
    use super::*;
    use crate::audio::{self, RateConverter};
    use anyhow::{Context, anyhow, bail, ensure};
    use cpal::{
        FromSample, Sample, SampleFormat, SizedSample,
        traits::{DeviceTrait, StreamTrait},
    };
    use crossbeam_queue::ArrayQueue;
    use futures::{FutureExt, StreamExt, future::LocalBoxFuture, stream::FuturesUnordered};
    /// macOS's stand-in id for its main display, which it finds by itself.
    #[cfg(target_os = "macos")]
    const MAIN_DISPLAY: u64 = 0;

    use libwebrtc::{
        MediaType,
        audio_frame::AudioFrame,
        audio_source::{AudioSourceOptions, native::NativeAudioSource},
        audio_track::RtcAudioTrack,
        desktop_capturer::{
            CaptureError, DesktopCaptureSourceType, DesktopCapturer, DesktopCapturerOptions,
        },
        ice_candidate::IceCandidate,
        media_stream_track::MediaStreamTrack,
        peer_connection::{OfferOptions, PeerConnection, PeerConnectionState},
        peer_connection_factory::{
            ContinualGatheringPolicy, IceServer, PeerConnectionFactory, RtcConfiguration,
            native::PeerConnectionFactoryExt,
        },
        rtp_parameters::{DegradationPreference, RtpCodecCapability},
        rtp_transceiver::{RtpTransceiverDirection, RtpTransceiverInit},
        session_description::{SdpType, SessionDescription},
        video_frame::{I420Buffer, VideoFrame, VideoRotation},
        video_source::{VideoResolution, native::NativeVideoSource},
        video_track::RtcVideoTrack,
    };
    use std::{
        borrow::Cow,
        collections::HashMap,
        sync::Mutex,
        time::{Duration, Instant},
    };

    /// With `picker`, macOS asks the user what to share; without it, the app says.
    fn capturer(window: bool, picker: bool) -> Result<DesktopCapturer> {
        #[cfg(target_os = "macos")]
        let source_type = match (picker, window) {
            (true, _) => DesktopCaptureSourceType::Generic,
            (false, true) => DesktopCaptureSourceType::Window,
            (false, false) => DesktopCaptureSourceType::Screen,
        };
        #[cfg(target_os = "windows")]
        let _ = picker;
        #[cfg(target_os = "windows")]
        let source_type = if window {
            DesktopCaptureSourceType::Window
        } else {
            DesktopCaptureSourceType::Screen
        };
        let mut options = DesktopCapturerOptions::new(source_type);
        options.set_include_cursor(true);
        #[cfg(target_os = "macos")]
        options.set_sck_system_picker(picker);
        DesktopCapturer::new(options).ok_or_else(|| {
            anyhow!("Screen capture is unavailable. Check screen-recording permission.")
        })
    }
    /// The screen source the system calls primary. Its position among display devices is
    /// also the capturer's id for it.
    #[cfg(target_os = "windows")]
    fn primary_screen() -> Option<u64> {
        use windows_sys::Win32::Graphics::Gdi::{
            DISPLAY_DEVICE_PRIMARY_DEVICE, DISPLAY_DEVICEW, EnumDisplayDevicesW,
        };
        (0..16_u32).find_map(|index| {
            // SAFETY: zeroed is valid for this plain struct, whose size is set as the call needs.
            unsafe {
                let mut device: DISPLAY_DEVICEW = std::mem::zeroed();
                device.cb = std::mem::size_of::<DISPLAY_DEVICEW>() as u32;
                (EnumDisplayDevicesW(std::ptr::null(), index, &mut device, 0) != 0
                    && device.StateFlags & DISPLAY_DEVICE_PRIMARY_DEVICE != 0)
                    .then_some(u64::from(index))
            }
        })
    }

    pub fn quick_sources() -> Result<Vec<ShareSource>> {
        #[cfg(target_os = "macos")]
        let main = Some(MAIN_DISPLAY);
        #[cfg(target_os = "windows")]
        let main = {
            let screens = capturer(false, false)?.get_source_list();
            let primary = primary_screen();
            screens
                .iter()
                .find(|screen| Some(screen.id()) == primary)
                .or(screens.first())
                .map(|screen| screen.id())
        };
        let mut quick: Vec<ShareSource> = main
            .map(|id| ShareSource {
                kind: SourceKind::Screen(Some(id)),
                name: "Main screen".into(),
            })
            .into_iter()
            .collect();
        let is_game = super::super::games::detector();
        quick.extend(
            capturer(true, false)?
                .get_source_list()
                .into_iter()
                .find(|window| is_game(window.id()))
                .map(|window| ShareSource {
                    kind: SourceKind::Window(window.id()),
                    name: window
                        .title()
                        .chars()
                        .filter(|c| !c.is_control())
                        .take(160)
                        .collect(),
                }),
        );
        Ok(quick)
    }

    pub async fn camera_access() -> Result<()> {
        camera::request_access().await
    }

    pub fn still(source: &ShareSource) -> Result<Option<Still>> {
        let (window, id) = match &source.kind {
            SourceKind::Camera(id) => return camera::still(id).map(Some),
            // The system picker chooses the screen or window only once sharing starts.
            SourceKind::Screen(None) => return Ok(None),
            SourceKind::Screen(Some(id)) => (false, *id),
            SourceKind::Window(id) => (true, *id),
        };
        let mut desktop = capturer(window, false)?;
        let selected = desktop
            .get_source_list()
            .into_iter()
            .find(|source| source.id() == id)
            .ok_or_else(|| anyhow!("The selected screen or window is no longer available"))?;
        let taken = std::sync::Arc::new(Mutex::new(None));
        let slot = taken.clone();
        desktop.start_capture(Some(selected), move |frame| {
            if let Ok(frame) = frame {
                *slot.lock().unwrap() =
                    Still::from_bgra(frame.data(), frame.stride(), frame.width(), frame.height());
            }
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            desktop.capture_frame();
            if let Some(still) = taken.lock().unwrap().take() {
                return Ok(Some(still));
            }
            ensure!(Instant::now() < deadline, "No picture arrived in time");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn sources() -> Result<Vec<ShareSource>> {
        #[cfg(target_os = "macos")]
        {
            let mut sources = vec![ShareSource {
                kind: SourceKind::Screen(None),
                name: "Choose a screen or window…".into(),
            }];
            sources.extend(camera::sources());
            Ok(sources)
        }
        #[cfg(target_os = "windows")]
        {
            let mut sources = Vec::new();
            for window in [false, true] {
                for (index, source) in capturer(window, false)?
                    .get_source_list()
                    .into_iter()
                    .take(128)
                    .enumerate()
                {
                    let name = source
                        .title()
                        .chars()
                        .filter(|c| !c.is_control())
                        .take(160)
                        .collect::<String>();
                    sources.push(ShareSource {
                        kind: if window {
                            SourceKind::Window(source.id())
                        } else {
                            SourceKind::Screen(Some(source.id()))
                        },
                        name: if name.is_empty() {
                            format!("Screen {}", index + 1)
                        } else {
                            name
                        },
                    });
                }
            }
            sources.extend(camera::sources());
            Ok(sources)
        }
    }
    #[cfg(test)]
    fn video_buffer(
        data: &[u8],
        stride: u32,
        width: i32,
        height: i32,
        (max_width, max_height): (u32, u32),
    ) -> Result<I420Buffer> {
        video_buffer_reusing(
            data,
            stride,
            width,
            height,
            (max_width, max_height),
            &mut None,
        )
    }

    fn video_buffer_reusing(
        data: &[u8],
        stride: u32,
        width: i32,
        height: i32,
        (max_width, max_height): (u32, u32),
        intermediate: &mut Option<I420Buffer>,
    ) -> Result<I420Buffer> {
        ensure!(
            (1..=8192).contains(&width) && (1..=8192).contains(&height),
            "Unsupported capture dimensions"
        );
        ensure!(
            stride >= width as u32 * 4
                && stride <= 131072
                && data.len() >= stride as usize * height as usize,
            "Invalid capture buffer"
        );
        let mut buffer = intermediate
            .take()
            .filter(|buffer| {
                use libwebrtc::video_frame::VideoBuffer;
                buffer.width() == width as u32 && buffer.height() == height as u32
            })
            .unwrap_or_else(|| I420Buffer::new(width as u32, height as u32));
        let (sy, su, sv) = buffer.strides();
        let (y, u, v) = buffer.data_mut();
        // DesktopFrame stores little-endian ARGB (BGRA bytes), including row padding.
        libwebrtc::native::yuv_helper::argb_to_i420(
            data, stride, y, sy, u, su, v, sv, width, height,
        );
        let scale = (f64::from(max_width) / width as f64)
            .min(f64::from(max_height) / height as f64)
            .min(1.0);
        if scale < 1.0 {
            let scaled = buffer.scale(
                ((width as f64 * scale) as i32 / 2 * 2).max(2),
                ((height as f64 * scale) as i32 / 2 * 2).max(2),
            );
            // Only the scaled destination escapes the callback.
            *intermediate = Some(buffer);
            Ok(scaled)
        } else {
            Ok(buffer)
        }
    }

    struct SystemAudio {
        _stream: cpal::Stream,
        queue: Arc<ArrayQueue<[f32; 2]>>,
        errors: Arc<ArrayQueue<String>>,
        converter: RateConverter,
    }
    impl SystemAudio {
        fn open(selected: &audio::DeviceChoice, active: Arc<AtomicBool>) -> Result<Self> {
            let device = audio::find_device(selected, false)?;
            // CPAL treats a duplex Mac endpoint as microphone input, never as loopback.
            #[cfg(target_os = "macos")]
            ensure!(
                !device.supports_input(),
                "This output also exposes microphone input. Choose an output-only system-audio device, such as the Mac speakers."
            );
            let config = device.default_output_config()?;
            ensure!(
                config.channels() > 0 && config.sample_rate() >= 8000,
                "Unsupported system-audio format"
            );
            let queue = Arc::new(ArrayQueue::new(config.sample_rate() as usize / 4));
            let errors = Arc::new(ArrayQueue::new(4));
            macro_rules! stream {
                ($t:ty) => {
                    loopback::<$t>(&device, &config, queue.clone(), errors.clone(), active)
                };
            }
            let stream = match config.sample_format() {
                SampleFormat::F32 => stream!(f32), SampleFormat::I16 => stream!(i16),
                SampleFormat::I32 => stream!(i32), SampleFormat::U16 => stream!(u16),
                SampleFormat::F64 => stream!(f64), other => bail!("Unsupported system-audio format: {other}"),
            }.context("Cannot capture system audio. Check system-audio recording permission and the selected output device.")?;
            let converter = RateConverter::new(
                config.sample_rate(),
                48000,
                2,
                config.sample_rate() as usize / 100,
            )?;
            stream.play()?;
            Ok(Self {
                _stream: stream,
                queue,
                errors,
                converter,
            })
        }
        fn next(&mut self) -> Result<Option<(AudioFrame<'static>, f32)>> {
            if let Some(error) = self.errors.pop() {
                bail!("System audio stopped: {error}");
            }
            if self.queue.len() < self.converter.input[0].len() {
                return Ok(None);
            }
            let mut level = 0.0_f32;
            for i in 0..self.converter.input[0].len() {
                let frame = self.queue.pop().unwrap_or([0.0; 2]);
                for (c, sample) in frame.into_iter().enumerate() {
                    self.converter.input[c][i] = sample;
                    level = level.max(sample.abs());
                }
            }
            let (samples, count) = self.converter.process()?;
            let mut data = Vec::with_capacity(count * 2);
            for i in 0..count {
                for channel in samples.iter().take(2) {
                    data.push((channel[i].clamp(-1.0, 1.0) * 32767.0) as i16);
                }
            }
            Ok(Some((
                AudioFrame {
                    data: Cow::Owned(data),
                    sample_rate: 48000,
                    num_channels: 2,
                    samples_per_channel: count as u32,
                },
                level,
            )))
        }
    }
    #[cfg(target_os = "macos")]
    pub(super) fn request_system_audio_access(device: audio::DeviceChoice) -> Result<()> {
        // Starting tap I/O triggers macOS consent. A false flag discards every callback sample.
        let access = SystemAudio::open(&device, Arc::new(AtomicBool::new(false)))?;
        drop(access);
        Ok(())
    }
    fn stereo_sample<T: SizedSample>(frame: &[T]) -> [f32; 2]
    where
        f32: FromSample<T>,
    {
        let sample = |value| {
            let value = f32::from_sample(value);
            if value.is_finite() {
                value.clamp(-1.0, 1.0)
            } else {
                0.0
            }
        };
        match frame {
            [left, right] => [sample(*left), sample(*right)],
            _ => {
                // CPAL exposes channel count, not a speaker map; retain every surround channel.
                let mono = frame.iter().map(|value| sample(*value)).sum::<f32>()
                    / frame.len().max(1) as f32;
                [mono, mono]
            }
        }
    }
    fn loopback<T: SizedSample>(
        device: &cpal::Device,
        config: &cpal::SupportedStreamConfig,
        queue: Arc<ArrayQueue<[f32; 2]>>,
        errors: Arc<ArrayQueue<String>>,
        active: Arc<AtomicBool>,
    ) -> Result<cpal::Stream>
    where
        f32: FromSample<T>,
    {
        let channels = config.channels() as usize;
        Ok(device.build_input_stream(
            config.config(),
            move |data: &[T], _| {
                if !active.load(Ordering::Acquire) {
                    return;
                }
                for frame in data.chunks_exact(channels) {
                    // Prefer recent sound to a growing backlog when encoding stalls.
                    let _ = queue.force_push(stereo_sample(frame));
                }
            },
            move |error| audio::report_stream_error(error, &errors),
            Some(Duration::from_secs(5)),
        )?)
    }

    enum Callback {
        Candidate(PeerKey, Value),
        State(PeerKey, PeerConnectionState),
    }
    struct Peer {
        pc: PeerConnection,
        answered: bool,
        /// Its answer is being applied; candidates wait for it.
        answering: bool,
        candidates: Vec<protocol::Input>,
        started: Instant,
        candidate_count: usize,
        /// When the connection last dropped to `Disconnected`, which it can recover from.
        disconnected: Option<Instant>,
    }
    /// How long one signaling step of a viewer may take.
    const NEGOTIATION_TIMEOUT: Duration = Duration::from_secs(10);
    /// How long a viewer's connection may stay `Disconnected` before it counts as gone.
    const RECONNECT_GRACE: Duration = Duration::from_secs(10);
    /// How often a share nobody watches yet captures, to keep its checks alive.
    const IDLE_FRAME: Duration = Duration::from_secs(1);
    /// A finished signaling step of one viewer.
    enum Step {
        Offered(Peer, String),
        Answered,
        Candidates,
    }
    type Negotiation = LocalBoxFuture<'static, (PeerKey, Result<Step>)>;
    /// Runs a viewer's signaling step beside the media, bounded by `NEGOTIATION_TIMEOUT`.
    fn step(id: PeerKey, work: impl Future<Output = Result<Step>> + 'static) -> Negotiation {
        async move {
            let result = tokio::time::timeout(NEGOTIATION_TIMEOUT, work)
                .await
                .unwrap_or_else(|_| Err(anyhow!("Viewer negotiation timed out")));
            (id, result)
        }
        .boxed_local()
    }
    async fn add_candidates(pc: PeerConnection, inputs: Vec<protocol::Input>) -> Result<Step> {
        for input in inputs {
            add_candidate(&pc, input).await;
        }
        Ok(Step::Candidates)
    }
    /// One viewer's answer or candidate. An error ends only that viewer.
    fn signal(
        peer: &mut Peer,
        id: PeerKey,
        signal: protocol::Input,
        negotiating: &mut FuturesUnordered<Negotiation>,
    ) -> Result<()> {
        match signal {
            protocol::Input::Answer(sdp) if !peer.answered && !peer.answering => {
                let answer = SessionDescription::parse(&sdp, SdpType::Answer)?;
                let pc = peer.pc.clone();
                peer.answering = true;
                negotiating.push(step(id, async move {
                    pc.set_remote_description(answer).await?;
                    Ok(Step::Answered)
                }));
            }
            candidate @ protocol::Input::Candidate { .. } => {
                peer.candidate_count += 1;
                ensure!(
                    peer.candidate_count <= MAX_CANDIDATES,
                    "Too many viewer candidates"
                );
                if peer.answered {
                    negotiating.push(step(id, add_candidates(peer.pc.clone(), vec![candidate])));
                } else {
                    peer.candidates.push(candidate);
                }
            }
            _ => {}
        }
        Ok(())
    }
    /// Ends one viewer's connection and tells the publisher it left.
    fn leave(
        peers: &mut HashMap<PeerKey, Peer>,
        id: PeerKey,
        events: &mpsc::Sender<Output>,
        reason: &anyhow::Error,
    ) -> Result<()> {
        tracing::warn!("Viewer {id:?} left the share: {reason:#}");
        peers.remove(&id);
        emit(events, Output::Left(id))
    }
    impl Drop for Peer {
        fn drop(&mut self) {
            self.pc.close();
        }
    }
    fn emit(events: &mpsc::Sender<Output>, event: Output) -> Result<()> {
        events
            .try_send(event)
            .map_err(|_| anyhow!("The publisher event queue is unavailable"))
    }
    async fn add_candidate(pc: &PeerConnection, input: protocol::Input) {
        if let protocol::Input::Candidate { sdp, mid, mline } = input
            && let Ok(candidate) = IceCandidate::parse(&mid, mline, &sdp)
        {
            let _ = pc.add_ice_candidate(candidate).await;
        }
    }
    /// Offered video codecs, most preferred first. With `hardware_av1` (an experimental
    /// option) on Windows, AV1 leads when a hardware encoder (NVENC) offers it: the build
    /// leaves libaom's software AV1 encoder out, because it is slower than VP8. macOS's
    /// VideoToolbox factory can still list a software AV1 encoder, so there AV1 is not
    /// offered at all. VP8 always follows, for viewers that cannot decode AV1.
    fn video_codecs(
        factory: &PeerConnectionFactory,
        hardware_av1: bool,
    ) -> Vec<RtpCodecCapability> {
        let codecs = factory.get_rtp_sender_capabilities(MediaType::Video).codecs;
        let named = |name: &'static str| {
            codecs
                .iter()
                .filter(move |c| c.mime_type.eq_ignore_ascii_case(name))
                .cloned()
        };
        named("video/AV1")
            .filter(|_| hardware_av1 && cfg!(target_os = "windows"))
            .chain(named("video/VP8"))
            .collect()
    }
    #[allow(clippy::too_many_arguments)]
    async fn add_peer(
        factory: &PeerConnectionFactory,
        video: &RtcVideoTrack,
        audio: &RtcAudioTrack,
        id: PeerKey,
        callbacks: mpsc::Sender<Callback>,
        overflow: Arc<AtomicBool>,
        servers: Vec<String>,
        quality: Quality,
        codecs: &[RtpCodecCapability],
    ) -> Result<(Peer, String)> {
        let mut config = RtcConfiguration::default();
        config.continual_gathering_policy = ContinualGatheringPolicy::GatherOnce;
        if !servers.is_empty() {
            config.ice_servers = vec![IceServer {
                urls: servers,
                username: String::new(),
                password: String::new(),
            }];
        }
        let pc = factory.create_peer_connection(config)?;
        let peer = Peer {
            pc,
            answered: false,
            answering: false,
            candidates: vec![],
            started: Instant::now(),
            candidate_count: 0,
            disconnected: None,
        };
        let tx = callbacks.clone();
        let full = overflow.clone();
        peer.pc.on_ice_candidate(Some(Box::new(move |c| {
            if tx.try_send(Callback::Candidate(id,serde_json::json!({"cmd":"iceCandidate","args":{"sdp":c.candidate(),"mid":c.sdp_mid(),"mLine":c.sdp_mline_index()}}))).is_err() { full.store(true,Ordering::Relaxed); }
        })));
        peer.pc
            .on_connection_state_change(Some(Box::new(move |state| {
                if callbacks.try_send(Callback::State(id, state)).is_err() {
                    overflow.store(true, Ordering::Relaxed);
                }
            })));
        let init = || RtpTransceiverInit {
            direction: RtpTransceiverDirection::SendOnly,
            stream_ids: vec!["screen-share".into()],
            send_encodings: vec![],
        };
        let transceiver = peer
            .pc
            .add_transceiver(MediaStreamTrack::Video(video.clone()), init())?;
        let codecs = codecs.to_vec();
        ensure!(
            codecs
                .iter()
                .any(|c| c.mime_type.eq_ignore_ascii_case("video/VP8")),
            "The native VP8 encoder is unavailable"
        );
        transceiver.set_codec_preferences(codecs)?;
        peer.pc
            .add_transceiver(MediaStreamTrack::Audio(audio.clone()), init())?;
        let offer = peer.pc.create_offer(OfferOptions::default()).await?;
        let wire = offer.to_string();
        protocol::offer(&wire)?;
        peer.pc.set_local_description(offer).await?;
        for sender in peer.pc.senders() {
            let video = matches!(sender.track(), Some(MediaStreamTrack::Video(_)));
            let mut params = sender.parameters();
            if video {
                params.set_degradation_preference(DegradationPreference::MaintainResolution);
            }
            for encoding in &mut params.encodings {
                encoding.max_bitrate = Some(if video {
                    quality.bitrate()
                } else {
                    quality.audio_bitrate()
                });
                if video {
                    encoding.max_framerate = Some(f64::from(quality.fps()));
                }
            }
            sender.set_parameters(params)?;
        }
        Ok((peer, wire))
    }
    pub async fn run(
        options: Options,
        mut input: mpsc::Receiver<Input>,
        mut cancel: watch::Receiver<bool>,
        active: Arc<AtomicBool>,
        events: &mpsc::Sender<Output>,
    ) -> Result<()> {
        if !active.load(Ordering::Acquire) {
            return Ok(());
        }
        let factory = PeerConnectionFactory::default();
        factory.set_adm_recording_enabled(false);
        factory.set_adm_playout_enabled(false);
        let video = NativeVideoSource::new(
            VideoResolution {
                width: options.quality.max_size().0,
                height: options.quality.max_size().1,
            },
            !options.source.kind.is_camera(),
        );
        let video_track = factory.create_video_track("shared-video", video.clone());
        let codecs = video_codecs(&factory, options.hardware_av1);
        let hardware_av1 = codecs
            .first()
            .is_some_and(|c| c.mime_type.eq_ignore_ascii_case("video/AV1"));
        // The official TS6 client silently drops an offer without an audio section, so a share
        // without system audio still negotiates this track and simply never feeds it.
        let audio = NativeAudioSource::new(AudioSourceOptions::default(), 48000, 2, 50);
        let audio_track = factory.create_audio_track("system-audio", audio.clone());
        let quality = options.quality;
        quality.validate()?;
        let max_size = quality.max_size();
        let frame_interval = Duration::from_secs_f64(1.0 / f64::from(quality.fps()));
        let latest = Arc::new(Mutex::new(None));
        let failure = Arc::new(Mutex::new(None));
        let slot = latest.clone();
        let failed = failure.clone();
        let running = active.clone();
        let intermediate = Mutex::new(None);
        // Until someone watches, a camera's frames are converted only now and then.
        let idle = Arc::new(AtomicBool::new(false));
        let unwatched = idle.clone();
        let last_conversion = Mutex::new(
            Instant::now()
                .checked_sub(IDLE_FRAME)
                .unwrap_or_else(Instant::now),
        );
        let receive = move |data: &[u8], stride, width, height| {
            if !running.load(Ordering::Acquire) {
                return;
            }
            if unwatched.load(Ordering::Relaxed) {
                let mut last = last_conversion.lock().unwrap();
                if last.elapsed() < IDLE_FRAME / 2 {
                    return;
                }
                *last = Instant::now();
            }
            match video_buffer_reusing(
                data,
                stride,
                width,
                height,
                max_size,
                &mut intermediate.lock().unwrap(),
            ) {
                Ok(buffer) => *slot.lock().unwrap() = Some(buffer),
                Err(error) => *failed.lock().unwrap() = Some(error.to_string()),
            }
        };
        let mut capture = None;
        let mut camera_capture = None;
        #[cfg(target_os = "windows")]
        let mut fast = None;
        #[cfg(target_os = "windows")]
        if options.fast_capture {
            let target = match options.source.kind {
                SourceKind::Screen(Some(id)) => Some(crate::streaming::wgc::Target::Screen(id)),
                SourceKind::Window(id) => Some(crate::streaming::wgc::Target::Window(id)),
                _ => None,
            };
            if let Some(target) = target {
                match crate::streaming::wgc::Capture::open(target, max_size, frame_interval) {
                    Ok(started) => fast = Some(started),
                    Err(error) => {
                        tracing::warn!(
                            "Fast capture unavailable, using standard capture: {error:#}"
                        )
                    }
                }
            }
        }
        #[cfg(target_os = "windows")]
        let fast_started = fast.is_some();
        // Fast capture's newest frame. Windows sends frames only when the picture changes,
        // but encoders expect one every tick (a viewer who joins needs one to start from),
        // so it is sent again until a newer one arrives, as the standard capture does.
        #[cfg(target_os = "windows")]
        let mut fast_frame: Option<VideoFrame<I420Buffer>> = None;
        #[cfg(not(target_os = "windows"))]
        let fast_started = false;
        if let SourceKind::Camera(id) = &options.source.kind {
            tokio::select! {
                biased;
                _ = cancel.changed() => return Ok(()),
                result = camera::request_access() => result?,
            }
            if !active.load(Ordering::Acquire) {
                return Ok(());
            }
            camera_capture = Some(camera::Capture::open(id, quality, receive)?);
        } else if !fast_started {
            let (window, id) = match options.source.kind {
                SourceKind::Window(id) => (true, Some(id)),
                SourceKind::Screen(id) => (false, id),
                SourceKind::Camera(_) => unreachable!(),
            };
            let mut desktop = capturer(window, id.is_none())?;
            // Without its picker, macOS captures the main display when given no source.
            #[cfg(target_os = "macos")]
            let id = id.filter(|_| window);
            let selected = id
                .map(|id| {
                    desktop
                        .get_source_list()
                        .into_iter()
                        .find(|source| source.id() == id)
                        .ok_or_else(|| {
                            anyhow!("The selected screen or window is no longer available")
                        })
                })
                .transpose()?;
            let failed = failure.clone();
            desktop.start_capture(selected, move |frame| match frame {
                Ok(frame) => receive(frame.data(), frame.stride(), frame.width(), frame.height()),
                Err(CaptureError::Permanent) => {
                    *failed.lock().unwrap() = Some(
                        "Screen capture stopped. Check permission or select another window.".into(),
                    )
                }
                Err(CaptureError::Temporary) => {}
            });
            capture = Some(desktop);
        }
        let mut system_audio = options
            .audio
            .as_ref()
            .map(|device| SystemAudio::open(device, active.clone()))
            .transpose()?;
        let (callbacks, mut callback_rx) = mpsc::channel(128);
        let overflow = Arc::new(AtomicBool::new(false));
        let mut peers: HashMap<PeerKey, Peer> = HashMap::new();
        // Viewers whose offer is still being made, with the candidates gathered meanwhile.
        let mut adding: HashMap<PeerKey, Vec<Value>> = HashMap::new();
        // Each viewer's signaling runs here, beside the media, so a slow or failing viewer
        // delays and ends only itself.
        let mut negotiating: FuturesUnordered<Negotiation> = FuturesUnordered::new();
        let servers: Vec<String> = vec![
            "stun:turn.teamspeak.com:3478".into(),
            "stun:turn2.teamspeak.com:3478".into(),
        ];
        let start = Instant::now();
        let mut last_video = Instant::now();
        let mut last_stats = Instant::now();
        let mut frames = 0;
        let mut audio_frames = 0;
        let mut level = 0.0_f32;
        let mut tick = tokio::time::interval(if system_audio.is_some() {
            Duration::from_millis(10)
        } else {
            Duration::from_secs(1)
        });
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut video_tick = tokio::time::interval(frame_interval);
        video_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let result = async {
            loop {
                if !active.load(Ordering::Acquire) { return Ok(()); }
                ensure!(!overflow.load(Ordering::Relaxed),"Too many publisher connection events");
                idle.store(peers.is_empty() && adding.is_empty() && frames > 0, Ordering::Relaxed);
                tokio::select! {
                    biased;
                    _ = cancel.changed() => return Ok(()),
                    command = input.recv() => {
                        let Some(command) = command else { return Ok(()); };
                        match command {
                            Input::Add(id) if !peers.contains_key(&id) && !adding.contains_key(&id) => {
                                if peers.len() + adding.len() >= MAX_VIEWERS {
                                    leave(&mut peers, id, events, &anyhow!("Too many viewers"))?;
                                    continue;
                                }
                                adding.insert(id, Vec::new());
                                let (factory, video, audio) = (factory.clone(), video_track.clone(), audio_track.clone());
                                let (callbacks, overflow, servers, codecs) = (callbacks.clone(), overflow.clone(), servers.clone(), codecs.clone());
                                negotiating.push(step(id, async move {
                                    let (peer, offer) = add_peer(&factory,&video,&audio,id,callbacks,overflow,servers,quality,&codecs).await?;
                                    Ok(Step::Offered(peer, offer))
                                }));
                            },
                            Input::Remove(id) => { peers.remove(&id); adding.remove(&id); },
                            Input::Signal(id,input) => {
                                if let Some(Err(error)) = peers.get_mut(&id).map(|peer| signal(peer,id,input,&mut negotiating)) {
                                    leave(&mut peers, id, events, &error)?;
                                }
                            },
                            Input::Add(_) => {},
                        }
                    },
                    Some((id,result)) = negotiating.next(), if !negotiating.is_empty() => match result {
                        // A viewer removed meanwhile drops (and closes) its connection here.
                        Ok(Step::Offered(peer,offer)) => if let Some(gathered) = adding.remove(&id) {
                            let sent = emit(events,Output::Offer(id,offer))
                                .and_then(|()| gathered.into_iter().try_for_each(|value| emit(events,Output::Signal(id,value))));
                            match sent {
                                Ok(()) => { peers.insert(id,peer); },
                                Err(error) => leave(&mut peers, id, events, &error)?,
                            }
                        },
                        Ok(Step::Answered) => if let Some(peer) = peers.get_mut(&id) {
                            peer.answering = false;
                            peer.answered = true;
                            let early = std::mem::take(&mut peer.candidates);
                            if !early.is_empty() { negotiating.push(step(id, add_candidates(peer.pc.clone(), early))); }
                        },
                        Ok(Step::Candidates) => {},
                        Err(error) => if adding.remove(&id).is_some() || peers.contains_key(&id) {
                            leave(&mut peers, id, events, &error)?;
                        },
                    },
                    Some(callback) = callback_rx.recv() => match callback {
                        Callback::Candidate(id,value) => if let Some(gathered) = adding.get_mut(&id) {
                            if gathered.len() < MAX_CANDIDATES { gathered.push(value); }
                        } else if peers.contains_key(&id) && let Err(error) = emit(events,Output::Signal(id,value)) {
                            leave(&mut peers, id, events, &error)?;
                        },
                        Callback::State(id,PeerConnectionState::Connected) => if let Some(peer) = peers.get_mut(&id) {
                            peer.disconnected = None;
                            emit(events,Output::Connected(id))?;
                        },
                        // Disconnected can recover by itself; the tick ends it after a grace.
                        Callback::State(id,PeerConnectionState::Disconnected) => if let Some(peer) = peers.get_mut(&id) {
                            peer.disconnected.get_or_insert_with(Instant::now);
                        },
                        Callback::State(id,PeerConnectionState::Failed | PeerConnectionState::Closed) if peers.contains_key(&id) => {
                            leave(&mut peers, id, events, &anyhow!("The connection ended"))?;
                        },
                        _ => {},
                    },
                    _ = video_tick.tick() => {
                        // Nobody watches yet: an occasional frame keeps the checks and the
                        // first viewer's start, without capturing at full rate for no one.
                        if idle.load(Ordering::Relaxed) && last_video.elapsed() < IDLE_FRAME { continue; }
                        if let Some(capture) = &mut capture { capture.capture_frame(); }
                        #[cfg(target_os = "windows")]
                        if let Some(fast) = &mut fast {
                            if let Some(buffer) = fast.poll()? {
                                fast_frame = Some(VideoFrame::new(VideoRotation::VideoRotation0,buffer));
                            }
                            if let Some(frame) = &mut fast_frame {
                                frame.timestamp_us = start.elapsed().as_micros() as i64;
                                video.capture_frame(frame); last_video = Instant::now(); frames += 1;
                                if frames == 1 { emit(events,Output::Ready { fast_capture: true, hardware_av1 })?; }
                            }
                        }
                        if let Some(buffer) = latest.lock().unwrap().take() {
                            let mut frame = VideoFrame::new(VideoRotation::VideoRotation0,buffer);
                            frame.timestamp_us = start.elapsed().as_micros() as i64;
                            video.capture_frame(&frame); last_video = Instant::now(); frames += 1;
                            if frames == 1 { emit(events,Output::Ready { fast_capture: fast_started, hardware_av1 })?; }
                        }
                    }
                    _ = tick.tick() => {
                        if let Some(error) = failure.lock().unwrap().take() { bail!(error); }
                        let stalled_camera = options.source.kind.is_camera() && frames > 0 && last_video.elapsed() > Duration::from_secs(5);
                        ensure!(!stalled_camera, "Camera stopped delivering video. Check its connection and start sharing again.");
                        ensure!(frames > 0 || last_video.elapsed() < Duration::from_secs(60),"No video frames arrived. Check capture permission and choose an available source.");
                        if let Some(capture) = &mut system_audio {
                            for _ in 0..4 {
                                let Some((frame,peak)) = capture.next()? else { break; };
                                if !peers.is_empty() {
                                    tokio::select! { _ = cancel.changed() => return Ok(()), result = tokio::time::timeout(Duration::from_secs(1),audio.capture_frame(&frame)) => result.context("System audio delivery stalled")?? }
                                }
                                level = level.max(peak); audio_frames += 1;
                            }
                        }
                        let expired: Vec<_> = peers.iter().filter_map(|(id,p)| {
                            let unanswered = !p.answered && p.started.elapsed() > Duration::from_secs(45);
                            let lost = p.disconnected.is_some_and(|at| at.elapsed() > RECONNECT_GRACE);
                            (unanswered || lost).then_some(*id)
                        }).collect();
                        for id in expired { leave(&mut peers, id, events, &anyhow!("It did not answer or reconnect in time"))?; }
                        if last_stats.elapsed() >= Duration::from_secs(1) {
                            let _ = events.try_send(Output::Stats { video:frames,audio:audio_frames,level });
                            level = 0.0; last_stats = Instant::now();
                        }
                    }
                }
            }
        }.await;
        active.store(false, Ordering::Release);
        video_track.set_enabled(false);
        audio_track.set_enabled(false);
        audio.clear_buffer();
        peers.clear();
        drop(capture);
        drop(camera_capture);
        drop(system_audio);
        result
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::streaming::{Surface, rtc};
        use libwebrtc::video_frame::VideoBuffer;
        #[test]
        fn system_audio_keeps_stereo_and_surround_center_without_invalid_samples() {
            assert_eq!(stereo_sample(&[0.75_f32, -0.25]), [0.75, -0.25]);
            assert_eq!(stereo_sample(&[0.5_f32]), [0.5, 0.5]);
            assert_eq!(
                stereo_sample(&[0.0_f32, 0.0, 0.6, 0.0, 0.0, 0.0]),
                [0.1, 0.1]
            );
            assert_eq!(stereo_sample(&[f32::NAN, f32::INFINITY]), [0.0, 0.0]);
        }
        const HD: (u32, u32) = (1280, 720);

        #[test]
        fn capture_conversion_preserves_bgra_channels_padding_and_bounds() {
            let mut pixels = vec![0_u8; 24 * 4];
            for row in pixels.as_chunks_mut::<24>().0 {
                for pixel in row[..16].as_chunks_mut::<4>().0 {
                    pixel.copy_from_slice(&[0, 0, 255, 255]);
                }
            }
            let buffer = video_buffer(&pixels, 24, 4, 4, HD).unwrap();
            let (y, u, v) = buffer.data();
            assert!(y[0] > 60 && y[0] < 100);
            assert!(v[0] > 200 && u[0] < 130, "Red must not become blue");
            assert!(video_buffer(&pixels[..5], 24, 4, 4, HD).is_err());
            assert!(video_buffer(&pixels, 2, 4, 4, HD).is_err());
            assert!(video_buffer(&pixels, 24, -1, 4, HD).is_err());
            let pixels = vec![128; 1920 * 1080 * 4];
            let scaled = video_buffer(&pixels, 1920 * 4, 1920, 1080, HD).unwrap();
            assert_eq!((scaled.width(), scaled.height()), (1280, 720));
            let full = video_buffer(
                &pixels,
                1920 * 4,
                1920,
                1080,
                super::super::super::publishing::Preset::Sharp
                    .quality()
                    .max_size(),
            );
            assert_eq!(full.map(|b| (b.width(), b.height())).unwrap(), (1920, 1080));
            let saver = video_buffer(
                &pixels,
                1920 * 4,
                1920,
                1080,
                super::super::super::publishing::Preset::Saver
                    .quality()
                    .max_size(),
            );
            assert_eq!(saver.map(|b| (b.width(), b.height())).unwrap(), (852, 480));
        }

        /// The codec name of the first payload type on the SDP's video line.
        fn first_video_codec(sdp: &str) -> Option<String> {
            let payload = sdp
                .lines()
                .find(|l| l.starts_with("m=video"))?
                .split(' ')
                .nth(3)?;
            let prefix = format!("a=rtpmap:{payload} ");
            let rtpmap = sdp.lines().find_map(|l| l.strip_prefix(prefix.as_str()))?;
            Some(rtpmap.split('/').next()?.to_owned())
        }

        #[test]
        fn first_video_codec_reads_the_preferred_payload() {
            let sdp = "v=0
m=audio 9 UDP/TLS/RTP/SAVPF 111
m=video 9 UDP/TLS/RTP/SAVPF 45 96
a=rtpmap:96 VP8/90000
a=rtpmap:45 AV1/90000
";
            assert_eq!(first_video_codec(sdp).as_deref(), Some("AV1"));
            assert_eq!(
                first_video_codec(
                    "v=0
"
                ),
                None
            );
        }

        #[tokio::test]
        #[ignore = "Native codec/UDP integration; generated pixels and audio only, no capture or server"]
        async fn publisher_sends_generated_video_and_stereo_audio() -> Result<()> {
            let factory = PeerConnectionFactory::default();
            factory.set_adm_recording_enabled(false);
            factory.set_adm_playout_enabled(false);
            let video = NativeVideoSource::new(
                VideoResolution {
                    width: 320,
                    height: 180,
                },
                true,
            );
            let video_track = factory.create_video_track("test-screen", video.clone());
            let audio = NativeAudioSource::new(AudioSourceOptions::default(), 48000, 2, 50);
            let audio_track = factory.create_audio_track("test-system-audio", audio.clone());
            let (callbacks, mut callback_rx) = mpsc::channel(128);
            let key = PeerKey {
                client: 7,
                serial: 1,
            };
            let (sender, sdp) = add_peer(
                &factory,
                &video_track,
                &audio_track,
                key,
                callbacks,
                Arc::new(AtomicBool::new(false)),
                vec![],
                Quality::default(),
                &video_codecs(&factory, true),
            )
            .await?;
            ensure!(
                sdp.contains("m=video") && sdp.contains("m=audio"),
                "Offer must contain both tracks"
            );
            let surface = Arc::new(Surface::default());
            let generation = surface.begin();
            let mut receiver =
                rtc::start_with_servers(sdp, vec![], generation, surface.clone(), vec![])?;
            ensure!(
                video_codecs(&factory, false)
                    .iter()
                    .all(|c| c.mime_type.eq_ignore_ascii_case("video/VP8")),
                "Without the hardware AV1 option only VP8 may be offered"
            );
            let expected = if video_codecs(&factory, true)[0]
                .mime_type
                .eq_ignore_ascii_case("video/AV1")
            {
                "AV1"
            } else {
                "VP8"
            };
            let mut negotiated = None;
            let mut answered = false;
            let mut pending = Vec::new();
            let mut decoded_video = 0;
            let mut decoded_audio = 0;
            let mut peak = 0.0_f32;
            let mut distinct = HashMap::new();
            let start = Instant::now();
            let mut count = 0_u64;
            let mut tick = tokio::time::interval(Duration::from_millis(10));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            tokio::time::timeout(Duration::from_secs(20),async {
                loop {
                    tokio::select! {
                        Some(callback) = callback_rx.recv() => if let Callback::Candidate(_,value) = callback
                            && let Some(candidate) = protocol::incoming_signal(&value.to_string())? {
                                receiver.input.send(candidate).await?;
                            },
                        event = receiver.output.recv() => match event {
                            Some(rtc::Output::Signal(value)) if value["cmd"] == "answer" => {
                                let sdp = value["args"]["answer"].as_str().context("Missing answer")?;
                                ensure!(!sdp.contains("a=sendrecv") && !sdp.contains("a=sendonly"),"Receiver must not send media");
                                negotiated = first_video_codec(sdp);
                                sender.pc.set_remote_description(SessionDescription::parse(sdp,SdpType::Answer)?).await?;
                                answered = true;
                                for candidate in pending.drain(..) { add_candidate(&sender.pc,candidate).await; }
                            },
                            Some(rtc::Output::Signal(value)) => if let Some(candidate) = protocol::incoming_signal(&value.to_string())? {
                                if answered { add_candidate(&sender.pc,candidate).await; } else { pending.push(candidate); }
                            },
                            Some(rtc::Output::Failed(error)) => bail!(error),
                            Some(_) => {},
                            None => bail!("Receiver stopped"),
                        },
                        _ = tick.tick() => {
                            count += 1;
                            if count.is_multiple_of(3) {
                                let mut buffer = I420Buffer::new(320,180);
                                let (y,u,v) = buffer.data_mut(); y.fill(60+(count%100) as u8); u.fill(90); v.fill(220);
                                let mut frame = VideoFrame::new(VideoRotation::VideoRotation0,buffer); frame.timestamp_us = start.elapsed().as_micros() as i64;
                                video.capture_frame(&frame);
                            }
                            let mut data = Vec::with_capacity(960);
                            for i in 0..480 { let sample = (((count*480+i) as f64 * 440.0 * std::f64::consts::TAU/48000.0).sin()*8000.0) as i16; data.extend([sample,sample]); }
                            audio.capture_frame(&AudioFrame {data:Cow::Owned(data),sample_rate:48000,num_channels:2,samples_per_channel:480}).await?;
                            if let Some(crate::streaming::Image { pixels, .. }) = surface.take() {
                                let pixels: &[u8] = pixels.as_ref(); distinct.insert(pixels[(90*320+160)*4],()); decoded_video += 1;
                            }
                            let mut pcm = [0.0_f32;960]; surface.mix_audio(&mut pcm);
                            let block_peak = pcm.iter().map(|v| v.abs()).fold(0.0_f32,f32::max);
                            if block_peak > 0.01 { decoded_audio += 1; } peak = peak.max(block_peak);
                        },
                    }
                    if decoded_video>=8 && decoded_audio>=20 && distinct.len()>=2 && peak>0.03 { break; }
                }
                Ok::<_,anyhow::Error>(())
            }).await.context("Generated publishing test timed out")??;
            ensure!(
                !factory.adm_recording_enabled(),
                "The publishing test must never enable microphone capture"
            );
            ensure!(
                negotiated.as_deref() == Some(expected),
                "Expected {expected} video, negotiated {negotiated:?}"
            );
            println!(
                "Publisher proof: {expected} video, {decoded_video} decoded video frames, {} distinct levels, {decoded_audio} stereo audio blocks, peak {peak}; no capture devices or server",
                distinct.len()
            );
            video_track.set_enabled(false);
            audio_track.set_enabled(false);
            audio.clear_buffer();
            drop(sender);
            let mut receiver_output = std::mem::replace(&mut receiver.output, mpsc::channel(1).1);
            drop(receiver);
            tokio::time::timeout(Duration::from_secs(5), async {
                while receiver_output.recv().await.is_some() {}
            })
            .await?;
            assert!(surface.take().is_none());
            let mut pcm = [0.0; 960];
            surface.mix_audio(&mut pcm);
            assert!(pcm.iter().all(|v| *v == 0.0));
            Ok(())
        }
    }
}
