use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use super::{
    model::RunId,
    packages::{PackagePlan, PlanId},
    ports::{
        ActivationStore, ComfySource, CuratedNodeCatalog, EnvVerifier, EnvironmentRepository,
        HardwareProbe, InstallManifestStore, ManagerConfigWriter, NodeRequirementSource,
        PackageInstaller, ProfileCatalog, ProvisioningRunRepository, PythonProvisioner,
        RootSwitcher, ToolProvisioner, WheelFetcher, WizardDraftStore,
    },
};
use crate::{
    comfy_runtime::RuntimeMaintenance,
    custom_nodes::CommitDecision,
    shared::{CancelToken, Clock, IdGenerator, MaintenanceGate},
};

/// The in-process cancellation tokens of runs that are registered or executing, and the curated
/// node decisions a retry carries to the executor. Decisions are never persisted, so a restart
/// drops them together with the reviews they answer.
#[derive(Default)]
pub struct ActiveRuns {
    tokens: Mutex<HashMap<RunId, CancelToken>>,
    decisions: Mutex<HashMap<RunId, Vec<CommitDecision>>>,
}

impl ActiveRuns {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<RunId, CancelToken>> {
        self.tokens
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
        self.decisions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(id);
    }

    /// Replaces the curated decisions the next execution of `id` consumes.
    pub fn set_decisions(&self, id: &RunId, decisions: Vec<CommitDecision>) {
        let mut all = self
            .decisions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if decisions.is_empty() {
            all.remove(id);
        } else {
            all.insert(id.clone(), decisions);
        }
    }

    /// Hands the decisions to the executor exactly once.
    pub fn take_decisions(&self, id: &RunId) -> Vec<CommitDecision> {
        self.decisions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(id)
            .unwrap_or_default()
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
    pub curated: Arc<dyn CuratedNodeCatalog>,
    pub tools: Arc<dyn ToolProvisioner>,
    pub python: Arc<dyn PythonProvisioner>,
    pub packages: Arc<dyn PackageInstaller>,
    pub wheels: Arc<dyn WheelFetcher>,
    pub verifier: Arc<dyn EnvVerifier>,
    pub source: Arc<dyn ComfySource>,
    pub manager_config: Arc<dyn ManagerConfigWriter>,
    pub node_requirements: Arc<dyn NodeRequirementSource>,
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
    pub installation: Arc<dyn super::InstallationProbe>,
    pub relocation: Arc<dyn super::RelocationStore>,
    pub relocation_runtime: Arc<dyn RuntimeMaintenance>,
}

impl EnvironmentDeps {
    pub fn envs_dir(root: &Path) -> PathBuf {
        root.join("envs")
    }
}

/// True when two locations name the same place (case-insensitive on Windows).
pub fn same_location(left: &Path, right: &Path) -> bool {
    super::normalized_path(left) == super::normalized_path(right)
}
