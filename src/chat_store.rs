//! Chat history on disk, so conversations come back after a restart.
//!
//! One SQLite file beside the settings file holds every server's lines, filed under the address
//! it was connected by. Nothing leaves this computer, and the text is stored as plainly as the
//! settings are; Settings › Chat turns keeping it off and clears it.

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{Connection, params};

use crate::chat::{Entry, Kind, Scope};

/// The file's name, beside `settings.json`.
pub const FILE: &str = "chat.db";
/// Lines brought back per conversation when a server connects.
const PER_CONVERSATION: usize = 200;
/// Lines kept per server; the oldest go when it connects.
const KEPT: usize = 10_000;

/// A line as it was stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    pub scope: Scope,
    pub kind: Kind,
    pub own: bool,
    pub from: String,
    pub place: String,
    /// The channel a channel line was said in; 0 for the others.
    pub room: u64,
    pub text: String,
    /// Seconds since 1970, UTC.
    pub at: i64,
}

/// What a server's history holds: its lines, oldest first, and the conversations whose tab was
/// closed and has had nothing new since.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Loaded {
    pub lines: Vec<Stored>,
    pub closed: Vec<Scope>,
}

pub struct Store {
    db: Connection,
}

/// How a conversation is filed: its kind, and the channel number or the person's key.
fn filed(scope: &Scope, room: u64) -> (i64, String) {
    match scope {
        Scope::Channel => (0, room.to_string()),
        Scope::Past(id) => (0, id.to_string()),
        Scope::Server => (1, String::new()),
        Scope::Private(key) => (2, key.clone()),
    }
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)
                .with_context(|| format!("Could not create {}", folder.display()))?;
        }
        let db =
            Connection::open(path).with_context(|| format!("Could not open {}", path.display()))?;
        // Write-ahead logging keeps a line's insert off the disk's sync path.
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.pragma_update(None, "synchronous", "NORMAL")?;
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS lines (
                 id INTEGER PRIMARY KEY,
                 server TEXT NOT NULL,
                 scope INTEGER NOT NULL,
                 key TEXT NOT NULL,
                 kind INTEGER NOT NULL,
                 own INTEGER NOT NULL,
                 sender TEXT NOT NULL,
                 place TEXT NOT NULL,
                 body TEXT NOT NULL,
                 at INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS lines_by_conversation
                 ON lines (server, scope, key, id);
             CREATE TABLE IF NOT EXISTS closed (
                 server TEXT NOT NULL,
                 scope INTEGER NOT NULL,
                 key TEXT NOT NULL,
                 upto INTEGER NOT NULL,
                 PRIMARY KEY (server, scope, key)
             );",
        )
        .context("Could not prepare the chat history")?;
        Ok(Self { db })
    }

    /// Adds lines to a server's history, in order.
    pub fn append(&mut self, server: &str, entries: &[Entry]) -> Result<()> {
        let change = self.db.transaction()?;
        {
            let mut insert = change.prepare_cached(
                "INSERT INTO lines (server, scope, key, kind, own, sender, place, body, at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )?;
            for entry in entries {
                let (scope, key) = filed(&entry.scope, entry.room);
                insert.execute(params![
                    server,
                    scope,
                    key,
                    i64::from(entry.kind == Kind::Poke),
                    entry.own,
                    entry.from,
                    entry.place,
                    entry.text,
                    entry.at,
                ])?;
            }
        }
        change.commit()?;
        Ok(())
    }

    /// A server's latest lines, up to `PER_CONVERSATION` of each conversation, oldest first.
    /// Lines past what is kept per server are dropped on the way.
    pub fn load(&mut self, server: &str) -> Result<Loaded> {
        self.db.execute(
            "DELETE FROM lines WHERE server = ?1 AND id <= (
                 SELECT id FROM lines WHERE server = ?1 ORDER BY id DESC LIMIT 1 OFFSET ?2
             )",
            params![server, KEPT as i64],
        )?;
        let mut lines = self
            .db
            .prepare(
                "SELECT scope, key, kind, own, sender, place, body, at FROM (
                     SELECT *, ROW_NUMBER() OVER (
                         PARTITION BY scope, key ORDER BY id DESC
                     ) AS nth FROM lines WHERE server = ?1
                 ) WHERE nth <= ?2 ORDER BY id DESC LIMIT ?3",
            )?
            .query_map(
                params![server, PER_CONVERSATION as i64, crate::chat::LIMIT as i64],
                |row| {
                    let (scope, key): (i64, String) = (row.get(0)?, row.get(1)?);
                    let room = if scope == 0 {
                        key.parse().unwrap_or(0)
                    } else {
                        0
                    };
                    Ok(Stored {
                        scope: match scope {
                            0 => Scope::Channel,
                            1 => Scope::Server,
                            _ => Scope::Private(key),
                        },
                        kind: if row.get::<_, i64>(2)? == 1 {
                            Kind::Poke
                        } else {
                            Kind::Message
                        },
                        own: row.get(3)?,
                        from: row.get(4)?,
                        place: row.get(5)?,
                        room,
                        text: row.get(6)?,
                        at: row.get(7)?,
                    })
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        lines.reverse();
        let closed = self
            .db
            .prepare(
                "SELECT scope, key FROM closed WHERE server = ?1 AND upto >= (
                     SELECT COALESCE(MAX(id), 0) FROM lines
                     WHERE lines.server = closed.server
                       AND lines.scope = closed.scope
                       AND lines.key = closed.key
                 )",
            )?
            .query_map(params![server], |row| {
                let (scope, key): (i64, String) = (row.get(0)?, row.get(1)?);
                Ok(match scope {
                    0 => Scope::Past(key.parse().unwrap_or(0)),
                    1 => Scope::Server,
                    _ => Scope::Private(key),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(Loaded { lines, closed })
    }

    /// A conversation's tab was closed: it stays away after a restart until something new is
    /// said there.
    pub fn close(&mut self, server: &str, scope: &Scope) -> Result<()> {
        let (kind, key) = filed(scope, 0);
        self.db.execute(
            "INSERT INTO closed (server, scope, key, upto)
             VALUES (?1, ?2, ?3, (SELECT COALESCE(MAX(id), 0) FROM lines))
             ON CONFLICT (server, scope, key) DO UPDATE SET upto = excluded.upto",
            params![server, kind, key],
        )?;
        Ok(())
    }

    /// A closed conversation was opened again.
    pub fn reopen(&mut self, server: &str, scope: &Scope) -> Result<()> {
        let (kind, key) = filed(scope, 0);
        self.db.execute(
            "DELETE FROM closed WHERE server = ?1 AND scope = ?2 AND key = ?3",
            params![server, kind, key],
        )?;
        Ok(())
    }

    /// Forgets every server's history and gives the space back.
    pub fn clear(&mut self) -> Result<()> {
        self.db
            .execute_batch("DELETE FROM lines; DELETE FROM closed; VACUUM;")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{Chat, Incoming};

    fn folder(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "flexspeak-chat-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    fn said(chat: &mut Chat, scope: Scope, from: &str, text: &str) {
        chat.receive(
            Incoming {
                scope,
                own: false,
                from: from.into(),
                peer: from.into(),
                text: text.into(),
            },
            "Lobby",
        );
    }

    #[test]
    fn lines_come_back_per_server_in_order_with_their_conversation() {
        let root = folder("roundtrip");
        let path = root.join("nested").join(FILE);
        let mut chat = Chat::default();
        chat.enter(7, "Lobby");
        said(&mut chat, Scope::Channel, "Theo", "in the lobby");
        said(&mut chat, Scope::Server, "Lukas", "restart at 4");
        said(&mut chat, Scope::Private("uid-2".into()), "Mara", "psst");
        chat.poke("uid-2".into(), "Mara", true, "wake up");
        {
            let mut store = Store::open(&path).unwrap();
            store.append("one.example", &chat.unsaved()).unwrap();
            assert!(chat.unsaved().is_empty(), "Saved once");
            store.append("two.example", &[]).unwrap();
        }
        // A new run of the app.
        let mut store = Store::open(&path).unwrap();
        assert_eq!(store.load("two.example").unwrap(), Loaded::default());
        let loaded = store.load("one.example").unwrap();
        let texts: Vec<_> = loaded.lines.iter().map(|line| line.text.as_str()).collect();
        assert_eq!(texts, ["in the lobby", "restart at 4", "psst", "wake up"]);
        assert_eq!(
            (loaded.lines[0].room, &loaded.lines[0].scope),
            (7, &Scope::Channel)
        );
        assert_eq!(loaded.lines[3].kind, Kind::Poke);
        assert!(loaded.lines[3].own && !loaded.lines[2].own);
        assert!(loaded.lines[0].at > 1_700_000_000);

        let mut restored = Chat::default();
        restored.restore(loaded);
        restored.enter(7, "Lobby");
        assert_eq!(
            restored.shown(crate::chat::Layout::Tabs)[0].text,
            "in the lobby"
        );
        assert_eq!(restored.peers[0].name, "Mara");
        assert!(
            restored.unsaved().is_empty(),
            "What came from disk isn't written again"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_closed_tab_stays_closed_until_something_new_is_said() {
        let root = folder("closed");
        let mut store = Store::open(&root.join(FILE)).unwrap();
        let mara = Scope::Private("uid-2".into());
        let mut chat = Chat::default();
        said(&mut chat, mara.clone(), "Mara", "psst");
        store.append("one.example", &chat.unsaved()).unwrap();
        store.close("one.example", &mara).unwrap();

        let loaded = store.load("one.example").unwrap();
        assert_eq!(loaded.closed, std::slice::from_ref(&mara));
        let mut restored = Chat::default();
        restored.restore(loaded);
        assert!(restored.peers.is_empty(), "No tab");
        restored.open(mara.clone(), "Mara");
        assert_eq!(
            restored.shown(crate::chat::Layout::Tabs).len(),
            1,
            "The lines are there"
        );

        said(&mut chat, mara.clone(), "Mara", "hello?");
        store.append("one.example", &chat.unsaved()).unwrap();
        assert!(store.load("one.example").unwrap().closed.is_empty());
        store.close("one.example", &mara).unwrap();
        store.reopen("one.example", &mara).unwrap();
        assert!(store.load("one.example").unwrap().closed.is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn each_conversation_keeps_its_latest_lines_and_clearing_empties_it() {
        let root = folder("limits");
        let mut store = Store::open(&root.join(FILE)).unwrap();
        let mut chat = Chat::default();
        chat.enter(1, "Lobby");
        said(
            &mut chat,
            Scope::Private("uid-2".into()),
            "Mara",
            "an old private line",
        );
        for index in 0..PER_CONVERSATION + 20 {
            said(&mut chat, Scope::Channel, "Theo", &index.to_string());
        }
        store.append("one.example", &chat.unsaved()).unwrap();
        let loaded = store.load("one.example").unwrap();
        // A busy channel doesn't push a quiet conversation out.
        assert_eq!(loaded.lines.len(), PER_CONVERSATION + 1);
        assert_eq!(loaded.lines[0].text, "an old private line");
        assert_eq!(loaded.lines[1].text, "20");

        store.clear().unwrap();
        assert_eq!(store.load("one.example").unwrap(), Loaded::default());
        let _ = std::fs::remove_dir_all(root);
    }
}
