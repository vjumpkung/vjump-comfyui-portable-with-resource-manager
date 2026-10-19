//! Root selection available before an installation can be composed.

use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use application::{
    comfy_runtime::{RuntimeBlock, RuntimeMaintenance},
    environment::{
        Checkout, ComfySource, EnvironmentRepository, ExistingInstallOpener, ManifestState,
        RootInfo, RootSource, RootSwitchError, normalized_path, same_location,
    },
    shared::{MaintenanceGate, MaintenanceLease},
};
use async_trait::async_trait;
use domain::shared::model_values::SafeFileName;

use crate::{
    environment::{
        manifest::parse_manifest,
        python::normalize_path,
        runtime_guard::{Verdict, assess},
        source::GitSource,
        sqlite::SqliteEnvironmentStore,
        switcher::nested,
    },
    shared::{
        model_paths::{PinnedDirectory, pin_absolute},
        paths::{AppPaths, write_bootstrap},
        procscan::{self, SnapshotFn},
        sqlite::{Db, DbError},
    },
};

pub struct FsExistingInstallOpener {
    current: Option<RootInfo>,
    recorded_old: Option<PathBuf>,
    bootstrap: PathBuf,
    gate: Arc<dyn MaintenanceGate>,
    source: GitSource,
    /// The process-list source; always the real one outside tests.
    snapshot: SnapshotFn,
    restart_lease: Mutex<Option<Box<dyn MaintenanceLease>>>,
}

impl FsExistingInstallOpener {
    pub fn new(
        current: Option<RootInfo>,
        recorded_old: Option<PathBuf>,
        bootstrap: PathBuf,
        gate: Arc<dyn MaintenanceGate>,
    ) -> Self {
        Self {
            current,
            recorded_old,
            bootstrap,
            gate,
            source: GitSource::new(),
            snapshot: procscan::snapshot,
            restart_lease: Mutex::new(None),
        }
    }

    fn check_override(&self) -> Result<(), RootSwitchError> {
        if self
            .current
            .as_ref()
            .is_some_and(|current| current.source == RootSource::DebugOverride)
        {
            return Err(RootSwitchError::FixedByOverride);
        }
        Ok(())
    }

    /// The destination must stay clear of the current and recorded roots, as for a root switch.
    fn check_separate(&self, destination: &Path) -> Result<(), RootSwitchError> {
        let existing = self
            .current
            .iter()
            .map(|current| &current.root)
            .chain(&self.recorded_old);
        for root in existing {
            let root = normalize_path(root);
            if nested(destination, &root) || nested(&root, destination) {
                return Err(invalid(
                    "the folder must be different from, and not inside or around, the current or recorded install root",
                ));
            }
        }
        Ok(())
    }

    fn roots(&self, candidate: &Path, recorded: Option<&Path>) -> Vec<PathBuf> {
        let mut roots = vec![candidate.to_owned()];
        roots.extend(self.current.as_ref().map(|current| current.root.clone()));
        roots.extend(self.recorded_old.clone());
        roots.extend(recorded.map(Path::to_owned));
        roots
    }

    async fn publish(
        &self,
        destination: PathBuf,
        lease: Box<dyn MaintenanceLease>,
    ) -> Result<(), RootSwitchError> {
        let bootstrap = self.bootstrap.clone();
        tokio::task::spawn_blocking(move || write_bootstrap(&bootstrap, &destination))
            .await
            .map_err(|_| RootSwitchError::Failed)?
            .map_err(|_| RootSwitchError::Failed)?;
        // Keep maintenance excluded while the controller quiesces workers and restarts.
        *self.restart_lease.lock().unwrap_or_else(|p| p.into_inner()) = Some(lease);
        Ok(())
    }
}

fn invalid(reason: &str) -> RootSwitchError {
    RootSwitchError::InvalidDestination(reason.to_owned())
}

fn name(value: &str) -> Result<SafeFileName, RootSwitchError> {
    SafeFileName::new(value).map_err(|_| invalid("the folder contains an invalid path"))
}

fn normalized_root(path: &Path) -> Result<PathBuf, RootSwitchError> {
    if !path.is_absolute() {
        return Err(invalid("choose an absolute folder path"));
    }
    let normalized = normalize_path(path);
    if normalized.parent().is_none() {
        return Err(invalid("choose a folder, not a drive or filesystem root"));
    }
    Ok(normalized)
}

