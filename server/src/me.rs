//! Sessions, and the signed-in account.

use axum::{extract::FromRequestParts, http::StatusCode, http::request::Parts};
use monster_core::api::{LinkedIdentity, Me, UpdateMe};
use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    AppState,
    error::{ApiError, ApiResult, Body},
    secret,
};
use axum::{Json, extract::State};

/// How long a session lasts without being used.
pub const SESSION_SECONDS: i64 = 30 * 24 * 3600;

/// The account a request's session belongs to. Using a session renews it.
pub struct Caller {
    pub user: i64,
    session_hash: Vec<u8>,
}

impl FromRequestParts<AppState> for Caller {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> ApiResult<Self> {
        let token = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .ok_or_else(ApiError::signed_out)?;
        let session_hash = secret::sha256(token.as_bytes());
        let now = state.now();
        let db = state.db();
        let user = db
            .query_row(
                "UPDATE sessions SET expires_at = ?1
                 WHERE token_hash = ?2 AND expires_at > ?3
                 RETURNING user_id",
                params![now + SESSION_SECONDS, session_hash, now],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(ApiError::signed_out)?;
        Ok(Self { user, session_hash })
    }
}

/// Starts a session for `user` and gives back its token, which is stored only as a hash.
pub fn new_session(conn: &Connection, user: i64, now: i64) -> rusqlite::Result<String> {
    let token = secret::token();
    conn.execute(
        "INSERT INTO sessions (token_hash, user_id, expires_at) VALUES (?1, ?2, ?3)",
        params![
            secret::sha256(token.as_bytes()),
            user,
            now + SESSION_SECONDS
        ],
    )?;
    Ok(token)
}

pub fn load_me(conn: &Connection, user: i64) -> rusqlite::Result<Me> {
    let mut me = conn.query_row(
        "SELECT id, name, email, hidden, imported FROM users WHERE id = ?1",
        [user],
        |row| {
            Ok(Me {
                id: row.get(0)?,
                name: row.get(1)?,
                email: row.get(2)?,
                hidden: row.get(3)?,
                imported: row.get(4)?,
                identities: Vec::new(),
            })
        },
    )?;
    let mut stmt = conn.prepare(
        "SELECT uid, source, linked_at FROM identities WHERE user_id = ?1 ORDER BY linked_at, uid",
    )?;
    me.identities = stmt
        .query_map([user], |row| {
            Ok(LinkedIdentity {
                uid: row.get(0)?,
                source: row.get(1)?,
                linked_at: row.get(2)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(me)
}

pub async fn get_me(State(state): State<AppState>, caller: Caller) -> ApiResult<Json<Me>> {
    Ok(Json(load_me(&state.db(), caller.user)?))
}

/// The longest name the leaderboard shows, in characters.
const NAME_MAX: usize = 32;

pub async fn update_me(
    State(state): State<AppState>,
    caller: Caller,
    Body(update): Body<UpdateMe>,
) -> ApiResult<Json<Me>> {
    let name = match update.name {
        Some(name) => {
            let name = name.trim().to_string();
            if name.is_empty() || name.chars().count() > NAME_MAX {
                return Err(ApiError::invalid(format!(
                    "A name is 1 to {NAME_MAX} characters."
                )));
            }
            if name.chars().any(char::is_control) {
                return Err(ApiError::invalid("A name can't hold control characters."));
            }
            Some(name)
        }
        None => None,
    };
    let db = state.db();
    if let Some(name) = name {
        db.execute(
            "UPDATE users SET name = ?1 WHERE id = ?2",
            params![name, caller.user],
        )?;
    }
    if let Some(hidden) = update.hidden {
        db.execute(
            "UPDATE users SET hidden = ?1 WHERE id = ?2",
            params![hidden, caller.user],
        )?;
    }
    Ok(Json(load_me(&db, caller.user)?))
}

/// Removes the account; its sessions, passkeys, identities, cans and tickets go with it.
pub async fn delete_me(State(state): State<AppState>, caller: Caller) -> ApiResult<StatusCode> {
    let mut db = state.db();
    let tx = db.transaction()?;
    let email: String = tx.query_row(
        "SELECT email FROM users WHERE id = ?1",
        [caller.user],
        |row| row.get(0),
    )?;
    tx.execute("DELETE FROM login_codes WHERE email = ?1", [&email])?;
    tx.execute(
        "DELETE FROM hits WHERE bucket = ?1",
        [format!("code-email:{email}")],
    )?;
    tx.execute("DELETE FROM users WHERE id = ?1", [caller.user])?;
    tx.commit()?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn sign_out(State(state): State<AppState>, caller: Caller) -> ApiResult<StatusCode> {
    state.db().execute(
        "DELETE FROM sessions WHERE token_hash = ?1",
        [&caller.session_hash],
    )?;
    Ok(StatusCode::NO_CONTENT)
}
