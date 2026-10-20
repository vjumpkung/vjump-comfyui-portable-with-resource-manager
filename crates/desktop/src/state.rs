//! The composition root: use cases wired to their infrastructure adapters.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU32, Ordering},
    },
};

use application::{
    comfy_runtime::{
        ComfyRepository, ExtraModelPaths, ExtraModelPathsWriter, GetComfyStatus, GetLaunchOptions,
        LaunchPreflight, ListComfyVersions, ManageExtraModelPaths, NodeLifecycle, PortProbe,
        ReadExtraModelPaths, RestartComfy, RuntimeDeps, RuntimeError, RuntimeEvents, RuntimeStore,
        RuntimeSupervisor, SetLaunchOptions, StartComfy, StopComfy, UpdateComfy,
    },
    custom_nodes::{NodeContext, NodeRequirements, NodeService, PrepareRequest},
    environment::{
        ActiveRuns, ApplyPackageChange, CancelSetup, CuratedNodeCatalog, DeleteEnvironment,
        DetectHardware, EnvironmentDeps, ExecuteSetup, ExistingInstallOpener, GetEnvironmentInfo,
        GetSetupStatus, InstallManifestStore, ListPackages, ListProfiles, ManifestState, PlanStore,
        PreviewPackageChange, ReconcileStartup, RootInfo, RootSource, SelectInstallRoot,
        SetupNodes, StartSetup, ValidatePython,
    },
    models::{
        acquisition::{
            CancelJob, DownloadFiles, EnqueueDownloads, ExportResourceList, ImportPack,
            JobRepository, ListJobs, ModelPacks, PauseJob, ResolveDownload, ResumeJob, RetryJob,
            SourceResolver,
        },
        library::{
            DeleteModels, DeleteModelsPermanently, GetLibraryStatus, GetModelDetails,
            GetOperationStatus, IdentifyModel, ListModels, MoveModel, PlanDelete,
            ReconcileModelOperations, RenameModel, RescanModels,
        },
    },
    settings::{
        ClearCredential, CredentialStore, GetCredentialStatus, GetSettings, SetCredential,
        SettingsRepository, UpdateSettings,
        credential_tester::{CredentialTester, TestCredential},
        diagnostics::{CollectDiagnostics, PinnedTool},
        updates::{AppUpdater, InstallAppUpdate},
    },
    shared::{CancelToken, IdGenerator, MaintenanceGate, MaintenanceLease},
};
use domain::settings::Settings;
use infrastructure::{
    comfy_runtime::{
        extra_paths::YamlExtraModelPaths,
        git::GitRepository,
        logs::RuntimeLogHub,
        port::TcpPortProbe,
        process::{ComfyRuntimeSupervisor, PythonLaunchPreflight, SharedRuntimeRunner},
        sqlite::SqliteRuntimeStore,
    },
    custom_nodes::stack::{HubSink, NodeAdapters, NodeEvent, NodeHub, NodeOptions, NodeStack},
    environment::{
        curated::BundledCuratedNodes,
        hardware::SystemHardware,
        manifest::FsManifestStore,
        packages::UvPackages,
        pins,
        profiles::BundledProfiles,
        python::PythonAdapter,
        relocation::{
            opener::{FsExistingInstallOpener, RelocationRuntimeGuard},
            persistence::SqliteRelocationStore,
            probe::{FsInstallationProbe, root_moved},
        },
        source::GitSource,
        sqlite::SqliteEnvironmentStore,
        switcher::FsRootSwitcher,
        tools::Tools,
        verify::PythonVerifier,
    },
    models::{
        acquisition::stack::{DownloadStack, path_locks},
        library::stack::LibraryStack,
    },
    settings::{
        secrets::{KeyringCredentialStore, SecretIngress},
        sqlite::{Db, SqliteSettingsRepository},
    },
    shared::{
        clock::{RandomIds, SystemClock},
        gate::TokioMaintenanceGate,
        log_tail::FileLogTail,
        paths::{AppPaths, ResolvedRoot, RootError, bootstrap_path},
    },
};

use crate::{
    app_lifecycle::{CloseCoordinator, ShellActions},
    diagnostics::{Clipboard, UnavailableClipboard},
    ipc::{
        error::IpcError,
        node_dto::{NodeOutcomeDto, PlanReviewDto},
    },
    picker::FilePicker,
    updates::SignedUpdater,
};

/// Why the app started without an install root. Only recovery guidance is available then.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryInfo {
    pub code: String,
    pub message: String,
    pub root: Option<RootInfo>,
}

