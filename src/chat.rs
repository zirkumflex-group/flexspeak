//! Text chat on one server: channel, server and private messages, and pokes, as one history.
//!
//! TeamSpeak sends text to one of three places. Both layouts read the same history: Tabs shows
//! one conversation at a time, One Feed shows them all with each line tagged by where it went.
//! Nothing is saved; a server keeps no history either, so it lasts as long as the connection.

use std::collections::VecDeque;
use std::ops::Range;
use std::time::{Duration, Instant};

use chrono::{Local, NaiveDate, TimeZone};
use serde::{Deserialize, Serialize};

/// Lines held per server while the app runs; the oldest go first. The history on disk keeps
/// more (`chat_store`).
pub const LIMIT: usize = 2000;
/// The longest message a TeamSpeak server takes, in bytes.
pub const MAX_BYTES: usize = 1024;
/// How long a sent message waits for the server to send it back before it is shown anyway.
/// TeamSpeak servers return your own lines, and those are what the history shows; this only
/// covers one that doesn't.
const ECHO_WAIT: Duration = Duration::from_secs(2);
/// A late copy of a line already shown that way is recognised for this long.
const ECHO_LATE: Duration = Duration::from_secs(60);
/// Longer incoming text is cut here.
const MAX_RECEIVED: usize = 4096;

/// How the chat column lays conversations out (Settings › Chat).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Layout {
    /// One tab per conversation.
    #[default]
    Tabs,
    /// Everything in one stream.
    Feed,
}

/// How Tabs fits many conversations into its bar (Settings › Chat).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TabBar {
    /// Every conversation is a tab; the bar scrolls sideways.
    #[default]
    Scroll,
    /// One tab per kind: channels, the server, people. Pointing at one lists its conversations.
    Grouped,
}

impl Layout {
    pub const ALL: [Self; 2] = [Self::Tabs, Self::Feed];

    pub fn title(self) -> &'static str {
        match self {
            Self::Tabs => "Tabs",
            Self::Feed => "One Feed",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            Self::Tabs => "One tab each for your channel, the server and every person you message.",
            Self::Feed => "Everything in one stream, tagged by where it was sent.",
        }
    }
}

/// Where a message goes. A private one is keyed by the other person's identity, so the
/// conversation survives them reconnecting under a new client number.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub enum Scope {
    /// The channel you are in, wherever that is.
    #[default]
    Channel,
    Server,
    Private(String),
    /// What was said in a channel you have since left, by its number. To read, not to write to.
    Past(u64),
}

impl Scope {
    pub fn is_private(&self) -> bool {
        matches!(self, Self::Private(_))
    }