fn real_root(path: &Path) -> Result<Arc<PinnedDirectory>, RootSwitchError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(RootSwitchError::Missing);
        }
        Err(_) => return Err(invalid("the installation folder cannot be read")),
        Ok(_) => {}
    }
    // Pin the spelling supplied by the user before canonicalization can hide a link.
    pin_absolute(path).map_err(|_| invalid("choose a real installation folder without links"))
}

fn has_publication_journal(root: &Path) -> Result<bool, RootSwitchError> {
    let root = match pin_absolute(root) {
        Ok(root) => root,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err(invalid("the folder cannot be read safely")),
    };
    for entry in fs::read_dir(&root.path).map_err(|_| invalid("the folder cannot be read"))? {
        let entry = entry.map_err(|_| invalid("the folder cannot be read"))?;
        let text = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if text.starts_with('.') && (text.ends_with(".publish.json") || text.ends_with(".cloning"))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn check_journals(db: &Db) -> Result<(), RootSwitchError> {
    let pending = db
        .run(|connection| {
            connection
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM provisioning_runs WHERE activation_phase NOT IN \
                     ('none', 'committed')) OR EXISTS(SELECT 1 FROM comfy_update_runs \
                     WHERE phase <> 'complete')",
                    [],
                    |row| row.get::<_, bool>(0),
                )
                .map_err(|_| DbError::Unavailable)
        })
        .await
        .map_err(|_| invalid("the installation database cannot be validated"))?;
    if pending {
        return Err(RootSwitchError::RelocationRecoveryRequired);
    }
    Ok(())
}

/// A strict, nonmutating process inventory; unreadable ownership is always a refusal.
pub async fn ensure_roots_stopped(roots: &[PathBuf]) -> Result<(), RootSwitchError> {
    ensure_roots_stopped_with(roots, procscan::snapshot).await
}

/// [`ensure_roots_stopped`] over an injected process-list source (the real one outside tests).
async fn ensure_roots_stopped_with(
    roots: &[PathBuf],
    snapshot: SnapshotFn,
) -> Result<(), RootSwitchError> {
    let roots: Vec<_> = roots.iter().map(|root| normalize_path(root)).collect();
    tokio::task::spawn_blocking(move || {
        let snapshot = snapshot();
        for root in roots {
            match assess(&snapshot, &root, std::process::id()) {
                Verdict::Stopped => {}
                Verdict::Running(_) => return Err(RootSwitchError::ComfyUiRunning),
                Verdict::Unreadable | Verdict::SnapshotFailed => {
                    return Err(RootSwitchError::ComfyUiStateUnknown);
                }
            }
        }
        Ok(())
    })
    .await
    .map_err(|_| RootSwitchError::ComfyUiStateUnknown)?
}

pub struct RelocationRuntimeGuard;

#[async_trait]
impl RuntimeMaintenance for RelocationRuntimeGuard {
    async fn ensure_stopped(&self, root: &Path) -> Result<(), RuntimeBlock> {
        ensure_roots_stopped(&[root.to_owned()])
            .await
            .map_err(|error| match error {
                RootSwitchError::ComfyUiRunning => RuntimeBlock::Running,
                _ => RuntimeBlock::CannotAssess,
            })
    }
}

