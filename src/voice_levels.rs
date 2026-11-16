//! How loud the people you hear are, summed up once a second for the log with debug logging on:
//! each person as they arrive, the volume they are played at, and everything you hear. It tells
//! whether someone who sounds quiet arrives quiet or is turned down on your end. Only what is
//! heard is measured, since the people talking may use another client.

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

/// What the audio engine measures: the sum played to you.
#[derive(Debug, Default)]
pub struct EngineLevels {
    /// Set by the hub each tick from [`on`].
    pub on: bool,
    /// Everything played (voices, stream audio and sounds), before the device volume.
    pub output: Meter,
    /// Samples of `output` (48 kHz stereo) loud enough for the limiter to ease them down.
    pub limited: usize,
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
}
