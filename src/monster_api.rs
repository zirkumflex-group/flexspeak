//! Calls to the Monster collection server, one async function for each route in
//! `monster_core::api`. Each answers the contract's type or a [`Problem`]: what the server said
//! went wrong, or one made up here when it couldn't be reached ([`OFFLINE`]).
//!
//! Nothing here decides anything; `monster::Tracker::answer` does, so tests can feed it answers.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use monster_core::Ranking;
pub use monster_core::api::{
    self, AddCan, Can, Cans, Challenge, Collection, Import, Imported, Leaderboard, LinkIdentity,
    LoginFinish, LoginFinished, LoginStart, Me, Polled, Problem, Ticket, UnlinkIdentity, UpdateMe,
    problem,
};
use reqwest::{Method, StatusCode, header};
use serde::{Serialize, de::DeserializeOwned};

/// A [`Problem`] code the server never sends: the call didn't reach it, or its answer didn't
/// arrive.
pub const OFFLINE: &str = "offline";

/// Who is asking: the server's address and, once signed in, the session.
#[derive(Debug, Clone)]
pub struct Server {
    base: String,
    session: Option<String>,
}

impl Server {
    pub fn new(base: &str, session: Option<String>) -> Self {
        Self {
            base: base.trim().trim_end_matches('/').to_owned(),
            session,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&impl Serialize>,
    ) -> Result<T, Problem> {
        let bytes = self.send(method, path, body).await?;
        serde_json::from_slice(&bytes).map_err(|error| {
            tracing::warn!("The collection server's answer to {path} doesn't parse: {error}");
            Problem {
                error: problem::SERVER.into(),
                message: "The collection server sent an answer this version can't read.".into(),
                tries_left: None,
            }
        })
    }

    /// A call whose answer has no body (204).
    async fn call_empty(
        &self,
        method: Method,
        path: &str,
        body: Option<&impl Serialize>,
    ) -> Result<(), Problem> {
        self.send(method, path, body).await.map(drop)
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        body: Option<&impl Serialize>,
    ) -> Result<Vec<u8>, Problem> {
        let mut request = client()?
            .request(method, self.url(path))
            .header(header::ACCEPT, "application/json")
            .timeout(Duration::from_secs(30));
        if let Some(session) = &self.session {
            request = request.bearer_auth(session);
        }
        if let Some(body) = body {
            let json = serde_json::to_vec(body).map_err(|error| offline(error.to_string()))?;
            request = request
                .header(header::CONTENT_TYPE, "application/json")
                .body(json);
        }
        let response = request.send().await.map_err(|error| {
            tracing::warn!(
                "The collection server can't be reached: {}",
                describe(&error)
            );
            offline(describe(&error))
        })?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|error| offline(describe(&error)))?;
        if status.is_success() {
            Ok(bytes.to_vec())
        } else {
            Err(problem_from(status, &bytes))
        }
    }

    pub async fn login_start(&self, email: &str) -> Result<(), Problem> {
        let body = LoginStart {
            email: email.to_owned(),
        };
        self.call_empty(Method::POST, "/v1/login/start", Some(&body))
            .await
    }

    pub async fn login_finish(&self, email: &str, code: &str) -> Result<LoginFinished, Problem> {
        let body = LoginFinish {
            email: email.to_owned(),
            code: code.to_owned(),
        };
        self.call(Method::POST, "/v1/login/finish", Some(&body))
            .await
    }

    pub async fn login_passkey(&self) -> Result<Ticket, Problem> {
        self.call(Method::POST, "/v1/login/passkey", None::<&()>)
            .await
    }

    pub async fn login_poll(&self, ticket: &str) -> Result<Polled, Problem> {
        let body = Ticket {
            ticket: ticket.to_owned(),
        };
        self.call(Method::POST, "/v1/login/poll", Some(&body)).await
    }

    pub async fn me(&self) -> Result<Me, Problem> {
        self.call(Method::GET, "/v1/me", None::<&()>).await
    }

