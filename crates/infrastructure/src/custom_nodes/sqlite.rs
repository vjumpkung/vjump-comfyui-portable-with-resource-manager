//! SQLite persistence for installed nodes, the operation journal, dependency outcomes, the
//! registry response cache and remembered trust.
use crate::shared::{clock::SystemClock, sqlite::Db};
use application::{custom_nodes::*, shared::Clock};
use async_trait::async_trait;
use domain::{
    custom_nodes::*,
    environment::{DistName, EnvId, PinSet},
    shared::model_values::SafeFileName,
};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

pub struct SqliteNodes {
    pub db: Db,
}

fn storage<E>(_: E) -> NodeAppError {
    NodeAppError::Storage
}

pub(crate) fn source_to_json(source: &NodeSource) -> Value {
    match source {
        NodeSource::Registry {
            id,
            publisher,
            version,
            repository,
        } => {
            json!({"kind": "registry", "id": id, "publisher": publisher, "version": version, "repository": repository})
        }
        NodeSource::Git {
            repository,
            commit,
            branch,
        } => json!({"kind": "git", "repository": repository, "commit": commit, "branch": branch}),
        NodeSource::Manual => json!({"kind": "manual"}),
    }
}

pub(crate) fn source_from_json(value: &Value) -> Option<NodeSource> {
    let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
    match value.get("kind")?.as_str()? {
        "registry" => Some(NodeSource::Registry {
            id: text("id")?,
            publisher: text("publisher"),
            version: text("version")?,
            repository: text("repository"),
        }),
        "git" => Some(NodeSource::Git {
            repository: text("repository")?,
            commit: text("commit")?,
            branch: text("branch"),
        }),
        "manual" => Some(NodeSource::Manual),
        _ => None,
    }
}

fn source_kind(source: &NodeSource) -> &'static str {
    match source {
        NodeSource::Registry { .. } => "registry",
        NodeSource::Git { .. } => "git",
        NodeSource::Manual => "manual",
    }
}

fn dependency_str(state: DependencyState) -> &'static str {
    match state {
        DependencyState::NotRun => "not_run",
        DependencyState::Ok => "ok",
        DependencyState::Failed => "failed",
    }
}

fn dependency_from(text: &str) -> Option<DependencyState> {
    Some(match text {
        "not_run" => DependencyState::NotRun,
        "ok" => DependencyState::Ok,
        "failed" => DependencyState::Failed,
        _ => return None,
    })
}

fn origin_str(origin: ProvenanceOrigin) -> &'static str {
    match origin {
        ProvenanceOrigin::App => "app",
        ProvenanceOrigin::ExternallyObserved => "observed",
    }
}

pub(crate) fn node_to_json(node: &InstalledNode) -> Value {
    json!({
        "id": node.id.as_str(), "name": node.name.as_str(), "source": source_to_json(&node.source),
        "origin": origin_str(node.origin), "enabled": node.enabled, "single_file": node.single_file,
        "tree_digest": node.tree_digest, "dependencies": dependency_str(node.dependencies),
        "conflict": node.conflict, "dirty": node.dirty,
    })
}

pub(crate) fn node_from_json(value: &Value) -> Option<InstalledNode> {
    Some(InstalledNode {
        id: NodeId::new(value.get("id")?.as_str()?).ok()?,
        name: SafeFileName::new(value.get("name")?.as_str()?).ok()?,
        source: source_from_json(value.get("source")?)?,
        origin: match value.get("origin")?.as_str()? {
            "app" => ProvenanceOrigin::App,
            "observed" => ProvenanceOrigin::ExternallyObserved,
            _ => return None,
        },
        enabled: value.get("enabled")?.as_bool()?,
        single_file: value.get("single_file")?.as_bool()?,
        tree_digest: value.get("tree_digest")?.as_str()?.to_owned(),
        dependencies: dependency_from(value.get("dependencies")?.as_str()?)?,
        conflict: value.get("conflict")?.as_bool()?,
        dirty: value.get("dirty").and_then(Value::as_bool),
    })
}

fn pins_to_json(pins: &PinSet) -> Value {
    Value::Object(
        pins.iter()
            .map(|(name, version)| (name.as_str().to_owned(), Value::String(version.to_owned())))
            .collect(),
    )
}

fn pins_from_json(value: &Value) -> Option<PinSet> {
    let mut pins = PinSet::new();
    for (name, version) in value.as_object()? {
        pins.insert(DistName::new(name).ok()?, version.as_str()?);
    }
    Some(pins)
}

fn kind_str(kind: OperationKind) -> &'static str {
    match kind {
        OperationKind::Install => "install",
        OperationKind::Update => "update",
        OperationKind::RetryDependencies => "retry_dependencies",
        OperationKind::Enable => "enable",
        OperationKind::Disable => "disable",
        OperationKind::Uninstall => "uninstall",
    }
}

