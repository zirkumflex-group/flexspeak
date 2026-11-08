use std::collections::{BTreeMap, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    Connection,
    Audio,
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
    Move,
    Moderate,
    Groups,
    Install,
}

impl Source {
    /// Whether this kind of problem belongs to one server rather than to the whole app.
    pub fn server_scoped(self) -> bool {
        matches!(
            self,
            Self::Connection
                | Self::Join
                | Self::Controls
                | Self::Subscription
                | Self::Flood
                | Self::Poke
                | Self::Move
                | Self::Moderate
                | Self::Groups
                | Self::StreamDiscovery
                | Self::StreamWatch
                | Self::StreamFrame
                | Self::StreamPublish
        )
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Connection => "Connection failed",
            Self::Audio => "Audio unavailable",
            Self::Worker => "Command could not be delivered",
            Self::Join => "Could not join channel",
            Self::Controls => "Server audio state could not be updated",
            Self::Subscription => "Channel list could not be subscribed",
            Self::Flood => "Server is limiting requests",
            Self::SettingsLoad => "Could not load settings",
            Self::SettingsSave => "Could not save settings",
            Self::Shortcuts => "Global shortcuts unavailable",
            Self::Trace => "Trace could not be recorded or saved",
            Self::StreamDiscovery => "Could not find streams",
            Self::StreamWatch => "Could not watch stream",
            Self::StreamFrame => "Video frame could not be displayed",
            Self::StreamPublish => "Screen sharing unavailable",
            Self::Poke => "Could not poke",
            Self::Move => "Could not move clients",
            Self::Moderate => "Could not kick or ban",
            Self::Groups => "Could not change groups",
            Self::Install => "Could not install Flexspeak",
        }
    }
}

#[derive(Debug)]
pub struct Incident {
    pub source: Source,
    pub detail: String,
    pub reports: u32,
    /// Which server this happened on, when that needs saying.
    pub server: Option<String>,
}

type Key = (Option<u64>, Source);

/// One incident per operation and server; dismissing suppresses repeats until recovery or an
/// explicit retry.
#[derive(Default)]
pub struct Alerts {
    incidents: BTreeMap<Key, Incident>,
    visible: VecDeque<Key>,
}

impl Alerts {
    pub fn raise(&mut self, source: Source, detail: impl AsRef<str>) {
        self.insert((None, source), None, detail);
    }
    /// Raises an alert for one server, naming it when `server` is given.
    pub fn raise_in(
        &mut self,
        scope: u64,
        server: Option<String>,
        source: Source,
        detail: impl AsRef<str>,
    ) {
        self.insert((Some(scope), source), server, detail);
    }
    fn insert(&mut self, key: Key, server: Option<String>, detail: impl AsRef<str>) {
        let detail: String = detail.as_ref().chars().take(4096).collect();
        if let Some(incident) = self.incidents.get_mut(&key) {
            incident.detail = detail;
            incident.reports = incident.reports.saturating_add(1);
            if server.is_some() {
                incident.server = server;
            }
        } else {
            self.incidents.insert(
                key,
                Incident {
                    source: key.1,
                    detail,
                    reports: 1,
                    server,
                },
            );
            self.visible.push_back(key);
        }
    }
    pub fn current(&self) -> Option<&Incident> {
        self.visible.front().and_then(|key| self.incidents.get(key))
    }
    pub fn dismiss(&mut self) {
        self.visible.pop_front();
    }
    fn clear(&mut self, key: Key) {
        self.incidents.remove(&key);
        self.visible.retain(|visible| *visible != key);
    }
    pub fn recover(&mut self, source: Source) {
        self.clear((None, source));
    }
    pub fn recover_in(&mut self, scope: u64, source: Source) {
        self.clear((Some(scope), source));
    }
    pub fn begin(&mut self, source: Source) {
        self.recover(source);
    }
    pub fn begin_in(&mut self, scope: u64, source: Source) {
        self.recover_in(scope, source);
    }
    /// Forgets everything that happened on a server that has gone.
    pub fn close_server(&mut self, scope: u64) {
        self.incidents.retain(|key, _| key.0 != Some(scope));
        self.visible.retain(|key| key.0 != Some(scope));
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
        assert_eq!(alerts.visible.len(), 2);
        alerts.dismiss();
        alerts.raise(Source::Flood, "Still blocked");
        assert_eq!(alerts.current().unwrap().source, Source::Audio);
        alerts.recover(Source::Flood);
        assert_eq!(alerts.current().unwrap().source, Source::Audio);
        alerts.dismiss();
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
}
