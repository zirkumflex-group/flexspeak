//! The sound of a card being opened: one buffer per rarity, synthesised here, so no audio file
//! ships. It has its own output stream because the voice engine's only exists on a server, and a
//! can gets opened whether or not you're connected.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};

use crate::audio::{DeviceChoice, RATE, Voice, soft_note};
use crate::monster::Rarity;

/// The loudest the mix may get, well under a voice call's level.
const PEAK: f32 = 0.28;
/// A skipped reveal's sound fades this fast instead of clicking off.
const FADE_SECONDS: f32 = 0.03;

fn frequency(midi: u8) -> f32 {
    440.0 * 2_f32.powf((f32::from(midi) - 69.0) / 12.0)
}

/// One buffer the score is laid down on.
#[derive(Default)]
struct Mix(Vec<f32>);

impl Mix {
    fn lay(&mut self, at_ms: usize, samples: &[f32]) {
        let start = at_ms * RATE / 1000;
        if self.0.len() < start + samples.len() {
            self.0.resize(start + samples.len(), 0.0);
        }
        for (out, sample) in self.0[start..].iter_mut().zip(samples) {
            *out += sample;
        }
    }

    fn note(&mut self, at_ms: usize, voice: Voice, midi: u8, glide: f32, gain: f32) {
        self.lay(at_ms, &soft_note(voice, frequency(midi), glide, gain));
    }

    fn run(&mut self, at_ms: usize, voice: Voice, notes: &[u8], step_ms: usize, gain: f32) {
        for (index, midi) in notes.iter().enumerate() {
            self.note(at_ms + index * step_ms, voice, *midi, 0.0, gain);
        }
    }

    /// Rising filtered noise, ending in a quick fade.
    fn riser(&mut self, from_ms: usize, to_ms: usize, gain: f32) {
        let length = (to_ms - from_ms) * RATE / 1000;
        let mut random = 22_222_u32;
        let mut low = 0.0_f32;
        let samples: Vec<f32> = (0..length)
            .map(|i| {
                let progress = i as f32 / length as f32;
                let cutoff = 400.0 * (3000.0_f32 / 400.0).powf(progress);
                let coefficient = 1.0 - (-std::f32::consts::TAU * cutoff / RATE as f32).exp();
                random = random.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let white = (random >> 8) as f32 / 8_388_608.0 - 1.0;
                low += (white - low) * coefficient;
                let fade = ((length - i) as f32 / (RATE as f32 * 0.03)).min(1.0);
                low * gain * progress * fade
            })
            .collect();
        self.lay(from_ms, &samples);
    }

    /// A falling sine, the weight under the Legendary's turn.
    fn thud(&mut self, at_ms: usize, gain: f32) {
        let length = 180 * RATE / 1000;
        let mut phase = 0.0_f32;
        let samples: Vec<f32> = (0..length)
            .map(|i| {
                let progress = i as f32 / length as f32;
                phase += std::f32::consts::TAU * (55.0 - 15.0 * progress) / RATE as f32;
                phase.sin() * gain * (1.0 - progress)
            })
            .collect();
        self.lay(at_ms, &samples);
    }

    /// A can cracking open: the tab's snap, then gas hissing out.
    fn crack(&mut self, at_ms: usize, gain: f32) {
        let length = 340 * RATE / 1000;
        let mut random = 77_777_u32;
        let mut low = 0.0_f32;
        let samples: Vec<f32> = (0..length)
            .map(|i| {
                let t = i as f32 / RATE as f32;
                random = random.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let white = (random >> 8) as f32 / 8_388_608.0 - 1.0;
                // What's left above a low-pass is the hiss.
                low += (white - low) * 0.35;
                let hiss = (white - low) * (-t / 0.085).exp() * (t / 0.004).min(1.0);
                let snap = white * (-t / 0.004).exp() * 1.6;
                (hiss + snap) * gain
            })
            .collect();
        self.lay(at_ms, &samples);
    }

    fn finish(mut self) -> Vec<f32> {
        let loudest = self.0.iter().fold(0.0_f32, |max, s| max.max(s.abs()));
        if loudest > PEAK {
            let scale = PEAK / loudest;
            for sample in &mut self.0 {
                *sample *= scale;
            }
        }
        self.0
    }
}

