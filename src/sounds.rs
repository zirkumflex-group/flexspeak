//! The short sounds the app plays for events, and where each one comes from.
//!
//! Flexspeak's own sounds are generated in `audio.rs`. TeamSpeak's sounds are never shipped: they
//! are read from a TeamSpeak install on this computer, and the pack only appears if one exists.
//!
//! One pack plays every event. Which events have a sound is a switch per group and per sound.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::audio::{Cue, RATE, SoftCue, Voice, cue_samples, soft_samples};

/// Longer than any cue; anything beyond it isn't a notification sound.
const MAX_SECONDS: usize = 4;
/// How loud every pack plays, as [`loudness`] measures it: where most of TeamSpeak's files
/// already are, so the volume slider means the same whichever pack is chosen.
const LEVEL_DB: f32 = -14.0;
/// How far the volume slider reaches down from full, in even steps: every tenth is 4 dB.
const VOLUME_RANGE_DB: f32 = 40.0;

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
    PersonConnected,
    PersonDisconnected,
    PersonLostConnection,
    PersonMovedIn,
    PersonMovedOut,
    PersonKickedIntoChannel,
    PersonKickedFromChannel,
    PersonKickedFromServer,
    PersonBanned,
    Connected,
    Disconnected,
    ConnectionLost,
    Reconnected,
    StreamStarted,
    StreamStopped,
    Kicked,
    KickedFromChannel,
    Banned,
    PersonRecordingStarted,
    PersonRecordingStopped,
    TalkPowerGranted,
    TalkPowerRevoked,
    WhisperReceived,
    InsufficientPermissions,
    ServerGroupAssigned,
    ServerGroupRevoked,
    ChannelGroupChanged,
    PersonStreamStarted,
    PersonStreamStopped,
}

impl Sound {
    pub const ALL: [Self; 40] = [
        Self::MicOff,
        Self::MicOn,
        Self::Deafened,
        Self::Undeafened,
        Self::Poke,
        Self::JoinedChannel,
        Self::MovedChannel,
        Self::PersonJoinedChannel,
        Self::PersonLeftChannel,
        Self::PersonConnected,
        Self::PersonDisconnected,
        Self::PersonLostConnection,
        Self::PersonMovedIn,
        Self::PersonMovedOut,
        Self::PersonKickedIntoChannel,
        Self::PersonKickedFromChannel,
        Self::PersonKickedFromServer,
        Self::PersonBanned,
        Self::StreamStarted,
        Self::StreamStopped,
        Self::PersonStreamStarted,
        Self::PersonStreamStopped,
        Self::ViewerJoined,
        Self::ViewerLeft,
        Self::Connected,
        Self::Disconnected,
        Self::ConnectionLost,
        Self::Reconnected,
        Self::Kicked,
        Self::KickedFromChannel,
        Self::Banned,
        Self::PersonRecordingStarted,
        Self::PersonRecordingStopped,
        Self::TalkPowerGranted,
        Self::TalkPowerRevoked,
        Self::WhisperReceived,
        Self::InsufficientPermissions,
        Self::ServerGroupAssigned,
        Self::ServerGroupRevoked,
        Self::ChannelGroupChanged,
    ];

    /// The group it is switched on and off with in Settings.
    pub fn group(self) -> Group {
        match self {
            Self::MicOff | Self::MicOn | Self::Deafened | Self::Undeafened => Group::Controls,
            Self::JoinedChannel | Self::MovedChannel => Group::Channel,
            Self::PersonJoinedChannel
            | Self::PersonLeftChannel
            | Self::PersonConnected
            | Self::PersonDisconnected
            | Self::PersonLostConnection
            | Self::PersonMovedIn
            | Self::PersonMovedOut
            | Self::PersonKickedIntoChannel
            | Self::PersonKickedFromChannel
            | Self::PersonKickedFromServer
            | Self::PersonBanned
            | Self::PersonRecordingStarted
            | Self::PersonRecordingStopped => Group::People,
            Self::StreamStarted
            | Self::StreamStopped
            | Self::PersonStreamStarted
            | Self::PersonStreamStopped
            | Self::ViewerJoined
            | Self::ViewerLeft => Group::Streams,
            Self::Connected | Self::Disconnected | Self::ConnectionLost | Self::Reconnected => {
                Group::Connection
            }
            Self::Poke | Self::WhisperReceived => Group::Messages,
            Self::Kicked
            | Self::KickedFromChannel
            | Self::Banned
            | Self::TalkPowerGranted
            | Self::TalkPowerRevoked
            | Self::InsufficientPermissions
            | Self::ServerGroupAssigned
            | Self::ServerGroupRevoked
            | Self::ChannelGroupChanged => Group::Moderation,
        }
    }

