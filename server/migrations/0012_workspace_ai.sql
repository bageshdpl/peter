-- A workspace's own AI keys, and a record of every AI call.

-- The key is encrypted (see src/secrets.rs) and never sent back; the last
-- four characters are kept to tell keys apart.
CREATE TABLE workspace_ai_keys (
    workspace_id  TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    provider      TEXT NOT NULL CHECK (provider IN ('openrouter', 'gemini')),
    key_encrypted TEXT NOT NULL,
    key_hint      TEXT NOT NULL,
    -- Empty: the server's default model.
    model         TEXT NOT NULL DEFAULT '',
    set_by        TEXT NOT NULL REFERENCES users(id),
    set_at        INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, provider)
);

CREATE TABLE ai_usage (
    id                TEXT PRIMARY KEY,
    workspace_id      TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    user_id           TEXT NOT NULL REFERENCES users(id),
    session_id        TEXT REFERENCES validation_sessions(id) ON DELETE SET NULL,
    step_id           TEXT,
    provider          TEXT NOT NULL,
    model             TEXT NOT NULL DEFAULT '',
    -- Whose key paid: the workspace's own, or the server's.
    key_source        TEXT NOT NULL CHECK (key_source IN ('workspace', 'server')),
    prompt_tokens     INTEGER,
    completion_tokens INTEGER,
    cost_usd          REAL,
    ok                INTEGER NOT NULL,
    error             TEXT,
    created_at        INTEGER NOT NULL
);
CREATE INDEX ai_usage_workspace ON ai_usage(workspace_id, created_at);
