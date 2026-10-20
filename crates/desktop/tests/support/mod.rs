//! Shared fixtures of the desktop integration tests: a mock Tauri app wired with counting fakes
//! for every port that would touch the network, a subprocess or the OS, and real adapters
//! (SQLite, manifest, profiles, gate) over a temporary install root.

#![allow(dead_code)]

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use application::{
    comfy_runtime::{RuntimeBlock, RuntimeMaintenance},
    custom_nodes::NodeRequirements,
    environment::*,
    settings::{
        CredentialBackend, CredentialPresence, CredentialStore, CredentialStoreError,
        PendingCredentialRef, SettingsRepoError, SettingsRepository,
    },
};
use async_trait::async_trait;
use desktop::{
    AppState, MANAGER_WINDOW, RecoveryInfo, Services, app_lifecycle::ShellActions,
    comfy_window::open_comfyui_window, ipc::bindings, picker::FilePicker, state::RuntimeAdapters,
};
use domain::{
    environment::{
        ComputeCapability, CudaVersion, EnvId, GpuObservation, GpuProbe, HardwareObservation,
        OsKind, PythonEnvironment, PythonVersion,
    },
    settings::{CredentialKind, Settings},
};
use infrastructure::{
    comfy_runtime::{
        extra_paths::YamlExtraModelPaths,
        git::GitRepository,
        logs::RuntimeLogHub,
        port::TcpPortProbe,
        process::{ComfyRuntimeSupervisor, PythonLaunchPreflight, SharedRuntimeRunner},
        sqlite::SqliteRuntimeStore,
    },
    custom_nodes::stack::{NodeAdapters, NodeOptions},
    environment::{
        manifest::FsManifestStore, profiles::BundledProfiles, sqlite::SqliteEnvironmentStore,
    },
    models::{
        acquisition::stack::{DownloadStack, path_locks},
        library::stack::LibraryStack,
    },
    settings::secrets::{CredentialSnapshot, CredentialVault, SecretIngress},
    shared::{
        clock::{RandomIds, SystemClock},
        gate::TokioMaintenanceGate,
        sqlite::Db,
    },
};
use tauri::{
    Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    ipc::{CallbackFn, InvokeBody},
    test::{INVOKE_KEY, MockRuntime, mock_builder},
    webview::InvokeRequest,
};

/// Counts every call that reaches an application port, i.e. every command body that ran.
#[derive(Clone, Default)]
pub struct HandlerCalls(Arc<AtomicUsize>);

impl HandlerCalls {
    pub fn hit(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }

    pub fn count(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
}

pub struct FakeRepository {
    pub calls: HandlerCalls,
    pub stored: Mutex<Option<Settings>>,
}

#[async_trait]
impl SettingsRepository for FakeRepository {
    async fn load(&self) -> Result<Option<Settings>, SettingsRepoError> {
        self.calls.hit();
        Ok(self.stored.lock().unwrap().clone())
    }

    async fn save(&self, settings: &Settings) -> Result<(), SettingsRepoError> {
        self.calls.hit();
        *self.stored.lock().unwrap() = Some(settings.clone());
        Ok(())
    }
}

pub struct FakeCredentials {
    pub calls: HandlerCalls,
}

#[async_trait]
impl CredentialStore for FakeCredentials {
    async fn commit(
        &self,
        _kind: CredentialKind,
        _pending: PendingCredentialRef,
    ) -> Result<(), CredentialStoreError> {
        self.calls.hit();
        Ok(())
    }

    async fn clear(&self, _kind: CredentialKind) -> Result<(), CredentialStoreError> {
        self.calls.hit();
        Ok(())
    }

    async fn status(
        &self,
        _kind: CredentialKind,
    ) -> Result<CredentialPresence, CredentialStoreError> {
        self.calls.hit();
        Ok(CredentialPresence::NotSet)
    }

