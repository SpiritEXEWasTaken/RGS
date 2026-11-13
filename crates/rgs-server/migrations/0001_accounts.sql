-- Accounts, login sessions, 2FA recovery codes, headset device codes and the admin audit log.
-- Times are Unix seconds.

CREATE TABLE accounts (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    username        TEXT    NOT NULL UNIQUE COLLATE NOCASE,
    email           TEXT,
    password_hash   TEXT    NOT NULL,
    -- 2FA secret, encrypted with the server key. NULL when 2FA is off.
    totp_secret     TEXT,
    -- A secret waiting for its first code during 2FA setup.
    totp_pending    TEXT,
    role            TEXT    NOT NULL DEFAULT 'player',
    banned_until    INTEGER,
    ban_reason      TEXT,
    created_at      INTEGER NOT NULL,
    last_seen_at    INTEGER
);

CREATE TABLE sessions (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id      INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- SHA-256 of the bearer token; the token itself is never stored.
    token_hash      TEXT    NOT NULL UNIQUE,
    -- Signed in with a 2FA code (needed for the admin panel).
    mfa             INTEGER NOT NULL DEFAULT 0,
    -- "web", "admin" or a device name such as "Quest 3".
    kind            TEXT    NOT NULL,
    created_at      INTEGER NOT NULL,
    expires_at      INTEGER NOT NULL
);
CREATE INDEX sessions_account ON sessions(account_id);

CREATE TABLE recovery_codes (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id      INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    code_hash       TEXT    NOT NULL,
    used_at         INTEGER
);
CREATE INDEX recovery_account ON recovery_codes(account_id);

CREATE TABLE device_codes (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    -- What the game polls with (secret).
    device_hash     TEXT    NOT NULL UNIQUE,
    -- What the player types on the website, e.g. KX7P2M.
    user_code       TEXT    NOT NULL UNIQUE,
    device_name     TEXT    NOT NULL,
    -- Set when a signed-in player approves the code.
    account_id      INTEGER REFERENCES accounts(id) ON DELETE CASCADE,
    created_at      INTEGER NOT NULL,
    expires_at      INTEGER NOT NULL
);

CREATE TABLE audit_log (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    actor_id        INTEGER REFERENCES accounts(id) ON DELETE SET NULL,
    action          TEXT    NOT NULL,
    target_id       INTEGER,
    details         TEXT,
    at              INTEGER NOT NULL
);
