use std::{
    collections::VecDeque,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering},
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
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
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
    resampler: Option<SincFixedIn<f32>>,
    pub input: Vec<Vec<f32>>,
    output: Vec<Vec<f32>>,
}

impl RateConverter {
    pub fn new(from: u32, to: u32, channels: usize, frames: usize) -> Result<Self> {
        let resampler = if from == to {
            None
        } else {
            Some(SincFixedIn::new(
                to as f64 / from as f64,
                1.0,
                SincInterpolationParameters {
                    sinc_len: 128,
                    f_cutoff: 0.95,
                    interpolation: SincInterpolationType::Linear,
                    oversampling_factor: 128,
                    window: WindowFunction::BlackmanHarris2,
                },
                frames,
                channels,
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
            let (_, written) =
                resampler.process_into_buffer(&self.input, &mut self.output, None)?;
            Ok((&self.output, written))
        } else {
            Ok((&self.input, self.input[0].len()))
        }
    }

    fn reset(&mut self) {
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
    /// A notification sound still to play, mixed over voice even while deafened.
    chime: VecDeque<f32>,
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
            chime: VecDeque::new(),
            encoder,
            codec: CodecType::OpusVoice,
            output_target,
            description,
        })
    }

    pub fn set_capture(&mut self, enabled: bool) -> Result<()> {
        if self.capture.enabled() != enabled {
            self.reset_capture(enabled)?;
        }
        Ok(())
    }

    fn reset_capture(&mut self, enabled: bool) -> Result<()> {
        self.packet_boundary.end();
        self.capture.clear(enabled);
        self.pcm.clear();
        self.monitor.clear();
        self.input_rate.reset();
        self.input_clock = ResampleClock::default();
        self.input_len = 0;
        self.input_at = None;
        self.processor.reset()?;
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
        self.reset_capture(false)?;
        let mut packets = Vec::new();
        self.packet_boundary.flush(&mut packets);
        Ok(packets)
    }

    /// Plays a short cue over whatever is playing.
    pub fn cue(&mut self, cue: Cue) {
        self.chime = cue_samples(cue).into();
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
            bail!("Audio device failed: {error}. Refresh devices and reconnect.");
        }
        if self.capture.discontinuity.swap(false, Ordering::AcqRel) {
            self.reset_capture(self.capture.enabled())?;
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
                    let Some(sample) = self.chime.pop_front() else {
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

/// A short sound the app plays itself. Rising means on and falling means off; the microphone's
/// notes sit higher than the headphones'.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    Poke,
    MicOff,
    MicOn,
    SoundOff,
    SoundOn,
}

impl Cue {
    pub const ALL: [Self; 5] = [
        Self::Poke,
        Self::MicOff,
        Self::MicOn,
        Self::SoundOff,
        Self::SoundOn,
    ];
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

fn cue_samples(cue: Cue) -> Vec<f32> {
    let step = RATE * 9 / 100;
    match cue {
        // A5 then E6.
        Cue::Poke => tones(&[(880.0, RATE * 3 / 20), (1318.5, RATE / 5)], 0.28),
        Cue::MicOff => tones(&[(659.3, step), (440.0, step)], 0.2),
        Cue::MicOn => tones(&[(440.0, step), (659.3, step)], 0.2),
        Cue::SoundOff => tones(&[(392.0, step), (261.6, step)], 0.2),
        Cue::SoundOn => tones(&[(261.6, step), (392.0, step)], 0.2),
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

fn make_output<T>(
    device: &Device,
    config: &SupportedStreamConfig,
    queue: Arc<ArrayQueue<[f32; 2]>>,
    errors: Arc<ArrayQueue<String>>,
    volume: Arc<PlaybackVolume>,
    reference: Option<Arc<ArrayQueue<Timed<[f32; 2]>>>>,
    discontinuity: Arc<AtomicBool>,
) -> Result<Stream>
where
    T: SizedSample + FromSample<f32>,
    f32: FromSample<T>,
{
    let channels = config.channels() as usize;
    let rate = config.sample_rate();
    let failure = discontinuity.clone();
    let mut gain = volume.gain();
    // Smooth live volume changes over about 5 ms to avoid abrupt sample jumps.
    let gain_step = 1.0 / (config.sample_rate() as f32 * 0.005);
    Ok(device.build_output_stream(
        config.config(),
        move |data: &mut [T], info| {
            let _trace = profiler::scope(Category::AudioCallbacks, "audio.playback_callback");
            let observed = reference.as_ref().map(|_| Instant::now());
            write_playback(data, channels, &queue, &mut gain, volume.gain(), gain_step);
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
    gain: &mut f32,
    target_gain: f32,
    gain_step: f32,
) {
    for frame in data.chunks_exact_mut(channels) {
        *gain += (target_gain - *gain).clamp(-gain_step, gain_step);
        let [left, right] = queue.pop().unwrap_or([0.0; 2]);
        frame.fill(T::EQUILIBRIUM);
        frame[0] = T::from_sample(if channels == 1 {
            (left + right) * 0.5 * *gain
        } else {
            left * *gain
        });
        if channels > 1 {
            frame[1] = T::from_sample(right * *gain);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use audiopus::coder::Decoder;
    use tsclientlib::{ClientId, audio::AudioHandler};
    use tsproto_packets::packets::{AudioData, InAudioBuf};

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
        let cues = [Cue::MicOff, Cue::MicOn, Cue::SoundOff, Cue::SoundOn].map(cue_samples);
        for cue in &cues {
            assert!(cue.len() < RATE / 4, "brief enough to repeat quickly");
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
            write_playback(&mut submitted, channels, &playback, &mut 0.0, 0.5, 0.25);
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
                if index == 2 {
                    assert_eq!(sample.value, [0.0; 2]);
                }
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
            write_playback(&mut output, 2, &queue, &mut gain, volume.gain(), 0.01);
            assert_eq!(output, expected);
            assert!(queue.is_empty());
        }
        let queue = ArrayQueue::new(1);
        queue.push([0.8, -0.4]).unwrap();
        let mut mono = [0.0_f32];
        write_playback(&mut mono, 1, &queue, &mut 0.5, 0.5, 0.01);
        assert!((mono[0] - 0.1).abs() < 1e-6);
    }

    #[test]
    fn live_volume_changes_ramp_without_overshoot() {
        let queue = ArrayQueue::new(241);
        for _ in 0..241 {
            queue.push([1.0, 1.0]).unwrap();
        }
        let mut output = [0.0_f32; 482];
        let mut gain = 1.0;
        write_playback(&mut output, 2, &queue, &mut gain, 0.0, 1.0 / 240.0);
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
