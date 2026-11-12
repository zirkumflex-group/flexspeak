//! The Monster collection server: accounts, the cans people add and the cards they roll, and
//! the leaderboard. Every call it answers is specified in `monster_core::api`.

pub mod config;
mod db;
mod error;
mod identity;
mod login;
mod mail;
mod me;
mod passkey;
mod secret;
mod shelf;

use std::{
    collections::HashMap,
    net::SocketAddr,
    ops::Deref,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicI64, Ordering},
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use axum::{
    Router,
    extract::{ConnectInfo, FromRequestParts, Request},
    http::{StatusCode, request::Parts},
    middleware::{self, Next},
    response::Response,
    routing::{delete, get, post},
};
use rusqlite::Connection;
use webauthn_rs::{Webauthn, WebauthnBuilder};

pub use config::{Config, Mail};
use error::ApiError;

/// The server's sense of time, in Unix seconds. Tests set and move it by hand.
#[derive(Clone, Default)]
pub struct Clock(Option<Arc<AtomicI64>>);

impl Clock {
    pub fn system() -> Self {
        Self(None)
    }

    /// A clock that stands at `at` until [`Clock::advance`] moves it.
    pub fn manual(at: i64) -> Self {
        Self(Some(Arc::new(AtomicI64::new(at))))
    }

    pub fn now(&self) -> i64 {
        match &self.0 {
            Some(at) => at.load(Ordering::SeqCst),
            None => SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs() as i64),
        }
    }

    /// Moves a manual clock on by `seconds`; does nothing to the system clock.
    pub fn advance(&self, seconds: i64) {
        if let Some(at) = &self.0 {
            at.fetch_add(seconds, Ordering::SeqCst);
        }
    }
}

/// Everything a request handler needs.
#[derive(Clone)]
pub struct AppState(Arc<Shared>);

pub struct Shared {
    db: Mutex<Connection>,
    config: Config,
    clock: Clock,
    mailer: mail::Mailer,
    webauthn: Webauthn,
    /// Passkey ceremonies the page has started, by ticket hash. Kept in memory: they last a
    /// few minutes, and a restart only means the page asks for a new one.
    ceremonies: Mutex<HashMap<Vec<u8>, passkey::Ceremony>>,
}

impl Deref for AppState {
    type Target = Shared;

    fn deref(&self) -> &Shared {
        &self.0
    }
}

impl AppState {
    /// Opens the database in `config.data_dir`, creating the folder if need be.
    pub fn new(config: Config, clock: Clock) -> Result<Self, String> {
        std::fs::create_dir_all(&config.data_dir)
            .map_err(|e| format!("could not create {}: {e}", config.data_dir.display()))?;
        let db =
            db::open(&config.data_dir).map_err(|e| format!("could not open the database: {e}"))?;
        let rp_id = config
            .public_url
            .host_str()
            .ok_or("PUBLIC_URL has no host")?
            .to_string();
        let webauthn = WebauthnBuilder::new(&rp_id, &config.public_url)
            .and_then(|b| b.rp_name("Flexspeak").build())
            .map_err(|e| format!("PUBLIC_URL doesn't work for passkeys: {e}"))?;
        Ok(Self(Arc::new(Shared {
            db: Mutex::new(db),
            mailer: mail::Mailer::new(config.mail.clone()),
            config,
            clock,
            webauthn,
            ceremonies: Mutex::default(),
        })))
    }

    fn db(&self) -> MutexGuard<'_, Connection> {
        // A panic while holding the lock leaves the connection itself usable.
        self.db.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn now(&self) -> i64 {
        self.clock.now()
    }
}

/// Every route the server answers.
pub fn router(state: AppState) -> Router {
    let v1 = Router::new()
        .route("/login/start", post(login::start))
        .route("/login/finish", post(login::finish))
        .route("/login/passkey", post(login::with_passkey))
        .route("/login/poll", post(login::poll))
        .route(
            "/me",
            get(me::get_me).patch(me::update_me).delete(me::delete_me),
        )
        .route("/session", delete(me::sign_out))
        .route("/identities/challenge", post(identity::challenge))
        .route("/identities", post(identity::link))
        .route("/identities/unlink", post(identity::unlink))
        .route("/cans", get(shelf::list).post(shelf::add))
        .route("/cans/import", post(shelf::import))
        .route("/cans/{id}", delete(shelf::remove))
        .route("/leaderboard", get(shelf::leaderboard))
        .route("/users/{id}/cans", get(shelf::collection))
        .route("/passkeys/options", post(passkey::options))
        .route("/passkeys/finish", post(passkey::finish));
    Router::new()
        .nest("/v1", v1)
        .route("/passkey", get(passkey::page))
        .route("/health", get(health))
        .fallback(|| async { ApiError::not_found("There's nothing here.") })
        .layer(middleware::from_fn(log_request))
        .with_state(state)
}

async fn health() -> StatusCode {
    StatusCode::OK
}

/// One line for each request: method, path, status and time. Paths never hold a secret:
/// tickets ride in bodies and the page's fragment, sessions in a header.
async fn log_request(req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let started = Instant::now();
    let res = next.run(req).await;
    tracing::info!(
        "{method} {path} {} {}ms",
        res.status().as_u16(),
        started.elapsed().as_millis()
    );
    res
}

/// The caller's address, for per-address limits. Behind a proxy it is the last address the
/// proxy put in `X-Forwarded-For`, since earlier ones are whatever the caller wrote.
pub(crate) struct ClientIp(Option<String>);

impl<S: Send + Sync> FromRequestParts<S> for ClientIp {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        let forwarded = parts
            .headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.rsplit(',').next())
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        let peer = || {
            parts
                .extensions
                .get::<ConnectInfo<SocketAddr>>()
                .map(|ConnectInfo(addr)| addr.ip().to_string())
        };
        Ok(Self(forwarded.or_else(peer)))
    }
}

/// How many times `bucket` was hit in the last `window` seconds.
fn hits(conn: &Connection, bucket: &str, now: i64, window: i64) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM hits WHERE bucket = ?1 AND at > ?2",
        rusqlite::params![bucket, now - window],
        |row| row.get(0),
    )
}

fn hit(conn: &Connection, bucket: &str, now: i64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO hits (bucket, at) VALUES (?1, ?2)",
        rusqlite::params![bucket, now],
    )?;
    Ok(())
}
