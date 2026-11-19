use super::*;
use crate::{
    environment::StepContext,
    shared::{CancelToken, MaintenanceLease},
};
use async_trait::async_trait;
use domain::{comfy_runtime::*, environment::PythonEnvironment};
use std::{path::Path, sync::Arc};

#[async_trait]
pub trait RuntimeStore: Send + Sync {
    async fn launch_options(&self) -> Result<LaunchOptions, RuntimeError>;
    async fn save_launch_options(&self, options: &LaunchOptions) -> Result<(), RuntimeError>;
    async fn save_session(
        &self,
        session: &RuntimeSession,
        finished: bool,
    ) -> Result<(), RuntimeError>;
    async fn unfinished_sessions(&self) -> Result<Vec<RuntimeSession>, RuntimeError>;
    async fn save_update(&self, run: &UpdateRun) -> Result<(), RuntimeError>;
    async fn unfinished_update(&self) -> Result<Option<UpdateRun>, RuntimeError>;
}

#[async_trait]
pub trait ComfyRepository: Send + Sync {
    async fn revision(&self, root: &Path) -> Result<Revision, RuntimeError>;
    async fn status(&self, root: &Path) -> Result<RepoState, RuntimeError>;
    async fn fetch(
        &self,
        root: &Path,
        target: &UpdateTarget,
        ctx: &StepContext,
    ) -> Result<(), RuntimeError>;
    async fn freeze(
        &self,
        root: &Path,
        target: &UpdateTarget,
    ) -> Result<FrozenUpdate, RuntimeError>;
    async fn apply(
        &self,
        root: &Path,
        frozen: &FrozenUpdate,
        ctx: &StepContext,
    ) -> Result<(), RuntimeError>;
    async fn versions(&self, root: &Path) -> Result<VersionList, RuntimeError>;
}
#[async_trait]
pub trait PortProbe: Send + Sync {
    async fn available(&self, port: Port, listen_lan: bool) -> Result<bool, RuntimeError>;
    async fn next_free(&self, after: Port, listen_lan: bool) -> Result<Option<Port>, RuntimeError>;
}
#[async_trait]
pub trait LaunchPreflight: Send + Sync {
    async fn capabilities(
        &self,
        root: &Path,
        env: &PythonEnvironment,
        cancel: &CancelToken,
    ) -> Result<LaunchCapabilities, RuntimeError>;
}
#[async_trait]
pub trait ExtraModelPaths: Send + Sync {
    async fn read(&self, root: &Path) -> Result<Vec<ModelRoot>, RuntimeError>;
}
/// Writes the one section of `extra_model_paths.yaml` that the app owns. Separate from
/// [`ExtraModelPaths`] so read-only consumers never depend on it.
#[async_trait]
pub trait ExtraModelPathsWriter: Send + Sync {
    /// Points every model folder at `base` and creates the missing primary folders.
    async fn generate(&self, root: &Path, base: &Path) -> Result<(), RuntimeError>;
    /// Removes the app's section only; a missing file or section is a successful no-op.
    async fn remove(&self, root: &Path) -> Result<(), RuntimeError>;
}

/// `before_start` and `reconcile_after_stop` run under the runtime workflow's held lease and must
/// not acquire the gate. `note_start`, `start_finished`, `request_scan`, `scans_held_off` and
/// `scan_settling` never touch the gate either.
#[async_trait]
pub trait NodeLifecycle: Send + Sync {
    /// Settles interrupted node work before ComfyUI starts. Fails when something must be settled
    /// but cannot be. The start itself is recorded by [`Self::note_start`], before this runs.
    async fn before_start(&self, lease: &mut dyn MaintenanceLease) -> Result<(), RuntimeError>;
    /// Settles interrupted node work after ComfyUI stopped, before source or package changes.
    async fn reconcile_after_stop(
        &self,
        lease: &mut dyn MaintenanceLease,
    ) -> Result<(), RuntimeError>;
    /// Records that a start of ComfyUI began, before anything else of it runs, so a scan never
    /// trusts a stopped reading taken while the start was in progress. Every call is paired with
    /// one [`Self::start_finished`], however the start ends.
    fn note_start(&self);
    /// Records that the start announced by [`Self::note_start`] ended.
    fn start_finished(&self);
    /// Asks for a custom-node scan in the background and returns at once. A scan never blocks
    /// or fails Stop; Start, Restart and Update wait at most a few seconds for a scan that is
    /// settling interrupted node operations, and otherwise a scan never delays them.
    fn request_scan(&self);
    /// True while closing holds scans off, so a Start, Restart or Update that just took the gate
    /// must not go on.
    fn scans_held_off(&self) -> bool;
    /// True while a scan is settling interrupted node operations under the maintenance gate. It
    /// holds the gate only briefly, so Start, Restart and Update wait for it instead of failing.
    fn scan_settling(&self) -> bool;
}

/// One installation-scoped supervisor, also used by M1 RuntimeMaintenance.
#[async_trait]
pub trait RuntimeSupervisor: super::RuntimeMaintenance {
    async fn status(&self) -> RuntimeStatus;
    async fn inventory(&self, root: &Path) -> Result<InventoryState, RuntimeError>;
    /// Owns the lease through readiness or failed-start cleanup, even if the caller disconnects.
    async fn start(
        &self,
        session: RuntimeSession,
        args: Vec<String>,
        lease: Box<dyn MaintenanceLease>,
    ) -> Result<RuntimeStatus, RuntimeError>;
    /// Retained preflight lets Stop cancel verification/help commands as well as readiness.
    async fn start_with_preflight(
        &self,
        session: RuntimeSession,
        env: PythonEnvironment,
        preflight: Arc<dyn LaunchPreflight>,
        lease: Box<dyn MaintenanceLease>,
    ) -> Result<RuntimeStatus, RuntimeError> {
        let root = session
            .checkout
            .parent()
            .ok_or(RuntimeError::InvalidRecord)?;
        let capabilities = preflight
            .capabilities(root, &env, &CancelToken::new())
            .await?;
        let args = session.options.args(cfg!(windows), &capabilities)?;
        self.start(session, args, lease).await
    }
    /// No gate acquisition: can cancel a start which already holds it.
    async fn stop(&self) -> Result<RuntimeStatus, RuntimeError>;
}
#[async_trait]
pub trait RuntimeEventReceiver: Send {
    async fn next(&mut self) -> Option<RuntimeEvent>;
}
pub struct RuntimeSubscription {
    pub id: u64,
    pub receiver: Box<dyn RuntimeEventReceiver>,
}
pub trait RuntimeEvents: Send + Sync {
    fn subscribe(&self, after_cursor: Option<u64>) -> RuntimeSubscription;
    fn unsubscribe(&self, id: u64);
}
pub trait RuntimeLogReader: Send + Sync {
    fn last_lines(&self, limit: usize) -> Vec<String>;
}

#[derive(Clone)]
pub struct RuntimeDeps {
    pub environment: crate::environment::EnvironmentDeps,
    pub nodes: Arc<dyn NodeLifecycle>,
    pub supervisor: Arc<dyn RuntimeSupervisor>,
    pub store: Arc<dyn RuntimeStore>,
    pub repository: Arc<dyn ComfyRepository>,
    pub ports: Arc<dyn PortProbe>,
    pub preflight: Arc<dyn LaunchPreflight>,
    pub extra_paths: Arc<dyn ExtraModelPaths>,
}