    /// The older sound this one took its choice from in settings files written when every
    /// sound chose its own pack. Only reading those files uses it.
    fn follows(self) -> Option<Self> {
        match self {
            Self::PersonRecordingStarted => Some(Self::PersonJoinedChannel),
            Self::PersonRecordingStopped => Some(Self::PersonLeftChannel),
            Self::TalkPowerGranted => Some(Self::MicOn),
            Self::TalkPowerRevoked => Some(Self::MicOff),
            Self::WhisperReceived => Some(Self::Poke),
            Self::InsufficientPermissions => Some(Self::Poke),
            Self::ServerGroupAssigned => Some(Self::Poke),
            Self::ServerGroupRevoked => Some(Self::Poke),
            Self::ChannelGroupChanged => Some(Self::Poke),
            Self::PersonConnected | Self::PersonMovedIn | Self::PersonKickedIntoChannel => {
                Some(Self::PersonJoinedChannel)
            }
            Self::PersonDisconnected
            | Self::PersonLostConnection
            | Self::PersonMovedOut
            | Self::PersonKickedFromChannel
            | Self::PersonKickedFromServer
            | Self::PersonBanned => Some(Self::PersonLeftChannel),
            Self::KickedFromChannel => Some(Self::Kicked),
            Self::PersonStreamStarted => Some(Self::StreamStarted),
            Self::PersonStreamStopped => Some(Self::StreamStopped),
            _ => None,
        }
    }

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
            Self::Kicked => "Kicked from the server",
            Self::PersonConnected => "Someone connected to your channel",
            Self::PersonDisconnected => "Someone in your channel disconnected",
            Self::PersonLostConnection => "Someone in your channel lost connection",
            Self::PersonMovedIn => "Someone was moved into your channel",
            Self::PersonMovedOut => "Someone was moved out of your channel",
            Self::PersonKickedIntoChannel => "Someone was kicked into your channel",
            Self::PersonKickedFromChannel => "Someone was kicked from your channel",
            Self::PersonKickedFromServer => "Someone in your channel was kicked",
            Self::PersonBanned => "Someone in your channel was banned",
            Self::KickedFromChannel => "Kicked from the channel",
            Self::Banned => "Banned",
            Self::PersonRecordingStarted => "Someone in your channel started recording",
            Self::PersonRecordingStopped => "Someone in your channel stopped recording",
            Self::TalkPowerGranted => "You were given talk power",
            Self::TalkPowerRevoked => "Your talk power was taken away",
            Self::WhisperReceived => "Someone is whispering to you",
            Self::InsufficientPermissions => "The server refused an action",
            Self::ServerGroupAssigned => "You were added to a server group",
            Self::ServerGroupRevoked => "You were removed from a server group",
            Self::ChannelGroupChanged => "Your channel group changed",
            Self::PersonStreamStarted => "Someone in your channel started streaming",
            Self::PersonStreamStopped => "Someone in your channel stopped streaming",
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
            Self::PersonConnected => "sound-person-connected",
            Self::PersonDisconnected => "sound-person-disconnected",
            Self::PersonLostConnection => "sound-person-connection-lost",
            Self::PersonMovedIn => "sound-person-moved-in",
            Self::PersonMovedOut => "sound-person-moved-out",
            Self::PersonKickedIntoChannel => "sound-person-kicked-in",
            Self::PersonKickedFromChannel => "sound-person-kicked-channel",
            Self::PersonKickedFromServer => "sound-person-kicked-server",
            Self::PersonBanned => "sound-person-banned",
            Self::KickedFromChannel => "sound-kicked-channel",
            Self::Banned => "sound-banned",
            Self::PersonRecordingStarted => "sound-person-recording-started",
            Self::PersonRecordingStopped => "sound-person-recording-stopped",
            Self::TalkPowerGranted => "sound-talk-power-granted",
            Self::TalkPowerRevoked => "sound-talk-power-revoked",
            Self::WhisperReceived => "sound-whisper",
            Self::InsufficientPermissions => "sound-denied",
            Self::ServerGroupAssigned => "sound-server-group-assigned",
            Self::ServerGroupRevoked => "sound-server-group-revoked",
            Self::ChannelGroupChanged => "sound-channel-group-changed",
            Self::PersonStreamStarted => "sound-person-stream-started",
            Self::PersonStreamStopped => "sound-person-stream-stopped",
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
            Self::PersonRecordingStarted => Cue::Poke,
            Self::PersonRecordingStopped => Cue::ViewerLeft,
            Self::TalkPowerGranted => Cue::MicOn,
            Self::TalkPowerRevoked => Cue::MicOff,
            Self::WhisperReceived => Cue::ViewerJoined,
            Self::InsufficientPermissions => Cue::SoundOff,
            Self::ServerGroupAssigned => Cue::ViewerJoined,
            Self::ServerGroupRevoked => Cue::ViewerLeft,
            Self::ChannelGroupChanged => Cue::Moved,
            Self::PersonConnected => Cue::ViewerJoined,
            Self::PersonDisconnected => Cue::ViewerLeft,
            Self::PersonLostConnection => Cue::ViewerLeft,
            Self::PersonMovedIn => Cue::ViewerJoined,
            Self::PersonMovedOut => Cue::ViewerLeft,
            Self::PersonKickedIntoChannel => Cue::ViewerJoined,
            Self::PersonKickedFromChannel => Cue::Moved,
            Self::PersonKickedFromServer => Cue::Moved,
            Self::PersonBanned => Cue::Moved,
            Self::KickedFromChannel => Cue::Moved,
            Self::PersonStreamStarted => Cue::StreamOn,
            Self::PersonStreamStopped => Cue::StreamOff,
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
                Self::PersonConnected => (Voice::Drop, &[74, 79], 90, 0.0, 0.09),
                Self::PersonDisconnected => (Voice::Drop, &[79, 74], 90, 0.0, 0.09),
                Self::PersonLostConnection => (Voice::Hum, &[67, 62], 180, 0.0, 0.09),
                Self::PersonMovedIn => (Voice::Felt, &[67, 72], 90, 0.0, 0.09),
                Self::PersonMovedOut => (Voice::Felt, &[72, 67], 90, 0.0, 0.09),
                Self::PersonKickedIntoChannel => (Voice::Wood, &[57, 62], 110, 0.0, 0.12),
                Self::PersonKickedFromChannel => (Voice::Wood, &[57, 57], 110, 0.0, 0.12),
                Self::PersonKickedFromServer => (Voice::Wood, &[55, 55], 130, 0.0, 0.12),
                Self::PersonBanned => (Voice::Wood, &[55, 52, 48], 140, 0.0, 0.12),
                Self::KickedFromChannel => (Voice::Wood, &[53, 53], 130, 0.0, 0.16),
                Self::Banned => (Voice::Wood, &[50, 47, 43], 170, 0.0, 0.16),
                Self::PersonRecordingStarted => (Voice::Bell, &[88, 88], 100, 0.0, 0.09),
                Self::PersonRecordingStopped => (Voice::Bell, &[88, 81], 100, 0.0, 0.07),
                Self::TalkPowerGranted => (Voice::Hum, &[60, 67], 130, 0.0, 0.11),
                Self::TalkPowerRevoked => (Voice::Hum, &[67, 60], 130, 0.0, 0.11),
                Self::WhisperReceived => (Voice::Glass, &[91], 0, -3.0, 0.07),
                Self::InsufficientPermissions => (Voice::Wood, &[45], 0, 0.0, 0.16),
                Self::ServerGroupAssigned => (Voice::Felt, &[72, 76, 79], 100, 0.0, 0.1),
                Self::ServerGroupRevoked => (Voice::Felt, &[79, 76, 72], 100, 0.0, 0.1),
                Self::ChannelGroupChanged => (Voice::Felt, &[76, 79, 76], 100, 0.0, 0.1),
                Self::PersonStreamStarted => (Voice::Glass, &[76, 83], 110, 0.0, 0.07),
                Self::PersonStreamStopped => (Voice::Glass, &[83, 76], 110, 0.0, 0.07),
            };
        SoftCue {
            voice,
            notes,
            step_ms,
            glide,
            gain,
        }
    }

    /// Whether this answers something you just did, so it plays at once, in place of whatever
    /// was playing. Everything else waits its turn.
    pub fn urgent(self) -> bool {
        matches!(
            self,
            Self::MicOff
                | Self::MicOn
                | Self::Deafened
                | Self::Undeafened
                | Self::JoinedChannel
                | Self::Disconnected
        )
    }

    /// TeamSpeak has no sound for people watching your stream. Its pack gets one made to sit
    /// beside its stream sounds: the same plain, swelling tone, stepping up to the note
    /// "stream started" ends on when someone arrives and back down when they leave.
    fn matched(self) -> Option<SoftCue> {
        let notes: &'static [u8] = match self {
            Self::ViewerJoined => &[82, 87],
            Self::ViewerLeft => &[87, 82],
            _ => return None,
        };
        Some(SoftCue {
            voice: Voice::Swell,
            notes,
            step_ms: 120,
            glide: 0.0,
            gain: 0.2,
        })
    }

    /// This event's file in TeamSpeak's sound folder, if TeamSpeak has a sound for it.
    fn file_name(self) -> Option<&'static str> {
        Some(match self {
            Self::ViewerJoined | Self::ViewerLeft => return None,
            Self::MicOff => "mic_muted.wav",
            Self::MicOn => "mic_activated.wav",
            Self::Deafened => "sound_muted.wav",
            Self::Undeafened => "sound_resumed.wav",
            Self::Poke => "you_were_poked.wav",
            Self::JoinedChannel => "channel_switched.wav",
            Self::MovedChannel => "you_were_moved.wav",
            Self::PersonJoinedChannel => "neutral_switched_tocurrentchannel.wav",
            Self::PersonLeftChannel => "neutral_switched_awayfromcurrentchannel.wav",
            Self::Connected | Self::Reconnected => "connected.wav",
            Self::Disconnected => "disconnected.wav",
            Self::ConnectionLost => "connection_lost.wav",
            // TeamSpeak plays these for anyone's stream, yours included.
            Self::StreamStarted | Self::PersonStreamStarted => "stream_started.wav",
            Self::StreamStopped | Self::PersonStreamStopped => "stream_stopped.wav",
            Self::Kicked => "you_kicked_server.wav",
            Self::PersonConnected => "neutral_connection_connected_currentchannel.wav",
            Self::PersonDisconnected => "neutral_connection_disconnected_currentchannel.wav",
            Self::PersonLostConnection => "neutral_connection_connectionlost_currentchannel.wav",
            Self::PersonMovedIn => "neutral_moved_tocurrentchannel.wav",
            Self::PersonMovedOut => "neutral_moved_awayfromcurrentchannel.wav",
            Self::PersonKickedIntoChannel => "neutral_kicked_channel_tocurrentchannel.wav",
            Self::PersonKickedFromChannel => "neutral_kicked_channel_awayfromcurrentchannel.wav",
            Self::PersonKickedFromServer => "neutral_kicked_server_currentchannel.wav",
            Self::PersonBanned => "neutral_banned_server_currentchannel.wav",
            Self::KickedFromChannel => "you_kicked_channel.wav",
            Self::Banned => "you_were_banned.wav",
            Self::PersonRecordingStarted => "neutral_recording_started_currentchannel.wav",
            Self::PersonRecordingStopped => "neutral_recording_stopped_currentchannel.wav",
            Self::TalkPowerGranted => "talkpower_granted.wav",
            Self::TalkPowerRevoked => "talkpower_revoked.wav",
            Self::WhisperReceived => "whisper_received.wav",
            Self::InsufficientPermissions => "insufficient_permissions.wav",
            Self::ServerGroupAssigned => "servergroup_assigned.wav",
            Self::ServerGroupRevoked => "servergroup_revoked.wav",
            Self::ChannelGroupChanged => "channelgroup_changed.wav",
        })
    }
}

