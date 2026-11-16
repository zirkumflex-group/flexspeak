//! Whisper lists from TeamSpeak 3: a `.whisperlist` file it exported, or its settings.
//!
//! Both hold TeamSpeak's sync items, the protobuf records its settings database keeps
//! (`Item_Data`; a whisper list is field 18). An exported file is one item written out as text:
//! every byte was read as Latin-1 and saved as UTF-8, so a byte from 0x80 up takes two. The
//! settings also hold the hotkey profiles, whose keys say which shortcut starts a list, and
//! the bookmarks, which say which server address a list's server ID belongs to. TeamSpeak 6
//! keeps the same items, so its settings are read the same way.
//!
//! Only lists, their shortcuts and bookmark addresses are read. Identities, passwords and
//! everything else in an item are skipped and never kept. Anything that doesn't look as
//! expected is skipped too: TeamSpeak doesn't document this format.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use base64::Engine as _;
use rusqlite::{Connection, OpenFlags};

use crate::ts6_import::{Field, fields, server_address};
use crate::whisper::{Entry, Group, GroupKind, List, Scope, Whom};

const ITEM_UUID: u64 = 2;
const ITEM_BOOKMARK: u64 = 16;
const ITEM_WHISPER_LIST: u64 = 18;
const ITEM_HOTKEY_PROFILE: u64 = 19;
const BOOKMARK_SERVER_UID: u64 = 15;
/// TeamSpeak's limit on a list's name, and enough for any real list.
const MAX_NAME_CHARS: usize = 64;
const MAX_ENTRIES: usize = 256;
const MAX_LISTS: usize = 64;
/// An exported file is a few hundred bytes; anything this big is something else.
pub const MAX_FILE_BYTES: u64 = 1 << 20;

/// A list as TeamSpeak 3 had it, with what the app can't take over by itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// `server` is set when the settings name the server's address.
    pub list: List,
    /// The server's unique ID, from the list or its channels; empty when it names none.
    pub server_uid: String,
    /// The server's name as TeamSpeak showed it, when it kept one.
    pub server_name: String,
    /// The list's shortcut there that has no name here, such as a mouse button or a key on
    /// another system, as TeamSpeak wrote it.
    pub unmapped_key: Option<String>,
}

/// TeamSpeak 3's settings on this computer, and TeamSpeak 6's, where they are.
pub fn settings_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(dirs) = directories::BaseDirs::new() {
        if cfg!(windows) {
            paths.push(dirs.config_dir().join("TS3Client/settings.db"));
        } else if cfg!(target_os = "macos") {
            paths.push(dirs.data_dir().join("TeamSpeak 3/settings.db"));
        } else {
            paths.push(dirs.home_dir().join(".ts3client/settings.db"));
        }
    }
    paths.extend(crate::ts6_import::path());
    paths.retain(|path| path.is_file());
    paths
}

/// The lists in an exported `.whisperlist` file.
pub fn read_file(path: &Path) -> Result<Vec<Found>> {
    let size = std::fs::metadata(path)
        .context("Cannot read the whisper list")?
        .len();
    if size > MAX_FILE_BYTES {
        bail!("That file is too big to be a TeamSpeak whisper list.");
    }
    let bytes = std::fs::read(path).context("Cannot read the whisper list")?;
    let found = parse_file(&bytes);
    if found.is_empty() {
        bail!("That file isn't a TeamSpeak whisper list.");
    }
    Ok(found)
}

/// An exported file's lists. It is tried as the text TeamSpeak writes first, then as bare
/// protobuf, in case a list was saved some other way.
pub fn parse_file(bytes: &[u8]) -> Vec<Found> {
    let mut found = demangle(bytes)
        .and_then(|item| whisper_list(&item))
        .or_else(|| whisper_list(bytes))
        .map(|(found, _)| vec![found])
        .unwrap_or_default();
    found.truncate(MAX_LISTS);
    found
}

/// The bytes behind TeamSpeak's text export: each character is one byte.
fn demangle(bytes: &[u8]) -> Option<Vec<u8>> {
    std::str::from_utf8(bytes)
        .ok()?
        .chars()
        .map(|c| u8::try_from(u32::from(c)).ok())
        .collect()
}

