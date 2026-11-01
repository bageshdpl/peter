-- Security hardening: sign-in limits, two-factor sign-in, session details.

-- Failed password checks per email (known or not), so guessing is slowed and
-- the lock itself does not reveal which emails have accounts.
CREATE TABLE login_throttle (
    key          TEXT PRIMARY KEY,
    failures     INTEGER NOT NULL DEFAULT 0,
    locked_until INTEGER,
    updated_at   INTEGER NOT NULL
);

-- One authenticator-app secret per person. enabled_at stays NULL until the
-- person proves the app works by entering a code.
CREATE TABLE user_mfa (
    user_id    TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    secret     TEXT NOT NULL,
    enabled_at INTEGER,
    -- The last 30-second time step a code was accepted for, so a code cannot be used twice.
    last_step  INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);

-- One-time recovery codes for when the phone is lost. Only hashes are stored.
CREATE TABLE mfa_recovery_codes (
    id        TEXT PRIMARY KEY,
    user_id   TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    code_hash TEXT NOT NULL UNIQUE,
    used_at   INTEGER
);
CREATE INDEX mfa_recovery_codes_user ON mfa_recovery_codes(user_id);

-- After a correct password, a short-lived ticket for the second step.
CREATE TABLE mfa_challenges (
    id           TEXT PRIMARY KEY,
    token_hash   TEXT NOT NULL UNIQUE,
    user_id      TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    workspace_id TEXT REFERENCES workspaces(id) ON DELETE CASCADE,
    created_at   INTEGER NOT NULL,
    expires_at   INTEGER NOT NULL,
    used_at      INTEGER
);

ALTER TABLE workspaces ADD COLUMN require_mfa INTEGER NOT NULL DEFAULT 0;

-- Which browser or app a session is, and when it was last refreshed.
ALTER TABLE sessions ADD COLUMN user_agent TEXT NOT NULL DEFAULT '';
ALTER TABLE sessions ADD COLUMN last_used_at INTEGER;
