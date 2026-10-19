//! The environment workflows over the real SQLite store, the real install manifest and the
//! real maintenance gate. Only the machine (tools, Python, packages, GPU, network) is simulated,
//! so "restart" means: drop every in-memory object, reopen the database file, recompose.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use application::{
    comfy_runtime::{RuntimeBlock, RuntimeMaintenance},
    environment::*,
    settings::SettingsRepository,
};
use async_trait::async_trait;
use domain::{
    environment::{
        ComputeCapability, CudaVersion, DistName, EnvId, EnvState, GpuObservation, GpuProbe,
        HardwareObservation, OsKind, PinSet, PythonEnvironment, PythonVersion, SetupMode,
        VerifyExpectations,
    },
    settings::{CloseBehavior, HfEngine, Settings, Theme},
};
use infrastructure::{
    environment::{
        manifest::FsManifestStore, profiles::BundledProfiles, python::PythonAdapter,
        sinks::run_sink, sqlite::SqliteEnvironmentStore, switcher::FsRootSwitcher,
    },
    settings::sqlite::SqliteSettingsRepository,
    shared::{
        clock::{RandomIds, SystemClock},
        gate::TokioMaintenanceGate,
        paths::{AppPaths, read_bootstrap},
        sqlite::Db,
    },
};

/// The simulated machine. It records calls and can fail an operation once.
#[derive(Default)]
struct Sim {
    calls: Mutex<Vec<String>>,
    failures: Mutex<HashMap<String, usize>>,
    installed: Mutex<Vec<InstalledPackage>>,
    checkout: Mutex<Option<String>>,
    venvs: Mutex<Vec<PathBuf>>,
}

impl Sim {
    fn fail_once(&self, operation: &str) {
        *self
            .failures
            .lock()
            .unwrap()
            .entry(operation.to_owned())
            .or_default() += 1;
    }

    fn step(&self, operation: &str) -> Result<(), ProvisionError> {
        self.calls.lock().unwrap().push(operation.to_owned());
        let mut failures = self.failures.lock().unwrap();
        match failures.get_mut(operation) {
            Some(remaining) if *remaining > 0 => {
                *remaining -= 1;
                Err(ProvisionError::Failed(format!("{operation} failed")))
            }
            _ => Ok(()),
        }
    }

    fn count(&self, operation: &str) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| *call == operation)
            .count()
    }

    fn put(&self, name: &str, version: &str) {
        let mut installed = self.installed.lock().unwrap();
        installed.retain(|package| package.name != name);
        installed.push(InstalledPackage {
            name: name.into(),
            version: version.into(),
        });
    }
}

const COMMIT: &str = "abc1230000000000000000000000000000000000";

#[async_trait]
impl HardwareProbe for Sim {
    async fn probe(&self, _: &Path, _: Option<&Path>) -> HardwareObservation {
        HardwareObservation {
            gpus: GpuProbe::Detected(vec![GpuObservation {
                index: 0,
                name: "RTX 3060".into(),
                compute_capability: Some(ComputeCapability::new(8, 6)),
                vram_mib: Some(12288),
            }]),
            driver_version: Some("591.86".into()),
            driver_cuda: Some(CudaVersion::new(13, 1)),
            os: if cfg!(windows) {
                OsKind::Windows
            } else {
                OsKind::Linux
            },
            os_version: "test".into(),
            install_volume_free: Some(1 << 40),
            env_volume_free: None,
            env_on_separate_volume: false,
            long_paths_enabled: Some(true),
        }
    }
}

#[async_trait]
impl ToolProvisioner for Sim {
    async fn ensure_uv(&self, _: &Path, _: &StepContext) -> Result<ToolInfo, ProvisionError> {
        self.step("ensure_uv")?;
        Ok(ToolInfo {
            name: "uv".into(),
            version: "0.13.1".into(),
        })
    }

    async fn ensure_git(&self, _: &Path, _: &StepContext) -> Result<ToolInfo, ProvisionError> {
        self.step("ensure_git")?;
        Ok(ToolInfo {
            name: "git".into(),
            version: "2.55".into(),
        })
    }

