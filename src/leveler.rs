//! Evens out how loud the people you hear are: each talker gets a slow gain of their own that
//! brings their speech to one level, before the volume you set for them.
//!
//! The level is measured as broadcast loudness is (ITU-R BS.1770): K-weighted, in 100 ms
//! blocks, and gated twice, so silence, breaths and a noisy room between words don't count.
//! A block below [`FLOOR`] is ignored, and so is one more than [`GATE`] under the talker's own
//! running level; a quiet talker is therefore measured by their speech, not thrown out for
//! being quiet. Once their speech level is known, nothing more than [`REACH`] under it counts
//! either, so an open microphone's room, heard alone for minutes, is never taken for a voice
//! and turned up. Someone who really drops further than that stays as quiet as they made
//! themselves until they reconnect, which is the safe way round. The gain follows that level
//! slowly, as a fader would, not as a compressor: shouting stays louder than talking. It holds
//! while they are silent, is capped both ways, and peaks it pushes too far are eased down by
//! the output's limiter.

/// Where every talker's speech is brought to, in LKFS per channel. Well-set microphones arrive
/// around here, so they are left nearly as they are.
pub const TARGET: f32 = -20.0;
/// The most a talker is turned up, in dB. Their noise rises with them, so not without end.
pub const MAX_BOOST: f32 = 20.0;
/// The most a talker is turned down, in dB.
pub const MAX_CUT: f32 = 20.0;

/// Blocks quieter than this are silence, as in BS.1770.
const FLOOR: f32 = -70.0;
/// Blocks this far under the talker's running level are pauses, as in BS.1770.
const GATE: f32 = 10.0;
/// How far under their settled speech level a block may be and still count as speech.
const REACH: f32 = 15.0;
/// 100 ms at 48 kHz: short enough that one word settles a level.
const BLOCK_FRAMES: usize = 4800;
/// Blocks the running levels look back over: about 8 s of speech.
const MEMORY_BLOCKS: f64 = 80.0;
/// Blocks of speech before the level is trusted: until then the gain may rise faster.
const SETTLED_BLOCKS: f64 = 20.0;
/// How fast the gain rises once the level is settled, in dB per second of their audio.
const RISE: f32 = 6.0;
/// How fast it falls, and rises while the level is new. Too loud is corrected sooner.
const FALL: f32 = 12.0;

/// A second-order filter section in transposed direct form II.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Section {
    b: [f64; 3],
    a: [f64; 2],
    z: [f64; 2],
}

impl Section {
    const fn new(b: [f64; 3], a: [f64; 2]) -> Self {
        Self { b, a, z: [0.0; 2] }
    }

