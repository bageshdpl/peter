-- A session can build on an earlier validation of the same system (for
-- example a change validation on top of the initial validation). Drafts then
-- cite that session's documents as [B:step].
ALTER TABLE validation_sessions ADD COLUMN baseline_session_id TEXT REFERENCES validation_sessions(id) ON DELETE SET NULL;
