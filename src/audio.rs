use std::{
    collections::VecDeque,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering},
    },
    time::Duration,
};

use crate::profiler::{self, Category};
use anyhow::{Context, Result, bail};
use audiopus::{Application, Bitrate, Channels, SampleRate, coder::Encoder};
use cpal::{
    Device, FromSample, Sample, SampleFormat, SizedSample, Stream, SupportedStreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use crossbeam_queue::ArrayQueue;
use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use tsclientlib::{ClientId, audio::AudioHandler};
use tsproto_packets::packets::{AudioData, CodecType, InAudioBuf};

pub const RATE: usize = 48_000;
pub const FRAME: usize = 960;
const BLOCK: usize = 480;

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

    fn buffer_bytes(&self) -> usize {
        self.input
            .iter()
            .chain(&self.output)
            .map(|buffer| buffer.capacity() * std::mem::size_of::<f32>())
            .sum()
    }
}

pub struct AudioEngine {
    _input_stream: Stream,
    _output_stream: Stream,
    capture: Arc<ArrayQueue<f32>>,
    playback: Arc<ArrayQueue<[f32; 2]>>,
    errors: Arc<ArrayQueue<String>>,
    enabled: Arc<AtomicBool>,
    level: Arc<AtomicU32>,
    input_rate: RateConverter,
    output_rate: RateConverter,
    pcm: VecDeque<f32>,
    monitor: VecDeque<f32>,
    encoder: Encoder,
    incoming: AudioHandler,
    pub codec: CodecType,
    output_target: usize,
    pub description: String,
}

