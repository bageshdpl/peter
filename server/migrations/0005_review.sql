-- M5: comments on documents, and signatures on document versions.

CREATE TABLE document_comments (
    id           TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    document_id  TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    -- Replies point at the comment that opened the thread.
    parent_id    TEXT REFERENCES document_comments(id) ON DELETE CASCADE,
    -- The version on screen when the comment was written.
    version      INTEGER NOT NULL,
    -- Text the comment is about, copied from the document (optional).
    quote        TEXT NOT NULL DEFAULT '',
    body         TEXT NOT NULL,
    author_id    TEXT NOT NULL REFERENCES users(id),
    created_at   INTEGER NOT NULL,
    resolved_at  INTEGER,
    resolved_by  TEXT REFERENCES users(id)
);
CREATE INDEX document_comments_document ON document_comments(document_id, created_at);

-- A person's signature on one version: their name, the meaning and the time
-- are recorded, after they re-entered their password. Signatures are never
-- changed or removed.
CREATE TABLE document_signatures (
    id          TEXT PRIMARY KEY,
    document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    version     INTEGER NOT NULL,
    user_id     TEXT NOT NULL REFERENCES users(id),
    -- The name and email at signing time, as shown to the signer.
    signer_name  TEXT NOT NULL,
    signer_email TEXT NOT NULL,
    meaning     TEXT NOT NULL CHECK (meaning IN ('reviewed', 'approved')),
    signed_at   INTEGER NOT NULL,
    UNIQUE (document_id, version, user_id, meaning)
);

CREATE TRIGGER document_signatures_no_update BEFORE UPDATE ON document_signatures
BEGIN SELECT RAISE(ABORT, 'signatures cannot be changed'); END;
CREATE TRIGGER document_signatures_no_delete BEFORE DELETE ON document_signatures
BEGIN SELECT RAISE(ABORT, 'signatures cannot be removed'); END;
