-- Model library (M4): sidecar flags, scan roots, hash cache, operation journal and a catalog
-- revision that every catalog mutation bumps (triggers), so a stale scan cannot overwrite a
-- newer download, rename or delete.
ALTER TABLE models ADD COLUMN sidecars INTEGER NOT NULL DEFAULT 0;
CREATE INDEX models_category ON models(category);

CREATE TABLE library_state (
 key TEXT PRIMARY KEY, value INTEGER NOT NULL
);
INSERT INTO library_state(key, value) VALUES ('catalog_revision', 0);
CREATE TRIGGER models_revision_insert AFTER INSERT ON models
BEGIN UPDATE library_state SET value = value + 1 WHERE key = 'catalog_revision'; END;
CREATE TRIGGER models_revision_update AFTER UPDATE ON models
BEGIN UPDATE library_state SET value = value + 1 WHERE key = 'catalog_revision'; END;
CREATE TRIGGER models_revision_delete AFTER DELETE ON models
BEGIN UPDATE library_state SET value = value + 1 WHERE key = 'catalog_revision'; END;

CREATE TABLE model_roots (
 id TEXT PRIMARY KEY, path TEXT NOT NULL, category TEXT, status TEXT NOT NULL,
 scanned_at TEXT
);

-- Keyed by location plus size and nanosecond mtime; a changed file never reuses a digest.
CREATE TABLE model_hash_cache (
 root_id TEXT NOT NULL, path TEXT NOT NULL, size INTEGER NOT NULL, mtime_ns TEXT NOT NULL,
 file_identity TEXT NOT NULL, sha256 TEXT NOT NULL, PRIMARY KEY(root_id, path)
);

CREATE TABLE model_operations (
 id TEXT PRIMARY KEY, kind TEXT NOT NULL CHECK(kind IN ('rename','move')),
 phase TEXT NOT NULL CHECK(phase IN ('intent','recovery_required')),
 document TEXT NOT NULL, created_at TEXT NOT NULL
);
