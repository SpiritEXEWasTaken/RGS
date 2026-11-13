//! Scheduled maintenance and restarts, announced ahead of time like "Servers restarting in 45
//! minutes", and the gate that turns players away with a clear error while the server is down
//! or their game is out of date.
//!
//! Games poll `GET /status` (any time, even during maintenance) and show `announcement.text`
//! whenever `announcement.step` changes: 60, 45, 30, 15, 10, 5 and 1 minutes before, then 0 when
//! it starts. A restart stops the server `restart_grace_seconds` after it starts, and the run
//! script (scripts/run-server.ps1) starts it again, swapping in a new build if one is waiting.

use crate::admin::Staff;
use crate::accounts::{self, Role};
use crate::error::{ApiError, ApiResult};
use crate::state::{now, AppState};
use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::Row;
use std::time::Duration;

/// Minutes before the start when a new announcement goes out.
pub const STEPS: [i64; 7] = [60, 45, 30, 15, 10, 5, 1];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Maintenance {
    pub starts_at: i64,
    pub reason: String,
    /// Restart the server when it starts (an update). Otherwise the server stays up in
    /// maintenance mode until staff end it.
    pub restart: bool,
    pub scheduled_by: String,
}

impl Maintenance {
    pub fn active(&self) -> bool {
        now() >= self.starts_at
    }

    /// What players should be told right now, and which step of the countdown it is.
    pub fn announcement(&self) -> (i64, String) {
        let left = self.starts_at - now();
        let what = if self.restart { "Servers restarting" } else { "Maintenance starting" };
        if left <= 0 {
            return (0, self.down_message());
        }
        let minutes = (left + 59) / 60;
        let step = STEPS.iter().rev().copied().find(|s| minutes <= *s).unwrap_or(minutes);
        let when = match minutes {
            1 => "1 minute".to_owned(),
            m if m < 120 => format!("{m} minutes"),
            m => format!("{} hours", m / 60),
        };
        let reason = if self.reason.is_empty() { String::new() } else { format!(" ({})", self.reason) };
        (step, format!("{what} in {when}{reason}."))
    }

    pub fn down_message(&self) -> String {
        if self.restart {
            "RGS is rebooting right now. Try again in a few minutes.".into()
        } else {
            let reason = if self.reason.is_empty() { String::new() } else { format!(" ({})", self.reason) };
            format!("RGS is down for maintenance right now{reason}. Try again later.")
        }
    }
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/status", get(status))
}

pub fn admin_routes() -> Router<AppState> {
    Router::new().route("/maintenance", get(admin_get).post(schedule).delete(cancel))
}

/// Public: the server's state, any countdown, and the versions each game needs.
async fn status(State(state): State<AppState>) -> Json<Value> {
    let m = state.maintenance.lock().unwrap().clone();
    let status = match &m {
        Some(m) if m.active() => "maintenance",
        Some(_) => "maintenance_scheduled",
        None => "online",
    };
    let maintenance = m.map(|m| {
        let (step, text) = m.announcement();
        json!({
            "starts_at": m.starts_at,
            "seconds_left": (m.starts_at - now()).max(0),
            "reason": m.reason,
            "restart": m.restart,
            // Show a popup when `step` changes; 0 means it has started.
            "announcement": { "step": step, "text": text, "show": step <= 60 },
        })
    });
    Json(json!({
        "status": status,
        "server_version": env!("CARGO_PKG_VERSION"),
        "now": now(),
        "maintenance": maintenance,
        "games": state.config.games,
    }))
}

// ---------- Gate ----------

/// Runs before every API route. During maintenance only staff can sign in, and the status and
/// admin routes stay open; everyone else gets `maintenance`. A game older than its
/// `min_version` gets `update_required`.
pub async fn gate(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let path = req.uri().path().trim_start_matches("/api/v1");
    let always_open = matches!(path, "/health" | "/status" | "/auth/login" | "/auth/login/2fa" | "/auth/logout" | "/auth/me")
        || path.starts_with("/admin");

    if !always_open {
        if let Some(message) = down_message(&state) {
            return ApiError::Maintenance(message).into_response();
        }
    }
    if let Err(e) = check_version(&state, req.headers()) {
        return e.into_response();
    }
    next.run(req).await
}

/// The message to turn players away with, if maintenance has started.
pub fn down_message(state: &AppState) -> Option<String> {
    state.maintenance.lock().unwrap().as_ref().filter(|m| m.active()).map(|m| m.down_message())
}

fn check_version(state: &AppState, headers: &axum::http::HeaderMap) -> ApiResult<()> {
    let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).map(str::trim);
    let (Some(game_id), Some(version)) = (header("x-game-id"), header("x-game-version")) else {
        return Ok(());
    };
    let Some(game) = state.config.games.iter().find(|g| g.id.eq_ignore_ascii_case(game_id)) else {
        return Ok(());
    };
    if older(version, &game.min_version) {
        return Err(ApiError::UpdateRequired(format!(
            "Update {} to keep playing. You have version {version}, the newest is {}.",
            game.name, game.latest_version
        )));
    }
    Ok(())
}

