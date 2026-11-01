use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};

pub const AVAILABLE: bool = cfg!(any(target_os = "macos", target_os = "windows"));
pub const SAMPLES: usize = 480;
const RATE: u32 = 48_000;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub noise_suppression: bool,
    pub automatic_gain: bool,
    pub echo_cancellation: bool,
}

impl Settings {
    pub fn enabled(self) -> bool {
        self.noise_suppression || self.automatic_gain || self.echo_cancellation
    }

    pub fn for_session(self, local_test: bool) -> Self {
        if !AVAILABLE {
            return Self::default();
        }
        Self {
            echo_cancellation: self.echo_cancellation && !local_test,
            ..self
        }
    }
}

#[derive(Clone, Copy)]
pub struct Timed<T> {
    pub value: T,
    pub at: Option<Instant>,
}

pub fn sample_time(at: Option<Instant>, offset: usize, rate: u32) -> Option<Instant> {
    at.and_then(|at| at.checked_add(Duration::from_secs_f64(offset as f64 / rate as f64)))
}

/// Map each stream independently: CPAL does not guarantee a shared clock origin.
pub fn callback_time(
    event: cpal::StreamInstant,
    callback: cpal::StreamInstant,
    now: Instant,
) -> Option<Instant> {
    if let Some(offset) = event.checked_duration_since(callback) {
        (offset <= Duration::from_millis(500))
            .then(|| now.checked_add(offset))
            .flatten()
    } else {
        let offset = callback.checked_duration_since(event)?;
        (offset <= Duration::from_millis(500))
            .then(|| now.checked_sub(offset))
            .flatten()
    }
}

fn pcm16(sample: f32) -> i16 {
    if sample.is_finite() {
        (sample.clamp(-1.0, 1.0) * 32768.0).round() as i16
    } else {
        0
    }
}

fn signed_ms(later: Instant, earlier: Instant) -> f64 {
    if later >= earlier {
        later.duration_since(earlier).as_secs_f64() * 1000.0
    } else {
        -earlier.duration_since(later).as_secs_f64() * 1000.0
    }
}

pub struct Processor {
    settings: Settings,
    capture: [f32; SAMPLES],
    capture_len: usize,
    captured_at: Option<Instant>,
    render: [i16; SAMPLES * 2],
    render_len: usize,
    rendered_at: Option<Instant>,
    reference: Option<(Instant, Instant)>,
    started: Instant,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    apm: Option<libwebrtc::native::apm::AudioProcessingModule>,
}

impl Processor {
    pub fn new(settings: Settings) -> Result<Self> {
        ensure!(
            AVAILABLE || !settings.enabled(),
            "Microphone processing is available on Mac and Windows"
        );
        Ok(Self {
            settings,
            capture: [0.0; SAMPLES],
            capture_len: 0,
            captured_at: None,
            render: [0; SAMPLES * 2],
            render_len: 0,
            rendered_at: None,
            reference: None,
            started: Instant::now(),
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            apm: settings.enabled().then(|| {
                libwebrtc::native::apm::AudioProcessingModule::new(
                    settings.echo_cancellation,
                    settings.automatic_gain,
                    true,
                    settings.noise_suppression,
                )
            }),
        })
    }

    pub fn reset(&mut self) {
        *self = Self::new(self.settings).expect("An existing processor has supported settings");
    }

    pub fn echo_enabled(&self) -> bool {
        self.settings.echo_cancellation
    }

    pub fn render(
        &mut self,
        channels: &[Vec<f32>],
        count: usize,
        at: Option<Instant>,
    ) -> Result<()> {
        if !self.echo_enabled() {
            return Ok(());
        }
        ensure!(
            at.is_some(),
            "Echo cancellation cannot read playback timing. Disable it or choose another playback device."
        );
        for (i, (&left, &right)) in channels[0].iter().zip(&channels[1]).take(count).enumerate() {
            if self.render_len == 0 {
                self.rendered_at = sample_time(at, i, RATE);
            }
            self.render[self.render_len * 2] = pcm16(left);
            self.render[self.render_len * 2 + 1] = pcm16(right);
            self.render_len += 1;
            if self.render_len == SAMPLES {
                let analyzed = Instant::now();
                #[cfg(any(target_os = "macos", target_os = "windows"))]
                self.apm
                    .as_mut()
                    .unwrap()
                    .process_reverse_stream(&mut self.render, RATE as i32, 2)
                    .map_err(|error| {
                        anyhow::anyhow!("Echo reference processing failed: {error}")
                    })?;
                self.reference = self.rendered_at.map(|played| (played, analyzed));
                self.render_len = 0;
            }
        }
        Ok(())
    }

