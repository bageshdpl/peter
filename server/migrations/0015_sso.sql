-- Single sign-on with OpenID Connect, set up per workspace. A workspace's
-- identity provider is trusted for that workspace only: a session started
-- through it cannot switch to another workspace.

CREATE TABLE workspace_sso (
    workspace_id  TEXT PRIMARY KEY REFERENCES workspaces(id) ON DELETE CASCADE,
    issuer        TEXT NOT NULL,
    client_id     TEXT NOT NULL,
    -- Encrypted with the server's secret key.
    client_secret TEXT NOT NULL,
    enabled       INTEGER NOT NULL DEFAULT 0,
    -- Password sign-in to this workspace is refused, except for owners.
    enforce       INTEGER NOT NULL DEFAULT 0,
    -- A first sign-in creates the account and membership.
    auto_join     INTEGER NOT NULL DEFAULT 1,
    default_role  TEXT NOT NULL DEFAULT 'member' CHECK (default_role IN ('member', 'viewer')),
    -- Email domains the provider may sign in, comma separated; empty allows any.
    domains       TEXT NOT NULL DEFAULT '',
    updated_by    TEXT NOT NULL REFERENCES users(id),
    updated_at    INTEGER NOT NULL
);

-- One sign-in (or signature confirmation) in progress at the provider.
CREATE TABLE sso_logins (
    id            TEXT PRIMARY KEY,
    state_hash    TEXT NOT NULL UNIQUE,
    workspace_id  TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    nonce         TEXT NOT NULL,
    code_verifier TEXT NOT NULL,
    purpose       TEXT NOT NULL CHECK (purpose IN ('login', 'sign')),
    -- For 'sign': the person who must be the one coming back.
    user_id       TEXT REFERENCES users(id) ON DELETE CASCADE,
    created_at    INTEGER NOT NULL,
    expires_at    INTEGER NOT NULL,
    used_at       INTEGER
);

-- Which Peter account a provider's subject is, per workspace.
CREATE TABLE sso_identities (
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    subject      TEXT NOT NULL,
    user_id      TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at   INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, subject)
);

-- Proof of a fresh sign-in at the provider, spent on one signature.
CREATE TABLE sso_sign_tickets (
    token_hash   TEXT PRIMARY KEY,
    user_id      TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    expires_at   INTEGER NOT NULL,
    used_at      INTEGER
);

ALTER TABLE sessions ADD COLUMN sso INTEGER NOT NULL DEFAULT 0;
ALTER TABLE extension_codes ADD COLUMN sso INTEGER NOT NULL DEFAULT 0;
