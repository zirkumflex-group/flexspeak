//! How loud the people you hear are, summed up once a second for the log with debug logging on:
//! each person as they arrive, the volume they are played at, and everything you hear. It tells
//! whether someone who sounds quiet arrives quiet or is turned down on your end. Only what is
//! heard is measured, since the people talking may use another client.
//!
//! Your own microphone is measured too, while it is not muted: in three bands at three points
//! (as captured, after WebRTC's processing, as sent), with echo cancellation's own figures and
//! what it let through in each band (`echo_gain`). A voice that sounds muffled or weak then
//! shows which stage took it away.

use std::{
    collections::BTreeMap,
    fmt,
    time::{Duration, Instant},
};

/// How often the levels are written.
const EVERY: Duration = Duration::from_secs(1);

/// Whether levels are logged at all; nothing is measured otherwise.
pub fn on() -> bool {
    tracing::enabled!(target: "flexspeak::voice", tracing::Level::DEBUG)
}

/// Loudness of the samples seen since it was last read.
#[derive(Debug, Default)]
pub struct Meter {
    energy: f64,
    peak: f32,
    samples: usize,
}

impl Meter {
    pub fn add(&mut self, samples: &[f32]) {
        for &sample in samples.iter().filter(|sample| sample.is_finite()) {
            self.energy += f64::from(sample) * f64::from(sample);
            self.peak = self.peak.max(sample.abs());
            self.samples += 1;
        }
    }

    /// The level since the last read, or `None` without samples.
    fn take(&mut self) -> Option<Level> {
        let meter = std::mem::take(self);
        (meter.samples > 0).then(|| Level {
            rms: (meter.energy / meter.samples as f64).sqrt() as f32,
            peak: meter.peak,
        })
    }
}

#[derive(Debug, PartialEq)]
struct Level {
    rms: f32,
    peak: f32,
}

fn db(value: f32) -> f32 {
    20.0 * value.max(1e-6).log10()
}

impl fmt::Display for Level {
    /// Decibels below full scale, the average then the peak, as in "-24.3/-6.1".
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:.1}/{:.1}", db(self.rms), db(self.peak))
    }
}

/// A second-order section (RBJ cookbook, Q = 1/sqrt 2) at 48 kHz.
#[derive(Debug, Clone, Copy)]
struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    z: [f32; 2],
}

