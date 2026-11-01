-- M8 Sharing and notifications.

-- Pointing a colleague at a session or one of its documents, optionally
-- asking them to review and sign it. Everyone in a workspace can already
-- open every session; a share says "please look at this".
CREATE TABLE shares (
    id           TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    session_id   TEXT NOT NULL REFERENCES validation_sessions(id) ON DELETE CASCADE,
    -- NULL: the whole session.
    step_id      TEXT,
    user_id      TEXT NOT NULL REFERENCES users(id),
    shared_by    TEXT NOT NULL REFERENCES users(id),
    kind         TEXT NOT NULL CHECK (kind IN ('share', 'review')),
    message      TEXT NOT NULL DEFAULT '',
    created_at   INTEGER NOT NULL,
    -- Done: the review was signed, or the person marked it done.
    done_at      INTEGER,
    revoked_at   INTEGER
);
CREATE INDEX shares_user ON shares(workspace_id, user_id, done_at, revoked_at);
CREATE INDEX shares_session ON shares(session_id, revoked_at);

-- A message for one person in one workspace. The text is written when the
-- event happens, so it reads the same later.
CREATE TABLE notifications (
    id           TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    user_id      TEXT NOT NULL REFERENCES users(id),
    actor_id     TEXT REFERENCES users(id),
    kind         TEXT NOT NULL,
    session_id   TEXT REFERENCES validation_sessions(id) ON DELETE CASCADE,
    step_id      TEXT,
    text         TEXT NOT NULL,
    created_at   INTEGER NOT NULL,
    read_at      INTEGER
);
CREATE INDEX notifications_user ON notifications(workspace_id, user_id, read_at, created_at);
