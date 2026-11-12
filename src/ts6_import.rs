//! What setup can carry over from a TeamSpeak 6 install on this computer.
//!
//! The client keeps its settings in a SQLite database. Only the audio settings, saved server
//! bookmarks and display name are read, and only when the user asks. The display name shares a
//! record with login data: that field alone is copied out, and the rest is never kept or logged.
//! Server passwords and identities are not copied.
//!
//! TeamSpeak doesn't document this format, so anything that doesn't look as expected is skipped.

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

const DEFAULT_PORT: u64 = 9987;
const MAX_SERVERS: usize = 32;
/// The bookmark field holding a plain server, among the other bookmark kinds.
const SERVER_BOOKMARK: u64 = 16;
const BOOKMARK_NAME: u64 = 1;
const BOOKMARK_ADDRESS: u64 = 2;
const BOOKMARK_PORT: u64 = 3;
const ACCOUNT_DISPLAY_NAME: u64 = 4;
/// TeamSpeak's limit on a nickname.
const MAX_NAME_CHARS: usize = 30;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Imported {
    pub nickname: Option<String>,
    pub servers: Vec<String>,
    /// The bookmarks' names, by address, for those that have one.
    pub names: std::collections::BTreeMap<String, String>,
    /// Microphone and speakers by name, because device IDs differ between the two apps.
    pub input: Option<String>,
    pub output: Option<String>,
    pub push_to_talk: bool,
    pub ptt_release_ms: u16,
}

pub fn path() -> Option<PathBuf> {
    let dirs = directories::BaseDirs::new()?;
    Some(dirs.preference_dir().join("TeamSpeak/Default/settings.db"))
}

pub fn read(path: &Path) -> Result<Imported> {
    let db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .context("Cannot open the TeamSpeak 6 settings")?;
    // TeamSpeak may be running and writing.
    db.busy_timeout(Duration::from_secs(1))?;
    let mut imported = Imported::default();
    let audio: Option<Vec<u8>> = db
        .query_row(
            "select value from json_blobs where key = 'audio_settings'",
            [],
            |row| Ok(row.get_ref(0)?.as_bytes().ok().map(<[u8]>::to_vec)),
        )
        .unwrap_or(None);
    if let Some(audio) = audio.and_then(|json| serde_json::from_slice(&json).ok()) {
        read_audio(&audio, &mut imported);
    }
    imported.nickname = db
        .query_row(
            "select value from AccountData where key = 'Account'",
            [],
            |row| Ok(row.get_ref(0)?.as_bytes().ok().and_then(display_name)),
        )
        .unwrap_or(None);
    // Values aren't always valid UTF-8 despite the column type, so they're read as bytes.
    let mut items = db.prepare("select value from ProtobufItems where key <> 'Checksum'")?;
    let mut rows = items.query([])?;
    while let Some(row) = rows.next()? {
        if let Ok(item) = row.get_ref(0)?.as_bytes()
            && let Some((server, name)) = server_address(item)
            && !imported.servers.contains(&server)
            && imported.servers.len() < MAX_SERVERS
        {
            if let Some(name) = name {
                imported.names.insert(server.clone(), name);
            }
            imported.servers.push(server);
        }
    }
    Ok(imported)
}

fn read_audio(audio: &Value, imported: &mut Imported) {
    let name = |side: &str| {
        audio[side]["device"]["name"]
            .as_str()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
    };
    imported.output = name("0");
    imported.input = name("1");
    let ptt = &audio["1"]["ptt"];
    imported.push_to_talk = ptt["active"].as_bool().unwrap_or(false);
    let delay = &ptt["releaseDelay"];
    if delay["active"].as_bool().unwrap_or(false) {
        imported.ptt_release_ms = delay["ms"].as_u64().unwrap_or(0).min(500) as u16;
    }
}

enum Field<'a> {
    Number(u64),
    Bytes(&'a [u8]),
}

