//! Two-factor login with an authenticator app (TOTP). Optional and recommended for players,
//! required for staff in the admin panel. Secrets are stored encrypted; recovery codes hashed.

use crate::accounts::{self, Account};
use crate::auth::Authed;
use crate::crypto;
use crate::error::{ApiError, ApiResult};
use crate::state::{now, AppState};
use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use totp_rs::{Algorithm, Secret, TOTP};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/2fa/setup", post(setup))
        .route("/auth/2fa/confirm", post(confirm))
        .route("/auth/2fa/disable", post(disable))
}

fn totp(state: &AppState, account: &Account, secret: Vec<u8>) -> ApiResult<TOTP> {
    TOTP::new(Algorithm::SHA1, 6, 1, 30, secret, Some(state.config.issuer.clone()), account.username.clone())
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("totp: {e:?}")))
}

/// True if `code` is the current authenticator code, or an unused recovery code (which is
/// then used up).
pub async fn check_code(state: &AppState, account: &Account, code: &str) -> ApiResult<bool> {
    let code = code.trim().replace([' ', '-'], "");
    if let Some(sealed) = &account.totp_secret {
        let secret = state.secrets.open(sealed)?;
        if code.len() == 6 && totp(state, account, secret)?.check_current(&code).unwrap_or(false) {
            return Ok(true);
        }
    }
    let used = sqlx::query("UPDATE recovery_codes SET used_at = ? WHERE account_id = ? AND code_hash = ? AND used_at IS NULL")
        .bind(now())
        .bind(account.id)
        .bind(crypto::sha256_hex(&code.to_uppercase()))
        .execute(&state.db)
        .await?;
    Ok(used.rows_affected() > 0)
}

/// Step 1: make a new secret. The app shows it as a QR code (from `otpauth_url`) or the
/// text key; nothing changes until the first code is confirmed.
async fn setup(State(state): State<AppState>, auth: Authed) -> ApiResult<Json<Value>> {
    if auth.account.mfa_enabled {
        return Err(ApiError::Conflict("2FA is already on. Turn it off first to switch apps.".into()));
    }
    let secret = Secret::generate_secret().to_bytes().map_err(|e| ApiError::Internal(anyhow::anyhow!("secret: {e:?}")))?;
    let t = totp(&state, &auth.account, secret.clone())?;
    sqlx::query("UPDATE accounts SET totp_pending = ? WHERE id = ?")
        .bind(state.secrets.seal(&secret)?)
        .bind(auth.account.id)
        .execute(&state.db)
        .await?;
    Ok(Json(json!({ "secret": t.get_secret_base32(), "otpauth_url": t.get_url() })))
}

#[derive(Deserialize)]
struct CodeBody {
    code: String,
}

/// Step 2: the first code from the app turns 2FA on and returns ten one-time recovery codes.
async fn confirm(State(state): State<AppState>, auth: Authed, Json(body): Json<CodeBody>) -> ApiResult<Json<Value>> {
    let pending = auth.account.totp_pending.clone().ok_or_else(|| ApiError::BadRequest("Start 2FA setup first.".into()))?;
    let secret = state.secrets.open(&pending)?;
    let code = body.code.trim().replace(' ', "");
    if !totp(&state, &auth.account, secret)?.check_current(&code).unwrap_or(false) {
        return Err(ApiError::Unauthorized("That code didn't work. Check your phone's clock and try the newest code.".into()));
    }

    let mut tx = state.db.begin().await?;
    sqlx::query("UPDATE accounts SET totp_secret = totp_pending, totp_pending = NULL WHERE id = ?").bind(auth.account.id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM recovery_codes WHERE account_id = ?").bind(auth.account.id).execute(&mut *tx).await?;
    let mut codes = Vec::new();
    for _ in 0..10 {
        let code = format!("{}-{}", crypto::human_code(5), crypto::human_code(5));
        sqlx::query("INSERT INTO recovery_codes (account_id, code_hash) VALUES (?, ?)")
            .bind(auth.account.id)
            .bind(crypto::sha256_hex(&code.replace('-', "")))
            .execute(&mut *tx)
            .await?;
        codes.push(code);
    }
    // This login now counts as signed in with 2FA.
    sqlx::query("UPDATE sessions SET mfa = 1 WHERE id = ?").bind(auth.session_id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true, "recovery_codes": codes })))
}

/// Turns 2FA off (needs a current code). Staff lose their admin panel logins.
async fn disable(State(state): State<AppState>, auth: Authed, Json(body): Json<CodeBody>) -> ApiResult<Json<Value>> {
    if !auth.account.mfa_enabled {
        return Err(ApiError::BadRequest("2FA isn't on.".into()));
    }
    if !check_code(&state, &auth.account, &body.code).await? {
        return Err(ApiError::Unauthorized("That code didn't work.".into()));
    }
    sqlx::query("UPDATE accounts SET totp_secret = NULL, totp_pending = NULL WHERE id = ?").bind(auth.account.id).execute(&state.db).await?;
    sqlx::query("DELETE FROM recovery_codes WHERE account_id = ?").bind(auth.account.id).execute(&state.db).await?;
    sqlx::query("DELETE FROM sessions WHERE account_id = ? AND kind = 'admin'").bind(auth.account.id).execute(&state.db).await?;
    if auth.account.role.is_staff() {
        accounts::audit(&state.db, auth.account.id, "2fa_disabled", Some(auth.account.id), "").await?;
    }
    let remaining: i64 = sqlx::query("SELECT COUNT(*) AS n FROM sessions WHERE account_id = ?").bind(auth.account.id).fetch_one(&state.db).await?.get("n");
    Ok(Json(json!({ "ok": true, "sessions_left": remaining })))
}