    fn delay(&self, captured: Option<Instant>, now: Instant) -> Result<Option<i32>> {
        let captured = captured.ok_or_else(|| anyhow::anyhow!(
            "Echo cancellation cannot read microphone timing. Disable it or choose another microphone."
        ))?;
        ensure!(
            captured <= now && now.duration_since(captured) <= Duration::from_millis(500),
            "Echo microphone timing is out of range. Reconnect or disable echo cancellation."
        );
        if let Some((played, analyzed)) = self.reference {
            ensure!(
                now.saturating_duration_since(analyzed) <= Duration::from_millis(250),
                "Echo playback reference stopped. Reconnect or disable echo cancellation."
            );
            let delay = signed_ms(played, analyzed) + signed_ms(now, captured);
            if (0.0..=500.0).contains(&delay) {
                return Ok(Some(delay.round() as i32));
            }
        }
        // Discard startup capture until the first usable render history arrives.
        if now.saturating_duration_since(self.started) <= Duration::from_millis(500) {
            return Ok(None);
        }
        bail!("Echo timing is unavailable or out of range. Reconnect or disable echo cancellation.")
    }

    pub fn capture(
        &mut self,
        samples: &[f32],
        at: Option<Instant>,
        output: &mut VecDeque<f32>,
    ) -> Result<()> {
        if !self.settings.enabled() {
            output.extend(samples.iter().copied());
            return Ok(());
        }
        for (index, &sample) in samples.iter().enumerate() {
            if self.capture_len == 0 {
                self.captured_at = sample_time(at, index, RATE);
            }
            self.capture[self.capture_len] = sample;
            self.capture_len += 1;
            if self.capture_len == SAMPLES {
                self.capture_len = 0;
                let delay = if self.echo_enabled() {
                    let Some(delay) = self.delay(self.captured_at, Instant::now())? else {
                        continue;
                    };
                    Some(delay)
                } else {
                    None
                };
                let mut pcm = self.capture.map(pcm16);
                #[cfg(any(target_os = "macos", target_os = "windows"))]
                {
                    let apm = self.apm.as_mut().unwrap();
                    if let Some(delay) = delay {
                        apm.set_stream_delay_ms(delay)
                            .map_err(|error| anyhow::anyhow!("Echo timing failed: {error}"))?;
                    }
                    apm.process_stream(&mut pcm, RATE as i32, 1)
                        .map_err(|error| {
                            anyhow::anyhow!("Microphone processing failed: {error}")
                        })?;
                }
                #[cfg(not(any(target_os = "macos", target_os = "windows")))]
                let _ = (delay, &mut pcm);
                output.extend(pcm.map(|sample| f32::from(sample) / 32768.0));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_bypass_without_quantization_and_local_test_keeps_echo_off() -> Result<()> {
        let mut processor = Processor::new(Settings::default())?;
        let input = [0.000_001, -0.0, -0.321_987, 1.0];
        let mut output = VecDeque::new();
        processor.capture(&input[..1], None, &mut output)?;
        processor.capture(&input[1..], None, &mut output)?;
        assert_eq!(
            output.into_iter().map(f32::to_bits).collect::<Vec<_>>(),
            input.map(f32::to_bits)
        );
        let selected = Settings {
            noise_suppression: true,
            automatic_gain: true,
            echo_cancellation: true,
        };
        let local = selected.for_session(true);
        assert!(!local.echo_cancellation);
        assert_eq!(local.noise_suppression, AVAILABLE);
        assert_eq!(local.automatic_gain, AVAILABLE);
        assert_eq!(selected.for_session(false).echo_cancellation, AVAILABLE);
        assert_eq!(pcm16(-1.0), i16::MIN);
        assert_eq!(pcm16(1.0), i16::MAX);
        assert_eq!(pcm16(0.5), 16384);
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(pcm16(invalid), 0);
        }
        Ok(())
    }

    #[test]
    fn callback_timing_maps_independent_origins_and_rejects_implausible_latency() {
        let now = Instant::now();
        let stamp = |milliseconds| cpal::StreamInstant::from_millis(milliseconds);
        assert_eq!(
            callback_time(stamp(990), stamp(1000), now),
            Some(now - Duration::from_millis(10))
        );
        assert_eq!(
            callback_time(stamp(90_020), stamp(90_000), now),
            Some(now + Duration::from_millis(20))
        );
        assert!(callback_time(stamp(0), stamp(501), now).is_none());
        assert!(callback_time(stamp(1501), stamp(1000), now).is_none());
        assert_eq!(
            sample_time(Some(now), 441, 44_100),
            Some(now + Duration::from_millis(10))
        );
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn native_processing_handles_irregular_chunks_and_reset_discards_partial_speech() -> Result<()>
    {
        let settings = Settings {
            noise_suppression: true,
            automatic_gain: true,
            ..Settings::default()
        };
        let mut processor = Processor::new(settings)?;
        let mut whole = Processor::new(settings)?;
        let input: Vec<_> = (0..SAMPLES * 8)
            .map(|i| (i as f32 * 0.0576).sin() * 0.2)
            .collect();
        let mut expected = VecDeque::new();
        whole.capture(&input, None, &mut expected)?;
        let mut output = VecDeque::new();
        for chunk in input.chunks(137) {
            processor.capture(chunk, None, &mut output)?;
        }
        assert_eq!(output, expected);
        assert_eq!(output.len(), input.len());
        assert!(
            output
                .iter()
                .all(|sample| sample.is_finite() && sample.abs() <= 1.0)
        );
        assert!(output.iter().any(|sample| sample.abs() > 0.001));
        processor.capture(&[0.8; 123], None, &mut output)?;
        processor.reset();
        output.clear();
        processor.capture(&[0.0; SAMPLES - 1], None, &mut output)?;
        assert!(output.is_empty());
        processor.capture(&[0.0], None, &mut output)?;
        assert_eq!(output, VecDeque::from([0.0; SAMPLES]));
        Ok(())
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn echo_delay_accounts_for_both_devices_and_rejects_missing_or_stale_reference() -> Result<()> {
        let mut processor = Processor::new(Settings {
            echo_cancellation: true,
            ..Settings::default()
        })?;
        let now = Instant::now();
        assert_eq!(processor.delay(Some(now), now)?, None);
        assert!(processor.delay(None, now).is_err());
        processor.reference = Some((
            now + Duration::from_millis(20),
            now - Duration::from_millis(5),
        ));
        assert_eq!(
            processor.delay(Some(now - Duration::from_millis(15)), now)?,
            Some(40)
        );
        assert!(
            processor
                .delay(Some(now + Duration::from_millis(1)), now)
                .is_err()
        );
        assert!(
            processor
                .delay(Some(now - Duration::from_millis(501)), now)
                .is_err()
        );
        assert!(
            processor
                .delay(
                    Some(now - Duration::from_millis(15)),
                    now + Duration::from_millis(300)
                )
                .is_err()
        );
        processor.reset();
        processor.started = now - Duration::from_secs(1);
        let mut output = VecDeque::new();
        assert!(
            processor
                .capture(&[0.4; SAMPLES], Some(now), &mut output)
                .is_err()
        );
        assert!(
            output.is_empty(),
            "Missing reference must not leak unprocessed microphone audio"
        );
        Ok(())
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn native_echo_canceller_reduces_a_delayed_synthetic_echo() -> Result<()> {
        let mut processor = Processor::new(Settings {
            echo_cancellation: true,
            ..Settings::default()
        })?;
        let mut random = 12345_u32;
        let mut history = VecDeque::from(vec![vec![0.0; SAMPLES]; 6]);
        let mut input_energy = 0.0_f64;
        let mut output_energy = 0.0_f64;
        for frame in 0..400 {
            let render: Vec<f32> = (0..SAMPLES)
                .map(|_| {
                    random ^= random << 13;
                    random ^= random >> 17;
                    random ^= random << 5;
                    (random as f64 / u32::MAX as f64 * 0.4 - 0.2) as f32
                })
                .collect();
            let now = Instant::now();
            processor.render(
                &[render.clone(), render.clone()],
                SAMPLES,
                Some(now + Duration::from_millis(20)),
            )?;
            history.push_back(render);
            let echo: Vec<_> = history
                .pop_front()
                .unwrap()
                .iter()
                .map(|value| value * 0.6)
                .collect();
            let mut output = VecDeque::new();
            processor.capture(&echo, Some(now - Duration::from_millis(20)), &mut output)?;
            assert_eq!(output.len(), SAMPLES);
            if frame >= 300 {
                input_energy += echo.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>();
                output_energy += output.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>();
            }
        }
        assert!(
            output_energy < input_energy * 0.5,
            "Echo should be attenuated: input {input_energy}, output {output_energy}"
        );
        Ok(())
    }
}
