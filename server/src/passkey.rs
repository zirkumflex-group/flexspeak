//! The browser page that creates or uses a passkey, and the two calls it makes.
//!
//! The app holds a ticket and opens `/passkey#ticket=...`. The page sends the ticket to
//! `POST /v1/passkeys/options`, runs the browser's passkey dialog with what comes back, and
//! sends the result to `POST /v1/passkeys/finish`. That marks the ticket done, and the app's
//! next `login/poll` gets the session. These two calls are between the page and the server
//! only; the app never makes them.

use axum::{
    Json,
    extract::State,
    http::{StatusCode, header},
    response::IntoResponse,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use webauthn_rs::prelude::{
    DiscoverableAuthentication, Passkey, PasskeyRegistration, PublicKeyCredential,
    RegisterPublicKeyCredential, Uuid,
};

use crate::{
    AppState,
    error::{ApiError, ApiResult, Body},
    secret,
};

/// How long a ticket works.
pub const TICKET_SECONDS: i64 = 15 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketKind {
    /// After a sign-in code: make a passkey for the account.
    Create,
    /// Sign in with a passkey made earlier; whose it is comes out of the ceremony.
    SignIn,
}

impl TicketKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::SignIn => "sign_in",
        }
    }
}

/// Makes a ticket and gives it back; only its hash is stored.
pub fn new_ticket(
    conn: &Connection,
    kind: TicketKind,
    user: Option<i64>,
    now: i64,
) -> rusqlite::Result<String> {
    let ticket = secret::token();
    conn.execute(
        "INSERT INTO tickets (ticket_hash, kind, user_id, expires_at) VALUES (?1, ?2, ?3, ?4)",
        params![
            secret::sha256(ticket.as_bytes()),
            kind.as_str(),
            user,
            now + TICKET_SECONDS
        ],
    )?;
    Ok(ticket)
}

/// A passkey dialog the page has been given options for and not yet finished.
pub struct Ceremony {
    started: i64,
    state: CeremonyState,
}

enum CeremonyState {
    Create(PasskeyRegistration),
    SignIn(DiscoverableAuthentication),
}