impl RecoveryInfo {
    pub fn from_root_error(error: &RootError) -> Self {
        let (code, message) = match error {
            RootError::BootstrapCorrupt => (
                "bootstrap_corrupt",
                "The saved install location (bootstrap.json) is unreadable. Fix or delete that file in the app's configuration folder, then restart.",
            ),
            RootError::BootstrapNewer => (
                "bootstrap_newer",
                "The saved install location was written by a newer version of this app. Update the app, or remove bootstrap.json to choose the location again.",
            ),
            RootError::BootstrapInaccessible => (
                "bootstrap_inaccessible",
                "The saved install location (bootstrap.json) cannot be read. Check the file permissions in the app's configuration folder, then restart.",
            ),
            RootError::NoLocalDataDirectory => (
                "no_data_directory",
                "The operating system did not provide a local data folder for the app.",
            ),
            RootError::RootMissing(_) => (
                "root_missing",
                "The saved installation folder is missing. Open an existing installation or select a new folder.",
            ),
            RootError::RootInaccessible(_) => (
                "root_inaccessible",
                "The saved installation folder cannot be accessed. Check its permissions.",
            ),
        };
        Self {
            code: code.to_owned(),
            message: message.to_owned(),
            root: match error {
                RootError::RootMissing(path) | RootError::RootInaccessible(path) => {
                    Some(RootInfo {
                        root: path.clone(),
                        source: RootSource::Bootstrap,
                    })
                }
                _ => None,
            },
        }
    }

    pub fn startup_failed() -> Self {
        Self {
            code: "startup_failed".to_owned(),
            message:
                "The app could not open its database or logs. Check the install folder and restart."
                    .to_owned(),
            root: None,
        }
    }

    pub fn startup_failed_for(resolved: &ResolvedRoot) -> Self {
        let mut info = Self::startup_failed();
        info.root = Some(services_root(resolved));
        info
    }
}

pub struct SetupServices {
    pub(crate) deps: EnvironmentDeps,
    pub(crate) setup_status: GetSetupStatus,
    pub(crate) start_setup: StartSetup,
    pub(crate) execute_setup: Arc<ExecuteSetup>,
    pub(crate) cancel_setup: CancelSetup,
    pub(crate) root: RootInfo,
    pub(crate) logs_dir: PathBuf,
}

impl SetupServices {
    fn new(
        deps: EnvironmentDeps,
        nodes: Arc<dyn SetupNodes>,
        root: RootInfo,
        logs_dir: PathBuf,
    ) -> Self {
        Self {
            deps: deps.clone(),
            setup_status: GetSetupStatus::new(deps.clone()),
            start_setup: StartSetup::new(deps.clone()),
            execute_setup: Arc::new(ExecuteSetup::new(deps.clone(), nodes)),
            cancel_setup: CancelSetup::new(deps),
            root,
            logs_dir,
        }
    }
}

/// The ComfyUI runtime ports; one installation-scoped supervisor serves every use case.
pub struct RuntimeAdapters {
    pub supervisor: Arc<dyn RuntimeSupervisor>,
    pub store: Arc<dyn RuntimeStore>,
    pub repository: Arc<dyn ComfyRepository>,
    pub ports: Arc<dyn PortProbe>,
    pub preflight: Arc<dyn LaunchPreflight>,
    pub extra_paths: Arc<dyn ExtraModelPaths>,
    pub extra_paths_writer: Arc<dyn ExtraModelPathsWriter>,
    pub hub: RuntimeLogHub,
}

/// ComfyUI runtime use cases and the live subscription registry.
pub struct ComfyServices {
    pub(crate) supervisor: Arc<dyn RuntimeSupervisor>,
    pub(crate) hub: RuntimeLogHub,
    pub(crate) events: Arc<dyn RuntimeEvents>,
    pub(crate) store: Arc<dyn RuntimeStore>,
    pub(crate) status: GetComfyStatus,
    pub(crate) start: StartComfy,
    pub(crate) stop: StopComfy,
    pub(crate) restart: RestartComfy,
    pub(crate) get_options: GetLaunchOptions,
    pub(crate) set_options: SetLaunchOptions,
    pub(crate) versions: ListComfyVersions,
    pub(crate) update: UpdateComfy,
    pub(crate) extra_paths: ReadExtraModelPaths,
    pub(crate) manage_extra_paths: ManageExtraModelPaths,
    pub(crate) subscriptions: Mutex<HashMap<u32, tauri::async_runtime::JoinHandle<()>>>,
}

impl ComfyServices {
    fn new(
        environment: EnvironmentDeps,
        adapters: RuntimeAdapters,
        nodes: Arc<dyn NodeLifecycle>,
    ) -> Self {
        let hub = adapters.hub;
        let manage_extra_paths =
            ManageExtraModelPaths::new(environment.root.clone(), adapters.extra_paths_writer);
        let deps = RuntimeDeps {
            environment,
            nodes,
            supervisor: adapters.supervisor.clone(),
            store: adapters.store.clone(),
            repository: adapters.repository,
            ports: adapters.ports,
            preflight: adapters.preflight,
            extra_paths: adapters.extra_paths,
        };
        Self {
            supervisor: adapters.supervisor,
            events: Arc::new(hub.clone()),
            store: adapters.store.clone(),
            status: GetComfyStatus::new(deps.clone()),
            start: StartComfy::new(deps.clone()),
            stop: StopComfy::new(deps.clone()),
            restart: RestartComfy::new(deps.clone()),
            get_options: GetLaunchOptions::new(adapters.store.clone()),
            set_options: SetLaunchOptions::new(adapters.store),
            versions: ListComfyVersions::new(deps.clone()),
            update: UpdateComfy::new(deps.clone()),
            extra_paths: ReadExtraModelPaths::new(deps),
            manage_extra_paths,
            hub,
            subscriptions: Mutex::default(),
        }
    }
}

