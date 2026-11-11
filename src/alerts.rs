//! Problems worth telling you about, shown as rows in the notice strip under the title bar.
//!
//! Nothing here blocks the window. A problem that is a state, such as a missing device or a
//! shortcut another app holds, stays until whatever caused it recovers, which clears it on its
//! own. A problem that is an event, such as a poke the server refused, fades after a few
//! seconds. Either can be closed early, and a closed problem stays quiet until it recovers.

use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};

use chrono::{Local, NaiveDateTime};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    /// A connection that never got as far as the server.
    Connection,
    /// A connection the server or the network ended.
    Dropped,
    /// No microphone or playback device could be opened.
    Audio,
    /// A device went away and another one stands in until it's back.
    AudioFallback,
    Worker,
    Join,
    Controls,
    Subscription,
    Flood,
    SettingsLoad,
    SettingsSave,
    Shortcuts,
    Trace,
    StreamDiscovery,
    StreamWatch,
    StreamFrame,
    StreamPublish,
    Poke,
    /// A text message the server didn't take.
    Chat,
    /// The chat history file couldn't be read or written.
    ChatHistory,
    Move,
    Moderate,
    Groups,
    Install,
}

/// How much a problem matters, which sets its colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Something changed under you, and Flexspeak has already worked around it.
    Notice,
    /// Something you asked for didn't happen, or a feature is off for now.
    Warning,
    /// You lost something: a connection, or all audio.
    Error,
}

impl Source {
    /// Whether this kind of problem belongs to one server rather than to the whole app.
    pub fn server_scoped(self) -> bool {
        matches!(
            self,
            Self::Connection
                | Self::Dropped
                | Self::Join
                | Self::Controls
                | Self::Subscription
                | Self::Flood
                | Self::Poke
                | Self::Chat
                | Self::Move
                | Self::Moderate
                | Self::Groups
                | Self::StreamDiscovery
                | Self::StreamWatch
                | Self::StreamFrame
                | Self::StreamPublish
        )
    }

    /// Whether this is a state that lasts until it recovers, rather than a one-off failure.
    pub fn lasting(self) -> bool {
        matches!(
            self,
            Self::Connection
                | Self::Dropped
                | Self::Audio
                | Self::AudioFallback
                | Self::Flood
                | Self::SettingsLoad
                | Self::SettingsSave
                | Self::Shortcuts
                | Self::StreamPublish
                | Self::Install
        )
    }

    pub fn severity(self) -> Severity {
        match self {
            Self::Connection | Self::Dropped | Self::Audio => Severity::Error,
            Self::AudioFallback | Self::Flood => Severity::Notice,
            _ => Severity::Warning,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Connection => "Couldn't connect",
            Self::Dropped => "Disconnected",
            Self::Audio => "No audio device",
            Self::AudioFallback => "Audio device switched",
            Self::Worker => "Command not delivered",
            Self::Join => "Couldn't join channel",
            Self::Controls => "Mute state not synced",
            Self::Subscription => "Channel list incomplete",
            Self::Flood => "Server is slowing requests",
            Self::SettingsLoad => "Settings not loaded",
            Self::SettingsSave => "Settings not saved",
            Self::Shortcuts => "Shortcut unavailable",
            Self::Trace => "Trace not saved",
            Self::StreamDiscovery => "Couldn't list streams",
            Self::StreamWatch => "Couldn't watch stream",
            Self::StreamFrame => "Video frame dropped",
            Self::StreamPublish => "Screen sharing stopped",
            Self::Poke => "Poke not sent",
            Self::Chat => "Message not sent",
            Self::ChatHistory => "Chat history not saved",
            Self::Move => "Couldn't move",
            Self::Moderate => "Couldn't kick or ban",
            Self::Groups => "Groups not changed",
            Self::Install => "Install failed",
        }
    }
}

/// How long a one-off failure stays up, counted from its last report.
pub const PASSING: Duration = Duration::from_secs(8);

#[derive(Debug, Clone)]
pub struct Incident {
    /// Stable while it shows, for the strip's keys and its close button.
    pub id: u64,
    pub source: Source,
    pub detail: String,
    pub reports: u32,
    /// Which server this happened on, when that needs saying.
    pub server: Option<String>,
    /// The saved address a lost connection can be made again to.
    pub address: Option<String>,
    /// When a one-off failure goes away by itself.
    until: Option<Instant>,
    hidden: bool,
}