    fn run(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

/// BS.1770's K-weighting for 48 kHz, with the standard's coefficients: a shelf that lifts
/// what the ear hears best, then a high-pass that takes rumble out of the measurement.
pub(crate) const K_WEIGHTING: [Section; 2] = [
    Section::new(
        [1.53512485958697, -2.69169618940638, 1.19839281085285],
        [-1.69065929318241, 0.73248077421585],
    ),
    Section::new([1.0, -2.0, 1.0], [-1.99004745483398, 0.99007225036621]),
];

/// One sample through both stages of a [`K_WEIGHTING`] filter.
pub(crate) fn k_weighted(filter: &mut [Section; 2], sample: f32) -> f64 {
    let shelved = filter[0].run(f64::from(sample));
    filter[1].run(shelved)
}

/// A mean of block energies that forgets: plain at first, so the first blocks count in full,
/// then weighted towards the last [`MEMORY_BLOCKS`].
#[derive(Debug, Clone, Copy, Default)]
struct Running {
    energy: f64,
    blocks: f64,
}

impl Running {
    fn add(&mut self, energy: f64) {
        self.blocks = (self.blocks + 1.0).min(MEMORY_BLOCKS);
        self.energy += (energy - self.energy) / self.blocks;
    }

    fn level(&self) -> Option<f32> {
        (self.blocks > 0.0).then(|| loudness(self.energy))
    }
}

/// Mean square to LKFS for one channel, with BS.1770's offset.
fn loudness(energy: f64) -> f32 {
    (-0.691 + 10.0 * energy.max(1e-12).log10()) as f32
}

/// One talker's level and the gain that evens it out.
#[derive(Debug, Clone)]
pub struct Leveler {
    filters: [[Section; 2]; 2],
    /// K-weighted energy and frames of the block being filled.
    energy: f64,
    frames: usize,
    /// Every block above the floor; the gate hangs from it.
    ungated: Running,
    /// The blocks above the gate: their speech.
    speech: Running,
    /// The gain in use, in dB. It moves towards what `speech` asks for.
    gain: f32,
}

impl Default for Leveler {
    fn default() -> Self {
        Self {
            filters: [K_WEIGHTING; 2],
            energy: 0.0,
            frames: 0,
            ungated: Running::default(),
            speech: Running::default(),
            gain: 0.0,
        }
    }
}

impl Leveler {
    /// The gain to play this talker at, in dB.
    pub fn gain_db(&self) -> f32 {
        self.gain
    }

    /// The same as a factor for samples.
    pub fn gain(&self) -> f32 {
        10f32.powf(self.gain / 20.0)
    }

    /// Their speech level so far in LKFS, once any was heard.
    #[cfg(test)]
    fn level(&self) -> Option<f32> {
        self.speech.level()
    }

    /// Measures the talker's decoded audio (48 kHz, interleaved stereo, before any volume) and
    /// moves the gain towards what it asks for.
    pub fn hear(&mut self, stereo: &[f32]) {
        for frame in stereo.as_chunks::<2>().0 {
            for (filter, &sample) in self.filters.iter_mut().zip(frame) {
                let sample = if sample.is_finite() { sample } else { 0.0 };
                let weighted = k_weighted(filter, sample);
                self.energy += weighted * weighted;
            }
            self.frames += 1;
            if self.frames == BLOCK_FRAMES {
                // The mean over both channels, so one voice reads the same in mono and stereo.
                let energy = std::mem::take(&mut self.energy) / (2 * BLOCK_FRAMES) as f64;
                self.frames = 0;
                self.block(energy);
            }
        }
        self.approach(self.wanted(), stereo.len() / 2);
    }

    /// Lets the gain go back to none, for a talker who is no longer evened out.
    pub fn rest(&mut self, frames: usize) {
        self.approach(0.0, frames);
    }

    fn block(&mut self, energy: f64) {
        let level = loudness(energy);
        if level < FLOOR {
            return;
        }
        self.ungated.add(energy);
        let mut gate = self.ungated.level().map_or(FLOOR, |ungated| ungated - GATE);
        if self.speech.blocks >= SETTLED_BLOCKS
            && let Some(speech) = self.speech.level()
        {
            gate = gate.max(speech - REACH);
        }
        if level >= gate {
            self.speech.add(energy);
        }
    }

    /// The gain their speech asks for; none until they have spoken.
    fn wanted(&self) -> f32 {
        self.speech
            .level()
            .map_or(0.0, |level| (TARGET - level).clamp(-MAX_CUT, MAX_BOOST))
    }

    fn approach(&mut self, wanted: f32, frames: usize) {
        let seconds = frames as f32 / 48_000.0;
        let rise = if self.speech.blocks < SETTLED_BLOCKS {
            FALL
        } else {
            RISE
        };
        self.gain = if wanted > self.gain {
            (self.gain + rise * seconds).min(wanted)
        } else {
            (self.gain - FALL * seconds).max(wanted)
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sine in both channels at `amplitude`, `seconds` long.
    fn tone(frequency: f32, amplitude: f32, seconds: f32) -> Vec<f32> {
        (0..(48_000.0 * seconds) as usize)
            .flat_map(|frame| {
                let sample =
                    amplitude * (std::f32::consts::TAU * frequency * frame as f32 / 48_000.0).sin();
                [sample, sample]
            })
            .collect()
    }

    /// Speech-like: syllables of a few voice harmonics at `level` dBFS RMS, with gaps of
    /// `noise` dBFS between them, fed in 10 ms pieces as the mixer does.
    fn talk(leveler: &mut Leveler, level: f32, noise: f32, seconds: f32) {
        let amplitude = 10f32.powf(level / 20.0) * std::f32::consts::SQRT_2 / 3f32.sqrt();
        let noise = 10f32.powf(noise / 20.0);
        let mut seed = 0x2545_f491u32;
        let samples: Vec<f32> = (0..(48_000.0 * seconds) as usize)
            .flat_map(|frame| {
                let time = frame as f32 / 48_000.0;
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                let hiss = noise * 1.7 * (seed as f32 / u32::MAX as f32 * 2.0 - 1.0);
                // 250 ms of voice, 150 ms of pause.
                let voiced = time % 0.4 < 0.25;
                let voice: f32 = [180.0, 540.0, 1260.0]
                    .iter()
                    .map(|harmonic| (std::f32::consts::TAU * harmonic * time).sin())
                    .sum();
                let sample = if voiced { amplitude * voice } else { 0.0 } + hiss;
                [sample, sample]
            })
            .collect();
        for piece in samples.chunks(960) {
            leveler.hear(piece);
        }
    }

    #[test]
    fn k_weighting_reads_a_full_scale_997_hz_sine_as_the_standard_says() {
        let mut leveler = Leveler::default();
        leveler.hear(&tone(997.0, 1.0, 2.0));
        let level = leveler.level().unwrap();
        assert!((level - -3.01).abs() < 0.05, "{level}");
    }

    #[test]
    fn a_quiet_and_a_loud_talker_end_up_at_the_same_level() {
        for arrives in [-38.0, -30.0, -18.0, -8.0] {
            let mut leveler = Leveler::default();
            talk(&mut leveler, arrives, -90.0, 20.0);
            let played = leveler.level().unwrap() + leveler.gain_db();
            assert!(
                (played - TARGET).abs() < 0.5,
                "{arrives} dB plays at {played} LKFS"
            );
        }
    }

    #[test]
    fn noise_between_words_is_not_taken_for_their_voice() {
        let mut clean = Leveler::default();
        talk(&mut clean, -35.0, -90.0, 20.0);
        let mut noisy = Leveler::default();
        talk(&mut noisy, -35.0, -60.0, 20.0);
        assert!(
            (clean.gain_db() - noisy.gain_db()).abs() < 1.0,
            "{} against {}",
            clean.gain_db(),
            noisy.gain_db()
        );
    }

    #[test]
    fn the_gain_is_capped_both_ways() {
        let mut faint = Leveler::default();
        talk(&mut faint, -62.0, -90.0, 20.0);
        assert_eq!(faint.gain_db(), MAX_BOOST);
        // Louder than anything a sample can hold: only the cap stops the cut.
        let mut screaming = Leveler {
            speech: Running {
                energy: 1e3,
                blocks: MEMORY_BLOCKS,
            },
            ..Leveler::default()
        };
        screaming.approach(screaming.wanted(), 48_000 * 10);
        assert_eq!(screaming.gain_db(), -MAX_CUT);
    }

    #[test]
    fn silence_and_room_noise_leave_the_gain_where_it_was() {
        let mut leveler = Leveler::default();
        talk(&mut leveler, -30.0, -90.0, 20.0);
        let settled = leveler.gain_db();
        assert!(settled > 5.0 && settled < 15.0, "room to rise: {settled}");
        for _ in 0..3000 {
            leveler.hear(&[0.0; 960]);
        }
        assert_eq!(leveler.gain_db(), settled, "digital silence");
        // Three minutes of the room alone on an open microphone, 30 dB under their voice.
        talk(&mut leveler, -200.0, -60.0, 180.0);
        assert!(
            (leveler.gain_db() - settled).abs() < 0.5,
            "{} after {settled}",
            leveler.gain_db()
        );
    }

    #[test]
    fn someone_who_turns_their_microphone_down_is_followed() {
        let mut leveler = Leveler::default();
        talk(&mut leveler, -20.0, -90.0, 20.0);
        let before = leveler.gain_db();
        // Turning up is the slow direction: energies are averaged, and the louder past weighs
        // more than the quieter present.
        talk(&mut leveler, -32.0, -90.0, 80.0);
        assert!(
            (leveler.gain_db() - before - 12.0).abs() < 1.0,
            "{} after {before}",
            leveler.gain_db()
        );
    }

    #[test]
    fn the_gain_moves_slowly_and_starts_at_none() {
        let mut leveler = Leveler::default();
        assert_eq!(leveler.gain_db(), 0.0);
        assert_eq!(leveler.gain(), 1.0);
        // Half a second into a quiet talker's first words, less than 6 dB has been added.
        talk(&mut leveler, -40.0, -90.0, 0.5);
        assert!(leveler.gain_db() > 2.0 && leveler.gain_db() <= 6.0);
        // Two seconds of speech later they are nearly there.
        talk(&mut leveler, -40.0, -90.0, 2.5);
        assert!(leveler.gain_db() > 17.0, "{}", leveler.gain_db());

        // Settled, someone raising their voice by 10 dB is followed over seconds, not at once.
        talk(&mut leveler, -40.0, -90.0, 20.0);
        let settled = leveler.gain_db();
        talk(&mut leveler, -30.0, -90.0, 1.0);
        assert!(
            settled - leveler.gain_db() < 4.0,
            "fell {} dB in a second",
            settled - leveler.gain_db()
        );
    }

    #[test]
    fn a_talker_no_longer_evened_out_goes_back_to_their_own_level() {
        let mut leveler = Leveler::default();
        talk(&mut leveler, -40.0, -90.0, 20.0);
        leveler.rest(4800);
        assert!(leveler.gain_db() > 15.0, "no jump");
        leveler.rest(48_000 * 2);
        assert_eq!(leveler.gain_db(), 0.0);
    }

    #[test]
    fn odd_input_does_no_harm() {
        let mut leveler = Leveler::default();
        leveler.hear(&[f32::NAN, f32::INFINITY, 0.5]);
        leveler.hear(&[]);
        assert_eq!(leveler.gain_db(), 0.0);
        assert!(leveler.gain().is_finite());
    }
}
