CREATE TABLE jobs (
 id TEXT PRIMARY KEY, provider_identity TEXT NOT NULL, root_id TEXT NOT NULL,
 relative_path TEXT NOT NULL, reservation TEXT NOT NULL, state TEXT NOT NULL
 CHECK(state IN ('queued','resolving','downloading','verifying','paused','completed','failed','cancelled')),
 generation INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
 retry_at INTEGER, finalize_phase TEXT, document TEXT NOT NULL
);
CREATE UNIQUE INDEX jobs_active_identity ON jobs(provider_identity)
 WHERE state NOT IN ('completed','failed','cancelled') OR finalize_phase IS NOT NULL AND state != 'completed';
CREATE UNIQUE INDEX jobs_active_destination ON jobs(reservation)
 WHERE state NOT IN ('completed','failed','cancelled') OR finalize_phase IS NOT NULL AND state != 'completed';
CREATE INDEX jobs_history ON jobs(created_at DESC,id);
CREATE TABLE models (
 root_id TEXT NOT NULL, path TEXT NOT NULL, category TEXT NOT NULL, size INTEGER NOT NULL,
 mtime TEXT NOT NULL, file_identity TEXT NOT NULL, sha256 TEXT, source_job_id TEXT,
 civitai_version_id INTEGER, meta_json TEXT, PRIMARY KEY(root_id,path)
);
CREATE INDEX models_sha256 ON models(sha256);
CREATE TABLE credential_tests (
 kind TEXT PRIMARY KEY, generation INTEGER NOT NULL, tested_at INTEGER NOT NULL, result TEXT NOT NULL
);