/// One set of sounds, used for every event.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pack {
    #[default]
    Flexspeak,
    Soft,
    TeamSpeak,
}

impl Pack {
    pub const ALL: [Self; 3] = [Self::Flexspeak, Self::Soft, Self::TeamSpeak];

    pub fn title(self) -> &'static str {
        match self {
            Self::Flexspeak => "Flexspeak",
            Self::Soft => "Soft",
            Self::TeamSpeak => "TeamSpeak",
        }
    }

    fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// What brings this pack's sounds, as its recipes make them, to the level every pack plays
    /// at. One figure for the pack, so the differences made between its sounds stay.
    /// TeamSpeak's files are levelled one by one as they are read.
    fn gain(self) -> f32 {
        let db = match self {
            Self::Flexspeak => 6.5,
            Self::Soft => 13.2,
            Self::TeamSpeak => 0.0,
        };
        10_f32.powf(db / 20.0)
    }
}

/// The sound for an event from the chosen pack, before the volume. TeamSpeak's pack plays
/// Flexspeak's sound for an event it has no file for, and for all of them while TeamSpeak isn't
/// installed.
pub fn samples(pack: Pack, sound: Sound, bank: &Bank) -> Vec<f32> {
    let (mut samples, made_by) = match (pack, bank.get(sound)) {
        (Pack::TeamSpeak, Some(samples)) => return samples.to_vec(),
        (Pack::Soft, _) => (soft_samples(sound.soft()), Pack::Soft),
        _ => (cue_samples(sound.cue()), Pack::Flexspeak),
    };
    let gain = made_by.gain();
    samples.iter_mut().for_each(|sample| *sample *= gain);
    samples
}

