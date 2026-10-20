//! A fake machine: one struct implements every environment port, records the calls in order
//! and supports failure injection.

use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
};

use async_trait::async_trait;
use domain::custom_nodes::{DependencyState, NodeId};
use domain::environment::{
    AcceleratorSource, AcceleratorSpec, ComputeCapability, CudaVariant, CudaVersion, DiskBudget,
    DistName, EnvId, GpuObservation, GpuProbe, HardwareObservation, OsKind, PROFILE_SCHEMA_VERSION,
    PinSet, ProfileStatus, PythonEnvironment, PythonVersion, SetupProfile, TorchBuild, TritonSpec,
    VerifyExpectations,
};

use crate::{
    comfy_runtime::{RuntimeBlock, RuntimeMaintenance},
    custom_nodes::{CommitDecision, NodeOutcome, OperationKind, PlanReview},
    environment::*,
    shared::{Clock, GateError, IdGenerator, MaintenanceGate, MaintenanceLease},
};

pub const ROOT: &str = "/install";

pub fn name(value: &str) -> DistName {
    DistName::new(value).unwrap()
}

pub fn profile() -> SetupProfile {
    SetupProfile {
        id: "win-py313-torch2141-cu130".into(),
        schema_version: PROFILE_SCHEMA_VERSION,
        revision: 1,
        label: "Python 3.13, PyTorch 2.14.1, CUDA 13.0".into(),
        status: ProfileStatus::Stable,
        recommended: true,
        os: OsKind::Windows,
        python: PythonVersion::new(3, 13, 16),
        torch: TorchBuild {
            torch: "2.14.1".into(),
            torchvision: "0.29.1".into(),
            torchaudio: None,
            cuda: CudaVariant::Cu130,
            index_url: "https://download.pytorch.org/whl/cu130".into(),
        },
        min_driver_cuda: CudaVersion::new(13, 0),
        min_cc: ComputeCapability::new(8, 0),
        torch_archs: [(7, 5), (8, 0), (8, 6), (9, 0), (10, 0), (12, 0)]
            .into_iter()
            .map(|(major, minor)| ComputeCapability::new(major, minor))
            .collect(),
        triton: TritonSpec {
            distribution: name("triton-windows"),
            specifier: ">=3.8,<3.9".into(),
        },
        accelerators: Vec::new(),
        constraints: Vec::new(),
        verify: VerifyExpectations {
            torch_version_prefix: "2.14.1+cu130".into(),
            require_cuda: true,
            require_triton: true,
        },
        disk: DiskBudget {
            env_bytes: 16 << 30,
            other_bytes: 8 << 30,
            margin_bytes: 4 << 30,
            calibrated: false,
        },
        provenance: vec!["fixture".into()],
    }
}

pub fn curated_spec(id: &str) -> CuratedNodeSpec {
    CuratedNodeSpec {
        id: id.into(),
        name: format!("Node-{id}"),
        repository: format!("https://github.com/example/{id}.git"),
        provenance: "fixture".into(),
        unavailable: None,
    }
}

pub fn other_minor_profile() -> SetupProfile {
    let mut profile = profile();
    profile.id = "win-py312-torch2100-cu130".into();
    profile.python = PythonVersion::new(3, 12, 15);
    profile
}

pub fn healthy_hardware() -> HardwareObservation {
    HardwareObservation {
        gpus: GpuProbe::Detected(vec![GpuObservation {
            index: 0,
            name: "NVIDIA GeForce RTX 3060".into(),
            compute_capability: Some(ComputeCapability::new(8, 6)),
            vram_mib: Some(12288),
        }]),
        driver_version: Some("591.86".into()),
        driver_cuda: Some(CudaVersion::new(13, 1)),
        os: OsKind::Windows,
        os_version: "Windows 11".into(),
        install_volume_free: Some(200 << 30),
        env_volume_free: None,
        env_on_separate_volume: false,
        long_paths_enabled: Some(true),
    }
}

pub fn package(name: &str, version: &str) -> InstalledPackage {
    InstalledPackage {
        name: name.into(),
        version: version.into(),
    }
}

/// Holds the gate flag and counts acquisitions.
#[derive(Default)]
pub struct FakeGate {
    pub busy: Arc<AtomicBool>,
    pub acquires: AtomicUsize,
    pub try_acquires: AtomicUsize,
}

struct FakeLease(Arc<FakeLeaseOwner>);
struct FakeLeaseOwner(Arc<AtomicBool>);

impl MaintenanceLease for FakeLease {
    fn retained(&self) -> crate::shared::Retained {
        crate::shared::Retained::new(self.0.clone())
    }
}

impl Drop for FakeLeaseOwner {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

impl FakeGate {
    fn take(&self) -> Result<Box<dyn MaintenanceLease>, GateError> {
        if self.busy.swap(true, Ordering::SeqCst) {
            return Err(GateError::Busy);
        }
        Ok(Box::new(FakeLease(Arc::new(FakeLeaseOwner(
            self.busy.clone(),
        )))))
    }
}

#[test]
fn fake_lease_retention_delays_release() {
    let gate = FakeGate::default();
    let lease = gate.take().unwrap();
    let retained = lease.retained();
    drop(lease);
    assert!(gate.busy.load(Ordering::SeqCst));
    assert_eq!(gate.take().err(), Some(GateError::Busy));
    drop(retained);
    assert!(!gate.busy.load(Ordering::SeqCst));
}

#[async_trait]
impl MaintenanceGate for FakeGate {
    async fn acquire(&self) -> Result<Box<dyn MaintenanceLease>, GateError> {
        self.acquires.fetch_add(1, Ordering::SeqCst);
        self.take()
    }