/// Download use cases, the installation-scoped queue supervisor and the live subscriptions.
pub struct DownloadServices {
    pub(crate) stack: DownloadStack,
    pub(crate) resolve: ResolveDownload,
    pub(crate) enqueue: EnqueueDownloads,
    pub(crate) resume: ResumeJob,
    pub(crate) pause: PauseJob,
    pub(crate) cancel: CancelJob,
    pub(crate) retry: RetryJob,
    pub(crate) list: ListJobs,
    pub(crate) import: ImportPack,
    pub(crate) export: ExportResourceList,
    pub(crate) test_credential: TestCredential,
    pub(crate) last_test: Arc<dyn CredentialTester>,
    pub(crate) ids: RandomIds,
    pub(crate) subscriptions: Mutex<HashMap<u32, tauri::async_runtime::JoinHandle<()>>>,
    run: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
}

impl DownloadServices {
    pub fn new(stack: DownloadStack) -> Self {
        let repository: Arc<dyn JobRepository> = stack.repository.clone();
        let files: Arc<dyn DownloadFiles> = stack.files.clone();
        let resolver: Arc<dyn SourceResolver> = stack.resolver.clone();
        let packs: Arc<dyn ModelPacks> = stack.packs.clone();
        let tester: Arc<dyn CredentialTester> = stack.tester.clone();
        let resume = || ResumeJob {
            repository: repository.clone(),
            files: files.clone(),
            events: stack.hub.clone(),
            control: stack.supervisor.clone(),
        };
        Self {
            resolve: ResolveDownload {
                resolver: resolver.clone(),
            },
            enqueue: EnqueueDownloads {
                resolver: resolver.clone(),
                repository: repository.clone(),
                files: files.clone(),
                events: stack.hub.clone(),
                control: stack.supervisor.clone(),
            },
            resume: resume(),
            pause: PauseJob {
                control: stack.supervisor.clone(),
            },
            cancel: CancelJob {
                control: stack.supervisor.clone(),
            },
            retry: RetryJob { resume: resume() },
            list: ListJobs {
                repository: repository.clone(),
            },
            import: ImportPack {
                packs: packs.clone(),
                resolver,
            },
            export: ExportResourceList { packs },
            test_credential: TestCredential {
                tester: tester.clone(),
            },
            last_test: tester,
            ids: RandomIds::default(),
            subscriptions: Mutex::default(),
            run: Mutex::default(),
            stack,
        }
    }

    /// Starts the queue. Interrupted work is reconciled before any job is admitted.
    pub fn start(&self) {
        let supervisor = self.stack.supervisor.clone();
        let handle = tauri::async_runtime::spawn(async move {
            if let Err(error) = supervisor.run().await {
                tracing::error!(?error, "the download supervisor stopped");
            }
        });
        *self
            .run
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(handle);
    }

    /// Pauses running transfers and waits for their cleanup. Paused work resumes at next start.
    pub async fn shutdown(&self) {
        self.stack.supervisor.request_shutdown();
        let handle = self
            .run
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        if let Some(handle) = handle {
            // Keep ownership until transfer/process cleanup has finished. Dropping a timed-out
            // join handle would detach writers while the installer replaces the application.
            let _ = handle.await;
        }
    }
}

/// Model library use cases, the installation-scoped scan supervisor and the live subscriptions.
pub struct LibraryServices {
    pub(crate) stack: LibraryStack,
    pub(crate) list: ListModels,
    pub(crate) rescan: RescanModels,
    pub(crate) details: GetModelDetails,
    pub(crate) rename: RenameModel,
    pub(crate) move_model: MoveModel,
    pub(crate) plan_delete: PlanDelete,
    pub(crate) delete: DeleteModels,
    pub(crate) delete_permanently: DeleteModelsPermanently,
    pub(crate) identify: IdentifyModel,
    pub(crate) operations: GetOperationStatus,
    reconcile: ReconcileModelOperations,
    pub(crate) status: Arc<GetLibraryStatus>,
    pub(crate) epoch: String,
    pub(crate) subscriptions: Mutex<HashMap<u32, tauri::async_runtime::JoinHandle<()>>>,
    next_subscription: AtomicU32,
    run: Mutex<Option<LibraryRun>>,
}

struct LibraryRun {
    handle: tauri::async_runtime::JoinHandle<()>,
    shutdown: tokio::sync::oneshot::Sender<()>,
}

impl LibraryServices {
    pub fn new(stack: LibraryStack) -> Self {
        let deps = stack.deps.clone();
        Self {
            list: ListModels::new(deps.clone()),
            rescan: RescanModels::new(deps.clone()),
            details: GetModelDetails::new(deps.clone()),
            rename: RenameModel::new(deps.clone()),
            move_model: MoveModel::new(deps.clone()),
            plan_delete: PlanDelete::new(deps.clone()),
            delete: DeleteModels::new(deps.clone()),
            delete_permanently: DeleteModelsPermanently::new(deps.clone()),
            identify: IdentifyModel::new(deps.clone()),
            operations: GetOperationStatus::new(deps.clone()),
            reconcile: ReconcileModelOperations::new(deps.clone()),
            status: Arc::new(GetLibraryStatus::new(deps)),
            epoch: IdGenerator::new_id(&RandomIds::default()),
            subscriptions: Mutex::default(),
            next_subscription: AtomicU32::new(1),
            run: Mutex::default(),
            stack,
        }
    }

