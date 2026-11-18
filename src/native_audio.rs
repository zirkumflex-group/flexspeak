use anyhow::{Result, anyhow, ensure};
use std::{ffi::c_void, ptr::NonNull};

unsafe extern "C" {
    fn flex_apm_create(echo: bool, gain: bool, noise: bool, level: i32) -> *mut c_void;
    fn flex_apm_destroy(processor: *mut c_void);
    fn flex_apm_capture(processor: *mut c_void, pcm: *mut i16, delay_ms: i32) -> i32;
    fn flex_apm_render(processor: *mut c_void, pcm: *mut i16) -> i32;
    fn flex_apm_set_output_muted(processor: *mut c_void, muted: bool) -> i32;
    fn flex_apm_stats(processor: *mut c_void, out: *mut EchoStatsC) -> i32;
    fn flex_vad_create() -> *mut c_void;
    fn flex_vad_destroy(vad: *mut c_void);
    fn flex_vad_process(vad: *mut c_void, pcm: *const i16) -> i32;
}

/// Mirrors `FlexEchoStats` in native/audio.cpp.
#[repr(C)]
#[derive(Default)]
struct EchoStatsC {
    erl: f64,
    erle: f64,
    divergent_filter_fraction: f64,
    residual_echo_likelihood: f64,
    delay_ms: i32,
    present: u32,
}

pub struct Apm(NonNull<c_void>);
impl Apm {
    pub fn new(settings: crate::audio_processing::Settings) -> Result<Self> {
        // The pointer owns one native processor, used only by the voice worker.
        let pointer = unsafe {
            flex_apm_create(
                settings.echo_cancellation,
                settings.automatic_gain,
                settings.noise_suppression,
                settings.suppression_level as i32,
            )
        };
        Ok(Self(NonNull::new(pointer).ok_or_else(|| {
            anyhow!("Cannot initialize microphone processing")
        })?))
    }
    pub fn capture(&mut self, pcm: &mut [i16; 480], delay: Option<i32>) -> Result<()> {
        ensure!(
            delay.is_none_or(|value| (0..=500).contains(&value)),
            "Invalid echo delay"
        );
        // Fixed-size slices match the native 48 kHz mono capture contract.
        ensure!(
            unsafe { flex_apm_capture(self.0.as_ptr(), pcm.as_mut_ptr(), delay.unwrap_or(-1)) }
                == 0,
            "Microphone processing failed"
        );
        Ok(())
    }
    pub fn render(&mut self, pcm: &mut [i16; 960]) -> Result<()> {
        // Fixed-size slices match the native 48 kHz stereo reference contract.
        ensure!(
            unsafe { flex_apm_render(self.0.as_ptr(), pcm.as_mut_ptr()) } == 0,
            "Echo reference processing failed"
        );
        Ok(())
    }
}
impl Apm {
    /// Tells the processor whether its output is sent. Capture still runs while muted.
    pub fn set_output_muted(&mut self, muted: bool) -> Result<()> {
        ensure!(
            unsafe { flex_apm_set_output_muted(self.0.as_ptr(), muted) } == 0,
            "Microphone processing failed"
        );
        Ok(())
    }

    pub fn stats(&mut self) -> Option<crate::audio_processing::EchoStats> {
        let mut raw = EchoStatsC::default();
        if unsafe { flex_apm_stats(self.0.as_ptr(), &mut raw) } != 0 {
            return None;
        }
        let has = |bit: u32| raw.present & bit != 0;
        Some(crate::audio_processing::EchoStats {
            erl: has(1).then_some(raw.erl),
            erle: has(2).then_some(raw.erle),
            divergent_filter_fraction: has(4).then_some(raw.divergent_filter_fraction),
            residual_echo_likelihood: has(8).then_some(raw.residual_echo_likelihood),
            delay_ms: has(16).then_some(raw.delay_ms),
        })
    }
}

impl Drop for Apm {
    fn drop(&mut self) {
        unsafe { flex_apm_destroy(self.0.as_ptr()) };
    }
}

