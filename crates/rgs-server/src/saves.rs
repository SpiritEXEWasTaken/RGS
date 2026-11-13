//! Cloud saves: named slots per game, per account. The game decides what goes in them.

use crate::auth::Authed;
use crate::error::{ApiError, ApiResult};
use crate::state::{now, AppState};
use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;

const MAX_BYTES: usize = 256 * 1024;
const MAX_SLOTS: i64 = 200;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/saves/:game", get(list))
        .route("/saves/:game/:slot", get(load).put(store).delete(remove))
}

fn check_names(game: &str, slot: &str) -> ApiResult<()> {
    let ok = |s: &str, max: usize| !s.trim().is_empty() && s.chars().count() <= max;
    if !ok(game, 40) || !ok(slot, 60) {
        return Err(ApiError::BadRequest("Save names need 1 to 60 characters.".into()));
    }
    Ok(())
}

/// Every slot for this game, newest first (without the data).
async fn list(State(state): State<AppState>, auth: Authed, Path(game): Path<String>) -> ApiResult<Json<Value>> {
    let rows = sqlx::query("SELECT slot, updated_at, LENGTH(data) AS size FROM saves WHERE account_id = ? AND game = ? ORDER BY updated_at DESC")
        .bind(auth.account.id)
        .bind(&game)
        .fetch_all(&state.db)
        .await?;
    let saves: Vec<Value> = rows
        .iter()
        .map(|r| json!({ "slot": r.get::<String, _>("slot"), "updated_at": r.get::<i64, _>("updated_at"), "size": r.get::<i64, _>("size") }))
        .collect();
    Ok(Json(json!({ "saves": saves })))
}

async fn load(State(state): State<AppState>, auth: Authed, Path((game, slot)): Path<(String, String)>) -> ApiResult<Json<Value>> {
    let row = sqlx::query("SELECT data, updated_at FROM saves WHERE account_id = ? AND game = ? AND slot = ?")
        .bind(auth.account.id)
        .bind(&game)
        .bind(&slot)
        .fetch_optional(&state.db)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(json!({ "slot": slot, "data": row.get::<String, _>("data"), "updated_at": row.get::<i64, _>("updated_at") })))
}

#[derive(Deserialize)]
struct StoreBody {
    data: String,
}

async fn store(State(state): State<AppState>, auth: Authed, Path((game, slot)): Path<(String, String)>, Json(body): Json<StoreBody>) -> ApiResult<Json<Value>> {
    check_names(&game, &slot)?;
    if body.data.len() > MAX_BYTES {
        return Err(ApiError::BadRequest("That save is too big (256 KB at most).".into()));
    }
    let exists = sqlx::query("SELECT 1 FROM saves WHERE account_id = ? AND game = ? AND slot = ?")
        .bind(auth.account.id)
        .bind(&game)
        .bind(&slot)
        .fetch_optional(&state.db)
        .await?
        .is_some();
    if !exists {
        let count: i64 = sqlx::query("SELECT COUNT(*) AS n FROM saves WHERE account_id = ? AND game = ?")
            .bind(auth.account.id)
            .bind(&game)
            .fetch_one(&state.db)
            .await?
            .get("n");
        if count >= MAX_SLOTS {
            return Err(ApiError::Conflict(format!("You have {MAX_SLOTS} saves already. Delete one first.")));
        }
    }
    sqlx::query(
        "INSERT INTO saves (account_id, game, slot, data, updated_at) VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(account_id, game, slot) DO UPDATE SET data = excluded.data, updated_at = excluded.updated_at",
    )
    .bind(auth.account.id)
    .bind(&game)
    .bind(&slot)
    .bind(&body.data)
    .bind(now())
    .execute(&state.db)
    .await?;
    Ok(Json(json!({ "ok": true })))
}

async fn remove(State(state): State<AppState>, auth: Authed, Path((game, slot)): Path<(String, String)>) -> ApiResult<Json<Value>> {
    sqlx::query("DELETE FROM saves WHERE account_id = ? AND game = ? AND slot = ?")
        .bind(auth.account.id)
        .bind(&game)
        .bind(&slot)
        .execute(&state.db)
        .await?;
    Ok(Json(json!({ "ok": true })))
}
