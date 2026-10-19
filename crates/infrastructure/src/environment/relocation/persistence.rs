//! Explicit relocation acceptance. The caller holds the maintenance gate and has proved that
//! ComfyUI processes for the recorded and current roots have stopped.

use std::{
    cmp::Reverse,
    fs, io,
    path::{Path, PathBuf},
};

use application::environment::{RelocationError, RelocationStore, normalized_path, rebase_path};
use async_trait::async_trait;
use rusqlite::{Connection, Transaction, params};

use crate::{environment::python::normalize_path, shared::sqlite::Db};

pub struct SqliteRelocationStore {
    db: Db,
}

impl SqliteRelocationStore {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

fn storage(_: impl std::fmt::Debug) -> RelocationError {
    RelocationError::Storage
}

fn existing_or_recorded(path: &Path) -> Result<PathBuf, RelocationError> {
    if !path.is_absolute() {
        return Err(RelocationError::Storage);
    }
    let mut existing = path;
    let mut missing = Vec::new();
    loop {
        match fs::canonicalize(existing) {
            Ok(mut canonical) => {
                for component in missing.into_iter().rev() {
                    canonical.push(component);
                }
                return Ok(canonical);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                match (existing.file_name(), existing.parent()) {
                    (Some(name), Some(parent)) => {
                        missing.push(name);
                        existing = parent;
                    }
                    _ => return Ok(path.to_owned()),
                }
            }
            // Offline shares and denied access cannot be canonicalized; their spelling stands.
            Err(_) => return Ok(path.to_owned()),
        }
    }
}

fn path_text(path: &Path) -> Result<String, RelocationError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or(RelocationError::Storage)
}

/// `text` moved from the first of `olds` (longest component prefix first) that contains it. The
/// canonical spelling only proves containment; the stored text never carries a verbatim prefix.
fn rebased(text: &str, olds: &[PathBuf], new: &Path) -> Result<Option<String>, RelocationError> {
    let path = Path::new(text);
    if !path.is_absolute() {
        return Ok(None);
    }
    let proof = existing_or_recorded(path)?;
    olds.iter()
        .find_map(|old| rebase_path(&proof, old, new))
        .map(|path| path_text(&normalize_path(&path)))
        .transpose()
}

/// Interrupted ComfyUI publication markers that startup recovery has not resolved.
fn publication_journal_in(directory: &Path) -> Result<bool, RelocationError> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(storage(error)),
    };
    for entry in entries {
        let name = entry
            .map_err(storage)?
            .file_name()
            .to_string_lossy()
            .to_ascii_lowercase();
        if name.starts_with('.') && (name.ends_with(".publish.json") || name.ends_with(".cloning"))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn source_publication_pending(root: &Path) -> Result<bool, RelocationError> {
    Ok(publication_journal_in(root)?
        || publication_journal_in(&root.join("ComfyUI").join("custom_nodes"))?)
}

fn requires_recovery(transaction: &Transaction<'_>) -> Result<bool, RelocationError> {
    transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM provisioning_runs \
             WHERE activation_phase NOT IN ('none', 'committed')) \
             OR EXISTS(SELECT 1 FROM comfy_update_runs WHERE phase <> 'complete') \
             OR EXISTS(SELECT 1 FROM model_operations) \
             OR EXISTS(SELECT 1 FROM node_operations \
             WHERE phase NOT IN ('prepared', 'completed', 'failed')) \
             OR EXISTS(SELECT 1 FROM jobs \
             WHERE finalize_phase IS NOT NULL AND state <> 'completed')",
            [],
            |row| row.get(0),
        )
        .map_err(storage)
}

struct EnvironmentPaths {
    id: String,
    path: String,
    base_interpreter: Option<String>,
    authorized_location: String,
    owned: bool,
    state: String,
}

fn rebase_environments(
    transaction: &Transaction<'_>,
    olds: &[PathBuf],
    new: &Path,
) -> Result<(), RelocationError> {
    let environments = {
        let mut statement = transaction
            .prepare(
                "SELECT id, path, base_interpreter, authorized_location, owned, state FROM envs",
            )
            .map_err(storage)?;
        statement
            .query_map([], |row| {
                Ok(EnvironmentPaths {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    base_interpreter: row.get(2)?,
                    authorized_location: row.get(3)?,
                    owned: row.get(4)?,
                    state: row.get(5)?,
                })
            })
            .map_err(storage)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage)?
    };
    for environment in environments {
        let path = rebased(&environment.path, olds, new)?;
        let base = environment
            .base_interpreter
            .as_deref()
            .map(|base| rebased(base, olds, new))
            .transpose()?
            .flatten();
        let authorized = rebased(&environment.authorized_location, olds, new)?;
        let state = if environment.owned && path.is_some() {
            match environment.state.as_str() {
                "verified" => "repair_required",
                "provisioning" => "failed",
                state => state,
            }
        } else {
            &environment.state
        };
        if path.is_none() && base.is_none() && authorized.is_none() && state == environment.state {
            continue;
        }
        transaction
            .execute(
                "UPDATE envs SET path = ?2, base_interpreter = ?3, authorized_location = ?4, \
                 state = ?5, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
                params![
                    environment.id,
                    path.as_deref().unwrap_or(&environment.path),
                    base.as_deref().or(environment.base_interpreter.as_deref()),
                    authorized
                        .as_deref()
                        .unwrap_or(&environment.authorized_location),
                    state,
                ],
            )
            .map_err(storage)?;
    }
    Ok(())
}