    async fn ensure_hf_tool(&self, _: &Path, _: &StepContext) -> Result<ToolInfo, ProvisionError> {
        self.step("ensure_hf_tool")?;
        Ok(ToolInfo {
            name: "hf".into(),
            version: "2.2.0".into(),
        })
    }
}

#[async_trait]
impl PythonProvisioner for Sim {
    async fn install_managed_python(
        &self,
        _: &Path,
        _: PythonVersion,
        _: &StepContext,
    ) -> Result<PathBuf, ProvisionError> {
        self.step("install_managed_python")?;
        Ok(PathBuf::from("python"))
    }

    async fn create_venv(
        &self,
        _: &Path,
        _: &Path,
        target: &Path,
        _: &StepContext,
    ) -> Result<VenvInfo, ProvisionError> {
        self.step("create_venv")?;
        std::fs::create_dir_all(target).unwrap();
        std::fs::write(
            target.join("pyvenv.cfg"),
            b"home = /x\nversion_info = 3.13.16\n",
        )
        .unwrap();
        self.venvs.lock().unwrap().push(target.to_owned());
        Ok(VenvInfo {
            version: PythonVersion::new(3, 13, 16),
            base_interpreter: None,
        })
    }

    async fn inspect_venv(&self, _: &Path, _: &Path) -> Result<VenvInfo, ProvisionError> {
        Ok(VenvInfo {
            version: PythonVersion::new(3, 13, 16),
            base_interpreter: None,
        })
    }

    async fn validate(
        &self,
        root: &Path,
        source: &PythonSourceInput,
    ) -> Result<PythonValidationReport, ProvisionError> {
        // Folder classification is real; only interpreter probing would need Python.
        PythonAdapter::new().validate(root, source).await
    }

    async fn seed(
        &self,
        _: &Path,
        _: &Path,
        _: OsKind,
        _: &StepContext,
    ) -> Result<(), ProvisionError> {
        self.step("seed")
    }

    async fn remove_environment(&self, env: &PythonEnvironment) -> Result<(), ProvisionError> {
        std::fs::remove_dir_all(&env.path).ok();
        Ok(())
    }
}

#[async_trait]
impl PackageInstaller for Sim {
    async fn install(
        &self,
        _: &Path,
        _: &Path,
        spec: &InstallSpec,
        _: &StepContext,
    ) -> Result<(), ProvisionError> {
        let operation = if spec.requirements.iter().any(|r| r.starts_with("torch==")) {
            "install_torch"
        } else if spec.requirements.iter().any(|r| r.contains("triton")) {
            "install_triton"
        } else {
            "install_requirements"
        };
        self.step(operation)?;
        if operation == "install_torch" {
            self.put("torch", "2.14.1+cu130");
            self.put("torchvision", "0.29.1+cu130");
        } else if operation == "install_triton" {
            let name = if cfg!(windows) {
                "triton-windows"
            } else {
                "triton"
            };
            self.put(name, "3.8.0");
        }
        Ok(())
    }

    async fn uninstall(
        &self,
        _: &Path,
        _: &Path,
        _: &[DistName],
        _: &StepContext,
    ) -> Result<(), ProvisionError> {
        Ok(())
    }

    async fn preview_install(
        &self,
        _: &Path,
        _: &Path,
        _: &InstallSpec,
        _: &StepContext,
    ) -> Result<ResolutionPreview, ProvisionError> {
        Ok(ResolutionPreview::default())
    }

    async fn list(
        &self,
        _: &Path,
        _: &Path,
        _: Option<&StepContext>,
    ) -> Result<Vec<InstalledPackage>, ProvisionError> {
        Ok(self.installed.lock().unwrap().clone())
    }

    async fn write_constraints(
        &self,
        root: &Path,
        env: &EnvId,
        pins: &PinSet,
        extra: &[String],
    ) -> Result<PathBuf, ProvisionError> {
        infrastructure::environment::packages::UvPackages::new()
            .write_constraints(root, env, pins, extra)
            .await
    }