    pub fn is_channel(&self) -> bool {
        matches!(self, Self::Channel | Self::Past(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Message,
    /// A poke, kept as history: drawn faded, never a chat line.
    Poke,
}

/// What One Feed is narrowed to.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub enum Filter {
    #[default]
    All,
    /// Every channel, the ones you left included.
    Channel,
    Server,
    /// Every private conversation.
    Private,
    /// One conversation: the channel you are in, one you left, or one person.
    Only(Scope),
}

impl Filter {
    pub const ALL: [Self; 4] = [Self::All, Self::Channel, Self::Server, Self::Private];

    /// Whether a conversation's lines pass. `Scope::Channel` is the channel you are in, so a
    /// feed narrowed to one you left shows nothing that arrives.
    pub fn shows(&self, scope: &Scope) -> bool {
        match self {
            Self::All => true,
            Self::Channel => scope.is_channel(),
            Self::Server => *scope == Scope::Server,
            Self::Private => scope.is_private(),
            Self::Only(only) => only == scope,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Entry {
    pub id: u64,
    pub scope: Scope,
    pub kind: Kind,
    /// You wrote it, or you sent the poke.
    pub own: bool,
    /// Who wrote it.
    pub from: String,
    /// A channel message: the channel's name then. A private one: the other person's name.
    pub place: String,
    /// The channel a channel message was said in; 0 for the others.
    pub room: u64,
    pub text: String,
    /// Local time, as `21:04`.
    pub time: String,
    /// The local day it was said on, for the lines between days.
    pub day: NaiveDate,
    /// Seconds since 1970, UTC.
    pub at: i64,
}

/// A message as the connection reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Incoming {
    pub scope: Scope,
    pub own: bool,
    pub from: String,
    /// The other person's name, for a private message.
    pub peer: String,
    pub text: String,
}

/// Someone you have a private conversation with.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Peer {
    pub key: String,
    pub name: String,
}

/// A channel something was said in while you were there.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Room {
    pub id: u64,
    pub name: String,
}

/// What the chat column asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Draft(String),
    Send,
    /// Shows a conversation (Tabs) and points the message box at it.
    Target(Scope),
    Filter(Filter),
    /// Starts or shows the private conversation with a client.
    Message(u16),
    /// Starts or shows the private conversation with whoever wrote a line, by its id.
    MessageFrom(u64),
    /// Forgets a private conversation's tab; its lines come back if it is opened again.
    ClosePeer(String),
    /// Forgets a left channel's tab.
    ClosePast(u64),
    /// Shows a private conversation: one with something unread, else the latest.
    People,
    /// Tabs: shows every conversation together, as the feed does.
    Everything,
    /// Points the message box somewhere without changing what is shown.
    SendTo(Scope),
    /// One Feed: narrows the stream to one conversation.
    Narrow(Scope),
    /// Forgets every server's saved conversations. Asked twice before it happens.
    ClearHistory,
    /// Hides the column behind the window's edge, or docks it again.
    Toggle,
    /// Docks the column on this conversation.
    Open(Scope),
    /// Peeks the column on this conversation with the message box ready, from a pop-up.
    Reply(Scope),
    DismissToast,
    Layout(Layout),
    Options(crate::settings::Chat),
}

#[derive(Debug, Default)]
pub struct Chat {
    entries: VecDeque<Entry>,
    next: u64,
    /// Bumped whenever the lines change, so views can skip rebuilding them.
    pub version: u64,
    /// People with a private conversation, oldest first.
    pub peers: Vec<Peer>,
    /// The channel you are in.
    pub here: u64,
    /// Channels with lines in the history, oldest first, the current one among them.
    rooms: Vec<Room>,
    /// Where the message box sends, and in Tabs the conversation shown.
    pub target: Scope,
    pub filter: Filter,
    /// Tabs is on its All tab: every conversation together, tagged as the feed tags them.
    pub everything: bool,
    pub draft: String,
    /// The last paste didn't fit and was cut at `MAX_BYTES`; cleared by the next edit.
    pub draft_cut: bool,
    unread: Vec<(Scope, u32)>,
    /// Sent and not yet seen coming back, oldest first.
    sent: Vec<Sent>,
    /// Shown without waiting any longer; their late copies are dropped.
    assumed: Vec<Sent>,
    /// Lines up to this one are in the history on disk, or came from it.
    saved: u64,
    /// The history on disk has been brought back, so a reconnect doesn't bring it twice.
    restored: bool,
}

/// A message on its way to the server.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Sent {
    scope: Scope,
    /// As the history would show it.
    text: String,
    /// As it was typed, to put back in the box if the server refuses it.
    draft: String,
    at: Instant,
}

impl Chat {
    pub fn entries(&self) -> impl DoubleEndedIterator<Item = &Entry> {
        self.entries.iter()
    }

    #[cfg(test)]
    fn is_empty_for_tests(&self) -> bool {
        self.entries.is_empty()
    }

    /// The lines a layout shows, oldest first.
    pub fn shown(&self, layout: Layout) -> Vec<&Entry> {
        self.entries
            .iter()
            .filter(|entry| {
                let one = match layout {
                    Layout::Tabs if self.everything => return true,
                    Layout::Tabs => &self.target,
                    Layout::Feed => match &self.filter {
                        Filter::Only(one) => one,
                        filter => return filter.shows(&entry.scope),
                    },
                };
                match one {
                    // The channel you're in; one you left has lines of its own.
                    Scope::Channel => entry.scope == Scope::Channel && entry.room == self.here,
                    Scope::Past(room) => entry.scope == Scope::Channel && entry.room == *room,
                    one => entry.scope == *one,
                }
            })
            .collect()
    }

    /// Whether lines are drawn as the feed draws them: tagged by where they went.
    pub fn tagged(&self, layout: Layout) -> bool {
        layout == Layout::Feed || self.everything
    }

    /// You are in this channel now. Its earlier lines, if you were here before, are the
    /// channel tab's again.
    pub fn enter(&mut self, id: u64, name: &str) {
        if let Some(room) = self.rooms.iter_mut().find(|room| room.id == id)
            && room.name != name
            && !name.is_empty()
        {
            room.name = name.to_owned();
            self.version += 1;
        }
        if self.here == id {
            return;
        }
        self.here = id;
        if self.target == Scope::Past(id) {
            self.target = Scope::Channel;
        }
        if self.filter == Filter::Only(Scope::Past(id)) {
            self.filter = Filter::Only(Scope::Channel);
        }
        self.version += 1;
    }

    /// The channels you have left that still have something to read, oldest first.
    pub fn past(&self) -> impl Iterator<Item = &Room> {
        self.rooms.iter().filter(|room| room.id != self.here)
    }

    pub fn close_past(&mut self, id: u64) {
        self.rooms
            .retain(|room| room.id != id || room.id == self.here);
        if self.target == Scope::Past(id) {
            self.target = Scope::Channel;
        }
        if self.filter == Filter::Only(Scope::Past(id)) {
            self.filter = Filter::Channel;
        }
        self.version += 1;
    }

    /// Whether a layout has this conversation on screen while the column is showing.
    pub fn shows(&self, layout: Layout, scope: &Scope) -> bool {
        match layout {
            Layout::Tabs => self.everything || *scope == self.target,
            Layout::Feed => self.filter.shows(scope),
        }
    }