/// Events that are switched on and off together in Settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    Controls,
    Channel,
    People,
    Streams,
    Connection,
    Messages,
    Moderation,
}

impl Group {
    pub const ALL: [Self; 7] = [
        Self::Controls,
        Self::Channel,
        Self::People,
        Self::Streams,
        Self::Connection,
        Self::Messages,
        Self::Moderation,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Controls => "Your microphone and headphones",
            Self::Channel => "Changing channel",
            Self::People => "People in your channel",
            Self::Streams => "Streams",
            Self::Connection => "Connection",
            Self::Messages => "Pokes and whispers",
            Self::Moderation => "Kicks, bans and permissions",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Controls => "sounds-controls",
            Self::Channel => "sounds-channel",
            Self::People => "sounds-people",
            Self::Streams => "sounds-streams",
            Self::Connection => "sounds-connection",
            Self::Messages => "sounds-messages",
            Self::Moderation => "sounds-moderation",
        }
    }

    pub fn sounds(self) -> impl Iterator<Item = Sound> {
        Sound::ALL
            .into_iter()
            .filter(move |sound| sound.group() == self)
    }
}

/// Which sounds play: one pack for all of them, and a switch for each group and each sound.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Saved")]
pub struct Settings {
    /// 0 to 100 in even steps of loudness, with 0 silent and 100 the sounds as they are.
    pub volume: u8,
    #[serde(skip_serializing_if = "Pack::is_default")]
    pub pack: Pack,
    /// Whole groups switched off. The switches inside one are kept for when it comes back, and
    /// a sound added to it later is off with the rest.
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    off_groups: BTreeSet<Group>,
    /// Single sounds switched off.
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    off: BTreeSet<Sound>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            // Full volume is far louder than a sound over a call should be.
            volume: 50,
            pack: Pack::default(),
            off_groups: BTreeSet::new(),
            off: BTreeSet::new(),
        }
    }
}

impl Settings {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// What the volume multiplies every sound by.
    pub fn gain(&self) -> f32 {
        if self.volume == 0 {
            return 0.0;
        }
        let below_full = 1.0 - f32::from(self.volume.min(100)) / 100.0;
        10_f32.powf(-VOLUME_RANGE_DB * below_full / 20.0)
    }

    /// The volume that sounds the same as an old one, from when the slider was a plain
    /// percentage and the packs weren't yet equally loud.
    fn volume_from(percent: u8, pack: Pack) -> u8 {
        if percent == 0 {
            return 0;
        }
        let gain = f32::from(percent.min(100)) / 100.0 / pack.gain();
        let volume = 100.0 * (1.0 + 20.0 * gain.log10() / VOLUME_RANGE_DB);
        volume.round().clamp(1.0, 100.0) as u8
    }

    /// Whether this event makes a sound: its group is on, and so is it.
    pub fn plays(&self, sound: Sound) -> bool {
        self.group_on(sound.group()) && self.sound_on(sound)
    }

    pub fn group_on(&self, group: Group) -> bool {
        !self.off_groups.contains(&group)
    }

    /// The sound's own switch, which only counts while its group is on.
    pub fn sound_on(&self, sound: Sound) -> bool {
        !self.off.contains(&sound)
    }

    pub fn set_group(&mut self, group: Group, on: bool) {
        if on {
            self.off_groups.remove(&group);
        } else {
            self.off_groups.insert(group);
        }
    }

    pub fn set_sound(&mut self, sound: Sound, on: bool) {
        if on {
            self.off.remove(&sound);
        } else {
            self.off.insert(sound);
        }
    }
}

/// What a sound was set to when each one chose its own pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Legacy {
    Flexspeak,
    Soft,
    TeamSpeak,
    Off,
}

