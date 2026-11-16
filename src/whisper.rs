//! Whisper lists: talking to chosen people, channels or a group instead of your own channel.
//!
//! A list is kept the way TeamSpeak 3 keeps it, so lists imported from it work unchanged:
//! people by identity, channels by their path of names from the top (`Games/Raid night`), and
//! a group by kind and ID. A list either names people and channels, or one group, because the
//! two go to the server as different packets. Nothing here touches the network; `resolve`
//! turns a list into the IDs a whisper packet carries, for the server as it is right now.

use serde::{Deserialize, Serialize};

/// Settings > Whisper.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub lists: Vec<List>,
    /// Held: whispers back to whoever whispered to you last.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub reply_key: String,
    /// Off: whispers to you are dropped, with no cue.
    #[serde(skip_serializing_if = "is_true")]
    pub accept: bool,
    /// Everyone else is played at half volume while someone whispers to you.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub duck: bool,
}

fn is_true(value: &bool) -> bool {
    *value
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            lists: Vec::new(),
            reply_key: String::new(),
            accept: true,
            duck: false,
        }
    }
}

impl Settings {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct List {
    /// Stays the same when the list is renamed; TeamSpeak 3's own ID for an imported list.
    pub id: String,
    pub name: String,
    /// The saved server's address it is for; empty for any server where its targets are.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub server: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub key: String,
    /// One press starts the whisper and the next ends it, instead of holding the key.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub toggle: bool,
    pub to: Whom,
}

impl List {
    pub fn new(id: String, name: String) -> Self {
        Self {
            id,
            name,
            server: String::new(),
            key: String::new(),
            toggle: false,
            to: Whom::Listed(Vec::new()),
        }
    }

    /// Whether this list can be used on the server at `address`.
    pub fn for_server(&self, address: &str) -> bool {
        self.server.is_empty() || self.server.trim().eq_ignore_ascii_case(address.trim())
    }

    /// What the list whispers to, in a few words: "Mara, Theo and 1 channel".
    pub fn summary(&self) -> String {
        match &self.to {
            Whom::Listed(entries) if entries.is_empty() => "Nobody yet".into(),
            Whom::Listed(entries) => {
                let names: Vec<&str> = entries
                    .iter()
                    .filter_map(|entry| match entry {
                        Entry::Client { name, .. } => Some(name.as_str()),
                        Entry::Channel { .. } => None,
                    })
                    .collect();
                let channels = entries.len() - names.len();
                let mut parts = Vec::new();
                match names.len() {
                    0 => {}
                    1..=3 => parts.push(names.join(", ")),
                    n => parts.push(format!("{n} people")),
                }
                match channels {
                    0 => {}
                    1 => parts.push("1 channel".into()),
                    n => parts.push(format!("{n} channels")),
                }
                parts.join(" and ")
            }
            Whom::Group(group) => group.title(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Whom {
    Listed(Vec<Entry>),
    Group(Group),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Entry {
    /// `uid` is the identity as the app keys it (`client::uid_key`); `name` was their name
    /// when added, shown while they are away.
    Client { uid: String, name: String },
    /// The channel's names from the top, separated by `/`.
    Channel {
        path: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        subchannels: bool,
    },
}

impl Entry {
    pub fn title(&self) -> &str {
        match self {
            Self::Client { name, .. } => name,
            Self::Channel { path, .. } => path.rsplit('/').next().unwrap_or(path),
        }
    }
}

/// A channel path as people read it: `›` between names, and a spacer by its text alone
/// (`[cspacer42]Police` reads `Police`).
pub fn readable_path(path: &str) -> String {
    path.split('/')
        .map(|name| {
            name.strip_prefix('[')
                .and_then(|rest| rest.split_once(']'))
                .filter(|(tag, text)| {
                    tag.to_ascii_lowercase().contains("spacer") && !text.trim().is_empty()
                })
                .map_or(name, |(_, text)| text)
                .trim()
        })
        .collect::<Vec<_>>()
        .join(" › ")
}

/// A whisper to a group of people, as the server works it out.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Group {
    pub kind: GroupKind,
    /// The server or channel group; unused for the other kinds.
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub scope: Scope,
}

impl Group {
    pub fn title(&self) -> String {
        let who = match self.kind {
            GroupKind::ServerGroup => format!("Server group {}", self.id),
            GroupKind::ChannelGroup => format!("Channel group {}", self.id),
            GroupKind::ChannelCommanders => "Channel commanders".into(),
            GroupKind::Everyone => "Everyone".into(),
        };
        match self.scope {
            Scope::Everywhere => who,
            scope => format!("{who} · {}", scope.title().to_lowercase()),
        }
    }
}

/// TeamSpeak's group whisper types, in their wire order.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GroupKind {
    ServerGroup = 0,
    ChannelGroup = 1,
    ChannelCommanders = 2,
    Everyone = 3,
}

impl GroupKind {
    pub const ALL: [Self; 4] = [
        Self::ServerGroup,
        Self::ChannelGroup,
        Self::ChannelCommanders,
        Self::Everyone,
    ];

    pub fn from_wire(value: i64) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| *kind as i64 == value)
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::ServerGroup => "Server group",
            Self::ChannelGroup => "Channel group",
            Self::ChannelCommanders => "Channel commanders",
            Self::Everyone => "Everyone",
        }
    }

