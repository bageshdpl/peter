-- The Library: regulations in scope, company templates, and branding.

-- Which regulations from Peter's catalog apply to a workspace.
CREATE TABLE workspace_standards (
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    standard_id  TEXT NOT NULL,
    PRIMARY KEY (workspace_id, standard_id)
);

-- A company's own template for a step's document. Its text guides drafting.
CREATE TABLE library_templates (
    id           TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    step_id      TEXT NOT NULL,
    name         TEXT NOT NULL,
    filename     TEXT NOT NULL,
    text         TEXT NOT NULL,
    sha256       TEXT NOT NULL,
    created_by   TEXT NOT NULL REFERENCES users(id),
    created_at   INTEGER NOT NULL,
    -- Replaced or removed templates are kept, hidden, as a record.
    archived_at  INTEGER
);
CREATE INDEX library_templates_workspace ON library_templates(workspace_id, step_id, archived_at);

-- How exported documents present the company.
CREATE TABLE workspace_branding (
    workspace_id TEXT PRIMARY KEY REFERENCES workspaces(id) ON DELETE CASCADE,
    company_name TEXT NOT NULL DEFAULT '',
    doc_prefix   TEXT NOT NULL DEFAULT '',
    footer_text  TEXT NOT NULL DEFAULT '',
    logo_type    TEXT,
    logo_sha256  TEXT,
    updated_by   TEXT REFERENCES users(id),
    updated_at   INTEGER NOT NULL
);