    async fn clean_cache(&self, _: &Path, _: &StepContext) -> Result<(), ProvisionError> {
        Ok(())
    }
}

#[async_trait]
impl EnvVerifier for Sim {
    async fn verify_torch(
        &self,
        _: &Path,
        _: &Path,
        _: &VerifyExpectations,
        _: &StepContext,
    ) -> Result<TorchReport, ProvisionError> {
        self.step("verify_torch")?;
        Ok(TorchReport {
            version: "2.14.1+cu130".into(),
            cuda_available: true,
        })
    }

    async fn verify_triton(
        &self,
        _: &Path,
        _: &Path,
        _: &StepContext,
    ) -> Result<String, ProvisionError> {
        self.step("verify_triton")?;
        Ok("3.8.0".into())
    }

    async fn verify_final(
        &self,
        _: &Path,
        _: &Path,
        _: &VerifyExpectations,
        _: &StepContext,
    ) -> Result<VerificationReport, ProvisionError> {
        self.step("verify_final")?;
        Ok(VerificationReport {
            python_version: "3.13.16".into(),
            torch_version: "2.14.1+cu130".into(),
            cuda_available: true,
            cuda_device: None,
            triton_version: Some("3.8.0".into()),
            comfy_path: None,
            accelerators: Vec::new(),
        })
    }
}

#[async_trait]
impl ComfySource for Sim {
    async fn resolve(&self, _: &Path, _: &ComfyRef) -> Result<ResolvedGit, ProvisionError> {
        self.step("resolve")?;
        Ok(ResolvedGit {
            url: "https://example.invalid/ComfyUI.git".into(),
            reference: "v0.40.0".into(),
            branch: None,
            commit: COMMIT.into(),
        })
    }

    async fn inspect_checkout(&self, _: &Path) -> Result<Checkout, ProvisionError> {
        Ok(match self.checkout.lock().unwrap().clone() {
            Some(commit) => Checkout::Repo { commit },
            None => Checkout::Absent,
        })
    }

    async fn clone_comfyui(
        &self,
        _: &Path,
        resolved: &ResolvedGit,
        _: &StepContext,
    ) -> Result<(), ProvisionError> {
        self.step("clone_comfyui")?;
        *self.checkout.lock().unwrap() = Some(resolved.commit.clone());
        Ok(())
    }

    async fn requirement_files(&self, root: &Path) -> Result<RequirementFiles, ProvisionError> {
        Ok(RequirementFiles {
            comfyui: root.join("ComfyUI/requirements.txt"),
            nodes: Vec::new(),
        })
    }

    async fn resolve_manager(&self, _: &Path) -> Result<ResolvedGit, ProvisionError> {
        Err(ProvisionError::Failed("not used".into()))
    }

    async fn install_manager(
        &self,
        _: &Path,
        _: &ResolvedGit,
        _: &StepContext,
    ) -> Result<Vec<PathBuf>, ProvisionError> {
        Err(ProvisionError::Failed("not used".into()))
    }
}

struct Stopped;

#[async_trait]
impl RuntimeMaintenance for Stopped {
    async fn ensure_stopped(&self, _: &Path) -> Result<(), RuntimeBlock> {
        Ok(())
    }
}

/// Consumes one scheduled failure, if any.
fn take_failure(remaining: &Mutex<usize>) -> bool {
    let mut remaining = remaining.lock().unwrap();
    if *remaining > 0 {
        *remaining -= 1;
        true
    } else {
        false
    }
}

/// Fails the first `publish`, then delegates to the real manifest store.
struct FlakyManifests {
    inner: FsManifestStore,
    fail_publishes: Mutex<usize>,
}

#[async_trait]
impl InstallManifestStore for FlakyManifests {
    async fn read(&self, root: &Path) -> Result<ManifestState, ManifestError> {
        self.inner.read(root).await
    }