    fn backend(&self) -> CredentialBackend {
        self.calls.hit();
        CredentialBackend::SessionMemory
    }
}

/// A vault without credentials; no test reaches a provider.
pub struct EmptyVault;

#[async_trait]
impl CredentialVault for EmptyVault {
    async fn read(&self, _: CredentialKind) -> Result<CredentialSnapshot, CredentialStoreError> {
        Ok(CredentialSnapshot {
            generation: 0,
            value: None,
        })
    }

    async fn generation(&self, _: CredentialKind) -> Result<u64, CredentialStoreError> {
        Ok(0)
    }
}

pub struct FakePicker {
    pub calls: HandlerCalls,
    pub answer: Option<PathBuf>,
}

pub struct FakeShell {
    pub calls: HandlerCalls,
}

impl ShellActions for FakeShell {
    fn open_comfy_window(&self, _: tauri::Url) -> Result<(), desktop::ipc::error::IpcError> {
        self.calls.hit();
        Ok(())
    }

    fn open_browser(&self, _: tauri::Url) {
        self.calls.hit();
    }

    fn perform(&self, _: application::comfy_runtime::CloseAction) {
        self.calls.hit();
    }
    fn request_restart(&self) {
        self.calls.hit();
    }
}

#[async_trait]
impl FilePicker for FakePicker {
    async fn pick_folder(&self, _title: Option<String>) -> Option<PathBuf> {
        self.calls.hit();
        self.answer.clone()
    }

    async fn pick_file(&self, _title: Option<String>, _extensions: Vec<String>) -> Option<PathBuf> {
        self.calls.hit();
        self.answer.clone()
    }
}

/// Counts and delegates the persistence ports.
#[derive(Clone)]
pub struct Counted {
    inner: Arc<SqliteEnvironmentStore>,
    calls: HandlerCalls,
}

#[async_trait]
impl EnvironmentRepository for Counted {
    async fn list(&self) -> Result<Vec<PythonEnvironment>, EnvRepoError> {
        self.calls.hit();
        EnvironmentRepository::list(&*self.inner).await
    }

    async fn get(&self, id: &EnvId) -> Result<Option<PythonEnvironment>, EnvRepoError> {
        self.calls.hit();
        EnvironmentRepository::get(&*self.inner, id).await
    }

    async fn active(&self) -> Result<Option<PythonEnvironment>, EnvRepoError> {
        self.calls.hit();
        self.inner.active().await
    }

    async fn save(&self, env: &PythonEnvironment) -> Result<(), EnvRepoError> {
        self.calls.hit();
        EnvironmentRepository::save(&*self.inner, env).await
    }

    async fn load_components(&self, id: &EnvId) -> Result<InstalledComponents, EnvRepoError> {
        self.calls.hit();
        self.inner.load_components(id).await
    }
    async fn save_components(
        &self,
        id: &EnvId,
        components: &InstalledComponents,
    ) -> Result<(), EnvRepoError> {
        self.calls.hit();
        self.inner.save_components(id, components).await
    }
    async fn delete(&self, id: &EnvId) -> Result<(), EnvRepoError> {
        self.calls.hit();
        EnvironmentRepository::delete(&*self.inner, id).await
    }
}

#[async_trait]
impl ProvisioningRunRepository for Counted {
    async fn insert(&self, run: &ProvisioningRun) -> Result<(), EnvRepoError> {
        self.calls.hit();
        ProvisioningRunRepository::insert(&*self.inner, run).await
    }

    async fn save(&self, run: &ProvisioningRun) -> Result<(), EnvRepoError> {
        self.calls.hit();
        ProvisioningRunRepository::save(&*self.inner, run).await
    }

    async fn get(&self, id: &RunId) -> Result<Option<ProvisioningRun>, EnvRepoError> {
        self.calls.hit();
        ProvisioningRunRepository::get(&*self.inner, id).await
    }

    async fn latest(&self) -> Result<Option<ProvisioningRun>, EnvRepoError> {
        self.calls.hit();
        self.inner.latest().await
    }

