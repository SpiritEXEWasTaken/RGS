//! Accounts and roles.

use crate::state::now;
use serde::Serialize;
use sqlx::{Row, SqlitePool};

/// Each role can do everything the ones before it can.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Player,
    Moderator,
    Admin,
    Owner,
}

impl Role {
    pub fn parse(s: &str) -> Option<Role> {
        match s {
            "player" => Some(Role::Player),
            "moderator" => Some(Role::Moderator),
            "admin" => Some(Role::Admin),
            "owner" => Some(Role::Owner),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Player => "player",
            Role::Moderator => "moderator",
            Role::Admin => "admin",
            Role::Owner => "owner",
        }
    }

    /// Moderators and up use the admin panel, which needs 2FA.
    pub fn is_staff(self) -> bool {
        self >= Role::Moderator
    }
}

/// A ban that never ends.
pub const PERMANENT: i64 = i64::MAX;

#[derive(Debug, Clone, Serialize)]
pub struct Account {
    pub id: i64,
    pub username: String,
    pub email: Option<String>,
    pub role: Role,
    pub mfa_enabled: bool,
    pub banned_until: Option<i64>,
    pub ban_reason: Option<String>,
    pub created_at: i64,
    pub last_seen_at: Option<i64>,
    /// Staff can hide their badge in games.
    pub show_badge: bool,
    #[serde(skip)]
    pub password_hash: String,
    #[serde(skip)]
    pub totp_secret: Option<String>,
    #[serde(skip)]
    pub totp_pending: Option<String>,
}

impl Account {
    pub fn is_banned(&self) -> bool {
        self.banned_until.map_or(false, |until| until > now())
    }

    /// The badge games show under this player's name (staff only, unless they hid it).
    pub fn badge(&self) -> Option<&'static str> {
        (self.role.is_staff() && self.show_badge).then(|| match self.role {
            Role::Owner => "Owner",
            Role::Admin => "Admin",
            _ => "Moderator",
        })
    }

    /// What players see about their own account.
    pub fn public_json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "username": self.username,
            "email": self.email,
            "role": self.role,
            "mfa_enabled": self.mfa_enabled,
            // Shown as a gentle nudge on the website until 2FA is on.
            "mfa_recommended": !self.mfa_enabled,
            "show_badge": self.show_badge,
            "created_at": self.created_at,
        })
    }
}

const COLUMNS: &str = "id, username, email, password_hash, totp_secret, totp_pending, role, banned_until, ban_reason, created_at, last_seen_at, show_badge";

fn from_row(row: &sqlx::sqlite::SqliteRow) -> Account {
    let totp_secret: Option<String> = row.get("totp_secret");
    Account {
        id: row.get("id"),
        username: row.get("username"),
        email: row.get("email"),
        password_hash: row.get("password_hash"),
        mfa_enabled: totp_secret.is_some(),
        totp_secret,
        totp_pending: row.get("totp_pending"),
        role: Role::parse(row.get::<String, _>("role").as_str()).unwrap_or(Role::Player),
        banned_until: row.get("banned_until"),
        ban_reason: row.get("ban_reason"),
        created_at: row.get("created_at"),
        last_seen_at: row.get("last_seen_at"),
        show_badge: row.get::<i64, _>("show_badge") != 0,
    }
}

pub async fn by_id(db: &SqlitePool, id: i64) -> sqlx::Result<Option<Account>> {
    let row = sqlx::query(&format!("SELECT {COLUMNS} FROM accounts WHERE id = ?")).bind(id).fetch_optional(db).await?;
    Ok(row.as_ref().map(from_row))
}

pub async fn by_username(db: &SqlitePool, username: &str) -> sqlx::Result<Option<Account>> {
    let row = sqlx::query(&format!("SELECT {COLUMNS} FROM accounts WHERE username = ?")).bind(username).fetch_optional(db).await?;
    Ok(row.as_ref().map(from_row))
}

pub async fn search(db: &SqlitePool, query: &str, limit: i64) -> sqlx::Result<Vec<Account>> {
    let rows = sqlx::query(&format!("SELECT {COLUMNS} FROM accounts WHERE username LIKE ? ORDER BY id DESC LIMIT ?"))
        .bind(format!("%{query}%"))
        .bind(limit)
        .fetch_all(db)
        .await?;
    Ok(rows.iter().map(from_row).collect())
}

/// Records something a staff member did in the admin panel.
pub async fn audit(db: &SqlitePool, actor: i64, action: &str, target: Option<i64>, details: &str) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO audit_log (actor_id, action, target_id, details, at) VALUES (?, ?, ?, ?, ?)")
        .bind(actor)
        .bind(action)
        .bind(target)
        .bind(details)
        .bind(now())
        .execute(db)
        .await?;
    Ok(())
}