    async fn publish(&self, manifest: &InstallManifest) -> Result<(), ManifestError> {
        if take_failure(&self.fail_publishes) {
            return Err(ManifestError::WriteFailed);
        }
        self.inner.publish(manifest).await
    }
}

/// Fails the first `commit_activation`, then delegates to the real store.
struct FlakyActivation {
    inner: Arc<SqliteEnvironmentStore>,
    fail_commits: Mutex<usize>,
}

#[async_trait]
impl ActivationStore for FlakyActivation {
    async fn commit_activation(
        &self,
        run: &ProvisioningRun,
        new_active: &EnvId,
    ) -> Result<(), EnvRepoError> {
        if take_failure(&self.fail_commits) {
            return Err(EnvRepoError::Unavailable);
        }
        self.inner.commit_activation(run, new_active).await
    }
}

struct Machine {
    root: PathBuf,
    deps: EnvironmentDeps,
    settings: Arc<SqliteSettingsRepository>,
}

#[derive(Default)]
struct Faults {
    manifest_publish_failures: usize,
    commit_failures: usize,
}

/// Opens the database file under `root` and composes every use case over it — a (re)start.
async fn start(root: &Path, sim: Arc<Sim>, faults: Faults) -> Machine {
    let paths = AppPaths::from_root(root);
    paths.ensure_dirs().unwrap();
    let db = Db::open(&paths.db_path).await.unwrap();
    let store = Arc::new(SqliteEnvironmentStore::new(db.clone()));
    let settings = Arc::new(SqliteSettingsRepository::new(db));
    let current = RootInfo {
        root: root.to_owned(),
        source: RootSource::Default,
    };
    let deps = EnvironmentDeps {
        hardware: sim.clone(),
        catalog: Arc::new(BundledProfiles::load().unwrap()),
        tools: sim.clone(),
        python: sim.clone(),
        packages: sim.clone(),
        verifier: sim.clone(),
        source: sim.clone(),
        envs: store.clone(),
        runs: store.clone(),
        activation: Arc::new(FlakyActivation {
            inner: store.clone(),
            fail_commits: Mutex::new(faults.commit_failures),
        }),
        manifests: Arc::new(FlakyManifests {
            inner: FsManifestStore::new(),
            fail_publishes: Mutex::new(faults.manifest_publish_failures),
        }),
        drafts: store,
        root: Arc::new(FsRootSwitcher::new(
            current,
            root.join("..").join("bootstrap-test.json"),
            settings.clone(),
        )),
        gate: Arc::new(TokioMaintenanceGate::new()),
        runtime: Arc::new(Stopped),
        clock: Arc::new(SystemClock),
        ids: Arc::new(RandomIds::default()),
        active_runs: Arc::new(ActiveRuns::default()),
        plans: Arc::new(PlanStore::default()),
    };
    Machine {
        root: root.to_owned(),
        deps,
        settings,
    }
}

impl Machine {
    fn request(&self) -> SetupRequest {
        SetupRequest {
            mode: SetupMode::FirstRun,
            install_root: self.root.clone(),
            python_source: PythonSourceInput::Managed,
            profile_id: if cfg!(windows) {
                "win-py313-torch2141-cu130".into()
            } else {
                "linux-py313-torch2141-cu130".into()
            },
            comfyui_ref: ComfyRef::Latest,
            install_manager: false,
            clean_cache: false,
            confirm_adopted_changes: false,
            allow_source_builds: false,
        }
    }

    async fn register(&self, request: SetupRequest) -> RunId {
        StartSetup::new(self.deps.clone())
            .execute(SetupAction::New(Box::new(request)))
            .await
            .unwrap()
    }

    async fn run(&self, run_id: &RunId) -> RunOutcome {
        let (sink, _) = run_sink(
            &AppPaths::from_root(&self.root).logs_dir,
            run_id.as_str(),
            Arc::new(NullSink),
        );
        ExecuteSetup::new(self.deps.clone())
            .execute(run_id, sink)
            .await
            .unwrap()
    }

