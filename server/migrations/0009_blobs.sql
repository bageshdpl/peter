-- Files (screenshots, source files, logos) live in the database rather than
-- in loose files, so one backup of the database file covers everything.
-- Older installs keep reading files left on disk under peter-data.
CREATE TABLE blobs (
    key        TEXT PRIMARY KEY,
    data       BLOB NOT NULL,
    created_at INTEGER NOT NULL
);