    pub(crate) fn next_subscription(&self) -> u32 {
        self.next_subscription.fetch_add(1, Ordering::SeqCst)
    }

    /// Finishes interrupted file operations. Runs before downloads may publish.
    pub async fn reconcile(&self) {
        match self.reconcile.execute().await {
            Ok(report) => tracing::info!(?report, "model operation recovery finished"),
            Err(error) => tracing::error!(?error, "model operation recovery failed"),
        }
    }

    /// Starts the scan loop and file watching.
    pub fn start(&self) {
        let mut run = self.run.lock().unwrap_or_else(|p| p.into_inner());
        if run.is_some() {
            return;
        }
        let supervisor = self.stack.supervisor.clone();
        let (shutdown, stopped) = tokio::sync::oneshot::channel();
        let handle = tauri::async_runtime::spawn(supervisor.run_until(stopped));
        *run = Some(LibraryRun { handle, shutdown });
    }

    /// Finishes any in-flight scan, then drops the watcher before returning.
    pub async fn shutdown(&self) {
        let run = self.run.lock().unwrap_or_else(|p| p.into_inner()).take();
        if let Some(run) = run {
            let _ = run.shutdown.send(());
            let _ = run.handle.await;
        }
    }
}

/// A running node operation registered for cancellation. Dropping it unregisters the key.
pub struct ActiveOperation<'a> {
    services: &'a NodeServices,
    key: String,
    pub(crate) cancel: CancelToken,
}

impl Drop for ActiveOperation<'_> {
    fn drop(&mut self) {
        self.services
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&self.key);
    }
}

/// True when a broadcast receiver fell behind and events were dropped.
pub(crate) fn subscription_gap(error: &tokio::sync::broadcast::error::RecvError) -> bool {
    matches!(error, tokio::sync::broadcast::error::RecvError::Lagged(_))
}

/// Custom-node use cases, the event hub and the live subscriptions.
pub struct NodeServices {
    pub(crate) stack: NodeStack,
    pub(crate) service: NodeService,
    active: Mutex<HashMap<String, CancelToken>>,
    pub(crate) subscriptions: Mutex<HashMap<u32, tauri::async_runtime::JoinHandle<()>>>,
    next_subscription: AtomicU32,
}

struct NotifyingNodeLifecycle {
    service: Arc<dyn NodeLifecycle>,
    hub: Arc<NodeHub>,
}

#[async_trait::async_trait]
impl NodeLifecycle for NotifyingNodeLifecycle {
    async fn before_start(&self, lease: &mut dyn MaintenanceLease) -> Result<(), RuntimeError> {
        self.service.before_start(lease).await
    }

    async fn after_stop(&self, lease: &mut dyn MaintenanceLease) -> Result<(), RuntimeError> {
        let result = self.service.after_stop(lease).await;
        self.hub.publish(NodeEvent::Changed);
        result
    }
}

impl NodeServices {
    pub fn new(stack: NodeStack) -> Self {
        Self {
            service: NodeService::new(stack.deps.clone()),
            stack,
            active: Mutex::default(),
            subscriptions: Mutex::default(),
            next_subscription: AtomicU32::new(1),
        }
    }

    pub(crate) fn next_subscription(&self) -> u32 {
        self.next_subscription.fetch_add(1, Ordering::SeqCst)
    }

    /// Tells subscribers to refetch the installed nodes and operations.
    pub(crate) fn changed(&self) {
        self.stack.hub.publish(NodeEvent::Changed);
    }

    /// Registers `key`; a second operation under the same key is refused.
    pub(crate) fn begin(&self, key: &str) -> Result<ActiveOperation<'_>, IpcError> {
        let cancel = CancelToken::new();
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if active.contains_key(key) {
            return Err(IpcError::new(
                "node_operation_running",
                "That operation is already running.",
            ));
        }
        active.insert(key.to_owned(), cancel.clone());
        Ok(ActiveOperation {
            services: self,
            key: key.to_owned(),
            cancel,
        })
    }

    pub(crate) fn context(&self, operation: &ActiveOperation<'_>) -> NodeContext {
        NodeContext {
            cancel: operation.cancel.clone(),
            sink: Arc::new(HubSink(self.stack.hub.clone())),
        }
    }

    /// Cancels a running operation; false when nothing runs under `key`.
    pub(crate) fn cancel(&self, key: &str) -> bool {
        match self
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(key)
        {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        }
    }

    pub(crate) async fn prepare(
        &self,
        key: &str,
        request: PrepareRequest,
    ) -> Result<PlanReviewDto, IpcError> {
        let guard = self.begin(key)?;
        let ctx = self.context(&guard);
        let result = self.service.prepare(request, &ctx).await;
        self.changed();
        Ok(PlanReviewDto::from(&result?))
    }

    /// Enable, disable and uninstall: prepare and commit under one lease with the user's click as
    /// the confirmation.
    pub(crate) async fn direct(&self, request: PrepareRequest) -> Result<NodeOutcomeDto, IpcError> {
        let key = format!("direct-{}", self.next_subscription());
        let guard = self.begin(&key)?;
        let ctx = self.context(&guard);
        let result = self.service.run_direct(request, &ctx).await;
        self.changed();
        Ok(NodeOutcomeDto::from(&result?))
    }

    /// Reconcile and scan before normal services are exposed at startup. A failure is logged
    /// and never blocks the app: a launch still re-checks the journal under its own lease.
    pub async fn reconcile(&self) {
        let result = self.service.rescan().await;
        self.changed();
        match result {
            Ok(_) => tracing::info!("custom node recovery and scan finished"),
            Err(error) => tracing::error!(%error, "custom node recovery or scan needs attention"),
        }
    }
}

