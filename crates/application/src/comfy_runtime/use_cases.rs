use super::*;
use crate::{
    environment::{ManifestState, RootSwitcher, same_location},
    shared::MaintenanceLease,
};
use domain::{
    comfy_runtime::*,
    environment::{EnvState, PythonEnvironment},
};

pub(crate) async fn selected(
    deps: &RuntimeDeps,
    allow_repair: bool,
) -> Result<PythonEnvironment, RuntimeError> {
    let environment = &deps.environment;
    let root = environment.root.current().root;
    if environment
        .runs
        .unfinished()
        .await
        .map_err(|_| RuntimeError::Storage)?
        .iter()
        .any(|r| r.status.is_active() || r.activation.is_incomplete())
    {
        return Err(RuntimeError::RecoveryRequired(
            "finish environment setup/activation first".into(),
        ));
    }
    let env = environment
        .envs
        .active()
        .await
        .map_err(|_| RuntimeError::Storage)?
        .ok_or(RuntimeError::EnvironmentNotUsable)?;
    if env.state() != EnvState::Verified
        && !(allow_repair && env.state() == EnvState::RepairRequired)
    {
        return Err(RuntimeError::EnvironmentNotUsable);
    }
    match environment
        .manifests
        .read(&root)
        .await
        .map_err(|_| RuntimeError::Storage)?
    {
        ManifestState::Valid(manifest)
            if manifest.env_id == env.id
                && same_location(&manifest.env_path, &env.path)
                && same_location(&manifest.install_root, &root) => {}
        _ => {
            return Err(RuntimeError::RecoveryRequired(
                "reconcile install.json and active environment first".into(),
            ));
        }
    }
    Ok(env)
}

pub struct StartComfy {
    deps: RuntimeDeps,
}
impl StartComfy {
    pub fn new(deps: RuntimeDeps) -> Self {
        Self { deps }
    }
    pub async fn execute(&self) -> Result<RuntimeStatus, RuntimeError> {
        let lease = self
            .deps
            .environment
            .gate
            .try_acquire()
            .await
            .map_err(|_| RuntimeError::Busy)?;
        self.start_under_lease(lease).await
    }
    pub(crate) async fn start_under_lease(
        &self,
        mut lease: Box<dyn MaintenanceLease>,
    ) -> Result<RuntimeStatus, RuntimeError> {
        let deps = &self.deps;
        deps.nodes.before_start(&mut *lease).await?;
        let root = deps.environment.root.current().root;
        match deps.supervisor.inventory(&root).await? {
            InventoryState::Stopped => {}
            InventoryState::OwnedRunning => return Err(RuntimeError::NeedsStop),
            InventoryState::VerifiedSurvivor(_) | InventoryState::Unknown(_) => {
                return Err(RuntimeError::RecoveryRequired(
                    "process ownership requires recovery before start".into(),
                ));
            }
        }
        if deps.store.unfinished_update().await?.is_some() {
            return Err(RuntimeError::RecoveryRequired(
                "retry the interrupted ComfyUI update or repair it first".into(),
            ));
        }
        let env = selected(deps, false).await?;
        let options = deps.store.launch_options().await?;
        options.validate()?;
        if !deps
            .ports
            .available(options.port, options.listen_lan)
            .await?
        {
            return Err(RuntimeError::PortConflict {
                requested: options.port,
                suggested: deps
                    .ports
                    .next_free(options.port, options.listen_lan)
                    .await?,
            });
        }
        let revision = deps.repository.revision(&root).await?;
        let interpreter = if cfg!(windows) {
            env.path.join("Scripts/python.exe")
        } else {
            env.path.join("bin/python")
        };
        let session = RuntimeSession {
            id: SessionId::new(deps.environment.ids.new_id())?,
            env_id: env.id.clone(),
            interpreter,
            checkout: root.join("ComfyUI"),
            revision,
            options,
            started_at: deps.environment.clock.now(),
            identity: None,
        };
        deps.supervisor
            .start_with_preflight(session, env, deps.preflight.clone(), lease)
            .await
    }
}
pub struct StopComfy {
    deps: RuntimeDeps,
}
impl StopComfy {
    pub fn new(deps: RuntimeDeps) -> Self {
        Self { deps }
    }
    pub async fn execute(&self) -> Result<RuntimeStatus, RuntimeError> {
        // Stop must be able to cancel a Start which already holds the gate.
        self.deps.supervisor.stop().await?;
        let mut lease = self
            .deps
            .environment
            .gate
            .acquire()
            .await
            .map_err(|_| RuntimeError::Busy)?;
        // Another Start may have won the interval before we acquired the gate.
        let status = self.deps.supervisor.stop().await?;
        self.deps.nodes.after_stop(&mut *lease).await?;
        Ok(status)
    }
}
pub struct RestartComfy {
    deps: RuntimeDeps,
}
impl RestartComfy {
    pub fn new(deps: RuntimeDeps) -> Self {
        Self { deps }
    }
    pub async fn execute(&self) -> Result<RuntimeStatus, RuntimeError> {
        let mut lease = self
            .deps
            .environment
            .gate
            .try_acquire()
            .await
            .map_err(|_| RuntimeError::Busy)?;
        self.deps.supervisor.stop().await?;
        self.deps.nodes.after_stop(&mut *lease).await?;
        StartComfy::new(self.deps.clone())
            .start_under_lease(lease)
            .await
    }
}
pub struct GetComfyStatus {
    deps: RuntimeDeps,
}
impl GetComfyStatus {
    pub fn new(deps: RuntimeDeps) -> Self {
        Self { deps }
    }
    pub async fn execute(&self) -> Result<RuntimeStatus, RuntimeError> {
        let mut status = self.deps.supervisor.status().await;
        if let Some(run) = self.deps.store.unfinished_update().await? {
            status.recovery = Some(format!(
                "update {} requires Retry/Repair ({})",
                run.id.as_str(),
                run.phase.as_str()
            ));
        }
        Ok(status)
    }
}
pub struct GetLaunchOptions {
    store: std::sync::Arc<dyn RuntimeStore>,
}
impl GetLaunchOptions {
    pub fn new(store: std::sync::Arc<dyn RuntimeStore>) -> Self {
        Self { store }
    }
    pub async fn execute(&self) -> Result<LaunchOptions, RuntimeError> {
        self.store.launch_options().await
    }
}
pub struct SetLaunchOptions {
    store: std::sync::Arc<dyn RuntimeStore>,
}
impl SetLaunchOptions {
    pub fn new(store: std::sync::Arc<dyn RuntimeStore>) -> Self {
        Self { store }
    }
    /// LAN consent belongs to the manager's explicit warning/setting flow.
    pub async fn execute(&self, options: LaunchOptions) -> Result<(), RuntimeError> {
        options.validate()?;
        self.store.save_launch_options(&options).await
    }
}
pub struct ListComfyVersions {
    deps: RuntimeDeps,
}
impl ListComfyVersions {
    pub fn new(deps: RuntimeDeps) -> Self {
        Self { deps }
    }
    pub async fn execute(&self) -> Result<VersionList, RuntimeError> {
        self.deps
            .repository
            .versions(&self.deps.environment.root.current().root)
            .await
    }
}
pub struct ReadExtraModelPaths {
    deps: RuntimeDeps,
}