    async fn retry(&self, run_id: &RunId) {
        StartSetup::new(self.deps.clone())
            .execute(SetupAction::Retry {
                run_id: run_id.clone(),
                manager_consent: None,
            })
            .await
            .unwrap();
    }

    async fn status(&self) -> SetupStatus {
        GetSetupStatus::new(self.deps.clone())
            .execute()
            .await
            .unwrap()
    }

    async fn reconcile(&self) -> ReconcileReport {
        ReconcileStartup::new(self.deps.clone())
            .execute()
            .await
            .unwrap()
    }

    async fn envs(&self) -> Vec<PythonEnvironment> {
        self.deps.envs.list().await.unwrap()
    }
}

fn fresh_root() -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("install root ไทย");
    (directory, root)
}

fn manifest_env(root: &Path) -> Option<String> {
    let text = std::fs::read_to_string(root.join("install.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value["env_id"].as_str().map(str::to_owned)
}

#[tokio::test]
async fn a_completed_setup_survives_a_restart() {
    let (_directory, root) = fresh_root();
    let sim = Arc::new(Sim::default());
    let machine = start(&root, sim.clone(), Faults::default()).await;
    assert_eq!(machine.status().await.phase, SetupPhase::NeedsSetup);
    let run_id = machine.register(machine.request()).await;
    assert_eq!(machine.run(&run_id).await, RunOutcome::Succeeded);
    drop(machine);

    // Restart: a new connection to the same database file, nothing in memory.
    let restarted = start(&root, sim, Faults::default()).await;
    assert_eq!(restarted.reconcile().await, ReconcileReport::default());
    let status = restarted.status().await;
    assert_eq!(status.phase, SetupPhase::Ready);
    let active = status.active_env.unwrap();
    assert_eq!(active.state, EnvState::Verified);
    assert_eq!(manifest_env(&root).as_deref(), Some(active.id.as_str()));
    let run = status.run.unwrap();
    assert_eq!(run.status, RunStatus::Succeeded);
    assert_eq!(run.activation, ActivationPhase::Committed);
    assert_eq!(restarted.envs().await.len(), 1);
    // The per-run setup log exists beside the app log.
    assert!(
        AppPaths::from_root(&root)
            .logs_dir
            .join(format!("setup-{}.log", run.id))
            .is_file()
    );
}

#[tokio::test]
async fn a_restart_after_the_activation_intent_rolls_forward_without_duplicates() {
    let (_directory, root) = fresh_root();
    let sim = Arc::new(Sim::default());
    let machine = start(
        &root,
        sim.clone(),
        Faults {
            manifest_publish_failures: 1,
            ..Faults::default()
        },
    )
    .await;
    let run_id = machine.register(machine.request()).await;
    let RunOutcome::Failed(failure) = machine.run(&run_id).await else {
        panic!("the manifest failure must stop the run")
    };
    assert_eq!(failure.step, StepId::FinalVerify);
    assert!(manifest_env(&root).is_none());
    let before = machine.status().await;
    assert_eq!(
        before.run.as_ref().unwrap().activation,
        ActivationPhase::Intent
    );
    assert!(before.active_env.is_none());
    drop(machine);

    let restarted = start(&root, sim, Faults::default()).await;
    let report = restarted.reconcile().await;
    assert_eq!(report.activations_completed, 1);
    let status = restarted.status().await;
    assert_eq!(status.phase, SetupPhase::Ready);
    let active = status.active_env.unwrap();
    assert_eq!(manifest_env(&root).as_deref(), Some(active.id.as_str()));
    assert_eq!(restarted.envs().await.len(), 1);
    assert_eq!(status.run.unwrap().status, RunStatus::Succeeded);
}

#[tokio::test]
async fn a_restart_after_the_manifest_but_before_the_database_switch_rolls_forward() {
    let (_directory, root) = fresh_root();
    let sim = Arc::new(Sim::default());
    let machine = start(
        &root,
        sim.clone(),
        Faults {
            commit_failures: 1,
            ..Faults::default()
        },
    )
    .await;
    let run_id = machine.register(machine.request()).await;
    assert!(matches!(machine.run(&run_id).await, RunOutcome::Failed(_)));
    let before = machine.status().await;
    assert_eq!(
        before.run.as_ref().unwrap().activation,
        ActivationPhase::ManifestPublished
    );
    assert!(
        before.active_env.is_none(),
        "the database has not switched yet"
    );
    assert!(
        manifest_env(&root).is_some(),
        "the manifest was already replaced"
    );
    // The mismatch is reported, never papered over.
    assert!(matches!(
        before.phase,
        SetupPhase::Recovery(RecoveryReason::ManifestMismatch)
    ));
    drop(machine);

    let restarted = start(&root, sim, Faults::default()).await;
    restarted.reconcile().await;
    let status = restarted.status().await;
    assert_eq!(status.phase, SetupPhase::Ready);
    assert_eq!(
        status.active_env.unwrap().id.as_str(),
        manifest_env(&root).unwrap()
    );
    assert_eq!(restarted.envs().await.len(), 1);
}

#[tokio::test]
async fn retrying_after_a_failure_resumes_the_same_run_and_environment() {
    let (_directory, root) = fresh_root();
    let sim = Arc::new(Sim::default());
    sim.fail_once("install_torch");
    let machine = start(&root, sim.clone(), Faults::default()).await;
    let run_id = machine.register(machine.request()).await;
    assert!(matches!(machine.run(&run_id).await, RunOutcome::Failed(_)));
    let failed_env = machine.envs().await.remove(0);
    assert_eq!(failed_env.state(), EnvState::Failed);
    drop(machine);

    // The failure and its run survive a restart; retry reuses the run and the environment.
    let restarted = start(&root, sim.clone(), Faults::default()).await;
    assert_eq!(restarted.status().await.phase, SetupPhase::RunFailed);
    restarted.retry(&run_id).await;
    assert_eq!(restarted.run(&run_id).await, RunOutcome::Succeeded);
    let envs = restarted.envs().await;
    assert_eq!(envs.len(), 1, "no duplicate environment");
    assert_eq!(envs[0].id, failed_env.id);
    assert_eq!(envs[0].state(), EnvState::Verified);
    assert_eq!(
        sim.count("clone_comfyui"),
        1,
        "the completed steps are re-checked, not redone"
    );
    let status = restarted.status().await;
    assert_eq!(
        status.run.unwrap().steps[StepId::InstallTorch.number() as usize].attempts,
        2
    );
}

#[tokio::test]
async fn a_run_that_was_running_when_the_app_died_becomes_retryable() {
    let (_directory, root) = fresh_root();
    let sim = Arc::new(Sim::default());
    let machine = start(&root, sim.clone(), Faults::default()).await;
    let run_id = machine.register(machine.request()).await;
    // Simulate the process dying mid-run: the persisted record says Running.
    let mut run = machine.deps.runs.get(&run_id).await.unwrap().unwrap();
    run.status = RunStatus::Running;
    run.step_mut(StepId::Tools).status = StepStatus::Running;
    machine.deps.runs.save(&run).await.unwrap();
    drop(machine);

    let restarted = start(&root, sim, Faults::default()).await;
    assert_eq!(restarted.reconcile().await.interrupted_runs, 1);
    let status = restarted.status().await;
    assert_eq!(status.phase, SetupPhase::RunFailed);
    assert_eq!(status.run.as_ref().unwrap().status, RunStatus::Interrupted);
    restarted.retry(&run_id).await;
    assert_eq!(restarted.run(&run_id).await, RunOutcome::Succeeded);
}

#[tokio::test]
async fn a_custom_empty_folder_outside_the_root_becomes_an_app_owned_environment() {
    let (directory, root) = fresh_root();
    let sim = Arc::new(Sim::default());
    let machine = start(&root, sim.clone(), Faults::default()).await;
    let external = directory.path().join("external venvs ไทย").join("my venv");
    let run_id = machine
        .register(SetupRequest {
            python_source: PythonSourceInput::CustomVenv(external.clone()),
            ..machine.request()
        })
        .await;
    assert_eq!(machine.run(&run_id).await, RunOutcome::Succeeded);
    let env = machine.envs().await.remove(0);
    assert_eq!(env.ownership, domain::environment::Ownership::AppOwned);
    assert!(
        env.path
            .ends_with(Path::new("external venvs ไทย").join("my venv"))
    );
    assert!(env.path.join("pyvenv.cfg").is_file());
    assert!(!root.join("envs").join(env.id.as_str()).exists());
    // The adapter deletes it only inside its authorized location.
    assert_eq!(env.authorized_location, env.path);
}

#[tokio::test]
async fn choosing_another_root_carries_settings_and_draft_to_the_restarted_app() {
    let (directory, root) = fresh_root();
    let sim = Arc::new(Sim::default());
    let machine = start(&root, sim.clone(), Faults::default()).await;
    let settings = Settings::new(
        9,
        HfEngine::Native,
        CloseBehavior::StopComfy,
        Theme::Dark,
        24,
        Some(4096),
    )
    .unwrap();
    machine.settings.save(&settings).await.unwrap();

    let chosen = directory.path().join("chosen root ราก");
    SelectInstallRoot::new(machine.deps.clone())
        .execute(chosen.clone(), Some("{\"profile\":\"x\"}".into()))
        .await
        .unwrap();
    drop(machine);

    // The pointer lives beside the old root in this test; the new app composes on it.
    let pointer = root.join("..").join("bootstrap-test.json");
    let new_root = read_bootstrap(&pointer).unwrap().unwrap();
    assert!(new_root.ends_with("chosen root ราก"));
    let restarted = start(&new_root, sim, Faults::default()).await;
    assert_eq!(restarted.settings.load().await.unwrap(), Some(settings));
    let status = restarted.status().await;
    assert_eq!(status.phase, SetupPhase::NeedsSetup);
    assert_eq!(status.draft.as_deref(), Some("{\"profile\":\"x\"}"));
    assert_eq!(status.root.root, new_root);
    // Setup now runs on the new root.
    let run_id = restarted.register(restarted.request()).await;
    assert_eq!(restarted.run(&run_id).await, RunOutcome::Succeeded);
    assert!(new_root.join("install.json").is_file());
    assert!(!root.join("install.json").exists());
}

#[tokio::test]
async fn the_environment_can_only_be_changed_by_a_second_run_after_the_first_finished() {
    let (_directory, root) = fresh_root();
    let sim = Arc::new(Sim::default());
    let machine = start(&root, sim.clone(), Faults::default()).await;
    let first = machine.register(machine.request()).await;
    assert!(matches!(
        StartSetup::new(machine.deps.clone())
            .execute(SetupAction::New(Box::new(machine.request())))
            .await,
        Err(EnvironmentAppError::SetupInProgress)
    ));
    assert_eq!(machine.run(&first).await, RunOutcome::Succeeded);
    // A re-setup builds a second environment next to the first; both stay in the database.
    sim.installed.lock().unwrap().clear();
    let second = machine
        .register(SetupRequest {
            mode: SetupMode::ChangeTorch,
            comfyui_ref: ComfyRef::KeepCurrent,
            ..machine.request()
        })
        .await;
    assert_eq!(machine.run(&second).await, RunOutcome::Succeeded);
    let envs = machine.envs().await;
    assert_eq!(envs.len(), 2);
    let status = machine.status().await;
    assert_eq!(
        status.active_env.unwrap().id.as_str(),
        machine
            .deps
            .runs
            .get(&second)
            .await
            .unwrap()
            .unwrap()
            .target_env
            .as_str()
    );
    assert_eq!(
        manifest_env(&root).as_deref(),
        Some(status.run.unwrap().target_env.as_str())
    );
}