/// Compares dotted versions number by number: "0.2" < "0.10" < "1.0".
pub fn older(have: &str, need: &str) -> bool {
    let parts = |v: &str| v.split('.').map(|p| p.trim().parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
    let (a, b) = (parts(have), parts(need));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x < y;
        }
    }
    false
}

// ---------- Admin ----------

async fn admin_get(State(state): State<AppState>, _staff: Staff) -> Json<Value> {
    let m = state.maintenance.lock().unwrap().clone();
    Json(json!({ "maintenance": m.map(|m| { let (step, text) = m.announcement(); json!({ "window": m, "step": step, "text": text }) }) }))
}

#[derive(Deserialize)]
struct ScheduleBody {
    /// Minutes from now; 60 gives the full countdown. 0 starts right away.
    #[serde(default = "sixty")]
    in_minutes: i64,
    #[serde(default)]
    reason: String,
    #[serde(default = "yes")]
    restart: bool,
}

fn sixty() -> i64 {
    60
}
fn yes() -> bool {
    true
}

async fn schedule(State(state): State<AppState>, staff: Staff, Json(body): Json<ScheduleBody>) -> ApiResult<Json<Value>> {
    if staff.0.account.role < Role::Admin {
        return Err(ApiError::Forbidden("Only admins can schedule maintenance.".into()));
    }
    if !(0..=7 * 24 * 60).contains(&body.in_minutes) {
        return Err(ApiError::BadRequest("Pick a time within the next week.".into()));
    }
    let m = Maintenance {
        starts_at: now() + body.in_minutes * 60,
        reason: body.reason.trim().chars().take(120).collect(),
        restart: body.restart,
        scheduled_by: staff.0.account.username.clone(),
    };
    save(&state, Some(&m)).await?;
    *state.maintenance.lock().unwrap() = Some(m.clone());
    let kind = if m.restart { "restart" } else { "maintenance" };
    accounts::audit(&state.db, staff.0.account.id, "schedule_maintenance", None, &format!("{kind} in {} min: {}", body.in_minutes, m.reason)).await?;
    tracing::info!("{kind} scheduled by {} in {} minutes", m.scheduled_by, body.in_minutes);
    Ok(Json(json!({ "ok": true, "maintenance": m })))
}

async fn cancel(State(state): State<AppState>, staff: Staff) -> ApiResult<Json<Value>> {
    if staff.0.account.role < Role::Admin {
        return Err(ApiError::Forbidden("Only admins can end maintenance.".into()));
    }
    save(&state, None).await?;
    *state.maintenance.lock().unwrap() = None;
    accounts::audit(&state.db, staff.0.account.id, "end_maintenance", None, "").await?;
    Ok(Json(json!({ "ok": true })))
}

// ---------- Storage and the restart timer ----------

async fn save(state: &AppState, m: Option<&Maintenance>) -> ApiResult<()> {
    match m {
        Some(m) => {
            sqlx::query("INSERT INTO settings (key, value) VALUES ('maintenance', ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
                .bind(serde_json::to_string(m).map_err(anyhow::Error::from)?)
                .execute(&state.db)
                .await?;
        }
        None => {
            sqlx::query("DELETE FROM settings WHERE key = 'maintenance'").execute(&state.db).await?;
        }
    }
    Ok(())
}

/// At startup: a restart that already happened is over (that's why we're starting); anything
/// else still scheduled or in progress carries on.
pub async fn load(db: &sqlx::SqlitePool) -> anyhow::Result<Option<Maintenance>> {
    let row = sqlx::query("SELECT value FROM settings WHERE key = 'maintenance'").fetch_optional(db).await?;
    let Some(row) = row else { return Ok(None) };
    let m: Maintenance = serde_json::from_str(&row.get::<String, _>("value"))?;
    if m.restart && m.active() {
        sqlx::query("DELETE FROM settings WHERE key = 'maintenance'").execute(db).await?;
        tracing::info!("back up after the scheduled restart");
        return Ok(None);
    }
    Ok(Some(m))
}

/// Logs each countdown step, and stops the server once a restart's grace period is over.
pub fn spawn_timer(state: AppState) {
    tokio::spawn(async move {
        let mut last_step = i64::MAX;
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let Some(m) = state.maintenance.lock().unwrap().clone() else {
                last_step = i64::MAX;
                continue;
            };
            let (step, text) = m.announcement();
            if step != last_step && step <= 60 {
                tracing::info!("announcement: {text}");
                last_step = step;
            }
            if m.restart && now() >= m.starts_at + state.config.restart_grace_seconds {
                tracing::info!("restarting now");
                state.shutdown.notify_waiters();
                return;
            }
        }
    });
}
