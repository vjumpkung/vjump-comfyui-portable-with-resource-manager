//! `install.json`: the marker that identifies the active environment of an install root.
//! Unreadable, invalid or newer files are reported, never silently replaced.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use application::environment::{
    InstallManifest, InstallManifestStore, MANIFEST_SCHEMA_VERSION, ManifestError, ManifestState,
    RunId,
};
use async_trait::async_trait;
use domain::environment::{EnvId, Ownership, PythonVersion};
use serde::{Deserialize, Serialize};

use super::{layout::Layout, sqlite::ComponentsDoc};
use crate::shared::fs::atomic_write;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestDoc {
    schema_version: u32,
    install_root: PathBuf,
    generation: u64,
    env_id: String,
    env_path: PathBuf,
    ownership: String,
    python_version: String,
    profile_id: String,
    profile_revision: u32,
    comfyui_reference: Option<String>,
    comfyui_commit: Option<String>,
    run_id: String,
    activated_at: String,
    #[serde(default)]
    components: Option<ComponentsDoc>,
}

impl From<&InstallManifest> for ManifestDoc {
    fn from(manifest: &InstallManifest) -> Self {
        Self {
            schema_version: manifest.schema_version,
            install_root: manifest.install_root.clone(),
            generation: manifest.generation,
            env_id: manifest.env_id.to_string(),
            env_path: manifest.env_path.clone(),
            ownership: manifest.ownership.as_str().to_owned(),
            python_version: manifest.python_version.to_string(),
            profile_id: manifest.profile_id.clone(),
            profile_revision: manifest.profile_revision,
            comfyui_reference: manifest.comfyui_reference.clone(),
            comfyui_commit: manifest.comfyui_commit.clone(),
            run_id: manifest.run_id.to_string(),
            activated_at: manifest.activated_at.clone(),
            components: Some(ComponentsDoc::from(&manifest.components)),
        }
    }
}

impl ManifestDoc {
    fn into_manifest(self) -> Option<InstallManifest> {
        if self.schema_version == 2 && self.components.is_none() {
            return None;
        }
        Some(InstallManifest {
            schema_version: self.schema_version,
            install_root: self.install_root,
            generation: self.generation,
            env_id: EnvId::new(self.env_id).ok()?,
            env_path: self.env_path,
            ownership: self.ownership.parse::<Ownership>().ok()?,
            python_version: self.python_version.parse::<PythonVersion>().ok()?,
            profile_id: self.profile_id,
            profile_revision: self.profile_revision,
            comfyui_reference: self.comfyui_reference,
            comfyui_commit: self.comfyui_commit,
            run_id: RunId::new(self.run_id).ok()?,
            activated_at: self.activated_at,
            components: if self.schema_version == 1 {
                Default::default()
            } else {
                self.components.map(Into::into).unwrap_or_default()
            },
        })
    }
}

/// Parses the file content into one of the non-absent states.
pub fn parse_manifest(text: &str) -> ManifestState {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return ManifestState::Corrupt;
    };
    match value
        .get("schema_version")
        .and_then(|version| version.as_u64())
    {
        Some(version) if version > u64::from(MANIFEST_SCHEMA_VERSION) => ManifestState::Newer,
        Some(1 | 2) => {
            match serde_json::from_value::<ManifestDoc>(value)
                .ok()
                .and_then(ManifestDoc::into_manifest)
            {
                Some(manifest) => ManifestState::Valid(manifest),
                None => ManifestState::Corrupt,
            }
        }
        _ => ManifestState::Corrupt,
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct FsManifestStore;

impl FsManifestStore {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl InstallManifestStore for FsManifestStore {
    async fn read(&self, root: &Path) -> Result<ManifestState, ManifestError> {
        let path = Layout::new(root).manifest_path();
        tokio::task::spawn_blocking(move || match fs::read_to_string(&path) {
            Ok(text) => parse_manifest(&text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => ManifestState::Absent,
            // Unreadable (permissions, a folder in its place, invalid UTF-8): needs recovery.
            Err(_) => ManifestState::Corrupt,
        })
        .await
        .map_err(|_| ManifestError::ReadFailed)
    }

    async fn publish(&self, manifest: &InstallManifest) -> Result<(), ManifestError> {
        let path = Layout::new(&manifest.install_root).manifest_path();
        let text = serde_json::to_string_pretty(&ManifestDoc::from(manifest))
            .map_err(|_| ManifestError::WriteFailed)?;
        tokio::task::spawn_blocking(move || {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            atomic_write(&path, text.as_bytes())
        })
        .await
        .map_err(|_| ManifestError::WriteFailed)?
        .map_err(|_| ManifestError::WriteFailed)
    }
}

#[cfg(test)]
mod tests;
