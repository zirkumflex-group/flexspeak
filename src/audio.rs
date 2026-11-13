use std::{
    collections::VecDeque,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use crate::audio_processing::{self, Processor, Timed, callback_time, sample_time};
use crate::profiler::{self, Category};
use anyhow::{Context, Result, bail};
use audiopus::{
    Application, Bitrate, Channels, SampleRate,
    coder::{Encoder, GenericCtl},
};
use cpal::{
    Device, FromSample, Sample, SampleFormat, SizedSample, Stream, SupportedStreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use crossbeam_queue::ArrayQueue;
use rubato::{
    Async, FixedAsync, Resampler, SincInterpolationParameters, SincInterpolationType,
    WindowFunction, audioadapter_buffers::direct::SequentialSliceOfVecs,
};
use tsproto_packets::packets::CodecType;

pub const RATE: usize = 48_000;
pub const FRAME: usize = 960;
const BLOCK: usize = 480;
/// Meter reading (the square root of the microphone level) above which you count as speaking.
pub const SPEAKING_METER: f32 = 0.08;

struct Capture {
    samples: ArrayQueue<(u64, Timed<f32>)>,
    generation: AtomicU64,
    level: AtomicU32,
    discontinuity: Arc<AtomicBool>,
}

impl Capture {
    fn enabled(&self) -> bool {
        self.generation.load(Ordering::Acquire) & 1 != 0
    }

    fn clear(&self, enabled: bool) {
        let previous = self.generation.load(Ordering::Acquire);
        self.generation.store(
            (previous.wrapping_add(2) & !1) | u64::from(enabled),
            Ordering::Release,
        );
        while self.samples.pop().is_some() {}
        self.level.store(0.0_f32.to_bits(), Ordering::Relaxed);
    }

    fn pop(&self) -> Option<Timed<f32>> {
        let current = self.generation.load(Ordering::Acquire);
        for _ in 0..self.samples.len() {
            let (generation, sample) = self.samples.pop()?;
            if generation == current && current & 1 != 0 {
                return Some(sample);
            }
        }
        None
    }
}

#[derive(Default)]
struct ResampleClock {
    input: usize,
    output: usize,
}

impl ResampleClock {
    fn advance(
        &mut self,
        at: Option<Instant>,
        input: usize,
        output: usize,
        from: u32,
        delay: usize,
    ) -> Option<Instant> {
        let at = sample_time(at, self.output, RATE as u32)
            .and_then(|at| at.checked_sub(Duration::from_secs_f64(self.input as f64 / from as f64)))
            .and_then(|at| at.checked_sub(Duration::from_secs_f64(delay as f64 / RATE as f64)));
        self.input += input;
        self.output += output;
        at
    }
}

struct Reference {
    samples: Arc<ArrayQueue<Timed<[f32; 2]>>>,
    converter: RateConverter,
    clock: ResampleClock,
    device_rate: u32,
}

impl Reference {
    fn new(rate: u32) -> Result<Self> {
        Ok(Self {
            samples: Arc::new(ArrayQueue::new(rate as usize / 4)),
            converter: RateConverter::new(rate, RATE as u32, 2, rate as usize / 100)?,
            clock: ResampleClock::default(),
            device_rate: rate,
        })
    }

    fn reset(&mut self) {
        while self.samples.pop().is_some() {}
        self.converter.reset();
        self.clock = ResampleClock::default();
    }

    fn process(&mut self, processor: &mut Processor) -> Result<()> {
        let frames = self.converter.input[0].len();
        for _ in 0..self.samples.len() / frames {
            let mut at = None;
            for i in 0..frames {
                let sample = self
                    .samples
                    .pop()
                    .expect("Only the worker consumes reference audio");
                anyhow::ensure!(
                    sample.at.is_some(),
                    "Echo cancellation cannot read playback timing. Disable it or choose another playback device."
                );
                if i == 0 {
                    at = sample.at;
                }
                self.converter.input[0][i] = sample.value[0];
                self.converter.input[1][i] = sample.value[1];
            }
            let delay = self.converter.output_delay();
            let (samples, count) = self.converter.process()?;
            let at = self
                .clock
                .advance(at, frames, count, self.device_rate, delay);
            processor.render(samples, count, at)?;
        }
        Ok(())
    }
}

pub struct PlaybackVolume(AtomicU8);

impl Default for PlaybackVolume {
    fn default() -> Self {
        Self(AtomicU8::new(100))
    }
}

impl PlaybackVolume {
    pub fn percent(&self) -> u8 {
        self.0.load(Ordering::Relaxed)
    }

    pub fn set_percent(&self, percent: u8) {
        self.0.store(percent.min(100), Ordering::Relaxed);
    }

    fn gain(&self) -> f32 {
        f32::from(self.percent()) / 100.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceChoice {
    pub id: String,
    pub name: String,
}

impl fmt::Display for DeviceChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Devices {
    pub inputs: Vec<DeviceChoice>,
    pub outputs: Vec<DeviceChoice>,
    pub input: Option<DeviceChoice>,
    pub output: Option<DeviceChoice>,
}

fn choice(device: &Device) -> Result<DeviceChoice> {
    Ok(DeviceChoice {
        id: device.id()?.to_string(),
        name: device.description()?.name().to_owned(),
    })
}

pub fn devices() -> Result<Devices> {
    let host = cpal::default_host();
    Ok(Devices {
        inputs: host
            .input_devices()?
            .filter_map(|d| choice(&d).ok())
            .collect(),
        outputs: host
            .output_devices()?
            .filter_map(|d| choice(&d).ok())
            .collect(),
        input: host.default_input_device().and_then(|d| choice(&d).ok()),
        output: host.default_output_device().and_then(|d| choice(&d).ok()),
    })
}

/// The chosen device while it's plugged in, else the system's default, else `None`.
pub(crate) fn stand_in(selected: &DeviceChoice, input: bool) -> Option<DeviceChoice> {
    let host = cpal::default_host();
    let devices = if input {
        host.input_devices()
    } else {
        host.output_devices()
    }
    .ok()?;
    let mut devices = devices;
    if devices.any(|d| d.id().is_ok_and(|id| id.to_string() == selected.id)) {
        return Some(selected.clone());
    }
    let default = if input {
        host.default_input_device()
    } else {
        host.default_output_device()
    };
    default.and_then(|d| choice(&d).ok())
}

pub(crate) fn find_device(selected: &DeviceChoice, input: bool) -> Result<Device> {
    let host = cpal::default_host();
    let devices = if input {
        host.input_devices()?
    } else {
        host.output_devices()?
    };
    devices
        .into_iter()
        .find(|d| d.id().is_ok_and(|id| id.to_string() == selected.id))
        .with_context(|| {
            format!(
                "Device '{}' is no longer available. Refresh devices and reconnect.",
                selected.name
            )
        })
}

fn config(device: &Device, input: bool) -> Result<SupportedStreamConfig> {
    // Preserve the hardware's current rate; changing a shared CoreAudio clock
    // can invalidate another stream. Conversion to/from Opus stays in software.
    if input {
        device.default_input_config()
    } else {
        device.default_output_config()
    }
    .map_err(Into::into)
}

pub struct RateConverter {
    resampler: Option<Async<f32>>,
    pub input: Vec<Vec<f32>>,
    output: Vec<Vec<f32>>,
}

impl RateConverter {
    pub fn new(from: u32, to: u32, channels: usize, frames: usize) -> Result<Self> {
        let resampler = if from == to {
            None
        } else {
            // A fixed input chunk: callers fill `input` with `frames` frames and take
            // however many the ratio gives back. The explicit cutoff keeps the filter
            // we have always used; rubato would otherwise pick one per filter length.
            Some(Async::new_sinc(
                to as f64 / from as f64,
                1.0,
                &SincInterpolationParameters {
                    sinc_len: 128,
                    f_cutoff: Some(0.95),
                    interpolation: SincInterpolationType::Linear,
                    oversampling_factor: 128,
                    window: WindowFunction::BlackmanHarris2,
                },
                frames,
                channels,
                FixedAsync::Input,
            )?)
        };
        let output_len = resampler.as_ref().map_or(frames, |r| r.output_frames_max());
        Ok(Self {
            resampler,
            input: vec![vec![0.0; frames]; channels],
            output: vec![vec![0.0; output_len]; channels],
        })
    }

    pub fn process(&mut self) -> Result<(&[Vec<f32>], usize)> {
        if let Some(resampler) = &mut self.resampler {
            let channels = self.input.len();
            let input = SequentialSliceOfVecs::new(&self.input, channels, self.input[0].len())?;
            let frames = self.output[0].len();
            let mut output = SequentialSliceOfVecs::new_mut(&mut self.output, channels, frames)?;
            let (_, written) = resampler.process_into_buffer(&input, &mut output, None)?;
            Ok((&self.output, written))
        } else {
            Ok((&self.input, self.input[0].len()))
        }
    }

    pub fn reset(&mut self) {
        if let Some(resampler) = &mut self.resampler {
            resampler.reset();
        }
        for channel in &mut self.input {
            channel.fill(0.0);
        }
    }

    fn output_delay(&self) -> usize {
        self.resampler
            .as_ref()
            .map_or(0, |resampler| resampler.output_delay())
    }

    fn buffer_bytes(&self) -> usize {
        self.input
            .iter()
            .chain(&self.output)
            .map(|buffer| buffer.capacity() * std::mem::size_of::<f32>())
            .sum()
    }
}

/// The audio devices and everything that works on their samples. One opens at a time: the
/// hub's, shared by every server, or the local microphone test's.
pub struct AudioEngine {
    _streams: Option<(Stream, Stream)>,
    capture: Arc<Capture>,
    playback: Arc<ArrayQueue<[f32; 2]>>,
    errors: Arc<ArrayQueue<String>>,
    processor: Processor,
    voice_gate: crate::transmission::VoiceGate,
    packet_boundary: crate::transmission::PacketBoundary,
    last_frame: Option<Instant>,
    reference: Option<Reference>,
    input_clock: ResampleClock,
    input_device_rate: u32,
    input_len: usize,
    input_at: Option<Instant>,
    input_rate: RateConverter,
    output_rate: RateConverter,
    pcm: VecDeque<f32>,
    monitor: VecDeque<f32>,
    /// Notification sounds still to play, mixed over voice even while deafened.
    chimes: Chimes,
    encoder: Encoder,
    pub codec: CodecType,
    output_target: usize,
    pub description: String,
}

impl AudioEngine {
    pub fn open(
        input: &DeviceChoice,
        output: &DeviceChoice,
        volume: Arc<PlaybackVolume>,
        processing: audio_processing::Settings,
        transmission: crate::transmission::Settings,
    ) -> Result<Self> {
        let voice_gate = crate::transmission::VoiceGate::new(transmission)?;
        let input_device = find_device(input, true)?;
        let output_device = find_device(output, false)?;
        let input_config =
            config(&input_device, true).context("No usable microphone configuration")?;
        let output_config =
            config(&output_device, false).context("No usable playback configuration")?;
        let processor = Processor::new(processing)?;
        tracing::info!(
            input = input.name.as_str(),
            input_rate = input_config.sample_rate(),
            input_channels = input_config.channels(),
            input_format = ?input_config.sample_format(),
            output = output.name.as_str(),
            output_rate = output_config.sample_rate(),
            output_channels = output_config.channels(),
            output_format = ?output_config.sample_format(),
            ?processing,
            ?transmission,
            "audio devices opening"
        );
        let discontinuity = Arc::new(AtomicBool::new(false));
        let capture = Arc::new(Capture {
            samples: ArrayQueue::new(input_config.sample_rate() as usize / 4),
            generation: AtomicU64::new(0),
            level: AtomicU32::new(0.0_f32.to_bits()),
            discontinuity: discontinuity.clone(),
        });
        let reference = processing
            .echo_cancellation
            .then(|| Reference::new(output_config.sample_rate()))
            .transpose()?;
        let playback = Arc::new(ArrayQueue::new(output_config.sample_rate() as usize / 4));
        let errors = Arc::new(ArrayQueue::new(4));

        macro_rules! input_stream {
            ($t:ty) => {
                make_input::<$t>(
                    &input_device,
                    &input_config,
                    capture.clone(),
                    errors.clone(),
                    processing.echo_cancellation,
                )
            };
        }
        let input_stream = match input_config.sample_format() {
            SampleFormat::F32 => input_stream!(f32),
            SampleFormat::I16 => input_stream!(i16),
            SampleFormat::U16 => input_stream!(u16),
            SampleFormat::I32 => input_stream!(i32),
            SampleFormat::F64 => input_stream!(f64),
            format => bail!("Unsupported microphone sample format: {format}"),
        }
        .context("Cannot open microphone. Check microphone permission and device availability.")?;
        macro_rules! output_stream {
            ($t:ty) => {
                make_output::<$t>(
                    &output_device,
                    &output_config,
                    playback.clone(),
                    errors.clone(),
                    volume.clone(),
                    reference
                        .as_ref()
                        .map(|reference| reference.samples.clone()),
                    discontinuity.clone(),
                    true,
                )
            };
        }
        let output_stream = match output_config.sample_format() {
            SampleFormat::F32 => output_stream!(f32),
            SampleFormat::I16 => output_stream!(i16),
            SampleFormat::U16 => output_stream!(u16),
            SampleFormat::I32 => output_stream!(i32),
            SampleFormat::F64 => output_stream!(f64),
            format => bail!("Unsupported playback sample format: {format}"),
        }
        .context("Cannot open playback device")?;
        let input_rate = RateConverter::new(
            input_config.sample_rate(),
            RATE as u32,
            1,
            input_config.sample_rate() as usize / 100,
        )?;
        let output_rate = RateConverter::new(RATE as u32, output_config.sample_rate(), 2, BLOCK)?;
        let description = format!(
            "Input {} Hz / {} ch · Output {} Hz / {} ch · Opus 48 kHz",
            input_config.sample_rate(),
            input_config.channels(),
            output_config.sample_rate(),
            output_config.channels()
        );
        output_stream.play()?;
        input_stream.play()?;
        Self::assemble(
            Some((input_stream, output_stream)),
            capture,
            playback,
            errors,
            processor,
            voice_gate,
            reference,
            input_config.sample_rate(),
            input_rate,
            output_rate,
            output_config.sample_rate() as usize / 50,
            description,
        )
    }

    /// The same engine with no devices behind it: tests feed the capture queue and read the
    /// playback queue directly.
    #[cfg(test)]
    pub fn without_devices(transmission: crate::transmission::Settings) -> Result<Self> {
        let discontinuity = Arc::new(AtomicBool::new(false));
        Self::assemble(
            None,
            Arc::new(Capture {
                samples: ArrayQueue::new(RATE / 4),
                generation: AtomicU64::new(0),
                level: AtomicU32::new(0.0_f32.to_bits()),
                discontinuity,
            }),
            Arc::new(ArrayQueue::new(RATE / 4)),
            Arc::new(ArrayQueue::new(4)),
            Processor::new(audio_processing::Settings::default())?,
            crate::transmission::VoiceGate::new(transmission)?,
            None,
            RATE as u32,
            RateConverter::new(RATE as u32, RATE as u32, 1, RATE / 100)?,
            RateConverter::new(RATE as u32, RATE as u32, 2, BLOCK)?,
            RATE / 50,
            String::new(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn assemble(
        streams: Option<(Stream, Stream)>,
        capture: Arc<Capture>,
        playback: Arc<ArrayQueue<[f32; 2]>>,
        errors: Arc<ArrayQueue<String>>,
        processor: Processor,
        voice_gate: crate::transmission::VoiceGate,
        reference: Option<Reference>,
        input_device_rate: u32,
        input_rate: RateConverter,
        output_rate: RateConverter,
        output_target: usize,
        description: String,
    ) -> Result<Self> {
        let mut encoder = Encoder::new(SampleRate::Hz48000, Channels::Mono, Application::Voip)?;
        encoder.set_bitrate(Bitrate::BitsPerSecond(32_768))?;
        Ok(Self {
            _streams: streams,
            capture,
            playback,
            errors,
            processor,
            voice_gate,
            packet_boundary: crate::transmission::PacketBoundary::default(),
            last_frame: None,
            reference,
            input_clock: ResampleClock::default(),
            input_device_rate,
            input_len: 0,
            input_at: None,
            input_rate,
            output_rate,
            pcm: VecDeque::new(),
            monitor: VecDeque::new(),
            chimes: Chimes::default(),
            encoder,
            codec: CodecType::OpusVoice,
            output_target,
            description,
        })
    }

    /// Takes a new threshold or release delay without reopening the devices. Returns false
    /// for a change of mode, which needs the engine opened again.
    pub fn tune_transmission(&mut self, settings: crate::transmission::Settings) -> bool {
        self.voice_gate.tune(settings)
    }

    pub fn set_capture(&mut self, enabled: bool) -> Result<()> {
        if self.capture.enabled() != enabled {
            self.reset_capture(enabled, false)?;
        }
        Ok(())
    }

    /// Empties the microphone pipeline. `fresh` also rebuilds the processor, which takes long
    /// enough to make playback wait: only a change of server needs that. Muting and a dropped
    /// device buffer keep the one that has already adapted to the room.
    fn reset_capture(&mut self, enabled: bool, fresh: bool) -> Result<()> {
        self.packet_boundary.end();
        self.capture.clear(enabled);
        self.pcm.clear();
        self.monitor.clear();
        self.input_rate.reset();
        self.input_clock = ResampleClock::default();
        self.input_len = 0;
        self.input_at = None;
        if fresh {
            self.processor.reset()?;
        } else {
            self.processor.clear();
        }
        self.voice_gate.reset()?;
        self.last_frame = None;
        if let Some(reference) = &mut self.reference {
            reference.reset();
        }
        Ok(())
    }

    pub fn set_codec(&mut self, codec: CodecType, quality: u8) -> Result<()> {
        let quality = quality.min(10) as usize;
        let bitrate = match codec {
            CodecType::OpusVoice => [
                4096, 8192, 12288, 16384, 20480, 24576, 28672, 32768, 36864, 40960, 45056,
            ][quality],
            CodecType::OpusMusic => [
                7200, 14400, 21600, 28800, 36000, 43200, 50400, 57600, 64800, 72000, 79200,
            ][quality],
            _ => bail!(
                "This prototype supports Opus channels only. Select an Opus Voice or Opus Music channel."
            ),
        };
        self.encoder
            .set_application(if codec == CodecType::OpusMusic {
                Application::Audio
            } else {
                Application::Voip
            })?;
        self.encoder.set_bitrate(Bitrate::BitsPerSecond(bitrate))?;
        self.codec = codec;
        Ok(())
    }

    /// Ends any utterance in progress and clears the microphone pipeline, so nothing spoken to
    /// one server reaches another. Returns the packets that close that utterance.
    pub fn end_utterance(&mut self) -> Result<Vec<Vec<u8>>> {
        self.reset_capture(false, true)?;
        let mut packets = Vec::new();
        self.packet_boundary.flush(&mut packets);
        Ok(packets)
    }

    /// Plays a short sound over the voices, after any sound already going unless it is urgent.
    pub fn play(&mut self, chime: Chime) {
        self.chimes.add(chime);
    }

    /// Whether a notification sound is playing or waiting.
    pub fn sounding(&self) -> bool {
        self.chimes.sounding()
    }

    /// Drops every notification sound, playing or waiting.
    pub fn hush(&mut self) {
        self.chimes = Chimes::default();
    }

    pub fn level(&self) -> f32 {
        f32::from_bits(self.capture.level.load(Ordering::Relaxed))
    }

    pub fn transmitting(&self) -> bool {
        self.capture.enabled()
            && self.voice_gate.active()
            && self
                .last_frame
                .is_some_and(|at| at.elapsed() < Duration::from_millis(100))
    }

    pub fn buffer_bytes(&self) -> usize {
        self.capture.samples.capacity() * std::mem::size_of::<(u64, Timed<f32>)>()
            + self.playback.capacity() * std::mem::size_of::<[f32; 2]>()
            + (self.pcm.capacity() + self.monitor.capacity()) * std::mem::size_of::<f32>()
            + self.input_rate.buffer_bytes()
            + self.output_rate.buffer_bytes()
            + self.voice_gate.buffer_bytes()
            + self.reference.as_ref().map_or(0, |reference| {
                reference.samples.capacity() * std::mem::size_of::<Timed<[f32; 2]>>()
                    + reference.converter.buffer_bytes()
            })
    }

    /// Codec work and resampling happen on the worker, never in an OS audio callback. `mix` adds
    /// whatever should be heard into each stereo block.
    pub fn tick(
        &mut self,
        loopback: bool,
        mut mix: impl FnMut(&mut [f32]),
    ) -> Result<Vec<Vec<u8>>> {
        let _trace = profiler::scope(Category::AudioProcessing, "audio.process");
        if let Some(error) = self.errors.pop() {
            bail!("Audio device failed: {error}");
        }
        if self.capture.discontinuity.swap(false, Ordering::AcqRel) {
            self.reset_capture(self.capture.enabled(), false)?;
        }
        if let Some(reference) = &mut self.reference {
            reference.process(&mut self.processor)?;
        }
        let mut packets = Vec::new();
        self.packet_boundary.flush(&mut packets);
        for _ in 0..self.capture.samples.len() {
            let Some(sample) = self.capture.pop() else {
                break;
            };
            anyhow::ensure!(
                !self.processor.echo_enabled() || sample.at.is_some(),
                "Echo cancellation cannot read microphone timing. Disable it or choose another microphone."
            );
            if self.input_len == 0 {
                self.input_at = sample.at;
            }
            self.input_rate.input[0][self.input_len] = sample.value;
            self.input_len += 1;
            if self.input_len < self.input_rate.input[0].len() {
                continue;
            }
            let delay = self.input_rate.output_delay();
            let (samples, count) = self.input_rate.process()?;
            let at = self.input_clock.advance(
                self.input_at,
                self.input_len,
                count,
                self.input_device_rate,
                delay,
            );
            self.input_len = 0;
            self.processor
                .capture(&samples[0][..count], at, &mut self.pcm)?;
        }
        while self.pcm.len() >= FRAME {
            let mut frame = [0.0; FRAME];
            for sample in &mut frame {
                *sample = self.pcm.pop_front().unwrap_or_default();
            }
            self.voice_gate.process(frame)?;
            self.last_frame = Some(Instant::now());
            while let Some(frame) = self.voice_gate.pop() {
                if loopback {
                    self.monitor.extend(frame);
                    while self.monitor.len() > RATE / 4 {
                        self.monitor.pop_front();
                    }
                } else {
                    if self.packet_boundary.begin(&mut packets) {
                        self.encoder.reset_state()?;
                    }
                    let mut encoded = [0; 1275];
                    let count = self.encoder.encode_float(&frame, &mut encoded)?;
                    packets.push(encoded[..count].to_vec());
                }
            }
            if !self.voice_gate.active() {
                self.packet_boundary.end();
                self.packet_boundary.flush(&mut packets);
            }
        }
        if !self.transmitting() {
            self.packet_boundary.end();
        }
        self.packet_boundary.flush(&mut packets);
        // Keep only ~20 ms of device audio queued, so stop/mute stays responsive.
        for _ in 0..4 {
            if self.playback.len() >= self.output_target {
                break;
            }
            let mut stereo = [0.0; BLOCK * 2];
            if loopback {
                for frame in stereo.as_chunks_mut::<2>().0 {
                    let sample = self.monitor.pop_front().unwrap_or_default();
                    frame.fill(sample);
                }
            } else {
                mix(&mut stereo);
                for frame in stereo.as_chunks_mut::<2>().0 {
                    let Some(sample) = self.chimes.next() else {
                        break;
                    };
                    frame[0] += sample;
                    frame[1] += sample;
                }
                stereo
                    .iter_mut()
                    .for_each(|sample| *sample = soft_limit(*sample));
            }
            for (i, frame) in stereo.as_chunks::<2>().0.iter().enumerate() {
                self.output_rate.input[0][i] = frame[0];
                self.output_rate.input[1][i] = frame[1];
            }
            let (samples, count) = self.output_rate.process()?;
            for (&left, &right) in samples[0].iter().zip(&samples[1]).take(count) {
                let _ = self
                    .playback
                    .push([left.clamp(-1.0, 1.0), right.clamp(-1.0, 1.0)]);
            }
        }
        Ok(packets)
    }
}

#[cfg(test)]
impl AudioEngine {
    pub fn push_mic(&self, value: f32) {
        let generation = self.capture.generation.load(Ordering::Acquire);
        let sample = Timed { value, at: None };
        let _ = self.capture.samples.push((generation, sample));
    }

    pub fn drain_playback(&self) -> Vec<[f32; 2]> {
        std::iter::from_fn(|| self.playback.pop()).collect()
    }
}

/// Eases peaks into the -1..1 range instead of chopping them off, so several servers talking
/// at once stay clean. Anything below the knee is left untouched.
fn soft_limit(sample: f32) -> f32 {
    const KNEE: f32 = 0.8;
    let level = sample.abs();
    if level <= KNEE || !level.is_finite() {
        return sample;
    }
    let room = 1.0 - KNEE;
    sample.signum() * (KNEE + room * ((level - KNEE) / room).tanh())
}

/// One notification sound on its way to the output.
#[derive(Debug, Clone, PartialEq)]
pub struct Chime {
    /// Which sound this is. One that is already playing or waiting isn't added again.
    pub id: u16,
    /// Starts now, in place of whatever was playing: what you just did yourself must answer at
    /// once, and doing it again must not line up behind itself.
    pub urgent: bool,
    pub samples: Vec<f32>,
}

/// The sounds due on the output: one playing, and a short line behind it.
///
/// Sounds wait their turn rather than cut each other off, but only a little: a burst of events
/// is heard as its first few sounds, not as a backlog that plays on after it is over.
#[derive(Debug, Default)]
struct Chimes {
    playing: VecDeque<f32>,
    playing_id: Option<u16>,
    /// How much of the playing sound has been heard.
    played: usize,
    waiting: VecDeque<Chime>,
}

impl Chimes {
    /// How many sounds may wait behind the one playing.
    const WAITING: usize = 2;
    /// A sound that would start later than this says nothing useful any more.
    const LATE: usize = RATE * 3;
    /// The same sound again this soon is the same news, as when several people arrive at once.
    const REPEAT: usize = RATE;
    /// How long a replaced sound takes to fade, so cutting it doesn't click.
    const FADE: usize = RATE / 200;

    fn add(&mut self, chime: Chime) {
        if chime.samples.is_empty() {
            return;
        }
        if chime.urgent {
            self.waiting.retain(|waiting| waiting.id != chime.id);
            let fading: Vec<f32> = self.playing.iter().take(Self::FADE).copied().collect();
            self.start(chime);
            let length = fading.len();
            for (index, (sample, old)) in self.playing.iter_mut().zip(fading).enumerate() {
                *sample += old * (length - index) as f32 / length as f32;
            }
            return;
        }
        if !self.sounding() {
            return self.start(chime);
        }
        let repeated = (self.playing_id == Some(chime.id) && self.played < Self::REPEAT)
            || self.waiting.iter().any(|waiting| waiting.id == chime.id);
        let delay = self.playing.len()
            + self
                .waiting
                .iter()
                .map(|waiting| waiting.samples.len())
                .sum::<usize>();
        if !repeated && self.waiting.len() < Self::WAITING && delay <= Self::LATE {
            self.waiting.push_back(chime);
        }
    }

    fn start(&mut self, chime: Chime) {
        self.playing = chime.samples.into();
        self.playing_id = Some(chime.id);
        self.played = 0;
    }

    fn next(&mut self) -> Option<f32> {
        if self.playing.is_empty() {
            let chime = self.waiting.pop_front()?;
            self.start(chime);
        }
        let sample = self.playing.pop_front();
        self.played += 1;
        if self.playing.is_empty() {
            self.playing_id = None;
        }
        sample
    }

    fn sounding(&self) -> bool {
        !self.playing.is_empty() || !self.waiting.is_empty()
    }
}

/// A short sound the app plays itself. Rising means on and falling means off; the microphone's
/// notes sit higher than the headphones'.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    Poke,
    MicOff,
    MicOn,
    SoundOff,
    SoundOn,
    Joined,
    Moved,
    ViewerJoined,
    ViewerLeft,
    StreamOn,
    StreamOff,
}

/// Sine notes, each with a quick attack and a soft decay.
fn tones(notes: &[(f32, usize)], gain: f32) -> Vec<f32> {
    notes
        .iter()
        .flat_map(|&(frequency, length)| {
            (0..length).map(move |i| {
                let t = i as f32 / RATE as f32;
                let attack = (i as f32 / (RATE as f32 * 0.005)).min(1.0);
                let decay = (-t * 9.0).exp();
                (std::f32::consts::TAU * frequency * t).sin() * attack * decay * gain
            })
        })
        .collect()
}

/// The character of a Soft sound. Each kind of action has its own, so it can be told by ear.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Voice {
    Felt,
    Wood,
    Hum,
    Bell,
    Glass,
    Drop,
}

/// Attack and length in seconds, then each partial's frequency ratio, level and decay.
struct Shape {
    attack: f32,
    seconds: f32,
    partials: [(f32, f32, f32); 3],
}

impl Voice {
    fn shape(self) -> Shape {
        let (attack, seconds, partials) = match self {
            Self::Felt => (
                0.014,
                0.62,
                [(1.0, 1.0, 7.0), (2.0, 0.16, 16.0), (3.0, 0.04, 30.0)],
            ),
            Self::Wood => (
                0.003,
                0.24,
                [(1.0, 1.0, 24.0), (4.0, 0.22, 70.0), (2.0, 0.0, 1.0)],
            ),
            Self::Hum => (
                0.06,
                0.7,
                [(1.0, 1.0, 4.5), (2.0, 0.22, 7.0), (3.0, 0.0, 1.0)],
            ),
            Self::Bell => (
                0.004,
                0.85,
                [(1.0, 1.0, 5.0), (2.76, 0.22, 9.0), (5.4, 0.06, 18.0)],
            ),
            Self::Glass => (
                0.004,
                0.5,
                [(1.0, 1.0, 9.0), (3.0, 0.18, 22.0), (4.2, 0.06, 40.0)],
            ),
            Self::Drop => (
                0.006,
                0.22,
                [(1.0, 1.0, 14.0), (2.0, 0.1, 30.0), (3.0, 0.0, 1.0)],
            ),
        };
        Shape {
            attack,
            seconds,
            partials,
        }
    }
}

/// One Soft sound: MIDI notes this far apart, each bending by `glide` semitones.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SoftCue {
    pub voice: Voice,
    pub notes: &'static [u8],
    pub step_ms: usize,
    pub glide: f32,
    pub gain: f32,
}

pub(crate) fn soft_note(voice: Voice, frequency: f32, glide: f32, gain: f32) -> Vec<f32> {
    let shape = voice.shape();
    let length = (RATE as f32 * shape.seconds) as usize;
    let tau = std::f32::consts::TAU;
    let mut phases = [0.0_f32; 3];
    (0..length)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let bend = 2_f32.powf(glide / 12.0 * (t / (shape.seconds * 0.5)).min(1.0));
            let attack = 0.5 - 0.5 * (std::f32::consts::PI * (t / shape.attack).min(1.0)).cos();
            let fade = ((length - i) as f32 / (RATE as f32 * 0.05)).min(1.0);
            let mut sample = 0.0;
            for (phase, (ratio, level, decay)) in phases.iter_mut().zip(shape.partials) {
                *phase = (*phase + tau * frequency * bend * ratio / RATE as f32) % tau;
                sample += phase.sin() * level * (-t * decay).exp();
            }
            sample * attack * fade * gain
        })
        .collect()
}

/// The notes overlap and settle, each a little quieter than the last.
pub fn soft_samples(cue: SoftCue) -> Vec<f32> {
    let step = RATE * cue.step_ms / 1000;
    let mut mix = Vec::new();
    for (index, &note) in cue.notes.iter().enumerate() {
        let frequency = 440.0 * 2_f32.powf((f32::from(note) - 69.0) / 12.0);
        let gain = cue.gain * 0.9_f32.powi(index as i32);
        let samples = soft_note(cue.voice, frequency, cue.glide, gain);
        let start = step * index;
        mix.resize(mix.len().max(start + samples.len()), 0.0);
        for (out, sample) in mix[start..].iter_mut().zip(samples) {
            *out += sample;
        }
    }
    mix
}

pub fn cue_samples(cue: Cue) -> Vec<f32> {
    let step = RATE * 9 / 100;
    match cue {
        // A5 then E6.
        Cue::Poke => tones(&[(880.0, RATE * 3 / 20), (1318.5, RATE / 5)], 0.28),
        Cue::MicOff => tones(&[(659.3, step), (440.0, step)], 0.2),
        Cue::MicOn => tones(&[(440.0, step), (659.3, step)], 0.2),
        Cue::SoundOff => tones(&[(392.0, step), (261.6, step)], 0.2),
        Cue::SoundOn => tones(&[(261.6, step), (392.0, step)], 0.2),
        Cue::Joined => tones(&[(523.3, step), (784.0, step)], 0.2),
        Cue::Moved => tones(&[(659.3, step), (523.3, step), (659.3, step)], 0.2),
        Cue::ViewerJoined => tones(&[(784.0, step), (1046.5, step)], 0.16),
        Cue::ViewerLeft => tones(&[(1046.5, step), (784.0, step)], 0.16),
        Cue::StreamOn => tones(&[(659.3, step), (784.0, step), (1046.5, step)], 0.16),
        Cue::StreamOff => tones(&[(1046.5, step), (784.0, step), (659.3, step)], 0.16),
    }
}

pub(crate) fn report_stream_error(error: cpal::Error, errors: &ArrayQueue<String>) {
    // CPAL continues the stream after an xrun; a dropped buffer is not a device failure.
    if error.kind() != cpal::ErrorKind::Xrun {
        let _ = errors.push(error.to_string());
    }
}

fn make_input<T>(
    device: &Device,
    config: &SupportedStreamConfig,
    capture: Arc<Capture>,
    errors: Arc<ArrayQueue<String>>,
    echo: bool,
) -> Result<Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = config.channels() as usize;
    let rate = config.sample_rate();
    let failure = capture.discontinuity.clone();
    Ok(device.build_input_stream(
        config.config(),
        move |data: &[T], info| {
            let _trace = profiler::scope(Category::AudioCallbacks, "audio.capture_callback");
            let generation = capture.generation.load(Ordering::Acquire);
            if generation & 1 == 0 {
                return;
            }
            let timestamp = info.timestamp();
            let at = if echo {
                callback_time(timestamp.capture, timestamp.callback, Instant::now())
            } else {
                None
            };
            let mut peak = 0.0_f32;
            for (index, frame) in data.chunks_exact(channels).enumerate() {
                let mono =
                    frame.iter().map(|&v| f32::from_sample(v)).sum::<f32>() / channels as f32;
                peak = peak.max(mono.abs());
                if capture
                    .samples
                    .push((
                        generation,
                        Timed {
                            value: mono,
                            at: sample_time(at, index, rate),
                        },
                    ))
                    .is_err()
                {
                    capture.discontinuity.store(true, Ordering::Release);
                    break;
                }
            }
            if capture.generation.load(Ordering::Acquire) == generation {
                capture.level.store(peak.to_bits(), Ordering::Relaxed);
            }
        },
        move |error| {
            if error.kind() == cpal::ErrorKind::Xrun {
                failure.store(true, Ordering::Release);
            }
            report_stream_error(error, &errors);
        },
        Some(Duration::from_secs(5)),
    )?)
}

/// Plays one sound on the output with no server connected, for hearing it in Settings. The
/// device opens on a thread of its own, so the window never waits, and closes once it's over.
pub fn play_alone(output: DeviceChoice, samples: Vec<f32>, volume: Arc<PlaybackVolume>) {
    let _ = std::thread::Builder::new()
        .name("sound-preview".into())
        .spawn(move || {
            if let Err(error) = play_alone_now(&output, &samples, volume) {
                tracing::warn!(%error, "Sound preview failed");
            }
        });
}

fn play_alone_now(
    output: &DeviceChoice,
    samples: &[f32],
    volume: Arc<PlaybackVolume>,
) -> Result<()> {
    let output = stand_in(output, false).context("No playback device is connected")?;
    let device = find_device(&output, false)?;
    let config = config(&device, false).context("No usable playback configuration")?;
    let rate = config.sample_rate();
    // A sound is a fraction of a second, so straight-line resampling from 48 kHz is plenty.
    let step = RATE as f64 / f64::from(rate);
    let frames = (samples.len() as f64 / step) as usize;
    let queue = Arc::new(ArrayQueue::new(frames.max(1)));
    for frame in 0..frames {
        let at = frame as f64 * step;
        let index = at as usize;
        let Some(&a) = samples.get(index) else { break };
        let b = samples.get(index + 1).copied().unwrap_or(a);
        let value = a + (b - a) * (at - index as f64) as f32;
        let _ = queue.push([value; 2]);
    }
    let errors = Arc::new(ArrayQueue::new(4));
    let discontinuity = Arc::new(AtomicBool::new(false));
    macro_rules! output_stream {
        ($t:ty) => {
            make_output::<$t>(
                &device,
                &config,
                queue.clone(),
                errors.clone(),
                volume.clone(),
                None,
                discontinuity.clone(),
                false,
            )
        };
    }
    let stream = match config.sample_format() {
        SampleFormat::F32 => output_stream!(f32),
        SampleFormat::I16 => output_stream!(i16),
        SampleFormat::U16 => output_stream!(u16),
        SampleFormat::I32 => output_stream!(i32),
        SampleFormat::F64 => output_stream!(f64),
        format => bail!("Unsupported playback sample format: {format}"),
    }
    .context("Cannot open playback device")?;
    stream.play()?;
    // What the device still holds once the last sample has been handed to it.
    std::thread::sleep(
        Duration::from_secs_f64(frames as f64 / f64::from(rate)) + Duration::from_millis(150),
    );
    Ok(())
}

/// How often playback ran out of audio since the app started. Each time is a short dropout:
/// the worker filling the queue fell behind the device.
static PLAYBACK_GAPS: AtomicU64 = AtomicU64::new(0);

pub fn playback_gaps() -> u64 {
    PLAYBACK_GAPS.load(Ordering::Relaxed)
}

/// What became of incoming voice since the app started, across servers and counted in packets.
/// Loss on the network shows up here as packets filled in or dropped, not as playback gaps.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VoicePlayback {
    /// Decoded and played as received.
    pub played: u64,
    /// Missing when due and filled in by the decoder.
    pub concealed: u64,
    /// Missing when due and rebuilt from the packet after it.
    pub recovered: u64,
    /// Received but not played: too late, a duplicate, or the queue was full.
    pub dropped: u64,
    /// Talkers taken away because nothing arrived for too long.
    pub timed_out: u64,
    /// The most packets any talker had waiting after the last refill.
    pub queued: usize,
}

