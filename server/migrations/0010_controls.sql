-- M7 Controls: what a project's SOPs require, and how each session covers
-- those controls and the regulations' clauses.

-- A control from a project's SOPs, found in a source file or added by hand.
CREATE TABLE project_controls (
    id           TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    project_id   TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    -- CTL-001, CTL-002, ... within the project; numbers are never reused.
    number       INTEGER NOT NULL,
    text         TEXT NOT NULL,
    -- The SOP's section number the statement came from, if it had one.
    section      TEXT NOT NULL DEFAULT '',
    source_id    TEXT REFERENCES project_sources(id) ON DELETE SET NULL,
    created_by   TEXT NOT NULL REFERENCES users(id),
    created_at   INTEGER NOT NULL,
    -- Removed controls are kept, hidden, as a record.
    archived_at  INTEGER,
    UNIQUE (project_id, number)
);
CREATE INDEX project_controls_project ON project_controls(project_id, archived_at);

-- A session's decisions about one control: requirements linked by hand, or
-- "not applicable" with the reason.
CREATE TABLE session_controls (
    session_id     TEXT NOT NULL REFERENCES validation_sessions(id) ON DELETE CASCADE,
    control_key    TEXT NOT NULL,
    requirements   TEXT NOT NULL DEFAULT '[]',
    not_applicable INTEGER NOT NULL DEFAULT 0,
    reason         TEXT NOT NULL DEFAULT '',
    updated_by     TEXT NOT NULL REFERENCES users(id),
    updated_at     INTEGER NOT NULL,
    PRIMARY KEY (session_id, control_key)
);