pub struct Vad(NonNull<c_void>);
impl Vad {
    pub fn new() -> Result<Self> {
        Ok(Self(
            NonNull::new(unsafe { flex_vad_create() })
                .ok_or_else(|| anyhow!("Cannot initialize speech detection"))?,
        ))
    }
    pub fn speech(&mut self, pcm: &[i16; 960]) -> Result<bool> {
        // The detector accepts one 20 ms mono frame at 48 kHz.
        match unsafe { flex_vad_process(self.0.as_ptr(), pcm.as_ptr()) } {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(anyhow!("Speech detection failed")),
        }
    }
}
impl Drop for Vad {
    fn drop(&mut self) {
        unsafe { flex_vad_destroy(self.0.as_ptr()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    struct Noise(u64);

    impl Noise {
        fn next(&mut self) -> f32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0
        }
    }

    /// A second-order low or high pass (RBJ cookbook, Q = 1/sqrt 2) at 48 kHz.
    #[derive(Clone, Copy)]
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

    /// Energy below 1 kHz and above 4 kHz, in dB.
    struct Bands {
        low: [Biquad; 2],
        high: [Biquad; 2],
        energy: [f64; 2],
    }

    impl Bands {
        fn new() -> Self {
            Self {
                low: [Biquad::new(false, 1000.0); 2],
                high: [Biquad::new(true, 4000.0); 2],
                energy: [0.0; 2],
            }
        }

        fn add(&mut self, x: f32) {
            let low = self.low[0].run(x);
            let low = self.low[1].run(low);
            let high = self.high[0].run(x);
            let high = self.high[1].run(high);
            self.energy[0] += f64::from(low * low);
            self.energy[1] += f64::from(high * high);
        }

        fn take(&mut self) -> [f64; 2] {
            std::mem::take(&mut self.energy).map(|e| 10.0 * e.max(1e-12).log10())
        }
    }

    struct Run {
        /// Per second of talking: how much more processing took above 4 kHz than below 1 kHz.
        treble_loss: Vec<f64>,
        /// Per second: the output's level against the microphone's, in dB.
        level: Vec<f64>,
    }

    /// Music plays the whole time, as from a music bot, and `echo` of it reaches the
    /// microphone. The processor starts muted, then after `muted_s` the output is used, and
    /// with `talk` someone speaks: syllables of shaped noise, unrelated to the music.
    fn simulate(muted_s: usize, music: f32, echo: f32, talk: bool) -> Run {
        let settings = crate::audio_processing::Settings {
            noise_suppression: talk,
            suppression_level: crate::audio_processing::SuppressionLevel::Moderate,
            automatic_gain: talk,
            echo_cancellation: true,
            typing_suppression: false,
        };
        let mut apm = Apm::new(settings).unwrap();
        apm.set_output_muted(true).unwrap();
        let (mut music_noise, mut voice_noise) = (Noise(0x1234_5678_9abc_def1), Noise(0x0fed_cba9));
        let (mut music_tone, mut voice_tone) =
            (Biquad::new(false, 3000.0), Biquad::new(false, 900.0));
        // 8 ms from the speaker to the microphone.
        let mut path = VecDeque::from(vec![0.0f32; 384]);
        let (mut before, mut after) = (Bands::new(), Bands::new());
        let mut energy = [0.0f64; 2];
        let mut run = Run {
            treble_loss: Vec::new(),
            level: Vec::new(),
        };
        let mut phase = 0.0f32;
        for block in 0..(muted_s + 12) * 100 {
            let open = block >= muted_s * 100;
            if block == muted_s * 100 {
                apm.set_output_muted(false).unwrap();
            }
            let mut render = [0i16; 960];
            let mut capture = [0i16; 480];
            for i in 0..480 {
                let t = (block * 480 + i) as f32 / 48_000.0;
                let pitch = 220.0 * (1.0 + ((t * 0.5).floor() % 4.0) * 0.25);
                phase += std::f32::consts::TAU * pitch / 48_000.0;
                let m = music * (phase.sin() + 0.5 * (phase * 1.5).sin())
                    + music * music_tone.run(music_noise.next());
                render[i * 2] = (m * 32767.0) as i16;
                render[i * 2 + 1] = render[i * 2];
                path.push_back(m);
                let heard = echo * path.pop_front().unwrap();
                let word = talk && open && (t * 1000.0) as u32 % 300 < 200;
                let voice = if word {
                    0.25 * voice_tone.run(voice_noise.next()) + 0.01 * voice_noise.next()
                } else {
                    0.00003 * voice_noise.next()
                };
                capture[i] = ((voice + heard) * 32767.0) as i16;
            }
            apm.render(&mut render).unwrap();
            let raw = capture;
            apm.capture(&mut capture, Some(8)).unwrap();
            for (x, y) in raw.iter().zip(&capture) {
                let (x, y) = (f32::from(*x) / 32768.0, f32::from(*y) / 32768.0);
                energy[0] += f64::from(x * x);
                energy[1] += f64::from(y * y);
                if open {
                    before.add(x);
                    after.add(y);
                }
            }
            if block % 100 == 99 {
                let [x, y] = std::mem::take(&mut energy).map(|e| 10.0 * e.max(1e-12).log10());
                run.level.push(y - x);
                if open {
                    let ([low_in, high_in], [low_out, high_out]) = (before.take(), after.take());
                    run.treble_loss
                        .push((low_out - low_in) - (high_out - high_in));
                }
            }
        }
        run
    }

    #[test]
    fn a_headset_voice_keeps_its_treble_right_after_the_processor_starts() {
        // A headset: the music reaches the microphone 30 dB down, almost no echo.
        let run = simulate(6, 0.004, 0.03, true);
        let settled = run.treble_loss[8..].iter().sum::<f64>() / 4.0;
        for (second, loss) in run.treble_loss.iter().enumerate() {
            assert!(
                loss - settled < 2.0,
                "second {second} after unmuting lost {loss:.1} dB of treble, settled {settled:.1}: {:?}",
                run.treble_loss
            );
        }
    }

    #[test]
    fn echo_from_speakers_is_still_removed() {
        for echo in [0.1, 0.5] {
            let run = simulate(30, 0.05, echo, false);
            for (second, level) in run.level.iter().enumerate().skip(31) {
                assert!(
                    *level < -25.0,
                    "echo {echo}: second {second} only {level:.1} dB down: {:?}",
                    run.level
                );
            }
        }
    }
}