    /// Notes a message handed to the connection, so its return can be told from a new line.
    pub fn sent(&mut self, scope: Scope, text: &str, now: Instant) {
        if self.sent.len() >= 32 {
            self.sent.remove(0);
        }
        self.sent.push(Sent {
            scope,
            text: plain(text),
            draft: self.draft.trim().to_owned(),
            at: now,
        });
    }

    /// The server refused a message: the oldest one waiting isn't coming back. Its text goes
    /// back in the box unless something new is being written there.
    pub fn refused(&mut self) {
        if self.sent.is_empty() {
            return;
        }
        let lost = self.sent.remove(0);
        if self.draft.trim().is_empty() {
            self.draft = lost.draft;
        }
    }

    /// Shows the messages a server never sent back, under `own` as the name. True when it
    /// added any.
    pub fn overdue(&mut self, now: Instant, own: &str, channel: &str) -> bool {
        self.assumed
            .retain(|sent| now.saturating_duration_since(sent.at) < ECHO_LATE);
        let waited = |sent: &Sent| now.saturating_duration_since(sent.at) >= ECHO_WAIT;
        if !self.sent.iter().any(waited) {
            return false;
        }
        let (due, waiting) = std::mem::take(&mut self.sent).into_iter().partition(waited);
        self.sent = waiting;
        for sent in due {
            let place = match &sent.scope {
                Scope::Channel => channel.to_owned(),
                Scope::Server | Scope::Past(_) => String::new(),
                Scope::Private(key) => self
                    .peer(key)
                    .map_or_else(String::new, |peer| peer.name.clone()),
            };
            self.push(
                sent.scope.clone(),
                Kind::Message,
                true,
                own.to_owned(),
                place,
                &sent.text,
            );
            self.assumed.push(Sent { at: now, ..sent });
        }
        true
    }

    /// Adds a received message and returns it; `None` for the late copy of a line of yours
    /// that is already shown.
    pub fn receive(&mut self, message: Incoming, channel: &str) -> Option<&Entry> {
        if message.own {
            let text = plain(&message.text);
            let same = |sent: &Sent| sent.scope == message.scope && sent.text == text;
            if let Some(index) = self.sent.iter().position(same) {
                self.sent.remove(index);
            } else if let Some(index) = self.assumed.iter().position(same) {
                self.assumed.remove(index);
                return None;
            }
        }
        let place = match &message.scope {
            Scope::Channel => channel.to_owned(),
            Scope::Server | Scope::Past(_) => String::new(),
            Scope::Private(_) => message.peer,
        };
        Some(self.push(
            message.scope,
            Kind::Message,
            message.own,
            message.from,
            place,
            &message.text,
        ))
    }

    /// Records a poke with `peer`: one they sent you, or one you sent them when `own`.
    pub fn poke(&mut self, key: String, peer: &str, own: bool, text: &str) -> &Entry {
        let from = if own { "You" } else { peer };
        self.push(
            Scope::Private(key),
            Kind::Poke,
            own,
            from.to_owned(),
            peer.to_owned(),
            text,
        )
    }

    fn push(
        &mut self,
        scope: Scope,
        kind: Kind,
        own: bool,
        from: String,
        place: String,
        text: &str,
    ) -> &Entry {
        let time = Local::now().format("%H:%M").to_string();
        self.add(scope, kind, own, from, place, text, time)
    }

    /// Adds a line stamped `time`; `receive` and `poke` stamp the time it is now.
    #[allow(clippy::too_many_arguments)]
    pub fn add(
        &mut self,
        scope: Scope,
        kind: Kind,
        own: bool,
        from: String,
        place: String,
        text: &str,
        time: String,
    ) -> &Entry {
        if let Scope::Private(key) = &scope {
            self.remember(key, &place);
        }
        // A channel line belongs to the channel you're in as it arrives.
        let room = if scope == Scope::Channel {
            self.here
        } else {
            0
        };
        if scope == Scope::Channel && !self.rooms.iter().any(|known| known.id == room) {
            self.rooms.push(Room {
                id: room,
                name: place.clone(),
            });
        }
        let now = Local::now();
        self.entries.push_back(Entry {
            id: self.next,
            scope,
            kind,
            own,
            from,
            place,
            room,
            text: plain(text),
            time,
            day: now.date_naive(),
            at: now.timestamp(),
        });
        self.next += 1;
        self.version += 1;
        if self.entries.len() > LIMIT {
            self.entries.pop_front();
        }
        self.entries.back().expect("just pushed")
    }

    pub fn has_unsaved(&self) -> bool {
        self.saved < self.next
    }

    /// The lines not yet in the history on disk, each handed out once.
    pub fn unsaved(&mut self) -> Vec<Entry> {
        let from = std::mem::replace(&mut self.saved, self.next);
        self.entries
            .iter()
            .filter(|entry| entry.id >= from)
            .cloned()
            .collect()
    }

