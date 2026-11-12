//! Signing in: a code by email, then a passkey (or, with passkeys off, straight in).

use axum::{Json, extract::State, http::StatusCode};
use monster_core::api::{LoginFinish, LoginFinished, LoginStart, Polled, Ticket};
use rusqlite::{Connection, OptionalExtension, params};
use webauthn_rs::prelude::Uuid;

use crate::{
    AppState, ClientIp, db,
    error::{ApiError, ApiResult, Body},
    hit, hits,
    me::{load_me, new_session},
    passkey::{self, TicketKind},
    secret,
};

/// How long a sign-in code works.
pub const CODE_SECONDS: i64 = 10 * 60;
/// Wrong tries before a code is cancelled.
pub const CODE_TRIES: u32 = 5;
/// Codes sent to one address in an hour; and no more than one a minute.
pub const CODES_PER_ADDRESS_HOUR: i64 = 5;
/// Codes asked for from one network address in an hour.
pub const CODES_PER_IP_HOUR: i64 = 20;
/// Passkey sign-ins started from one network address in an hour.
pub const PASSKEY_TICKETS_PER_IP_HOUR: i64 = 30;

/// Trims and lowercases an address, and turns away anything that can't be one.
fn normalise_email(email: &str) -> ApiResult<String> {
    let email = email.trim().to_lowercase();
    let valid = email.len() <= 254
        && !email.chars().any(|c| c.is_whitespace() || c.is_control())
        && match email.split_once('@') {
            Some((local, domain)) => {
                !local.is_empty()
                    && !domain.contains('@')
                    && domain.contains('.')
                    && !domain.starts_with('.')
                    && !domain.ends_with('.')
            }
            None => false,
        };
    if valid {
        Ok(email)
    } else {
        Err(ApiError::invalid("That isn't an email address."))
    }
}

