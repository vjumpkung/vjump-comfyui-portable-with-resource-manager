//! Switching the install root before setup. The new root's database is created and filled
//! (settings through the repository, the wizard draft), and only then the bootstrap pointer is
//! atomically published. The app restarts to recompose its database and logging on the new root.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use application::{
    environment::{RootInfo, RootSource, RootSwitchError, RootSwitcher, WizardDraftStore},
    settings::{SettingsRepoError, SettingsRepository},
};
use async_trait::async_trait;

use super::{python::normalize_path, sqlite::SqliteEnvironmentStore};
use crate::{
    settings::sqlite::SqliteSettingsRepository,
    shared::{
        paths::{AppPaths, write_bootstrap},
        sqlite::Db,
    },
};

pub struct FsRootSwitcher {
    current: RootInfo,
    bootstrap_file: PathBuf,
    settings: Arc<dyn SettingsRepository>,
}

impl FsRootSwitcher {
    pub fn new(
        current: RootInfo,
        bootstrap_file: PathBuf,
        settings: Arc<dyn SettingsRepository>,
    ) -> Self {
        Self {
            current,
            bootstrap_file,
            settings,
        }
    }
}

fn components(path: &Path) -> Vec<String> {
    path.components()
        .map(|part| {
            let text = part.as_os_str().to_string_lossy().into_owned();
            if cfg!(windows) {
                text.to_lowercase()
            } else {
                text
            }
        })
        .collect()
}

pub(crate) fn nested(inner: &Path, outer: &Path) -> bool {
    let (inner, outer) = (components(inner), components(outer));
    inner.len() >= outer.len() && inner[..outer.len()] == outer[..]
}

/// Validates a candidate root without creating anything. Returns its normalized path.
pub fn validate_destination(candidate: &Path, current: &Path) -> Result<PathBuf, RootSwitchError> {
    let invalid = |reason: &str| RootSwitchError::InvalidDestination(reason.to_owned());
    if !candidate.is_absolute() {
        return Err(invalid("choose an absolute folder path"));
    }
    let normalized = normalize_path(candidate);
    if normalized.parent().is_none() {
        return Err(invalid("choose a folder, not a drive or filesystem root"));
    }
    let current = normalize_path(current);
    if nested(&normalized, &current) || nested(&current, &normalized) {
        return Err(invalid(
            "the folder must be different from, and not inside or around, the current install root",
        ));
    }
    if normalized.exists() {
        if !normalized.is_dir() {
            return Err(invalid("the selected path is a file"));
        }
        let entries: Vec<String> = fs::read_dir(&normalized)
            .map_err(|_| invalid("the folder cannot be read"))?
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        // An empty folder, or an existing install of this app, may be used.
        let existing_install = entries
            .iter()
            .any(|name| name == "install.json" || name == "data");
        if !entries.is_empty() && !existing_install {
            return Err(invalid("the folder is not empty"));
        }
    }
    Ok(normalized)
}

fn map_settings(error: SettingsRepoError) -> RootSwitchError {
    match error {
        SettingsRepoError::InvalidPersisted(_) | SettingsRepoError::Unavailable => {
            RootSwitchError::Failed
        }
    }
}

#[async_trait]
impl RootSwitcher for FsRootSwitcher {
    fn current(&self) -> RootInfo {
        self.current.clone()
    }

    async fn switch_root(
        &self,
        new_root: &Path,
        draft: Option<&str>,
    ) -> Result<(), RootSwitchError> {
        if self.current.source == RootSource::DebugOverride {
            return Err(RootSwitchError::FixedByOverride);
        }
        let destination = validate_destination(new_root, &self.current.root)?;
        let paths = AppPaths::from_root(&destination);
        paths.ensure_dirs().map_err(|_| {
            RootSwitchError::InvalidDestination("the folder cannot be written".to_owned())
        })?;
        let settings = self.settings.load().await.map_err(map_settings)?;
        {
            // The new database is opened, filled and closed before the pointer is published.
            let db = Db::open(&paths.db_path)
                .await
                .map_err(|_| RootSwitchError::Failed)?;
            if let Some(settings) = settings {
                SqliteSettingsRepository::new(db.clone())
                    .save(&settings)
                    .await
                    .map_err(map_settings)?;
            }
            if let Some(draft) = draft {
                WizardDraftStore::save(&SqliteEnvironmentStore::new(db), draft)
                    .await
                    .map_err(|_| RootSwitchError::Failed)?;
            }
        }
        let bootstrap_file = self.bootstrap_file.clone();
        tokio::task::spawn_blocking(move || write_bootstrap(&bootstrap_file, &destination))
            .await
            .map_err(|_| RootSwitchError::Failed)?
            .map_err(|_| RootSwitchError::Failed)
    }
}

#[cfg(test)]
mod tests;
