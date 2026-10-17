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
        ComfyRepository, ExtraModelPaths, GetComfyDiagnostics, GetComfyStatus, GetLaunchOptions,
        LaunchPreflight, ListComfyVersions, NodeLifecycle, PortProbe, ReadExtraModelPaths,
        RestartComfy, RuntimeDeps, RuntimeError, RuntimeEvents, RuntimeStore, RuntimeSupervisor,
        SetLaunchOptions, StartComfy, StopComfy, UpdateComfy,
    },
    custom_nodes::{NodeContext, NodeService, PrepareRequest},
    environment::{
        ActiveRuns, ApplyPackageChange, CancelSetup, DeleteEnvironment, DetectHardware,
        EnvironmentDeps, ExecuteSetup, GetEnvironmentInfo, GetSetupStatus, ListPackages,
        ListProfiles, PlanStore, PreviewPackageChange, ReconcileStartup, RootInfo,
        SelectInstallRoot, StartSetup, ValidatePython,
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
    },
    shared::{CancelToken, IdGenerator, MaintenanceLease},
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
    custom_nodes::stack::{HubSink, NodeEvent, NodeHub, NodeStack},
    environment::{
        hardware::SystemHardware, manifest::FsManifestStore, packages::UvPackages,
        profiles::BundledProfiles, python::PythonAdapter, source::GitSource,
        sqlite::SqliteEnvironmentStore, switcher::FsRootSwitcher, tools::Tools,
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
        paths::{AppPaths, ResolvedRoot, RootError, bootstrap_path},
    },
};

use crate::{
    app_lifecycle::{CloseCoordinator, ShellActions},
    ipc::{
        error::IpcError,
        node_dto::{NodeOutcomeDto, PlanReviewDto},
    },
    picker::FilePicker,
};