    async fn unfinished(&self) -> Result<Vec<ProvisioningRun>, EnvRepoError> {
        self.calls.hit();
        self.inner.unfinished().await
    }
}

#[async_trait]
impl ActivationStore for Counted {
    async fn commit_activation(
        &self,
        run: &ProvisioningRun,
        new_active: &EnvId,
    ) -> Result<(), EnvRepoError> {
        self.calls.hit();
        self.inner.commit_activation(run, new_active).await
    }
}

#[async_trait]
impl WizardDraftStore for Counted {
    async fn load(&self) -> Result<Option<String>, EnvRepoError> {
        self.calls.hit();
        WizardDraftStore::load(&*self.inner).await
    }

    async fn save(&self, draft: &str) -> Result<(), EnvRepoError> {
        self.calls.hit();
        WizardDraftStore::save(&*self.inner, draft).await
    }
}

/// Counts the reads of the curated node catalog.
pub struct CountedCatalog {
    inner: infrastructure::environment::curated::BundledCuratedNodes,
    calls: HandlerCalls,
}

impl CuratedNodeCatalog for CountedCatalog {
    fn list(&self) -> Vec<CuratedNodeSpec> {
        self.calls.hit();
        self.inner.list()
    }
}

/// The machine, the network and the subprocesses: every step fails cleanly.
pub struct Stubs {
    pub calls: HandlerCalls,
    pub root: PathBuf,
    pub switches: Mutex<Vec<PathBuf>>,
}

#[async_trait]
impl InstallationProbe for Stubs {
    async fn inspect(
        &self,
        _: &Path,
        _: &InstallManifest,
        _: &PythonEnvironment,
    ) -> Result<InstallationHealth, ProvisionError> {
        self.calls.hit();
        Ok(InstallationHealth::Healthy)
    }
}

#[async_trait]
impl ExistingInstallOpener for Stubs {
    async fn open_existing(&self, _: &Path) -> Result<(), RootSwitchError> {
        self.calls.hit();
        Ok(())
    }
    async fn select_new(&self, _: &Path) -> Result<(), RootSwitchError> {
        self.calls.hit();
        Ok(())
    }
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
        os: if cfg!(windows) {
            OsKind::Windows
        } else {
            OsKind::Linux
        },
        os_version: "Test OS".into(),
        install_volume_free: Some(500 << 30),
        env_volume_free: None,
        env_on_separate_volume: false,
        long_paths_enabled: Some(true),
    }
}

fn stub_failure() -> ProvisionError {
    ProvisionError::Failed("not available in tests".into())
}

#[async_trait]
impl HardwareProbe for Stubs {
    async fn probe(&self, _root: &Path, _env: Option<&Path>) -> HardwareObservation {
        self.calls.hit();
        healthy_hardware()
    }
}

#[async_trait]
impl ToolProvisioner for Stubs {
    async fn ensure_uv(&self, _: &Path, _: &StepContext) -> Result<ToolInfo, ProvisionError> {
        self.calls.hit();
        Err(stub_failure())
    }

    async fn ensure_git(&self, _: &Path, _: &StepContext) -> Result<ToolInfo, ProvisionError> {
        Err(stub_failure())
    }

    async fn ensure_hf_tool(&self, _: &Path, _: &StepContext) -> Result<ToolInfo, ProvisionError> {
        Err(stub_failure())
    }
}

#[async_trait]
impl PythonProvisioner for Stubs {
    async fn install_managed_python(
        &self,
        _: &Path,
        _: PythonVersion,
        _: &StepContext,
    ) -> Result<PathBuf, ProvisionError> {
        Err(stub_failure())
    }

    async fn create_venv(
        &self,
        _: &Path,
        _: &Path,
        _: &Path,
        _: &StepContext,
    ) -> Result<VenvInfo, ProvisionError> {
        Err(stub_failure())
    }

    async fn inspect_venv(&self, _: &Path, _: &Path) -> Result<VenvInfo, ProvisionError> {
        Err(stub_failure())
    }

