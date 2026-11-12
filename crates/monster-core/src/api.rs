//! Everything the app and the server say to each other: JSON over HTTP, under `/v1`.
//!
//! After signing in, every call carries `Authorization: Bearer <session>`. A call that fails
//! answers with a 4xx or 5xx status and a [`Problem`].
//!
//! | Call | Body | Answer |
//! | --- | --- | --- |
//! | `POST /v1/login/start` | [`LoginStart`] | 204, whether or not the address has an account |
//! | `POST /v1/login/finish` | [`LoginFinish`] | [`LoginFinished`] |
//! | `POST /v1/login/passkey` | none | [`Ticket`], to sign in with a passkey made earlier |
//! | `POST /v1/login/poll` | [`Ticket`] | [`Polled`] |
//! | `GET /v1/me` | | [`Me`] |
//! | `PATCH /v1/me` | [`UpdateMe`] | [`Me`] |
//! | `DELETE /v1/me` | | 204; the account and everything in it is gone |
//! | `DELETE /v1/session` | | 204 |
//! | `POST /v1/identities/challenge` | none | [`Challenge`] |
//! | `POST /v1/identities` | [`LinkIdentity`] | [`Me`] |
//! | `POST /v1/identities/unlink` | [`UnlinkIdentity`] | [`Me`] |
//! | `GET /v1/cans` | | [`Cans`], oldest first |
//! | `POST /v1/cans` | [`AddCan`] | [`Can`] |
//! | `DELETE /v1/cans/{id}` | | 204 |
//! | `POST /v1/cans/import` | [`Import`] | [`Imported`]; once for each account |
//! | `GET /v1/leaderboard?by=collection` | | [`Leaderboard`], best first |
//! | `GET /v1/users/{id}/cans` | | [`Collection`]; 404 if that person is hidden |
//! | `GET /health` | | 200 |
//!
//! # Signing in
//!
//! 1. `login/start` emails a six-digit code. `login/finish` checks it.
//! 2. If the server asks for passkeys, `login/finish` answers [`LoginFinished::Passkey`] with a
//!    ticket. The app opens [`passkey_page`] in the browser, where the person creates a
//!    passkey, and asks `login/poll` every second or two until it answers [`Polled::Done`].
//! 3. If the server doesn't ask for passkeys, `login/finish` answers
//!    [`LoginFinished::SignedIn`] straight away.
//! 4. Later, `login/passkey` gives a ticket for the same page, which signs in with the
//!    passkey alone: no email.
//!
//! # Linking a TeamSpeak identity
//!
//! The app asks for a [`Challenge`], signs [`link_message`] with the identity's private key
//! (ECDSA over P-256 with SHA-256, the signature DER-encoded, as `tsproto` signs), and sends
//! [`LinkIdentity`]. The server verifies it and works out the uid itself.

use serde::{Deserialize, Serialize};

use crate::{Ranking, Rarity, Standing};

/// What a TeamSpeak identity signs to be linked: this, then the challenge. The prefix keeps
/// the signature from meaning anything to a TeamSpeak server.
pub const LINK_PREFIX: &str = "flexspeak-monster-link-v1:";

/// The bytes an identity signs for `challenge`.
pub fn link_message(challenge: &str) -> Vec<u8> {
    format!("{LINK_PREFIX}{challenge}").into_bytes()
}

/// The page to open in the browser for a passkey, on the server at `base` (no trailing
/// slash). The ticket rides in the fragment, which browsers don't send or log.
pub fn passkey_page(base: &str, ticket: &str) -> String {
    format!("{}/passkey#ticket={ticket}", base.trim_end_matches('/'))
}

/// Why a call failed. `error` is one of the codes in [`problem`]; `message` says it in words
/// fit to show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Problem {
    pub error: String,
    pub message: String,
    /// After a wrong sign-in code: how many more tries that code allows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tries_left: Option<u32>,
}