/// The settings as written to the file, by this version or an older one.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Saved {
    volume: Option<u8>,
    /// From when the volume was a plain percentage of full scale.
    volume_percent: Option<u8>,
    pack: Pack,
    off_groups: BTreeSet<Group>,
    off: BTreeSet<Sound>,
    /// From when each sound chose its own pack, or none.
    sources: BTreeMap<Sound, Legacy>,
    /// From when the TeamSpeak folder was typed in; the install is now found automatically.
    #[serde(rename = "folder")]
    _folder: Option<String>,
}

impl Default for Saved {
    fn default() -> Self {
        let settings = Settings::default();
        Self {
            volume: None,
            volume_percent: None,
            pack: settings.pack,
            off_groups: settings.off_groups,
            off: settings.off,
            sources: BTreeMap::new(),
            _folder: None,
        }
    }
}

impl From<Saved> for Settings {
    /// Per-sound packs become the one pack most sounds used, and the sounds that were off stay
    /// off: a group that was off entirely as the group, the rest one by one. An old volume
    /// becomes the one that sounds the same with that pack.
    fn from(saved: Saved) -> Self {
        let mut settings = Self::packed(&saved);
        settings.volume = match (saved.volume, saved.volume_percent) {
            (Some(volume), _) => volume.min(100),
            (None, Some(percent)) => Self::volume_from(percent, settings.pack),
            (None, None) => Self::default().volume,
        };
        settings
    }
}

impl Settings {
    /// The pack and switches of a saved file, with the volume still to come.
    fn packed(saved: &Saved) -> Self {
        fn was(sources: &BTreeMap<Sound, Legacy>, sound: Sound) -> Legacy {
            sources.get(&sound).copied().unwrap_or_else(|| {
                sound
                    .follows()
                    .map_or(Legacy::Flexspeak, |older| was(sources, older))
            })
        }
        let mut settings = Self {
            volume: Self::default().volume,
            pack: saved.pack,
            off_groups: saved.off_groups.clone(),
            off: saved.off.clone(),
        };
        if saved.sources.is_empty() {
            return settings;
        }
        let was = Sound::ALL.map(|sound| (sound, was(&saved.sources, sound)));
        let used = |pack: Pack| {
            let legacy = match pack {
                Pack::Flexspeak => Legacy::Flexspeak,
                Pack::Soft => Legacy::Soft,
                Pack::TeamSpeak => Legacy::TeamSpeak,
            };
            was.iter().filter(|(_, choice)| *choice == legacy).count()
        };
        // The last of equals wins, so going backwards a tie goes to the earlier pack.
        settings.pack = Pack::ALL
            .into_iter()
            .rev()
            .max_by_key(|pack| used(*pack))
            .unwrap_or_default();
        settings.off.extend(
            was.iter()
                .filter(|(_, choice)| *choice == Legacy::Off)
                .map(|(sound, _)| *sound),
        );
        for group in Group::ALL {
            if group.sounds().all(|sound| settings.off.contains(&sound)) {
                settings.off_groups.insert(group);
                settings.off.retain(|sound| sound.group() != group);
            }
        }
        settings
    }
}

/// Where TeamSpeak keeps its sounds. The macOS and Windows locations have been confirmed.
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
    sounds: HashMap<Sound, Arc<[f32]>>,
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

    /// Whatever can be read; a missing or unreadable file only loses that one sound. Every sound
    /// comes out equally loud, and an install also gets the sounds made to match it.
    fn from_folder(folder: &Path) -> Self {
        // Some events share a file, which is read and kept once.
        let mut files: HashMap<&str, Option<Arc<[f32]>>> = HashMap::new();
        let mut sounds: HashMap<Sound, Arc<[f32]>> = Sound::ALL
            .into_iter()
            .filter_map(|sound| {
                let name = sound.file_name()?;
                let samples = files.entry(name).or_insert_with(|| {
                    let bytes = std::fs::read(folder.join(name)).ok()?;
                    Some(leveled(decode(&bytes)?).into())
                });
                Some((sound, samples.clone()?))
            })
            .collect();
        if !sounds.is_empty() {
            for sound in Sound::ALL {
                if let Some(cue) = sound.matched() {
                    sounds.insert(sound, leveled(crate::audio::soft_samples(cue)).into());
                }
            }
        }
        Self { sounds }
    }

    pub fn get(&self, sound: Sound) -> Option<&[f32]> {
        self.sounds.get(&sound).map(|samples| &**samples)
    }

    /// The pack to show as chosen: a saved TeamSpeak choice reads as Flexspeak while
    /// TeamSpeak isn't installed, since that is what plays. With an install, an event it has
    /// no file for plays Flexspeak's sound.
    pub fn shown(&self, pack: Pack) -> Pack {
        if pack == Pack::TeamSpeak && self.is_empty() {
            Pack::Flexspeak
        } else {
            pack
        }
    }

    pub fn is_empty(&self) -> bool {
        self.sounds.is_empty()
    }

    #[cfg(test)]
    pub fn with(sound: Sound, samples: Vec<f32>) -> Self {
        Self {
            sounds: HashMap::from([(sound, samples.into())]),
        }
    }
}