/// A pending ticket: its kind and, for `Create`, whose.
fn pending_ticket(
    conn: &Connection,
    hash: &[u8],
    now: i64,
) -> ApiResult<(TicketKind, Option<i64>)> {
    let row: Option<(String, Option<i64>, bool, i64)> = conn
        .query_row(
            "SELECT kind, user_id, done, expires_at FROM tickets WHERE ticket_hash = ?1",
            [hash],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    match row {
        Some((kind, user, false, expires_at)) if expires_at > now => {
            let kind = if kind == "create" {
                TicketKind::Create
            } else {
                TicketKind::SignIn
            };
            Ok((kind, user))
        }
        Some((_, _, true, _)) => Err(ApiError::expired(
            "This passkey step is already done. Go back to Flexspeak.",
        )),
        _ => Err(ApiError::expired(
            "This link has run out. Start again in Flexspeak.",
        )),
    }
}

#[derive(Deserialize)]
pub struct TicketBody {
    ticket: String,
}

#[derive(Serialize)]
pub struct Options {
    /// `create` or `get`: which browser call to make.
    mode: &'static str,
    /// What to pass to `navigator.credentials.create` or `.get` as `publicKey`, with every
    /// binary value as base64url.
    #[serde(rename = "publicKey")]
    public_key: Value,
}

pub async fn options(
    State(state): State<AppState>,
    Body(req): Body<TicketBody>,
) -> ApiResult<Json<Options>> {
    let now = state.now();
    let hash = secret::sha256(req.ticket.as_bytes());
    let (mode, public_key, ceremony) = {
        let db = state.db();
        match pending_ticket(&db, &hash, now)? {
            (TicketKind::Create, Some(user)) => {
                let (email, name, handle): (String, String, String) = db.query_row(
                    "SELECT email, name, handle FROM users WHERE id = ?1",
                    [user],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )?;
                let handle = Uuid::parse_str(&handle).map_err(|_| ApiError::server())?;
                let existing = user_passkeys(&db, user)?
                    .iter()
                    .map(|(_, p)| p.cred_id().clone())
                    .collect::<Vec<_>>();
                let (ccr, reg) = state
                    .webauthn
                    .start_passkey_registration(handle, &email, &name, Some(existing))
                    .map_err(|e| {
                        tracing::error!("passkey registration didn't start: {e}");
                        ApiError::server()
                    })?;
                let mut json =
                    serde_json::to_value(&ccr.public_key).map_err(|_| ApiError::server())?;
                // Ask for a passkey that can be found without naming the account, since
                // signing in later starts with no address.
                json["authenticatorSelection"]["residentKey"] = "required".into();
                json["authenticatorSelection"]["requireResidentKey"] = true.into();
                ("create", json, CeremonyState::Create(reg))
            }
            (TicketKind::SignIn, _) => {
                let (rcr, auth) =
                    state
                        .webauthn
                        .start_discoverable_authentication()
                        .map_err(|e| {
                            tracing::error!("passkey sign-in didn't start: {e}");
                            ApiError::server()
                        })?;
                let json = serde_json::to_value(&rcr.public_key).map_err(|_| ApiError::server())?;
                ("get", json, CeremonyState::SignIn(auth))
            }
            (TicketKind::Create, None) => return Err(ApiError::server()),
        }
    };
    let mut ceremonies = state.ceremonies.lock().unwrap_or_else(|e| e.into_inner());
    ceremonies.retain(|_, c| c.started > now - TICKET_SECONDS);
    ceremonies.insert(
        hash,
        Ceremony {
            started: now,
            state: ceremony,
        },
    );
    Ok(Json(Options { mode, public_key }))
}

fn user_passkeys(conn: &Connection, user: i64) -> rusqlite::Result<Vec<(Vec<u8>, Passkey)>> {
    let mut stmt =
        conn.prepare("SELECT credential_id, passkey FROM passkeys WHERE user_id = ?1")?;
    let rows = stmt.query_map([user], |row| {
        Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut keys = Vec::new();
    for row in rows {
        let (id, json) = row?;
        match serde_json::from_str(&json) {
            Ok(passkey) => keys.push((id, passkey)),
            Err(e) => tracing::error!("a stored passkey doesn't parse: {e}"),
        }
    }
    Ok(keys)
}

#[derive(Deserialize)]
pub struct FinishBody {
    ticket: String,
    /// What the browser's `create` or `get` gave, with binary values as base64url.
    credential: Value,
}

pub async fn finish(
    State(state): State<AppState>,
    Body(req): Body<FinishBody>,
) -> ApiResult<StatusCode> {
    let now = state.now();
    let hash = secret::sha256(req.ticket.as_bytes());
    let ceremony = state
        .ceremonies
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&hash)
        .filter(|c| c.started > now - TICKET_SECONDS)
        .ok_or_else(|| ApiError::expired("This passkey step has run out. Reload the page."))?;
    let failed = |what: &str, e: &dyn std::fmt::Display| {
        tracing::info!("passkey {what} refused: {e}");
        ApiError::invalid("That passkey didn't work. Try again.")
    };
    let db = state.db();
    let (kind, ticket_user) = pending_ticket(&db, &hash, now)?;
    match (ceremony.state, kind, ticket_user) {
        (CeremonyState::Create(reg), TicketKind::Create, Some(user)) => {
            let credential: RegisterPublicKeyCredential = serde_json::from_value(req.credential)
                .map_err(|e| ApiError::invalid(format!("That isn't a new passkey: {e}")))?;
            let passkey = state
                .webauthn
                .finish_passkey_registration(&credential, &reg)
                .map_err(|e| failed("creation", &e))?;
            let json = serde_json::to_string(&passkey).map_err(|_| ApiError::server())?;
            let added = db.execute(
                "INSERT OR IGNORE INTO passkeys (credential_id, user_id, passkey, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![passkey.cred_id().as_slice(), user, json, now],
            )?;
            if added == 0 {
                return Err(ApiError::taken("That passkey is saved already."));
            }
            db.execute(
                "UPDATE tickets SET done = 1 WHERE ticket_hash = ?1",
                [&hash],
            )?;
        }
        (CeremonyState::SignIn(auth), TicketKind::SignIn, _) => {
            let credential: PublicKeyCredential = serde_json::from_value(req.credential)
                .map_err(|e| ApiError::invalid(format!("That isn't a passkey answer: {e}")))?;
            let (handle, credential_id) = state
                .webauthn
                .identify_discoverable_authentication(&credential)
                .map_err(|e| failed("sign-in", &e))?;
            let found: Option<(i64, String)> = db
                .query_row(
                    "SELECT p.user_id, p.passkey FROM passkeys p JOIN users u ON u.id = p.user_id
                     WHERE p.credential_id = ?1 AND u.handle = ?2",
                    params![credential_id, handle.to_string()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            let Some((user, json)) = found else {
                return Err(ApiError::not_found(
                    "No account has this passkey. Sign in with your email address.",
                ));
            };
            let mut passkey: Passkey =
                serde_json::from_str(&json).map_err(|_| ApiError::server())?;
            let result = state
                .webauthn
                .finish_discoverable_authentication_v2(&credential, auth, &passkey)
                .map_err(|e| failed("sign-in", &e))?;
            let json = if passkey.update_credential(&result) == Some(true) {
                serde_json::to_string(&passkey).map_err(|_| ApiError::server())?
            } else {
                json
            };
            db.execute(
                "UPDATE passkeys SET passkey = ?1, last_used_at = ?2 WHERE credential_id = ?3",
                params![json, now, result.cred_id().as_slice()],
            )?;
            db.execute(
                "UPDATE tickets SET done = 1, user_id = ?1 WHERE ticket_hash = ?2",
                params![user, hash],
            )?;
        }
        _ => {
            return Err(ApiError::expired(
                "This passkey step has run out. Reload the page.",
            ));
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

const PAGE: &str = include_str!("passkey.html");

/// The page itself: one file, no outside resources.
pub async fn page() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
            (header::REFERRER_POLICY, "no-referrer"),
            (
                header::CONTENT_SECURITY_POLICY,
                "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; \
                 connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'",
            ),
        ],
        PAGE,
    )
}