static VOICE: [AtomicU64; 5] = [const { AtomicU64::new(0) }; 5];
static VOICE_QUEUED: AtomicUsize = AtomicUsize::new(0);

pub fn voice_playback() -> VoicePlayback {
    let [played, concealed, recovered, dropped, timed_out] =
        VOICE.each_ref().map(|count| count.load(Ordering::Relaxed));
    VoicePlayback {
        played,
        concealed,
        recovered,
        dropped,
        timed_out,
        queued: VOICE_QUEUED.load(Ordering::Relaxed),
    }
}

/// Adds what the jitter buffers of every server did since the last refill.
pub fn count_voice(stats: tsclientlib::audio::VoiceStats, queued: usize) {
    let counts = [
        stats.played,
        stats.concealed,
        stats.recovered,
        stats.dropped,
        stats.timed_out,
    ];
    for (total, count) in VOICE.iter().zip(counts) {
        if count > 0 {
            total.fetch_add(count, Ordering::Relaxed);
        }
    }
    VOICE_QUEUED.store(queued, Ordering::Relaxed);
}

/// Keeps the output continuous when the queue runs dry. Silence cut into the middle of a wave
/// is a click, so the last sample dies away instead and the audio that follows fades in over it.
struct Flow {
    last: [f32; 2],
    tail: [f32; 2],
    fade: f32,
    fade_step: f32,
    decay: f32,
    dry: bool,
    /// Set by the first sample, so the wait for the worker to start is not counted.
    primed: bool,
    gaps: u32,
}

