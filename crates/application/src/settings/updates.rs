//! Signed self-update of the application (FR-SET-4).
//!
//! The updater adapter retains the exact release the user reviewed. Installation names that
//! release by an opaque, process-local id and can never resolve or install another one. The
//! workflow here owns the maintenance gate and the stopped-runtime proof for the whole
//! installation, including the adapter's cleanup before an installer may end the process.

use std::sync::Arc;

use async_trait::async_trait;
use thiserror::Error;

use crate::{
    comfy_runtime::{RuntimeBlock, RuntimeMaintenance},
    environment::{EnvironmentDeps, RootSwitcher},
    shared::{GateError, MaintenanceGate},
};

/// A newer signed release awaiting the user's review. `update_id` is only meaningful to the
/// updater that issued it and only until the next check or installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateOffer {
    pub update_id: String,
    pub version: String,
    pub date: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum UpdateError {
    #[error("signed updates are not configured in this build")]
    NotConfigured,
    #[error("the reviewed update is no longer available; check again")]
    Stale,
    #[error("the update server could not be checked")]
    CheckFailed,
    #[error("the update signature is not valid")]
    SignatureInvalid,
    #[error("the update could not be installed")]
    InstallFailed,
    #[error(transparent)]
    Gate(#[from] GateError),
    #[error(transparent)]
    Runtime(#[from] RuntimeBlock),
}

impl UpdateError {
    /// The stable machine-readable code of the failure.
    pub fn code(self) -> &'static str {
        match self {
            Self::NotConfigured => "updater_not_configured",
            Self::Stale => "update_stale",
            Self::CheckFailed => "update_check_failed",
            Self::SignatureInvalid => "update_signature_invalid",
            Self::InstallFailed => "update_install_failed",
            Self::Gate(GateError::Busy) => "maintenance_busy",
            Self::Gate(GateError::Unavailable) => "maintenance_unavailable",
            Self::Runtime(RuntimeBlock::Running) => "comfyui_running",
            Self::Runtime(RuntimeBlock::CannotAssess) => "comfyui_state_unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateProgress {
    /// `total` is unknown when the server does not announce a length.
    Downloading {
        downloaded: u64,
        total: Option<u64>,
    },
    Installing,
    Restarting,
    Failed(UpdateError),
}

/// Receives installation progress. A sink whose consumer is gone must drop events silently:
/// losing the UI never cancels the installation.
pub trait UpdateProgressSink: Send + Sync {
    fn report(&self, progress: UpdateProgress);
}

#[async_trait]
pub trait AppUpdater: Send + Sync {
    /// False when the build carries no complete, valid updater configuration. An unconfigured
    /// updater never touches the network.
    fn configured(&self) -> bool;

    /// The newer release, if any. Replaces every earlier offer.
    async fn check(&self) -> Result<Option<UpdateOffer>, UpdateError>;

    /// Downloads and verifies the release named by `update_id`, stops the application's own
    /// background work, installs it and restarts. The id is consumed, and an id that is not the
    /// retained reviewed offer fails with [`UpdateError::Stale`] before anything is downloaded.
    /// The installer may end the process, so all cleanup happens before it is invoked.
    async fn install(
        &self,
        update_id: &str,
        sink: Arc<dyn UpdateProgressSink>,
    ) -> Result<(), UpdateError>;
}

/// Installs the reviewed release under the maintenance gate. The lease is held until the
/// updater returns, so no other maintenance operation can start while the application's
/// background work is quiesced and the installer runs.
pub struct InstallAppUpdate {
    gate: Arc<dyn MaintenanceGate>,
    runtime: Arc<dyn RuntimeMaintenance>,
    root: Arc<dyn RootSwitcher>,
}

impl InstallAppUpdate {
    pub fn new(deps: &EnvironmentDeps) -> Self {
        Self {
            gate: deps.gate.clone(),
            runtime: deps.runtime.clone(),
            root: deps.root.clone(),
        }
    }

    pub async fn execute(
        &self,
        updater: &dyn AppUpdater,
        update_id: &str,
        sink: Arc<dyn UpdateProgressSink>,
    ) -> Result<(), UpdateError> {
        let result = self.run(updater, update_id, sink.clone()).await;
        if let Err(error) = result {
            sink.report(UpdateProgress::Failed(error));
        }
        result
    }

    async fn run(
        &self,
        updater: &dyn AppUpdater,
        update_id: &str,
        sink: Arc<dyn UpdateProgressSink>,
    ) -> Result<(), UpdateError> {
        if !updater.configured() {
            return Err(UpdateError::NotConfigured);
        }
        let _lease = self.gate.try_acquire().await?;
        // The process inventory is read only under the gate, so nothing can start meanwhile. A
        // retained headless runtime is a running runtime: the installer would replace the
        // application under it.
        let root = self.root.current().root;
        self.runtime.ensure_stopped(&root).await?;
        updater.install(update_id, sink).await
    }
}

#[cfg(test)]
mod tests;