    pub fn has_id(self) -> bool {
        matches!(self, Self::ServerGroup | Self::ChannelGroup)
    }
}

/// Where a group whisper reaches, seen from your channel; TeamSpeak's wire order.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    #[default]
    Everywhere = 0,
    MyChannel = 1,
    ParentChannel = 2,
    AllParentChannels = 3,
    MyChannelFamily = 4,
    ParentChannelFamily = 5,
    Subchannels = 6,
}

impl Scope {
    pub const ALL: [Self; 7] = [
        Self::Everywhere,
        Self::MyChannel,
        Self::ParentChannel,
        Self::AllParentChannels,
        Self::MyChannelFamily,
        Self::ParentChannelFamily,
        Self::Subchannels,
    ];

    pub fn from_wire(value: i64) -> Option<Self> {
        Self::ALL.into_iter().find(|scope| *scope as i64 == value)
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Everywhere => "Everywhere",
            Self::MyChannel => "My channel",
            Self::ParentChannel => "Parent channel",
            Self::AllParentChannels => "All parent channels",
            Self::MyChannelFamily => "My channel and subchannels",
            Self::ParentChannelFamily => "Parent channel and its subchannels",
            Self::Subchannels => "Subchannels",
        }
    }
}

/// A channel as `resolve` needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelNode {
    pub id: u64,
    pub parent: u64,
    pub name: String,
}

/// Someone on the server as `resolve` needs them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientNode {
    pub id: u16,
    pub uid: String,
    pub channel: u64,
}

/// What one whisper packet is addressed to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    Listed {
        channels: Vec<u64>,
        clients: Vec<u16>,
    },
    Group {
        kind: u8,
        scope: u8,
        id: u64,
    },
}

impl Resolved {
    pub fn is_empty(&self) -> bool {
        matches!(self, Self::Listed { channels, clients } if channels.is_empty() && clients.is_empty())
    }
}

/// A channel's names from the top, separated by `/`.
pub fn channel_path(channels: &[ChannelNode], id: u64) -> Option<String> {
    let mut names = Vec::new();
    let mut at = id;
    // A loop in a broken tree ends at its length.
    for _ in 0..=channels.len() {
        if at == 0 {
            names.reverse();
            return Some(names.join("/"));
        }
        let channel = channels.iter().find(|channel| channel.id == at)?;
        names.push(channel.name.as_str());
        at = channel.parent;
    }
    None
}

/// The channel at `path`. Names are compared as written; if none matches, ignoring case.
pub fn find_channel(channels: &[ChannelNode], path: &str) -> Option<u64> {
    let find = |exact: bool| {
        let mut parent = 0;
        for name in path.split('/') {
            parent = channels
                .iter()
                .find(|channel| {
                    channel.parent == parent
                        && if exact {
                            channel.name == name
                        } else {
                            channel.name.trim().eq_ignore_ascii_case(name.trim())
                        }
                })?
                .id;
        }
        (parent != 0).then_some(parent)
    };
    find(true).or_else(|| find(false))
}

fn descendants(channels: &[ChannelNode], id: u64, out: &mut Vec<u64>) {
    for child in channels.iter().filter(|channel| channel.parent == id) {
        if !out.contains(&child.id) {
            out.push(child.id);
            descendants(channels, child.id, out);
        }
    }
}

/// The IDs `list` whispers to on this server, leaving out yourself (`own`) and whoever isn't
/// there. People in a channel the list names are reached through the channel.
pub fn resolve(
    list: &List,
    channels: &[ChannelNode],
    clients: &[ClientNode],
    own: u16,
) -> Resolved {
    match &list.to {
        Whom::Group(group) => Resolved::Group {
            kind: group.kind as u8,
            scope: group.scope as u8,
            id: if group.kind.has_id() { group.id } else { 0 },
        },
        Whom::Listed(entries) => {
            let mut ids = Vec::new();
            let mut people = Vec::new();
            for entry in entries {
                match entry {
                    Entry::Channel { path, subchannels } => {
                        if let Some(id) = find_channel(channels, path) {
                            if !ids.contains(&id) {
                                ids.push(id);
                            }
                            if *subchannels {
                                descendants(channels, id, &mut ids);
                            }
                        }
                    }
                    Entry::Client { uid, .. } => {
                        for client in clients.iter().filter(|client| {
                            client.id != own && !uid.is_empty() && client.uid == *uid
                        }) {
                            if !people.contains(&client.id) && !ids.contains(&client.channel) {
                                people.push(client.id);
                            }
                        }
                    }
                }
            }
            people.retain(|id| {
                clients
                    .iter()
                    .find(|client| client.id == *id)
                    .is_none_or(|client| !ids.contains(&client.channel))
            });
            Resolved::Listed {
                channels: ids,
                clients: people,
            }
        }
    }
}

