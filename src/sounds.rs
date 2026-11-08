//! The short sounds the app plays for events, and where each one comes from.
//!
//! Flexspeak's own sounds are generated in `audio.rs`. TeamSpeak's sounds are embedded in the
//! executable from `assets/sounds/teamspeak`, so they work without a separate installation.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::audio::{Cue, RATE};

/// Longer than any cue; anything beyond it isn't a notification sound.
const MAX_SECONDS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sound {
    MicOff,
    MicOn,
    Deafened,
    Undeafened,
    Poke,
    JoinedChannel,
    MovedChannel,
    ViewerJoined,
    ViewerLeft,
}

impl Sound {
    pub const ALL: [Self; 9] = [
        Self::MicOff,
        Self::MicOn,
        Self::Deafened,
        Self::Undeafened,
        Self::Poke,
        Self::JoinedChannel,
        Self::MovedChannel,
        Self::ViewerJoined,
        Self::ViewerLeft,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::MicOff => "Microphone muted",
            Self::MicOn => "Microphone unmuted",
            Self::Deafened => "Deafened",
            Self::Undeafened => "Undeafened",
            Self::Poke => "Poked",
            Self::JoinedChannel => "Joined a channel",
            Self::MovedChannel => "Moved to a channel",
            Self::ViewerJoined => "Someone joined your stream",
            Self::ViewerLeft => "Someone left your stream",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::MicOff => "sound-mic-off",
            Self::MicOn => "sound-mic-on",
            Self::Deafened => "sound-deafened",
            Self::Undeafened => "sound-undeafened",
            Self::Poke => "sound-poked",
            Self::JoinedChannel => "sound-joined",
            Self::MovedChannel => "sound-moved",
            Self::ViewerJoined => "sound-viewer-joined",
            Self::ViewerLeft => "sound-viewer-left",
        }
    }

    pub fn cue(self) -> Cue {
        match self {
            Self::MicOff => Cue::MicOff,
            Self::MicOn => Cue::MicOn,
            Self::Deafened => Cue::SoundOff,
            Self::Undeafened => Cue::SoundOn,
            Self::Poke => Cue::Poke,
            Self::JoinedChannel => Cue::Joined,
            Self::MovedChannel => Cue::Moved,
            Self::ViewerJoined => Cue::ViewerJoined,
            Self::ViewerLeft => Cue::ViewerLeft,
        }
    }

    /// The bundled WAV for this event.
    fn wav(self) -> &'static [u8] {
        match self {
            Self::MicOff => include_bytes!("../assets/sounds/teamspeak/mic_muted.wav"),
            Self::MicOn => include_bytes!("../assets/sounds/teamspeak/mic_activated.wav"),
            Self::Deafened => include_bytes!("../assets/sounds/teamspeak/sound_muted.wav"),
            Self::Undeafened => include_bytes!("../assets/sounds/teamspeak/sound_resumed.wav"),
            Self::Poke => include_bytes!("../assets/sounds/teamspeak/you_were_poked.wav"),
            Self::JoinedChannel => {
                include_bytes!("../assets/sounds/teamspeak/channel_switched.wav")
            }
            Self::MovedChannel => include_bytes!("../assets/sounds/teamspeak/you_were_moved.wav"),
            Self::ViewerJoined => include_bytes!("../assets/sounds/teamspeak/client_joined.wav"),
            Self::ViewerLeft => include_bytes!("../assets/sounds/teamspeak/client_left.wav"),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    #[default]
    Flexspeak,
    TeamSpeak,
    Off,
}

impl Source {
    pub fn title(self) -> &'static str {
        match self {
            Self::Flexspeak => "Flexspeak",
            Self::TeamSpeak => "TeamSpeak",
            Self::Off => "Off",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub volume_percent: u8,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub sources: BTreeMap<Sound, Source>,
    /// Accept older settings files; sounds no longer use an external folder.
    #[serde(rename = "folder", skip_serializing)]
    legacy_folder: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            volume_percent: 100,
            sources: BTreeMap::new(),
            legacy_folder: None,
        }
    }
}

impl Settings {
    pub fn is_default(&self) -> bool {
        self.volume_percent == Self::default().volume_percent && self.sources.is_empty()
    }

    pub fn source(&self, sound: Sound) -> Source {
        self.sources.get(&sound).copied().unwrap_or_default()
    }

    /// Stores a choice, leaving the default unwritten.
    pub fn set_source(&mut self, sound: Sound, source: Source) {
        if source == Source::default() {
            self.sources.remove(&sound);
        } else {
            self.sources.insert(sound, source);
        }
    }
}

/// The bundled TeamSpeak sounds decoded at the app's sample rate.
#[derive(Debug)]
pub struct Bank {
    sounds: HashMap<Sound, Vec<f32>>,
}

impl Bank {
    pub fn bundled() -> Self {
        let sounds = Sound::ALL
            .into_iter()
            .map(|sound| (sound, decode(sound.wav()).expect("valid bundled sound")))
            .collect();
        Self { sounds }
    }

    pub fn get(&self, sound: Sound) -> &[f32] {
        &self.sounds[&sound]
    }
}