/// The lists in TeamSpeak's settings, with their shortcuts and servers.
pub fn read_settings(path: &Path) -> Result<Vec<Found>> {
    let db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .context("Cannot open TeamSpeak's settings")?;
    // TeamSpeak may be running and writing.
    db.busy_timeout(Duration::from_secs(1))?;
    let mut statement = db.prepare("select value from ProtobufItems where key <> 'Checksum'")?;
    let mut rows = statement.query([])?;
    let mut items = Vec::new();
    while let Some(row) = rows.next()? {
        // Values aren't always valid UTF-8 despite the column type, so they're read as bytes.
        if let Ok(item) = row.get_ref(0)?.as_bytes() {
            items.push(item.to_vec());
        }
    }
    Ok(from_items(&items))
}

/// What a set of sync items holds: lists, given their shortcuts and servers from the rest.
fn from_items(items: &[Vec<u8>]) -> Vec<Found> {
    let mut lists: Vec<(Found, Vec<String>)> = Vec::new();
    let mut servers: Vec<(String, String)> = Vec::new();
    let mut hotkeys: Vec<Hotkey> = Vec::new();
    for item in items {
        if let Some(list) = whisper_list(item) {
            if lists.len() < MAX_LISTS {
                lists.push(list);
            }
        } else if let Some(server) = bookmark_server(item) {
            servers.push(server);
        } else {
            hotkeys.extend(hotkey_profile(item));
        }
    }
    lists
        .into_iter()
        .map(|(mut found, ids)| {
            if let Some((_, address)) = servers
                .iter()
                .find(|(uid, _)| !uid.is_empty() && *uid == found.server_uid)
            {
                found.list.server = address.clone();
            }
            if let Some(hotkey) = hotkeys.iter().find(|hotkey| ids.contains(&hotkey.target)) {
                match key_name(&hotkey.keys, hotkey.windows) {
                    Some(key) => found.list.key = key,
                    None => found.unmapped_key = Some(hotkey.keys.join(" + ")),
                }
                found.list.toggle = hotkey.toggle;
            }
            found
        })
        .collect()
}

fn text(value: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(value).ok()?;
    Some(text.chars().filter(|c| !c.is_control()).collect())
}

fn bytes_at<'a>(fields: &[(u64, Field<'a>)], number: u64) -> Option<&'a [u8]> {
    fields.iter().find_map(|(at, field)| match field {
        Field::Bytes(value) if *at == number => Some(*value),
        _ => None,
    })
}

