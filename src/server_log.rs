//! What has happened on a server, oldest first, grouped by hour, and searching through it.

use std::collections::{HashMap, VecDeque};
use std::ops::Range;

use chrono::{Local, NaiveDate, NaiveDateTime, Timelike};

/// Enough for a very long session; the oldest go first.
const LIMIT: usize = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Arrived,
    Moved,
    Left,
    Stream,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: u64,
    pub at: NaiveDateTime,
    pub time: String,
    pub tone: Tone,
    pub name: String,
    pub action: String,
}

/// One hour with something to show. `entries` index into the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub key: String,
    pub label: String,
    /// Set when this hour starts a new day after an earlier section.
    pub day: Option<String>,
    pub entries: Vec<usize>,
}

/// Where `query` appears in `text`, ignoring case. An empty query appears nowhere.
pub fn find(text: &str, query: &str) -> Option<Range<usize>> {
    let wanted: Vec<char> = query.trim().chars().flat_map(char::to_lowercase).collect();
    if wanted.is_empty() {
        return None;
    }
    text.char_indices().find_map(|(start, _)| {
        let mut found = 0;
        for (offset, c) in text[start..].char_indices() {
            for lower in c.to_lowercase() {
                if wanted.get(found) != Some(&lower) {
                    return None;
                }
                found += 1;
            }
            if found == wanted.len() {
                return Some(start..start + offset + c.len_utf8());
            }
        }
        None
    })
}

#[derive(Debug, Default)]
pub struct Log {
    entries: VecDeque<Entry>,
    next: u64,
    sections: Vec<Section>,
    query: String,
    person: Option<String>,
    /// Whether the search bar is showing.
    pub searching: bool,
}

impl Log {
    pub fn add(&mut self, tone: Tone, name: impl Into<String>, action: impl Into<String>) {
        self.add_at(Local::now().naive_local(), tone, name, action);
    }

    pub fn add_at(
        &mut self,
        at: NaiveDateTime,
        tone: Tone,
        name: impl Into<String>,
        action: impl Into<String>,
    ) {
        self.entries.push_back(Entry {
            id: self.next,
            at,
            time: at.format("%H:%M").to_string(),
            tone,
            name: name.into(),
            action: action.into(),
        });
        self.next += 1;
        if self.entries.len() > LIMIT {
            self.entries.pop_front();
        }
        self.group();
    }

    pub fn get(&self, index: usize) -> Option<&Entry> {
        self.entries.get(index)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn person(&self) -> Option<&str> {
        self.person.as_deref()
    }

    pub fn filtering(&self) -> bool {
        !self.query.trim().is_empty() || self.person.is_some()
    }

    pub fn set_query(&mut self, query: String) {
        self.query = query;
        self.group();
    }

    pub fn set_person(&mut self, person: Option<String>) {
        self.person = person;
        self.group();
    }

    pub fn clear_filter(&mut self) {
        self.query.clear();
        self.person = None;
        self.group();
    }

    /// Everyone in the log, even if they've left, most frequent first.
    pub fn people(&self) -> Vec<(String, usize)> {
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for entry in self.entries.iter().filter(|entry| !entry.name.is_empty()) {
            *counts.entry(&entry.name).or_default() += 1;
        }
        let mut people: Vec<_> = counts
            .into_iter()
            .map(|(name, count)| (name.to_owned(), count))
            .collect();
        people.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        people
    }

    fn shows(&self, entry: &Entry) -> bool {
        let query = self.query.trim();
        self.person
            .as_ref()
            .is_none_or(|person| *person == entry.name)
            && (query.is_empty()
                || find(&entry.name, query).is_some()
                || find(&entry.action, query).is_some())
    }

    fn group(&mut self) {
        let mut sections: Vec<Section> = Vec::new();
        let mut hour = None;
        let mut date: Option<NaiveDate> = None;
        for (index, entry) in self.entries.iter().enumerate() {
            if !self.shows(entry) {
                continue;
            }
            let this_hour = (entry.at.date(), entry.at.hour());
            if hour != Some(this_hour) {
                hour = Some(this_hour);
                let day = date
                    .is_some_and(|last| last != this_hour.0)
                    .then(|| entry.at.format("%A %-d %B").to_string());
                date = Some(this_hour.0);
                sections.push(Section {
                    key: format!("{} {}", this_hour.0, this_hour.1),
                    label: format!("{:02}:00", this_hour.1),
                    day,
                    entries: Vec::new(),
                });
            }
            if let Some(section) = sections.last_mut() {
                section.entries.push(index);
            }
        }
        self.sections = sections;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(day: u32, hour: u32, minute: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, day)
            .unwrap()
            .and_hms_opt(hour, minute, 0)
            .unwrap()
    }

    fn sample() -> Log {
        let mut log = Log::default();
        log.add_at(at(8, 23, 10), Tone::Arrived, "Mara", "connected");
        log.add_at(at(8, 23, 40), Tone::Moved, "Theo", "→ Raid night");
        log.add_at(at(9, 0, 5), Tone::Moved, "Mara", "→ Raid night");
        log.add_at(at(9, 0, 9), Tone::Left, "Ines", "disconnected");
        log
    }

    #[test]
    fn entries_group_by_hour_and_mark_a_new_day() {
        let log = sample();
        let sections = log.sections();
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].label, "23:00");
        assert_eq!(sections[0].entries, [0, 1]);
        assert_eq!(sections[0].day, None, "nothing to separate yet");
        assert_eq!(sections[1].label, "00:00");
        assert_eq!(sections[1].day.as_deref(), Some("Monday 9 November"));
        assert_eq!(log.get(2).unwrap().time, "00:05");
    }

    #[test]
    fn the_oldest_entries_go_first_and_ids_stay_unique() {
        let mut log = Log::default();
        for _ in 0..LIMIT + 5 {
            log.add_at(at(8, 12, 0), Tone::Moved, "Mara", "→ Lobby");
        }
        assert_eq!(log.len(), LIMIT);
        assert_eq!(log.get(0).unwrap().id, 5);
        assert_eq!(log.sections()[0].entries.len(), LIMIT);
    }

    #[test]
    fn search_finds_names_and_actions_and_combines_with_a_person() {
        let mut log = sample();
        log.set_query("RAID".into());
        assert!(log.filtering());
        let found = |log: &Log| -> Vec<usize> {
            log.sections()
                .iter()
                .flat_map(|s| s.entries.clone())
                .collect()
        };
        assert_eq!(found(&log), [1, 2]);
        log.set_person(Some("Mara".into()));
        assert_eq!(found(&log), [2]);
        log.set_query("nobody".into());
        assert!(log.sections().is_empty());
        log.clear_filter();
        assert!(!log.filtering());
        assert_eq!(found(&log), [0, 1, 2, 3]);
    }

    #[test]
    fn people_count_everyone_most_frequent_first() {
        let mut log = sample();
        log.add_at(at(9, 0, 10), Tone::Arrived, "", "Reconnected");
        assert_eq!(
            log.people(),
            [
                ("Mara".to_owned(), 2),
                ("Ines".to_owned(), 1),
                ("Theo".to_owned(), 1)
            ]
        );
    }

    #[test]
    fn find_ignores_case_and_keeps_byte_ranges_right() {
        assert_eq!(find("→ Raid night", "raid"), Some(4..8));
        assert_eq!(find("Größe", "GRÖ"), Some(0..4));
        assert_eq!(find("anything", "  "), None);
        assert_eq!(find("Lobby", "lobbyy"), None);
    }
}