impl AudioEngine {
    pub fn open(
        input: &DeviceChoice,
        output: &DeviceChoice,
        volume: Arc<PlaybackVolume>,
    ) -> Result<Self> {
        let input_device = find_device(input, true)?;
        let output_device = find_device(output, false)?;
        let input_config =
            config(&input_device, true).context("No usable microphone configuration")?;
        let output_config =
            config(&output_device, false).context("No usable playback configuration")?;
        let capture = Arc::new(ArrayQueue::new(input_config.sample_rate() as usize / 4));
        let playback = Arc::new(ArrayQueue::new(output_config.sample_rate() as usize / 4));
        let errors = Arc::new(ArrayQueue::new(4));
        let enabled = Arc::new(AtomicBool::new(false));
        let level = Arc::new(AtomicU32::new(0.0_f32.to_bits()));

        macro_rules! input_stream {
            ($t:ty) => {
                make_input::<$t>(
                    &input_device,
                    &input_config,
                    capture.clone(),
                    errors.clone(),
                    enabled.clone(),
                    level.clone(),
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
        let mut encoder = Encoder::new(SampleRate::Hz48000, Channels::Mono, Application::Voip)?;
        encoder.set_bitrate(Bitrate::BitsPerSecond(32_768))?;
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
        Ok(Self {
            _input_stream: input_stream,
            _output_stream: output_stream,
            capture,
            playback,
            errors,
            enabled,
            level,
            input_rate,
            output_rate,
            pcm: VecDeque::new(),
            monitor: VecDeque::new(),
            encoder,
            incoming: AudioHandler::new(),
            codec: CodecType::OpusVoice,
            output_target: output_config.sample_rate() as usize / 50,
            description,
        })
    }

    pub fn set_capture(&mut self, enabled: bool) {
        if self.enabled.swap(enabled, Ordering::Relaxed) != enabled {
            while self.capture.pop().is_some() {}
            self.pcm.clear();
            self.monitor.clear();
            self.input_rate.reset();
            self.level.store(0.0_f32.to_bits(), Ordering::Relaxed);
        }
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

    pub fn receive(&mut self, packet: InAudioBuf) {
        let _trace = profiler::scope(Category::AudioProcessing, "audio.receive");
        let id = match packet.data().data() {
            AudioData::S2C { from, .. } | AudioData::S2CWhisper { from, .. } => ClientId(*from),
            _ => return,
        };
        if let Err(error) = self.incoming.handle_packet(id, packet) {
            tracing::debug!(%error, "Dropped incoming audio packet");
        }
    }

    pub fn clear_playback(&mut self) {
        self.incoming = AudioHandler::new();
        while self.playback.pop().is_some() {}
        self.output_rate.reset();
    }

    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }

    pub fn buffer_bytes(&self) -> usize {
        self.capture.capacity() * std::mem::size_of::<f32>()
            + self.playback.capacity() * std::mem::size_of::<[f32; 2]>()
            + (self.pcm.capacity() + self.monitor.capacity()) * std::mem::size_of::<f32>()
            + self.input_rate.buffer_bytes()
            + self.output_rate.buffer_bytes()
    }

    /// Codec work and resampling happen on the worker, never in an OS audio callback.
    pub fn tick(
        &mut self,
        loopback: bool,
        deafened: bool,
        stream_audio: Option<&crate::streaming::Surface>,
    ) -> Result<Vec<Vec<u8>>> {
        let _trace = profiler::scope(Category::AudioProcessing, "audio.process");
        if let Some(error) = self.errors.pop() {
            bail!("Audio device failed: {error}. Refresh devices and reconnect.");
        }
        let mut packets = Vec::new();
        while self.capture.len() >= self.input_rate.input[0].len() {
            for sample in &mut self.input_rate.input[0] {
                *sample = self.capture.pop().unwrap_or_default();
            }
            let (samples, count) = self.input_rate.process()?;
            if loopback {
                self.monitor.extend(samples[0][..count].iter().copied());
                while self.monitor.len() > RATE / 4 {
                    self.monitor.pop_front();
                }
            } else {
                self.pcm.extend(samples[0][..count].iter().copied());
            }
        }
        while self.pcm.len() >= FRAME {
            let mut frame = [0.0; FRAME];
            for sample in &mut frame {
                *sample = self.pcm.pop_front().unwrap_or_default();
            }
            let mut encoded = [0; 1275];
            let count = self.encoder.encode_float(&frame, &mut encoded)?;
            packets.push(encoded[..count].to_vec());
        }
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
            } else if !deafened {
                self.incoming.fill_buffer(&mut stereo);
                if let Some(surface) = stream_audio {
                    surface.mix_audio(&mut stereo);
                }
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

pub(crate) fn report_stream_error(error: cpal::Error, errors: &ArrayQueue<String>) {
    // CPAL continues the stream after an xrun; a dropped buffer is not a device failure.
    if error.kind() != cpal::ErrorKind::Xrun {
        let _ = errors.push(error.to_string());
    }
}

fn make_input<T>(
    device: &Device,
    config: &SupportedStreamConfig,
    queue: Arc<ArrayQueue<f32>>,
    errors: Arc<ArrayQueue<String>>,
    enabled: Arc<AtomicBool>,
    level: Arc<AtomicU32>,
) -> Result<Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = config.channels() as usize;
    Ok(device.build_input_stream(
        config.config(),
        move |data: &[T], _| {
            let _trace = profiler::scope(Category::AudioCallbacks, "audio.capture_callback");
            if !enabled.load(Ordering::Relaxed) {
                return;
            }
            let mut peak = 0.0_f32;
            for frame in data.chunks_exact(channels) {
                let mono =
                    frame.iter().map(|&v| f32::from_sample(v)).sum::<f32>() / channels as f32;
                peak = peak.max(mono.abs());
                if queue.push(mono).is_err() {
                    break;
                }
            }
            level.store(peak.to_bits(), Ordering::Relaxed);
        },
        move |error| report_stream_error(error, &errors),
        Some(Duration::from_secs(5)),
    )?)
}

fn make_output<T>(
    device: &Device,
    config: &SupportedStreamConfig,
    queue: Arc<ArrayQueue<[f32; 2]>>,
    errors: Arc<ArrayQueue<String>>,
    volume: Arc<PlaybackVolume>,
) -> Result<Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels() as usize;
    let mut gain = volume.gain();
    // Smooth live volume changes over about 5 ms to avoid abrupt sample jumps.
    let gain_step = 1.0 / (config.sample_rate() as f32 * 0.005);
    Ok(device.build_output_stream(
        config.config(),
        move |data: &mut [T], _| {
            let _trace = profiler::scope(Category::AudioCallbacks, "audio.playback_callback");
            write_playback(data, channels, &queue, &mut gain, volume.gain(), gain_step);
        },
        move |error| report_stream_error(error, &errors),
        Some(Duration::from_secs(5)),
    )?)
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
