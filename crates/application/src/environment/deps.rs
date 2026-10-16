use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use super::{
    model::RunId,
    packages::{PackagePlan, PlanId},
    ports::{
        ActivationStore, ComfySource, EnvVerifier, EnvironmentRepository, HardwareProbe,
        InstallManifestStore, PackageInstaller, ProfileCatalog, ProvisioningRunRepository,
        PythonProvisioner, RootSwitcher, ToolProvisioner, WizardDraftStore,
    },
};
use crate::{
    comfy_runtime::RuntimeMaintenance,
    shared::{CancelToken, Clock, IdGenerator, MaintenanceGate},
};

/// The in-process cancellation tokens of runs that are registered or executing.
#[derive(Default)]
pub struct ActiveRuns(Mutex<HashMap<RunId, CancelToken>>);

impl ActiveRuns {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<RunId, CancelToken>> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Registers a fresh token, replacing a stale one.
    pub fn register(&self, id: &RunId) -> CancelToken {
        let token = CancelToken::new();
        self.lock().insert(id.clone(), token.clone());
        token
    }

    pub fn get(&self, id: &RunId) -> Option<CancelToken> {
        self.lock().get(id).cloned()
    }

    pub fn remove(&self, id: &RunId) {
        self.lock().remove(id);
    }
}

/// Package plans waiting for confirmation. They live only in this process, so a restart
/// invalidates them.
#[derive(Default)]
pub struct PlanStore(Mutex<Vec<PackagePlan>>);

const MAX_PLANS: usize = 16;

impl PlanStore {
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<PackagePlan>> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn insert(&self, plan: PackagePlan) {
        let mut plans = self.lock();
        if plans.len() >= MAX_PLANS {
            plans.remove(0);
        }
        plans.push(plan);
    }

    pub fn get(&self, key: &PlanId) -> Option<PackagePlan> {
        self.lock().iter().find(|plan| &plan.id == key).cloned()
    }

    pub fn take(&self, key: &PlanId) -> Option<PackagePlan> {
        let mut plans = self.lock();
        let index = plans.iter().position(|plan| &plan.id == key)?;
        Some(plans.remove(index))
    }
}

/// Every port the environment workflows use. Each use case holds a clone.
#[derive(Clone)]
pub struct EnvironmentDeps {
    pub hardware: Arc<dyn HardwareProbe>,
    pub catalog: Arc<dyn ProfileCatalog>,
    pub tools: Arc<dyn ToolProvisioner>,
    pub python: Arc<dyn PythonProvisioner>,
    pub packages: Arc<dyn PackageInstaller>,
    pub verifier: Arc<dyn EnvVerifier>,
    pub source: Arc<dyn ComfySource>,
    pub envs: Arc<dyn EnvironmentRepository>,
    pub runs: Arc<dyn ProvisioningRunRepository>,
    pub activation: Arc<dyn ActivationStore>,
    pub manifests: Arc<dyn InstallManifestStore>,
    pub drafts: Arc<dyn WizardDraftStore>,
    pub root: Arc<dyn RootSwitcher>,
    pub gate: Arc<dyn MaintenanceGate>,
    pub runtime: Arc<dyn RuntimeMaintenance>,
    pub clock: Arc<dyn Clock>,
    pub ids: Arc<dyn IdGenerator>,
    pub active_runs: Arc<ActiveRuns>,
    pub plans: Arc<PlanStore>,
}

impl EnvironmentDeps {
    pub fn envs_dir(root: &Path) -> PathBuf {
        root.join("envs")
    }
}

/// True when two locations name the same place (case-insensitive on Windows).
pub fn same_location(left: &Path, right: &Path) -> bool {
    fn normalize(path: &Path) -> String {
        let text = path.to_string_lossy();
        let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
        let text = text.replace('\\', "/");
        let text = text.trim_end_matches('/').to_owned();
        if cfg!(windows) {
            text.to_lowercase()
        } else {
            text
        }
    }
    normalize(left) == normalize(right)
}