    async fn validate(
        &self,
        _root: &Path,
        _source: &PythonSourceInput,
    ) -> Result<PythonValidationReport, ProvisionError> {
        self.calls.hit();
        Ok(PythonValidationReport {
            target: ValidationTarget::Interpreter,
            resolved_path: PathBuf::from("python"),
            python_version: None,
            checks: Vec::new(),
        })
    }

    async fn seed(
        &self,
        _: &Path,
        _: &Path,
        _: OsKind,
        _: &StepContext,
    ) -> Result<(), ProvisionError> {
        Err(stub_failure())
    }

    async fn remove_environment(&self, _: &PythonEnvironment) -> Result<(), ProvisionError> {
        self.calls.hit();
        Err(stub_failure())
    }
}

#[async_trait]
impl PackageInstaller for Stubs {
    async fn install(
        &self,
        _: &Path,
        _: &Path,
        _: &InstallSpec,
        _: &StepContext,
    ) -> Result<(), ProvisionError> {
        Err(stub_failure())
    }

    async fn uninstall(
        &self,
        _: &Path,
        _: &Path,
        _: &[domain::environment::DistName],
        _: &StepContext,
    ) -> Result<(), ProvisionError> {
        Err(stub_failure())
    }

    async fn preview_install(
        &self,
        _: &Path,
        _: &Path,
        _: &InstallSpec,
        _: &StepContext,
    ) -> Result<ResolutionPreview, ProvisionError> {
        self.calls.hit();
        Err(stub_failure())
    }

    async fn list(
        &self,
        _: &Path,
        _: &Path,
        _: Option<&StepContext>,
    ) -> Result<Vec<InstalledPackage>, ProvisionError> {
        self.calls.hit();
        Ok(Vec::new())
    }

    async fn write_constraints(
        &self,
        _: &Path,
        _: &EnvId,
        _: &domain::environment::PinSet,
        _: &[String],
    ) -> Result<PathBuf, ProvisionError> {
        Err(stub_failure())
    }

    async fn clean_cache(&self, _: &Path, _: &StepContext) -> Result<(), ProvisionError> {
        Err(stub_failure())
    }
}

#[async_trait]
impl EnvVerifier for Stubs {
    async fn verify_accelerator(
        &self,
        _root: &Path,
        _env_path: &Path,
        _module: &str,
        _ctx: &StepContext,
    ) -> Result<(), ProvisionError> {
        Ok(())
    }

    async fn verify_torch(
        &self,
        _: &Path,
        _: &Path,
        _: &domain::environment::VerifyExpectations,
        _: &StepContext,
    ) -> Result<TorchReport, ProvisionError> {
        Err(stub_failure())
    }

    async fn verify_triton(
        &self,
        _: &Path,
        _: &Path,
        _: &StepContext,
    ) -> Result<String, ProvisionError> {
        Err(stub_failure())
    }

    async fn verify_final(
        &self,
        _: &Path,
        _: &Path,
        _: &domain::environment::VerifyExpectations,
        _: &StepContext,
    ) -> Result<VerificationReport, ProvisionError> {
        Err(stub_failure())
    }
}

#[async_trait]
impl ComfySource for Stubs {
    async fn resolve(&self, _: &Path, _: &ComfyRef) -> Result<ResolvedGit, ProvisionError> {
        Err(stub_failure())
    }

    async fn inspect_checkout(&self, _: &Path) -> Result<Checkout, ProvisionError> {
        self.calls.hit();
        Ok(Checkout::Absent)
    }

    async fn clone_comfyui(
        &self,
        _: &Path,
        _: &ResolvedGit,
        _: &StepContext,
    ) -> Result<(), ProvisionError> {
        Err(stub_failure())
    }

    async fn requirement_files(&self, _: &Path) -> Result<RequirementFiles, ProvisionError> {
        Err(stub_failure())
    }

    async fn resolve_manager(&self, _: &Path) -> Result<ResolvedGit, ProvisionError> {
        Err(stub_failure())
    }

    async fn install_manager(
        &self,
        _: &Path,
        _: &ResolvedGit,
        _: &StepContext,
    ) -> Result<Vec<PathBuf>, ProvisionError> {
        Err(stub_failure())
    }
}