fn text_at(fields: &[(u64, Field<'_>)], number: u64) -> String {
    bytes_at(fields, number).and_then(text).unwrap_or_default()
}

fn number_at(fields: &[(u64, Field<'_>)], number: u64) -> Option<u64> {
    fields.iter().find_map(|(at, field)| match field {
        Field::Number(value) if *at == number => Some(*value),
        _ => None,
    })
}

/// TeamSpeak's base64 identity as the app keys it (`client::uid_key`).
fn uid_key(uid: &str) -> Option<String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(uid.trim())
        .ok()?;
    (!bytes.is_empty()).then(|| crate::client::uid_key(&bytes))
}

/// A sync item's whisper list, with the IDs a hotkey may point at it by.
fn whisper_list(item: &[u8]) -> Option<(Found, Vec<String>)> {
    let top = fields(item)?;
    let data = fields(bytes_at(&top, ITEM_WHISPER_LIST)?)?;
    let uuid = text_at(&data, 1);
    let ids: Vec<String> = [uuid.clone(), text_at(&top, ITEM_UUID)]
        .into_iter()
        .filter(|id| !id.is_empty())
        .collect();
    let name: String = text_at(&data, 2)
        .trim()
        .chars()
        .take(MAX_NAME_CHARS)
        .collect();
    let mut server_uid = text_at(&data, 8).trim().to_owned();
    let server_name = text_at(&data, 9).trim().to_owned();
    // 1 is a list of people and channels, 2 one of TeamSpeak's group targets.
    let to = match number_at(&data, 3).unwrap_or(1) {
        2 => {
            let signed = |n: u64| n as u32 as i32 as i64;
            Whom::Group(Group {
                kind: GroupKind::from_wire(signed(number_at(&data, 4).unwrap_or(0)))?,
                scope: Scope::from_wire(signed(number_at(&data, 5).unwrap_or(0)))
                    .unwrap_or_default(),
                id: number_at(&data, 6).unwrap_or(0),
            })
        }
        _ => {
            let mut entries = Vec::new();
            for (number, field) in &data {
                let (10, Field::Bytes(entry)) = (*number, field) else {
                    continue;
                };
                let Some(entry) = fields(entry) else { continue };
                let name = text_at(&entry, 2);
                let uid = text_at(&entry, 3);
                match number_at(&entry, 1) {
                    Some(1) => {
                        if let Some(uid) = uid_key(&uid) {
                            entries.push(Entry::Client {
                                uid,
                                name: name.trim().chars().take(MAX_NAME_CHARS).collect(),
                            });
                        }
                    }
                    // A channel is kept by path; the ID beside it is its server's.
                    Some(2) if !name.trim().is_empty() => {
                        if server_uid.is_empty() {
                            server_uid = uid.trim().to_owned();
                        }
                        entries.push(Entry::Channel {
                            path: name,
                            subchannels: false,
                        });
                    }
                    _ => {}
                }
                if entries.len() >= MAX_ENTRIES {
                    break;
                }
            }
            Whom::Listed(entries)
        }
    };
    let id = if uuid.is_empty() {
        ids.first().cloned().unwrap_or_default()
    } else {
        uuid
    };
    let mut list = List::new(
        id,
        if name.is_empty() {
            "Whisper list".into()
        } else {
            name
        },
    );
    list.to = to;
    Some((
        Found {
            list,
            server_uid,
            server_name,
            unmapped_key: None,
        },
        ids,
    ))
}

/// A bookmark's server ID and address.
fn bookmark_server(item: &[u8]) -> Option<(String, String)> {
    let top = fields(item)?;
    let bookmark = fields(bytes_at(&top, ITEM_BOOKMARK)?)?;
    let uid = text_at(&bookmark, BOOKMARK_SERVER_UID).trim().to_owned();
    let (address, _) = server_address(item)?;
    Some((uid, address))
}

struct Hotkey {
    /// The list's ID it starts.
    target: String,
    keys: Vec<String>,
    windows: bool,
    toggle: bool,
}

fn hotkey_profile(item: &[u8]) -> Vec<Hotkey> {
    let Some(top) = fields(item) else {
        return Vec::new();
    };
    let Some(profile) = bytes_at(&top, ITEM_HOTKEY_PROFILE).and_then(fields) else {
        return Vec::new();
    };
    profile
        .iter()
        .filter_map(|(number, field)| match (*number, field) {
            (3, Field::Bytes(hotkey)) => fields(hotkey),
            _ => None,
        })
        .filter_map(|hotkey| {
            let target = text_at(&hotkey, 9).trim().to_owned();
            let keys: Vec<String> = hotkey
                .iter()
                .filter_map(|(number, field)| match (*number, field) {
                    (7, Field::Bytes(key)) => fields(key),
                    _ => None,
                })
                .map(|key| {
                    let key = [text_at(&key, 1), text_at(&key, 2)]
                        .into_iter()
                        .find(|part| part.contains(':'))
                        .unwrap_or_default();
                    key.split('@').next().unwrap_or_default().trim().to_owned()
                })
                .filter(|key| !key.is_empty())
                .collect();
            (!target.is_empty() && !keys.is_empty()).then(|| Hotkey {
                target,
                keys,
                windows: number_at(&hotkey, 3).unwrap_or(0) == 0,
                toggle: text_at(&hotkey, 11).eq_ignore_ascii_case("toggle"),
            })
        })
        .collect()
}

/// TeamSpeak's keys (`Keyboard:29`, DirectInput scan codes on Windows) in the app's shortcut
/// grammar, or `None` for one it can't name, such as a mouse button.
pub fn key_name(keys: &[String], windows: bool) -> Option<String> {
    if !windows || keys.is_empty() {
        return None;
    }
    let (mut ctrl, mut alt, mut shift, mut logo) = (false, false, false, false);
    let mut main = None;
    for key in keys {
        let code: u32 = key.strip_prefix("Keyboard:")?.trim().parse().ok()?;
        match code {
            0x1d | 0x9d => ctrl = true,
            0x38 | 0xb8 => alt = true,
            0x2a | 0x36 => shift = true,
            0xdb | 0xdc => logo = true,
            code => {
                if main.is_some() {
                    return None;
                }
                main = Some(scan_code(code)?);
            }
        }
    }
    let mut parts: Vec<String> = Vec::new();
    if ctrl {
        parts.push("Ctrl".into());
    }
    if alt {
        parts.push("Alt".into());
    }
    if shift {
        parts.push("Shift".into());
    }
    if logo {
        parts.push("Super".into());
    }
    parts.push(main?);
    Some(parts.join("+"))
}

/// A DirectInput scan code's key, named as the shortcut grammar names it.
fn scan_code(code: u32) -> Option<String> {
    const ROWS: [(u32, &str); 4] = [
        (0x10, "QWERTYUIOP"),
        (0x1e, "ASDFGHJKL"),
        (0x2c, "ZXCVBNM"),
        (0x02, "1234567890"),
    ];
    for (start, keys) in ROWS {
        if let Some(key) = code
            .checked_sub(start)
            .and_then(|at| keys.chars().nth(at as usize))
        {
            return Some(key.to_string());
        }
    }
    let name = match code {
        0x01 => "Escape",
        0x0c => "Minus",
        0x0d => "Equal",
        0x0e => "Backspace",
        0x0f => "Tab",
        0x1a => "BracketLeft",
        0x1b => "BracketRight",
        0x1c => "Enter",
        0x27 => "Semicolon",
        0x28 => "Quote",
        0x29 => "Backquote",
        0x2b => "Backslash",
        0x33 => "Comma",
        0x34 => "Period",
        0x35 => "Slash",
        0x37 => "NumpadMultiply",
        0x39 => "Space",
        0x3b..=0x44 => return Some(format!("F{}", code - 0x3a)),
        0x47 => "Numpad7",
        0x48 => "Numpad8",
        0x49 => "Numpad9",
        0x4a => "NumpadSubtract",
        0x4b => "Numpad4",
        0x4c => "Numpad5",
        0x4d => "Numpad6",
        0x4e => "NumpadAdd",
        0x4f => "Numpad1",
        0x50 => "Numpad2",
        0x51 => "Numpad3",
        0x52 => "Numpad0",
        0x53 => "NumpadDecimal",
        0x57 => "F11",
        0x58 => "F12",
        0x64..=0x66 => return Some(format!("F{}", code - 0x64 + 13)),
        0x9c => "NumpadEnter",
        0xb5 => "NumpadDivide",
        0xc7 => "Home",
        0xc8 => "ArrowUp",
        0xc9 => "PageUp",
        0xcb => "ArrowLeft",
        0xcd => "ArrowRight",
        0xcf => "End",
        0xd0 => "ArrowDown",
        0xd1 => "PageDown",
        0xd2 => "Insert",
        0xd3 => "Delete",
        _ => return None,
    };
    Some(name.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A list TeamSpeak 3 exported: two channels on one server.
    const EXPORTED: &[u8] = include_bytes!("../tests/fixtures/POL20THW.whisperlist");
    const SERVER: &str = "434vwrnukODFTcJmqk7utCDpPdQ=";

    fn tag(number: u64, wire: u64) -> Vec<u8> {
        varint((number << 3) | wire)
    }

    fn varint(mut value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let byte = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                out.push(byte);
                return out;
            }
            out.push(byte | 0x80);
        }
    }

    fn bytes(number: u64, value: &[u8]) -> Vec<u8> {
        let mut out = tag(number, 2);
        out.extend(varint(value.len() as u64));
        out.extend_from_slice(value);
        out
    }

    fn number(number: u64, value: u64) -> Vec<u8> {
        let mut out = tag(number, 0);
        out.extend(varint(value));
        out
    }

    fn item(field: u64, data: Vec<u8>) -> Vec<u8> {
        let mut out = bytes(ITEM_UUID, b"item-uuid");
        out.extend(bytes(field, &data));
        out
    }

    #[test]
    fn reads_the_list_teamspeak_3_exported() {
        let found = parse_file(EXPORTED);
        assert_eq!(found.len(), 1);
        let found = &found[0];
        assert_eq!(found.list.id, "b2538cbe-f762-7045-8532-292de2e54a04");
        assert_eq!(found.list.name, "POL THW");
        assert_eq!(found.server_uid, SERVER);
        assert_eq!(found.list.server, "");
        assert_eq!(
            found.list.to,
            Whom::Listed(vec![
                Entry::Channel {
                    path: "[cspacer42]Polizeistreifen - ALTIS/Leitstelle - Altis".into(),
                    subchannels: false,
                },
                Entry::Channel {
                    path: "[cspacer46]THW Einsatz/Einsatzleitung - ALTIS".into(),
                    subchannels: false,
                },
            ])
        );
    }

    #[test]
    fn rejects_what_isnt_a_list() {
        assert!(parse_file(b"").is_empty());
        assert!(parse_file(b"hello world").is_empty());
        assert!(parse_file(&EXPORTED[..40]).is_empty());
        let dir = std::env::temp_dir().join(format!("flexspeak-ts3-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("not.whisperlist");
        std::fs::write(&path, b"{}").unwrap();
        assert!(read_file(&path).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn settings_give_lists_their_shortcuts_servers_and_people() {
        let mut person = number(1, 1);
        person.extend(bytes(2, b"Mara"));
        person.extend(bytes(3, b"AAECAwQ="));
        let mut list = bytes(1, b"list-1");
        list.extend(bytes(2, b"Raid leads"));
        list.extend(number(3, 1));
        list.extend(bytes(8, SERVER.as_bytes()));
        list.extend(bytes(10, &person));
        let mut group = bytes(1, b"list-2");
        group.extend(bytes(2, b"Officers"));
        group.extend(number(3, 2));
        group.extend(number(4, 0));
        group.extend(number(5, 1));
        group.extend(number(6, 12));
        let mut mouse = bytes(1, b"list-3");
        mouse.extend(bytes(2, b"Mouse"));
        let mut bookmark = bytes(1, b"Altis");
        bookmark.extend(bytes(2, b"ts.example.com"));
        bookmark.extend(bytes(BOOKMARK_SERVER_UID, SERVER.as_bytes()));
        let hotkey = |target: &[u8], keys: &[&[u8]], mode: &[u8]| {
            let mut out = number(3, 0);
            for key in keys {
                out.extend(bytes(7, &bytes(1, key)));
            }
            out.extend(bytes(9, target));
            out.extend(bytes(11, mode));
            bytes(3, &out)
        };
        let mut profile = bytes(1, b"profile");
        profile.extend(hotkey(
            b"list-1",
            &[b"Keyboard:29", b"Keyboard:42@0", b"Keyboard:2"],
            b"Activate",
        ));
        profile.extend(hotkey(b"list-2", &[b"Keyboard:59"], b"Toggle"));
        profile.extend(hotkey(b"list-3", &[b"Mouse:4"], b""));
        let found = from_items(&[
            item(ITEM_WHISPER_LIST, list),
            item(ITEM_WHISPER_LIST, group),
            item(ITEM_WHISPER_LIST, mouse),
            item(ITEM_BOOKMARK, bookmark),
            item(ITEM_HOTKEY_PROFILE, profile),
            b"not protobuf \xff".to_vec(),
        ]);
        assert_eq!(found.len(), 3);
        assert_eq!(found[0].list.server, "ts.example.com");
        assert_eq!(found[0].list.key, "Ctrl+Shift+1");
        assert!(!found[0].list.toggle);
        assert_eq!(
            found[0].list.to,
            Whom::Listed(vec![Entry::Client {
                uid: "0001020304".into(),
                name: "Mara".into()
            }])
        );
        assert_eq!(found[1].list.key, "F1");
        assert!(found[1].list.toggle);
        assert_eq!(
            found[1].list.to,
            Whom::Group(Group {
                kind: GroupKind::ServerGroup,
                scope: Scope::MyChannel,
                id: 12
            })
        );
        assert_eq!(found[2].list.key, "");
        assert_eq!(found[2].unmapped_key.as_deref(), Some("Mouse:4"));
    }

    #[test]
    fn reads_lists_from_a_settings_database() {
        let dir = std::env::temp_dir().join(format!("flexspeak-ts3-db-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.db");
        let db = Connection::open(&path).unwrap();
        db.execute_batch(
            "create table ProtobufItems (timestamp integer, key varchar, value varchar);",
        )
        .unwrap();
        let exported = demangle(EXPORTED).unwrap();
        db.execute(
            "insert into ProtobufItems values (0, '1', ?1), (0, 'Checksum', 'x')",
            rusqlite::params![exported],
        )
        .unwrap();
        drop(db);
        let found = read_settings(&path).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].list.name, "POL THW");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn scan_codes_become_shortcuts_global_hotkey_accepts() {
        let key = |codes: &[u32]| {
            key_name(
                &codes
                    .iter()
                    .map(|code| format!("Keyboard:{code}"))
                    .collect::<Vec<_>>(),
                true,
            )
        };
        assert_eq!(key(&[0x21]).as_deref(), Some("F"));
        assert_eq!(key(&[0x1d, 0x38, 0x3b]).as_deref(), Some("Ctrl+Alt+F1"));
        assert_eq!(key(&[0x1d]), None);
        assert_eq!(key(&[0x21, 0x22]), None);
        assert_eq!(key_name(&["Keyboard:33".into()], false), None);
        for code in (0x01..=0xdc).filter_map(scan_code) {
            code.parse::<global_hotkey::hotkey::HotKey>()
                .unwrap_or_else(|error| panic!("{code}: {error}"));
        }
    }
}