    /// Whether the history on disk still has to be brought back for this connection.
    pub fn needs_restoring(&self) -> bool {
        !self.restored
    }

    /// Brings back what the history on disk holds, ahead of anything said since connecting.
    /// A conversation whose tab was closed keeps its lines but gets no tab.
    pub fn restore(&mut self, history: crate::chat_store::Loaded) {
        if std::mem::replace(&mut self.restored, true) {
            return;
        }
        let fresh: Vec<Entry> = self.entries.drain(..).collect();
        for line in history.lines {
            let tab = match &line.scope {
                Scope::Channel => Scope::Past(line.room),
                scope => scope.clone(),
            };
            if !history.closed.contains(&tab) {
                match &line.scope {
                    Scope::Private(key) => self.remember(key, &line.place),
                    Scope::Channel if !self.rooms.iter().any(|room| room.id == line.room) => {
                        self.rooms.push(Room {
                            id: line.room,
                            name: line.place.clone(),
                        });
                    }
                    _ => {}
                }
            }
            let when = Local
                .timestamp_opt(line.at, 0)
                .single()
                .unwrap_or_else(Local::now);
            self.entries.push_back(Entry {
                id: self.next,
                scope: line.scope,
                kind: line.kind,
                own: line.own,
                from: line.from,
                place: line.place,
                room: line.room,
                text: line.text,
                time: when.format("%H:%M").to_string(),
                day: when.date_naive(),
                at: line.at,
            });
            self.next += 1;
        }
        // What was said since connecting keeps its place after the history, still unsaved.
        self.saved = self.next;
        for mut entry in fresh {
            entry.id = self.next;
            self.next += 1;
            self.entries.push_back(entry);
        }
        while self.entries.len() > LIMIT {
            self.entries.pop_front();
        }
        self.version += 1;
    }

    /// Forgets every line and conversation, as clearing the history does.
    pub fn clear(&mut self) {
        let (here, restored) = (self.here, self.restored);
        *self = Self {
            here,
            restored,
            version: self.version + 1,
            next: self.next,
            saved: self.next,
            ..Self::default()
        };
    }

    /// Keeps a tab for this person, under the name they go by now.
    fn remember(&mut self, key: &str, name: &str) {
        match self.peers.iter_mut().find(|peer| peer.key == key) {
            Some(peer) if !name.is_empty() && peer.name != name => {
                peer.name = name.to_owned();
                self.version += 1;
            }
            Some(_) => {}
            None => {
                self.peers.push(Peer {
                    key: key.to_owned(),
                    name: name.to_owned(),
                });
                self.version += 1;
            }
        }
    }

    pub fn peer(&self, key: &str) -> Option<&Peer> {
        self.peers.iter().find(|peer| peer.key == key)
    }

    /// Shows a conversation and points the message box at it. A private one gets its tab.
    pub fn open(&mut self, scope: Scope, name: &str) {
        if let Scope::Private(key) = &scope {
            self.remember(key, name);
        }
        // A narrowed feed widens rather than hide the conversation just asked for.
        if !self.filter.shows(&scope) {
            self.filter = Filter::All;
        }
        self.target = scope;
        self.everything = false;
    }

    /// Points the message box at a conversation, leaving what is shown as it is.
    pub fn aim(&mut self, scope: Scope, name: &str) {
        if let Scope::Private(key) = &scope {
            self.remember(key, name);
        }
        self.target = scope;
    }

    /// One Feed shows this conversation alone, and writes to it where it can be written to.
    pub fn narrow(&mut self, scope: Scope) {
        if matches!(scope, Scope::Channel | Scope::Private(_)) {
            self.target = scope.clone();
        }
        self.filter = Filter::Only(scope);
    }

    pub fn close_peer(&mut self, key: &str) {
        self.peers.retain(|peer| peer.key != key);
        self.unread
            .retain(|(scope, _)| *scope != Scope::Private(key.to_owned()));
        if self.target == Scope::Private(key.to_owned()) {
            self.target = Scope::Channel;
        }
        if self.filter == Filter::Only(Scope::Private(key.to_owned())) {
            self.filter = Filter::Private;
        }
        self.version += 1;
    }

    pub fn unread(&self, scope: &Scope) -> u32 {
        self.unread
            .iter()
            .find(|(other, _)| other == scope)
            .map_or(0, |(_, count)| *count)
    }

    pub fn unread_private(&self) -> u32 {
        self.unread
            .iter()
            .filter(|(scope, _)| scope.is_private())
            .map(|(_, count)| count)
            .sum()
    }

    pub fn unread_public(&self) -> u32 {
        self.unread
            .iter()
            .filter(|(scope, _)| !scope.is_private())
            .map(|(_, count)| count)
            .sum()
    }

    pub fn mark_unread(&mut self, scope: &Scope) {
        match self.unread.iter_mut().find(|(other, _)| other == scope) {
            Some((_, count)) => *count = count.saturating_add(1),
            None => self.unread.push((scope.clone(), 1)),
        }
    }