impl Biquad {
    fn new(high_pass: bool, frequency: f32) -> Self {
        let w = std::f32::consts::TAU * frequency / 48_000.0;
        let (sin, cos) = w.sin_cos();
        let alpha = sin * std::f32::consts::FRAC_1_SQRT_2;
        let a0 = 1.0 + alpha;
        let b = if high_pass {
            [(1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0]
        } else {
            [(1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0]
        };
        Self {
            b: b.map(|b| b / a0),
            a: [-2.0 * cos / a0, (1.0 - alpha) / a0],
            z: [0.0; 2],
        }
    }

    fn run(&mut self, x: f32) -> f32 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

/// Loudness in three bands: below 1 kHz (body), 1-4 kHz (clarity), above 4 kHz (air). A voice
/// that turns muffled loses the upper two against the first.
#[derive(Debug, Clone)]
pub struct BandMeter {
    filters: [Biquad; 8],
    energy: [f64; 3],
    samples: usize,
}

impl Default for BandMeter {
    fn default() -> Self {
        let low = |frequency| Biquad::new(false, frequency);
        let high = |frequency| Biquad::new(true, frequency);
        Self {
            filters: [
                low(1000.0),
                low(1000.0),
                high(1000.0),
                high(1000.0),
                low(4000.0),
                low(4000.0),
                high(4000.0),
                high(4000.0),
            ],
            energy: [0.0; 3],
            samples: 0,
        }
    }
}

impl BandMeter {
    pub fn add(&mut self, samples: &[f32]) {
        let [l0, l1, m0, m1, m2, m3, h0, h1] = &mut self.filters;
        for &x in samples.iter().filter(|sample| sample.is_finite()) {
            let low = l1.run(l0.run(x));
            let mid = m3.run(m2.run(m1.run(m0.run(x))));
            let high = h1.run(h0.run(x));
            for (energy, band) in self.energy.iter_mut().zip([low, mid, high]) {
                *energy += f64::from(band) * f64::from(band);
            }
            self.samples += 1;
        }
    }

    #[cfg(test)]
    pub fn samples(&self) -> usize {
        self.samples
    }

    /// Average level per band since the last read, or `None` without samples. The filters
    /// keep their state, so the next stretch continues seamlessly.
    fn take(&mut self) -> Option<Bands> {
        let samples = std::mem::take(&mut self.samples);
        let energy = std::mem::take(&mut self.energy);
        (samples > 0).then(|| Bands(energy.map(|e| (e / samples as f64).sqrt() as f32)))
    }
}

#[derive(Debug, PartialEq)]
struct Bands([f32; 3]);

impl fmt::Display for Bands {
    /// Decibels below full scale, low/mid/high, as in "-30.1/-38.4/-51.0".
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [low, mid, high] = self.0.map(db);
        write!(formatter, "{low:.1}/{mid:.1}/{high:.1}")
    }
}

/// Your microphone at the three points it is measured.
#[derive(Debug, Default)]
pub struct MicTaps {
    /// As captured, resampled to 48 kHz.
    pub captured: BandMeter,
    /// After WebRTC's echo cancellation, noise suppression and gain.
    pub processed: BandMeter,
    /// After typing suppression: what the voice gate and encoder get.
    pub sent: BandMeter,
}

/// What the audio engine measures: the sum played to you, and your microphone.
#[derive(Debug, Default)]
pub struct EngineLevels {
    /// Set by the hub each tick from [`on`].
    pub on: bool,
    /// Everything played (voices, stream audio and sounds), before the device volume.
    pub output: Meter,
    /// Samples of `output` (48 kHz stereo) loud enough for the limiter to ease them down.
    pub limited: usize,
    /// Your microphone, measured only while it is not muted.
    pub mic: MicTaps,
    /// 20 ms frames the voice gate let through to the encoder.
    pub sent_frames: usize,
    /// Echo cancellation's figures as last read, when it is on.
    pub echo: Option<crate::audio_processing::EchoStats>,
    /// Your microphone into and out of echo cancellation since the last report.
    pub echo_energy: crate::audio_processing::EchoEnergy,
}

/// One person heard on one server.
#[derive(Debug)]
struct Talker {
    meter: Meter,
    /// Their volume as set for them, applied after the meter.
    volume: f32,
}

#[derive(Debug)]
pub struct VoiceLevels {
    /// By server and client id. Names stay out of the log.
    talkers: BTreeMap<(u64, u16), Talker>,
    next: Instant,
}

impl Default for VoiceLevels {
    fn default() -> Self {
        Self {
            talkers: BTreeMap::new(),
            next: Instant::now() + EVERY,
        }
    }
}

impl VoiceLevels {
    /// One talker's decoded audio, before their volume.
    pub fn heard(&mut self, line: u64, client: u16, samples: &[f32]) {
        self.talkers
            .entry((line, client))
            .or_insert(Talker {
                meter: Meter::default(),
                volume: 1.0,
            })
            .meter
            .add(samples);
    }

    /// The volume a talker heard since the last report is played at.
    pub fn volume(&mut self, line: u64, client: u16, volume: f32) {
        if let Some(talker) = self.talkers.get_mut(&(line, client)) {
            talker.volume = volume;
        }
    }

    /// Writes a line once a second when anyone was heard, then starts over.
    pub fn report(&mut self, now: Instant, engine: &mut EngineLevels, volume_percent: u8) {
        if now < self.next {
            return;
        }
        self.next = now + EVERY;
        let output = engine.output.take();
        let limited = std::mem::take(&mut engine.limited);
        report_mic(engine);
        let heard: Vec<String> = std::mem::take(&mut self.talkers)
            .into_iter()
            .filter_map(|((line, client), mut talker)| {
                // Near silence is a talker between words.
                let level = talker.meter.take().filter(|level| level.peak >= 1e-4)?;
                Some(format!("{line}:{client} {level} x{:.2}", talker.volume))
            })
            .collect();
        if heard.is_empty() {
            return;
        }
        tracing::debug!(
            target: "flexspeak::voice",
            talkers = heard.len(),
            heard = heard.join(", ").as_str(),
            output = output.map(tracing::field::display),
            // 96 stereo samples are a millisecond; rounded to a tenth.
            limited_ms = (limited as f64 / 9.6).round() / 10.0,
            volume = volume_percent,
            "levels"
        );
    }
}

/// Writes your microphone's line when it was measured in the last second.
fn report_mic(engine: &mut EngineLevels) {
    let captured = engine.mic.captured.take();
    let processed = engine.mic.processed.take();
    let sent = engine.mic.sent.take();
    let sent_frames = std::mem::take(&mut engine.sent_frames);
    let echo_gain = std::mem::take(&mut engine.echo_energy).gain();
    let Some(captured) = captured else {
        return;
    };
    let echo = engine.echo.unwrap_or_default();
    let round = |value: Option<f64>| value.map(|value| (value * 10.0).round() / 10.0);
    tracing::debug!(
        target: "flexspeak::voice",
        captured = %captured,
        processed = processed.map(tracing::field::display),
        sent = sent.map(tracing::field::display),
        sent_ms = sent_frames * 20,
        echo_gain = echo_gain.as_deref(),
        erle = round(echo.erle),
        erl = round(echo.erl),
        echo_delay_ms = echo.delay_ms,
        divergent = round(echo.divergent_filter_fraction),
        residual_echo = round(echo.residual_echo_likelihood),
        "mic"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meter_reads_rms_and_peak_in_decibels_and_starts_over() {
        let mut meter = Meter::default();
        assert_eq!(meter.take(), None);
        meter.add(&[0.5, -0.5, 0.5, -0.5, f32::NAN, f32::INFINITY]);
        let level = meter.take().unwrap();
        assert_eq!(
            level,
            Level {
                rms: 0.5,
                peak: 0.5
            }
        );
        assert_eq!(level.to_string(), "-6.0/-6.0");
        assert_eq!(meter.take(), None);
    }

    #[test]
    fn talkers_are_kept_apart_and_cleared_after_each_report() {
        let mut levels = VoiceLevels::default();
        levels.heard(1, 7, &[0.1; 4]);
        levels.heard(1, 7, &[0.3; 4]);
        levels.heard(2, 7, &[0.0; 4]);
        levels.volume(1, 7, 0.5);
        levels.volume(3, 7, 0.2);
        assert_eq!(levels.talkers.len(), 2);
        assert_eq!(levels.talkers[&(1, 7)].volume, 0.5);
        assert_eq!(levels.talkers[&(2, 7)].volume, 1.0);
        let mut engine = EngineLevels {
            limited: 96,
            ..EngineLevels::default()
        };
        engine.output.add(&[0.2; 8]);
        // Not due yet: nothing is read.
        levels.report(Instant::now(), &mut engine, 100);
        assert_eq!(levels.talkers.len(), 2);
        assert_eq!(engine.limited, 96);
        levels.report(Instant::now() + EVERY, &mut engine, 100);
        assert!(levels.talkers.is_empty());
        assert_eq!(engine.limited, 0);
        assert_eq!(engine.output.take(), None);
    }

    #[test]
    fn band_meter_splits_body_clarity_and_air() {
        let tone = |hz: f32| -> Vec<f32> {
            (0..48_000)
                .map(|i| 0.5 * (std::f32::consts::TAU * hz * i as f32 / 48_000.0).sin())
                .collect()
        };
        for (hz, band) in [(200.0, 0), (2000.0, 1), (9000.0, 2)] {
            let mut meter = BandMeter::default();
            meter.add(&tone(hz));
            let Bands(levels) = meter.take().unwrap();
            let loudest = (0..3)
                .max_by(|&a, &b| levels[a].total_cmp(&levels[b]))
                .unwrap();
            assert_eq!(loudest, band, "{hz} Hz lands in its band: {levels:?}");
            // A sine at 0.5 has an RMS of about -9 dB; its own band keeps most of it.
            assert!(db(levels[band]) > -12.0, "{hz} Hz: {levels:?}");
        }
        let mut meter = BandMeter::default();
        assert_eq!(meter.take(), None);
        meter.add(&[f32::NAN]);
        assert_eq!(meter.take(), None);
    }

    #[test]
    fn mic_is_read_with_the_rest_and_starts_over() {
        let mut levels = VoiceLevels::default();
        let mut engine = EngineLevels::default();
        engine.mic.captured.add(&[0.1; 480]);
        engine.mic.sent.add(&[0.1; 480]);
        engine.sent_frames = 3;
        levels.report(Instant::now() + EVERY, &mut engine, 100);
        assert_eq!(engine.mic.captured.take(), None);
        assert_eq!(engine.mic.sent.take(), None);
        assert_eq!(engine.sent_frames, 0);
    }
}
