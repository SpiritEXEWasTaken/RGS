//! Shared server state handed to every request.

use crate::config::Config;
use crate::crypto::SecretBox;
use crate::ratelimit::RateLimiter;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub type AppState = Arc<State>;

pub struct State {
    pub config: Config,
    pub db: SqlitePool,
    pub secrets: SecretBox,
    /// Sign-up and login tries per IP and per username.
    pub auth_limit: RateLimiter,
    /// Half-finished logins waiting for a 2FA code: ticket -> (account, admin login?, created).
    pub mfa_tickets: Mutex<HashMap<String, MfaTicket>>,
    /// Scheduled or running maintenance (also saved in the database).
    pub maintenance: Mutex<Option<crate::maintenance::Maintenance>>,
    /// Fired to stop the server for a scheduled restart.
    pub shutdown: tokio::sync::Notify,
}

#[derive(Clone, Copy)]
pub struct MfaTicket {
    pub account_id: i64,
    pub admin: bool,
    pub created: Instant,
}

impl State {
    pub fn new(config: Config, db: SqlitePool, secrets: SecretBox, maintenance: Option<crate::maintenance::Maintenance>) -> Self {
        Self {
            config,
            db,
            secrets,
            auth_limit: RateLimiter::new(10, Duration::from_secs(60)),
            mfa_tickets: Mutex::new(HashMap::new()),
            maintenance: Mutex::new(maintenance),
            shutdown: tokio::sync::Notify::new(),
        }
    }
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