/// The codes a [`Problem`] carries.
pub mod problem {
    /// 401: no session, or one that has ended. The app signs in again.
    pub const SIGNED_OUT: &str = "signed_out";
    /// 401: the sign-in code doesn't match.
    pub const WRONG_CODE: &str = "wrong_code";
    /// 410: the code or ticket has run out, or was used up by wrong tries.
    pub const EXPIRED: &str = "expired";
    /// 429: too many tries; wait before the next.
    pub const SLOW_DOWN: &str = "slow_down";
    /// 400: something in the request isn't valid; `message` says what.
    pub const INVALID: &str = "invalid";
    /// 404.
    pub const NOT_FOUND: &str = "not_found";
    /// 409: the identity is linked to another account, or the cans were imported before.
    pub const TAKEN: &str = "taken";
    /// 5xx.
    pub const SERVER: &str = "server";
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginStart {
    pub email: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginFinish {
    pub email: String,
    /// The six digits from the email.
    pub code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "next", rename_all = "snake_case")]
pub enum LoginFinished {
    /// A passkey comes next: open [`passkey_page`] with this ticket, then poll.
    Passkey {
        ticket: String,
    },
    SignedIn {
        session: String,
        me: Me,
    },
}

/// A short-lived, single-use stand-in for a sign-in that a passkey has yet to finish.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ticket {
    pub ticket: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Polled {
    /// The passkey page isn't done yet.
    Pending,
    Done {
        session: String,
        me: Me,
    },
}

/// The account signed in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Me {
    pub id: i64,
    /// Shown on the leaderboard. Starts as the part of the email address before the `@`.
    pub name: String,
    pub email: String,
    /// Off the leaderboard, and closed to everyone else.
    pub hidden: bool,
    /// Whether the cans kept on this person's computer were uploaded already.
    pub imported: bool,
    pub identities: Vec<LinkedIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkedIdentity {
    /// The TeamSpeak uid: base64 of the SHA-1 of the public key as TeamSpeak writes it.
    pub uid: String,
    /// The app it was found in, such as `Flexspeak`.
    pub source: String,
    /// Unix seconds.
    pub linked_at: i64,
}

/// Only the fields present change.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateMe {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Challenge {
    pub challenge: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkIdentity {
    pub challenge: String,
    /// The public key as TeamSpeak writes it: base64 of its tomcrypt DER export
    /// (`EccKeyPubP256::to_ts`).
    pub public_key: String,
    /// Base64 of the DER-encoded ECDSA signature over [`link_message`].
    pub signature: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnlinkIdentity {
    pub uid: String,
}

/// A can in someone's collection, with the card it rolled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Can {
    /// Its number in that person's collection, from 1, never reused.
    pub id: i64,
    /// A `Flavour::id`.
    pub flavour: String,
    /// When it was drunk, in Unix seconds.
    pub at: i64,
    pub rarity: Rarity,
    /// Varies the card's art between cans of a flavour.
    pub seed: u16,
    /// Uploaded from a computer, with the card it rolled there. Kept and shown, but not
    /// counted on the leaderboard, since the server didn't roll it.
    #[serde(default)]
    pub legacy: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddCan {
    /// A `Flavour::id`. The server sets the time and rolls the card.
    pub flavour: String,
    /// Made up by the app for this one can, so sending it again after a lost answer returns
    /// the same can and doesn't add another.
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cans {
    pub cans: Vec<Can>,
}

/// The cans kept on a computer before it first signed in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Import {
    pub cans: Vec<ImportedCan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportedCan {
    pub flavour: String,
    pub at: i64,
    pub rarity: Rarity,
    pub seed: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Imported {
    pub imported: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaderboardQuery {
    #[serde(default)]
    pub by: Ranking,
}

/// Everyone who isn't hidden, best first by the ranking asked for. Ties come in the order
/// the accounts were made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Leaderboard {
    pub rows: Vec<LeaderboardRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaderboardRow {
    pub user: i64,
    pub name: String,
    /// The account asking.
    pub you: bool,
    /// Counted from the cans the server rolled; legacy cans are left out.
    pub standing: Standing,
}

/// Someone's collection, for their board.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Collection {
    pub user: i64,
    pub name: String,
    /// Oldest first.
    pub cans: Vec<Can>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_finished_login_says_what_comes_next() {
        let next = LoginFinished::Passkey {
            ticket: "abc".into(),
        };
        assert_eq!(
            serde_json::to_string(&next).unwrap(),
            r#"{"next":"passkey","ticket":"abc"}"#
        );
        assert_eq!(
            serde_json::to_string(&Polled::Pending).unwrap(),
            r#"{"status":"pending"}"#
        );
    }

    #[test]
    fn the_passkey_page_keeps_the_ticket_out_of_the_request() {
        assert_eq!(
            passkey_page("https://example.com/", "t1"),
            "https://example.com/passkey#ticket=t1"
        );
    }

    #[test]
    fn a_can_from_an_older_server_isnt_legacy() {
        let can: Can = serde_json::from_str(
            r#"{"id":1,"flavour":"original","at":5,"rarity":"rare","seed":9}"#,
        )
        .unwrap();
        assert!(!can.legacy);
        assert_eq!(can.rarity, Rarity::Rare);
    }

    #[test]
    fn the_link_message_starts_with_the_prefix() {
        assert_eq!(link_message("xyz"), b"flexspeak-monster-link-v1:xyz");
    }
}