/// How many of a list's entries are on this server right now, of how many.
pub fn present(list: &List, channels: &[ChannelNode], clients: &[ClientNode]) -> (usize, usize) {
    match &list.to {
        Whom::Group(_) => (1, 1),
        Whom::Listed(entries) => {
            let here = entries
                .iter()
                .filter(|entry| match entry {
                    Entry::Channel { path, .. } => find_channel(channels, path).is_some(),
                    Entry::Client { uid, .. } => clients.iter().any(|client| client.uid == *uid),
                })
                .count();
            (here, entries.len())
        }
    }
}

/// What a server whispers to while a whisper key is down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    List(List),
    /// Whoever whispered to you last.
    Reply,
}

/// What the app tells the servers and the audio thread about whispering without waiting for a
/// message: a key held keeps the microphone open in push-to-talk too.
#[derive(Debug)]
pub struct Shared {
    pub held: std::sync::atomic::AtomicBool,
    pub accept: std::sync::atomic::AtomicBool,
    pub duck: std::sync::atomic::AtomicBool,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            held: false.into(),
            accept: true.into(),
            duck: false.into(),
        }
    }
}

impl Shared {
    pub fn held(&self) -> bool {
        self.held.load(std::sync::atomic::Ordering::Acquire)
    }

    pub fn configure(&self, settings: &Settings) {
        use std::sync::atomic::Ordering::Release;
        self.accept.store(settings.accept, Release);
        self.duck.store(settings.duck, Release);
    }
}

/// One encoded voice frame for the channel (`None`), or as a whisper.
pub fn voice_packet(
    target: Option<&Resolved>,
    codec: tsproto_packets::packets::CodecType,
    data: &[u8],
) -> tsproto_packets::packets::OutPacket {
    use tsproto_packets::packets::{AudioData, OutAudio};
    match target {
        None => OutAudio::new(&AudioData::C2S { id: 0, codec, data }),
        Some(Resolved::Listed { channels, clients }) => OutAudio::new(&AudioData::C2SWhisper {
            id: 0,
            codec,
            channels: channels.clone(),
            clients: clients.clone(),
            data,
        }),
        Some(Resolved::Group { kind, scope, id }) => OutAudio::new(&AudioData::C2SWhisperNew {
            id: 0,
            codec,
            whisper_type: *kind,
            target: *scope,
            target_id: *id,
            data,
        }),
    }
}

