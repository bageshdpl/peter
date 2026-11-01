-- M2: connecting the Observer extension, and the steps it records.

-- Which kind of client a sign-in session belongs to.
ALTER TABLE sessions ADD COLUMN client TEXT NOT NULL DEFAULT 'web';

-- One-time codes the web app hands to the extension so it can sign in
-- without asking for the password again.
CREATE TABLE extension_codes (
    id           TEXT PRIMARY KEY,
    code_hash    TEXT NOT NULL UNIQUE,
    user_id      TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    created_at   INTEGER NOT NULL,
    expires_at   INTEGER NOT NULL,
    used_at      INTEGER
);

-- A step observed in the system under validation.
CREATE TABLE observations (
    id                TEXT PRIMARY KEY,
    workspace_id      TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    session_id        TEXT NOT NULL REFERENCES validation_sessions(id) ON DELETE CASCADE,
    -- Order within the session, starting at 1.
    seq               INTEGER NOT NULL,
    kind              TEXT NOT NULL CHECK (kind IN ('navigate', 'click', 'input', 'select', 'submit', 'note', 'capture')),
    url               TEXT NOT NULL DEFAULT '',
    page_title        TEXT NOT NULL DEFAULT '',
    target_label      TEXT NOT NULL DEFAULT '',
    target_selector   TEXT NOT NULL DEFAULT '',
    -- What was typed or chosen. Sensitive fields arrive already masked.
    value             TEXT NOT NULL DEFAULT '',
    masked            INTEGER NOT NULL DEFAULT 0,
    note              TEXT NOT NULL DEFAULT '',
    screenshot_type   TEXT,
    screenshot_bytes  INTEGER,
    screenshot_sha256 TEXT,
    -- When it happened in the browser (Unix milliseconds), and when the
    -- server stored it (Unix seconds, like every other table).
    captured_at_ms    INTEGER NOT NULL,
    created_by        TEXT NOT NULL REFERENCES users(id),
    created_at        INTEGER NOT NULL,
    deleted_at        INTEGER,
    deleted_by        TEXT REFERENCES users(id),
    UNIQUE (session_id, seq)
);
CREATE INDEX observations_session ON observations(session_id, seq);