impl Flow {
    /// Both the fade in and the dying away take about this long.
    const SECONDS: f32 = 0.004;

    fn new(rate: u32) -> Self {
        let samples = rate as f32 * Self::SECONDS;
        Self {
            last: [0.0; 2],
            tail: [0.0; 2],
            fade: 1.0,
            fade_step: 1.0 / samples,
            // Down 60 dB over those samples.
            decay: 0.001_f32.powf(1.0 / samples),
            dry: false,
            primed: false,
            gaps: 0,
        }
    }

    fn next(&mut self, queue: &ArrayQueue<[f32; 2]>) -> [f32; 2] {
        let Some(sample) = queue.pop() else {
            if !self.dry {
                self.dry = true;
                self.tail = self.last;
                self.fade = 0.0;
                self.gaps += u32::from(self.primed);
            }
            self.fall();
            self.last = self.tail;
            return self.tail;
        };
        self.primed = true;
        self.dry = false;
        if self.fade < 1.0 || self.tail != [0.0; 2] {
            self.fade = (self.fade + self.fade_step).min(1.0);
            self.fall();
            self.last = [
                sample[0] * self.fade + self.tail[0],
                sample[1] * self.fade + self.tail[1],
            ];
        } else {
            self.last = sample;
        }
        self.last
    }