/// How loud a sound is to the ear, in dB: the level of its loudest fifth of a second, weighted
/// the way hearing is (the filter of ITU-R BS.1770, whose numbers are for 48 kHz).
fn loudness(samples: &[f32]) -> Option<f32> {
    const _: () = assert!(RATE == 48_000);
    /// Each stage's `b0 b1 b2 a1 a2`: a high shelf, then a high-pass.
    const STAGES: [[f64; 5]; 2] = [
        [
            1.535_124_859_586_97,
            -2.691_696_189_406_38,
            1.198_392_810_852_85,
            -1.690_659_293_182_41,
            0.732_480_774_215_85,
        ],
        [1.0, -2.0, 1.0, -1.990_047_454_833_98, 0.990_072_250_366_21],
    ];
    let mut memory = [[0.0_f64; 4]; 2];
    let power: Vec<f64> = samples
        .iter()
        .map(|&sample| {
            let mut x = f64::from(sample);
            for (stage, [x1, x2, y1, y2]) in STAGES.iter().zip(&mut memory) {
                let y = stage[0] * x + stage[1] * *x1 + stage[2] * *x2
                    - stage[3] * *y1
                    - stage[4] * *y2;
                (*x2, *x1, *y2, *y1) = (*x1, x, *y1, y);
                x = y;
            }
            x * x
        })
        .collect();
    let window = power.len().min(RATE / 5);
    let mut sum: f64 = power[..window].iter().sum();
    let mut loudest = sum;
    for at in window..power.len() {
        sum += power[at] - power[at - window];
        loudest = loudest.max(sum);
    }
    (loudest > 0.0).then(|| (10.0 * (loudest / window as f64).log10()) as f32)
}

