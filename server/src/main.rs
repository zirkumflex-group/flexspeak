use std::net::SocketAddr;

use flexspeak_monster::{AppState, Clock, Config, Mail, router};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    if std::env::args().nth(1).as_deref() == Some("healthcheck") {
        std::process::exit(if healthy() { 0 } else { 1 });
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