    fn fall(&mut self) {
        for value in &mut self.tail {
            *value *= self.decay;
            if value.abs() < 1e-5 {
                *value = 0.0;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn make_output<T>(
    device: &Device,
    config: &SupportedStreamConfig,
    queue: Arc<ArrayQueue<[f32; 2]>>,
    errors: Arc<ArrayQueue<String>>,
    volume: Arc<PlaybackVolume>,
    reference: Option<Arc<ArrayQueue<Timed<[f32; 2]>>>>,
    discontinuity: Arc<AtomicBool>,
    count_gaps: bool,
) -> Result<Stream>
where
    T: SizedSample + FromSample<f32>,
    f32: FromSample<T>,
{
    let channels = config.channels() as usize;
    let rate = config.sample_rate();
    let failure = discontinuity.clone();
    let mut gain = volume.gain();
    let mut flow = Flow::new(rate);
    // Smooth live volume changes over about 5 ms to avoid abrupt sample jumps.
    let gain_step = 1.0 / (config.sample_rate() as f32 * 0.005);
    Ok(device.build_output_stream(
        config.config(),
        move |data: &mut [T], info| {
            let _trace = profiler::scope(Category::AudioCallbacks, "audio.playback_callback");
            let observed = reference.as_ref().map(|_| Instant::now());
            let gaps = flow.gaps;
            write_playback(
                data,
                channels,
                &queue,
                &mut flow,
                &mut gain,
                volume.gain(),
                gain_step,
            );
            if count_gaps && flow.gaps != gaps {
                PLAYBACK_GAPS.fetch_add(u64::from(flow.gaps - gaps), Ordering::Relaxed);
            }
            if let Some(reference) = &reference {
                let timestamp = info.timestamp();
                let at = callback_time(timestamp.playback, timestamp.callback, observed.unwrap());
                record_playback(data, channels, rate, at, reference, &discontinuity);
            }
        },
        move |error| {
            if error.kind() == cpal::ErrorKind::Xrun {
                failure.store(true, Ordering::Release);
            }
            report_stream_error(error, &errors);
        },
        Some(Duration::from_secs(5)),
    )?)
}

fn record_playback<T: SizedSample>(
    data: &[T],
    channels: usize,
    rate: u32,
    at: Option<Instant>,
    reference: &ArrayQueue<Timed<[f32; 2]>>,
    discontinuity: &AtomicBool,
) where
    f32: FromSample<T>,
{
    // Copy actual submitted samples, including gain, quantization, mono and underflow.
    for (index, frame) in data.chunks_exact(channels).enumerate() {
        let left = f32::from_sample(frame[0]);
        let right = if channels == 1 {
            left
        } else {
            f32::from_sample(frame[1])
        };
        if reference
            .push(Timed {
                value: [left, right],
                at: sample_time(at, index, rate),
            })
            .is_err()
        {
            discontinuity.store(true, Ordering::Release);
            break;
        }
    }
}

fn write_playback<T: SizedSample + FromSample<f32>>(
    data: &mut [T],
    channels: usize,
    queue: &ArrayQueue<[f32; 2]>,
    flow: &mut Flow,
    gain: &mut f32,
    target_gain: f32,
    gain_step: f32,
) {
    fn write<T: SizedSample + FromSample<f32>>(
        data: &mut [T],
        channels: usize,
        mut next: impl FnMut() -> [f32; 2],
        gain: &mut f32,
        target: f32,
        step: f32,
        mut frame_out: impl FnMut(&mut [T], [f32; 2], f32),
    ) {
        let mut frames = data.chunks_exact_mut(channels);
        while *gain != target {
            let Some(frame) = frames.next() else { return };
            *gain += (target - *gain).clamp(-step, step);
            frame_out(frame, next(), *gain);
        }
        for frame in frames {
            frame_out(frame, next(), *gain);
        }
    }
    match channels {
        1 => write(
            data,
            channels,
            || flow.next(queue),
            gain,
            target_gain,
            gain_step,
            |frame, [left, right], gain| {
                frame[0] = T::from_sample((left + right) * 0.5 * gain);
            },
        ),
        2 => write(
            data,
            channels,
            || flow.next(queue),
            gain,
            target_gain,
            gain_step,
            |frame, [left, right], gain| {
                frame[0] = T::from_sample(left * gain);
                frame[1] = T::from_sample(right * gain);
            },
        ),
        _ => write(
            data,
            channels,
            || flow.next(queue),
            gain,
            target_gain,
            gain_step,
            |frame, [left, right], gain| {
                frame[0] = T::from_sample(left * gain);
                frame[1] = T::from_sample(right * gain);
                frame[2..].fill(T::EQUILIBRIUM);
            },
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use audiopus::coder::Decoder;
    use tsclientlib::{ClientId, audio::AudioHandler};
    use tsproto_packets::packets::{AudioData, InAudioBuf};

    #[test]
    fn playback_matches_the_original_ramp_and_channel_conversion() {
        for channels in [1, 2, 6] {
            for (initial, target) in [(0.0_f32, 1.0_f32), (1.0, 0.0), (0.37, 0.37)] {
                let queue = ArrayQueue::new(700);
                let mut expected = vec![0_i16; 800 * channels];
                let mut old_gain = initial;
                // Past the end of the queue the last sample dies away.
                let decay = Flow::new(48_000).decay;
                let mut last = [0.0_f32; 2];
                for (index, frame) in expected.chunks_exact_mut(channels).enumerate() {
                    old_gain += (target - old_gain).clamp(-1.0_f32 / 240.0, 1.0 / 240.0);
                    if index < 700 {
                        last = [index as f32 / 1000.0, -(index as f32) / 2000.0];
                        queue.push(last).unwrap();
                    } else {
                        last = last.map(|value| {
                            let value = value * decay;
                            if value.abs() < 1e-5 { 0.0 } else { value }
                        });
                    }
                    let [left, right] = last;
                    frame[0] = i16::from_sample(if channels == 1 {
                        (left + right) * 0.5 * old_gain
                    } else {
                        left * old_gain
                    });
                    if channels > 1 {
                        frame[1] = i16::from_sample(right * old_gain);
                    }
                }
                let mut actual = vec![123_i16; expected.len()];
                let mut gain = initial;
                let mut flow = Flow::new(48_000);
                for chunk in actual.chunks_mut(channels * 137) {
                    write_playback(
                        chunk,
                        channels,
                        &queue,
                        &mut flow,
                        &mut gain,
                        target,
                        1.0 / 240.0,
                    );
                }
                assert_eq!(actual, expected);
                assert_eq!(gain, old_gain);
                assert!(queue.is_empty());
            }
        }
    }

    #[test]
    fn the_soft_limit_leaves_normal_levels_alone_and_keeps_peaks_inside_full_scale() {
        for sample in [0.0, 0.3, -0.8, 0.8] {
            assert_eq!(soft_limit(sample), sample);
        }
        let mut last = 0.8;
        for tenths in 9..40 {
            let limited = soft_limit(tenths as f32 / 10.0);
            assert!(limited >= last && limited <= 1.0, "{tenths}: {limited}");
            assert_eq!(soft_limit(-(tenths as f32) / 10.0), -limited);
            last = limited;
        }
    }

    #[test]
    fn mute_and_deafen_cues_are_short_quiet_and_all_different() {
        let cues = [
            Cue::MicOff,
            Cue::MicOn,
            Cue::SoundOff,
            Cue::SoundOn,
            Cue::Joined,
            Cue::Moved,
            Cue::ViewerJoined,
            Cue::ViewerLeft,
            Cue::StreamOn,
            Cue::StreamOff,
        ]
        .map(cue_samples);
        for cue in &cues {
            assert!(cue.len() < RATE * 3 / 10, "brief enough to repeat quickly");
            assert!(cue.iter().all(|sample| sample.abs() <= 0.2));
            assert!(cue.iter().any(|sample| sample.abs() > 0.08));
        }
        for (index, cue) in cues.iter().enumerate() {
            assert!(cues[index + 1..].iter().all(|other| other != cue));
        }
    }

    #[test]
    fn the_poke_chime_is_short_quiet_and_fades_out() {
        let chime = cue_samples(Cue::Poke);
        assert_eq!(chime.len(), RATE * 7 / 20);
        assert!(chime.iter().all(|sample| sample.abs() <= 0.28));
        assert!(chime.iter().any(|sample| sample.abs() > 0.1));
        assert!(
            chime[chime.len() - 10..]
                .iter()
                .all(|sample| sample.abs() < 0.06)
        );
    }

    fn chime(id: u16, level: f32, length: usize) -> Chime {
        Chime {
            id,
            urgent: false,
            samples: vec![level; length],
        }
    }

    fn heard(chimes: &mut Chimes) -> Vec<f32> {
        std::iter::from_fn(|| chimes.next()).collect()
    }

    #[test]
    fn sounds_wait_their_turn_instead_of_cutting_each_other_off() {
        let mut chimes = Chimes::default();
        chimes.add(chime(1, 0.1, 3));
        chimes.add(chime(2, 0.2, 2));
        assert_eq!(chimes.next(), Some(0.1), "the first is already playing");
        chimes.add(chime(3, 0.3, 1));
        assert_eq!(heard(&mut chimes), [0.1, 0.1, 0.2, 0.2, 0.3]);
        assert!(!chimes.sounding());
        chimes.add(chime(1, 0.1, 0));
        assert!(!chimes.sounding(), "nothing to play");
    }

    #[test]
    fn a_burst_of_sounds_is_cut_down_to_its_first_few() {
        let mut chimes = Chimes::default();
        for id in 1..=10 {
            chimes.add(chime(id, f32::from(id), 2));
        }
        assert_eq!(heard(&mut chimes), [1.0, 1.0, 2.0, 2.0, 3.0, 3.0]);

        // Five people arriving at once are one sound, and so are two a moment apart.
        for _ in 0..5 {
            chimes.add(chime(7, 0.7, RATE * 2));
        }
        assert!(chimes.waiting.is_empty());
        for _ in 0..RATE / 2 {
            chimes.next();
        }
        chimes.add(chime(7, 0.7, RATE * 2));
        assert!(chimes.waiting.is_empty(), "still the same news");
        // Later than that it is someone else, heard once more however many follow.
        for _ in 0..RATE {
            chimes.next();
        }
        chimes.add(chime(7, 0.7, RATE * 2));
        chimes.add(chime(7, 0.7, RATE * 2));
        assert_eq!(chimes.waiting.len(), 1);

        // Nothing lines up to start seconds after the event it is about.
        let mut chimes = Chimes::default();
        chimes.add(chime(1, 0.1, Chimes::LATE + 1));
        chimes.add(chime(2, 0.2, 2));
        assert!(chimes.waiting.is_empty(), "would start too late");
        for _ in 0..2 {
            chimes.next();
        }
        chimes.add(chime(2, 0.2, 2));
        assert_eq!(chimes.waiting.len(), 1);
    }

    #[test]
    fn what_you_just_did_is_heard_at_once_and_never_lines_up() {
        // Like a real sound, it starts from silence.
        let urgent = |id, level: f32| Chime {
            id,
            urgent: true,
            samples: [vec![0.0; Chimes::FADE], vec![level; Chimes::FADE]].concat(),
        };
        let mut chimes = Chimes::default();
        chimes.add(chime(1, 0.5, RATE));
        chimes.add(chime(2, 0.2, 2));
        chimes.next();
        // Muting and unmuting over and over: only the last one is left to play.
        for _ in 0..6 {
            chimes.add(urgent(8, 0.1));
            chimes.add(urgent(9, 0.3));
        }
        let played = heard(&mut chimes);
        assert_eq!(played.len(), Chimes::FADE * 2 + 2, "then what was waiting");
        assert_eq!(played[0], 0.5, "carries on from where the old sound was");
        assert!(
            played[..Chimes::FADE]
                .windows(2)
                .all(|pair| pair[1] < pair[0])
        );
        assert!(
            played[Chimes::FADE - 1] < 0.01,
            "and fades out under the new one"
        );
        assert_eq!(played[Chimes::FADE..Chimes::FADE * 2], [0.3; Chimes::FADE]);
        assert_eq!(played[Chimes::FADE * 2..], [0.2, 0.2]);
    }

    #[test]
    fn capture_generations_reject_inflight_audio_after_mute_and_resume() {
        let capture = Capture {
            samples: ArrayQueue::new(8),
            generation: AtomicU64::new(0),
            level: AtomicU32::new(0),
            discontinuity: Arc::new(AtomicBool::new(false)),
        };
        capture.clear(true);
        let before_mute = capture.generation.load(Ordering::Acquire);
        capture.clear(false);
        capture
            .samples
            .push((
                before_mute,
                Timed {
                    value: 0.9,
                    at: None,
                },
            ))
            .ok()
            .unwrap();
        assert!(capture.pop().is_none());
        capture.clear(true);
        capture
            .samples
            .push((
                before_mute,
                Timed {
                    value: 0.8,
                    at: None,
                },
            ))
            .ok()
            .unwrap();
        let current = capture.generation.load(Ordering::Acquire);
        capture
            .samples
            .push((
                current,
                Timed {
                    value: 0.2,
                    at: None,
                },
            ))
            .ok()
            .unwrap();
        assert_eq!(capture.pop().unwrap().value, 0.2);
        assert!(capture.pop().is_none());
    }

    #[test]
    fn echo_reference_matches_quantized_output_after_volume_layout_and_underflow() {
        let now = Instant::now();
        for channels in [1, 2, 6] {
            let playback = ArrayQueue::new(3);
            playback.push([0.8, -0.2]).unwrap();
            playback.push([0.4, 0.6]).unwrap();
            let mut submitted = vec![0_i16; channels * 3];
            write_playback(
                &mut submitted,
                channels,
                &playback,
                &mut Flow::new(48_000),
                &mut 0.0,
                0.5,
                0.25,
            );
            let reference = ArrayQueue::new(3);
            let dropped = AtomicBool::new(false);
            record_playback(&submitted, channels, 48000, Some(now), &reference, &dropped);
            for (index, actual) in submitted.chunks_exact(channels).enumerate() {
                let sample = reference.pop().unwrap();
                let left = f32::from_sample(actual[0]);
                let right = if channels == 1 {
                    left
                } else {
                    f32::from_sample(actual[1])
                };
                assert_eq!(sample.value, [left, right]);
                assert_eq!(sample.at, sample_time(Some(now), index, 48000));
            }
            assert!(!dropped.load(Ordering::Relaxed));
            let small = ArrayQueue::new(1);
            record_playback(&submitted, channels, 48000, Some(now), &small, &dropped);
            assert!(
                dropped.load(Ordering::Relaxed),
                "Overflow must request a DSP reset"
            );
        }
    }

    #[test]
    fn resampled_timestamps_include_filter_delay_and_variable_frame_counts() -> Result<()> {
        let start = Instant::now();
        for rate in [44100, 48000, 96000] {
            let mut converter = RateConverter::new(rate, 48000, 1, rate as usize / 100)?;
            let mut clock = ResampleClock::default();
            let mut produced = 0;
            for block in 0..100 {
                let delay = converter.output_delay();
                let (_, count) = converter.process()?;
                let first = start + Duration::from_millis(block * 10);
                let at = clock
                    .advance(Some(first), rate as usize / 100, count, rate, delay)
                    .unwrap();
                let expected = sample_time(Some(start), produced, 48000).unwrap()
                    - Duration::from_secs_f64(delay as f64 / 48000.0);
                assert!(
                    at.max(expected).duration_since(at.min(expected)) <= Duration::from_nanos(1)
                );
                produced += count;
            }
        }
        Ok(())
    }

    #[test]
    fn buffer_glitches_do_not_end_audio_or_crowd_out_device_failures() {
        let errors = ArrayQueue::new(4);
        for _ in 0..100 {
            report_stream_error(cpal::ErrorKind::Xrun.into(), &errors);
        }
        assert!(
            errors.is_empty(),
            "Buffer glitches must not fail the next audio tick"
        );

        for kind in [
            cpal::ErrorKind::DeviceNotAvailable,
            cpal::ErrorKind::StreamInvalidated,
            cpal::ErrorKind::PermissionDenied,
            cpal::ErrorKind::BackendError,
        ] {
            let error = cpal::Error::new(kind);
            report_stream_error(error.clone(), &errors);
            report_stream_error(cpal::ErrorKind::Xrun.into(), &errors);
            assert_eq!(errors.pop(), Some(error.to_string()));
            assert!(errors.is_empty());
        }
    }

    #[test]
    fn playback_volume_scales_channels_and_zero_volume_drains_to_silence() {
        let volume = PlaybackVolume::default();
        assert_eq!(volume.percent(), 100);
        volume.set_percent(200);
        assert_eq!(volume.percent(), 100);
        for (percent, expected) in [(100, [0.8, -0.4]), (50, [0.4, -0.2]), (0, [0.0, 0.0])] {
            volume.set_percent(percent);
            let queue = ArrayQueue::new(2);
            queue.push([0.8, -0.4]).unwrap();
            let mut output = [1.0_f32; 2];
            let mut gain = volume.gain();
            write_playback(
                &mut output,
                2,
                &queue,
                &mut Flow::new(48_000),
                &mut gain,
                volume.gain(),
                0.01,
            );
            assert_eq!(output, expected);
            assert!(queue.is_empty());
        }
        let queue = ArrayQueue::new(1);
        queue.push([0.8, -0.4]).unwrap();
        let mut mono = [0.0_f32];
        write_playback(
            &mut mono,
            1,
            &queue,
            &mut Flow::new(48_000),
            &mut 0.5,
            0.5,
            0.01,
        );
        assert!((mono[0] - 0.1).abs() < 1e-6);
    }

    #[test]
    fn a_dry_queue_dies_away_and_the_next_audio_fades_in() {
        let queue = ArrayQueue::new(2048);
        let mut flow = Flow::new(48_000);
        let mut output = [1.0_f32; 2 * 900];
        // Nothing has played yet: waiting for the first sample is not a gap.
        write_playback(&mut output, 2, &queue, &mut flow, &mut 1.0, 1.0, 0.01);
        assert_eq!(flow.gaps, 0);
        assert!(output.iter().all(|sample| *sample == 0.0));
        // A steady 0.8 for 300 frames, nothing for the next 300, then 0.8 again.
        for (part, gaps) in output.chunks_mut(2 * 300).zip([0, 1, 1]) {
            if gaps != 1 || flow.gaps == 1 {
                for _ in 0..300 {
                    queue.push([0.8, -0.8]).unwrap();
                }
            }
            write_playback(part, 2, &queue, &mut flow, &mut 1.0, 1.0, 0.01);
            assert_eq!(flow.gaps, gaps);
        }
        let left: Vec<f32> = output.iter().step_by(2).copied().collect();
        assert_eq!(left[299], 0.8);
        // No step anywhere: an abrupt cut would jump by 0.8.
        let step = left
            .windows(2)
            .map(|pair| (pair[1] - pair[0]).abs())
            .fold(0.0, f32::max);
        assert!(step < 0.03, "largest step {step}");
        assert!(left[599].abs() < 1e-3);
        assert_eq!(left[899], 0.8);
        assert!(
            output
                .as_chunks::<2>()
                .0
                .iter()
                .all(|frame| frame[0] == -frame[1])
        );
    }

    #[test]
    fn live_volume_changes_ramp_without_overshoot() {
        let queue = ArrayQueue::new(241);
        for _ in 0..241 {
            queue.push([1.0, 1.0]).unwrap();
        }
        let mut output = [0.0_f32; 482];
        let mut gain = 1.0;
        write_playback(
            &mut output,
            2,
            &queue,
            &mut Flow::new(48_000),
            &mut gain,
            0.0,
            1.0 / 240.0,
        );
        assert!(output[0] > 0.99 && output[0] < 1.0);
        assert_eq!(gain, 0.0);
        assert!(
            output
                .as_chunks::<2>()
                .0
                .iter()
                .all(|frame| frame[0] == frame[1] && (0.0..=1.0).contains(&frame[0]))
        );
        assert!(output.windows(2).all(|pair| pair[1] <= pair[0]));
        assert!(queue.is_empty());
    }

    #[test]
    fn teamspeak_voice_packets_decode_through_the_playback_queue() -> Result<()> {
        use tsproto_packets::packets::{Direction, OutAudio};
        let encoder = Encoder::new(SampleRate::Hz48000, Channels::Mono, Application::Voip)?;
        let mut handler = AudioHandler::new();
        for id in 0..12_u16 {
            let input: Vec<f32> = (0..FRAME)
                .map(|i| {
                    ((id as usize * FRAME + i) as f32 * 440.0 * std::f32::consts::TAU / RATE as f32)
                        .sin()
                        * 0.25
                })
                .collect();
            let mut encoded = [0; 1275];
            let len = encoder.encode_float(&input, &mut encoded)?;
            let packet = OutAudio::new(&AudioData::S2C {
                id,
                from: 42,
                codec: CodecType::OpusVoice,
                data: &encoded[..len],
            });
            handler.handle_packet(
                ClientId(42),
                InAudioBuf::try_new(Direction::S2C, packet.into_vec())?,
            )?;
        }
        let mut energy = 0.0;
        for _ in 0..16 {
            let mut output = [0.0; FRAME * 2];
            handler.fill_buffer(&mut output);
            assert!(output.iter().all(|sample| sample.is_finite()));
            energy += output.iter().map(|sample| sample * sample).sum::<f32>();
        }
        assert!(
            energy > 1.0,
            "Incoming voice packets must produce audible stereo PCM"
        );
        Ok(())
    }

    #[test]
    fn opus_roundtrip_preserves_audible_signal() -> Result<()> {
        let encoder = Encoder::new(SampleRate::Hz48000, Channels::Mono, Application::Voip)?;
        let mut decoder = Decoder::new(SampleRate::Hz48000, Channels::Mono)?;
        let mut energy = 0.0;
        for block in 0..10 {
            let input: Vec<f32> = (0..FRAME)
                .map(|i| {
                    ((block * FRAME + i) as f32 * 440.0 * std::f32::consts::TAU / RATE as f32).sin()
                        * 0.25
                })
                .collect();
            let mut encoded = [0; 1275];
            let len = encoder.encode_float(&input, &mut encoded)?;
            let mut output = [0.0; FRAME];
            let frames = decoder.decode_float(
                Some((&encoded[..len]).try_into()?),
                (&mut output[..]).try_into()?,
                false,
            )?;
            assert_eq!(frames, FRAME);
            energy += output.iter().map(|v| v * v).sum::<f32>();
        }
        assert!(energy / (FRAME * 10) as f32 > 0.005);
        Ok(())
    }

    #[test]
    fn converts_common_device_rates_without_drift() -> Result<()> {
        for (from, to) in [
            (44100, 48000),
            (48000, 44100),
            (16000, 48000),
            (96000, 48000),
        ] {
            let mut converter = RateConverter::new(from, to, 1, from as usize / 100)?;
            let mut written = 0;
            for _ in 0..100 {
                converter.input[0].fill(0.1);
                let (samples, count) = converter.process()?;
                assert!(samples[0][..count].iter().all(|s| s.is_finite()));
                written += count;
            }
            assert!(
                written.abs_diff(to as usize) < 500,
                "{from} -> {to}: {written}"
            );
        }
        Ok(())
    }
}