#[async_trait]
impl ExistingInstallOpener for FsExistingInstallOpener {
    async fn open_existing(&self, path: &Path) -> Result<(), RootSwitchError> {
        self.check_override()?;
        let lease = self
            .gate
            .try_acquire()
            .await
            .map_err(|_| RootSwitchError::MaintenanceBusy)?;
        let destination = normalized_root(path)?;
        self.check_separate(&destination)?;
        let root = real_root(path)?;
        let mut manifest_text = String::new();
        let mut manifest_file = root
            .open_read_pin(&name("install.json")?)
            .map_err(|_| invalid("a supported install.json is required"))?;
        manifest_file
            .read_to_string(&mut manifest_text)
            .map_err(|_| invalid("install.json cannot be read"))?;
        let ManifestState::Valid(manifest) = parse_manifest(&manifest_text) else {
            return Err(invalid("a supported install.json is required"));
        };
        if !manifest.install_root.is_absolute() || !manifest.env_path.is_absolute() {
            return Err(invalid("install.json contains an invalid location"));
        }
        let data = root
            .child(&name("data")?, false)
            .map_err(|_| invalid("the installation data folder is missing or unsafe"))?;
        let _database_pin = data
            .open_read_pin(&name("app.db")?)
            .map_err(|_| invalid("an existing readable installation database is required"))?;
        let db = Db::open_existing(AppPaths::from_root(&destination).db_path)
            .await
            .map_err(|_| invalid("an existing readable installation database is required"))?;
        let active = SqliteEnvironmentStore::new(db.clone())
            .active()
            .await
            .map_err(|_| invalid("the active environment cannot be read"))?
            .ok_or_else(|| invalid("the installation has no active environment"))?;
        if active.id != manifest.env_id
            || normalized_path(&active.path) != normalized_path(&manifest.env_path)
            || active.ownership != manifest.ownership
            || active.python_version != manifest.python_version
            || active.profile_id != manifest.profile_id
            || active.profile_revision != manifest.profile_revision
        {
            return Err(invalid(
                "install.json does not match the active environment",
            ));
        }
        let checkout = root
            .child(&name("ComfyUI")?, false)
            .map_err(|_| invalid("the ComfyUI checkout is missing or unsafe"))?;
        let _git = checkout
            .child(&name(".git")?, false)
            .map_err(|_| invalid("a contained ComfyUI Git checkout is required"))?;
        let _main = checkout
            .open_read_pin(&name("main.py")?)
            .map_err(|_| invalid("the ComfyUI checkout is incomplete"))?;
        if !matches!(
            self.source.inspect_checkout(&destination).await,
            Ok(Checkout::Repo { .. })
        ) {
            return Err(invalid("a readable ComfyUI Git checkout is required"));
        }
        // An install that has not moved recovers its own journals at startup; only a relocated
        // one must have none, because rebuilding it cannot resolve them.
        if !same_location(&normalize_path(&manifest.install_root), &destination) {
            check_journals(&db).await?;
            if has_publication_journal(&destination)?
                || has_publication_journal(&checkout.path.join("custom_nodes"))?
            {
                return Err(RootSwitchError::RelocationRecoveryRequired);
            }
        }
        ensure_roots_stopped_with(
            &self.roots(&destination, Some(&manifest.install_root)),
            self.snapshot,
        )
        .await?;
        self.publish(destination, lease).await
    }

    async fn select_new(&self, path: &Path) -> Result<(), RootSwitchError> {
        self.check_override()?;
        let lease = self
            .gate
            .try_acquire()
            .await
            .map_err(|_| RootSwitchError::MaintenanceBusy)?;
        let destination = normalized_root(path)?;
        self.check_separate(&destination)?;
        let (mut destination_pin, missing) = match fs::symlink_metadata(path) {
            Ok(_) => {
                let root = real_root(path)?;
                let mut entries = fs::read_dir(root.path.clone())
                    .map_err(|_| invalid("the selected folder cannot be read"))?;
                if entries
                    .next()
                    .transpose()
                    .map_err(|_| invalid("the selected folder cannot be read"))?
                    .is_some()
                {
                    return Err(invalid("the selected folder must be empty"));
                }
                (root, Vec::new())
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let mut missing = vec![name(
                    path.file_name()
                        .and_then(|part| part.to_str())
                        .ok_or_else(|| invalid("choose a folder"))?,
                )?];
                let mut ancestor = path.parent().ok_or_else(|| invalid("choose a folder"))?;
                loop {
                    match fs::symlink_metadata(ancestor) {
                        Ok(_) => {
                            break (real_root(ancestor)?, missing);
                        }
                        Err(error) if error.kind() == io::ErrorKind::NotFound => {
                            missing.push(name(
                                ancestor
                                    .file_name()
                                    .and_then(|part| part.to_str())
                                    .ok_or_else(|| invalid("choose a folder"))?,
                            )?);
                            ancestor = ancestor
                                .parent()
                                .ok_or_else(|| invalid("choose a folder"))?;
                        }
                        Err(_) => return Err(invalid("the selected folder cannot be accessed")),
                    }
                }
            }
            Err(_) => return Err(invalid("the selected folder cannot be accessed")),
        };
        ensure_roots_stopped_with(&self.roots(&destination, None), self.snapshot).await?;
        for component in missing.into_iter().rev() {
            destination_pin = destination_pin
                .create_new_child(&component)
                .map_err(|_| invalid("the new installation folder could not be created"))?;
        }
        self.publish(destination, lease).await
    }
}

#[cfg(test)]
mod tests;
