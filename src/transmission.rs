use crate::audio::FRAME;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    fmt,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Continuous,
    PushToTalk,
    VoiceActivation,
}
impl Mode {
    pub const ALL: [Self; 3] = [Self::Continuous, Self::PushToTalk, Self::VoiceActivation];
}
impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Continuous => "Continuous",
            Self::PushToTalk => "Push to talk",
            Self::VoiceActivation => "Voice activation",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub mode: Mode,
    pub ptt_release_ms: u16,
    pub voice_threshold_db: i16,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            mode: Mode::Continuous,
            ptt_release_ms: 150,
            voice_threshold_db: -40,
        }
    }
}
impl Settings {
    pub fn validate(self) -> Result<()> {
        ensure!(
            self.ptt_release_ms <= 500,
            "Push-to-talk release delay must be 0–500 ms"
        );
        ensure!(
            (-60..=0).contains(&self.voice_threshold_db),
            "Voice threshold must be -60–0 dBFS"
        );
        Ok(())
    }
}

#[derive(Default)]
pub struct PttGate {
    held: bool,
    release_until: Option<Instant>,
}

#[derive(Default)]
pub struct PacketBoundary {
    active: bool,
    pending_end: bool,
}
impl PacketBoundary {
    pub fn begin(&mut self, packets: &mut Vec<Vec<u8>>) -> bool {
        self.flush(packets);
        !std::mem::replace(&mut self.active, true)
    }
    pub fn end(&mut self) {
        self.pending_end |= std::mem::replace(&mut self.active, false);
    }
    pub fn flush(&mut self, packets: &mut Vec<Vec<u8>>) {
        if std::mem::take(&mut self.pending_end) {
            packets.push(Vec::new());
        }
    }
}
impl PttGate {
    pub fn allows(
        &mut self,
        permitted: bool,
        held: bool,
        now: Instant,
        settings: Settings,
    ) -> bool {
        if !permitted {
            *self = Self::default();
            return false;
        }
        if settings.mode != Mode::PushToTalk {
            return true;
        }
        if held {
            self.release_until = None;
        } else if self.held {
            self.release_until =
                Some(now + Duration::from_millis(u64::from(settings.ptt_release_ms)));
        }
        self.held = held;
        held || self.release_until.is_some_and(|until| now < until)
    }
}

pub struct VoiceGate {
    settings: Settings,
    pre_roll: VecDeque<[f32; FRAME]>,
    ready: VecDeque<[f32; FRAME]>,
    hangover: usize,
    active: bool,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    vad: Option<crate::native_audio::Vad>,
}
impl VoiceGate {
    pub fn new(settings: Settings) -> Result<Self> {
        settings.validate()?;
        ensure!(
            settings.mode != Mode::VoiceActivation || crate::audio_processing::AVAILABLE,
            "Speech detection is available on Mac and Windows. Choose continuous transmission or push-to-talk."
        );
        Ok(Self {
            settings,
            pre_roll: VecDeque::with_capacity(3),
            ready: VecDeque::with_capacity(4),
            hangover: 0,
            active: false,
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            vad: (settings.mode == Mode::VoiceActivation)
                .then(crate::native_audio::Vad::new)
                .transpose()?,
        })
    }
    pub fn reset(&mut self) -> Result<()> {
        *self = Self::new(self.settings)?;
        Ok(())
    }
    /// Takes a new threshold or release delay as it runs. A different mode needs a new gate,
    /// so that is refused.
    pub fn tune(&mut self, settings: Settings) -> bool {
        if settings.mode != self.settings.mode || settings.validate().is_err() {
            return false;
        }
        self.settings = settings;
        true
    }

    pub fn active(&self) -> bool {
        self.active
    }
    pub fn pop(&mut self) -> Option<[f32; FRAME]> {
        self.ready.pop_front()
    }
    pub fn buffer_bytes(&self) -> usize {
        (self.pre_roll.capacity() + self.ready.capacity()) * std::mem::size_of::<[f32; FRAME]>()
    }

    pub fn process(&mut self, frame: [f32; FRAME]) -> Result<()> {
        let mut detected = true;
        if self.settings.mode == Mode::VoiceActivation {
            let power = frame
                .iter()
                .map(|sample| f64::from(*sample).powi(2))
                .sum::<f64>()
                / FRAME as f64;
            let above_threshold = power.is_finite()
                && power > 0.0
                && 10.0 * power.log10() >= f64::from(self.settings.voice_threshold_db);
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            let speech = self
                .vad
                .as_mut()
                .unwrap()
                .speech(&frame.map(crate::audio_processing::pcm16))?;
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            let speech = false;
            detected = above_threshold && speech;
        }
        self.queue(frame, detected);
        Ok(())
    }