/// A sound without the silence around it, at the loudness every sound is played at.
///
/// TeamSpeak's files differ by about 8 dB: the newer ones (streams, whispers) are much quieter
/// than the rest, and one volume slider can't suit both. The level is where most of its files
/// already are, so they stay as they were and the quiet ones come up to them.
fn leveled(mut samples: Vec<f32>) -> Vec<f32> {
    /// A file that is nearly silent stays quiet rather than becoming noise.
    const MOST_DB: f32 = 9.0;
    const PEAK: f32 = 0.9;
    /// What is kept before the sound and after it, faded so the cut isn't heard.
    const LEAD: usize = RATE / 200;
    const TAIL: usize = RATE * 3 / 100;
    let peak = samples.iter().fold(0.0_f32, |peak, s| peak.max(s.abs()));
    // The files end in a faint hiss rather than nothing, so silence is 40 dB under the peak.
    let audible = |sample: &f32| sample.abs() > peak * 0.01;
    let Some(first) = samples.iter().position(audible) else {
        return samples;
    };
    let last = samples.iter().rposition(audible).unwrap_or(first);
    samples.truncate((last + 1 + TAIL).min(samples.len()));
    for (index, sample) in samples[last + 1..].iter_mut().enumerate() {
        *sample *= 1.0 - (index + 1) as f32 / TAIL as f32;
    }
    let lead = first.min(LEAD);
    samples.drain(..first - lead);
    for (index, sample) in samples[..lead].iter_mut().enumerate() {
        *sample *= index as f32 / lead as f32;
    }
    let Some(loudness) = loudness(&samples) else {
        return samples;
    };
    let gain = 10_f32
        .powf((LEVEL_DB - loudness).min(MOST_DB) / 20.0)
        .min(PEAK / peak);
    samples.iter_mut().for_each(|sample| *sample *= gain);
    samples
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
            folder.join(Sound::Poke.file_name().unwrap()),
            wav(1, RATE as u32, &[16384]),
        )
        .unwrap();
        std::fs::write(folder.join(Sound::MicOn.file_name().unwrap()), b"not a wav").unwrap();
        let bank = Bank::from_folder(&folder);
        std::fs::remove_dir_all(&folder).unwrap();

        assert_eq!(bank.get(Sound::Poke).map(<[f32]>::len), Some(1));
        assert!(bank.get(Sound::MicOn).is_none(), "unreadable");
        assert!(bank.get(Sound::MicOff).is_none(), "missing");
        assert_eq!(bank.shown(Pack::TeamSpeak), Pack::TeamSpeak);
        let none = Bank::from_folder(Path::new("/no/such/folder"));
        assert!(none.is_empty());
        assert_eq!(none.shown(Pack::TeamSpeak), Pack::Flexspeak);
        assert_eq!(none.shown(Pack::Soft), Pack::Soft);
    }

    fn tone(amplitude: f32, hertz: f32, length: usize) -> Vec<f32> {
        (0..length)
            .map(|i| amplitude * (std::f32::consts::TAU * hertz * i as f32 / RATE as f32).sin())
            .collect()
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0, |peak, s| peak.max(s.abs()))
    }

    #[test]
    fn teamspeak_sounds_all_come_out_equally_loud_and_without_silence() {
        let quiet = leveled(tone(0.1, 1000.0, RATE / 2));
        let loud = leveled(tone(0.7, 1000.0, RATE / 2));
        assert!((peak(&quiet) - peak(&loud)).abs() < 0.005);
        assert!(
            (loudness(&quiet).unwrap() + 14.0).abs() < 0.1,
            "{:?}",
            loudness(&quiet)
        );
        // To the ear a high note is louder than a lower one of the same size, so it gets less.
        assert!(peak(&leveled(tone(0.7, 4000.0, RATE / 2))) < peak(&loud) * 0.85);

        // TeamSpeak's stream sounds end in half a second of nothing, which would hold up the
        // next sound.
        let padded = [
            vec![0.0; RATE / 10],
            tone(0.3, 1000.0, RATE / 4),
            vec![0.0; RATE / 2],
        ]
        .concat();
        let trimmed = leveled(padded);
        assert!(
            trimmed.len() >= RATE / 4 && trimmed.len() < RATE / 4 + RATE / 20,
            "{}",
            trimmed.len()
        );

        // Nothing is raised without limit, pushed into clipping, or made out of silence.
        let faint = leveled(tone(0.001, 1000.0, RATE / 2));
        assert!(peak(&faint) < 0.003, "{}", peak(&faint));
        let mut click = vec![0.0; RATE / 2];
        click[100] = 0.5;
        assert!(peak(&leveled(click)) <= 0.9 + 1e-6);
        assert!(leveled(vec![0.0; 64]).iter().all(|sample| *sample == 0.0));
        assert!(leveled(Vec::new()).is_empty());
    }

    #[test]
    fn a_teamspeak_install_gets_viewer_sounds_made_to_match_it() {
        let folder = std::env::temp_dir().join(format!("flexspeak-viewers-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let file: Vec<i16> = tone(0.2, 622.0, RATE / 4)
            .iter()
            .map(|sample| (sample * 32767.0) as i16)
            .collect();
        for sound in [Sound::StreamStarted, Sound::StreamStopped] {
            std::fs::write(
                folder.join(sound.file_name().unwrap()),
                wav(1, RATE as u32, &file),
            )
            .unwrap();
        }
        let bank = Bank::from_folder(&folder);
        std::fs::remove_dir_all(&folder).unwrap();

        // TeamSpeak plays the same file whoever starts the stream.
        assert_eq!(
            bank.get(Sound::PersonStreamStarted),
            bank.get(Sound::StreamStarted)
        );
        assert!(bank.get(Sound::PersonStreamStopped).is_some());
        // It has none for viewers, and the files named like one are never played by it.
        assert!(Sound::ViewerJoined.file_name().is_none());
        let joined = bank.get(Sound::ViewerJoined).expect("made to match");
        let left = bank.get(Sound::ViewerLeft).expect("made to match");
        assert_ne!(joined, left);
        let stream = loudness(bank.get(Sound::StreamStarted).unwrap()).unwrap();
        for viewer in [joined, left] {
            assert!(viewer.len() < RATE / 2, "a short sound, it happens often");
            assert!((loudness(viewer).unwrap() - stream).abs() < 0.5);
            assert!(viewer[0].abs() < 0.01 && viewer[viewer.len() - 1].abs() < 0.01);
        }
        assert!(
            Bank::from_folder(Path::new("/no/such/folder"))
                .get(Sound::ViewerJoined)
                .is_none(),
            "only beside TeamSpeak's own"
        );
    }

    #[test]
    fn every_pack_is_as_loud_as_the_others_and_none_clips() {
        let bank = Bank::default();
        for pack in [Pack::Flexspeak, Pack::Soft] {
            let mut levels: Vec<f32> = Sound::ALL
                .into_iter()
                .map(|sound| {
                    let made = samples(pack, sound, &bank);
                    assert!(peak(&made) > 0.1, "{pack:?} {sound:?} is too quiet");
                    assert!(peak(&made) <= 0.9, "{pack:?} {sound:?} is too loud");
                    loudness(&made).unwrap()
                })
                .collect();
            levels.sort_by(f32::total_cmp);
            let middle = (levels[levels.len() / 2 - 1] + levels[levels.len() / 2]) / 2.0;
            // A changed recipe moves this: put the difference into `Pack::gain`.
            assert!(
                (middle - LEVEL_DB).abs() < 0.3,
                "{pack:?} plays at {middle}"
            );
            assert!(
                levels[levels.len() - 1] - levels[0] < 12.0,
                "{pack:?} keeps its sounds within reach of each other"
            );
        }
        // Without TeamSpeak its pack is Flexspeak's, and with it a missing file is too.
        assert_eq!(
            samples(Pack::TeamSpeak, Sound::Poke, &bank),
            samples(Pack::Flexspeak, Sound::Poke, &bank)
        );
        let installed = Bank::with(Sound::Poke, vec![0.25]);
        assert_eq!(samples(Pack::TeamSpeak, Sound::Poke, &installed), [0.25]);
        assert_eq!(
            samples(Pack::TeamSpeak, Sound::MicOn, &installed),
            samples(Pack::Flexspeak, Sound::MicOn, &bank)
        );
    }

    #[test]
    fn the_volume_moves_in_even_steps_and_starts_in_the_middle() {
        let at = |volume| Settings {
            volume,
            ..Settings::default()
        };
        assert_eq!(Settings::default().volume, 50);
        assert_eq!(at(0).gain(), 0.0, "off");
        assert!(
            (at(100).gain() - 1.0).abs() < 1e-6,
            "the sounds as they are"
        );
        assert!((at(50).gain() - 0.1).abs() < 1e-6, "20 dB under full");
        for volume in (10..=90).step_by(10) {
            let step = at(volume + 10).gain() / at(volume).gain();
            assert!((20.0 * step.log10() - 4.0).abs() < 1e-3, "{volume}");
        }
        assert!(at(1).gain() > 0.01, "the quietest setting is still heard");
        assert_eq!(at(200).gain(), at(100).gain());
    }

    #[test]
    fn an_older_volume_becomes_the_one_that_sounds_the_same() {
        let read = |json: &str| serde_json::from_str::<Settings>(json).unwrap();
        // A tenth of full scale with TeamSpeak's sounds is the middle of the new slider.
        let quiet = read(r#"{"volume_percent":10,"pack":"team_speak"}"#);
        assert_eq!(quiet.volume, 50);
        assert!((quiet.gain() - 0.1).abs() < 1e-6);
        assert_eq!(
            read(r#"{"volume_percent":100,"pack":"team_speak"}"#).volume,
            100
        );
        // The other packs got louder, so the same loudness is a lower setting.
        for (pack, name) in [(Pack::Flexspeak, "flexspeak"), (Pack::Soft, "soft")] {
            for percent in [100_u8, 40, 10] {
                let old = read(&format!(
                    r#"{{"volume_percent":{percent},"pack":"{name}"}}"#
                ));
                let now = old.gain() * pack.gain();
                let then = f32::from(percent) / 100.0;
                assert!(
                    (20.0 * (now / then).log10()).abs() < 0.25,
                    "{pack:?} at {percent}: {now} for {then}"
                );
            }
        }
        assert_eq!(read(r#"{"volume_percent":0}"#).volume, 0, "off stays off");
        assert_eq!(
            read(r#"{"volume_percent":1}"#).volume,
            1,
            "and quiet stays on"
        );
        // The new setting is read as it is, and a file without one starts in the middle.
        assert_eq!(read(r#"{"volume":30,"volume_percent":90}"#).volume, 30);
        assert_eq!(read(r#"{"pack":"soft"}"#).volume, 50);
        assert_eq!(read(r#"{"volume":250}"#).volume, 100);
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
    fn every_sound_is_in_one_group_and_every_group_has_sounds() {
        let grouped: usize = Group::ALL.iter().map(|group| group.sounds().count()).sum();
        assert_eq!(grouped, Sound::ALL.len());
        for group in Group::ALL {
            assert!(group.sounds().count() >= 2, "{group:?}");
            assert!(group.sounds().all(|sound| sound.group() == group));
        }
    }

    #[test]
    fn a_group_switch_silences_its_sounds_and_keeps_their_own_switches() {
        let mut settings = Settings::default();
        assert!(Sound::ALL.into_iter().all(|sound| settings.plays(sound)));

        settings.set_sound(Sound::ViewerLeft, false);
        assert!(!settings.plays(Sound::ViewerLeft));
        assert!(settings.plays(Sound::ViewerJoined));

        settings.set_group(Group::Streams, false);
        assert!(Group::Streams.sounds().all(|sound| !settings.plays(sound)));
        assert!(settings.plays(Sound::Poke), "other groups carry on");
        assert!(
            settings.sound_on(Sound::ViewerJoined),
            "its own switch is as it was"
        );

        settings.set_group(Group::Streams, true);
        assert!(settings.plays(Sound::ViewerJoined));
        assert!(!settings.plays(Sound::ViewerLeft), "still off by itself");
        settings.set_sound(Sound::ViewerLeft, true);
        assert!(settings.is_default());
    }

    #[test]
    fn only_what_was_changed_is_written() {
        let mut settings = Settings::default();
        assert_eq!(
            serde_json::to_string(&settings).unwrap(),
            r#"{"volume":50}"#
        );
        settings.pack = Pack::TeamSpeak;
        settings.set_group(Group::People, false);
        settings.set_sound(Sound::Poke, false);
        let json = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            json,
            r#"{"volume":50,"pack":"team_speak","off_groups":["people"],"off":["poke"]}"#
        );
        let read: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(read, settings);
        assert!(serde_json::from_str::<Settings>(r#"{"packs":1}"#).is_err());
    }

    #[test]
    fn per_sound_choices_from_older_files_become_one_pack_and_switches() {
        // The TeamSpeak pack as the old menu saved it: only the sounds others followed.
        let old: Settings = serde_json::from_str(
            r#"{"volume_percent":10,"folder":"/no-longer-installed/sound/default","sources":{
                "mic_off":"team_speak","mic_on":"team_speak","deafened":"team_speak",
                "undeafened":"team_speak","poke":"team_speak","joined_channel":"team_speak",
                "moved_channel":"team_speak","person_joined_channel":"team_speak",
                "person_left_channel":"off","person_banned":"team_speak",
                "viewer_joined":"soft","viewer_left":"off",
                "stream_started":"team_speak","stream_stopped":"team_speak",
                "connected":"off","disconnected":"off","connection_lost":"off",
                "reconnected":"off","kicked":"team_speak","banned":"team_speak"}}"#,
        )
        .unwrap();
        assert_eq!(old.volume, 50, "as loud as a tenth of full scale was");
        assert_eq!(old.pack, Pack::TeamSpeak, "what most sounds used");
        assert!(!old.group_on(Group::Connection), "all of it was off");
        assert!(old.group_on(Group::People) && old.group_on(Group::Streams));
        // Sounds without a choice of their own had followed an older one.
        for sound in [
            Sound::PersonLeftChannel,
            Sound::PersonDisconnected,
            Sound::ViewerLeft,
        ] {
            assert!(!old.plays(sound), "{sound:?}");
        }
        for sound in [
            Sound::PersonBanned,
            Sound::PersonConnected,
            Sound::ViewerJoined,
        ] {
            assert!(old.plays(sound), "{sound:?}");
        }
        let json = serde_json::to_string(&old).unwrap();
        assert!(
            !json.contains("sources") && !json.contains("folder"),
            "{json}"
        );
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), old);

        // A tie goes to Flexspeak's own, and so does a file with everything off.
        let none: Settings = serde_json::from_str(r#"{"sources":{"mic_off":"off"}}"#).unwrap();
        assert_eq!(none.pack, Pack::Flexspeak);
        assert!(!none.plays(Sound::MicOff) && !none.plays(Sound::TalkPowerRevoked));
        assert!(none.plays(Sound::MicOn));
        let defaults: Settings = serde_json::from_str(r#"{"folder":"/missing"}"#).unwrap();
        assert!(defaults.is_default());
    }
}