    async fn try_acquire(&self) -> Result<Box<dyn MaintenanceLease>, GateError> {
        self.try_acquires.fetch_add(1, Ordering::SeqCst);
        self.take()
    }
}

pub struct World {
    pub calls: Mutex<Vec<String>>,
    failures: Mutex<HashMap<String, VecDeque<ProvisionError>>>,
    repo_failures: Mutex<HashMap<String, usize>>,
    skips: Mutex<HashMap<String, usize>>,
    pub installed: Mutex<Vec<InstalledPackage>>,
    pub envs: Mutex<BTreeMap<String, PythonEnvironment>>,
    pub components: Mutex<BTreeMap<String, InstalledComponents>>,
    pub drift_on_accelerator: AtomicBool,
    pub drift_on_node: AtomicBool,
    pub drift_on_final_verify: AtomicBool,
    pub drift_before_final_verify: AtomicBool,
    pub refuse_pin_restore: AtomicBool,
    pub active: Mutex<Option<EnvId>>,
    pub runs: Mutex<BTreeMap<String, ProvisioningRun>>,
    pub manifest: Mutex<ManifestState>,
    pub hardware: Mutex<HardwareObservation>,
    pub validation: Mutex<Option<PythonValidationReport>>,
    pub venv_version: Mutex<PythonVersion>,
    pub checkout: Mutex<Checkout>,
    pub node_requirements: Mutex<Vec<PathBuf>>,
    pub node_outcomes: Mutex<BTreeMap<(String, String, String), DependencyState>>,
    pub node_scan_retained: Mutex<Vec<bool>>,
    pub node_install_retained: Mutex<Vec<(bool, bool)>>,
    pub previews: Mutex<VecDeque<Result<ResolutionPreview, ProvisionError>>>,
    node_failures: Mutex<HashMap<String, VecDeque<SetupNodeError>>>,
    pub node_reviews: Mutex<BTreeMap<String, PlanReview>>,
    pub node_installed: Mutex<Vec<String>>,
    pub runtime_block: Mutex<Option<RuntimeBlock>>,
    pub gate: Arc<FakeGate>,
    pub drift_on_requirements: AtomicBool,
    pub drift_on_node_requirements: AtomicBool,
    pub gate_busy_at_cancel_cleanup: Mutex<Option<bool>>,
    pub installs: Mutex<Vec<InstallSpec>>,
    pub preview_specs: Mutex<Vec<InstallSpec>>,
    /// The environment path of every package installation, in order.
    pub install_envs: Mutex<Vec<PathBuf>>,
    pub constraint_writes: Mutex<Vec<PinSet>>,
    pub constraint_extras: Mutex<Vec<Vec<String>>>,
    pub draft: Mutex<Option<String>>,
    pub resolve_calls: AtomicUsize,
    pub verify_final_fails: AtomicBool,
    /// Simulates a missing download tool venv, which Repair must notice.
    pub download_tools_missing: AtomicBool,
    /// Versions applied after every successful install, simulating the resolver result.
    pub install_overrides: Mutex<Vec<(String, String)>>,
    counter: AtomicU64,
    pub profiles: Mutex<Vec<SetupProfile>>,
    pub curated: Mutex<Vec<CuratedNodeSpec>>,
}

impl World {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            calls: Mutex::default(),
            failures: Mutex::default(),
            repo_failures: Mutex::default(),
            skips: Mutex::default(),
            installed: Mutex::default(),
            envs: Mutex::default(),
            components: Mutex::default(),
            drift_on_accelerator: AtomicBool::new(false),
            drift_on_node: AtomicBool::new(false),
            drift_on_final_verify: AtomicBool::new(false),
            drift_before_final_verify: AtomicBool::new(false),
            refuse_pin_restore: AtomicBool::new(false),
            active: Mutex::default(),
            runs: Mutex::default(),
            manifest: Mutex::new(ManifestState::Absent),
            hardware: Mutex::new(healthy_hardware()),
            validation: Mutex::default(),
            venv_version: Mutex::new(PythonVersion::new(3, 13, 16)),
            checkout: Mutex::new(Checkout::Absent),
            node_requirements: Mutex::default(),
            node_outcomes: Mutex::default(),
            node_scan_retained: Mutex::default(),
            node_install_retained: Mutex::default(),
            previews: Mutex::default(),
            node_failures: Mutex::default(),
            node_reviews: Mutex::default(),
            node_installed: Mutex::default(),
            runtime_block: Mutex::default(),
            gate: Arc::new(FakeGate::default()),
            drift_on_requirements: AtomicBool::new(false),
            drift_on_node_requirements: AtomicBool::new(false),
            gate_busy_at_cancel_cleanup: Mutex::default(),
            installs: Mutex::default(),
            preview_specs: Mutex::default(),
            install_envs: Mutex::default(),
            constraint_writes: Mutex::default(),
            constraint_extras: Mutex::default(),
            draft: Mutex::default(),
            resolve_calls: AtomicUsize::new(0),
            verify_final_fails: AtomicBool::new(false),
            download_tools_missing: AtomicBool::new(false),
            install_overrides: Mutex::default(),
            counter: AtomicU64::new(0),
            profiles: Mutex::new(vec![profile(), other_minor_profile()]),
            curated: Mutex::new(vec![curated_spec("alpha"), curated_spec("beta")]),
        })
    }

    pub fn nodes(self: &Arc<Self>) -> Arc<dyn SetupNodes> {
        self.clone()
    }

    pub fn deps(self: &Arc<Self>) -> EnvironmentDeps {
        EnvironmentDeps {
            hardware: self.clone(),
            catalog: self.clone(),
            curated: self.clone(),
            tools: self.clone(),
            python: self.clone(),
            packages: self.clone(),
            wheels: self.clone(),
            verifier: self.clone(),
            source: self.clone(),
            node_requirements: self.clone(),
            envs: self.clone(),
            runs: self.clone(),
            activation: self.clone(),
            manifests: self.clone(),
            drafts: self.clone(),
            root: self.clone(),
            gate: self.gate.clone(),
            runtime: self.clone(),
            clock: self.clone(),
            ids: self.clone(),
            active_runs: Arc::new(ActiveRuns::default()),
            plans: Arc::new(PlanStore::default()),
            installation: self.clone(),
            relocation: self.clone(),
            relocation_runtime: self.clone(),
        }
    }

    pub fn call(&self, name: &str) {
        self.calls.lock().unwrap().push(name.to_owned());
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    pub fn position(&self, name: &str) -> usize {
        self.calls()
            .iter()
            .position(|call| call == name)
            .unwrap_or_else(|| panic!("`{name}` was never called: {:?}", self.calls()))
    }

    pub fn count(&self, name: &str) -> usize {
        self.calls().iter().filter(|call| *call == name).count()
    }

    pub fn fail(&self, operation: &str, error: ProvisionError) {
        self.failures
            .lock()
            .unwrap()
            .entry(operation.to_owned())
            .or_default()
            .push_back(error);
    }

    pub fn fail_repo(&self, operation: &str) {
        *self
            .repo_failures
            .lock()
            .unwrap()
            .entry(operation.to_owned())
            .or_default() += 1;
    }

    /// The first `count` calls of `operation` succeed before injected failures apply.
    pub fn succeed_first(&self, operation: &str, count: usize) {
        self.skips
            .lock()
            .unwrap()
            .insert(operation.to_owned(), count);
    }

    fn injected(&self, operation: &str) -> Result<(), ProvisionError> {
        if let Some(remaining) = self.skips.lock().unwrap().get_mut(operation)
            && *remaining > 0
        {
            *remaining -= 1;
            return Ok(());
        }
        match self
            .failures
            .lock()
            .unwrap()
            .get_mut(operation)
            .and_then(VecDeque::pop_front)
        {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn repo_injected(&self, operation: &str) -> Result<(), EnvRepoError> {
        if let Some(remaining) = self.skips.lock().unwrap().get_mut(operation)
            && *remaining > 0
        {
            *remaining -= 1;
            return Ok(());
        }
        let mut failures = self.repo_failures.lock().unwrap();
        match failures.get_mut(operation) {
            Some(remaining) if *remaining > 0 => {
                *remaining -= 1;
                Err(EnvRepoError::Unavailable)
            }
            _ => Ok(()),
        }
    }

    pub fn set_installed(&self, packages: &[(&str, &str)]) {
        *self.installed.lock().unwrap() = packages
            .iter()
            .map(|(name, version)| package(name, version))
            .collect();
    }

    pub fn put_installed(&self, name: &str, version: &str) {
        let mut installed = self.installed.lock().unwrap();
        installed.retain(|package| package.name != name);
        installed.push(package(name, version));
    }

    pub fn active_id(&self) -> Option<String> {
        self.active
            .lock()
            .unwrap()
            .as_ref()
            .map(|id| id.as_str().to_owned())
    }

    pub fn env(&self, id: &str) -> PythonEnvironment {
        self.envs
            .lock()
            .unwrap()
            .get(id)
            .unwrap_or_else(|| panic!("unknown env {id}"))
            .clone()
    }

    pub fn run(&self, id: &RunId) -> ProvisioningRun {
        self.runs.lock().unwrap().get(id.as_str()).unwrap().clone()
    }

    pub fn node_outcome(&self, environment: &EnvId, node: &str) -> Option<DependencyState> {
        self.node_outcomes
            .lock()
            .unwrap()
            .get(&(
                environment.as_str().to_owned(),
                node.to_owned(),
                format!("current:{node}"),
            ))
            .copied()
    }

    pub fn seed_node_outcome(&self, environment: &EnvId, node: &str, state: DependencyState) {
        self.node_outcomes.lock().unwrap().insert(
            (
                environment.as_str().to_owned(),
                node.to_owned(),
                format!("current:{node}"),
            ),
            state,
        );
    }

    pub fn seed_active_env(&self, id: &str, version: PythonVersion) -> PythonEnvironment {
        use domain::environment::{NewEnvironment, Ownership, PythonSourceKind};
        let mut env = PythonEnvironment::provisioning(NewEnvironment {
            id: EnvId::new(id).unwrap(),
            path: PathBuf::from(ROOT).join("envs").join(id),
            ownership: Ownership::AppOwned,
            source: PythonSourceKind::Managed,
            base_interpreter: None,
            python_version: version,
            profile_id: "win-py313-torch2141-cu130".into(),
            profile_revision: 1,
            authorized_location: PathBuf::from(ROOT).join("envs").join(id),
            now: "2026-10-15T00:00:00Z".into(),
        });
        let mut pins = PinSet::new();
        pins.insert(name("torch"), "2.14.1+cu130");
        pins.insert(name("torchvision"), "0.29.1+cu130");
        pins.insert(name("triton-windows"), "3.8.0.post29");
        env.mark_verified(pins, "seeded".into(), "2026-10-15T00:00:01Z")
            .unwrap();
        self.envs.lock().unwrap().insert(id.to_owned(), env.clone());
        *self.active.lock().unwrap() = Some(env.id.clone());
        self.set_installed(&[
            ("torch", "2.14.1+cu130"),
            ("torchvision", "0.29.1+cu130"),
            ("triton-windows", "3.8.0.post29"),
            ("numpy", "2.2.0"),
        ]);
        env
    }
}

#[async_trait]
impl InstallationProbe for World {
    async fn inspect(
        &self,
        root: &Path,
        manifest: &InstallManifest,
        _: &PythonEnvironment,
    ) -> Result<InstallationHealth, ProvisionError> {
        Ok(if same_location(root, &manifest.install_root) {
            InstallationHealth::Healthy
        } else {
            InstallationHealth::RebuildRequired(RebuildReason::Moved)
        })
    }
}

#[async_trait]
impl RelocationStore for World {
    async fn accept(&self, _: &Path, _: &Path) -> Result<(), RelocationError> {
        self.call("relocation.accept");
        Ok(())
    }
}

// ---------------------------------------------------------------- clock and ids

impl Clock for World {
    fn now(&self) -> String {
        format!(
            "2026-10-16T00:00:{:02}Z",
            self.counter.fetch_add(1, Ordering::SeqCst) % 60
        )
    }
}

impl IdGenerator for World {
    fn new_id(&self) -> String {
        format!(
            "{:08x}",
            self.counter.fetch_add(1, Ordering::SeqCst) + 0x1000
        )
    }
}

// ---------------------------------------------------------------- hardware and catalog

#[async_trait]
impl HardwareProbe for World {
    async fn probe(&self, _root: &Path, _env: Option<&Path>) -> HardwareObservation {
        self.call("probe");
        self.hardware.lock().unwrap().clone()
    }
}

impl ProfileCatalog for World {
    fn list(&self) -> Vec<SetupProfile> {
        self.profiles.lock().unwrap().clone()
    }

    fn get(&self, id: &str) -> Option<SetupProfile> {
        self.profiles
            .lock()
            .unwrap()
            .iter()
            .find(|profile| profile.id == id)
            .cloned()
    }
}

impl CuratedNodeCatalog for World {
    fn list(&self) -> Vec<CuratedNodeSpec> {
        self.curated.lock().unwrap().clone()
    }
}

impl World {
    /// Scripts the next failure of a `SetupNodes` method (`prepare`, `commit` or `installed`).
    pub fn fail_nodes(&self, method: &str, error: SetupNodeError) {
        self.node_failures
            .lock()
            .unwrap()
            .entry(method.to_owned())
            .or_default()
            .push_back(error);
    }

    fn node_injected(&self, method: &str) -> Result<(), SetupNodeError> {
        match self
            .node_failures
            .lock()
            .unwrap()
            .get_mut(method)
            .and_then(VecDeque::pop_front)
        {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

/// A scripted node service for runs that must not depend on the real node workflow.
#[async_trait]
impl SetupNodes for World {
    async fn prepare(
        &self,
        _run: &RunId,
        entry: &CuratedNodeSpec,
        _ctx: &StepContext,
        _lease: &mut dyn MaintenanceLease,
    ) -> Result<PlanReview, SetupNodeError> {
        self.call(&format!("setup_nodes:prepare:{}", entry.id));
        self.node_injected("prepare")?;
        let id = format!("node-{}-{}", entry.id, self.new_id());
        let review = PlanReview {
            id: id.clone(),
            binding: format!("binding-{}", self.new_id()),
            kind: OperationKind::Install,
            source: domain::custom_nodes::NodeSource::Git {
                repository: entry.repository.trim_end_matches(".git").to_owned(),
                commit: "a".repeat(40),
                branch: Some("main".into()),
            },
            name: entry.name.clone(),
            prior_source: None,
            requirements: Vec::new(),
            artifact_digest: None,
            tree_digest: "tree".into(),
            requirements_digest: None,
            hook_digest: None,
            environment: EnvId::new("env-fixture").unwrap(),
            pins: PinSet::new(),
            protected_changes: Vec::new(),
            warnings: Vec::new(),
            trusted_source: false,
            trusted_install_py: false,
            has_requirements: false,
            has_install_py: false,
        };
        self.node_reviews.lock().unwrap().insert(id, review.clone());
        Ok(review)
    }

    async fn commit(
        &self,
        _run: &RunId,
        plan: &str,
        decision: CommitDecision,
        _ctx: &StepContext,
        _lease: &mut dyn MaintenanceLease,
    ) -> Result<NodeOutcome, SetupNodeError> {
        self.call(&format!("setup_nodes:commit:{plan}"));
        self.node_injected("commit")?;
        let review = self
            .node_reviews
            .lock()
            .unwrap()
            .get(plan)
            .cloned()
            .ok_or(SetupNodeError::PlanStale)?;
        if decision.consent.binding != review.binding {
            return Err(SetupNodeError::ConsentRequired);
        }
        self.node_reviews.lock().unwrap().remove(plan);
        let node = domain::custom_nodes::InstalledNode {
            id: domain::custom_nodes::NodeId::new(plan).unwrap(),
            name: domain::shared::model_values::SafeFileName::new(review.name.as_str()).unwrap(),
            source: review.source,
            origin: domain::custom_nodes::ProvenanceOrigin::App,
            enabled: true,
            single_file: false,
            tree_digest: review.tree_digest,
            dependencies: DependencyState::Ok,
            conflict: false,
            dirty: None,
        };
        self.node_installed.lock().unwrap().push(review.name);
        Ok(NodeOutcome {
            operation: node.id.clone(),
            node,
            cleanup_pending: false,
        })
    }

    fn pending(&self, _run: &RunId) -> Result<Vec<PlanReview>, SetupNodeError> {
        Ok(self
            .node_reviews
            .lock()
            .unwrap()
            .values()
            .cloned()
            .collect())
    }

    async fn publication_started(&self, _plan: &str) -> bool {
        // This fake commits atomically; published entries are always recorded as Installed.
        false
    }

    async fn installed(
        &self,
        _run: &RunId,
        entry: &CuratedNodeSpec,
        _lease: &mut dyn MaintenanceLease,
    ) -> Result<bool, SetupNodeError> {
        self.call(&format!("setup_nodes:installed:{}", entry.id));
        self.node_injected("installed")?;
        Ok(self.node_installed.lock().unwrap().contains(&entry.name))
    }
}

#[async_trait]
impl RuntimeMaintenance for World {
    async fn ensure_stopped(&self, _root: &Path) -> Result<(), RuntimeBlock> {
        self.call("ensure_stopped");
        match *self.runtime_block.lock().unwrap() {
            Some(block) => Err(block),
            None => Ok(()),
        }
    }
}

// ---------------------------------------------------------------- tools, python, packages

#[async_trait]
impl ToolProvisioner for World {
    async fn ensure_uv(
        &self,
        _root: &Path,
        _ctx: &StepContext,
    ) -> Result<ToolInfo, ProvisionError> {
        self.call("ensure_uv");
        self.injected("ensure_uv")?;
        Ok(ToolInfo {
            name: "uv".into(),
            version: "0.13.1".into(),
        })
    }

    async fn ensure_git(
        &self,
        _root: &Path,
        _ctx: &StepContext,
    ) -> Result<ToolInfo, ProvisionError> {
        self.call("ensure_git");
        self.injected("ensure_git")?;
        Ok(ToolInfo {
            name: "git".into(),
            version: "2.55.0".into(),
        })
    }

    async fn ensure_hf_tool(
        &self,
        _root: &Path,
        _ctx: &StepContext,
    ) -> Result<ToolInfo, ProvisionError> {
        self.call("ensure_hf_tool");
        self.injected("ensure_hf_tool")?;
        Ok(ToolInfo {
            name: "hf".into(),
            version: "2.2.0".into(),
        })
    }

    async fn recreate_hf_tool(
        &self,
        root: &Path,
        ctx: &StepContext,
    ) -> Result<ToolInfo, ProvisionError> {
        self.call("recreate_hf_tool");
        self.ensure_hf_tool(root, ctx).await
    }

    async fn ensure_gdown_tool(
        &self,
        _root: &Path,
        _ctx: &StepContext,
    ) -> Result<ToolInfo, ProvisionError> {
        self.call("ensure_gdown_tool");
        self.injected("ensure_gdown_tool")?;
        Ok(ToolInfo {
            name: "gdown".into(),
            version: "6.4.2".into(),
        })
    }

    async fn recreate_gdown_tool(
        &self,
        root: &Path,
        ctx: &StepContext,
    ) -> Result<ToolInfo, ProvisionError> {
        self.call("recreate_gdown_tool");
        self.ensure_gdown_tool(root, ctx).await
    }

    fn download_tools_ready(&self, _root: &Path) -> bool {
        !self.download_tools_missing.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl PythonProvisioner for World {
    async fn install_managed_python(
        &self,
        _root: &Path,
        version: PythonVersion,
        _ctx: &StepContext,
    ) -> Result<PathBuf, ProvisionError> {
        self.call("install_managed_python");
        self.call(&format!("install_managed_python_version:{version}"));
        self.injected("install_managed_python")?;
        Ok(PathBuf::from("/managed/python"))
    }

    async fn create_venv(
        &self,
        _root: &Path,
        interpreter: &Path,
        target: &Path,
        _ctx: &StepContext,
    ) -> Result<VenvInfo, ProvisionError> {
        self.call("create_venv");
        self.call(&format!("create_venv_from:{}", interpreter.display()));
        self.call(&format!("create_venv_at:{}", target.display()));
        self.injected("create_venv")?;
        Ok(VenvInfo {
            version: *self.venv_version.lock().unwrap(),
            base_interpreter: Some(interpreter.to_path_buf()),
        })
    }

    async fn inspect_venv(&self, _root: &Path, _venv: &Path) -> Result<VenvInfo, ProvisionError> {
        self.call("inspect_venv");
        Ok(VenvInfo {
            version: *self.venv_version.lock().unwrap(),
            base_interpreter: None,
        })
    }

    async fn validate(
        &self,
        _root: &Path,
        _source: &PythonSourceInput,
    ) -> Result<PythonValidationReport, ProvisionError> {
        self.call("validate");
        self.validation
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| ProvisionError::Failed("no validation scripted".into()))
    }

    async fn seed(
        &self,
        _root: &Path,
        _venv: &Path,
        _os: OsKind,
        _ctx: &StepContext,
    ) -> Result<(), ProvisionError> {
        self.call("seed");
        self.injected("seed")
    }

    async fn remove_environment(&self, env: &PythonEnvironment) -> Result<(), ProvisionError> {
        self.call(&format!("remove_environment:{}", env.id));
        self.injected("remove_environment")
    }
}

impl World {
    fn install_effects(&self, spec: &InstallSpec) {
        for requirement in &spec.requirements {
            if requirement.starts_with("torch==") {
                self.put_installed("torch", "2.14.1+cu130");
            } else if requirement.starts_with("torchvision==") {
                self.put_installed("torchvision", "0.29.1+cu130");
            } else if requirement.starts_with("triton-windows") {
                self.put_installed("triton-windows", "3.8.0.post29");
            } else {
                let package = requirement
                    .split(['=', '<', '>', ' '])
                    .next()
                    .unwrap_or(requirement);
                self.put_installed(package, "1.0.0");
            }
        }
        let overrides = self.install_overrides.lock().unwrap().clone();
        for (name, version) in overrides {
            self.put_installed(&name, &version);
        }
        if !spec.requirement_files.is_empty() {
            self.put_installed("numpy", "2.2.0");
            if self.drift_on_requirements.load(Ordering::SeqCst) {
                self.put_installed("torch", "2.15.0");
            }
        }
    }

    fn install_key(spec: &InstallSpec) -> &'static str {
        if spec.requirements.iter().any(|r| r.starts_with("torch==")) {
            "install:torch"
        } else if spec
            .requirements
            .iter()
            .any(|r| r.starts_with("triton-windows"))
        {
            "install:triton"
        } else if spec
            .requirement_files
            .iter()
            .any(|file| file.to_string_lossy().contains("custom_nodes"))
        {
            "install:node"
        } else if !spec.requirement_files.is_empty() {
            "install:requirements"
        } else {
            "install:other"
        }
    }
}

#[async_trait]
impl PackageInstaller for World {
    async fn install(
        &self,
        _root: &Path,
        env_path: &Path,
        spec: &InstallSpec,
        ctx: &StepContext,
    ) -> Result<(), ProvisionError> {
        let key = Self::install_key(spec);
        self.call(key);
        self.install_envs
            .lock()
            .unwrap()
            .push(env_path.to_path_buf());
        if key == "install:node" {
            self.node_install_retained.lock().unwrap().push((
                !ctx.retained.is_empty(),
                spec.guarded
                    .as_ref()
                    .is_some_and(|file| !file.retained.is_empty()),
            ));
        }
        let mut recorded = spec.clone();
        if let Some(file) = recorded.guarded.as_mut() {
            file.retained = Retained::default();
        }
        self.installs.lock().unwrap().push(recorded);
        if spec.no_deps && spec.requirements.iter().any(|r| r.contains("==")) {
            self.call("restore_pins");
            assert!(!ctx.cancel.is_cancelled(), "cleanup uses a fresh token");
            assert!(
                self.gate.busy.load(Ordering::SeqCst),
                "cleanup retains the lease"
            );
            if self.refuse_pin_restore.load(Ordering::SeqCst) {
                return Err(ProvisionError::Failed("restore refused".into()));
            }
            for requirement in &spec.requirements {
                if let Some((name, version)) = requirement.split_once("==") {
                    self.put_installed(name, version);
                }
            }
            return Ok(());
        }
        if (key == "install:other" && self.drift_on_accelerator.load(Ordering::SeqCst))
            || (key == "install:node" && self.drift_on_node.load(Ordering::SeqCst))
        {
            self.put_installed("torch", "99.0.0");
        }
        if let Err(error) = self.injected(key) {
            if error == ProvisionError::Cancelled {
                ctx.cancel.cancel();
                *self.gate_busy_at_cancel_cleanup.lock().unwrap() =
                    Some(self.gate.busy.load(Ordering::SeqCst));
            }
            return Err(error);
        }
        self.install_effects(spec);
        if key == "install:node" && self.drift_on_node_requirements.load(Ordering::SeqCst) {
            self.put_installed("torch", "2.15.0");
        }
        Ok(())
    }

    async fn uninstall(
        &self,
        _root: &Path,
        _env_path: &Path,
        names: &[DistName],
        _ctx: &StepContext,
    ) -> Result<(), ProvisionError> {
        self.call("uninstall");
        self.injected("uninstall")?;
        self.installed
            .lock()
            .unwrap()
            .retain(|package| !names.iter().any(|name| name.as_str() == package.name));
        Ok(())
    }

    async fn preview_install(
        &self,
        _root: &Path,
        _env_path: &Path,
        spec: &InstallSpec,
        _ctx: &StepContext,
    ) -> Result<ResolutionPreview, ProvisionError> {
        let mut recorded = spec.clone();
        if let Some(file) = recorded.guarded.as_mut() {
            file.retained = Retained::default();
        }
        self.preview_specs.lock().unwrap().push(recorded);
        self.call(if spec.constraint_file.is_some() {
            "preview:constrained"
        } else {
            "preview:unconstrained"
        });
        self.previews
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Ok(ResolutionPreview::default()))
    }

    async fn list(
        &self,
        _root: &Path,
        _env_path: &Path,
        ctx: Option<&StepContext>,
    ) -> Result<Vec<InstalledPackage>, ProvisionError> {
        self.call("list");
        // Like the real adapter, a cancelled step cannot run `uv pip list`.
        if ctx.is_some_and(|ctx| ctx.cancel.is_cancelled()) {
            return Err(ProvisionError::Cancelled);
        }
        self.injected("list")?;
        Ok(self.installed.lock().unwrap().clone())
    }

    async fn write_constraints(
        &self,
        _root: &Path,
        env: &EnvId,
        pins: &PinSet,
        extra: &[String],
    ) -> Result<PathBuf, ProvisionError> {
        self.call("write_constraints");
        self.constraint_writes.lock().unwrap().push(pins.clone());
        self.constraint_extras.lock().unwrap().push(extra.to_vec());
        Ok(PathBuf::from(format!(
            "/install/data/cache/constraints-{env}.txt"
        )))
    }

    async fn clean_cache(&self, _root: &Path, _ctx: &StepContext) -> Result<(), ProvisionError> {
        self.call("clean_cache");
        self.injected("clean_cache")
    }
}

#[async_trait]
impl EnvVerifier for World {
    async fn verify_accelerator(
        &self,
        _root: &Path,
        _env_path: &Path,
        module: &str,
        _ctx: &StepContext,
    ) -> Result<(), ProvisionError> {
        self.call(&format!("import:{module}"));
        self.injected("verify_accelerator")
    }

    async fn verify_torch(
        &self,
        _root: &Path,
        _env_path: &Path,
        _expectations: &domain::environment::VerifyExpectations,
        _ctx: &StepContext,
    ) -> Result<TorchReport, ProvisionError> {
        self.call("verify_torch");
        self.injected("verify_torch")?;
        Ok(TorchReport {
            version: "2.14.1+cu130".into(),
            cuda_available: true,
        })
    }

    async fn verify_triton(
        &self,
        _root: &Path,
        _env_path: &Path,
        _ctx: &StepContext,
    ) -> Result<String, ProvisionError> {
        self.call("verify_triton");
        self.injected("verify_triton")?;
        Ok("3.8.0".into())
    }

    async fn verify_final(
        &self,
        _root: &Path,
        _env_path: &Path,
        _expectations: &domain::environment::VerifyExpectations,
        _ctx: &StepContext,
    ) -> Result<VerificationReport, ProvisionError> {
        self.call("verify_final");
        if self.drift_on_final_verify.load(Ordering::SeqCst) {
            self.put_installed("torch", "99.0.0");
        }
        self.injected("verify_final")?;
        if self.verify_final_fails.load(Ordering::SeqCst) {
            return Err(ProvisionError::Failed("verification failed".into()));
        }
        Ok(VerificationReport {
            python_version: "3.13.16".into(),
            torch_version: "2.14.1+cu130".into(),
            cuda_available: true,
            cuda_device: Some("RTX 3060".into()),
            triton_version: Some("3.8.0".into()),
            comfy_path: Some("ComfyUI/comfy".into()),
            accelerators: vec![("sageattention".into(), false)],
        })
    }
}

#[async_trait]
impl NodeRequirementSource for World {
    async fn requirement_sets(
        &self,
        _root: &Path,
        retained: Retained,
    ) -> Result<Vec<NodeRequirementSet>, ProvisionError> {
        self.call("node_requirement_sets");
        self.node_scan_retained
            .lock()
            .unwrap()
            .push(!retained.is_empty());
        self.injected("node_requirement_sets")?;
        Ok(self
            .node_requirements
            .lock()
            .unwrap()
            .iter()
            .map(|path| {
                let directory = path.parent().unwrap().to_path_buf();
                let name = directory.file_name().unwrap().to_str().unwrap();
                NodeRequirementSet {
                    node_id: NodeId::new(name).unwrap(),
                    name: domain::shared::model_values::SafeFileName::new(name).unwrap(),
                    source_identity: format!("current:{name}"),
                    file: Some(GuardedRequirements {
                        path: path.clone(),
                        directory,
                        text: Arc::from(b"fixture-dependency==1.0.0\n".as_slice()),
                        retained: retained.clone().merge(Retained::new(Arc::new(()))),
                    }),
                    extra: Vec::new(),
                }
            })
            .collect())
    }

    async fn record_outcome(
        &self,
        environment: &EnvId,
        set: &NodeRequirementSet,
        state: DependencyState,
    ) -> Result<(), ProvisionError> {
        self.call(&format!("node_outcome:{}:{state:?}", set.node_id.as_str()));
        self.injected("record_node_outcome")?;
        self.node_outcomes.lock().unwrap().insert(
            (
                environment.as_str().to_owned(),
                set.node_id.as_str().to_owned(),
                set.source_identity.clone(),
            ),
            state,
        );
        Ok(())
    }

    async fn needs_repair(
        &self,
        environment: &EnvId,
        root: &Path,
        retained: Retained,
    ) -> Result<bool, ProvisionError> {
        self.call("node_needs_repair");
        let sets = self.requirement_sets(root, retained).await?;
        let outcomes = self.node_outcomes.lock().unwrap();
        Ok(sets.iter().any(|set| {
            outcomes.get(&(
                environment.as_str().to_owned(),
                set.node_id.as_str().to_owned(),
                set.source_identity.clone(),
            )) != Some(&DependencyState::Ok)
        }))
    }
}

#[async_trait]
impl ComfySource for World {
    async fn resolve(
        &self,
        _root: &Path,
        reference: &ComfyRef,
    ) -> Result<ResolvedGit, ProvisionError> {
        self.call("resolve");
        self.resolve_calls.fetch_add(1, Ordering::SeqCst);
        self.injected("resolve")?;
        Ok(match reference {
            ComfyRef::Tag(tag) => ResolvedGit {
                url: "https://github.com/Comfy-Org/ComfyUI.git".into(),
                reference: tag.clone(),
                branch: None,
                commit: "c0ffee0000000000000000000000000000000001".into(),
            },
            _ => ResolvedGit {
                url: "https://github.com/Comfy-Org/ComfyUI.git".into(),
                reference: "v0.40.0".into(),
                branch: None,
                commit: "abc1230000000000000000000000000000000000".into(),
            },
        })
    }

    async fn inspect_checkout(&self, _root: &Path) -> Result<Checkout, ProvisionError> {
        self.call("inspect_checkout");
        Ok(self.checkout.lock().unwrap().clone())
    }

    async fn clone_comfyui(
        &self,
        _root: &Path,
        resolved: &ResolvedGit,
        _ctx: &StepContext,
    ) -> Result<(), ProvisionError> {
        self.call("clone_comfyui");
        self.injected("clone_comfyui")?;
        *self.checkout.lock().unwrap() = Checkout::Repo {
            commit: resolved.commit.clone(),
        };
        Ok(())
    }

    async fn requirement_files(&self, _root: &Path) -> Result<RequirementFiles, ProvisionError> {
        Ok(RequirementFiles {
            comfyui: PathBuf::from("/install/ComfyUI/requirements.txt"),
        })
    }

    async fn resolve_manager(&self, _root: &Path) -> Result<ResolvedGit, ProvisionError> {
        self.call("resolve_manager");
        Ok(ResolvedGit {
            url: "https://github.com/Comfy-Org/ComfyUI-Manager.git".into(),
            reference: "main".into(),
            branch: None,
            commit: "mgr0000000000000000000000000000000000001".into(),
        })
    }

    async fn install_manager(
        &self,
        _root: &Path,
        _resolved: &ResolvedGit,
        _ctx: &StepContext,
    ) -> Result<Vec<PathBuf>, ProvisionError> {
        self.call("install_manager");
        self.injected("install_manager")?;
        Ok(vec![PathBuf::from(
            "/install/ComfyUI/custom_nodes/ComfyUI-Manager/requirements.txt",
        )])
    }
}

// ---------------------------------------------------------------- persistence

#[async_trait]
impl EnvironmentRepository for World {
    async fn list(&self) -> Result<Vec<PythonEnvironment>, EnvRepoError> {
        Ok(self.envs.lock().unwrap().values().cloned().collect())
    }

    async fn get(&self, id: &EnvId) -> Result<Option<PythonEnvironment>, EnvRepoError> {
        Ok(self.envs.lock().unwrap().get(id.as_str()).cloned())
    }

    async fn active(&self) -> Result<Option<PythonEnvironment>, EnvRepoError> {
        let active = self.active.lock().unwrap().clone();
        Ok(active.and_then(|id| self.envs.lock().unwrap().get(id.as_str()).cloned()))
    }

    async fn save(&self, env: &PythonEnvironment) -> Result<(), EnvRepoError> {
        self.repo_injected("env_save")?;
        self.envs
            .lock()
            .unwrap()
            .insert(env.id.as_str().to_owned(), env.clone());
        Ok(())
    }

    async fn load_components(&self, id: &EnvId) -> Result<InstalledComponents, EnvRepoError> {
        Ok(self
            .components
            .lock()
            .unwrap()
            .get(id.as_str())
            .cloned()
            .unwrap_or_default())
    }
    async fn save_components(
        &self,
        id: &EnvId,
        components: &InstalledComponents,
    ) -> Result<(), EnvRepoError> {
        self.components
            .lock()
            .unwrap()
            .insert(id.to_string(), components.clone());
        Ok(())
    }
    async fn delete(&self, id: &EnvId) -> Result<(), EnvRepoError> {
        self.call(&format!("delete_env:{id}"));
        self.envs.lock().unwrap().remove(id.as_str());
        Ok(())
    }
}

#[async_trait]
impl ProvisioningRunRepository for World {
    async fn insert(&self, run: &ProvisioningRun) -> Result<(), EnvRepoError> {
        self.runs
            .lock()
            .unwrap()
            .insert(run.id.as_str().to_owned(), run.clone());
        Ok(())
    }

    async fn save(&self, run: &ProvisioningRun) -> Result<(), EnvRepoError> {
        if run.step(StepId::FinalVerify).status == StepStatus::Running
            && self.drift_before_final_verify.swap(false, Ordering::SeqCst)
        {
            self.put_installed("torch", "99.0.0");
        }
        let operation = format!("run_save:{}", run.activation.as_str());
        self.repo_injected(&operation)?;
        self.runs
            .lock()
            .unwrap()
            .insert(run.id.as_str().to_owned(), run.clone());
        Ok(())
    }

    async fn get(&self, id: &RunId) -> Result<Option<ProvisioningRun>, EnvRepoError> {
        Ok(self.runs.lock().unwrap().get(id.as_str()).cloned())
    }

    async fn latest(&self) -> Result<Option<ProvisioningRun>, EnvRepoError> {
        Ok(self
            .runs
            .lock()
            .unwrap()
            .values()
            .max_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)))
            .cloned())
    }

    async fn unfinished(&self) -> Result<Vec<ProvisioningRun>, EnvRepoError> {
        Ok(self
            .runs
            .lock()
            .unwrap()
            .values()
            .filter(|run| {
                run.status.is_active()
                    || run.activation.is_incomplete()
                    || run.status.is_retryable()
            })
            .cloned()
            .collect())
    }
}

#[async_trait]
impl ActivationStore for World {
    async fn commit_activation(
        &self,
        run: &ProvisioningRun,
        new_active: &EnvId,
    ) -> Result<(), EnvRepoError> {
        self.call("commit_activation");
        self.repo_injected("commit_activation")?;
        *self.active.lock().unwrap() = Some(new_active.clone());
        self.runs
            .lock()
            .unwrap()
            .insert(run.id.as_str().to_owned(), run.clone());
        Ok(())
    }
}

#[async_trait]
impl InstallManifestStore for World {
    async fn read(&self, _root: &Path) -> Result<ManifestState, ManifestError> {
        Ok(self.manifest.lock().unwrap().clone())
    }

    async fn publish(&self, manifest: &InstallManifest) -> Result<(), ManifestError> {
        self.call("publish_manifest");
        self.repo_injected("publish_manifest")
            .map_err(|_| ManifestError::WriteFailed)?;
        *self.manifest.lock().unwrap() = ManifestState::Valid(manifest.clone());
        Ok(())
    }
}

#[async_trait]
impl WizardDraftStore for World {
    async fn load(&self) -> Result<Option<String>, EnvRepoError> {
        Ok(self.draft.lock().unwrap().clone())
    }

    async fn save(&self, draft: &str) -> Result<(), EnvRepoError> {
        *self.draft.lock().unwrap() = Some(draft.to_owned());
        Ok(())
    }
}

#[async_trait]
impl RootSwitcher for World {
    fn current(&self) -> RootInfo {
        RootInfo {
            root: PathBuf::from(ROOT),
            source: RootSource::Default,
        }
    }

    async fn switch_root(
        &self,
        new_root: &Path,
        draft: Option<&str>,
    ) -> Result<(), RootSwitchError> {
        self.call(&format!("switch_root:{}", new_root.display()));
        *self.draft.lock().unwrap() = draft.map(str::to_owned);
        Ok(())
    }
}

/// An event sink that stores every event.
#[derive(Default)]
pub struct RecordingSink(pub Mutex<Vec<SetupEvent>>);

impl RecordingSink {
    /// The log lines in emission order.
    pub fn log_lines(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter_map(|event| match event {
                SetupEvent::Log { line, .. } => Some(line.clone()),
                _ => None,
            })
            .collect()
    }
}

impl SetupProgressSink for RecordingSink {
    fn emit(&self, event: SetupEvent) {
        self.0.lock().unwrap().push(event);
    }
}

#[async_trait]
impl WheelFetcher for World {
    async fn fetch(
        &self,
        _root: &Path,
        spec: &AcceleratorSpec,
        _ctx: &StepContext,
    ) -> Result<PathBuf, ProvisionError> {
        self.call("fetch_wheel");
        self.injected("fetch_wheel")?;
        let AcceleratorSource::Wheel { filename, .. } = &spec.source else {
            panic!("expected wheel")
        };
        Ok(PathBuf::from("/install/data/cache/wheels").join(filename.as_str()))
    }
}

pub fn accelerator(name: &str) -> AcceleratorSpec {
    AcceleratorSpec {
        id: name.into(),
        name: name.into(),
        modules: vec![name.replace('-', "_")],
        source: AcceleratorSource::Packages {
            requirements: vec![format!("{name}==1.0")],
        },
        no_deps: false,
        default_selected: false,
        non_fatal: true,
        provenance: "fixture; import unvalidated".into(),
    }
}