#[async_trait]
impl RootSwitcher for Stubs {
    fn current(&self) -> RootInfo {
        RootInfo {
            root: self.root.clone(),
            source: RootSource::Default,
        }
    }

    async fn switch_root(
        &self,
        new_root: &Path,
        _draft: Option<&str>,
    ) -> Result<(), RootSwitchError> {
        self.calls.hit();
        self.switches.lock().unwrap().push(new_root.to_owned());
        Ok(())
    }
}

#[async_trait]
impl RuntimeMaintenance for Stubs {
    async fn ensure_stopped(&self, _: &Path) -> Result<(), RuntimeBlock> {
        self.calls.hit();
        Ok(())
    }
}

pub struct CountedManifests {
    inner: FsManifestStore,
    calls: HandlerCalls,
}

#[async_trait]
impl InstallManifestStore for CountedManifests {
    async fn read(&self, root: &Path) -> Result<ManifestState, ManifestError> {
        self.calls.hit();
        self.inner.read(root).await
    }

    async fn publish(&self, manifest: &InstallManifest) -> Result<(), ManifestError> {
        self.calls.hit();
        self.inner.publish(manifest).await
    }
}

pub struct TestApp {
    pub app: tauri::App<MockRuntime>,
    pub calls: HandlerCalls,
    pub stubs: Arc<Stubs>,
    pub root: PathBuf,
    _directory: tempfile::TempDir,
}

#[derive(Default)]
pub struct Options {
    pub recovery: Option<RecoveryInfo>,
    pub picker_answer: Option<PathBuf>,
}

pub fn build_app() -> TestApp {
    build_app_with(Options::default())
}

