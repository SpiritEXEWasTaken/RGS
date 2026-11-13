//! Device login for headsets: nobody types a password in VR.
//! 1. The game calls `/device/start` and shows the short `user_code`.
//! 2. The player signs in on the website and enters that code (`/device/approve`).
//! 3. The game, polling `/device/poll` with its secret `device_code`, gets its own login token.

use crate::auth::{create_session, Authed};
use crate::crypto;
use crate::error::{ApiError, ApiResult};
use crate::state::{now, AppState};
use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;

const CODE_SECONDS: i64 = 600;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/device/start", post(start))
        .route("/device/poll", post(poll))
        .route("/device/approve", post(approve))
}

#[derive(Deserialize)]
struct StartBody {
    /// Shown in the player's list of logins, e.g. "Quest 3".
    device_name: String,
}

async fn start(State(state): State<AppState>, Json(body): Json<StartBody>) -> ApiResult<Json<Value>> {
    let name: String = body.device_name.trim().chars().take(40).collect();
    let name = if name.is_empty() { "Device".to_owned() } else { name };
    sqlx::query("DELETE FROM device_codes WHERE expires_at < ?").bind(now()).execute(&state.db).await?;

    let device_code = crypto::new_token();
    let user_code = crypto::human_code(6);
    sqlx::query("INSERT INTO device_codes (device_hash, user_code, device_name, created_at, expires_at) VALUES (?, ?, ?, ?, ?)")
        .bind(crypto::sha256_hex(&device_code))
        .bind(&user_code)
        .bind(&name)
        .bind(now())
        .bind(now() + CODE_SECONDS)
        .execute(&state.db)
        .await?;
    Ok(Json(json!({
        "device_code": device_code,
        // Shown as "ABC-DEF"; the dash is optional when typing it.
        "user_code": format!("{}-{}", &user_code[..3], &user_code[3..]),
        "expires_in": CODE_SECONDS,
        "poll_every": 3,
    })))
}

#[derive(Deserialize)]
struct PollBody {
    device_code: String,
}

async fn poll(State(state): State<AppState>, Json(body): Json<PollBody>) -> ApiResult<Json<Value>> {
    let row = sqlx::query("SELECT id, account_id, device_name FROM device_codes WHERE device_hash = ? AND expires_at > ?")
        .bind(crypto::sha256_hex(&body.device_code))
        .bind(now())
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| ApiError::Unauthorized("That code expired. Start again.".into()))?;
    let Some(account_id) = row.get::<Option<i64>, _>("account_id") else {
        return Ok(Json(json!({ "approved": false })));
    };
    // Used once: the device gets its own login and the code is gone.
    sqlx::query("DELETE FROM device_codes WHERE id = ?").bind(row.get::<i64, _>("id")).execute(&state.db).await?;
    let token = create_session(&state, account_id, &row.get::<String, _>("device_name"), false).await?;
    Ok(Json(json!({ "approved": true, "token": token })))
}

#[derive(Deserialize)]
struct ApproveBody {
    user_code: String,
}

async fn approve(State(state): State<AppState>, auth: Authed, Json(body): Json<ApproveBody>) -> ApiResult<Json<Value>> {
    if !state.auth_limit.allow(&format!("device:{}", auth.account.id)) {
        return Err(ApiError::TooManyRequests);
    }
    let code: String = body.user_code.to_uppercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let done = sqlx::query("UPDATE device_codes SET account_id = ? WHERE user_code = ? AND account_id IS NULL AND expires_at > ?")
        .bind(auth.account.id)
        .bind(&code)
        .bind(now())
        .execute(&state.db)
        .await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::BadRequest("That code isn't valid or has expired.".into()));
    }
    Ok(Json(json!({ "ok": true })))
}