    pub async fn update_me(&self, change: &UpdateMe) -> Result<Me, Problem> {
        self.call(Method::PATCH, "/v1/me", Some(change)).await
    }

    /// Deletes the account and everything in it. Only the end-to-end test calls it until the
    /// page has an account section.
    #[cfg_attr(not(test), allow(dead_code))]
    pub async fn delete_me(&self) -> Result<(), Problem> {
        self.call_empty(Method::DELETE, "/v1/me", None::<&()>).await
    }

    pub async fn sign_out(&self) -> Result<(), Problem> {
        self.call_empty(Method::DELETE, "/v1/session", None::<&()>)
            .await
    }

    pub async fn challenge(&self) -> Result<Challenge, Problem> {
        self.call(Method::POST, "/v1/identities/challenge", None::<&()>)
            .await
    }

    pub async fn link(&self, link: &LinkIdentity) -> Result<Me, Problem> {
        self.call(Method::POST, "/v1/identities", Some(link)).await
    }

    /// For unlinking from the page later; the Identity step only links.
    #[allow(dead_code)]
    pub async fn unlink(&self, uid: &str) -> Result<Me, Problem> {
        let body = UnlinkIdentity {
            uid: uid.to_owned(),
        };
        self.call(Method::POST, "/v1/identities/unlink", Some(&body))
            .await
    }

    /// Asks for a challenge, signs it with `identity` and links it, as `source`.
    pub async fn link_identity(
        &self,
        identity: &tsclientlib::Identity,
        source: &str,
    ) -> Result<Me, Problem> {
        let challenge = self.challenge().await?;
        self.link(&link_request(identity, &challenge.challenge, source))
            .await
    }

    /// Whether the server is up, without signing in.
    #[allow(dead_code)]
    pub async fn health(&self) -> Result<(), Problem> {
        self.call_empty(Method::GET, "/health", None::<&()>).await
    }

    pub async fn cans(&self) -> Result<Cans, Problem> {
        self.call(Method::GET, "/v1/cans", None::<&()>).await
    }

    pub async fn add_can(&self, can: &AddCan) -> Result<Can, Problem> {
        self.call(Method::POST, "/v1/cans", Some(can)).await
    }

    pub async fn remove_can(&self, id: i64) -> Result<(), Problem> {
        self.call_empty(Method::DELETE, &format!("/v1/cans/{id}"), None::<&()>)
            .await
    }

    pub async fn import(&self, cans: &Import) -> Result<Imported, Problem> {
        self.call(Method::POST, "/v1/cans/import", Some(cans)).await
    }

    pub async fn leaderboard(&self, by: Ranking) -> Result<Leaderboard, Problem> {
        // The query value is the ranking's serde name: its name in lower case.
        let path = format!("/v1/leaderboard?by={}", by.name().to_lowercase());
        self.call(Method::GET, &path, None::<&()>).await
    }

    pub async fn collection(&self, user: i64) -> Result<Collection, Problem> {
        self.call(Method::GET, &format!("/v1/users/{user}/cans"), None::<&()>)
            .await
    }
}

/// What an identity sends to be linked: its public key and its signature over the challenge.
pub fn link_request(
    identity: &tsclientlib::Identity,
    challenge: &str,
    source: &str,
) -> LinkIdentity {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let key = identity.key();
    LinkIdentity {
        challenge: challenge.to_owned(),
        public_key: key.to_pub().to_ts(),
        // `sign` takes the key by value; the clone is dropped with the signature made.
        signature: STANDARD.encode(key.clone().sign(&api::link_message(challenge))),
        source: source.to_owned(),
    }
}

