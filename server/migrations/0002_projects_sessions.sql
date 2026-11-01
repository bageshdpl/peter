-- M1: projects (the system being validated), sessions (validation runs) and
-- project sources (reference files).

CREATE TABLE projects (
    id           TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    system_name  TEXT NOT NULL DEFAULT '',
    vendor       TEXT NOT NULL DEFAULT '',
    description  TEXT NOT NULL DEFAULT '',
    created_by   TEXT NOT NULL REFERENCES users(id),
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL,
    archived_at  INTEGER
);
CREATE INDEX projects_workspace ON projects(workspace_id, archived_at);

CREATE TABLE validation_sessions (
    id              TEXT PRIMARY KEY,
    workspace_id    TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    -- NULL while the session is unfiled.
    project_id      TEXT REFERENCES projects(id) ON DELETE SET NULL,
    title           TEXT NOT NULL,
    validation_type TEXT NOT NULL,
    current_step    TEXT,
    created_by      TEXT NOT NULL REFERENCES users(id),
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL,
    archived_at     INTEGER
);
CREATE INDEX validation_sessions_workspace ON validation_sessions(workspace_id, project_id, archived_at);

CREATE TABLE project_sources (
    id           TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    project_id   TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    filename     TEXT NOT NULL,
    content_type TEXT NOT NULL,
    size_bytes   INTEGER NOT NULL,
    sha256       TEXT NOT NULL,
    uploaded_by  TEXT NOT NULL REFERENCES users(id),
    created_at   INTEGER NOT NULL,
    deleted_at   INTEGER
);
CREATE INDEX project_sources_project ON project_sources(project_id, deleted_at);
