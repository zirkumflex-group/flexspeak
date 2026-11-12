use std::net::SocketAddr;

use flexspeak_monster::{AppState, Clock, Config, Mail, backup, delete_account, router};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("healthcheck") => std::process::exit(if healthy() { 0 } else { 1 }),
        Some("backup") => std::process::exit(backup_to(args.get(1))),
        Some("delete-account") => std::process::exit(delete_account_of(args.get(1))),
        _ => {}
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = match Config::from_env() {
        Ok(config) => config,
        Err(problem) => {
            tracing::error!("not starting: {problem}");
            std::process::exit(2);
        }
    };
    if let Mail::Dev(code) = &config.mail {
        tracing::warn!(
            "DEV_CODE is set: every sign-in code is {code} and no email is sent. \
             Anyone can sign in as anyone. Never run a public server like this."
        );
    }
    if !config.passkeys {
        tracing::warn!("PASSKEYS=off: a sign-in code alone signs people in.");
    }
    let port = config.port;
    tracing::info!(
        "data in {}, public at {}",
        config.data_dir.display(),
        config.public_url
    );
    let state = match AppState::new(config, Clock::system()) {
        Ok(state) => state,
        Err(problem) => {
            tracing::error!("not starting: {problem}");
            std::process::exit(2);
        }
    };

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(e) => {
            tracing::error!("not starting: can't listen on {addr}: {e}");
            std::process::exit(2);
        }
    };
    tracing::info!("listening on {addr}");
    let app = router(state).into_make_service_with_connect_info::<SocketAddr>();
    if let Err(e) = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown())
        .await
    {
        tracing::error!("server stopped: {e}");
        std::process::exit(1);
    }
}

/// Ctrl-C, or the SIGTERM a container host sends before it stops the container.
async fn shutdown() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut term) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            term.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {},
        () = term => {},
    }
    tracing::info!("shutting down");
}

/// `flexspeak-monster healthcheck`: asks the running server for `/health`, so the container
/// image needs no HTTP client of its own.
fn healthy() -> bool {
    use std::{
        io::{Read, Write},
        net::TcpStream,
        time::Duration,
    };
    let port = std::env::var("PORT")
        .ok()
        .and_then(|p| p.trim().parse::<u16>().ok())
        .unwrap_or(8787);
    let Ok(mut stream) = TcpStream::connect_timeout(
        &SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_secs(3),
    ) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    if stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .is_err()
    {
        return false;
    }
    let mut head = [0u8; 12];
    stream.read_exact(&mut head).is_ok() && head.ends_with(b" 200")
}

/// `flexspeak-monster backup <file>`: writes a consistent copy of the database in `DATA_DIR`
/// to `<file>` while the server keeps running, so the image needs no `sqlite3` tool.
fn backup_to(dest: Option<&String>) -> i32 {
    let Some(dest) = dest else {
        eprintln!("usage: flexspeak-monster backup <file>");
        return 2;
    };
    match backup(&data_dir(), std::path::Path::new(dest)) {
        Ok(()) => {
            println!("backed up to {dest}");
            0
        }
        Err(problem) => {
            eprintln!("backup failed: {problem}");
            1
        }
    }
}

/// `flexspeak-monster delete-account <email>`: removes that account and everything in it,
/// as the app's own "delete account" does, for a request made to the server's owner.
fn delete_account_of(email: Option<&String>) -> i32 {
    let Some(email) = email else {
        eprintln!("usage: flexspeak-monster delete-account <email>");
        return 2;
    };
    match delete_account(&data_dir(), email) {
        Ok(true) => {
            println!("deleted the account of {email} and everything in it");
            0
        }
        Ok(false) => {
            eprintln!("there is no account with the address {email}");
            1
        }
        Err(problem) => {
            eprintln!("could not delete: {problem}");
            1
        }
    }
}

/// `DATA_DIR`, as the server reads it.
fn data_dir() -> std::path::PathBuf {
    std::env::var("DATA_DIR")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "/data".into())
        .into()
}