fn kind_from(text: &str) -> Option<OperationKind> {
    Some(match text {
        "install" => OperationKind::Install,
        "update" => OperationKind::Update,
        "retry_dependencies" => OperationKind::RetryDependencies,
        "enable" => OperationKind::Enable,
        "disable" => OperationKind::Disable,
        "uninstall" => OperationKind::Uninstall,
        _ => return None,
    })
}

fn phase_from(text: &str) -> Option<OperationPhase> {
    Some(match text {
        "prepared" => OperationPhase::Prepared,
        "publish_intent" => OperationPhase::PublishIntent,
        "old_backed_up" => OperationPhase::OldBackedUp,
        "published" => OperationPhase::Published,
        "dependencies_in_progress" => OperationPhase::DependenciesInProgress,
        "verified" => OperationPhase::Verified,
        "cleanup_pending" => OperationPhase::CleanupPending,
        "completed" => OperationPhase::Completed,
        "failed" => OperationPhase::Failed,
        _ => return None,
    })
}

/// Failure text is sanitized before it is stored: no URLs with credentials, no absolute paths.
fn sanitized(text: &str) -> String {
    let redacted = crate::shared::logging::redact(text);
    redacted.chars().take(500).collect()
}

struct OperationRow {
    id: String,
    kind: String,
    phase: String,
    node: String,
    prior: Option<String>,
    environment: String,
    pins: String,
    binding: String,
    artifact_digest: Option<String>,
    requirements_digest: Option<String>,
    hook_digest: Option<String>,
    consent_identity: Option<String>,
    failure: Option<String>,
    staging: String,
    backup: String,
    requirements: String,
}

fn operation_from(row: OperationRow) -> Option<NodeOperation> {
    let node = node_from_json(&serde_json::from_str(&row.node).ok()?)?;
    let prior = match row.prior {
        Some(text) => Some(node_from_json(&serde_json::from_str(&text).ok()?)?),
        None => None,
    };
    let requirements: Vec<String> = serde_json::from_str(&row.requirements).ok()?;
    Some(NodeOperation {
        id: NodeId::new(row.id).ok()?,
        kind: kind_from(&row.kind)?,
        phase: phase_from(&row.phase)?,
        node,
        prior,
        environment: EnvId::new(row.environment).ok()?,
        pins: pins_from_json(&serde_json::from_str(&row.pins).ok()?)?,
        binding: row.binding,
        artifact_digest: row.artifact_digest,
        requirements_digest: row.requirements_digest,
        hook_digest: row.hook_digest,
        consent_identity: row.consent_identity,
        failure: row.failure,
        staging: row.staging,
        backup: row.backup,
        dependency_requirements: requirements,
    })
}

#[async_trait]
impl NodeRepository for SqliteNodes {
    async fn nodes(&self) -> Result<Vec<InstalledNode>, NodeAppError> {
        self.db
            .run(|c| {
                let mut statement = c
                    .prepare(
                        "SELECT id, name, single_file, enabled, source_json, origin, tree_digest, \
                         dependencies, conflict, dirty FROM nodes ORDER BY name COLLATE NOCASE, id",
                    )
                    .map_err(|_| crate::shared::sqlite::DbError::Unavailable)?;
                let rows = statement
                    .query_map([], |row| {
                        Ok(json!({
                            "id": row.get::<_, String>(0)?, "name": row.get::<_, String>(1)?,
                            "single_file": row.get::<_, i64>(2)? != 0, "enabled": row.get::<_, i64>(3)? != 0,
                            "source": serde_json::from_str::<Value>(&row.get::<_, String>(4)?).unwrap_or(Value::Null),
                            "origin": row.get::<_, String>(5)?, "tree_digest": row.get::<_, String>(6)?,
                            "dependencies": row.get::<_, String>(7)?, "conflict": row.get::<_, i64>(8)? != 0,
                            "dirty": row.get::<_, Option<i64>>(9)?.map(|v| v != 0),
                        }))
                    })
                    .map_err(|_| crate::shared::sqlite::DbError::Unavailable)?;
                let mut values = Vec::new();
                for row in rows {
                    values.push(row.map_err(|_| crate::shared::sqlite::DbError::Unavailable)?);
                }
                Ok(values)
            })
            .await
            .map_err(storage)?
            .iter()
            .map(|value| node_from_json(value).ok_or(NodeAppError::Storage))
            .collect()
    }

