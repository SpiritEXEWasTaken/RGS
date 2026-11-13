//! Sign-up, login (with 2FA when it's on), sessions, and the `Authed` extractor.

use crate::accounts::{self, Account, Role};
use crate::crypto;
use crate::error::{ApiError, ApiResult};
use crate::mfa;
use crate::state::{now, AppState, MfaTicket};
use axum::extract::{ConnectInfo, FromRequestParts, Path, State};
use axum::http::request::Parts;
use axum::http::HeaderMap;
use axum::routing::{delete, get, post};
use axum::{async_trait, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/auth/login/2fa", post(login_2fa))
        .route("/auth/logout", post(logout))
        .route("/auth/me", get(me))
        .route("/auth/sessions", get(list_sessions))
        .route("/auth/sessions/:id", delete(revoke_session))
}

/// A signed-in request: the account and the session it used.
pub struct Authed {
    pub account: Account,
    pub session_id: i64,
    /// This session was signed in with a 2FA code.
    pub mfa: bool,
    /// "web", "admin", or a device name.
    pub kind: String,
}

#[async_trait]
impl FromRequestParts<AppState> for Authed {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or_else(|| ApiError::Unauthorized("Sign in first.".into()))?;

        let row = sqlx::query("SELECT id, account_id, mfa, kind FROM sessions WHERE token_hash = ? AND expires_at > ?")
            .bind(crypto::sha256_hex(token))
            .bind(now())
            .fetch_optional(&state.db)
            .await?
            .ok_or_else(|| ApiError::Unauthorized("Your login has expired. Sign in again.".into()))?;

        let account = accounts::by_id(&state.db, row.get("account_id"))
            .await?
            .ok_or_else(|| ApiError::Unauthorized("Account not found.".into()))?;
        if account.is_banned() {
            return Err(ApiError::Banned(ban_message(&account)));
        }
        sqlx::query("UPDATE accounts SET last_seen_at = ? WHERE id = ?").bind(now()).bind(account.id).execute(&state.db).await?;

        Ok(Authed { session_id: row.get("id"), mfa: row.get::<i64, _>("mfa") != 0, kind: row.get("kind"), account })
    }
}

pub fn ban_message(account: &Account) -> String {
    let reason = account.ban_reason.clone().unwrap_or_else(|| "No reason given".into());
    match account.banned_until {
        Some(accounts::PERMANENT) => format!("This account is banned. Reason: {reason}"),
        Some(until) => format!("This account is banned for another {} hours. Reason: {reason}", ((until - now()) / 3600).max(1)),
        None => String::new(),
    }
}

/// The client's address, used only in memory for rate limiting.
pub fn client_ip(state: &AppState, headers: &HeaderMap, addr: SocketAddr) -> String {
    if state.config.behind_cloudflare {
        if let Some(ip) = headers.get("cf-connecting-ip").and_then(|v| v.to_str().ok()) {
            return ip.to_owned();
        }
    }
    addr.ip().to_string()
}