pub struct GetComfyDiagnostics {
    deps: RuntimeDeps,
    logs: std::sync::Arc<dyn RuntimeLogReader>,
}
impl GetComfyDiagnostics {
    pub fn new(deps: RuntimeDeps, logs: std::sync::Arc<dyn RuntimeLogReader>) -> Self {
        Self { deps, logs }
    }
    pub async fn execute(&self) -> Result<RuntimeDiagnostics, RuntimeError> {
        let root = self.deps.environment.root.current().root;
        let env = self
            .deps
            .environment
            .envs
            .active()
            .await
            .map_err(|_| RuntimeError::Storage)?;
        let hardware = self
            .deps
            .environment
            .hardware
            .probe(&root, env.as_ref().map(|e| e.path.as_path()))
            .await;
        let gpus = match hardware.gpus {
            domain::environment::GpuProbe::Detected(gpus) => gpus
                .into_iter()
                .map(|g| {
                    format!(
                        "{} ({} MiB)",
                        g.name,
                        g.vram_mib
                            .map_or_else(|| "unknown".into(), |n| n.to_string())
                    )
                })
                .collect(),
            domain::environment::GpuProbe::NoNvidiaGpu => vec!["No NVIDIA GPU detected".into()],
            domain::environment::GpuProbe::Unavailable => {
                vec!["GPU observation unavailable".into()]
            }
        };
        Ok(RuntimeDiagnostics {
            os: hardware.os_version,
            gpus,
            driver_version: hardware.driver_version,
            env_id: env.as_ref().map(|e| e.id.clone()),
            python_version: env.as_ref().map(|e| e.python_version.to_string()),
            protected_packages: env.map_or_else(Vec::new, |e| {
                e.pins
                    .iter()
                    .map(|(n, v)| (n.to_string(), v.to_owned()))
                    .collect()
            }),
            revision: self.deps.repository.revision(&root).await?,
            status: GetComfyStatus::new(self.deps.clone()).execute().await?,
            log_tail: self.logs.last_lines(200),
        })
    }
}
impl ReadExtraModelPaths {
    pub fn new(deps: RuntimeDeps) -> Self {
        Self { deps }
    }
    pub async fn execute(&self) -> Result<Vec<ModelRoot>, RuntimeError> {
        self.deps
            .extra_paths
            .read(&self.deps.environment.root.current().root)
            .await
    }
}

/// Generates or removes the app's section of `extra_model_paths.yaml` in the current root.
pub struct ManageExtraModelPaths {
    root: std::sync::Arc<dyn RootSwitcher>,
    writer: std::sync::Arc<dyn ExtraModelPathsWriter>,
}
impl ManageExtraModelPaths {
    pub fn new(
        root: std::sync::Arc<dyn RootSwitcher>,
        writer: std::sync::Arc<dyn ExtraModelPathsWriter>,
    ) -> Self {
        Self { root, writer }
    }
    pub async fn generate(&self, base: &std::path::Path) -> Result<(), RuntimeError> {
        self.writer.generate(&self.root.current().root, base).await
    }
    pub async fn remove(&self) -> Result<(), RuntimeError> {
        self.writer.remove(&self.root.current().root).await
    }
}
