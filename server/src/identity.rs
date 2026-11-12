//! Linking TeamSpeak identities to an account, by a signed challenge.
//!
//! TeamSpeak writes a public key as base64 of tomcrypt's DER export: a sequence of a one-bit
//! bit string (set when the private key is included), the key size, and the point's x and y.
//! This reads and writes that format with `p256` directly, rather than through `tsproto-types`,
//! which would bring its code generator and a second elliptic-curve stack into the build; the
//! tests check the two agree.

use axum::{Json, extract::State};
use base64::{Engine, engine::general_purpose::STANDARD};
use monster_core::api::{Challenge, LinkIdentity, Me, UnlinkIdentity, link_message};
use p256::ecdsa::{Signature, VerifyingKey, signature::Verifier};
use rusqlite::{OptionalExtension, params};
use sha1::{Digest, Sha1};

use crate::{
    AppState,
    error::{ApiError, ApiResult, Body},
    me::{Caller, load_me},
    secret,
};

/// How long a challenge works.
pub const CHALLENGE_SECONDS: i64 = 5 * 60;

#[derive(Debug, PartialEq, Eq)]
pub enum KeyError {
    NotAKey,
    /// The export holds the private key too; it is refused, never stored.
    PrivateKey,
}

/// A public key in TeamSpeak's format.
pub struct TsPublicKey {
    key: VerifyingKey,
    x: [u8; 32],
    y: [u8; 32],
}

impl TsPublicKey {
    pub fn parse(ts: &str) -> Result<Self, KeyError> {
        let der = STANDARD.decode(ts.trim()).map_err(|_| KeyError::NotAKey)?;
        let (0x30, body, []) = tlv(&der).ok_or(KeyError::NotAKey)? else {
            return Err(KeyError::NotAKey);
        };
        let (0x03, flags, rest) = tlv(body).ok_or(KeyError::NotAKey)? else {
            return Err(KeyError::NotAKey);
        };
        if flags.len() < 2 {
            return Err(KeyError::NotAKey);
        }
        if flags[1] & 0x80 != 0 {
            return Err(KeyError::PrivateKey);
        }
        let (0x02, _size, rest) = tlv(rest).ok_or(KeyError::NotAKey)? else {
            return Err(KeyError::NotAKey);
        };
        let (0x02, x, rest) = tlv(rest).ok_or(KeyError::NotAKey)? else {
            return Err(KeyError::NotAKey);
        };
        let (0x02, y, _) = tlv(rest).ok_or(KeyError::NotAKey)? else {
            return Err(KeyError::NotAKey);
        };
        let (x, y) = (coordinate(x)?, coordinate(y)?);
        let mut sec1 = [0u8; 65];
        sec1[0] = 0x04;
        sec1[1..33].copy_from_slice(&x);
        sec1[33..].copy_from_slice(&y);
        let key = VerifyingKey::from_sec1_bytes(&sec1).map_err(|_| KeyError::NotAKey)?;
        Ok(Self { key, x, y })
    }

    /// The key as TeamSpeak writes it, whatever spelling it was read from.
    pub fn to_ts(&self) -> String {
        let mut body = vec![0x03, 0x02, 0x07, 0x00, 0x02, 0x01, 0x20];
        for coord in [&self.x, &self.y] {
            let trimmed = &coord[coord.iter().position(|b| *b != 0).unwrap_or(31)..];
            let pad = trimmed[0] & 0x80 != 0;
            body.push(0x02);
            body.push((trimmed.len() + usize::from(pad)) as u8);
            if pad {
                body.push(0);
            }
            body.extend_from_slice(trimmed);
        }
        let mut der = vec![0x30, body.len() as u8];
        der.extend_from_slice(&body);
        STANDARD.encode(der)
    }

    /// The TeamSpeak uid: base64 of the SHA-1 of the key as TeamSpeak writes it.
    pub fn uid(&self) -> String {
        STANDARD.encode(Sha1::digest(self.to_ts().as_bytes()))
    }

    /// Checks a DER-encoded ECDSA P-256 signature, with SHA-256, over `data`.
    pub fn verify(&self, data: &[u8], der_signature: &[u8]) -> bool {
        Signature::from_der(der_signature).is_ok_and(|sig| self.key.verify(data, &sig).is_ok())
    }
}

/// One DER element: its tag, its contents, and what follows it.
fn tlv(data: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, rest) = data.split_first()?;
    let (&first, rest) = rest.split_first()?;
    let (len, rest) = match first {
        0..=0x7f => (usize::from(first), rest),
        0x81 => (usize::from(*rest.first()?), &rest[1..]),
        0x82 => (
            usize::from(u16::from_be_bytes([*rest.first()?, *rest.get(1)?])),
            rest.get(2..)?,
        ),
        _ => return None,
    };
    (rest.len() >= len).then(|| (tag, &rest[..len], &rest[len..]))
}