fn varint(bytes: &mut &[u8]) -> Option<u64> {
    let mut value = 0_u64;
    for shift in (0..64).step_by(7) {
        let (first, rest) = bytes.split_first()?;
        *bytes = rest;
        value |= u64::from(first & 0x7f) << shift;
        if first & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

/// A protobuf message's fields, or `None` if it isn't one.
fn fields(mut bytes: &[u8]) -> Option<Vec<(u64, Field<'_>)>> {
    let mut found = Vec::new();
    while !bytes.is_empty() {
        let tag = varint(&mut bytes)?;
        let value = match tag & 7 {
            0 => Field::Number(varint(&mut bytes)?),
            1 => {
                bytes = bytes.get(8..)?;
                continue;
            }
            2 => {
                let length = usize::try_from(varint(&mut bytes)?).ok()?;
                let value = bytes.get(..length)?;
                bytes = &bytes[length..];
                Field::Bytes(value)
            }
            5 => {
                bytes = bytes.get(4..)?;
                continue;
            }
            _ => return None,
        };
        found.push((tag >> 3, value));
    }
    Some(found)
}

fn display_name(account: &[u8]) -> Option<String> {
    let fields = fields(account)?;
    let name = fields.iter().find_map(|(number, field)| match field {
        Field::Bytes(name) if *number == ACCOUNT_DISPLAY_NAME => Some(*name),
        _ => None,
    })?;
    let name = std::str::from_utf8(name).ok()?.trim();
    let plausible = !name.is_empty()
        && name.chars().count() <= MAX_NAME_CHARS
        && !name.contains('@')
        && !name.chars().any(char::is_control);
    plausible.then(|| name.to_owned())
}

/// A server bookmark's address, with `:port` when it isn't the default, and its name if it has one.
fn server_address(item: &[u8]) -> Option<(String, Option<String>)> {
    let top = fields(item)?;
    let bookmark = top.iter().find_map(|(number, field)| match field {
        Field::Bytes(bookmark) if *number == SERVER_BOOKMARK => Some(*bookmark),
        _ => None,
    })?;
    let bookmark = fields(bookmark)?;
    let mut address = None;
    let mut name = None;
    let mut port = DEFAULT_PORT;
    for (number, field) in &bookmark {
        match (*number, field) {
            (BOOKMARK_NAME, Field::Bytes(value)) => {
                name = std::str::from_utf8(value)
                    .ok()
                    .map(|name| {
                        name.chars()
                            .filter(|c| !c.is_control())
                            .take(64)
                            .collect::<String>()
                    })
                    .map(|name| name.trim().to_owned())
                    .filter(|name| !name.is_empty());
            }
            (BOOKMARK_ADDRESS, Field::Bytes(value)) => address = std::str::from_utf8(value).ok(),
            (BOOKMARK_PORT, Field::Number(value)) => port = *value,
            _ => {}
        }
    }
    let address = address?.trim();
    let plausible = !address.is_empty()
        && address.len() <= 255
        && address
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']'));
    if !plausible {
        return None;
    }
    let address = if port == DEFAULT_PORT || port == 0 || port > 65535 {
        address.to_owned()
    } else {
        format!("{address}:{port}")
    };
    // A bookmark named after its own address has no name worth showing.
    let name = name.filter(|name| !name.eq_ignore_ascii_case(&address));
    Some((address, name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(number: u8, wire: u8) -> u8 {
        (number << 3) | wire
    }

    fn text(number: u8, value: &[u8]) -> Vec<u8> {
        let mut out = vec![tag(number, 2), value.len() as u8];
        out.extend_from_slice(value);
        out
    }

    /// Port as a two-byte varint, as ports above 127 always are.
    fn bookmark(address: &str, port: u16) -> Vec<u8> {
        let mut inner = text(1, b"A name");
        inner.extend(text(BOOKMARK_ADDRESS as u8, address.as_bytes()));
        inner.extend([
            tag(BOOKMARK_PORT as u8, 0),
            (port & 0x7f) as u8 | 0x80,
            (port >> 7) as u8,
        ]);
        inner.extend(text(4, b"not read"));
        let mut outer = text(2, b"bookmark-id");
        outer.extend([
            tag(SERVER_BOOKMARK as u8, 2) | 0x80,
            0x01,
            inner.len() as u8,
        ]);
        outer.extend(inner);
        outer
    }

    fn database(root: &Path) -> PathBuf {
        let path = root.join("settings.db");
        let db = Connection::open(&path).unwrap();
        db.execute_batch(
            "create table json_blobs (timestamp integer, key varchar, value varchar);
             create table ProtobufItems (timestamp integer, key varchar, value varchar);
             create table AccountData (timestamp integer, key varchar, value varchar);",
        )
        .unwrap();
        path
    }

    fn folder(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("flexspeak-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn carries_over_servers_devices_and_push_to_talk() {
        let root = folder("ts6-import");
        let path = database(&root);
        let db = Connection::open(&path).unwrap();
        let audio = r#"{
            "0": {"device": {"id": "x", "name": "Studio Speakers"}},
            "1": {"device": {"id": "y", "name": "Desk Mic"},
                  "ptt": {"active": true, "releaseDelay": {"active": true, "ms": 900}}}
        }"#;
        db.execute(
            "insert into json_blobs values (0, 'audio_settings', ?1)",
            [audio],
        )
        .unwrap();
        for (key, address, port) in [
            ("1", "voice.example.org", 9987_u16),
            ("2", "games.example.org", 1234),
            ("3", "voice.example.org", 9987),
        ] {
            db.execute(
                "insert into ProtobufItems values (0, ?1, ?2)",
                rusqlite::params![key, bookmark(address, port)],
            )
            .unwrap();
        }
        // Not protobuf, a bookmark of another kind, and the checksum row: all skipped.
        for (key, value) in [
            ("4", vec![0xff, 0xff, 0xff]),
            ("5", text(17, b"\x0a\x03abc")),
            ("Checksum", vec![1, 2, 3]),
        ] {
            db.execute(
                "insert into ProtobufItems values (0, ?1, ?2)",
                rusqlite::params![key, value],
            )
            .unwrap();
        }
        let mut account = text(1, b"person@example.org");
        account.extend(text(2, b"secret-login-value"));
        account.extend(text(3, b"1234-uuid"));
        account.extend(text(ACCOUNT_DISPLAY_NAME as u8, b"Display Name"));
        db.execute(
            "insert into AccountData values (0, 'Account', ?1)",
            [account],
        )
        .unwrap();
        drop(db);
        let imported = read(&path).unwrap();
        let everything = format!("{imported:?}");
        assert!(!everything.contains("secret") && !everything.contains("person@"));
        assert_eq!(
            imported,
            Imported {
                nickname: Some("Display Name".into()),
                servers: vec!["voice.example.org".into(), "games.example.org:1234".into()],
                names: [
                    ("voice.example.org".to_owned(), "A name".to_owned()),
                    ("games.example.org:1234".to_owned(), "A name".to_owned()),
                ]
                .into(),
                input: Some("Desk Mic".into()),
                output: Some("Studio Speakers".into()),
                push_to_talk: true,
                ptt_release_ms: 500,
            }
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_empty_or_foreign_database_carries_nothing_over() {
        let root = folder("ts6-empty");
        let path = database(&root);
        assert_eq!(read(&path).unwrap(), Imported::default());
        let not_a_database = root.join("other.db");
        std::fs::write(&not_a_database, b"not sqlite").unwrap();
        assert!(read(&not_a_database).is_err());
        let _ = std::fs::remove_dir_all(root);
    }
}
