-- Custom nodes (M5): installed-node provenance, the phased operation journal, per-environment
-- dependency outcomes, the registry response cache and remembered trust.
CREATE TABLE nodes (
 id TEXT PRIMARY KEY,
 name TEXT NOT NULL,
 single_file INTEGER NOT NULL CHECK(single_file IN (0, 1)),
 enabled INTEGER NOT NULL CHECK(enabled IN (0, 1)),
 source_kind TEXT NOT NULL CHECK(source_kind IN ('registry', 'git', 'manual')),
 source_json TEXT NOT NULL,
 origin TEXT NOT NULL CHECK(origin IN ('app', 'observed')),
 tree_digest TEXT NOT NULL,
 dependencies TEXT NOT NULL CHECK(dependencies IN ('not_run', 'ok', 'failed')),
 conflict INTEGER NOT NULL CHECK(conflict IN (0, 1)),
 dirty INTEGER,
 updated_at TEXT NOT NULL,
 UNIQUE(name, single_file)
);

-- One row per install, update, enable, disable, uninstall or dependency retry. The phase is the
-- recovery boundary: startup reconciliation reads it together with the recorded identities.
CREATE TABLE node_operations (
 id TEXT PRIMARY KEY,
 kind TEXT NOT NULL CHECK(kind IN ('install', 'update', 'retry_dependencies', 'enable', 'disable', 'uninstall')),
 phase TEXT NOT NULL CHECK(phase IN ('prepared', 'publish_intent', 'old_backed_up', 'published', 'dependencies_in_progress', 'verified', 'cleanup_pending', 'completed', 'failed')),
 node_json TEXT NOT NULL,
 prior_json TEXT,
 environment_id TEXT NOT NULL,
 pins_json TEXT NOT NULL,
 binding TEXT NOT NULL,
 artifact_digest TEXT,
 requirements_digest TEXT,
 hook_digest TEXT,
 consent_identity TEXT,
 failure TEXT,
 staging TEXT NOT NULL,
 backup TEXT NOT NULL,
 requirements_json TEXT NOT NULL,
 created_at TEXT NOT NULL,
 updated_at TEXT NOT NULL
);
CREATE INDEX node_operations_phase ON node_operations(phase);

-- Dependency outcomes belong to an environment and an exact source revision.
CREATE TABLE node_env_deps (
 node_id TEXT NOT NULL,
 environment_id TEXT NOT NULL,
 source_identity TEXT NOT NULL,
 state TEXT NOT NULL CHECK(state IN ('not_run', 'ok', 'failed')),
 updated_at TEXT NOT NULL,
 PRIMARY KEY(node_id, environment_id, source_identity)
);

-- Keys include the endpoint, normalized query, page, limit and a cache schema version.
CREATE TABLE registry_cache (
 cache_key TEXT PRIMARY KEY,
 body TEXT NOT NULL,
 fetched_at INTEGER NOT NULL
);

-- Trust is namespaced by source kind plus publisher/node/repository identity.
CREATE TABLE trusted_sources (
 namespace TEXT PRIMARY KEY,
 allow_install_py INTEGER NOT NULL CHECK(allow_install_py IN (0, 1)),
 created_at TEXT NOT NULL
);
