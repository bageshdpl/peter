-- M0 Foundation: users, workspaces, memberships, invites, sessions,
-- password resets and the audit trail. Times are Unix seconds (UTC).

CREATE TABLE users (
    id            TEXT PRIMARY KEY,
    email         TEXT NOT NULL UNIQUE,
    name          TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    created_at    INTEGER NOT NULL
);

CREATE TABLE workspaces (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE memberships (
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    user_id      TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role         TEXT NOT NULL CHECK (role IN ('owner', 'admin', 'member', 'viewer')),
    created_at   INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, user_id)
);
CREATE INDEX memberships_user ON memberships(user_id);

CREATE TABLE invites (
    id           TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    email        TEXT NOT NULL,
    role         TEXT NOT NULL CHECK (role IN ('owner', 'admin', 'member', 'viewer')),
    token_hash   TEXT NOT NULL UNIQUE,
    invited_by   TEXT NOT NULL REFERENCES users(id),
    created_at   INTEGER NOT NULL,
    expires_at   INTEGER NOT NULL,
    accepted_at  INTEGER,
    revoked_at   INTEGER
);
CREATE INDEX invites_workspace ON invites(workspace_id);

-- One row per signed-in device and workspace. Both tokens are random and only
-- their SHA-256 hashes are stored. Refreshing rotates both.
CREATE TABLE sessions (
    id                 TEXT PRIMARY KEY,
    user_id            TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    workspace_id       TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    access_hash        TEXT NOT NULL UNIQUE,
    access_expires_at  INTEGER NOT NULL,
    refresh_hash       TEXT NOT NULL UNIQUE,
    refresh_expires_at INTEGER NOT NULL,
    created_at         INTEGER NOT NULL,
    revoked_at         INTEGER
);
CREATE INDEX sessions_user ON sessions(user_id);

CREATE TABLE password_resets (
    id         TEXT PRIMARY KEY,
    user_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    used_at    INTEGER
);

-- Append-only. The API has no way to change or delete an event.
CREATE TABLE audit_events (
    seq           INTEGER PRIMARY KEY AUTOINCREMENT,
    id            TEXT NOT NULL UNIQUE,
    workspace_id  TEXT,
    actor_user_id TEXT,
    action        TEXT NOT NULL,
    target        TEXT,
    detail        TEXT NOT NULL DEFAULT '{}',
    at            INTEGER NOT NULL
);
CREATE INDEX audit_workspace ON audit_events(workspace_id, seq);

CREATE TRIGGER audit_events_no_update BEFORE UPDATE ON audit_events
BEGIN SELECT RAISE(ABORT, 'audit events are append-only'); END;
CREATE TRIGGER audit_events_no_delete BEFORE DELETE ON audit_events
BEGIN SELECT RAISE(ABORT, 'audit events are append-only'); END;