/// A new list's ID, unlike any in `taken`.
pub fn new_id(taken: &[List]) -> String {
    let mut bytes = [0u8; 16];
    let _ = getrandom::fill(&mut bytes);
    let mut id: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    while taken.iter().any(|list| list.id == id) {
        id.push('0');
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> Vec<ChannelNode> {
        let node = |id, parent, name: &str| ChannelNode {
            id,
            parent,
            name: name.into(),
        };
        vec![
            node(1, 0, "Lobby"),
            node(2, 0, "[cspacer42]Polizeistreifen - ALTIS"),
            node(3, 2, "Leitstelle - Altis"),
            node(4, 3, "Funk 1"),
            node(5, 0, "[cspacer46]THW Einsatz"),
            node(6, 5, "Einsatzleitung - ALTIS"),
        ]
    }

    fn client(id: u16, uid: &str, channel: u64) -> ClientNode {
        ClientNode {
            id,
            uid: uid.into(),
            channel,
        }
    }

    fn listed(entries: Vec<Entry>) -> List {
        List {
            to: Whom::Listed(entries),
            ..List::new("id".into(), "List".into())
        }
    }

    #[test]
    fn channel_paths_go_both_ways_and_fall_back_to_any_case() {
        let channels = tree();
        assert_eq!(
            channel_path(&channels, 3).as_deref(),
            Some("[cspacer42]Polizeistreifen - ALTIS/Leitstelle - Altis")
        );
        assert_eq!(
            find_channel(
                &channels,
                "[cspacer42]Polizeistreifen - ALTIS/Leitstelle - Altis"
            ),
            Some(3)
        );
        assert_eq!(find_channel(&channels, "lobby"), Some(1));
        assert_eq!(find_channel(&channels, "Leitstelle - Altis"), None);
        assert_eq!(find_channel(&channels, "Lobby/Missing"), None);
        assert_eq!(
            readable_path("[cspacer42]Polizeistreifen - ALTIS/Leitstelle - Altis"),
            "Polizeistreifen - ALTIS › Leitstelle - Altis"
        );
        assert_eq!(readable_path("[spacer0]/A"), "[spacer0] › A");
    }

    #[test]
    fn listed_whispers_skip_yourself_the_absent_and_people_already_reached() {
        let list = listed(vec![
            Entry::Channel {
                path: "[cspacer42]Polizeistreifen - ALTIS/Leitstelle - Altis".into(),
                subchannels: true,
            },
            Entry::Channel {
                path: "Gone".into(),
                subchannels: false,
            },
            Entry::Client {
                uid: "aa".into(),
                name: "Mara".into(),
            },
            Entry::Client {
                uid: "bb".into(),
                name: "In Funk".into(),
            },
            Entry::Client {
                uid: "me".into(),
                name: "Me".into(),
            },
            Entry::Client {
                uid: "cc".into(),
                name: "Away".into(),
            },
        ]);
        let clients = [
            client(10, "aa", 1),
            client(11, "bb", 4),
            client(12, "me", 1),
        ];
        assert_eq!(
            resolve(&list, &tree(), &clients, 12),
            Resolved::Listed {
                channels: vec![3, 4],
                clients: vec![10],
            }
        );
        assert_eq!(present(&list, &tree(), &clients), (4, 6));
    }

    #[test]
    fn whisper_packets_carry_their_targets() {
        use tsproto_packets::packets::{CodecType, PacketType};
        let channel = voice_packet(None, CodecType::OpusVoice, &[1, 2]);
        assert_eq!(channel.header().packet_type(), PacketType::Voice);
        let listed = voice_packet(
            Some(&Resolved::Listed {
                channels: vec![7],
                clients: vec![3, 4],
            }),
            CodecType::OpusVoice,
            &[1, 2],
        );
        assert_eq!(listed.header().packet_type(), PacketType::VoiceWhisper);
        // Codec, one channel, two clients, the channel's 8 bytes, the clients' 2 each, the voice.
        assert_eq!(
            &listed.content()[2..],
            &[4, 1, 2, 0, 0, 0, 0, 0, 0, 0, 7, 0, 3, 0, 4, 1, 2]
        );
        let group = voice_packet(
            Some(&Resolved::Group {
                kind: 0,
                scope: 1,
                id: 12,
            }),
            CodecType::OpusVoice,
            &[9],
        );
        assert_eq!(group.header().packet_type(), PacketType::VoiceWhisper);
        assert_eq!(
            &group.content()[2..],
            &[4, 0, 1, 0, 0, 0, 0, 0, 0, 0, 12, 9]
        );
    }

    #[test]
    fn groups_carry_their_wire_values() {
        let list = List {
            to: Whom::Group(Group {
                kind: GroupKind::ChannelCommanders,
                id: 9,
                scope: Scope::MyChannelFamily,
            }),
            ..List::new("id".into(), "Commanders".into())
        };
        assert_eq!(
            resolve(&list, &tree(), &[], 1),
            Resolved::Group {
                kind: 2,
                scope: 4,
                id: 0
            }
        );
    }

    #[test]
    fn settings_round_trip_and_stay_out_of_the_file_when_default() {
        assert_eq!(serde_json::to_string(&Settings::default()).unwrap(), "{}");
        let settings = Settings {
            lists: vec![List {
                server: "ts.example.com".into(),
                key: "Ctrl+Shift+1".into(),
                ..listed(vec![Entry::Client {
                    uid: "aa".into(),
                    name: "Mara".into(),
                }])
            }],
            reply_key: "Ctrl+Shift+R".into(),
            accept: false,
            duck: true,
        };
        let json = serde_json::to_string(&settings).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), settings);
        assert!(settings.lists[0].for_server("TS.example.com "));
        assert!(!settings.lists[0].for_server("other"));
    }

    #[test]
    fn summaries_name_a_few_people_and_count_the_rest() {
        let client = |name: &str| Entry::Client {
            uid: name.into(),
            name: name.into(),
        };
        let channel = Entry::Channel {
            path: "A".into(),
            subchannels: false,
        };
        assert_eq!(listed(vec![]).summary(), "Nobody yet");
        assert_eq!(
            listed(vec![client("Mara"), client("Theo"), channel.clone()]).summary(),
            "Mara, Theo and 1 channel"
        );
        assert_eq!(
            listed(vec![client("A"), client("B"), client("C"), client("D")]).summary(),
            "4 people"
        );
        assert_eq!(
            listed(vec![channel.clone(), channel]).summary(),
            "2 channels"
        );
    }
}