    async fn save_node(&self, node: &InstalledNode) -> Result<(), NodeAppError> {
        let node = node.clone();
        let now = SystemClock.now();
        self.db
            .run(move |c| {
                // A different record for the same folder is replaced, never duplicated.
                let tx = c
                    .transaction()
                    .map_err(|_| crate::shared::sqlite::DbError::Unavailable)?;
                tx.execute(
                    "DELETE FROM nodes WHERE name = ?1 AND single_file = ?2 AND id <> ?3",
                    params![node.name.as_str(), node.single_file, node.id.as_str()],
                )
                .map_err(|_| crate::shared::sqlite::DbError::Unavailable)?;
                tx.execute(
                    "INSERT INTO nodes(id, name, single_file, enabled, source_kind, source_json, origin, \
                     tree_digest, dependencies, conflict, dirty, updated_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12) \
                     ON CONFLICT(id) DO UPDATE SET name = excluded.name, single_file = excluded.single_file, \
                     enabled = excluded.enabled, source_kind = excluded.source_kind, \
                     source_json = excluded.source_json, origin = excluded.origin, \
                     tree_digest = excluded.tree_digest, dependencies = excluded.dependencies, \
                     conflict = excluded.conflict, dirty = excluded.dirty, updated_at = excluded.updated_at",
                    params![
                        node.id.as_str(),
                        node.name.as_str(),
                        node.single_file,
                        node.enabled,
                        source_kind(&node.source),
                        source_to_json(&node.source).to_string(),
                        origin_str(node.origin),
                        node.tree_digest,
                        dependency_str(node.dependencies),
                        node.conflict,
                        node.dirty,
                        now
                    ],
                )
                .map_err(|_| crate::shared::sqlite::DbError::Unavailable)?;
                tx.commit()
                    .map_err(|_| crate::shared::sqlite::DbError::Unavailable)
            })
            .await
            .map_err(storage)
    }

    async fn remove_node(&self, id: &NodeId) -> Result<(), NodeAppError> {
        let id = id.as_str().to_owned();
        self.db
            .run(move |c| {
                c.execute("DELETE FROM nodes WHERE id = ?1", [&id])
                    .map(|_| ())
                    .map_err(|_| crate::shared::sqlite::DbError::Unavailable)
            })
            .await
            .map_err(storage)
    }

    async fn save_operation(&self, op: &NodeOperation) -> Result<(), NodeAppError> {
        let op = op.clone();
        let now = SystemClock.now();
        self.db
            .run(move |c| {
                c.execute(
                    "INSERT INTO node_operations(id, kind, phase, node_json, prior_json, environment_id, \
                     pins_json, binding, artifact_digest, requirements_digest, hook_digest, consent_identity, \
                     failure, staging, backup, requirements_json, created_at, updated_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?17) \
                     ON CONFLICT(id) DO UPDATE SET phase = excluded.phase, node_json = excluded.node_json, \
                     prior_json = excluded.prior_json, artifact_digest = excluded.artifact_digest, \
                     requirements_digest = excluded.requirements_digest, hook_digest = excluded.hook_digest, \
                     consent_identity = excluded.consent_identity, failure = excluded.failure, \
                     requirements_json = excluded.requirements_json, updated_at = excluded.updated_at",
                    params![
                        op.id.as_str(),
                        kind_str(op.kind),
                        op.phase.as_str(),
                        node_to_json(&op.node).to_string(),
                        op.prior.as_ref().map(|p| node_to_json(p).to_string()),
                        op.environment.as_str(),
                        pins_to_json(&op.pins).to_string(),
                        op.binding,
                        op.artifact_digest,
                        op.requirements_digest,
                        op.hook_digest,
                        op.consent_identity,
                        op.failure.as_deref().map(sanitized),
                        op.staging,
                        op.backup,
                        serde_json::to_string(&op.dependency_requirements).unwrap_or_default(),
                        now
                    ],
                )
                .map(|_| ())
                .map_err(|_| crate::shared::sqlite::DbError::Unavailable)
            })
            .await
            .map_err(storage)
    }

    async fn operations(&self) -> Result<Vec<NodeOperation>, NodeAppError> {
        let rows = self
            .db
            .run(|c| {
                let mut statement = c
                    .prepare(
                        "SELECT id, kind, phase, node_json, prior_json, environment_id, pins_json, binding, \
                         artifact_digest, requirements_digest, hook_digest, consent_identity, failure, \
                         staging, backup, requirements_json FROM node_operations ORDER BY created_at, rowid",
                    )
                    .map_err(|_| crate::shared::sqlite::DbError::Unavailable)?;
                let rows = statement
                    .query_map([], |row| {
                        Ok(OperationRow {
                            id: row.get(0)?,
                            kind: row.get(1)?,
                            phase: row.get(2)?,
                            node: row.get(3)?,
                            prior: row.get(4)?,
                            environment: row.get(5)?,
                            pins: row.get(6)?,
                            binding: row.get(7)?,
                            artifact_digest: row.get(8)?,
                            requirements_digest: row.get(9)?,
                            hook_digest: row.get(10)?,
                            consent_identity: row.get(11)?,
                            failure: row.get(12)?,
                            staging: row.get(13)?,
                            backup: row.get(14)?,
                            requirements: row.get(15)?,
                        })
                    })
                    .map_err(|_| crate::shared::sqlite::DbError::Unavailable)?;
                let mut values = Vec::new();
                for row in rows {
                    values.push(row.map_err(|_| crate::shared::sqlite::DbError::Unavailable)?);
                }
                Ok(values)
            })
            .await
            .map_err(storage)?;
        rows.into_iter()
            .map(|row| operation_from(row).ok_or(NodeAppError::Storage))
            .collect()
    }

