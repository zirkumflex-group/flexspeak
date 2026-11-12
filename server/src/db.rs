//! The one SQLite file and its tables.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension};

/// The database's file name inside the data folder.
pub const FILE: &str = "monster.sqlite";

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY,
    email TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    hidden INTEGER NOT NULL DEFAULT 0,
    imported INTEGER NOT NULL DEFAULT 0,
    -- The secret every roll of this person's cans is made with. Never leaves the server.
    roll_key BLOB NOT NULL,
    -- What a passkey stores to say whose it is.
    handle TEXT NOT NULL UNIQUE,
    -- The last can number handed out, so a removed can's number is never given again.
    last_can INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS login_codes (
    email TEXT PRIMARY KEY,
    code_hash BLOB NOT NULL,
    expires_at INTEGER NOT NULL,
    tries_left INTEGER NOT NULL
);

-- One row for each thing that is rate limited, such as a code sent to an address.
CREATE TABLE IF NOT EXISTS hits (
    bucket TEXT NOT NULL,
    at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS hits_bucket ON hits (bucket, at);

CREATE TABLE IF NOT EXISTS sessions (
    token_hash BLOB PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    expires_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS tickets (
    ticket_hash BLOB PRIMARY KEY,
    -- 'create': make a passkey for user_id. 'sign_in': use a passkey; user_id is whose it was.
    kind TEXT NOT NULL,
    user_id INTEGER REFERENCES users (id) ON DELETE CASCADE,
    done INTEGER NOT NULL DEFAULT 0,
    expires_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS passkeys (
    credential_id BLOB PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    passkey TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    last_used_at INTEGER
);

CREATE TABLE IF NOT EXISTS challenges (
    challenge TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    expires_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS identities (
    uid TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    public_key TEXT NOT NULL,
    source TEXT NOT NULL,
    linked_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS cans (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    number INTEGER NOT NULL,
    flavour TEXT NOT NULL,
    at INTEGER NOT NULL,
    rarity TEXT NOT NULL,
    seed INTEGER NOT NULL,
    legacy INTEGER NOT NULL DEFAULT 0,
    removed_at INTEGER,
    add_key TEXT,
    created_at INTEGER NOT NULL,
    UNIQUE (user_id, number),
    UNIQUE (user_id, add_key)
);

-- The account's settings as the app last saved them: JSON text the server never looks inside.
CREATE TABLE IF NOT EXISTS settings (
    user_id INTEGER PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    version INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    data TEXT NOT NULL
);
"#;

/// Opens (and if need be creates) `monster.sqlite` in `dir`.
pub fn open(dir: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(dir.join(FILE))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.execute_batch(SCHEMA)?;
    Ok(conn)
}

/// Drops rows that have run out, so the small tables stay small.
pub fn sweep(conn: &Connection, now: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM login_codes WHERE expires_at <= ?1", [now])?;
    conn.execute("DELETE FROM hits WHERE at <= ?1", [now - 3600])?;
    conn.execute("DELETE FROM sessions WHERE expires_at <= ?1", [now])?;
    conn.execute("DELETE FROM tickets WHERE expires_at <= ?1", [now])?;
    conn.execute("DELETE FROM challenges WHERE expires_at <= ?1", [now])?;
    Ok(())
}

/// Writes a consistent copy of the database in `dir` to `dest`, which must not exist yet,
/// and checks that the copy opens and is whole. Safe while the server is running: SQLite's
/// `VACUUM INTO` reads one snapshot, including what is still in the write-ahead log, which a
/// plain copy of the file would miss or tear.
pub fn backup(dir: &Path, dest: &Path) -> Result<(), String> {
    let source = dir.join(FILE);
    if !source.is_file() {
        return Err(format!("there is no database at {}", source.display()));
    }
    if dest.exists() {
        return Err(format!("{} already exists", dest.display()));
    }
    let dest_text = dest
        .to_str()
        .ok_or_else(|| format!("{} is not a usable file name", dest.display()))?;
    let conn = Connection::open(&source)
        .map_err(|e| format!("could not open {}: {e}", source.display()))?;
    conn.busy_timeout(std::time::Duration::from_secs(30))
        .map_err(|e| e.to_string())?;
    conn.execute("VACUUM INTO ?1", [dest_text])
        .map_err(|e| format!("could not write {}: {e}", dest.display()))?;
    drop(conn);

    let copy = Connection::open(dest).map_err(|e| format!("the copy does not open: {e}"))?;
    let check: String = copy
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(|e| format!("the copy could not be checked: {e}"))?;
    if check != "ok" {
        return Err(format!("the copy is damaged: {check}"));
    }
    Ok(())
}

/// Removes the account with `email` and everything in it, as `DELETE /v1/me` does, for a
/// server owner answering a request to delete someone's data. Safe while the server is
/// running. Answers whether there was such an account.
pub fn delete_account(dir: &Path, email: &str) -> Result<bool, String> {
    let source = dir.join(FILE);
    if !source.is_file() {
        return Err(format!("there is no database at {}", source.display()));
    }
    let email = email.trim().to_lowercase();
    let mut conn = open(dir).map_err(|e| format!("could not open {}: {e}", source.display()))?;
    let user: Option<i64> = conn
        .query_row("SELECT id FROM users WHERE email = ?1", [&email], |row| {
            row.get(0)
        })
        .optional()
        .map_err(|e| e.to_string())?;
    let Some(user) = user else {
        return Ok(false);
    };
    crate::me::delete_account(&mut conn, user).map_err(|e| e.to_string())?;
    Ok(true)
}
