//! RGS server: accounts, logins, 2FA, headset device codes, roles, saves, reports and maintenance.
//! It serves only the API; the admin panel and player pages are on the RGS website, which reaches
//! this server through the relay (see relay.rs). The first account to sign up becomes the owner.

mod accounts;
mod admin;
mod auth;
mod config;
mod crypto;
mod device;
mod discord;
mod error;
mod hub;
mod maintenance;
mod mfa;
mod players;
mod ratelimit;
mod relay;
mod reports;
mod saves;
mod state;

use axum::http::{header, HeaderValue, Method};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "rgs_server=info,tower_http=warn".into()))
        .init();

    let config = config::Config::load()?;
    std::fs::create_dir_all(&config.data_dir)?;

    let db_path = config.data_dir.join("rgs.db");
    let db = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&db_path)
                .create_if_missing(true)
                .foreign_keys(true)
                .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal),
        )
        .await?;
    sqlx::migrate!("./migrations").run(&db).await?;

    let secrets = crypto::SecretBox::load_or_create(&config.data_dir)?;
    let listen: SocketAddr = config.listen.parse()?;
    let scheduled = maintenance::load(&db).await?;
    let state: state::AppState = Arc::new(state::State::new(config, db, secrets, scheduled));
    maintenance::spawn_timer(state.clone());
    hub::spawn(state.clone());

    let app = router(state.clone());
    if let (Some(url), Some(key)) = (state.config.hub_url.clone(), state.config.hub_key.clone()) {
        relay::spawn(url, key, app.clone());
    }
    let listener = tokio::net::TcpListener::bind(listen).await?;
    tracing::info!("RGS {} listening on http://{listen} (API only; the admin panel is on the RGS website)", env!("CARGO_PKG_VERSION"));
    let stop = state.clone();
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(async move {
            tokio::select! {
                _ = stop.shutdown.notified() => {}
                _ = stop_signal() => tracing::info!("asked to stop"),
            }
        })
        .await?;
    tracing::info!("stopped");
    Ok(())
}

/// Ctrl+C anywhere, and SIGTERM on Mac and Linux (what systemd and launchd send to stop a service).
async fn stop_signal() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}

fn router(state: state::AppState) -> Router {
    let api = Router::new()
        .route("/health", get(|| async { Json(json!({ "ok": true, "version": env!("CARGO_PKG_VERSION") })) }))
        .merge(auth::routes())
        .merge(mfa::routes())
        .merge(device::routes())
        .merge(maintenance::routes())
        .merge(players::routes())
        .merge(saves::routes())
        .merge(reports::routes())
        .nest("/admin", admin::routes().merge(maintenance::admin_routes()).merge(reports::admin_routes()))
        // Turns players away during maintenance, and old game versions always.
        .layer(axum::middleware::from_fn_with_state(state.clone(), maintenance::gate));

    // The website lives on another domain (Cloudflare Pages); only it may call the API from a browser.
    let origins: Vec<HeaderValue> = state.config.allowed_origins.iter().filter_map(|o| o.parse().ok()).collect();
    let cors = CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]);

    Router::new()
        .nest("/api/v1", api)
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