impl Incident {
    /// The row's heading: the problem, and the server when that needs saying.
    pub fn heading(&self) -> String {
        match (&self.server, self.source) {
            (Some(server), Source::Connection) => format!("Couldn't connect to {server}"),
            (Some(server), Source::Dropped) => format!("Disconnected from {server}"),
            (Some(server), source) => format!("{server}: {}", source.title()),
            (None, source) => source.title().to_owned(),
        }
    }
}

/// Where a problem belongs: the app, one live connection, or a server you're no longer on.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scope {
    App,
    Session(u64),
    Address(String),
}

type Key = (Scope, Source);

/// How many problems the error log keeps; older ones drop off.
const HISTORY: usize = 200;

/// One line of the error log in Settings: a problem as it happened, kept after its notice has
/// gone.
#[derive(Debug, Clone)]
pub struct Logged {
    incident: u64,
    pub at: NaiveDateTime,
    pub source: Source,
    pub heading: String,
    pub detail: String,
    /// How many times it was reported before it changed or recovered.
    pub reports: u32,
    /// It went away on its own: the device came back, the connection was made again.
    pub resolved: bool,
}

/// One incident per operation and place; closing one keeps its repeats quiet until it recovers
/// or is tried again.
#[derive(Default)]
pub struct Alerts {
    incidents: BTreeMap<Key, Incident>,
    next_id: u64,
    history: VecDeque<Logged>,
}

impl Alerts {
    pub fn raise(&mut self, source: Source, detail: impl AsRef<str>) {
        self.insert((Scope::App, source), None, None, detail);
    }
    /// Raises an alert for one server, naming it when `server` is given.
    pub fn raise_in(
        &mut self,
        scope: u64,
        server: Option<String>,
        source: Source,
        detail: impl AsRef<str>,
    ) {
        self.insert((Scope::Session(scope), source), server, None, detail);
    }
    /// Raises an alert about a server whose connection has ended, which connecting to the same
    /// address again clears.
    pub fn raise_at(
        &mut self,
        address: &str,
        server: Option<String>,
        source: Source,
        detail: impl AsRef<str>,
    ) {
        self.insert(
            (Scope::Address(address.to_owned()), source),
            server,
            Some(address.to_owned()),
            detail,
        );
    }
    fn insert(
        &mut self,
        key: Key,
        server: Option<String>,
        address: Option<String>,
        detail: impl AsRef<str>,
    ) {
        let detail: String = detail.as_ref().chars().take(4096).collect();
        let source = key.1;
        let until = (!source.lasting()).then(|| Instant::now() + PASSING);
        if let Some(incident) = self.incidents.get_mut(&key) {
            incident.detail = detail;
            incident.reports = incident.reports.saturating_add(1);
            incident.until = until;
            if server.is_some() {
                incident.server = server;
            }
            if address.is_some() {
                incident.address = address;
            }
        } else {
            self.next_id += 1;
            self.incidents.insert(
                key.clone(),
                Incident {
                    id: self.next_id,
                    source,
                    detail,
                    reports: 1,
                    server,
                    address,
                    until,
                    hidden: false,
                },
            );
        }
        let incident = &self.incidents[&key];
        self.log(
            incident.id,
            incident.heading(),
            incident.detail.clone(),
            source,
        );
    }

    /// Adds a problem to the log, or counts it again if it's the same one saying the same thing.
    fn log(&mut self, incident: u64, heading: String, detail: String, source: Source) {
        if let Some(entry) = self
            .history
            .iter_mut()
            .rev()
            .find(|entry| entry.incident == incident)
            .filter(|entry| entry.detail == detail && !entry.resolved)
        {
            entry.reports = entry.reports.saturating_add(1);
            entry.at = Local::now().naive_local();
            return;
        }
        if self.history.len() == HISTORY {
            self.history.pop_front();
        }
        self.history.push_back(Logged {
            incident,
            at: Local::now().naive_local(),
            source,
            heading,
            detail,
            reports: 1,
            resolved: false,
        });
    }

