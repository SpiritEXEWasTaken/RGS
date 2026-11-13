//! The relay connection to the RGS website. With `hub_url` and `hub_key` set, the server
//! keeps one WebSocket open *out* to the website. People and games then reach this server at
//! `<website>/rgs/s/<server id>/api/v1/...`: the website sends each request down this socket,
//! the server answers it with its own API, and the answer goes back the same way.
//! Nothing needs to be opened on the server's network, and its address stays private.

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{HeaderName, HeaderValue, Method, Request};
use axum::Router;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::json;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tower::ServiceExt;

const MAX_RESPONSE: usize = 900 * 1024;

#[derive(Deserialize)]
struct Incoming {
    t: String,
    #[serde(default)]
    id: String,
    #[serde(default)]
    method: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    headers: std::collections::HashMap<String, String>,
    #[serde(default)]
    client_ip: String,
    body: Option<String>,
}

pub fn spawn(hub_url: String, key: String, app: Router) {
    // https://site/rgs/api -> wss://site/rgs/api/link
    let ws_url = format!("{}/link", hub_url.trim_end_matches('/')).replacen("https://", "wss://", 1).replacen("http://", "ws://", 1);
    tokio::spawn(async move {
        let mut wait = 5u64;
        loop {
            match run(&ws_url, &key, app.clone()).await {
                Ok(()) => {
                    tracing::info!("relay connection to the RGS website closed; reconnecting");
                    wait = 5;
                }
                Err(e) => {
                    tracing::warn!("relay connection to the RGS website failed: {e:#} (retrying in {wait}s)");
                    wait = (wait * 2).min(60);
                }
            }
            tokio::time::sleep(Duration::from_secs(wait)).await;
        }
    });
}

async fn run(ws_url: &str, key: &str, app: Router) -> anyhow::Result<()> {
    let mut request = ws_url.into_client_request()?;
    request.headers_mut().insert("Authorization", format!("Bearer {key}").parse()?);
    let (socket, _) = tokio_tungstenite::connect_async(request).await?;
    tracing::info!("connected to the RGS website relay");
    let (mut write, mut read) = socket.split();

    // Answers come from many tasks; one writer sends them, plus a keep-alive ping.
    let (tx, mut rx) = mpsc::channel::<String>(64);
    let writer = tokio::spawn(async move {
        let mut ping = tokio::time::interval(Duration::from_secs(30));
        loop {
            tokio::select! {
                Some(text) = rx.recv() => if write.send(Message::Text(text)).await.is_err() { break },
                _ = ping.tick() => if write.send(Message::Text(json!({ "t": "ping" }).to_string())).await.is_err() { break },
                else => break,
            }
        }
    });

    while let Some(message) = read.next().await {
        let text = match message? {
            Message::Text(t) => t,
            Message::Close(_) => break,
            _ => continue,
        };
        let Ok(incoming) = serde_json::from_str::<Incoming>(&text) else { continue };
        if incoming.t != "req" {
            continue;
        }
        let (app, tx) = (app.clone(), tx.clone());
        tokio::spawn(async move {
            let id = incoming.id.clone();
            let reply = match answer(app, incoming).await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("relayed request failed: {e:#}");
                    json!({ "t": "res", "id": id, "status": 500, "content_type": "application/json",
                            "body": STANDARD.encode(json!({ "error": "internal", "message": "Something went wrong on the server." }).to_string()) })
                }
            };
            let _ = tx.send(reply.to_string()).await;
        });
    }
    writer.abort();
    Ok(())
}

/// Runs one relayed request through this server's own API, as if it came in directly.
async fn answer(app: Router, incoming: Incoming) -> anyhow::Result<serde_json::Value> {
    anyhow::ensure!(incoming.path.starts_with("/api/v1/"), "relay may only call the API");
    let body = match incoming.body {
        Some(b) => Body::from(STANDARD.decode(b)?),
        None => Body::empty(),
    };
    let mut request = Request::builder().method(Method::from_bytes(incoming.method.as_bytes())?).uri(&incoming.path).body(body)?;
    for (name, value) in &incoming.headers {
        request.headers_mut().insert(HeaderName::from_bytes(name.as_bytes())?, HeaderValue::from_str(value)?);
    }
    // The visitor's address as the website saw it, for rate limiting (never stored).
    let ip: IpAddr = incoming.client_ip.parse().unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    request.extensions_mut().insert(ConnectInfo(SocketAddr::new(ip, 0)));

    let response = app.oneshot(request).await?;
    let status = response.status().as_u16();
    let content_type = response.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("application/json").to_owned();
    let bytes = axum::body::to_bytes(response.into_body(), MAX_RESPONSE).await?;
    Ok(json!({ "t": "res", "id": incoming.id, "status": status, "content_type": content_type, "body": STANDARD.encode(bytes) }))
}
