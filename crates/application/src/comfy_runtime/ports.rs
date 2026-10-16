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
    async fn fetch(&self, root: &Path, ctx: &StepContext) -> Result<(), RuntimeError>;
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
    pub supervisor: Arc<dyn RuntimeSupervisor>,
    pub store: Arc<dyn RuntimeStore>,
    pub repository: Arc<dyn ComfyRepository>,
    pub ports: Arc<dyn PortProbe>,
    pub preflight: Arc<dyn LaunchPreflight>,
    pub extra_paths: Arc<dyn ExtraModelPaths>,
}
