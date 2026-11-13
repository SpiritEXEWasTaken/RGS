-- Small server-wide settings that must survive a restart, stored as JSON.
-- "maintenance": the scheduled maintenance window, if any.
CREATE TABLE settings (
    key     TEXT PRIMARY KEY,
    value   TEXT NOT NULL
);
