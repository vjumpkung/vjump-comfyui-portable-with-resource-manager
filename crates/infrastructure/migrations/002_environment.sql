-- Environment context: environments, persisted provisioning runs and the wizard draft.
-- The active selection is enforced by a partial unique index: at most one active row.
CREATE TABLE envs (
    id TEXT PRIMARY KEY,
    path TEXT NOT NULL,
    owned INTEGER NOT NULL CHECK (owned IN (0, 1)),
    source TEXT NOT NULL CHECK (source IN ('managed', 'custom_venv', 'custom_interpreter')),
    base_interpreter TEXT,
    python_version TEXT NOT NULL,
    torch_version TEXT,
    cuda_variant TEXT,
    profile_id TEXT NOT NULL,
    profile_revision INTEGER NOT NULL,
    triton_dist TEXT,
    pins_json TEXT NOT NULL,
    verification TEXT,
    authorized_location TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('provisioning', 'verified', 'failed', 'repair_required')),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    active INTEGER NOT NULL DEFAULT 0 CHECK (active IN (0, 1))
);

CREATE UNIQUE INDEX envs_one_active ON envs (active) WHERE active = 1;

CREATE TABLE provisioning_runs (
    id TEXT PRIMARY KEY,
    status TEXT NOT NULL,
    mode TEXT NOT NULL,
    target_env_id TEXT NOT NULL,
    previous_env_id TEXT,
    doc_json TEXT NOT NULL,
    cancel_requested INTEGER NOT NULL DEFAULT 0 CHECK (cancel_requested IN (0, 1)),
    activation_phase TEXT NOT NULL DEFAULT 'none',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX provisioning_runs_status ON provisioning_runs (status);
CREATE INDEX provisioning_runs_created ON provisioning_runs (created_at);

CREATE TABLE wizard_draft (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