/// Why the app started without an install root. Only recovery guidance is available then.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryInfo {
    pub code: String,
    pub message: String,
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
        };
        Self {
            code: code.to_owned(),
            message: message.to_owned(),
        }
    }

    pub fn startup_failed() -> Self {
        Self {
            code: "startup_failed".to_owned(),
            message:
                "The app could not open its database or logs. Check the install folder and restart."
                    .to_owned(),
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
    pub hub: RuntimeLogHub,
}

/// ComfyUI runtime use cases and the live subscription registry.
pub struct ComfyServices {
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
    pub(crate) diagnostics: GetComfyDiagnostics,
    pub(crate) subscriptions: Mutex<HashMap<u32, tauri::async_runtime::JoinHandle<()>>>,
}

impl ComfyServices {
    fn new(
        environment: EnvironmentDeps,
        adapters: RuntimeAdapters,
        nodes: Arc<dyn NodeLifecycle>,
    ) -> Self {
        let hub = adapters.hub;
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
            extra_paths: ReadExtraModelPaths::new(deps.clone()),
            diagnostics: GetComfyDiagnostics::new(deps, Arc::new(hub.clone())),
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
        if let Some(handle) = handle
            && tokio::time::timeout(std::time::Duration::from_secs(15), handle)
                .await
                .is_err()
        {
            tracing::warn!("downloads did not stop within 15 seconds");
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
    run: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
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
        let supervisor = self.stack.supervisor.clone();
        let handle = tauri::async_runtime::spawn(supervisor.run());
        *self
            .run
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(handle);
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
    pub(crate) get_settings: GetSettings,
    pub(crate) update_settings: UpdateSettings,
    pub(crate) setup_status: GetSetupStatus,
    pub(crate) detect_hardware: DetectHardware,
    pub(crate) list_profiles: ListProfiles,
    pub(crate) validate_python: ValidatePython,
    pub(crate) select_root: SelectInstallRoot,
    pub(crate) start_setup: StartSetup,
    pub(crate) execute_setup: Arc<ExecuteSetup>,
    pub(crate) cancel_setup: CancelSetup,
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
        Self {
            comfy: ComfyServices::new(deps.clone(), runtime, node_lifecycle),
            downloads: DownloadServices::new(downloads),
            library: LibraryServices::new(library),
            nodes,
            get_settings: GetSettings::new(settings.clone()),
            update_settings: UpdateSettings::new(settings),
            setup_status: GetSetupStatus::new(deps.clone()),
            detect_hardware: DetectHardware::new(deps.clone()),
            list_profiles: ListProfiles::new(deps.clone()),
            validate_python: ValidatePython::new(deps.clone()),
            select_root: SelectInstallRoot::new(deps.clone()),
            start_setup: StartSetup::new(deps.clone()),
            execute_setup: Arc::new(ExecuteSetup::new(deps.clone())),
            cancel_setup: CancelSetup::new(deps.clone()),
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
    pub(crate) set_credential: SetCredential,
    pub(crate) clear_credential: ClearCredential,
    pub(crate) get_credential_status: GetCredentialStatus,
    pub(crate) ingress: SecretIngress,
    pub(crate) credentials: Arc<dyn CredentialStore + Send + Sync>,
    pub(crate) picker: Arc<dyn FilePicker>,
    pub(crate) data_dir: PathBuf,
    pub(crate) close: Arc<CloseCoordinator>,
    shell: OnceLock<Arc<dyn ShellActions>>,
}

impl AppState {
    pub fn new(
        services: Result<Services, RecoveryInfo>,
        credentials: Arc<dyn CredentialStore + Send + Sync>,
        ingress: SecretIngress,
        picker: Arc<dyn FilePicker>,
        data_dir: PathBuf,
    ) -> Self {
        Self {
            services,
            set_credential: SetCredential::new(credentials.clone()),
            clear_credential: ClearCredential::new(credentials.clone()),
            get_credential_status: GetCredentialStatus::new(credentials.clone()),
            ingress,
            credentials,
            picker,
            data_dir,
            close: Arc::new(CloseCoordinator::default()),
            shell: OnceLock::new(),
        }
    }

    /// The root-dependent use cases, or the recovery error every command returns when the
    /// app started without a usable install root.
    pub(crate) fn services(&self) -> Result<&Services, IpcError> {
        self.services.as_ref().map_err(IpcError::recovery)
    }

    /// Installs the window actions once the application handle exists.
    pub fn install_shell(&self, shell: Arc<dyn ShellActions>) {
        let _ = self.shell.set(shell);
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

    pub(crate) fn recovery(&self) -> Option<&RecoveryInfo> {
        self.services.as_ref().err()
    }

    /// Opens the production adapters for a resolved install root and reconciles interrupted
    /// work before any command is exposed.
    pub async fn compose(
        resolved: &ResolvedRoot,
        picker: Arc<dyn FilePicker>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let paths = AppPaths::from_root(&resolved.root);
        paths.ensure_dirs()?;
        let db = Db::open(&paths.db_path).await?;
        let ingress = SecretIngress::default();
        let credentials = Arc::new(KeyringCredentialStore::new(ingress.clone()).await?);
        let settings: Arc<dyn SettingsRepository> =
            Arc::new(SqliteSettingsRepository::new(db.clone()));
        let store = Arc::new(SqliteEnvironmentStore::new(db.clone()));
        let download_db = db.clone();
        let node_db = db.clone();
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
            hardware: Arc::new(SystemHardware::new()),
            catalog: Arc::new(BundledProfiles::load()?),
            tools: Arc::new(Tools::new()),
            python: Arc::new(PythonAdapter::new()),
            packages: Arc::new(UvPackages::new()),
            verifier: verifier.clone(),
            source: source.clone(),
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
        // any maintenance command can run.
        match ReconcileStartup::new(deps.clone()).execute().await {
            Ok(report) => tracing::info!(?report, "startup reconciliation finished"),
            Err(error) => tracing::error!(%error, "startup reconciliation failed"),
        }
        let runtime = RuntimeAdapters {
            supervisor,
            store: runtime_store,
            repository: Arc::new(GitRepository::new(source)),
            ports: Arc::new(TcpPortProbe),
            preflight: Arc::new(PythonLaunchPreflight::new(verifier)),
            extra_paths: Arc::new(YamlExtraModelPaths),
            hub,
        };
        let saved = match settings.load().await {
            Ok(saved) => saved.unwrap_or_default(),
            Err(_) => Settings::default(),
        };
        let locks = path_locks();
        let downloads = DownloadStack::open_with_bandwidth(
            &resolved.root,
            download_db.clone(),
            credentials.clone(),
            saved.download_concurrency.get() as usize,
            locks.clone(),
            saved.bandwidth_cap_kib_s,
        )
        .map_err(|error| format!("downloads could not be opened: {error}"))?;
        let library = LibraryStack::open(
            &resolved.root,
            download_db,
            credentials.clone(),
            locks,
            runtime.extra_paths.clone(),
            downloads.repository.clone(),
        )
        .map_err(|error| format!("the model library could not be opened: {error}"))?;
        let nodes = NodeStack::open(
            &resolved.root,
            node_db,
            deps.clone(),
            IdGenerator::new_id(&RandomIds::default()),
        );
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
        services.nodes.reconcile().await;
        services.library.start();
        services.downloads.start();
        Ok(Self::new(
            Ok(services),
            credentials,
            ingress,
            picker,
            paths.data_dir,
        ))
    }

    /// The state for a start without a usable root: credentials still work, everything that
    /// needs the install root answers with recovery guidance.
    pub async fn compose_recovery(
        info: RecoveryInfo,
        picker: Arc<dyn FilePicker>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let ingress = SecretIngress::default();
        let credentials = Arc::new(KeyringCredentialStore::new(ingress.clone()).await?);
        Ok(Self::new(
            Err(info),
            credentials,
            ingress,
            picker,
            PathBuf::new(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Lease;
    impl MaintenanceLease for Lease {}

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
            assert_eq!(lifecycle.before_start(&mut Lease).await, result);
            assert!(receiver.try_recv().is_err());
            assert_eq!(lifecycle.after_stop(&mut Lease).await, result);
            assert_eq!(receiver.try_recv().unwrap().1, NodeEvent::Changed);
        }
    }
}