pub fn build_app_with(options: Options) -> TestApp {
    let calls = HandlerCalls::default();
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("install root ไทย");
    std::fs::create_dir_all(root.join("data")).unwrap();
    let stubs = Arc::new(Stubs {
        calls: calls.clone(),
        root: root.clone(),
        switches: Mutex::default(),
    });
    let picker = Arc::new(FakePicker {
        calls: calls.clone(),
        answer: options.picker_answer,
    });
    let credentials = Arc::new(FakeCredentials {
        calls: calls.clone(),
    });
    let services = match options.recovery {
        Some(info) => Err(info),
        None => {
            let db = tauri::async_runtime::block_on(Db::open(root.join("data/app.db"))).unwrap();
            let runtime_store = Arc::new(SqliteRuntimeStore::new(db.clone()));
            let locks = path_locks();
            let downloads =
                DownloadStack::open(&root, db.clone(), Arc::new(EmptyVault), 2, locks.clone())
                    .unwrap();
            let library = LibraryStack::open(
                &root,
                db.clone(),
                Arc::new(EmptyVault),
                locks,
                Arc::new(YamlExtraModelPaths),
                downloads.repository.clone(),
                true,
            )
            .unwrap();
            let hub = RuntimeLogHub::default();
            let supervisor = Arc::new(ComfyRuntimeSupervisor::new(
                root.clone(),
                Arc::new(SharedRuntimeRunner::new()),
                runtime_store.clone(),
                hub.clone(),
            ));
            let runtime = RuntimeAdapters {
                supervisor,
                store: runtime_store,
                repository: Arc::new(GitRepository::new(stubs.clone())),
                ports: Arc::new(TcpPortProbe),
                preflight: Arc::new(PythonLaunchPreflight::new(stubs.clone())),
                extra_paths: Arc::new(YamlExtraModelPaths),
                extra_paths_writer: Arc::new(YamlExtraModelPaths),
                hub,
            };
            let store = Counted {
                inner: Arc::new(SqliteEnvironmentStore::new(db.clone())),
                calls: calls.clone(),
            };
            let settings: Arc<dyn SettingsRepository> = Arc::new(FakeRepository {
                calls: calls.clone(),
                stored: Mutex::new(None),
            });
            let node_adapters = NodeAdapters::open(
                &root,
                db.clone(),
                NodeOptions {
                    // No test reaches the network: any registry request fails to connect.
                    registry_base: "http://127.0.0.1:9".into(),
                    downloader: infrastructure::shared::download::Downloader::with_loopback_http(),
                    ..Default::default()
                },
            );
            let deps = EnvironmentDeps {
                installation: stubs.clone(),
                relocation: Arc::new(infrastructure::environment::relocation::persistence::SqliteRelocationStore::new(db.clone())),
                relocation_runtime: stubs.clone(),
                hardware: stubs.clone(),
                catalog: Arc::new(BundledProfiles::load().unwrap()),
                curated: Arc::new(CountedCatalog {
                    inner: infrastructure::environment::curated::BundledCuratedNodes::load()
                        .unwrap(),
                    calls: calls.clone(),
                }),
                tools: stubs.clone(),
                python: stubs.clone(),
                packages: stubs.clone(),
                wheels: Arc::new(infrastructure::environment::wheels::Wheels::new()),
                verifier: stubs.clone(),
                source: stubs.clone(),
                node_requirements: Arc::new(NodeRequirements::new(
                    node_adapters.store.clone(),
                    node_adapters.files.clone(),
                )),
                envs: Arc::new(store.clone()),
                runs: Arc::new(store.clone()),
                activation: Arc::new(store.clone()),
                manifests: Arc::new(CountedManifests {
                    inner: FsManifestStore::new(),
                    calls: calls.clone(),
                }),
                drafts: Arc::new(store),
                root: stubs.clone(),
                gate: Arc::new(TokioMaintenanceGate::new()),
                runtime: stubs.clone(),
                clock: Arc::new(SystemClock),
                ids: Arc::new(RandomIds::default()),
                active_runs: Arc::new(ActiveRuns::default()),
                plans: Arc::new(PlanStore::default()),
            };
            let nodes = node_adapters.bind(deps.clone(), "test-epoch".into());
            let services = Services::new(
                deps,
                settings,
                stubs.current(),
                root.join("data/logs"),
                runtime,
                downloads,
                library,
                nodes,
            );
            services.start_library();
            Ok(services)
        }
    };
    let mut state = AppState::new(
        services,
        credentials,
        SecretIngress::default(),
        picker,
        root.join("data"),
    );
    state.install_opener(stubs.clone());

    // The production context keeps the compiled app manifest and the `manager` capability.
    // Only the configured windows are cleared so the tests create each window explicitly.
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();

    let app = mock_builder()
        .manage(state)
        .invoke_handler(bindings::builder::<MockRuntime>().invoke_handler())
        .build(context)
        .expect("failed to build the mock app");
    app.state::<AppState>().install_shell(Arc::new(FakeShell {
        calls: calls.clone(),
    }));
    TestApp {
        app,
        calls,
        stubs,
        root,
        _directory: directory,
    }
}

pub fn local_url() -> tauri::Url {
    if cfg!(any(windows, target_os = "android")) {
        "http://tauri.localhost"
    } else {
        "tauri://localhost"
    }
    .parse()
    .unwrap()
}

pub fn request(cmd: &str, url: tauri::Url, body: serde_json::Value) -> InvokeRequest {
    InvokeRequest {
        cmd: cmd.into(),
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url,
        body: InvokeBody::Json(body),
        headers: Default::default(),
        invoke_key: INVOKE_KEY.to_string(),
    }
}

pub fn manager_window(app: &TestApp) -> WebviewWindow<MockRuntime> {
    WebviewWindowBuilder::new(&app.app, MANAGER_WINDOW, WebviewUrl::default())
        .build()
        .unwrap()
}

pub fn comfyui_window(app: &TestApp) -> WebviewWindow<MockRuntime> {
    open_comfyui_window(&app.app, "http://127.0.0.1:8188".parse().unwrap()).unwrap()
}

/// The profile id valid on the host OS.
pub fn host_profile() -> &'static str {
    if cfg!(windows) {
        "win-py313-torch2141-cu130"
    } else {
        "linux-py313-torch2141-cu130"
    }
}