/// PCM WAV to mono samples at the app's rate. `None` for anything else.
fn decode(bytes: &[u8]) -> Option<Vec<f32>> {
    if bytes.get(..4)? != b"RIFF" || bytes.get(8..12)? != b"WAVE" {
        return None;
    }
    let mut format = None;
    let mut data = None;
    let mut at = 12;
    while let Some(header) = bytes.get(at..at + 8) {
        let size = u32::from_le_bytes(header[4..8].try_into().ok()?) as usize;
        let start = at + 8;
        let end = start.saturating_add(size).min(bytes.len());
        let body = &bytes[start..end];
        match &header[..4] {
            b"fmt " => format = Some(body),
            b"data" => data = Some(body),
            _ => {}
        }
        at = start.saturating_add(size).saturating_add(size & 1);
    }
    let format = format?;
    let number = |at: usize| Some(u16::from_le_bytes(format.get(at..at + 2)?.try_into().ok()?));
    let channels = usize::from(number(2)?);
    let rate = u32::from_le_bytes(format.get(4..8)?.try_into().ok()?) as usize;
    if number(0)? != 1 || number(14)? != 16 || channels == 0 || rate == 0 {
        return None;
    }
    let frames: Vec<f32> = data?
        .chunks_exact(2 * channels)
        .take(rate.checked_mul(MAX_SECONDS)?)
        .map(|frame| {
            let sum: f32 = frame
                .as_chunks::<2>()
                .0
                .iter()
                .map(|sample| f32::from(i16::from_le_bytes(*sample)) / 32768.0)
                .sum();
            sum / channels as f32
        })
        .collect();
    if frames.is_empty() {
        return None;
    }
    let step = rate as f64 / RATE as f64;
    let length = (frames.len() as f64 / step) as usize;
    Some(
        (0..length)
            .map(|i| {
                let position = i as f64 * step;
                let first = position as usize;
                let next = frames.get(first + 1).copied().unwrap_or(frames[first]);
                frames[first] + (next - frames[first]) * (position - first as f64) as f32
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(channels: u16, rate: u32, samples: &[i16]) -> Vec<u8> {
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut out = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        out.extend(16_u32.to_le_bytes());
        out.extend(1_u16.to_le_bytes());
        out.extend(channels.to_le_bytes());
        out.extend(rate.to_le_bytes());
        out.extend((rate * u32::from(channels) * 2).to_le_bytes());
        out.extend((channels * 2).to_le_bytes());
        out.extend(16_u16.to_le_bytes());
        // A chunk to skip, of odd length, as the real files have.
        out.extend(b"LIST");
        out.extend(3_u32.to_le_bytes());
        out.extend(b"abc\0");
        out.extend(b"data");
        out.extend((data.len() as u32).to_le_bytes());
        out.extend(data);
        out
    }

    #[test]
    fn wav_files_decode_to_mono_at_the_app_rate() {
        let same = decode(&wav(1, RATE as u32, &[16384, -16384])).unwrap();
        assert_eq!(same, [0.5, -0.5]);
        let stereo = decode(&wav(2, RATE as u32, &[16384, 0])).unwrap();
        assert_eq!(stereo, [0.25]);
        let slower = decode(&wav(1, 24_000, &[0, 16384])).unwrap();
        assert_eq!(slower.len(), 4, "doubled to reach the app's rate");
        assert!((slower[1] - 0.25).abs() < 1e-3, "interpolated");
    }

    #[test]
    fn anything_but_16_bit_pcm_wav_is_refused() {
        assert!(decode(b"not a wav").is_none());
        assert!(decode(b"RIFF\0\0\0\0WAVE").is_none());
        let mut float = wav(1, RATE as u32, &[1]);
        float[20] = 3;
        assert!(decode(&float).is_none());
        let truncated = wav(1, RATE as u32, &[1, 2, 3]);
        assert!(decode(&truncated[..truncated.len() - 7]).is_none_or(|s| s.len() < 3));
    }

    #[test]
    fn every_bundled_sound_decodes_to_audible_samples() {
        let bank = Bank::bundled();
        for sound in Sound::ALL {
            let samples = bank.get(sound);
            assert!(!samples.is_empty(), "{sound:?} is empty");
            assert!(samples.len() <= RATE * MAX_SECONDS, "{sound:?} is too long");
            assert!(
                samples
                    .iter()
                    .all(|sample| sample.is_finite() && sample.abs() <= 1.0),
                "{sound:?} has invalid samples"
            );
            assert!(
                samples.iter().any(|sample| sample.abs() > 0.01),
                "{sound:?} is silent"
            );
        }
    }

    #[test]
    fn legacy_sound_folders_are_ignored_and_not_saved() {
        let settings: Settings = serde_json::from_str(
            r#"{"folder":"/no-longer-installed/sound/default","sources":{"poke":"team_speak"}}"#,
        )
        .unwrap();
        assert_eq!(settings.source(Sound::Poke), Source::TeamSpeak);
        assert!(!serde_json::to_string(&settings).unwrap().contains("folder"));
        let defaults: Settings = serde_json::from_str(r#"{"folder":"/missing"}"#).unwrap();
        assert!(defaults.is_default());
    }

    #[test]
    fn only_changed_sounds_are_written() {
        let mut settings = Settings::default();
        assert!(settings.is_default());
        settings.set_source(Sound::Poke, Source::TeamSpeak);
        assert_eq!(settings.source(Sound::Poke), Source::TeamSpeak);
        assert_eq!(settings.source(Sound::MicOn), Source::Flexspeak);
        let json = serde_json::to_string(&settings).unwrap();
        assert!(json.contains("\"poke\":\"team_speak\""), "{json}");
        let read: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(read, settings);
        settings.set_source(Sound::Poke, Source::Flexspeak);
        assert!(settings.is_default());
    }
}
