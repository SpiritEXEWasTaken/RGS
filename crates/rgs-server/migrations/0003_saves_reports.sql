-- Cloud saves, player reports, and whether a staff member shows their badge in games.

ALTER TABLE accounts ADD COLUMN show_badge INTEGER NOT NULL DEFAULT 1;

CREATE TABLE saves (
    account_id  INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    game        TEXT    NOT NULL,
    slot        TEXT    NOT NULL,
    -- Whatever the game stores (usually JSON); RGS doesn't look inside.
    data        TEXT    NOT NULL,
    updated_at  INTEGER NOT NULL,
    PRIMARY KEY (account_id, game, slot)
);

CREATE TABLE reports (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    reporter_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- The reported account, when they were signed in; otherwise just the name seen in game.
    target_id   INTEGER REFERENCES accounts(id) ON DELETE SET NULL,
    target_name TEXT    NOT NULL,
    game        TEXT    NOT NULL,
    reason      TEXT    NOT NULL,
    details     TEXT    NOT NULL DEFAULT '',
    -- Where it happened, e.g. "lobby code ABCD", filled in by the game.
    context     TEXT    NOT NULL DEFAULT '',
    -- open, resolved or dismissed
    status      TEXT    NOT NULL DEFAULT 'open',
    created_at  INTEGER NOT NULL,
    handled_by  INTEGER REFERENCES accounts(id) ON DELETE SET NULL,
    handled_at  INTEGER,
    note        TEXT
);
CREATE INDEX reports_status ON reports(status, id);