/// Use cases that depend on the install root's database.
pub struct Services {
    pub(crate) gate: Arc<dyn MaintenanceGate>,
    pub(crate) curated: Arc<dyn CuratedNodeCatalog>,
    pub(crate) get_settings: GetSettings,
    pub(crate) update_settings: UpdateSettings,
    pub(crate) setup: SetupServices,
    pub(crate) detect_hardware: DetectHardware,
    pub(crate) list_profiles: ListProfiles,
    pub(crate) validate_python: ValidatePython,
    pub(crate) select_root: SelectInstallRoot,
    pub(crate) env_info: GetEnvironmentInfo,
    pub(crate) list_packages: ListPackages,
    pub(crate) preview_pip: PreviewPackageChange,
    pub(crate) apply_pip: ApplyPackageChange,
    pub(crate) delete_env: DeleteEnvironment,
    pub(crate) root: RootInfo,
    pub(crate) logs_dir: PathBuf,
    pub(crate) comfy: ComfyServices,
    pub(crate) downloads: DownloadServices,
    pub(crate) library: LibraryServices,
    pub(crate) nodes: NodeServices,
    pub(crate) install_update: InstallAppUpdate,
    pub(crate) diagnostics: CollectDiagnostics,
}

impl Services {
    /// Starts the model scan loop (the production root starts it after recovery).
    pub fn start_library(&self) {
        self.library.start();
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        deps: EnvironmentDeps,
        settings: Arc<dyn SettingsRepository>,
        root: RootInfo,
        logs_dir: PathBuf,
        runtime: RuntimeAdapters,
        downloads: DownloadStack,
        library: LibraryStack,
        nodes: NodeStack,
    ) -> Self {
        let nodes = NodeServices::new(nodes);
        let node_lifecycle = Arc::new(NotifyingNodeLifecycle {
            service: Arc::new(nodes.service.clone()),
            hub: nodes.stack.hub.clone(),
        });
        let diagnostics = CollectDiagnostics::new(
            RuntimeDeps {
                environment: deps.clone(),
                nodes: node_lifecycle.clone(),
                supervisor: runtime.supervisor.clone(),
                store: runtime.store.clone(),
                repository: runtime.repository.clone(),
                ports: runtime.ports.clone(),
                preflight: runtime.preflight.clone(),
                extra_paths: runtime.extra_paths.clone(),
            },
            Arc::new(FileLogTail::new(&logs_dir)),
            Arc::new(runtime.hub.clone()),
            env!("CARGO_PKG_VERSION"),
            // Pinned and verified when provisioned, not probed at report time.
            vec![
                PinnedTool {
                    name: "uv".to_owned(),
                    version: pins::UV_VERSION.to_owned(),
                },
                PinnedTool {
                    name: "hf".to_owned(),
                    version: pins::HF_VERSION.to_owned(),
                },
                PinnedTool {
                    name: "gdown".to_owned(),
                    version: pins::GDOWN_VERSION.to_owned(),
                },
            ],
        );
        let install_update = InstallAppUpdate::new(&deps);
        // First-run setup installs its curated nodes through the same node service.
        let setup_nodes: Arc<dyn SetupNodes> = Arc::new(nodes.service.clone());
        Self {
            gate: deps.gate.clone(),
            curated: deps.curated.clone(),
            diagnostics,
            install_update,
            comfy: ComfyServices::new(deps.clone(), runtime, node_lifecycle),
            downloads: DownloadServices::new(downloads),
            library: LibraryServices::new(library),
            nodes,
            get_settings: GetSettings::new(settings.clone()),
            update_settings: UpdateSettings::new(settings),
            setup: SetupServices::new(deps.clone(), setup_nodes, root.clone(), logs_dir.clone()),
            detect_hardware: DetectHardware::new(deps.clone()),
            list_profiles: ListProfiles::new(deps.clone()),
            validate_python: ValidatePython::new(deps.clone()),
            select_root: SelectInstallRoot::new(deps.clone()),
            env_info: GetEnvironmentInfo::new(deps.clone()),
            list_packages: ListPackages::new(deps.clone()),
            preview_pip: PreviewPackageChange::new(deps.clone()),
            apply_pip: ApplyPackageChange::new(deps.clone()),
            delete_env: DeleteEnvironment::new(deps),
            root,
            logs_dir,
        }
    }
}

