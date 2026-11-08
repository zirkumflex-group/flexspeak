//! The release history on the Updates page, compiled in from `CHANGELOG.md` so it works offline
//! and matches this build. `scripts/release.py` adds each release's notes before building.

use std::sync::LazyLock;

use crate::updater::{Build, Notes};

/// What one release changed.
pub struct Entry {
    pub build: Build,
    pub date: &'static str,
    pub notes: Notes,
}

static ENTRIES: LazyLock<Vec<Entry>> = LazyLock::new(|| parse(include_str!("../CHANGELOG.md")));

/// Every release, newest first.
pub fn entries() -> &'static [Entry] {
    &ENTRIES
}

/// Reads `# v1.2.3-build.45 · 2026-01-31` headings, each over its notes.
fn parse(source: &'static str) -> Vec<Entry> {
    let mut entries: Vec<Entry> = source
        .split("\n# ")
        .filter_map(|chunk| {
            let chunk = chunk.strip_prefix("# ").unwrap_or(chunk);
            let (heading, body) = chunk.split_once('\n')?;
            let (tag, date) = heading.split_once(" · ")?;
            let mut notes = Notes::default();
            notes.add(body);
            Some(Entry {
                build: Build::parse_tag(tag.trim())?,
                date: date.trim(),
                notes,
            })
        })
        .collect();
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.build));
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "<!-- about this file -->\n\n# v0.1.0-build.3 · 2026-02-02\n\n## Fixed\n\n- Newer\n\n# v0.1.0-build.2 · 2026-02-01\n\n## New\n\n- Older\n";

    #[test]
    fn reads_each_build_newest_first_and_skips_the_preamble() {
        let entries = parse(SAMPLE);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].build.number, 3);
        assert_eq!(entries[0].date, "2026-02-02");
        assert_eq!(entries[0].notes.fixed, ["Newer"]);
        assert_eq!(entries[1].notes.new, ["Older"]);
    }

    #[test]
    fn a_file_that_starts_at_a_heading_keeps_its_first_build() {
        let entries = parse("# v0.1.0-build.9 · 2026-03-01\n\n## New\n\n- Only\n");
        assert_eq!(entries[0].build.number, 9);
        assert_eq!(entries[0].notes.new, ["Only"]);
    }

    /// A release adds its entry to the file on Windows, Mac and Linux alike.
    #[test]
    fn windows_line_endings_read_the_same() {
        let source: &'static str = Box::leak(SAMPLE.replace('\n', "\r\n").into_boxed_str());
        let entries = parse(source);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].notes.new, ["Older"]);
    }

    #[test]
    fn the_shipped_changelog_is_complete_and_nothing_is_cut_off() {
        let source = include_str!("../CHANGELOG.md");
        let headings = source.lines().filter(|l| l.starts_with("# ")).count();
        let items = source.lines().filter(|l| l.starts_with("- ")).count();
        let entries = entries();
        assert_eq!(entries.len(), headings, "a heading failed to parse");
        let mut shown = 0;
        for entry in entries {
            assert!(
                !entry.notes.is_empty(),
                "build {} says nothing",
                entry.build.number
            );
            shown += entry.notes.new.len() + entry.notes.fixed.len() + entry.notes.changed.len();
        }
        // Duplicates inside a release are dropped, so a shortfall means a lost line.
        assert_eq!(shown, items, "a note was dropped");
        assert!(
            source
                .lines()
                .all(|l| l.chars().count() <= 202 || !l.starts_with("- "))
        );
        assert!(entries.windows(2).all(|pair| pair[0].build > pair[1].build));
    }
}
