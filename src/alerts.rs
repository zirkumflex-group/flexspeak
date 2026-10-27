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
}

impl Source {
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
        }
    }
}

#[derive(Debug)]
pub struct Incident {
    pub source: Source,
    pub detail: String,
    pub reports: u32,
}

/// One incident per operation; dismissing suppresses repeats until recovery or an explicit retry.
#[derive(Default)]
pub struct Alerts {
    incidents: BTreeMap<Source, Incident>,
    visible: VecDeque<Source>,
}

impl Alerts {
    pub fn raise(&mut self, source: Source, detail: impl AsRef<str>) {
        let detail: String = detail.as_ref().chars().take(4096).collect();
        if let Some(incident) = self.incidents.get_mut(&source) {
            incident.detail = detail;
            incident.reports = incident.reports.saturating_add(1);
        } else {
            self.incidents.insert(
                source,
                Incident {
                    source,
                    detail,
                    reports: 1,
                },
            );
            self.visible.push_back(source);
        }
    }
    pub fn current(&self) -> Option<&Incident> {
        self.visible
            .front()
            .and_then(|source| self.incidents.get(source))
    }
    pub fn dismiss(&mut self) {
        self.visible.pop_front();
    }
    pub fn recover(&mut self, source: Source) {
        self.incidents.remove(&source);
        self.visible.retain(|key| *key != source);
    }
    pub fn begin(&mut self, source: Source) {
        self.recover(source);
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
}