/// A DER integer as a 32-byte big-endian coordinate.
fn coordinate(int: &[u8]) -> Result<[u8; 32], KeyError> {
    if int.first().is_none_or(|b| b & 0x80 != 0) {
        return Err(KeyError::NotAKey);
    }
    let start = int.iter().position(|b| *b != 0).unwrap_or(int.len());
    let digits = &int[start..];
    if digits.len() > 32 {
        return Err(KeyError::NotAKey);
    }
    let mut out = [0u8; 32];
    out[32 - digits.len()..].copy_from_slice(digits);
    Ok(out)
}

pub async fn challenge(
    State(state): State<AppState>,
    caller: Caller,
) -> ApiResult<Json<Challenge>> {
    let challenge = secret::token();
    state.db().execute(
        "INSERT INTO challenges (challenge, user_id, expires_at) VALUES (?1, ?2, ?3)",
        params![challenge, caller.user, state.now() + CHALLENGE_SECONDS],
    )?;
    Ok(Json(Challenge { challenge }))
}

pub async fn link(
    State(state): State<AppState>,
    caller: Caller,
    Body(req): Body<LinkIdentity>,
) -> ApiResult<Json<Me>> {
    let source = req.source.trim();
    if source.is_empty() || source.chars().count() > 40 || source.chars().any(char::is_control) {
        return Err(ApiError::invalid(
            "source is 1 to 40 characters, such as Flexspeak.",
        ));
    }
    let now = state.now();
    let db = state.db();
    // Taken whatever happens next, so each challenge gets one try.
    let expires_at: Option<i64> = db
        .query_row(
            "DELETE FROM challenges WHERE challenge = ?1 AND user_id = ?2 RETURNING expires_at",
            params![req.challenge, caller.user],
            |row| row.get(0),
        )
        .optional()?;
    if expires_at.is_none_or(|at| at <= now) {
        return Err(ApiError::expired(
            "That challenge has run out or was used. Ask for a new one.",
        ));
    }
    let key = TsPublicKey::parse(&req.public_key).map_err(|e| match e {
        KeyError::PrivateKey => {
            ApiError::invalid("That's a private key. Send only the public key.")
        }
        KeyError::NotAKey => ApiError::invalid("That isn't a TeamSpeak public key."),
    })?;
    let signature = STANDARD
        .decode(req.signature.trim())
        .map_err(|_| ApiError::invalid("The signature isn't base64."))?;
    if !key.verify(&link_message(&req.challenge), &signature) {
        return Err(ApiError::invalid(
            "The signature doesn't match that identity.",
        ));
    }
    let uid = key.uid();
    let owner: Option<i64> = db
        .query_row(
            "SELECT user_id FROM identities WHERE uid = ?1",
            [&uid],
            |row| row.get(0),
        )
        .optional()?;
    match owner {
        Some(owner) if owner != caller.user => {
            return Err(ApiError::taken(
                "That identity is linked to another account.",
            ));
        }
        Some(_) => {}
        None => {
            db.execute(
                "INSERT INTO identities (uid, user_id, public_key, source, linked_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![uid, caller.user, key.to_ts(), source, now],
            )?;
        }
    }
    Ok(Json(load_me(&db, caller.user)?))
}

pub async fn unlink(
    State(state): State<AppState>,
    caller: Caller,
    Body(req): Body<UnlinkIdentity>,
) -> ApiResult<Json<Me>> {
    let db = state.db();
    let removed = db.execute(
        "DELETE FROM identities WHERE uid = ?1 AND user_id = ?2",
        params![req.uid, caller.user],
    )?;
    if removed == 0 {
        return Err(ApiError::not_found(
            "That identity isn't linked to your account.",
        ));
    }
    Ok(Json(load_me(&db, caller.user)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsproto_types::crypto::EccKeyPrivP256;

    #[test]
    fn keys_read_and_write_like_tsproto() {
        for _ in 0..200 {
            let private = EccKeyPrivP256::create();
            let public = private.to_pub();
            let ours = TsPublicKey::parse(&public.to_ts()).unwrap();
            assert_eq!(ours.to_ts(), public.to_ts());
            assert_eq!(ours.uid(), public.get_uid());
            let signature = private.sign(b"hello");
            assert!(ours.verify(b"hello", &signature));
            assert!(!ours.verify(b"hullo", &signature));
        }
    }

    #[test]
    fn a_private_key_is_refused() {
        let private = EccKeyPrivP256::create();
        assert_eq!(
            TsPublicKey::parse(&private.to_ts()).err(),
            Some(KeyError::PrivateKey)
        );
        assert_eq!(
            TsPublicKey::parse("bm90IGEga2V5").err(),
            Some(KeyError::NotAKey)
        );
    }
}
