-- The company's writing style: sample documents, and a short style guide
-- (learnt from them by AI, or written by a person) that every draft follows.

-- Only the text of a sample is kept, not the file.
CREATE TABLE style_samples (
    id           TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    filename     TEXT NOT NULL,
    sha256       TEXT NOT NULL,
    text         TEXT NOT NULL,
    created_by   TEXT NOT NULL REFERENCES users(id),
    created_at   INTEGER NOT NULL,
    archived_at  INTEGER
);
CREATE INDEX style_samples_workspace ON style_samples(workspace_id, archived_at);

CREATE TABLE workspace_style (
    workspace_id TEXT PRIMARY KEY REFERENCES workspaces(id) ON DELETE CASCADE,
    guide        TEXT NOT NULL,
    -- 'ai' (learnt from samples) or 'person' (written or edited by hand).
    source       TEXT NOT NULL CHECK (source IN ('ai', 'person')),
    model        TEXT,
    updated_by   TEXT NOT NULL REFERENCES users(id),
    updated_at   INTEGER NOT NULL
);