/// Creates a session and returns its bearer token (shown once; only its hash is stored).
pub async fn create_session(state: &AppState, account_id: i64, kind: &str, mfa: bool) -> ApiResult<String> {
    let lifetime = match kind {
        "admin" => state.config.admin_session_hours * 3600,
        "web" => state.config.session_days * 86_400,
        _ => state.config.device_session_days * 86_400,
    };
    let token = crypto::new_token();
    sqlx::query("INSERT INTO sessions (account_id, token_hash, mfa, kind, created_at, expires_at) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(account_id)
        .bind(crypto::sha256_hex(&token))
        .bind(mfa as i64)
        .bind(kind)
        .bind(now())
        .bind(now() + lifetime)
        .execute(&state.db)
        .await?;
    Ok(token)
}

fn valid_username(name: &str) -> bool {
    (3..=20).contains(&name.len()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[derive(Deserialize)]
struct RegisterBody {
    username: String,
    password: String,
    email: Option<String>,
}

async fn register(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<RegisterBody>,
) -> ApiResult<Json<Value>> {
    if !state.auth_limit.allow(&format!("ip:{}", client_ip(&state, &headers, addr))) {
        return Err(ApiError::TooManyRequests);
    }
    let username = body.username.trim();
    if !valid_username(username) {
        return Err(ApiError::BadRequest("Usernames are 3 to 20 letters, numbers or underscores.".into()));
    }
    if body.password.len() < 8 || body.password.len() > 128 {
        return Err(ApiError::BadRequest("Passwords need 8 to 128 characters.".into()));
    }
    if accounts::by_username(&state.db, username).await?.is_some() {
        return Err(ApiError::Conflict("That username is taken.".into()));
    }

    // The very first account runs the server.
    let count: i64 = sqlx::query("SELECT COUNT(*) AS n FROM accounts").fetch_one(&state.db).await?.get("n");
    let role = if count == 0 { Role::Owner } else { Role::Player };
    let email = body.email.map(|e| e.trim().to_owned()).filter(|e| !e.is_empty());

    let id: i64 = sqlx::query("INSERT INTO accounts (username, email, password_hash, role, created_at) VALUES (?, ?, ?, ?, ?) RETURNING id")
        .bind(username)
        .bind(email)
        .bind(crypto::hash_password(&body.password)?)
        .bind(role.as_str())
        .bind(now())
        .fetch_one(&state.db)
        .await?
        .get("id");

    let token = create_session(&state, id, "web", false).await?;
    let account = accounts::by_id(&state.db, id).await?.ok_or(ApiError::NotFound)?;
    if role == Role::Owner {
        tracing::info!("first account '{username}' created as the server owner");
    }
    Ok(Json(json!({ "token": token, "account": account.public_json() })))
}

#[derive(Deserialize)]
struct LoginBody {
    username: String,
    password: String,
    /// Signing in to the admin panel: needs a staff role and 2FA.
    #[serde(default)]
    admin: bool,
}

async fn login(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<LoginBody>,
) -> ApiResult<Json<Value>> {
    let ip = client_ip(&state, &headers, addr);
    if !state.auth_limit.allow(&format!("ip:{ip}")) || !state.auth_limit.allow(&format!("user:{}", body.username.to_lowercase())) {
        return Err(ApiError::TooManyRequests);
    }

    let wrong = || ApiError::Unauthorized("Wrong username or password.".into());
    let account = accounts::by_username(&state.db, body.username.trim()).await?.ok_or_else(wrong)?;
    if !crypto::verify_password(&body.password, &account.password_hash) {
        return Err(wrong());
    }
    if account.is_banned() {
        return Err(ApiError::Banned(ban_message(&account)));
    }

    // During maintenance only staff can get in, and only to the admin panel.
    if !body.admin {
        if let Some(message) = crate::maintenance::down_message(&state) {
            return Err(ApiError::Maintenance(message));
        }
    }

    if body.admin {
        if !account.role.is_staff() {
            return Err(ApiError::Forbidden("Only staff can use the admin panel.".into()));
        }
        if !account.mfa_enabled {
            return Err(ApiError::Forbidden("Staff must turn on 2FA before using the admin panel. Set it up on the website first.".into()));
        }
    }

    if account.mfa_enabled {
        let ticket = crypto::new_token();
        let mut tickets = state.mfa_tickets.lock().unwrap();
        tickets.retain(|_, t| t.created.elapsed() < Duration::from_secs(300));
        tickets.insert(ticket.clone(), MfaTicket { account_id: account.id, admin: body.admin, created: Instant::now() });
        return Ok(Json(json!({ "mfa_required": true, "ticket": ticket })));
    }

    let token = create_session(&state, account.id, "web", false).await?;
    Ok(Json(json!({ "token": token, "account": account.public_json() })))
}

#[derive(Deserialize)]
struct Login2faBody {
    ticket: String,
    /// A 6-digit authenticator code, or one of the recovery codes.
    code: String,
}

async fn login_2fa(State(state): State<AppState>, Json(body): Json<Login2faBody>) -> ApiResult<Json<Value>> {
    let ticket = {
        let tickets = state.mfa_tickets.lock().unwrap();
        tickets.get(&body.ticket).copied()
    }
    .filter(|t| t.created.elapsed() < Duration::from_secs(300))
    .ok_or_else(|| ApiError::Unauthorized("That login took too long. Start again.".into()))?;

    if !state.auth_limit.allow(&format!("mfa:{}", ticket.account_id)) {
        return Err(ApiError::TooManyRequests);
    }
    let account = accounts::by_id(&state.db, ticket.account_id).await?.ok_or(ApiError::NotFound)?;
    if !mfa::check_code(&state, &account, &body.code).await? {
        return Err(ApiError::Unauthorized("That code didn't work.".into()));
    }
    state.mfa_tickets.lock().unwrap().remove(&body.ticket);

    let kind = if ticket.admin { "admin" } else { "web" };
    let token = create_session(&state, account.id, kind, true).await?;
    Ok(Json(json!({ "token": token, "account": account.public_json() })))
}

async fn logout(State(state): State<AppState>, auth: Authed) -> ApiResult<Json<Value>> {
    sqlx::query("DELETE FROM sessions WHERE id = ?").bind(auth.session_id).execute(&state.db).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn me(auth: Authed) -> Json<Value> {
    let mut account = auth.account.public_json();
    account["session"] = json!({ "kind": auth.kind, "mfa": auth.mfa });
    Json(account)
}

async fn list_sessions(State(state): State<AppState>, auth: Authed) -> ApiResult<Json<Value>> {
    let rows = sqlx::query("SELECT id, kind, mfa, created_at, expires_at FROM sessions WHERE account_id = ? AND expires_at > ? ORDER BY id DESC")
        .bind(auth.account.id)
        .bind(now())
        .fetch_all(&state.db)
        .await?;
    let sessions: Vec<Value> = rows
        .iter()
        .map(|r| {
            let id: i64 = r.get("id");
            json!({
                "id": id,
                "kind": r.get::<String, _>("kind"),
                "mfa": r.get::<i64, _>("mfa") != 0,
                "created_at": r.get::<i64, _>("created_at"),
                "expires_at": r.get::<i64, _>("expires_at"),
                "current": id == auth.session_id,
            })
        })
        .collect();
    Ok(Json(json!({ "sessions": sessions })))
}

async fn revoke_session(State(state): State<AppState>, auth: Authed, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let done = sqlx::query("DELETE FROM sessions WHERE id = ? AND account_id = ?").bind(id).bind(auth.account.id).execute(&state.db).await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(Json(json!({ "ok": true })))
}
