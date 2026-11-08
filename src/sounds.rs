//! The short sounds the app plays for events, and where each one comes from.
//!
//! Flexspeak's own sounds are generated in `audio.rs`. TeamSpeak's sounds are never shipped: they
//! are read from a TeamSpeak install on this computer, and the choice only appears if one exists.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::audio::{Cue, RATE, SoftCue, Voice};

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
    PersonJoinedChannel,
    PersonLeftChannel,
    Connected,
    Disconnected,
    ConnectionLost,
    Reconnected,
    StreamStarted,
    StreamStopped,
    Kicked,
    Banned,
}

impl Sound {
    pub const ALL: [Self; 19] = [
        Self::MicOff,
        Self::MicOn,
        Self::Deafened,
        Self::Undeafened,
        Self::Poke,
        Self::JoinedChannel,
        Self::MovedChannel,
        Self::PersonJoinedChannel,
        Self::PersonLeftChannel,
        Self::ViewerJoined,
        Self::ViewerLeft,
        Self::StreamStarted,
        Self::StreamStopped,
        Self::Connected,
        Self::Disconnected,
        Self::ConnectionLost,
        Self::Reconnected,
        Self::Kicked,
        Self::Banned,
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
            Self::PersonJoinedChannel => "Someone joined your channel",
            Self::PersonLeftChannel => "Someone left your channel",
            Self::Connected => "Connected",
            Self::Disconnected => "Disconnected",
            Self::ConnectionLost => "Connection lost",
            Self::Reconnected => "Reconnected",
            Self::StreamStarted => "You started streaming",
            Self::StreamStopped => "You stopped streaming",
            Self::Kicked => "Kicked",
            Self::Banned => "Banned",
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
            Self::PersonJoinedChannel => "sound-person-joined",
            Self::PersonLeftChannel => "sound-person-left",
            Self::Connected => "sound-connected",
            Self::Disconnected => "sound-disconnected",
            Self::ConnectionLost => "sound-connection-lost",
            Self::Reconnected => "sound-reconnected",
            Self::StreamStarted => "sound-stream-started",
            Self::StreamStopped => "sound-stream-stopped",
            Self::Kicked => "sound-kicked",
            Self::Banned => "sound-banned",
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
            Self::ViewerJoined | Self::PersonJoinedChannel => Cue::ViewerJoined,
            Self::ViewerLeft | Self::PersonLeftChannel | Self::StreamStopped => Cue::ViewerLeft,
            Self::Connected | Self::StreamStarted => Cue::Joined,
            Self::Disconnected | Self::ConnectionLost => Cue::SoundOff,
            Self::Reconnected => Cue::SoundOn,
            Self::Kicked | Self::Banned => Cue::Moved,
        }
    }

    /// The Soft pack. The voice says what kind of action it is, the notes and rhythm which one.
    pub fn soft(self) -> SoftCue {
        let (voice, notes, step_ms, glide, gain): (Voice, &'static [u8], usize, f32, f32) =
            match self {
                Self::MicOff => (Voice::Wood, &[69, 64], 75, 0.0, 0.13),
                Self::MicOn => (Voice::Wood, &[64, 69], 75, 0.0, 0.13),
                Self::Deafened => (Voice::Hum, &[60], 0, -7.0, 0.12),
                Self::Undeafened => (Voice::Hum, &[53], 0, 7.0, 0.12),
                Self::Poke => (Voice::Bell, &[84, 84], 150, 0.0, 0.1),
                Self::JoinedChannel => (Voice::Felt, &[60, 72], 130, 0.0, 0.11),
                Self::MovedChannel => (Voice::Felt, &[72, 67, 72], 110, 0.0, 0.11),
                Self::PersonJoinedChannel => (Voice::Drop, &[74], 0, 5.0, 0.09),
                Self::PersonLeftChannel => (Voice::Drop, &[79], 0, -5.0, 0.09),
                Self::ViewerJoined => (Voice::Glass, &[83, 88], 100, 0.0, 0.07),
                Self::ViewerLeft => (Voice::Glass, &[88, 83], 100, 0.0, 0.07),
                Self::StreamStarted => (Voice::Glass, &[71, 76, 79, 83], 90, 0.0, 0.08),
                Self::StreamStopped => (Voice::Glass, &[83, 79, 76, 71], 90, 0.0, 0.08),
                Self::Connected => (Voice::Felt, &[60, 64, 67, 72], 120, 0.0, 0.11),
                Self::Disconnected => (Voice::Felt, &[72, 67, 64, 60], 140, 0.0, 0.11),
                Self::ConnectionLost => (Voice::Hum, &[63, 58], 220, 0.0, 0.12),
                Self::Reconnected => (Voice::Hum, &[58, 65], 180, 0.0, 0.12),
                Self::Kicked => (Voice::Wood, &[50, 50], 130, 0.0, 0.16),
                Self::Banned => (Voice::Wood, &[50, 47, 43], 170, 0.0, 0.16),
            };
        SoftCue {
            voice,
            notes,
            step_ms,
            glide,
            gain,
        }
    }

    /// This event's file in TeamSpeak's sound folder.
    fn file_name(self) -> &'static str {
        match self {
            Self::MicOff => "mic_muted.wav",
            Self::MicOn => "mic_activated.wav",
            Self::Deafened => "sound_muted.wav",
            Self::Undeafened => "sound_resumed.wav",
            Self::Poke => "you_were_poked.wav",
            Self::JoinedChannel => "channel_switched.wav",
            Self::MovedChannel => "you_were_moved.wav",
            Self::ViewerJoined => "client_joined.wav",
            Self::ViewerLeft => "client_left.wav",
            Self::PersonJoinedChannel => "neutral_switched_tocurrentchannel.wav",
            Self::PersonLeftChannel => "neutral_switched_awayfromcurrentchannel.wav",
            Self::Connected | Self::Reconnected => "connected.wav",
            Self::Disconnected => "disconnected.wav",
            Self::ConnectionLost => "connection_lost.wav",
            Self::StreamStarted => "stream_started.wav",
            Self::StreamStopped => "stream_stopped.wav",
            Self::Kicked => "you_kicked_server.wav",
            Self::Banned => "you_were_banned.wav",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    #[default]
    Flexspeak,
    Soft,
    TeamSpeak,
    Off,
}

impl Source {
    pub fn title(self) -> &'static str {
        match self {
            Self::Flexspeak => "Flexspeak",
            Self::Soft => "Soft",
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
    /// Accept settings files that stored a folder; the install is now found automatically.
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

/// Where TeamSpeak keeps its sounds. Only the macOS location has been confirmed.
fn install_folders() -> Vec<PathBuf> {
    const SOUNDS: &str = "html/client_ui/sound/default";
    let env = |name: &str| std::env::var_os(name).map(PathBuf::from);
    let mut folders = Vec::new();
    if cfg!(target_os = "macos") {
        const APP: &str = "TeamSpeak.app/Contents/Resources";
        folders.push(Path::new("/Applications").join(APP).join(SOUNDS));
        folders.extend(env("HOME").map(|home| home.join("Applications").join(APP).join(SOUNDS)));
    } else if cfg!(windows) {
        folders.extend(env("LOCALAPPDATA").map(|dir| dir.join("Programs/TeamSpeak").join(SOUNDS)));
        folders.extend(env("ProgramFiles").map(|dir| dir.join("TeamSpeak").join(SOUNDS)));
    } else {
        folders.push(Path::new("/opt/teamspeak").join(SOUNDS));
        folders.extend(env("HOME").map(|home| home.join(".local/share/TeamSpeak").join(SOUNDS)));
    }
    folders
}

/// TeamSpeak's sounds from this computer's install, decoded at the app's sample rate.
#[derive(Debug, Default)]
pub struct Bank {
    sounds: HashMap<Sound, Vec<f32>>,
}

impl Bank {
    /// Empty when TeamSpeak isn't installed.
    pub fn installed() -> Self {
        install_folders()
            .iter()
            .map(|folder| Self::from_folder(folder))
            .find(|bank| !bank.sounds.is_empty())
            .unwrap_or_default()
    }

    /// Whatever can be read; a missing or unreadable file only loses that one sound.
    fn from_folder(folder: &Path) -> Self {
        let sounds = Sound::ALL
            .into_iter()
            .filter_map(|sound| {
                let bytes = std::fs::read(folder.join(sound.file_name())).ok()?;
                Some((sound, decode(&bytes)?))
            })
            .collect();
        Self { sounds }
    }

    pub fn get(&self, sound: Sound) -> Option<&[f32]> {
        self.sounds.get(&sound).map(Vec::as_slice)
    }

    /// A saved TeamSpeak choice uses Flexspeak's sound while TeamSpeak isn't installed.
    pub fn usable(&self, sound: Sound, source: Source) -> Source {
        if source == Source::TeamSpeak && self.get(sound).is_none() {
            Source::Flexspeak
        } else {
            source
        }
    }

    pub fn is_empty(&self) -> bool {
        self.sounds.is_empty()
    }

    #[cfg(test)]
    pub fn with(sound: Sound, samples: Vec<f32>) -> Self {
        Self {
            sounds: HashMap::from([(sound, samples)]),
        }
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
    fn an_install_gives_only_the_sounds_it_can_read() {
        let folder = std::env::temp_dir().join(format!("flexspeak-sounds-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join(Sound::Poke.file_name()),
            wav(1, RATE as u32, &[16384]),
        )
        .unwrap();
        std::fs::write(folder.join(Sound::MicOn.file_name()), b"not a wav").unwrap();
        let bank = Bank::from_folder(&folder);
        std::fs::remove_dir_all(&folder).unwrap();

        assert_eq!(bank.get(Sound::Poke), Some([0.5].as_slice()));
        assert!(bank.get(Sound::MicOn).is_none(), "unreadable");
        assert!(bank.get(Sound::MicOff).is_none(), "missing");
        assert_eq!(
            bank.usable(Sound::Poke, Source::TeamSpeak),
            Source::TeamSpeak
        );
        assert_eq!(
            bank.usable(Sound::MicOff, Source::TeamSpeak),
            Source::Flexspeak
        );
        assert_eq!(bank.usable(Sound::MicOff, Source::Off), Source::Off);
        assert!(Bank::from_folder(Path::new("/no/such/folder")).is_empty());
    }

    #[test]
    fn soft_sounds_are_audible_and_quieter_than_the_flexspeak_ones() {
        for sound in Sound::ALL {
            let soft = crate::audio::soft_samples(sound.soft());
            let peak = soft.iter().fold(0.0_f32, |peak, s| peak.max(s.abs()));
            assert!(peak > 0.05, "{sound:?} is too quiet");
            let louder = crate::audio::cue_samples(sound.cue())
                .iter()
                .fold(0.0_f32, |peak, s| peak.max(s.abs()));
            assert!(peak < louder, "{sound:?} is not subtler");
        }
    }

    #[test]
    fn every_soft_sound_has_its_own_recipe() {
        for (index, sound) in Sound::ALL.iter().enumerate() {
            for other in &Sound::ALL[index + 1..] {
                assert_ne!(sound.soft(), other.soft(), "{sound:?} and {other:?}");
            }
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