/// The state every command receives.
pub struct AppState {
    services: Result<Services, RecoveryInfo>,
    recovery_setup: Option<SetupServices>,
    pub(crate) gate: Arc<dyn MaintenanceGate>,
    opener: Option<Arc<dyn ExistingInstallOpener>>,
    pub(crate) set_credential: SetCredential,
    pub(crate) clear_credential: ClearCredential,
    pub(crate) get_credential_status: GetCredentialStatus,
    pub(crate) ingress: SecretIngress,
    pub(crate) credentials: Arc<dyn CredentialStore + Send + Sync>,
    pub(crate) picker: Arc<dyn FilePicker>,
    pub(crate) data_dir: PathBuf,
    pub(crate) close: Arc<CloseCoordinator>,
    shell: OnceLock<Arc<dyn ShellActions>>,
    updater: OnceLock<Arc<dyn AppUpdater>>,
    clipboard: OnceLock<Arc<dyn Clipboard>>,
}

impl AppState {
    pub fn new(
        services: Result<Services, RecoveryInfo>,
        credentials: Arc<dyn CredentialStore + Send + Sync>,
        ingress: SecretIngress,
        picker: Arc<dyn FilePicker>,
        data_dir: PathBuf,
    ) -> Self {
        let gate = services
            .as_ref()
            .map(|services| services.gate.clone())
            .unwrap_or_else(|_| Arc::new(TokioMaintenanceGate::new()));
        Self {
            services,
            recovery_setup: None,
            gate,
            opener: None,
            set_credential: SetCredential::new(credentials.clone()),
            clear_credential: ClearCredential::new(credentials.clone()),
            get_credential_status: GetCredentialStatus::new(credentials.clone()),
            ingress,
            credentials,
            picker,
            data_dir,
            close: Arc::new(CloseCoordinator::default()),
            shell: OnceLock::new(),
            updater: OnceLock::new(),
            clipboard: OnceLock::new(),
        }
    }

    /// The root-dependent use cases, or the recovery error every command returns when the
    /// app started without a usable install root.
    pub(crate) fn services(&self) -> Result<&Services, IpcError> {
        self.services.as_ref().map_err(IpcError::recovery)
    }

    pub(crate) fn setup_services(&self) -> Result<&SetupServices, IpcError> {
        match &self.services {
            Ok(services) => Ok(&services.setup),
            Err(info) => self
                .recovery_setup
                .as_ref()
                .ok_or_else(|| IpcError::recovery(info)),
        }
    }

    pub fn install_opener(&mut self, opener: Arc<dyn ExistingInstallOpener>) {
        self.opener = Some(opener);
    }

    pub(crate) fn opener(&self) -> Result<&Arc<dyn ExistingInstallOpener>, IpcError> {
        self.opener
            .as_ref()
            .ok_or_else(|| IpcError::new("root_switch_failed", "Root recovery is unavailable."))
    }

    fn configure_opener(
        &mut self,
        root: Option<RootInfo>,
        old_root: Option<PathBuf>,
    ) -> Result<(), RootError> {
        self.install_opener(Arc::new(FsExistingInstallOpener::new(
            root,
            old_root,
            bootstrap_path()?,
            self.gate.clone(),
        )));
        Ok(())
    }

    /// Installs the window actions once the application handle exists.
    pub fn install_shell(&self, shell: Arc<dyn ShellActions>) {
        let _ = self.shell.set(shell);
    }

    /// Installs the signed updater once the application handle exists.
    pub fn install_updater(&self, updater: Arc<dyn AppUpdater>) {
        let _ = self.updater.set(updater);
    }

    /// The installed updater, or one that refuses everything as not configured.
    pub(crate) fn updater(&self) -> Arc<dyn AppUpdater> {
        self.updater
            .get()
            .cloned()
            .unwrap_or_else(|| Arc::new(SignedUpdater::disabled()))
    }

    /// Installs the system clipboard once the application handle exists.
    pub fn install_clipboard(&self, clipboard: Arc<dyn Clipboard>) {
        let _ = self.clipboard.set(clipboard);
    }

    /// The installed clipboard, or one that reports every copy as failed.
    pub(crate) fn clipboard(&self) -> Arc<dyn Clipboard> {
        self.clipboard
            .get()
            .cloned()
            .unwrap_or_else(|| Arc::new(UnavailableClipboard))
    }

    pub(crate) fn shell(&self) -> Result<&Arc<dyn ShellActions>, IpcError> {
        self.shell
            .get()
            .ok_or_else(|| IpcError::new("window_unavailable", "The window system is not ready."))
    }

    /// Pauses the download queue and waits for transfer cleanup. A no-op in recovery mode.
    pub async fn shutdown_downloads(&self) {
        if let Ok(services) = self.services() {
            services.downloads.shutdown().await;
        }
    }

    pub async fn shutdown_library(&self) {
        if let Ok(services) = self.services() {
            services.library.shutdown().await;
        }
    }

    /// Library runs use fresh stop channels; the download supervisor is single-use.
    pub fn restore_library(&self) {
        if let Ok(services) = self.services() {
            services.library.start();
        }
    }

    pub(crate) fn recovery(&self) -> Option<&RecoveryInfo> {
        self.services.as_ref().err()
    }

