-- Versions can now also be built by Peter itself from other documents (the
-- traceability matrix), not only drafted by AI or edited by a person.
-- SQLite cannot change a CHECK constraint in place, so the table is rebuilt
-- with every row kept.

DROP TRIGGER document_versions_no_update;

CREATE TABLE document_versions_new (
    id                TEXT PRIMARY KEY,
    document_id       TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    version           INTEGER NOT NULL,
    body              TEXT NOT NULL,
    -- 'ai', 'person', or 'generated' (built by Peter from other documents).
    origin            TEXT NOT NULL CHECK (origin IN ('ai', 'person', 'generated')),
    provider          TEXT,
    model             TEXT,
    prompt_tokens     INTEGER,
    completion_tokens INTEGER,
    citations         TEXT NOT NULL DEFAULT '{}',
    created_by        TEXT NOT NULL REFERENCES users(id),
    created_at        INTEGER NOT NULL,
    UNIQUE (document_id, version)
);

INSERT INTO document_versions_new
    (id, document_id, version, body, origin, provider, model, prompt_tokens, completion_tokens, citations, created_by, created_at)
SELECT id, document_id, version, body, origin, provider, model, prompt_tokens, completion_tokens, citations, created_by, created_at
FROM document_versions;

DROP TABLE document_versions;
ALTER TABLE document_versions_new RENAME TO document_versions;

CREATE TRIGGER document_versions_no_update BEFORE UPDATE ON document_versions
BEGIN SELECT RAISE(ABORT, 'document versions cannot be changed'); END;