    /// The error log, newest first.
    pub fn history(&self) -> impl Iterator<Item = &Logged> {
        self.history.iter().rev()
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    /// The error log as plain text, newest first, for pasting into a bug report.
    pub fn history_text(&self) -> String {
        self.history()
            .map(|entry| {
                let mut line = format!(
                    "{}  {}: {}",
                    entry.at.format("%Y-%m-%d %H:%M:%S"),
                    entry.heading,
                    entry.detail
                );
                if entry.reports > 1 {
                    line.push_str(&format!(" (\u{d7}{})", entry.reports));
                }
                if entry.resolved {
                    line.push_str(" [resolved]");
                }
                line
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
    /// What the strip shows, oldest first.
    pub fn visible(&self) -> Vec<&Incident> {
        let mut shown: Vec<_> = self
            .incidents
            .values()
            .filter(|incident| !incident.hidden)
            .collect();
        shown.sort_by_key(|incident| incident.id);
        shown
    }
    #[cfg(test)]
    pub fn current(&self) -> Option<&Incident> {
        self.visible().into_iter().next()
    }
    /// Closes one row. A lasting problem stays quiet until it recovers; a one-off failure is
    /// forgotten once its time is up, so the next one shows.
    pub fn dismiss(&mut self, id: u64) {
        if let Some(incident) = self
            .incidents
            .values_mut()
            .find(|incident| incident.id == id)
        {
            incident.hidden = true;
        }
    }
    /// Forgets one-off failures whose time is up. True when the strip changed.
    pub fn expire(&mut self, now: Instant) -> bool {
        let before = self.visible().len();
        self.incidents
            .retain(|_, incident| incident.until.is_none_or(|until| until > now));
        before != self.visible().len()
    }
    /// When the next one-off failure fades, so the window can wake for it.
    pub fn next_expiry(&self) -> Option<Instant> {
        self.incidents
            .values()
            .filter(|incident| !incident.hidden)
            .filter_map(|incident| incident.until)
            .min()
    }
    /// Drops a problem. `resolved` marks it in the log as having gone away on its own, rather
    /// than being set aside for a retry.
    fn clear(&mut self, key: &Key, resolved: bool) {
        if let Some(incident) = self.incidents.remove(key)
            && resolved
        {
            self.resolve(incident.id);
        }
    }
    fn resolve(&mut self, incident: u64) {
        for entry in self
            .history
            .iter_mut()
            .filter(|entry| entry.incident == incident)
        {
            entry.resolved = true;
        }
    }
    pub fn recover(&mut self, source: Source) {
        self.clear(&(Scope::App, source), true);
    }
    pub fn recover_in(&mut self, scope: u64, source: Source) {
        self.clear(&(Scope::Session(scope), source), true);
    }
    /// Connecting to this address again: why it last failed no longer applies.
    pub fn recover_at(&mut self, address: &str) {
        let gone: Vec<Key> = self
            .incidents
            .keys()
            .filter(|key| matches!(&key.0, Scope::Address(other) if other == address))
            .cloned()
            .collect();
        for key in gone {
            self.clear(&key, true);
        }
    }
    /// Trying something again: its last failure is set aside, not resolved.
    pub fn begin(&mut self, source: Source) {
        self.clear(&(Scope::App, source), false);
    }
    pub fn begin_in(&mut self, scope: u64, source: Source) {
        self.clear(&(Scope::Session(scope), source), false);
    }
    /// Forgets everything that happened on a server that has gone.
    pub fn close_server(&mut self, scope: u64) {
        self.incidents
            .retain(|key, _| key.0 != Scope::Session(scope));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeats_are_bounded_dismissed_episodes_stay_closed_and_recovery_is_scoped() {
        let mut alerts = Alerts::default();
        for _ in 0..100 {
            alerts.raise(Source::Flood, "Back off");
        }
        alerts.raise(Source::Audio, "Device stopped");
        assert_eq!(alerts.current().unwrap().reports, 100);
        assert_eq!(alerts.visible().len(), 2);
        alerts.dismiss(alerts.current().unwrap().id);
        alerts.raise(Source::Flood, "Still blocked");
        assert_eq!(alerts.current().unwrap().source, Source::Audio);
        alerts.recover(Source::Flood);
        assert_eq!(alerts.current().unwrap().source, Source::Audio);
        alerts.dismiss(alerts.current().unwrap().id);
        assert!(alerts.current().is_none());
        alerts.begin(Source::Audio);
        alerts.raise(Source::Audio, "Retry failed");
        assert_eq!(alerts.current().unwrap().reports, 1);
        alerts.recover(Source::Audio);
        assert!(alerts.current().is_none());
    }

    #[test]
    fn one_servers_recovery_does_not_clear_another_servers_alert() {
        let mut alerts = Alerts::default();
        alerts.raise_in(1, Some("Work".into()), Source::Flood, "Busy");
        alerts.raise_in(2, None, Source::Flood, "Also busy");
        assert_eq!(alerts.current().unwrap().server.as_deref(), Some("Work"));
        alerts.recover_in(2, Source::Flood);
        assert_eq!(alerts.current().unwrap().detail, "Busy");
        alerts.close_server(1);
        assert!(alerts.current().is_none());
    }

    #[test]
    fn one_off_failures_fade_and_lasting_problems_wait_for_recovery() {
        let mut alerts = Alerts::default();
        alerts.raise_in(1, None, Source::Poke, "They left");
        alerts.raise(Source::Shortcuts, "Ctrl+Shift+M is taken");
        let fades = alerts.next_expiry().unwrap();
        assert!(!alerts.expire(fades - Duration::from_millis(1)));
        assert!(alerts.expire(fades));
        assert_eq!(alerts.visible().len(), 1);
        assert_eq!(alerts.current().unwrap().source, Source::Shortcuts);
        assert!(alerts.next_expiry().is_none());
        // A closed one-off failure is forgotten in time, so the next one shows again.
        alerts.raise_in(1, None, Source::Poke, "Again");
        alerts.dismiss(alerts.visible()[1].id);
        alerts.raise_in(1, None, Source::Poke, "And again");
        assert_eq!(alerts.visible().len(), 1);
        alerts.expire(Instant::now() + PASSING * 2);
        alerts.raise_in(1, None, Source::Poke, "Later");
        assert_eq!(alerts.visible().len(), 2);
    }

    #[test]
    fn the_log_keeps_problems_after_their_notices_go() {
        let mut alerts = Alerts::default();
        alerts.raise(Source::AudioFallback, "Speakers stand in");
        alerts.raise(Source::AudioFallback, "Speakers stand in");
        alerts.raise_in(1, None, Source::Poke, "They left");
        alerts.expire(Instant::now() + PASSING * 2);
        alerts.recover(Source::AudioFallback);
        alerts.begin(Source::Shortcuts);
        assert!(alerts.visible().is_empty());
        let log: Vec<_> = alerts.history().collect();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].heading, "Poke not sent");
        assert!(!log[0].resolved);
        assert_eq!(log[1].reports, 2);
        assert!(log[1].resolved);
        let text = alerts.history_text();
        assert!(text.contains("Audio device switched: Speakers stand in (\u{d7}2) [resolved]"));
        // Retrying doesn't count as resolved.
        alerts.raise(Source::Shortcuts, "Taken");
        alerts.begin(Source::Shortcuts);
        assert!(!alerts.history().next().unwrap().resolved);
        alerts.clear_history();
        assert_eq!(alerts.history().count(), 0);
    }

    #[test]
    fn a_lost_connection_is_kept_by_address_until_it_is_made_again() {
        let mut alerts = Alerts::default();
        alerts.raise_at(
            "ts.example.org",
            Some("Work".into()),
            Source::Dropped,
            "Kicked",
        );
        alerts.raise_at("voice.example.org", None, Source::Connection, "Timed out");
        let first = alerts.current().unwrap();
        assert_eq!(first.heading(), "Disconnected from Work");
        assert_eq!(first.address.as_deref(), Some("ts.example.org"));
        alerts.close_server(1);
        assert_eq!(alerts.visible().len(), 2);
        alerts.recover_at("ts.example.org");
        assert_eq!(alerts.current().unwrap().heading(), "Couldn't connect");
    }
}