fn rebase_model_roots(
    transaction: &Transaction<'_>,
    olds: &[PathBuf],
    new: &Path,
) -> Result<(), RelocationError> {
    let roots = {
        let mut statement = transaction
            .prepare("SELECT id, path FROM model_roots")
            .map_err(storage)?;
        statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(storage)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage)?
    };
    for (id, path) in roots {
        if let Some(path) = rebased(&path, olds, new)? {
            transaction
                .execute(
                    "UPDATE model_roots SET path = ?2 WHERE id = ?1",
                    params![id, path],
                )
                .map_err(storage)?;
        }
    }
    Ok(())
}

/// The recorded old root plus every root an earlier acceptance moved from or to, except the
/// current one, longest component prefix first. When a failed rebuild is followed by another
/// copy, the persisted paths still sit in the root the first acceptance moved them to.
fn source_roots(
    transaction: &Transaction<'_>,
    old: &Path,
    new_key: &str,
) -> Result<Vec<PathBuf>, RelocationError> {
    let mut candidates = vec![old.to_owned()];
    {
        let mut statement = transaction
            .prepare("SELECT old_root, new_root FROM install_relocations")
            .map_err(storage)?;
        let recorded = statement
            .query_map([], |row| {
                Ok([row.get::<_, String>(0)?, row.get::<_, String>(1)?])
            })
            .map_err(storage)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage)?;
        candidates.extend(recorded.into_iter().flatten().map(PathBuf::from));
    }
    let mut roots: Vec<PathBuf> = Vec::new();
    for root in candidates {
        let key = normalized_path(&root);
        if root.is_absolute()
            && path_text(&key)? != new_key
            && roots.iter().all(|seen| normalized_path(seen) != key)
        {
            roots.push(root);
        }
    }
    roots.sort_by_key(|root| Reverse(normalized_path(root).components().count()));
    Ok(roots)
}

fn accept(connection: &mut Connection, old: &Path, new: &Path) -> Result<(), RelocationError> {
    let old = existing_or_recorded(old)?;
    let new = existing_or_recorded(new)?;
    let old_key = path_text(&normalized_path(&old))?;
    let new_key = path_text(&normalized_path(&new))?;
    // A rebuild in place has nothing to relocate; startup reconciliation owns every journal.
    if old_key == new_key {
        return Ok(());
    }
    if source_publication_pending(&old)? || source_publication_pending(&new)? {
        return Err(RelocationError::RecoveryRequired);
    }
    let transaction = connection.transaction().map_err(storage)?;
    if requires_recovery(&transaction)? {
        return Err(RelocationError::RecoveryRequired);
    }
    let accepted: bool = transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM install_relocations WHERE old_root = ?1 AND new_root = ?2)",
            params![old_key, new_key],
            |row| row.get(0),
        )
        .map_err(storage)?;
    if accepted {
        return Ok(());
    }
    let olds = source_roots(&transaction, &old, &new_key)?;
    rebase_environments(&transaction, &olds, &new)?;
    rebase_model_roots(&transaction, &olds, &new)?;
    transaction
        .execute(
            "UPDATE comfy_runtime_sessions SET finished = 1 WHERE finished = 0",
            [],
        )
        .map_err(storage)?;
    transaction
        .execute(
            "INSERT INTO install_relocations(old_root, new_root) VALUES (?1, ?2)",
            params![old_key, new_key],
        )
        .map_err(storage)?;
    transaction.commit().map_err(storage)
}

#[async_trait]
impl RelocationStore for SqliteRelocationStore {
    async fn check(&self, old_root: &Path, new_root: &Path) -> Result<(), RelocationError> {
        let old = existing_or_recorded(old_root)?;
        let new = existing_or_recorded(new_root)?;
        if path_text(&normalized_path(&old))? == path_text(&normalized_path(&new))? {
            return Ok(());
        }
        if source_publication_pending(&old)? || source_publication_pending(&new)? {
            return Err(RelocationError::RecoveryRequired);
        }
        self.db
            .run(|connection| {
                let result = (|| {
                    let transaction = connection.transaction().map_err(storage)?;
                    if requires_recovery(&transaction)? {
                        Err(RelocationError::RecoveryRequired)
                    } else {
                        Ok(())
                    }
                })();
                Ok(result)
            })
            .await
            .map_err(storage)?
    }
    async fn accept(&self, old_root: &Path, new_root: &Path) -> Result<(), RelocationError> {
        let old = old_root.to_owned();
        let new = new_root.to_owned();
        self.db
            .run(move |connection| Ok(accept(connection, &old, &new)))
            .await
            .map_err(storage)?
    }
}

#[cfg(test)]
#[path = "persistence_tests.rs"]
mod tests;
