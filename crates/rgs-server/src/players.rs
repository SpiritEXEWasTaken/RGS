//! Public player lookups for games (name and staff badge, checked by everyone so badges can't be
//! faked), and the player's own settings.

use crate::accounts;
use crate::auth::Authed;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/players/:id", get(player))
        .route("/auth/me/settings", post(settings))
}

/// What other players may know: name and badge. Nothing private.
async fn player(State(state): State<AppState>, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let account = accounts::by_id(&state.db, id).await?.ok_or(ApiError::NotFound)?;
    Ok(Json(json!({ "id": account.id, "username": account.username, "badge": account.badge() })))
}

#[derive(Deserialize)]
struct SettingsBody {
    show_badge: Option<bool>,
}

async fn settings(State(state): State<AppState>, auth: Authed, Json(body): Json<SettingsBody>) -> ApiResult<Json<Value>> {
    if let Some(show) = body.show_badge {
        sqlx::query("UPDATE accounts SET show_badge = ? WHERE id = ?").bind(show as i64).bind(auth.account.id).execute(&state.db).await?;
    }
    let account = accounts::by_id(&state.db, auth.account.id).await?.ok_or(ApiError::NotFound)?;
    Ok(Json(account.public_json()))
}
