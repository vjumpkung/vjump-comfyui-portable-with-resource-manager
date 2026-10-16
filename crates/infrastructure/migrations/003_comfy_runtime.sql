CREATE TABLE comfy_launch_options (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    version INTEGER NOT NULL CHECK (version = 1),
    json TEXT NOT NULL
);
CREATE TABLE comfy_runtime_sessions (
    id TEXT PRIMARY KEY,
    json TEXT NOT NULL,
    finished INTEGER NOT NULL DEFAULT 0 CHECK (finished IN (0, 1))
);
CREATE TABLE comfy_update_runs (
    id TEXT PRIMARY KEY,
    json TEXT NOT NULL,
    phase TEXT NOT NULL CHECK (phase IN ('intent', 'source_changed', 'dependencies', 'verifying', 'publishing', 'complete'))
);
CREATE UNIQUE INDEX one_unfinished_comfy_update ON comfy_update_runs ((1)) WHERE phase <> 'complete';