/// The whole reveal's sound. The timings follow `monster::ladder`, and each rarity repeats the
/// one below it up to the point it's promoted.
pub fn render(rarity: Rarity) -> Vec<f32> {
    let mut mix = Mix::default();
    mix.note(0, Voice::Wood, 72, 0.0, 0.12);
    // The tab snaps 120 ms after the card turns, as the can on it levers open.
    mix.crack(
        match rarity {
            Rarity::Common => 280,
            Rarity::Uncommon => 640,
            Rarity::Rare => 1220,
            Rarity::Mythic => 2070,
            Rarity::Legendary => 3070,
        },
        0.09,
    );
    match rarity {
        Rarity::Common => mix.run(280, Voice::Felt, &[72, 76], 90, 0.09),
        Rarity::Uncommon => {
            mix.note(160, Voice::Glass, 79, 0.0, 0.07);
            mix.run(640, Voice::Felt, &[72, 76, 79], 80, 0.10);
        }
        Rarity::Rare => {
            mix.note(160, Voice::Glass, 79, 0.0, 0.07);
            mix.note(520, Voice::Glass, 84, 0.0, 0.08);
            mix.riser(520, 1220, 0.03);
            mix.run(1220, Voice::Bell, &[72, 79, 84], 75, 0.12);
            mix.note(1445, Voice::Glass, 88, 0.0, 0.10);
        }
        Rarity::Mythic => {
            mix.note(160, Voice::Glass, 79, 0.0, 0.07);
            mix.note(520, Voice::Glass, 84, 0.0, 0.08);
            mix.riser(520, 1800, 0.05);
            mix.note(1100, Voice::Bell, 88, 0.0, 0.10);
            mix.note(1100, Voice::Hum, 48, 0.0, 0.10);
            mix.run(2070, Voice::Bell, &[72, 76, 79, 84, 88], 70, 0.13);
            mix.note(2070, Voice::Hum, 48, 0.0, 0.12);
        }
        Rarity::Legendary => {
            mix.note(160, Voice::Glass, 79, 0.0, 0.07);
            mix.note(520, Voice::Glass, 84, 0.0, 0.08);
            mix.riser(520, 2700, 0.07);
            mix.note(1100, Voice::Bell, 88, 0.0, 0.10);
            mix.note(1100, Voice::Hum, 48, 0.0, 0.10);
            mix.note(1800, Voice::Bell, 91, 0.0, 0.10);
            mix.note(1800, Voice::Hum, 36, 12.0, 0.12);
            mix.thud(3100, 0.22);
            mix.riser(3100, 3220, 0.08);
            mix.run(3100, Voice::Bell, &[72, 79, 84, 88, 91, 96], 65, 0.14);
            mix.run(3900, Voice::Hum, &[60, 67], 200, 0.08);
        }
    }
    mix.finish()
}

/// Sound that's playing. Dropping this fades it out.
pub struct Playing {
    stop: Arc<AtomicBool>,
}

