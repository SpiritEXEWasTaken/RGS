-- Sudo mode: an admin panel session that confirmed a 2FA code for big actions (bans, roles,
-- maintenance). It ends with the session; the panel keeps sessions only while its tab is open.
ALTER TABLE sessions ADD COLUMN sudo INTEGER NOT NULL DEFAULT 0;