    /// Opens the production adapters for a resolved install root and reconciles interrupted
    /// work before any command is exposed.
    pub async fn compose(
        resolved: &ResolvedRoot,
        picker: Arc<dyn FilePicker>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        if let Some(error) = bootstrap_root_error(resolved) {
            return Self::compose_recovery(RecoveryInfo::from_root_error(&error), picker).await;
        }
        let paths = AppPaths::from_root(&resolved.root);
        let installation = Arc::new(FsInstallationProbe::new());
        let mut manifest = FsManifestStore::new().read(&resolved.root).await?;
        // Only a moved root restricts the app, decided by comparing roots alone: an interrupted
        // activation is rolled forward and a broken interpreter is offered a rebuild by the
        // normal start, so neither may lock the app out.
        let mut restricted = matches!(
            &manifest,
            ManifestState::Valid(manifest) if root_moved(&resolved.root, &manifest.install_root)
        );
        if !restricted {
            paths.ensure_dirs()?;
        }
        let db = Db::open(&paths.db_path).await?;
        let ingress = SecretIngress::default();
        let credentials = Arc::new(KeyringCredentialStore::new(ingress.clone()).await?);
        let settings: Arc<dyn SettingsRepository> =
            Arc::new(SqliteSettingsRepository::new(db.clone()));
        let store = Arc::new(SqliteEnvironmentStore::new(db.clone()));
        let download_db = db.clone();
        let node_adapters = NodeAdapters::open(&resolved.root, db.clone(), NodeOptions::default());
        let node_requirements = Arc::new(NodeRequirements::new(
            node_adapters.store.clone(),
            node_adapters.files.clone(),
        ));
        let relocation = Arc::new(SqliteRelocationStore::new(db.clone()));
        let runtime_store = Arc::new(SqliteRuntimeStore::new(db));
        let hub = RuntimeLogHub::default();
        let supervisor = Arc::new(ComfyRuntimeSupervisor::new(
            resolved.root.clone(),
            Arc::new(SharedRuntimeRunner::new()),
            runtime_store.clone(),
            hub.clone(),
        ));
        let source = Arc::new(GitSource::new());
        let verifier = Arc::new(PythonVerifier::new());
        let root = RootInfo {
            root: resolved.root.clone(),
            source: resolved.source,
        };
        let deps = EnvironmentDeps {
            installation,
            relocation,
            relocation_runtime: Arc::new(RelocationRuntimeGuard),
            hardware: Arc::new(SystemHardware::new()),
            catalog: Arc::new(BundledProfiles::load()?),
            curated: Arc::new(BundledCuratedNodes::load()?),
            tools: Arc::new(Tools::new()),
            python: Arc::new(PythonAdapter::new()),
            packages: Arc::new(UvPackages::new()),
            wheels: Arc::new(infrastructure::environment::wheels::Wheels::new()),
            verifier: verifier.clone(),
            source: source.clone(),
            node_requirements,
            envs: store.clone(),
            runs: store.clone(),
            activation: store.clone(),
            manifests: Arc::new(FsManifestStore::new()),
            drafts: store,
            root: Arc::new(FsRootSwitcher::new(
                root.clone(),
                bootstrap_path()?,
                settings.clone(),
            )),
            gate: Arc::new(TokioMaintenanceGate::new()),
            runtime: supervisor.clone(),
            clock: Arc::new(SystemClock),
            ids: Arc::new(RandomIds::default()),
            active_runs: Arc::new(ActiveRuns::default()),
            plans: Arc::new(PlanStore::default()),
        };
        // Interrupted runs become retryable and half-finished activations roll forward before
        // any maintenance command can run. After a move only the runs bound to the current root
        // are reconciled; the copied ones stay as history.
        match ReconcileStartup::new(deps.clone()).execute().await {
            Ok(report) => tracing::info!(?report, "startup reconciliation finished"),
            Err(error) => tracing::error!(%error, "startup reconciliation failed"),
        }
        if restricted {
            // A reconciled activation may have published this root's manifest.
            manifest = FsManifestStore::new().read(&resolved.root).await?;
            restricted = matches!(
                &manifest,
                ManifestState::Valid(manifest)
                    if root_moved(&resolved.root, &manifest.install_root)
            );
            if !restricted {
                paths.ensure_dirs()?;
            }
        }
        if restricted {
            let nodes =
                node_adapters.bind(deps.clone(), IdGenerator::new_id(&RandomIds::default()));
            let setup_nodes: Arc<dyn SetupNodes> = Arc::new(NodeService::new(nodes.deps));
            let info = RecoveryInfo {
                code: "rebuild_required".into(),
                message: "The installation moved or its interpreter is broken. Rebuild the environment to continue.".into(),
                root: Some(root.clone()),
            };
            let mut state = Self::new(Err(info), credentials, ingress, picker, paths.data_dir);
            state.gate = deps.gate.clone();
            state.recovery_setup = Some(SetupServices::new(
                deps,
                setup_nodes,
                root.clone(),
                paths.logs_dir,
            ));
            let old_root = match manifest {
                ManifestState::Valid(manifest) => Some(manifest.install_root),
                _ => None,
            };
            state.configure_opener(Some(root), old_root)?;
            return Ok(state);
        }
        let runtime = RuntimeAdapters {
            supervisor,
            store: runtime_store,
            repository: Arc::new(GitRepository::new(source)),
            ports: Arc::new(TcpPortProbe),
            preflight: Arc::new(PythonLaunchPreflight::new(verifier)),
            extra_paths: Arc::new(YamlExtraModelPaths),
            extra_paths_writer: Arc::new(YamlExtraModelPaths),
            hub,
        };
        let saved = match settings.load().await {
            Ok(saved) => saved.unwrap_or_default(),
            Err(_) => Settings::default(),
        };
        let locks = path_locks();
        // A root that was never set up has no ComfyUI yet. Setup clones it first, so nothing may
        // be created under `ComfyUI/` before then; the app restarts once setup has finished.
        let defer_models = matches!(manifest, ManifestState::Absent)
            && matches!(
                std::fs::symlink_metadata(resolved.root.join("ComfyUI")),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound
            );
        let downloads = DownloadStack::open_with_bandwidth(
            &resolved.root,
            download_db.clone(),
            credentials.clone(),
            saved.download_concurrency.get() as usize,
            locks.clone(),
            saved.bandwidth_cap_kib_s,
            !defer_models,
        )
        .map_err(|error| format!("downloads could not be opened: {error}"))?;
        let library = LibraryStack::open(
            &resolved.root,
            download_db,
            credentials.clone(),
            locks,
            runtime.extra_paths.clone(),
            downloads.repository.clone(),
            !defer_models,
        )
        .map_err(|error| format!("the model library could not be opened: {error}"))?;
        let nodes = node_adapters.bind(deps.clone(), IdGenerator::new_id(&RandomIds::default()));
        let services = Services::new(
            deps,
            settings,
            root,
            paths.logs_dir.clone(),
            runtime,
            downloads,
            library,
            nodes,
        );
        // Interrupted renames and moves are finished before any download may publish.
        services.library.reconcile().await;
        // Reconcile interrupted node work and observe Manager/manual changes before commands.
        // Setup reconciles under its own lease once ComfyUI exists.
        if defer_models {
            tracing::info!("custom node scan deferred until ComfyUI is installed");
        } else {
            services.nodes.reconcile().await;
        }
        services.library.start();
        services.downloads.start();
        let mut state = Self::new(Ok(services), credentials, ingress, picker, paths.data_dir);
        state.configure_opener(Some(services_root(resolved)), None)?;
        Ok(state)
    }

