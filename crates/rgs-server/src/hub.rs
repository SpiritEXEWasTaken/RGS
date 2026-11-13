//! Optional link to an RGS account on the RGS website. With `hub_url` and `hub_key` in
//! rgs.toml, the server reports its status there every minute, so its owner (and only its
//! owner) can see it from anywhere. The key comes from "Link a server" on the website.
//! Nothing about players is sent: only counts, the version and the maintenance state.

use crate::state::{now, AppState};
use serde_json::json;
use sqlx::Row;
use std::time::Duration;

pub fn spawn(state: AppState) {
    let (Some(url), Some(key)) = (state.config.hub_url.clone(), state.config.hub_key.clone()) else {
        return;
    };
    let url = format!("{}/heartbeat", url.trim_end_matches('/'));
    tokio::spawn(async move {
        let client = reqwest::Client::builder().timeout(Duration::from_secs(15)).build().expect("http client");
        let mut warned = false;
        loop {
            match report(&state).await {
                Ok(body) => match client.post(&url).bearer_auth(&key).json(&body).send().await {
                    Ok(r) if r.status().is_success() => {
                        if warned {
                            tracing::info!("reporting to the RGS website again");
                        }
                        warned = false;
                    }
                    Ok(r) if !warned => {
                        tracing::warn!("the RGS website refused this server's report ({}); check hub_key in rgs.toml", r.status());
                        warned = true;
                    }
                    Err(e) if !warned => {
                        tracing::warn!("couldn't reach the RGS website: {e}");
                        warned = true;
                    }
                    _ => {}
                },
                Err(e) => tracing::warn!("couldn't build the status report: {e:#}"),
            }
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });
}

async fn report(state: &AppState) -> anyhow::Result<serde_json::Value> {
    let count = |sql: &'static str| {
        let db = state.db.clone();
        async move { sqlx::query(sql).fetch_one(&db).await.map(|r| r.get::<i64, _>("n")) }
    };
    let active: i64 = sqlx::query("SELECT COUNT(*) AS n FROM accounts WHERE last_seen_at > ?")
        .bind(now() - 600)
        .fetch_one(&state.db)
        .await?
        .get("n");
    let maintenance = state.maintenance.lock().unwrap().clone();
    let (status, announcement) = match &maintenance {
        Some(m) if m.active() => ("maintenance", m.announcement().1),
        Some(m) => ("maintenance_scheduled", m.announcement().1),
        None => ("online", String::new()),
    };
    Ok(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "status": status,
        "announcement": announcement,
        "accounts": count("SELECT COUNT(*) AS n FROM accounts").await?,
        "active_players": active,
        "open_reports": count("SELECT COUNT(*) AS n FROM reports WHERE status = 'open'").await?,
        "games": state.config.games.iter().map(|g| format!("{} {}", g.name, g.latest_version)).collect::<Vec<_>>(),
    }))
}