fn client() -> Result<reqwest::Client, Problem> {
    // As the updater: TLS through ring, installed once; installing twice is harmless.
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder()
        .user_agent(concat!("Flexspeak/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| offline(format!("Cannot start an HTTPS client: {error}")))
}

fn describe(error: &reqwest::Error) -> String {
    let mut text = error.to_string();
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

/// The server couldn't be reached. The detail goes to the log; the message is for people.
fn offline(detail: String) -> Problem {
    tracing::debug!("Collection server: {detail}");
    Problem {
        error: OFFLINE.into(),
        message: "Can't reach the collection server. Check your connection and the address in \
                  Settings › WIP."
            .into(),
        tries_left: None,
    }
}

/// The server's own problem, or one made from the status when the body isn't one (a proxy's
/// error page, say).
fn problem_from(status: StatusCode, body: &[u8]) -> Problem {
    if let Ok(problem) = serde_json::from_slice::<Problem>(body) {
        return problem;
    }
    let error = match status.as_u16() {
        401 => problem::SIGNED_OUT,
        404 => problem::NOT_FOUND,
        409 => problem::TAKEN,
        410 => problem::EXPIRED,
        429 => problem::SLOW_DOWN,
        500.. => problem::SERVER,
        _ => problem::INVALID,
    };
    Problem {
        error: error.into(),
        message: format!(
            "The collection server answered {}.",
            status.canonical_reason().map_or_else(
                || status.as_u16().to_string(),
                |reason| format!("{} {reason}", status.as_u16())
            )
        ),
        tries_left: None,
    }
}

// ---- the session kept between launches ----

/// Where the session is kept on systems without DPAPI: beside `settings.json`, readable only by
/// the person signed in to the computer. Windows seals it into `settings.json` instead.
pub fn session_path(settings: &Path) -> PathBuf {
    settings.with_file_name("monster-session")
}

#[cfg(not(windows))]
pub fn read_session(path: &Path) -> Option<String> {
    let token = std::fs::read_to_string(path).ok()?;
    let token = token.trim();
    (!token.is_empty()).then(|| token.to_owned())
}

/// Saves `token`, or removes the file for `None`.
#[cfg(not(windows))]
pub fn write_session(path: &Path, token: Option<&str>) -> std::io::Result<()> {
    use std::{fs, io::Write};
    let Some(token) = token else {
        return match fs::remove_file(path) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        };
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    // Written beside and renamed over, so a crash never leaves half a token; made private
    // before anything is in it.
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options.open(&temporary)?;
        file.write_all(token.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_servers_own_problem_comes_through() {
        let body = br#"{"error":"wrong_code","message":"That code doesn't match.","tries_left":3}"#;
        let problem = problem_from(StatusCode::UNAUTHORIZED, body);
        assert_eq!(problem.error, problem::WRONG_CODE);
        assert_eq!(problem.tries_left, Some(3));
    }

    #[test]
    fn a_status_without_a_problem_still_says_what_kind() {
        let problem = problem_from(StatusCode::UNAUTHORIZED, b"<html>no</html>");
        assert_eq!(problem.error, problem::SIGNED_OUT);
        assert_eq!(
            problem_from(StatusCode::BAD_GATEWAY, b"").error,
            problem::SERVER
        );
        assert!(
            problem_from(StatusCode::BAD_GATEWAY, b"")
                .message
                .contains("502")
        );
    }

    #[test]
    fn an_address_loses_its_trailing_slash() {
        assert_eq!(
            Server::new(" https://example.com/ ", None).url("/v1/me"),
            "https://example.com/v1/me"
        );
    }

    #[test]
    fn a_server_that_isnt_there_is_offline() {
        // Nothing listens on port 9 of this machine.
        let answer = crate::updater::block_on(async {
            Ok::<_, String>(Server::new("http://127.0.0.1:9", None).me().await)
        })
        .unwrap();
        assert_eq!(answer.unwrap_err().error, OFFLINE);
    }

    #[test]
    fn a_linked_identity_signs_the_message_with_its_own_key() {
        let identity = tsclientlib::Identity::create();
        let link = link_request(&identity, "abc", "Flexspeak");
        let key = tsproto_types::crypto::EccKeyPubP256::from_ts(&link.public_key).unwrap();
        assert_eq!(key.get_uid(), identity.key().to_pub().get_uid());
        use base64::{Engine, engine::general_purpose::STANDARD};
        let signature = STANDARD.decode(&link.signature).unwrap();
        assert!(key.verify(&api::link_message("abc"), &signature).is_ok());
        assert!(key.verify(&api::link_message("abd"), &signature).is_err());
    }

    #[cfg(not(windows))]
    #[test]
    fn the_session_file_is_private_and_goes_on_sign_out() {
        let dir =
            std::env::temp_dir().join(format!("flexspeak-monster-session-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = session_path(&dir.join("settings.json"));
        assert_eq!(read_session(&path), None);
        write_session(&path, Some("token-1")).unwrap();
        assert_eq!(read_session(&path).as_deref(), Some("token-1"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        write_session(&path, None).unwrap();
        assert_eq!(read_session(&path), None);
        write_session(&path, None).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Against a real server: `MONSTER_TEST_SERVER` is its address, `MONSTER_TEST_CODE` the
    /// fixed sign-in code it runs with, passkeys off. The account it makes is deleted at the end.
    #[test]
    #[ignore = "needs a running collection server"]
    fn end_to_end_against_a_real_server() {
        let base = std::env::var("MONSTER_TEST_SERVER").expect("MONSTER_TEST_SERVER");
        let code = std::env::var("MONSTER_TEST_CODE").expect("MONSTER_TEST_CODE");
        let mut nonce = [0u8; 6];
        getrandom::fill(&mut nonce).unwrap();
        let email = format!(
            "e2e-{}@example.com",
            nonce.iter().map(|b| format!("{b:02x}")).collect::<String>()
        );
        crate::updater::block_on(async move {
            let anonymous = Server::new(&base, None);
            anonymous.login_start(&email).await.map_err(|p| p.message)?;
            let wrong = anonymous.login_finish(&email, "000000").await;
            if code != "000000" {
                let wrong = wrong.expect_err("a wrong code signs nobody in");
                assert_eq!(wrong.error, problem::WRONG_CODE);
            }
            let LoginFinished::SignedIn { session, me } = anonymous
                .login_finish(&email, &code)
                .await
                .map_err(|p| p.message)?
            else {
                panic!("the test server runs with passkeys off");
            };
            assert_eq!(me.email, email);
            let server = Server::new(&base, Some(session));
            assert_eq!(server.me().await.map_err(|p| p.message)?.id, me.id);

            let mut keys = [0u8; 16];
            getrandom::fill(&mut keys).unwrap();
            let keys: String = keys.iter().map(|b| format!("{b:02x}")).collect();
            let first = server
                .add_can(&AddCan {
                    flavour: "original".into(),
                    key: format!("{keys}-1"),
                })
                .await
                .map_err(|p| p.message)?;
            let again = server
                .add_can(&AddCan {
                    flavour: "original".into(),
                    key: format!("{keys}-1"),
                })
                .await
                .map_err(|p| p.message)?;
            assert_eq!(first, again, "the same key adds the can once");
            server
                .add_can(&AddCan {
                    flavour: "mango-loco".into(),
                    key: format!("{keys}-2"),
                })
                .await
                .map_err(|p| p.message)?;
            let cans = server.cans().await.map_err(|p| p.message)?.cans;
            assert_eq!(cans.len(), 2);
            assert_eq!(cans[0].flavour, "original");

            let board = server
                .leaderboard(Ranking::Flavours)
                .await
                .map_err(|p| p.message)?;
            let row = board.rows.iter().find(|row| row.you).expect("on the board");
            assert_eq!(row.standing.flavours, 2);
            let theirs = server.collection(me.id).await.map_err(|p| p.message)?;
            assert_eq!(theirs.cans.len(), 2);

            let identity = tsclientlib::Identity::create();
            let linked = server
                .link_identity(&identity, "Flexspeak")
                .await
                .map_err(|p| p.message)?;
            let uid = identity.key().to_pub().get_uid();
            assert!(linked.identities.iter().any(|linked| linked.uid == uid));

            server.delete_me().await.map_err(|p| p.message)?;
            let gone = server.me().await.expect_err("the account is gone");
            assert_eq!(gone.error, problem::SIGNED_OUT);
            Ok(())
        })
        .unwrap();
    }
}