    /// Unread lines among what `layout` has on screen: those that came while you read back.
    pub fn unread_shown(&self, layout: Layout) -> u32 {
        self.unread
            .iter()
            .filter(|(scope, _)| match layout {
                Layout::Tabs => self.everything || *scope == self.target,
                Layout::Feed => self.filter.shows(scope),
            })
            .map(|(_, count)| count)
            .sum()
    }

    /// Clears what `layout` has on screen. True when that changed a count.
    pub fn mark_seen(&mut self, layout: Layout) -> bool {
        let before = self.unread.len();
        let (target, filter, everything) =
            (self.target.clone(), self.filter.clone(), self.everything);
        self.unread.retain(|(scope, _)| match layout {
            Layout::Tabs => !everything && *scope != target,
            Layout::Feed => !filter.shows(scope),
        });
        self.unread.len() != before
    }
}

/// Text as people read it: BBCode tags removed, their content kept, TeamSpeak's own and the
/// common forum ones other clients send (`[quote]`, `[code]`, `[table]` …). A link written
/// `[URL=address]label[/URL]` keeps both when they differ. Control characters other than line
/// breaks are dropped.
pub fn plain(text: &str) -> String {
    const TAGS: [&str; 26] = [
        "b", "i", "u", "s", "url", "img", "color", "size", "center", "left", "right", "list", "*",
        "hr", "table", "tr", "th", "td", "quote", "code", "font", "ul", "ol", "li", "sub", "sup",
    ];
    let mut out = String::with_capacity(text.len().min(MAX_RECEIVED));
    // The address of an open `[URL=…]`, added after its label.
    let mut address: Option<(String, usize)> = None;
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find(']') else {
            out.push('[');
            rest = after;
            continue;
        };
        let inner = &after[..close];
        let (closing, body) = match inner.strip_prefix('/') {
            Some(body) => (true, body),
            None => (false, inner),
        };
        let (name, value) = body.split_once('=').unwrap_or((body, ""));
        if !TAGS.contains(&name.to_ascii_lowercase().as_str()) || (closing && !value.is_empty()) {
            out.push('[');
            rest = after;
            continue;
        }
        if name.eq_ignore_ascii_case("url") {
            if closing {
                if let Some((url, start)) = address.take()
                    && out[start..].trim() != url
                {
                    let label_empty = out[start..].trim().is_empty();
                    if !label_empty {
                        out.push_str(" (");
                    }
                    out.push_str(&url);
                    if !label_empty {
                        out.push(')');
                    }
                }
            } else if !value.is_empty() {
                address = Some((value.trim_matches('"').to_owned(), out.len()));
            }
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    out.chars()
        .filter(|c| *c == '\n' || !c.is_control())
        .take(MAX_RECEIVED)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// Where the web addresses in `text` are, as byte ranges: those that start `http://` or
/// `https://`, and bare ones that start `www.`.
pub fn links(text: &str) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = ["https://", "http://", "www."]
        .iter()
        .filter_map(|scheme| text[from..].find(scheme))
        .min()
    {
        let start = from + at;
        let bare = text[start..].starts_with("www.");
        // `www.` inside a word (`awww.`) or an address is no address of its own.
        if bare
            && text[..start]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || matches!(c, '.' | '/' | '@' | '-'))
        {
            from = start + 1;
            continue;
        }
        let mut end = text[start..]
            .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '[' | ']'))
            .map_or(text.len(), |length| start + length);
        // A full stop or bracket that ends the sentence isn't part of the address.
        while text[start..end].ends_with(['.', ',', ';', ':', '!', '?', ')', '\'']) {
            end -= 1;
        }
        let address = &text[start..end];
        let whole = if bare {
            // A name after `www.` with a dot in it: `www.example.org`.
            address["www.".len()..]
                .split_once('.')
                .is_some_and(|(name, rest)| !name.is_empty() && !rest.is_empty())
        } else {
            address.contains("://") && end - start > "https://".len()
        };
        if whole {
            found.push(start..end);
        }
        from = end.max(start + 1);
    }
    found
}

/// What a link found by `links` opens: a bare `www.` address over https.
pub fn address(link: &str) -> String {
    if link.starts_with("www.") {
        format!("https://{link}")
    } else {
        link.to_owned()
    }
}

/// A message as it is sent: trimmed, cut to what a server takes, and its web addresses wrapped
/// in `[URL]` so TeamSpeak's own clients show them as links. `None` when nothing is left.
pub fn outgoing(text: &str) -> Option<String> {
    let text: String = text
        .chars()
        .filter(|c| *c == '\n' || !c.is_control())
        .collect();
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(text.len() + 16);
    let mut at = 0;
    for link in links(text) {
        out.push_str(&text[at..link.start]);
        out.push_str("[URL]");
        out.push_str(&text[link.clone()]);
        out.push_str("[/URL]");
        at = link.end;
    }
    out.push_str(&text[at..]);
    // Too long with the tags: send it plain, cut on a character boundary.
    if out.len() > MAX_BYTES {
        let mut end = MAX_BYTES.min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        out = text[..end].to_owned();
    }
    Some(out)
}