impl Drop for Playing {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

/// Starts a reveal's sound on the chosen output, on its own thread so the window never waits for
/// the audio device.
pub fn play(rarity: Rarity, output: Option<DeviceChoice>, volume_percent: u8) -> Playing {
    let stop = Arc::new(AtomicBool::new(false));
    // A test run shouldn't make noise.
    if !cfg!(test) {
        let flag = stop.clone();
        let spawned = std::thread::Builder::new()
            .name("card reveal sound".into())
            .spawn(move || {
                if let Err(error) = run(rarity, output, volume_percent, flag) {
                    tracing::warn!("can't play the card reveal: {error:#}");
                }
            });
        if let Err(error) = spawned {
            tracing::warn!("can't start the card reveal sound: {error}");
        }
    }
    Playing { stop }
}

fn run(
    rarity: Rarity,
    output: Option<DeviceChoice>,
    volume_percent: u8,
    stop: Arc<AtomicBool>,
) -> Result<()> {
    let device = match &output {
        Some(choice) => crate::audio::find_device(choice, false)?,
        None => cpal::default_host()
            .default_output_device()
            .context("No output device")?,
    };
    let config = device.default_output_config()?;
    let rate = config.sample_rate() as usize;
    let gain = f32::from(volume_percent.min(100)) / 100.0;
    let samples = Arc::new(resample(&render(rarity), rate, gain));
    let position = Arc::new(AtomicUsize::new(0));
    let channels = usize::from(config.channels());
    let stream_config = config.config();
    let stream = match config.sample_format() {
        SampleFormat::F32 => build::<f32>(
            &device,
            &stream_config,
            &samples,
            &position,
            &stop,
            channels,
            rate,
        )?,
        SampleFormat::I16 => build::<i16>(
            &device,
            &stream_config,
            &samples,
            &position,
            &stop,
            channels,
            rate,
        )?,
        SampleFormat::U16 => build::<u16>(
            &device,
            &stream_config,
            &samples,
            &position,
            &stop,
            channels,
            rate,
        )?,
        other => bail!("Unsupported output format {other}"),
    };
    stream.play()?;
    while position.load(Ordering::Relaxed) < samples.len() && !stop.load(Ordering::Acquire) {
        std::thread::sleep(Duration::from_millis(15));
    }
    if stop.load(Ordering::Acquire) {
        std::thread::sleep(Duration::from_secs_f32(FADE_SECONDS * 1.5));
    } else {
        // What's left in the device's buffer.
        std::thread::sleep(Duration::from_millis(60));
    }
    Ok(())
}

/// The mono score at the device's rate, at the app's playback volume.
fn resample(score: &[f32], rate: usize, gain: f32) -> Vec<f32> {
    if score.is_empty() {
        return Vec::new();
    }
    let length = score.len() * rate / RATE;
    (0..length)
        .map(|i| {
            let at = i as f32 * RATE as f32 / rate as f32;
            let index = at as usize;
            let next = score.get(index + 1).copied().unwrap_or(0.0);
            let here = score.get(index).copied().unwrap_or(0.0);
            (here + (next - here) * at.fract()) * gain
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    samples: &Arc<Vec<f32>>,
    position: &Arc<AtomicUsize>,
    stop: &Arc<AtomicBool>,
    channels: usize,
    rate: usize,
) -> Result<cpal::Stream>
where
    T: SizedSample + FromSample<f32> + Send + 'static,
{
    let (samples, position, stop) = (samples.clone(), position.clone(), stop.clone());
    let fade_step = 1.0 / (rate as f32 * FADE_SECONDS);
    let mut level = 1.0_f32;
    Ok(device.build_output_stream(
        *config,
        move |data: &mut [T], _| {
            let mut at = position.load(Ordering::Relaxed);
            let stopping = stop.load(Ordering::Acquire);
            for frame in data.chunks_mut(channels.max(1)) {
                if stopping {
                    level = (level - fade_step).max(0.0);
                }
                let sample = samples.get(at).copied().unwrap_or(0.0) * level;
                for out in frame {
                    *out = T::from_sample(sample);
                }
                at += 1;
            }
            position.store(at, Ordering::Relaxed);
        },
        |error| tracing::warn!("card reveal sound stopped: {error}"),
        Some(Duration::from_secs(5)),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monster::ladder;

    #[test]
    fn every_reveal_sound_stays_quiet_enough_and_has_something_in_it() {
        for rarity in Rarity::ALL {
            let samples = render(rarity);
            let loudest = samples.iter().fold(0.0_f32, |max, s| max.max(s.abs()));
            assert!(loudest > 0.01, "{rarity:?}");
            assert!(
                loudest <= PEAK + f32::EPSILON,
                "{rarity:?} peaks at {loudest}"
            );
        }
    }

    #[test]
    fn a_rarer_card_sounds_longer_and_no_sound_overstays_much() {
        for pair in Rarity::ALL.windows(2) {
            assert!(render(pair[0]).len() < render(pair[1]).len(), "{pair:?}");
        }
        for rarity in Rarity::ALL {
            let seconds = render(rarity).len() as f32 / RATE as f32;
            let settled: u32 = ladder(rarity).iter().map(|(_, ms)| ms).sum();
            assert!(
                seconds < settled as f32 / 1000.0 + 2.5,
                "{rarity:?} rings for {seconds} s"
            );
        }
    }

    #[test]
    fn the_score_keeps_its_length_at_a_devices_rate() {
        let score = vec![0.5; RATE];
        assert_eq!(resample(&score, 44_100, 1.0).len(), 44_100);
        let quiet = resample(&score, RATE, 0.5);
        assert!((quiet[100] - 0.25).abs() < 0.001, "playback volume applies");
    }
}