pub async fn start(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Body(req): Body<LoginStart>,
) -> ApiResult<StatusCode> {
    let email = normalise_email(&req.email)?;
    let now = state.now();
    let code = state.mailer.new_code();
    {
        let db = state.db();
        db::sweep(&db, now)?;
        let by_address = format!("code-email:{email}");
        if hits(&db, &by_address, now, 60)? > 0 {
            return Err(ApiError::slow_down(
                "A code was sent a moment ago. Wait a minute before asking for another.",
            ));
        }
        if hits(&db, &by_address, now, 3600)? >= CODES_PER_ADDRESS_HOUR {
            return Err(ApiError::slow_down(
                "Too many codes for this address. Try again in an hour.",
            ));
        }
        let by_ip = ip.map(|ip| format!("code-ip:{ip}"));
        if let Some(by_ip) = &by_ip {
            if hits(&db, by_ip, now, 3600)? >= CODES_PER_IP_HOUR {
                return Err(ApiError::slow_down(
                    "Too many codes asked for from here. Try again in an hour.",
                ));
            }
            hit(&db, by_ip, now)?;
        }
        hit(&db, &by_address, now)?;
        db.execute(
            "INSERT OR REPLACE INTO login_codes (email, code_hash, expires_at, tries_left)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                email,
                secret::code_hash(&email, &code),
                now + CODE_SECONDS,
                CODE_TRIES
            ],
        )?;
    }
    if let Err(err) = state.mailer.send_code(&email, &code).await {
        tracing::error!("sign-in code not sent: {err}");
        state
            .db()
            .execute("DELETE FROM login_codes WHERE email = ?1", [&email])?;
        return Err(ApiError::server());
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn finish(
    State(state): State<AppState>,
    Body(req): Body<LoginFinish>,
) -> ApiResult<Json<LoginFinished>> {
    let email = normalise_email(&req.email)?;
    let now = state.now();
    let mut db = state.db();
    let row: Option<(Vec<u8>, i64, u32)> = db
        .query_row(
            "SELECT code_hash, expires_at, tries_left FROM login_codes WHERE email = ?1",
            [&email],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let ran_out = || ApiError::expired("That code has run out. Ask for a new one.");
    let Some((hash, expires_at, tries_left)) = row else {
        return Err(ran_out());
    };
    if expires_at <= now {
        db.execute("DELETE FROM login_codes WHERE email = ?1", [&email])?;
        return Err(ran_out());
    }
    if !secret::same(&hash, &secret::code_hash(&email, req.code.trim())) {
        let left = tries_left.saturating_sub(1);
        if left == 0 {
            db.execute("DELETE FROM login_codes WHERE email = ?1", [&email])?;
        } else {
            db.execute(
                "UPDATE login_codes SET tries_left = ?1 WHERE email = ?2",
                params![left, email],
            )?;
        }
        return Err(ApiError::wrong_code(left));
    }

    let tx = db.transaction()?;
    tx.execute("DELETE FROM login_codes WHERE email = ?1", [&email])?;
    let user = find_or_create_user(&tx, &email, now)?;
    let answer = if state.config.passkeys {
        LoginFinished::Passkey {
            ticket: passkey::new_ticket(&tx, TicketKind::Create, Some(user), now)?,
        }
    } else {
        LoginFinished::SignedIn {
            session: new_session(&tx, user, now)?,
            me: load_me(&tx, user)?,
        }
    };
    tx.commit()?;
    Ok(Json(answer))
}

/// The account for `email`, made now if this is its first sign-in.
fn find_or_create_user(conn: &Connection, email: &str, now: i64) -> rusqlite::Result<i64> {
    if let Some(id) = conn
        .query_row("SELECT id FROM users WHERE email = ?1", [email], |row| {
            row.get(0)
        })
        .optional()?
    {
        return Ok(id);
    }
    let local = email.split('@').next().unwrap_or(email);
    let name: String = local.chars().take(32).collect();
    conn.execute(
        "INSERT INTO users (email, name, created_at, roll_key, handle) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            email,
            name,
            now,
            secret::random::<32>().to_vec(),
            Uuid::new_v4().to_string()
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub async fn with_passkey(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
) -> ApiResult<Json<Ticket>> {
    if !state.config.passkeys {
        return Err(ApiError::not_found(
            "This server doesn't use passkeys. Sign in with a code.",
        ));
    }
    let now = state.now();
    let db = state.db();
    if let Some(ip) = ip {
        let bucket = format!("ticket-ip:{ip}");
        if hits(&db, &bucket, now, 3600)? >= PASSKEY_TICKETS_PER_IP_HOUR {
            return Err(ApiError::slow_down(
                "Too many sign-ins started from here. Try again later.",
            ));
        }
        hit(&db, &bucket, now)?;
    }
    let ticket = passkey::new_ticket(&db, TicketKind::SignIn, None, now)?;
    Ok(Json(Ticket { ticket }))
}

pub async fn poll(
    State(state): State<AppState>,
    Body(req): Body<Ticket>,
) -> ApiResult<Json<Polled>> {
    let now = state.now();
    let mut db = state.db();
    let hash = secret::sha256(req.ticket.as_bytes());
    let row: Option<(Option<i64>, bool, i64)> = db
        .query_row(
            "SELECT user_id, done, expires_at FROM tickets WHERE ticket_hash = ?1",
            [&hash],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let ran_out = || ApiError::expired("That sign-in has run out. Start again.");
    let Some((user, done, expires_at)) = row else {
        return Err(ran_out());
    };
    if expires_at <= now {
        db.execute("DELETE FROM tickets WHERE ticket_hash = ?1", [&hash])?;
        return Err(ran_out());
    }
    let (true, Some(user)) = (done, user) else {
        return Ok(Json(Polled::Pending));
    };
    // The ticket goes as the session is made, so a second poll can't get another.
    let tx = db.transaction()?;
    tx.execute("DELETE FROM tickets WHERE ticket_hash = ?1", [&hash])?;
    let answer = Polled::Done {
        session: new_session(&tx, user, now)?,
        me: load_me(&tx, user)?,
    };
    tx.commit()?;
    Ok(Json(answer))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_trimmed_and_lowercased() {
        assert_eq!(
            normalise_email("  Sam@Example.COM ").unwrap(),
            "sam@example.com"
        );
        for bad in [
            "",
            "sam",
            "@example.com",
            "sam@",
            "sam@example",
            "s am@example.com",
        ] {
            assert!(normalise_email(bad).is_err(), "{bad}");
        }
    }
}
