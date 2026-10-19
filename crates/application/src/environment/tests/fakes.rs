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
use domain::environment::{
    AcceleratorSpec, ComputeCapability, CudaVariant, CudaVersion, DiskBudget, DistName, EnvId,
    GpuObservation, GpuProbe, HardwareObservation, OsKind, PROFILE_SCHEMA_VERSION, PinSet,
    ProfileStatus, PythonEnvironment, PythonVersion, SetupProfile, TorchBuild, TritonSpec,
    VerifyExpectations,
};

use crate::{
    comfy_runtime::{RuntimeBlock, RuntimeMaintenance},
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
    pub active: Mutex<Option<EnvId>>,
    pub runs: Mutex<BTreeMap<String, ProvisioningRun>>,
    pub manifest: Mutex<ManifestState>,
    pub hardware: Mutex<HardwareObservation>,
    pub validation: Mutex<Option<PythonValidationReport>>,
    pub venv_version: Mutex<PythonVersion>,
    pub checkout: Mutex<Checkout>,
    pub node_requirements: Mutex<Vec<PathBuf>>,
    pub previews: Mutex<VecDeque<Result<ResolutionPreview, ProvisionError>>>,
    pub runtime_block: Mutex<Option<RuntimeBlock>>,
    pub gate: Arc<FakeGate>,
    pub drift_on_requirements: AtomicBool,
    pub gate_busy_at_cancel_cleanup: Mutex<Option<bool>>,
    pub installs: Mutex<Vec<InstallSpec>>,
    pub constraint_writes: Mutex<Vec<PinSet>>,
    pub draft: Mutex<Option<String>>,
    pub resolve_calls: AtomicUsize,
    pub verify_final_fails: AtomicBool,
    /// Versions applied after every successful install, simulating the resolver result.
    pub install_overrides: Mutex<Vec<(String, String)>>,
    counter: AtomicU64,
    pub profiles: Mutex<Vec<SetupProfile>>,
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
            active: Mutex::default(),
            runs: Mutex::default(),
            manifest: Mutex::new(ManifestState::Absent),
            hardware: Mutex::new(healthy_hardware()),
            validation: Mutex::default(),
            venv_version: Mutex::new(PythonVersion::new(3, 13, 16)),
            checkout: Mutex::new(Checkout::Absent),
            node_requirements: Mutex::default(),
            previews: Mutex::default(),
            runtime_block: Mutex::default(),
            gate: Arc::new(FakeGate::default()),
            drift_on_requirements: AtomicBool::new(false),
            gate_busy_at_cancel_cleanup: Mutex::default(),
            installs: Mutex::default(),
            constraint_writes: Mutex::default(),
            draft: Mutex::default(),
            resolve_calls: AtomicUsize::new(0),
            verify_final_fails: AtomicBool::new(false),
            install_overrides: Mutex::default(),
            counter: AtomicU64::new(0),
            profiles: Mutex::new(vec![profile(), other_minor_profile()]),
        })
    }

    pub fn deps(self: &Arc<Self>) -> EnvironmentDeps {
        EnvironmentDeps {
            hardware: self.clone(),
            catalog: self.clone(),
            tools: self.clone(),
            python: self.clone(),
            packages: self.clone(),
            verifier: self.clone(),
            source: self.clone(),
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
}

#[async_trait]
impl PythonProvisioner for World {
    async fn install_managed_python(
        &self,
        _root: &Path,
        _version: PythonVersion,
        _ctx: &StepContext,
    ) -> Result<PathBuf, ProvisionError> {
        self.call("install_managed_python");
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
        _env_path: &Path,
        spec: &InstallSpec,
        ctx: &StepContext,
    ) -> Result<(), ProvisionError> {
        let key = Self::install_key(spec);
        self.call(key);
        self.installs.lock().unwrap().push(spec.clone());
        if let Err(error) = self.injected(key) {
            if error == ProvisionError::Cancelled {
                ctx.cancel.cancel();
                *self.gate_busy_at_cancel_cleanup.lock().unwrap() =
                    Some(self.gate.busy.load(Ordering::SeqCst));
            }
            return Err(error);
        }
        self.install_effects(spec);
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
        _ctx: Option<&StepContext>,
    ) -> Result<Vec<InstalledPackage>, ProvisionError> {
        self.call("list");
        self.injected("list")?;
        Ok(self.installed.lock().unwrap().clone())
    }

    async fn write_constraints(
        &self,
        _root: &Path,
        env: &EnvId,
        pins: &PinSet,
        _extra: &[String],
    ) -> Result<PathBuf, ProvisionError> {
        self.call("write_constraints");
        self.constraint_writes.lock().unwrap().push(pins.clone());
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
            nodes: self.node_requirements.lock().unwrap().clone(),
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

impl SetupProgressSink for RecordingSink {
    fn emit(&self, event: SetupEvent) {
        self.0.lock().unwrap().push(event);
    }
}

pub fn accelerator(name: &str) -> AcceleratorSpec {
    AcceleratorSpec {
        name: name.into(),
        requirement: format!("{name}==1.0"),
        sha256: Some("0".repeat(64)),
        non_fatal: true,
    }
}
