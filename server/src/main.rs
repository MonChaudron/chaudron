use std::{
    env,
    io::{self, IsTerminal, Read, Write},
    net::{SocketAddr, TcpStream},
    path::PathBuf,
    process::ExitCode,
    time::Duration,
};

use axum::serve::ListenerExt;
use chaudron_server::{AppState, app, config::Config};
use sqlx::postgres::PgPoolOptions;

fn port() -> u16 {
    env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080)
}

fn main() -> ExitCode {
    // `chaudron healthcheck` : l'image `FROM scratch` n'a pas de curl.
    if env::args().nth(1).as_deref() == Some("healthcheck") {
        return healthcheck();
    }

    let level = env::var("LOG_LEVEL")
        .ok()
        .and_then(|l| l.parse().ok())
        .unwrap_or(tracing::Level::INFO);
    tracing_subscriber::fmt()
        .with_max_level(level)
        .with_ansi(io::stdout().is_terminal())
        .init();

    // Deux threads suffisent pour un foyer et gardent la RAM au plus bas.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime tokio");

    match runtime.block_on(serve()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!("{err}");
            ExitCode::FAILURE
        }
    }
}

async fn serve() -> Result<(), Box<dyn std::error::Error>> {
    let database_url = env::var("DATABASE_URL").map_err(|_| "DATABASE_URL manquant")?;
    let max_connections = env::var("DATABASE_MAX_CONNECTIONS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(3);
    let web_dir = PathBuf::from(env::var("WEB_DIR").unwrap_or_else(|_| "web".into()));
    let config = Config::from_env(port())?;
    if config.fake_oauth {
        tracing::warn!(
            "AUTH_FAKE : fournisseur de connexion factice actif, n'importe qui peut se connecter"
        );
    }
    let providers: Vec<_> = config.providers.iter().map(|p| p.id.as_str()).collect();
    tracing::info!(public_url = %config.public_url, ?providers, signup = ?config.signup, "configuration");

    let db = PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(2))
        .connect(&database_url)
        .await?;
    sqlx::migrate!().run(&db).await?;
    chaudron_server::ciqual::sync(&db).await?;

    let addr = SocketAddr::from(([0, 0, 0, 0], port()));
    // Sans TCP_NODELAY, les fichiers servis en plusieurs écritures attendent ~40 ms (Nagle + ACK retardé).
    let listener = tokio::net::TcpListener::bind(addr).await?.tap_io(|tcp| {
        if let Err(err) = tcp.set_nodelay(true) {
            tracing::warn!(%err, "TCP_NODELAY");
        }
    });
    tracing::info!(%addr, web_dir = %web_dir.display(), "Chaudron démarré");

    axum::serve(listener, app(AppState::new(db, config), &web_dir))
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}

async fn shutdown() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("signal SIGTERM");
        tokio::select! {
            _ = ctrl_c => {},
            _ = term.recv() => {},
        }
    }
    #[cfg(not(unix))]
    let _ = ctrl_c.await;
    tracing::info!("arrêt");
}

fn healthcheck() -> ExitCode {
    let check = || -> std::io::Result<bool> {
        let addr = SocketAddr::from(([127, 0, 0, 1], port()));
        let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2))?;
        stream.set_read_timeout(Some(Duration::from_secs(3)))?;
        stream.write_all(b"GET /health HTTP/1.0\r\nHost: localhost\r\n\r\n")?;
        let mut response = String::new();
        stream.read_to_string(&mut response)?;
        Ok(response.split(' ').nth(1) == Some("200"))
    };
    match check() {
        Ok(true) => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}