    fn queue(&mut self, frame: [f32; FRAME], detected: bool) {
        debug_assert!(
            self.ready.is_empty(),
            "Drain ready frames before processing more input"
        );
        // 60 ms pre-roll preserves starts; 200 ms hangover avoids chopping word endings.
        if detected {
            self.hangover = 10;
        }
        self.active = detected || self.hangover > 0;
        if !detected {
            self.hangover = self.hangover.saturating_sub(1);
        }
        if self.active {
            self.ready.extend(self.pre_roll.drain(..));
            self.ready.push_back(frame);
        } else {
            if self.pre_roll.len() == 3 {
                self.pre_roll.pop_front();
            }
            self.pre_roll.push_back(frame);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utterance_markers_survive_resets_and_multiple_transitions_in_one_tick() {
        let mut boundary = PacketBoundary::default();
        let mut packets = Vec::new();
        assert!(boundary.begin(&mut packets));
        packets.push(vec![1]);
        assert!(!boundary.begin(&mut packets));
        boundary.end();
        boundary.end();
        assert!(boundary.begin(&mut packets));
        packets.push(vec![2]);
        boundary.end();
        boundary.flush(&mut packets);
        boundary.flush(&mut packets);
        assert_eq!(packets, vec![vec![1], vec![], vec![2], vec![]]);
    }

    #[test]
    fn ptt_holds_releases_represses_and_permissions_cancel_the_tail() {
        let settings = Settings {
            mode: Mode::PushToTalk,
            ..Settings::default()
        };
        let mut gate = PttGate::default();
        let at = Instant::now();
        assert!(!gate.allows(true, false, at, settings));
        assert!(gate.allows(true, true, at, settings));
        assert!(gate.allows(true, false, at + Duration::from_millis(10), settings));
        assert!(gate.allows(true, false, at + Duration::from_millis(159), settings));
        assert!(!gate.allows(true, false, at + Duration::from_millis(160), settings));
        assert!(gate.allows(true, true, at + Duration::from_millis(170), settings));
        assert!(!gate.allows(false, true, at + Duration::from_millis(180), settings));
        assert!(!gate.allows(true, false, at + Duration::from_millis(190), settings));
        let instant = Settings {
            ptt_release_ms: 0,
            ..settings
        };
        assert!(gate.allows(true, true, at, instant));
        assert!(!gate.allows(true, false, at, instant));
    }

    #[test]
    fn voice_gate_preserves_starts_bounds_silence_and_clears_history() -> Result<()> {
        let mut gate = VoiceGate::new(Settings::default())?;
        for value in 1..=10 {
            gate.queue([value as f32; FRAME], false);
            assert!(gate.pop().is_none());
        }
        assert_eq!(gate.pre_roll.len(), 3);
        gate.queue([11.0; FRAME], true);
        for value in 8..=11 {
            assert_eq!(gate.pop().unwrap()[0], value as f32);
        }
        assert!(gate.pop().is_none());
        for _ in 0..10 {
            gate.queue([0.0; FRAME], false);
            assert!(gate.pop().is_some());
        }
        gate.queue([0.0; FRAME], false);
        assert!(!gate.active() && gate.pop().is_none());
        gate.reset()?;
        gate.queue([0.5; FRAME], true);
        assert_eq!(gate.pop().unwrap()[0], 0.5);
        assert!(gate.pop().is_none(), "Reset must not leak pre-mute audio");
        assert!(
            Settings {
                ptt_release_ms: 501,
                ..Settings::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            Settings {
                voice_threshold_db: -61,
                ..Settings::default()
            }
            .validate()
            .is_err()
        );
        Ok(())
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn native_speech_detection_and_threshold_gate_silence_and_voiced_audio() -> Result<()> {
        let mut gate = VoiceGate::new(Settings {
            mode: Mode::VoiceActivation,
            voice_threshold_db: -40,
            ..Settings::default()
        })?;
        for _ in 0..30 {
            gate.process([0.0; FRAME])?;
            assert!(gate.pop().is_none());
        }
        let mut detected = 0;
        for block in 0..50 {
            let frame = std::array::from_fn(|index| {
                let t = (block * FRAME + index) as f32 / 48_000.0;
                (std::f32::consts::TAU * 180.0 * t).sin() * 0.12
                    + (std::f32::consts::TAU * 540.0 * t).sin() * 0.04
            });
            gate.process(frame)?;
            while gate.pop().is_some() {
                detected += 1;
            }
        }
        assert!(
            detected > 0,
            "The native detector should recognize a voiced harmonic fixture"
        );
        let mut high_threshold = VoiceGate::new(Settings {
            voice_threshold_db: 0,
            ..gate.settings
        })?;
        for _ in 0..50 {
            high_threshold.process([0.02; FRAME])?;
            assert!(high_threshold.pop().is_none());
        }
        Ok(())
    }
}
