-- M3: progress through a session's steps. M4: the documents drafted in them.

CREATE TABLE step_states (
    session_id TEXT NOT NULL REFERENCES validation_sessions(id) ON DELETE CASCADE,
    step_id    TEXT NOT NULL,
    status     TEXT NOT NULL CHECK (status IN ('not_started', 'in_progress', 'reviewed', 'skipped')),
    updated_by TEXT NOT NULL REFERENCES users(id),
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (session_id, step_id)
);

-- One document per step of a session.
CREATE TABLE documents (
    id              TEXT PRIMARY KEY,
    workspace_id    TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    session_id      TEXT NOT NULL REFERENCES validation_sessions(id) ON DELETE CASCADE,
    step_id         TEXT NOT NULL,
    title           TEXT NOT NULL,
    -- 'drafting' while the AI works; 'ready' once a version exists or the last
    -- draft finished; 'failed' if the last draft failed.
    status          TEXT NOT NULL CHECK (status IN ('drafting', 'ready', 'failed')),
    error           TEXT,
    current_version INTEGER NOT NULL DEFAULT 0,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL,
    UNIQUE (session_id, step_id)
);

-- Every version is kept. Versions are never changed after they are written.
CREATE TABLE document_versions (
    id                TEXT PRIMARY KEY,
    document_id       TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    version           INTEGER NOT NULL,
    body              TEXT NOT NULL,
    -- 'ai' or 'person'.
    origin            TEXT NOT NULL CHECK (origin IN ('ai', 'person')),
    provider          TEXT,
    model             TEXT,
    prompt_tokens     INTEGER,
    completion_tokens INTEGER,
    -- JSON: the evidence ids cited, ids cited that do not exist, and the
    -- number of gaps the AI flagged.
    citations         TEXT NOT NULL DEFAULT '{}',
    created_by        TEXT NOT NULL REFERENCES users(id),
    created_at        INTEGER NOT NULL,
    UNIQUE (document_id, version)
);

CREATE TRIGGER document_versions_no_update BEFORE UPDATE ON document_versions
BEGIN SELECT RAISE(ABORT, 'document versions cannot be changed'); END;
