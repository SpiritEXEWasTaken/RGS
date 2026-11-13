//! Player reports from inside games, and the queue staff work through in the admin panel.

use crate::accounts;
use crate::admin::Staff;
use crate::auth::Authed;
use crate::error::{ApiError, ApiResult};
use crate::state::{now, AppState};
use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;

/// The reasons games offer. Anything else is filed as "other".
pub const REASONS: [&str; 5] = ["cheating", "harassment", "inappropriate_name", "griefing", "other"];

pub fn routes() -> Router<AppState> {
    Router::new().route("/reports", post(create))
}

pub fn admin_routes() -> Router<AppState> {
    Router::new().route("/reports", get(queue)).route("/reports/:id", post(handle))
}

#[derive(Deserialize)]
struct ReportBody {
    game: String,
    /// The reported player's RGS account, if they were signed in.
    target_id: Option<i64>,
    /// The name they had in the game (always sent, since not everyone is signed in).
    target_name: String,
    reason: String,
    #[serde(default)]
    details: String,
    #[serde(default)]
    context: String,
}

async fn create(State(state): State<AppState>, auth: Authed, Json(body): Json<ReportBody>) -> ApiResult<Json<Value>> {
    if !state.auth_limit.allow(&format!("report:{}", auth.account.id)) {
        return Err(ApiError::TooManyRequests);
    }
    if body.target_id == Some(auth.account.id) {
        return Err(ApiError::BadRequest("You can't report yourself.".into()));
    }
    let target_name: String = body.target_name.trim().chars().take(40).collect();
    if target_name.is_empty() {
        return Err(ApiError::BadRequest("Who are you reporting?".into()));
    }
    let reason = if REASONS.contains(&body.reason.as_str()) { body.reason.as_str() } else { "other" };
    let target_id = match body.target_id {
        Some(id) if accounts::by_id(&state.db, id).await?.is_some() => Some(id),
        _ => None,
    };
    let id: i64 = sqlx::query(
        "INSERT INTO reports (reporter_id, target_id, target_name, game, reason, details, context, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(auth.account.id)
    .bind(target_id)
    .bind(&target_name)
    .bind(body.game.chars().take(40).collect::<String>())
    .bind(reason)
    .bind(body.details.chars().take(1000).collect::<String>())
    .bind(body.context.chars().take(200).collect::<String>())
    .bind(now())
    .fetch_one(&state.db)
    .await?
    .get("id");

    if let Some(webhook) = &state.config.discord_reports_webhook {
        let target_account = match target_id {
            Some(tid) => accounts::by_id(&state.db, tid).await?.map(|a| a.username),
            None => None,
        };
        crate::discord::report(webhook, id, reason, &target_name, target_account.as_deref(), &auth.account.username, &body.game, &body.context, &body.details);
    }
    Ok(Json(json!({ "ok": true, "id": id, "message": "Thanks. Our team will look at your report." })))
}

#[derive(Deserialize)]
struct QueueQuery {
    #[serde(default = "open")]
    status: String,
}

fn open() -> String {
    "open".into()
}

async fn queue(State(state): State<AppState>, _staff: Staff, Query(q): Query<QueueQuery>) -> ApiResult<Json<Value>> {
    let rows = sqlx::query(
        "SELECT r.id, r.target_id, r.target_name, r.game, r.reason, r.details, r.context, r.status, r.created_at, r.note,
                rep.username AS reporter, t.username AS target_username, h.username AS handled_by
         FROM reports r
         JOIN accounts rep ON rep.id = r.reporter_id
         LEFT JOIN accounts t ON t.id = r.target_id
         LEFT JOIN accounts h ON h.id = r.handled_by
         WHERE r.status = ? ORDER BY r.id DESC LIMIT 200",
    )
    .bind(&q.status)
    .fetch_all(&state.db)
    .await?;
    let reports: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "id": r.get::<i64, _>("id"),
                "reporter": r.get::<String, _>("reporter"),
                "target_id": r.get::<Option<i64>, _>("target_id"),
                "target_name": r.get::<String, _>("target_name"),
                "target_username": r.get::<Option<String>, _>("target_username"),
                "game": r.get::<String, _>("game"),
                "reason": r.get::<String, _>("reason"),
                "details": r.get::<String, _>("details"),
                "context": r.get::<String, _>("context"),
                "status": r.get::<String, _>("status"),
                "created_at": r.get::<i64, _>("created_at"),
                "handled_by": r.get::<Option<String>, _>("handled_by"),
                "note": r.get::<Option<String>, _>("note"),
            })
        })
        .collect();
    let open: i64 = sqlx::query("SELECT COUNT(*) AS n FROM reports WHERE status = 'open'").fetch_one(&state.db).await?.get("n");
    Ok(Json(json!({ "reports": reports, "open": open })))
}

#[derive(Deserialize)]
struct HandleBody {
    /// "resolved" (action taken) or "dismissed" (nothing wrong).
    status: String,
    #[serde(default)]
    note: String,
}

async fn handle(State(state): State<AppState>, staff: Staff, Path(id): Path<i64>, Json(body): Json<HandleBody>) -> ApiResult<Json<Value>> {
    if body.status != "resolved" && body.status != "dismissed" {
        return Err(ApiError::BadRequest("Mark it resolved or dismissed.".into()));
    }
    let done = sqlx::query("UPDATE reports SET status = ?, note = ?, handled_by = ?, handled_at = ? WHERE id = ?")
        .bind(&body.status)
        .bind(body.note.trim())
        .bind(staff.0.account.id)
        .bind(now())
        .bind(id)
        .execute(&state.db)
        .await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    accounts::audit(&state.db, staff.0.account.id, &format!("report_{}", body.status), None, &format!("report #{id}: {}", body.note.trim())).await?;
    Ok(Json(json!({ "ok": true })))
}
