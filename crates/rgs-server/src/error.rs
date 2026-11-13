//! API errors, sent to clients as `{ "error": "<code>", "message": "<text to show the player>" }`.
//! Games should show `message` as is, and can switch on `error` for special handling.
//! The full list of codes is in docs/ERRORS.md.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug)]
pub enum ApiError {
    BadRequest(String),
    Unauthorized(String),
    Forbidden(String),
    Banned(String),
    NotFound,
    Conflict(String),
    TooManyRequests,
    /// The game client is too old: "Update Lanternfall ...".
    UpdateRequired(String),
    /// The server is down for maintenance or rebooting.
    Maintenance(String),
    Internal(anyhow::Error),
}

pub type ApiResult<T> = Result<T, ApiError>;

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            ApiError::BadRequest(m) => (StatusCode::BAD_REQUEST, "bad_request", m),
            ApiError::Unauthorized(m) => (StatusCode::UNAUTHORIZED, "unauthorized", m),
            ApiError::Forbidden(m) => (StatusCode::FORBIDDEN, "forbidden", m),
            ApiError::Banned(m) => (StatusCode::FORBIDDEN, "banned", m),
            ApiError::NotFound => (StatusCode::NOT_FOUND, "not_found", "Not found.".into()),
            ApiError::Conflict(m) => (StatusCode::CONFLICT, "conflict", m),
            ApiError::TooManyRequests => (StatusCode::TOO_MANY_REQUESTS, "rate_limited", "Too many tries. Wait a minute.".into()),
            ApiError::UpdateRequired(m) => (StatusCode::UPGRADE_REQUIRED, "update_required", m),
            ApiError::Maintenance(m) => (StatusCode::SERVICE_UNAVAILABLE, "maintenance", m),
            ApiError::Internal(e) => {
                tracing::error!("internal error: {e:#}");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal", "Something went wrong on the server.".into())
            }
        };
        (status, Json(json!({ "error": code, "message": message }))).into_response()
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        ApiError::Internal(e.into())
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError::Internal(e)
    }
}