    async fn save_dependency(&self, outcome: &DependencyOutcome) -> Result<(), NodeAppError> {
        let outcome = outcome.clone();
        let now = SystemClock.now();
        self.db
            .run(move |c| {
                c.execute(
                    "INSERT INTO node_env_deps(node_id, environment_id, source_identity, state, updated_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT(node_id, environment_id, source_identity) \
                     DO UPDATE SET state = excluded.state, updated_at = excluded.updated_at",
                    params![
                        outcome.node.as_str(),
                        outcome.environment.as_str(),
                        outcome.source_identity,
                        dependency_str(outcome.state),
                        now
                    ],
                )
                .map(|_| ())
                .map_err(|_| crate::shared::sqlite::DbError::Unavailable)
            })
            .await
            .map_err(storage)
    }

    async fn dependency(
        &self,
        node: &NodeId,
        environment: &EnvId,
        source_identity: &str,
    ) -> Result<Option<DependencyState>, NodeAppError> {
        let node = node.as_str().to_owned();
        let environment = environment.as_str().to_owned();
        let source_identity = source_identity.to_owned();
        let state = self
            .db
            .run(move |c| {
                c.query_row(
                    "SELECT state FROM node_env_deps WHERE node_id = ?1 AND environment_id = ?2 \
                     AND source_identity = ?3",
                    params![node, environment, source_identity],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(|_| crate::shared::sqlite::DbError::Unavailable)
            })
            .await
            .map_err(storage)?;
        state
            .map(|state| dependency_from(&state).ok_or(NodeAppError::Storage))
            .transpose()
    }

    async fn trust(&self, namespace: &str) -> Result<Option<Trust>, NodeAppError> {
        let namespace = namespace.to_owned();
        self.db
            .run(move |c| {
                c.query_row(
                    "SELECT allow_install_py FROM trusted_sources WHERE namespace = ?1",
                    [&namespace],
                    |row| {
                        Ok(Trust {
                            namespace: namespace.clone(),
                            allow_install_py: row.get::<_, i64>(0)? != 0,
                        })
                    },
                )
                .optional()
                .map_err(|_| crate::shared::sqlite::DbError::Unavailable)
            })
            .await
            .map_err(storage)
    }

    async fn save_trust(&self, trust: &Trust) -> Result<(), NodeAppError> {
        let trust = trust.clone();
        let now = SystemClock.now();
        self.db
            .run(move |c| {
                c.execute(
                    "INSERT INTO trusted_sources(namespace, allow_install_py, created_at) VALUES (?1, ?2, ?3) \
                     ON CONFLICT(namespace) DO UPDATE SET allow_install_py = excluded.allow_install_py",
                    params![trust.namespace, trust.allow_install_py, now],
                )
                .map(|_| ())
                .map_err(|_| crate::shared::sqlite::DbError::Unavailable)
            })
            .await
            .map_err(storage)
    }
}

/// Registry response cache: the body is stored exactly as normalized by the registry client.
impl SqliteNodes {
    pub async fn cache_get(&self, key: &str) -> Option<(String, i64)> {
        let key = key.to_owned();
        self.db
            .run(move |c| {
                Ok(c.query_row(
                    "SELECT body, fetched_at FROM registry_cache WHERE cache_key = ?1",
                    [&key],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
                )
                .optional()
                .ok()
                .flatten())
            })
            .await
            .ok()
            .flatten()
    }

    pub async fn cache_put(&self, key: &str, body: &str, fetched_at: i64) {
        let (key, body) = (key.to_owned(), body.to_owned());
        let _ = self
            .db
            .run(move |c| {
                c.execute(
                    "INSERT INTO registry_cache(cache_key, body, fetched_at) VALUES (?1, ?2, ?3) \
                     ON CONFLICT(cache_key) DO UPDATE SET body = excluded.body, fetched_at = excluded.fetched_at",
                    params![key, body, fetched_at],
                )
                .map(|_| ())
                .map_err(|_| crate::shared::sqlite::DbError::Unavailable)
            })
            .await;
    }
}