/// A short conversation that shows what a layout looks like, for setup and Settings.
pub fn demo() -> Vec<Entry> {
    let mara = || Scope::Private("mara".into());
    let line = |id, scope, own, from: &str, place: &str, text: &str, time: &str| Entry {
        id,
        scope,
        kind: Kind::Message,
        own,
        from: from.to_owned(),
        place: place.to_owned(),
        room: 0,
        text: text.to_owned(),
        time: time.to_owned(),
        day: NaiveDate::default(),
        at: 0,
    };
    vec![
        line(
            0,
            Scope::Server,
            false,
            "Lukas",
            "",
            "Server restarts at 4 for the update",
            "20:58",
        ),
        line(
            1,
            Scope::Channel,
            false,
            "Theo",
            "Lobby",
            "back, had to restart my router",
            "21:04",
        ),
        line(
            2,
            Scope::Channel,
            false,
            "Mara",
            "Lobby",
            "we start the raid at half past",
            "21:05",
        ),
        line(
            3,
            mara(),
            false,
            "Mara",
            "Mara",
            "psst, can you bring the spare key?",
            "21:06",
        ),
        line(4, mara(), true, "Jan", "Mara", "yes, got it", "21:06"),
        line(
            5,
            Scope::Channel,
            false,
            "Theo",
            "Lobby",
            "can someone share the boss notes?",
            "21:07",
        ),
    ]
}