    /// The state for a start without a usable root: credentials still work, everything that
    /// needs the install root answers with recovery guidance.
    pub async fn compose_recovery(
        info: RecoveryInfo,
        picker: Arc<dyn FilePicker>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let ingress = SecretIngress::default();
        let credentials = Arc::new(KeyringCredentialStore::new(ingress.clone()).await?);
        let root = info.root.clone();
        let mut state = Self::new(Err(info), credentials, ingress, picker, PathBuf::new());
        // Without a configuration folder the opener stays unset and root recovery reports
        // `root_switch_failed`; the rest of recovery must still start.
        if let Err(error) = state.configure_opener(root.clone(), root.map(|root| root.root)) {
            tracing::warn!(%error, "root recovery is unavailable");
        }
        Ok(state)
    }
}

/// Why a bootstrap-selected install root cannot be composed; `None` when it is a directory.
fn bootstrap_root_error(resolved: &ResolvedRoot) -> Option<RootError> {
    if resolved.source != RootSource::Bootstrap {
        return None;
    }
    match std::fs::metadata(&resolved.root) {
        Ok(metadata) if metadata.is_dir() => None,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Some(RootError::RootMissing(resolved.root.clone()))
        }
        _ => Some(RootError::RootInaccessible(resolved.root.clone())),
    }
}

/// The log folder to open before composing, so reconciliation reports and open failures are
/// logged. A missing or inaccessible bootstrap-selected root gets none: nothing is created there.
pub fn startup_logs_dir(resolved: &ResolvedRoot) -> Option<PathBuf> {
    bootstrap_root_error(resolved)
        .is_none()
        .then(|| AppPaths::from_root(&resolved.root).logs_dir)
}

fn services_root(resolved: &ResolvedRoot) -> RootInfo {
    RootInfo {
        root: resolved.root.clone(),
        source: resolved.source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use application::shared::MaintenanceGate;

    struct LifecycleResult(Result<(), RuntimeError>);

    #[async_trait::async_trait]
    impl NodeLifecycle for LifecycleResult {
        async fn before_start(&self, _: &mut dyn MaintenanceLease) -> Result<(), RuntimeError> {
            self.0.clone()
        }

        async fn after_stop(&self, _: &mut dyn MaintenanceLease) -> Result<(), RuntimeError> {
            self.0.clone()
        }
    }

    #[tokio::test]
    async fn node_lifecycle_publishes_changed_after_stop_even_on_failure() {
        for result in [Ok(()), Err(RuntimeError::NodeRescanFailed)] {
            let hub = NodeHub::new("test".into());
            let mut receiver = hub.subscribe();
            let lifecycle = NotifyingNodeLifecycle {
                service: Arc::new(LifecycleResult(result.clone())),
                hub,
            };
            let gate = TokioMaintenanceGate::new();
            let mut lease = gate.acquire().await.unwrap();
            assert_eq!(lifecycle.before_start(&mut *lease).await, result);
            assert!(receiver.try_recv().is_err());
            assert_eq!(lifecycle.after_stop(&mut *lease).await, result);
            assert_eq!(receiver.try_recv().unwrap().1, NodeEvent::Changed);
        }
    }
}

#[cfg(test)]
#[path = "state/recovery_tests.rs"]
mod recovery_tests;
