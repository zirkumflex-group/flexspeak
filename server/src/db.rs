//! The one SQLite file and its tables.

use std::path::Path;

use rusqlite::Connection;

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
"#;

/// Opens (and if need be creates) `monster.sqlite` in `dir`.
pub fn open(dir: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(dir.join("monster.sqlite"))?;
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