/// The first letter of a name, for its round tile.
pub fn initial(name: &str) -> String {
    name.trim()
        .chars()
        .find(|c| c.is_alphanumeric())
        .map_or_else(|| "?".to_owned(), |c| c.to_uppercase().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn incoming(scope: Scope, from: &str, text: &str) -> Incoming {
        Incoming {
            peer: from.to_owned(),
            scope,
            own: false,
            from: from.to_owned(),
            text: text.to_owned(),
        }
    }

    #[test]
    fn tabs_show_one_conversation_and_the_feed_shows_what_its_filter_allows() {
        let mut chat = Chat::default();
        chat.receive(incoming(Scope::Channel, "Theo", "back"), "Lobby");
        chat.receive(incoming(Scope::Server, "Lukas", "restart at 4"), "Lobby");
        chat.receive(
            incoming(Scope::Private("m".into()), "Mara", "psst"),
            "Lobby",
        );

        assert_eq!(chat.shown(Layout::Tabs).len(), 1);
        assert_eq!(chat.shown(Layout::Tabs)[0].place, "Lobby");
        assert_eq!(chat.shown(Layout::Feed).len(), 3);
        chat.filter = Filter::Private;
        assert_eq!(chat.shown(Layout::Feed)[0].from, "Mara");
        // A private message opens that person's tab.
        assert_eq!(chat.peers.len(), 1);
        chat.open(Scope::Private("m".into()), "Mara");
        assert_eq!(chat.shown(Layout::Tabs)[0].text, "psst");
    }

    #[test]
    fn unread_counts_clear_for_what_is_on_screen_only() {
        let mut chat = Chat::default();
        let mara = Scope::Private("m".into());
        chat.mark_unread(&Scope::Channel);
        chat.mark_unread(&mara);
        chat.mark_unread(&mara);
        assert_eq!((chat.unread_public(), chat.unread_private()), (1, 2));

        // Tabs is on the channel: Mara stays unread.
        assert!(chat.mark_seen(Layout::Tabs));
        assert_eq!((chat.unread(&Scope::Channel), chat.unread(&mara)), (0, 2));
        assert!(!chat.mark_seen(Layout::Tabs));
        // The feed shows everything.
        assert!(chat.mark_seen(Layout::Feed));
        assert_eq!(chat.unread_private(), 0);
    }

    #[test]
    fn pokes_join_the_private_conversation_both_ways() {
        let mut chat = Chat::default();
        chat.poke("m".into(), "Mara", false, "you there?");
        chat.poke("m".into(), "Mara", true, "");
        let lines: Vec<_> = chat.entries().collect();
        assert_eq!(lines[0].kind, Kind::Poke);
        assert_eq!((lines[0].from.as_str(), lines[0].own), ("Mara", false));
        assert_eq!((lines[1].from.as_str(), lines[1].own), ("You", true));
        assert_eq!(chat.peer("m").unwrap().name, "Mara");
    }

    #[test]
    fn closing_a_tab_returns_to_the_channel_and_keeps_the_lines() {
        let mut chat = Chat::default();
        chat.receive(incoming(Scope::Private("m".into()), "Mara", "hi"), "");
        chat.open(Scope::Private("m".into()), "Mara");
        chat.close_peer("m");
        assert_eq!(chat.target, Scope::Channel);
        assert!(chat.peers.is_empty());
        chat.open(Scope::Private("m".into()), "Mara");
        assert_eq!(chat.shown(Layout::Tabs).len(), 1);
    }

    #[test]
    fn a_sent_message_shows_once_whether_or_not_the_server_returns_it() {
        let base = Instant::now();
        let own = |text: &str| Incoming {
            own: true,
            ..incoming(Scope::Channel, "Jan", text)
        };
        // The usual case: it comes back and is shown then.
        let mut chat = Chat::default();
        chat.sent(Scope::Channel, "see [URL]https://example.org[/URL]", base);
        assert!(!chat.overdue(base + Duration::from_millis(500), "Jan", "Lobby"));
        assert!(
            chat.receive(own("see [URL]https://example.org[/URL]"), "Lobby")
                .is_some()
        );
        assert!(!chat.overdue(base + Duration::from_secs(5), "Jan", "Lobby"));
        assert_eq!(chat.entries().count(), 1);

        // A server that keeps it to itself: shown after the wait, and a late copy is dropped.
        let mut chat = Chat::default();
        chat.sent(Scope::Channel, "hello", base);
        assert!(chat.overdue(base + Duration::from_secs(3), "Jan", "Lobby"));
        let line = chat.entries().next().unwrap();
        assert_eq!(
            (line.from.as_str(), line.own, line.place.as_str()),
            ("Jan", true, "Lobby")
        );
        assert!(chat.receive(own("hello"), "Lobby").is_none());
        // Saying the same again later is a new line.
        assert!(chat.receive(own("hello"), "Lobby").is_some());
        assert_eq!(chat.entries().count(), 2);

        // Refused: nothing is shown, and the text goes back in the empty box.
        let mut chat = Chat {
            draft: "hi all ".into(),
            ..Chat::default()
        };
        chat.sent(Scope::Server, "hi all", base);
        chat.draft.clear();
        chat.refused();
        assert!(!chat.overdue(base + Duration::from_secs(3), "Jan", "Lobby"));
        assert!(chat.is_empty_for_tests());
        assert_eq!(chat.draft, "hi all");
        // Something new in the box stays.
        chat.sent(Scope::Server, "hi all", base);
        chat.draft = "next".into();
        chat.refused();
        assert_eq!(chat.draft, "next");
    }

    #[test]
    fn a_channel_you_leave_keeps_its_lines_as_history() {
        let mut chat = Chat::default();
        chat.enter(1, "Lobby");
        chat.receive(incoming(Scope::Channel, "Theo", "in the lobby"), "Lobby");
        chat.enter(2, "Raid night");
        assert!(chat.shown(Layout::Tabs).is_empty(), "A fresh channel tab");
        chat.receive(incoming(Scope::Channel, "Ines", "pulling"), "Raid night");
        assert_eq!(chat.shown(Layout::Tabs)[0].text, "pulling");
        let past: Vec<_> = chat.past().map(|room| room.name.as_str()).collect();
        assert_eq!(past, ["Lobby"]);
        // The feed still has both, each under its own channel's name.
        let places: Vec<_> = chat
            .shown(Layout::Feed)
            .iter()
            .map(|line| line.place.as_str())
            .collect();
        assert_eq!(places, ["Lobby", "Raid night"]);
        assert!(Filter::Channel.shows(&Scope::Past(1)));

        chat.open(Scope::Past(1), "");
        assert_eq!(chat.shown(Layout::Tabs)[0].text, "in the lobby");
        // Going back makes it the channel tab again, with what was said before.
        chat.enter(1, "Lobby");
        assert_eq!(chat.target, Scope::Channel);
        assert_eq!(chat.shown(Layout::Tabs)[0].text, "in the lobby");
        assert_eq!(chat.past().count(), 1, "Raid night is the one left now");
        chat.close_past(2);
        assert_eq!(chat.past().count(), 0);
        // A channel nothing was said in leaves no tab.
        chat.enter(3, "AFK");
        chat.enter(1, "Lobby");
        assert_eq!(chat.past().count(), 0);
    }

    #[test]
    fn the_feed_narrows_to_one_conversation_and_tabs_can_show_everything() {
        let mut chat = Chat::default();
        let mara = Scope::Private("m".into());
        chat.enter(1, "Lobby");
        chat.receive(incoming(Scope::Channel, "Theo", "lobby talk"), "Lobby");
        chat.enter(2, "Raid night");
        chat.receive(incoming(Scope::Channel, "Ines", "raid talk"), "Raid night");
        chat.receive(incoming(mara.clone(), "Mara", "psst"), "Raid night");
        chat.receive(
            incoming(Scope::Private("i".into()), "Ines", "hey"),
            "Raid night",
        );
        let texts = |chat: &Chat, layout| -> Vec<String> {
            chat.shown(layout)
                .iter()
                .map(|line| line.text.clone())
                .collect()
        };

        chat.narrow(Scope::Past(1));
        assert_eq!(texts(&chat, Layout::Feed), ["lobby talk"]);
        assert_eq!(chat.target, Scope::Channel, "Nothing to write to there");
        chat.narrow(Scope::Channel);
        assert_eq!(texts(&chat, Layout::Feed), ["raid talk"]);
        chat.narrow(mara.clone());
        assert_eq!(texts(&chat, Layout::Feed), ["psst"]);
        assert_eq!(chat.target, mara, "The box follows the person");
        // A line from someone else waits unread while narrowed.
        assert!(!chat.shows(Layout::Feed, &Scope::Private("i".into())));
        chat.close_peer("m");
        assert_eq!(chat.filter, Filter::Private);

        // Tabs' All tab shows the lot and reads the lot; opening a tab leaves it.
        chat.mark_unread(&Scope::Server);
        chat.everything = true;
        assert_eq!(chat.shown(Layout::Tabs).len(), 4);
        assert!(chat.tagged(Layout::Tabs));
        assert!(chat.mark_seen(Layout::Tabs));
        // The message box can be pointed elsewhere without leaving it.
        chat.aim(Scope::Server, "");
        assert!(chat.everything);
        chat.open(Scope::Server, "");
        assert!(!chat.everything && !chat.tagged(Layout::Tabs));
    }

    #[test]
    fn history_is_capped() {
        let mut chat = Chat::default();
        for index in 0..LIMIT + 5 {
            chat.receive(
                incoming(Scope::Channel, "Theo", &index.to_string()),
                "Lobby",
            );
        }
        assert_eq!(chat.entries().count(), LIMIT);
        assert_eq!(chat.entries().next().unwrap().text, "5");
    }

    #[test]
    fn bbcode_is_removed_and_link_addresses_kept() {
        assert_eq!(plain("[b]bold[/b] and [I]italic[/I]"), "bold and italic");
        assert_eq!(
            plain("[URL]https://example.org/a[/URL]"),
            "https://example.org/a"
        );
        assert_eq!(
            plain("see [url=https://example.org]the notes[/url]!"),
            "see the notes (https://example.org)!"
        );
        assert_eq!(
            plain("[URL=https://example.org]https://example.org[/URL]"),
            "https://example.org"
        );
        assert_eq!(plain("[color=#ff0000]red[/color]"), "red");
        // Brackets that aren't tags stay.
        assert_eq!(
            plain("array[0] and [not a tag]"),
            "array[0] and [not a tag]"
        );
        assert_eq!(plain("open [ only"), "open [ only");
        assert_eq!(plain("line\none\u{7}"), "line\none");
        // Tags other clients send lose their brackets and keep their text.
        assert_eq!(
            plain("[quote]wipe at 10%[/quote][hr][code]/dps[/code]"),
            "wipe at 10%/dps"
        );
        assert_eq!(
            plain("[table][tr][td]tank[/td][td]Ines[/td][/tr][/table]"),
            "tankInes"
        );
        assert_eq!(plain("[list][*]one[*]two[/list]"), "onetwo");
    }

    #[test]
    fn bare_www_addresses_are_links() {
        let text = "try www.example.org/raid, or awww.no and www.nodot";
        let found: Vec<_> = links(text).into_iter().map(|range| &text[range]).collect();
        assert_eq!(found, ["www.example.org/raid"]);
        assert_eq!(address(found[0]), "https://www.example.org/raid");
        assert_eq!(address("http://a.io"), "http://a.io");
        // Inside a full address it is found once, with the scheme.
        let text = "see https://www.example.org";
        let found: Vec<_> = links(text).into_iter().map(|range| &text[range]).collect();
        assert_eq!(found, ["https://www.example.org"]);
        assert_eq!(
            outgoing("www.example.org").unwrap(),
            "[URL]www.example.org[/URL]"
        );
    }

    #[test]
    fn links_are_found_without_trailing_punctuation() {
        let text = "notes: https://example.org/boss?id=3. and http://a.io/x)";
        let found: Vec<_> = links(text).into_iter().map(|range| &text[range]).collect();
        assert_eq!(found, ["https://example.org/boss?id=3", "http://a.io/x"]);
        assert!(links("no links here, https:// alone").is_empty());
    }

    #[test]
    fn outgoing_text_is_trimmed_wrapped_and_cut() {
        assert_eq!(outgoing("  \n "), None);
        assert_eq!(
            outgoing(" see https://example.org now ").unwrap(),
            "see [URL]https://example.org[/URL] now"
        );
        let long = "ä".repeat(MAX_BYTES);
        let sent = outgoing(&long).unwrap();
        assert!(sent.len() <= MAX_BYTES);
        assert!(sent.chars().all(|c| c == 'ä'));
    }

    #[test]
    fn initials_skip_decoration() {
        assert_eq!(initial(" mara"), "M");
        assert_eq!(initial("[TAG] theo"), "T");
        assert_eq!(initial("…"), "?");
    }
}
