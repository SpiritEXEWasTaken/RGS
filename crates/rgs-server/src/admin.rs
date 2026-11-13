//! Admin API, used by the admin panel. Every route needs a staff member signed in to the admin
//! panel with a 2FA code. Staff can only act on people below their own role, and everything
//! they change goes in the audit log.

use crate::accounts::{self, Role, PERMANENT};
use crate::auth::Authed;
use crate::error::{ApiError, ApiResult};
use crate::state::{now, AppState};
use axum::extract::{FromRequestParts, Path, Query, State};
use axum::http::request::Parts;
use axum::routing::{get, post};
use axum::{async_trait, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/stats", get(stats))
        .route("/accounts", get(list_accounts))
        .route("/accounts/:id/role", post(set_role))
        .route("/accounts/:id/ban", post(ban))
        .route("/accounts/:id/unban", post(unban))
        .route("/audit", get(audit_log))
}

/// A staff member in an admin panel session that was signed in with 2FA.
pub struct Staff(pub Authed);

#[async_trait]
impl FromRequestParts<AppState> for Staff {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let auth = Authed::from_request_parts(parts, state).await?;
        if !auth.account.role.is_staff() {
            return Err(ApiError::Forbidden("Only staff can use the admin panel.".into()));
        }
        if auth.kind != "admin" || !auth.mfa || !auth.account.mfa_enabled {
            return Err(ApiError::Forbidden("Sign in to the admin panel with your 2FA code.".into()));
        }
        Ok(Staff(auth))
    }
}

async fn stats(State(state): State<AppState>, _staff: Staff) -> ApiResult<Json<Value>> {
    let count = |sql: &'static str| {
        let db = state.db.clone();
        async move { sqlx::query(sql).fetch_one(&db).await.map(|r| r.get::<i64, _>("n")) }
    };
    let t = now();
    Ok(Json(json!({
        "accounts": count("SELECT COUNT(*) AS n FROM accounts").await?,
        "with_2fa": count("SELECT COUNT(*) AS n FROM accounts WHERE totp_secret IS NOT NULL").await?,
        "staff": count("SELECT COUNT(*) AS n FROM accounts WHERE role != 'player'").await?,
        "seen_today": sqlx::query("SELECT COUNT(*) AS n FROM accounts WHERE last_seen_at > ?").bind(t - 86_400).fetch_one(&state.db).await?.get::<i64, _>("n"),
        "banned": sqlx::query("SELECT COUNT(*) AS n FROM accounts WHERE banned_until > ?").bind(t).fetch_one(&state.db).await?.get::<i64, _>("n"),
    })))
}

#[derive(Deserialize)]
struct SearchQuery {
    #[serde(default)]
    q: String,
}

async fn list_accounts(State(state): State<AppState>, _staff: Staff, Query(query): Query<SearchQuery>) -> ApiResult<Json<Value>> {
    let found = accounts::search(&state.db, query.q.trim(), 100).await?;
    Ok(Json(json!({ "accounts": found })))
}

/// Loads the target and checks the staff member outranks them.
async fn target_below(state: &AppState, staff: &Staff, id: i64) -> ApiResult<accounts::Account> {
    let target = accounts::by_id(&state.db, id).await?.ok_or(ApiError::NotFound)?;
    if target.id == staff.0.account.id {
        return Err(ApiError::Forbidden("You can't do that to your own account here.".into()));
    }
    if target.role >= staff.0.account.role {
        return Err(ApiError::Forbidden("You can only act on people below your own role.".into()));
    }
    Ok(target)
}

#[derive(Deserialize)]
struct RoleBody {
    role: String,
}

async fn set_role(State(state): State<AppState>, staff: Staff, Path(id): Path<i64>, Json(body): Json<RoleBody>) -> ApiResult<Json<Value>> {
    let me = staff.0.account.role;
    if me < Role::Admin {
        return Err(ApiError::Forbidden("Only admins can change roles.".into()));
    }
    let role = Role::parse(&body.role).ok_or_else(|| ApiError::BadRequest("Unknown role.".into()))?;
    // Owners can make admins; nobody can hand out a role at or above their own otherwise.
    if role >= me && me != Role::Owner || role == Role::Owner {
        return Err(ApiError::Forbidden("You can't give out that role.".into()));
    }
    let target = target_below(&state, &staff, id).await?;
    sqlx::query("UPDATE accounts SET role = ? WHERE id = ?").bind(role.as_str()).bind(id).execute(&state.db).await?;
    // A demoted moderator loses admin panel access right away.
    if !role.is_staff() {
        sqlx::query("DELETE FROM sessions WHERE account_id = ? AND kind = 'admin'").bind(id).execute(&state.db).await?;
    }
    accounts::audit(&state.db, staff.0.account.id, "set_role", Some(id), &format!("{} -> {}", target.role.as_str(), role.as_str())).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct BanBody {
    /// Hours; leave out for a permanent ban (admins only).
    hours: Option<i64>,
    reason: String,
}

async fn ban(State(state): State<AppState>, staff: Staff, Path(id): Path<i64>, Json(body): Json<BanBody>) -> ApiResult<Json<Value>> {
    let reason = body.reason.trim();
    if reason.is_empty() {
        return Err(ApiError::BadRequest("Give a reason.".into()));
    }
    // Moderators can hand out bans of up to three days; longer and permanent bans are for admins.
    let until = match body.hours {
        Some(h) if h > 0 && (h <= 72 || staff.0.account.role >= Role::Admin) => now() + h * 3600,
        Some(_) => return Err(ApiError::Forbidden("Moderators can ban for up to 72 hours.".into())),
        None if staff.0.account.role >= Role::Admin => PERMANENT,
        None => return Err(ApiError::Forbidden("Only admins can ban permanently.".into())),
    };
    target_below(&state, &staff, id).await?;
    sqlx::query("UPDATE accounts SET banned_until = ?, ban_reason = ? WHERE id = ?").bind(until).bind(reason).bind(id).execute(&state.db).await?;
    sqlx::query("DELETE FROM sessions WHERE account_id = ?").bind(id).execute(&state.db).await?;
    let length = body.hours.map_or("permanent".to_owned(), |h| format!("{h}h"));
    accounts::audit(&state.db, staff.0.account.id, "ban", Some(id), &format!("{length}: {reason}")).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn unban(State(state): State<AppState>, staff: Staff, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    target_below(&state, &staff, id).await?;
    sqlx::query("UPDATE accounts SET banned_until = NULL, ban_reason = NULL WHERE id = ?").bind(id).execute(&state.db).await?;
    accounts::audit(&state.db, staff.0.account.id, "unban", Some(id), "").await?;
    Ok(Json(json!({ "ok": true })))
}

async fn audit_log(State(state): State<AppState>, _staff: Staff) -> ApiResult<Json<Value>> {
    let rows = sqlx::query(
        "SELECT a.id, a.action, a.target_id, a.details, a.at, actor.username AS actor, target.username AS target
         FROM audit_log a
         LEFT JOIN accounts actor ON actor.id = a.actor_id
         LEFT JOIN accounts target ON target.id = a.target_id
         ORDER BY a.id DESC LIMIT 200",
    )
    .fetch_all(&state.db)
    .await?;
    let entries: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "id": r.get::<i64, _>("id"),
                "action": r.get::<String, _>("action"),
                "actor": r.get::<Option<String>, _>("actor"),
                "target": r.get::<Option<String>, _>("target"),
                "details": r.get::<Option<String>, _>("details"),
                "at": r.get::<i64, _>("at"),
            })
        })
        .collect();
    Ok(Json(json!({ "entries": entries })))
}
