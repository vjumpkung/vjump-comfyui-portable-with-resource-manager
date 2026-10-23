//! Installation-scoped supervisor, backed by the shared owned-process runner.
use super::{
    http::SystemStatsProbe,
    logs::{RuntimeLogHub, SessionLog},
    port::TcpPortProbe,
};
use crate::{
    environment::layout::{Layout, Tool, command, venv_python},
    shared::{
        process::{self, CommandSpec, RunLimits},
        procscan::{self, SnapshotFn},
    },
};
use application::{
    comfy_runtime::*,
    environment::{EnvVerifier, NullSink, RunId, StepContext, StepId},
    shared::{CancelToken, MaintenanceLease},
};
use async_trait::async_trait;
use domain::{
    comfy_runtime::*,
    environment::{DistName, PythonEnvironment, VerifyExpectations},
};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{oneshot, watch};

/// The bridge to M1's existing Job Object/process-group runner. Implementations must never
/// create a second process owner, and must not return until output is drained and the entire
/// tree is verified gone (or cleanup_verified=false). The future stays owned by the core.
#[async_trait]
pub trait RuntimeProcessRunner: Send + Sync {
    async fn run(
        &self,
        spec: CommandSpec,
        observer: Arc<dyn RuntimeProcessObserver>,
        cancel: CancelToken,
    ) -> ProcessCompletion;
}
pub trait OwnedRuntimeProcess: Send + Sync {
    fn identity(&self) -> ProcessIdentity;
    fn is_alive(&self) -> Result<bool, RuntimeError>;
    /// Verifies membership in the actual live Job Object/group, never command-line similarity.
    fn contains_pid(&self, pid: u32) -> Result<bool, RuntimeError>;
}
pub trait RuntimeProcessObserver: Send + Sync {
    fn spawned(&self, process: Arc<dyn OwnedRuntimeProcess>);
    /// Raw pipe bytes, before any lossy newline conversion. M2 redacts complete records.
    fn chunk(&self, stderr: bool, bytes: &[u8]);
}
#[derive(Debug, Clone)]
pub struct ProcessCompletion {
    pub exit_code: Option<i32>,
    pub spawn_failed: bool,
    pub cleanup_verified: bool,
}

/// Concrete M1 runner bridge: the existing runner creates, owns, drains and terminates the tree.
#[derive(Default)]
pub struct SharedRuntimeRunner;
impl SharedRuntimeRunner {
    pub fn new() -> Self {
        Self
    }
}
struct SharedOwned {
    scope: Arc<process::ProcessScope>,
    identity: ProcessIdentity,
}
impl OwnedRuntimeProcess for SharedOwned {
    fn identity(&self) -> ProcessIdentity {
        self.identity.clone()
    }
    fn is_alive(&self) -> Result<bool, RuntimeError> {
        self.scope
            .is_alive()
            .map_err(|_| RuntimeError::Failed("owned process liveness is inaccessible".into()))
    }
    fn contains_pid(&self, pid: u32) -> Result<bool, RuntimeError> {
        self.scope.contains_pid(pid).map_err(|_| {
            RuntimeError::Failed("owned process scope membership is inaccessible".into())
        })
    }
}
struct SharedObserver {
    target: Arc<dyn RuntimeProcessObserver>,
    cancel: CancelToken,
}
impl process::ProcessObserver for SharedObserver {
    fn spawned(&self, scope: Arc<process::ProcessScope>) {
        let Ok(Some(created)) = super::inventory::creation_identity(scope.pid()) else {
            self.cancel.cancel();
            return;
        };
        let identity = ProcessIdentity {
            pid: scope.pid(),
            scope: format!(
                "{}:{}:{created}",
                if cfg!(windows) { "job" } else { "group" },
                scope.pid()
            ),
            created,
        };
        self.target
            .spawned(Arc::new(SharedOwned { scope, identity }));
    }
    fn chunk(&self, stderr: bool, bytes: &[u8]) {
        self.target.chunk(stderr, bytes);
    }
}
#[async_trait]
impl RuntimeProcessRunner for SharedRuntimeRunner {
    async fn run(
        &self,
        spec: CommandSpec,
        observer: Arc<dyn RuntimeProcessObserver>,
        cancel: CancelToken,
    ) -> ProcessCompletion {
        let observer = SharedObserver {
            target: observer,
            cancel: cancel.clone(),
        };
        let limits = RunLimits {
            max_stdout_bytes: 0,
            ..Default::default()
        };
        match process::run_observed(&spec, &process::DiscardSink, &cancel, &limits, &observer).await
        {
            Ok(output) => ProcessCompletion {
                exit_code: Some(output.exit_code),
                spawn_failed: false,
                cleanup_verified: true,
            },
            Err(process::RunError::Spawn(_) | process::RunError::InputFailed { .. }) => {
                ProcessCompletion {
                    exit_code: None,
                    spawn_failed: true,
                    cleanup_verified: true,
                }
            }
            Err(process::RunError::Failed { exit_code, .. }) => ProcessCompletion {
                exit_code,
                spawn_failed: false,
                cleanup_verified: true,
            },
            Err(process::RunError::Cancelled { .. } | process::RunError::TimedOut { .. }) => {
                ProcessCompletion {
                    exit_code: None,
                    spawn_failed: false,
                    cleanup_verified: true,
                }
            }
            Err(process::RunError::CleanupFailed { .. }) => ProcessCompletion {
                exit_code: None,
                spawn_failed: false,
                cleanup_verified: false,
            },
        }
    }
}

struct Observer {
    owned: Mutex<Option<Arc<dyn OwnedRuntimeProcess>>>,
    log: Arc<SessionLog>,
    spawned: tokio::sync::Notify,
}
impl Observer {
    fn process(&self) -> Option<Arc<dyn OwnedRuntimeProcess>> {
        self.owned.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}
impl RuntimeProcessObserver for Observer {
    fn spawned(&self, process: Arc<dyn OwnedRuntimeProcess>) {
        *self.owned.lock().unwrap_or_else(|p| p.into_inner()) = Some(process);
        self.spawned.notify_one();
    }
    fn chunk(&self, stderr: bool, bytes: &[u8]) {
        self.log.chunk(stderr, bytes);
    }
}
#[derive(Clone)]
struct Active {
    id: SessionId,
    cancel: CancelToken,
    done: watch::Receiver<Option<Result<RuntimeStatus, RuntimeError>>>,
}
enum LaunchArguments {
    Ready(Vec<String>),
    Preflight {
        env: Box<PythonEnvironment>,
        preflight: Arc<dyn LaunchPreflight>,
    },
}
struct StartTask {
    session: RuntimeSession,
    arguments: LaunchArguments,
    lease: Box<dyn MaintenanceLease>,
    cancel: CancelToken,
    started: oneshot::Sender<Result<RuntimeStatus, RuntimeError>>,
    done: watch::Sender<Option<Result<RuntimeStatus, RuntimeError>>>,
}
struct Inner {
    root: PathBuf,
    runner: Arc<dyn RuntimeProcessRunner>,
    store: Arc<dyn RuntimeStore>,
    hub: RuntimeLogHub,
    active: Mutex<Option<Active>>,
    quarantine: Mutex<Option<Box<dyn MaintenanceLease>>>,
    readiness_timeout: Duration,
    /// The process-list source for `inventory`; always the real one outside tests.
    snapshot: SnapshotFn,
}
#[derive(Clone)]
pub struct ComfyRuntimeSupervisor {
    inner: Arc<Inner>,
}
impl ComfyRuntimeSupervisor {
    pub fn new(
        root: PathBuf,
        runner: Arc<dyn RuntimeProcessRunner>,
        store: Arc<dyn RuntimeStore>,
        hub: RuntimeLogHub,
    ) -> Self {
        Self::with_readiness_timeout(root, runner, store, hub, Duration::from_secs(120))
    }
    pub fn with_readiness_timeout(
        root: PathBuf,
        runner: Arc<dyn RuntimeProcessRunner>,
        store: Arc<dyn RuntimeStore>,
        hub: RuntimeLogHub,
        readiness_timeout: Duration,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                root,
                runner,
                store,
                hub,
                active: Mutex::new(None),
                quarantine: Mutex::new(None),
                readiness_timeout,
                snapshot: procscan::snapshot,
            }),
        }
    }
    fn active(&self) -> Option<Active> {
        self.inner
            .active
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
    async fn launch(
        &self,
        session: RuntimeSession,
        arguments: LaunchArguments,
        lease: Box<dyn MaintenanceLease>,
    ) -> Result<RuntimeStatus, RuntimeError> {
        let (started_tx, started_rx) = oneshot::channel();
        let (done_tx, done_rx) = watch::channel(None);
        let cancel = CancelToken::new();
        {
            let mut active = self.inner.active.lock().unwrap_or_else(|p| p.into_inner());
            if active.is_some() {
                return Err(RuntimeError::NeedsStop);
            }
            self.inner.hub.set_status(RuntimeStatus {
                lifecycle: Lifecycle::Starting,
                session: Some(session.clone()),
                pending_operation: Some("Starting".into()),
                ..Default::default()
            });
            *active = Some(Active {
                id: session.id.clone(),
                cancel: cancel.clone(),
                done: done_rx,
            });
        }
        let this = self.clone();
        tokio::spawn(async move {
            this.run_session(StartTask {
                session,
                arguments,
                lease,
                cancel,
                started: started_tx,
                done: done_tx,
            })
            .await;
        });
        started_rx
            .await
            .map_err(|_| RuntimeError::Failed("runtime supervisor task stopped".into()))?
    }
    fn publish(
        &self,
        id: &SessionId,
        lifecycle: Lifecycle,
        failure: Option<RuntimeFailure>,
    ) -> bool {
        let active = self.inner.active.lock().unwrap_or_else(|p| p.into_inner());
        if active.as_ref().is_some_and(|a| {
            &a.id == id && (lifecycle != Lifecycle::Running || !a.cancel.is_cancelled())
        }) {
            let mut status = self.inner.hub.status();
            status.lifecycle = lifecycle;
            status.failure = failure;
            status.pending_operation =
                matches!(lifecycle, Lifecycle::Starting | Lifecycle::Stopping)
                    .then(|| format!("{lifecycle:?}"));
            self.inner.hub.set_status(status);
            return true;
        }
        false
    }
    async fn run_session(&self, task: StartTask) {
        let StartTask {
            mut session,
            arguments,
            lease,
            cancel,
            started,
            done,
        } = task;
        let mut lease = Some(lease);
        let id = session.id.clone();
        let prepare = async {
            session.checkout = tokio::fs::canonicalize(&session.checkout)
                .await
                .map_err(|_| RuntimeError::Failed("ComfyUI checkout is unavailable".into()))?;
            // Keep the venv interpreter spelling: resolving its Unix symlink to the base Python
            // would bypass venv selection. Validate existence and absolute location instead.
            if !session.interpreter.is_absolute() || !session.interpreter.is_file() {
                return Err(RuntimeError::EnvironmentNotUsable);
            }
            self.inner.store.save_session(&session, false).await?;
            let log = Arc::new(SessionLog::open(
                &self.inner.root.join("data/logs"),
                &session,
                self.inner.hub.clone(),
            )?);
            let args = match arguments {
                LaunchArguments::Ready(args) => args,
                LaunchArguments::Preflight { env, preflight } => {
                    let capabilities = preflight
                        .capabilities(&self.inner.root, &env, &cancel)
                        .await?;
                    session.options.args(cfg!(windows), &capabilities)?
                }
            };
            if cancel.is_cancelled() {
                return Err(RuntimeError::Cancelled);
            }
            if !TcpPortProbe
                .available(session.options.port, session.options.listen_lan)
                .await?
            {
                return Err(RuntimeError::PortConflict {
                    requested: session.options.port,
                    suggested: TcpPortProbe
                        .next_free(session.options.port, session.options.listen_lan)
                        .await?,
                });
            }
            Ok((log, args))
        }
        .await;
        let (log, args) = match prepare {
            Ok(value) => value,
            Err(error) => {
                let cancelled = matches!(error, RuntimeError::Cancelled) || cancel.is_cancelled();
                self.publish(
                    &id,
                    if cancelled {
                        Lifecycle::Stopped
                    } else {
                        Lifecycle::Crashed
                    },
                    (!cancelled).then(|| failure(FailureKind::Spawn, None, &error.to_string())),
                );
                let _ = self.inner.store.save_session(&session, true).await;
                let result = if cancelled {
                    Ok(self.inner.hub.status())
                } else {
                    Err(error)
                };
                self.finish_active(&id);
                drop(lease);
                let _ = started.send(result.clone());
                let _ = done.send(Some(result));
                return;
            }
        };
        let observer = Arc::new(Observer {
            owned: Mutex::new(None),
            log: log.clone(),
            spawned: tokio::sync::Notify::new(),
        });
        let mut spec = command(
            &Layout::new(&self.inner.root),
            Tool::Python,
            &session.interpreter,
        )
        .cwd(&session.checkout)
        .args(args);
        spec.env.push(("PYTHONUNBUFFERED".into(), "1".into()));
        let runner = self.inner.runner.clone();
        let runner_observer = observer.clone();
        let runner_cancel = cancel.clone();
        let mut running =
            tokio::spawn(async move { runner.run(spec, runner_observer, runner_cancel).await });
        enum First {
            Ready(Result<(), ReadinessError>),
            Exited(ProcessCompletion),
        }
        let first = {
            let readiness = self.readiness(&mut session, &observer, &cancel);
            tokio::pin!(readiness);
            tokio::select! {
                value=&mut readiness=>First::Ready(value),
                result=&mut running=>First::Exited(completion(result)),
            }
        };
        let mut started = Some(started);
        let (outcome, mut failed) = match first {
            First::Ready(Ok(())) => {
                // A stop may race the last readiness check; never publish Running after Stopping.
                if !self.publish(&id, Lifecycle::Running, None) {
                    (
                        completion(running.await),
                        Some(failure(FailureKind::Cancelled, None, "start cancelled")),
                    )
                } else {
                    lease.take();
                    let _ = started
                        .take()
                        .expect("start response exists")
                        .send(Ok(self.inner.hub.status()));
                    (completion(running.await), None)
                }
            }
            First::Ready(Err(error)) => {
                let was_cancelled = cancel.is_cancelled();
                cancel.cancel();
                let outcome = completion(running.await);
                let (kind, message) = match error {
                    _ if was_cancelled => (FailureKind::Cancelled, "start cancelled".to_owned()),
                    ReadinessError::Runtime(RuntimeError::Cancelled) => {
                        (FailureKind::Cancelled, "start cancelled".to_owned())
                    }
                    ReadinessError::Runtime(RuntimeError::Storage) => {
                        (FailureKind::Persistence, RuntimeError::Storage.to_string())
                    }
                    ReadinessError::Runtime(other) => {
                        (FailureKind::ReadinessTimeout, other.to_string())
                    }
                    ReadinessError::Exited => (
                        if outcome.spawn_failed {
                            FailureKind::Spawn
                        } else {
                            FailureKind::ExitBeforeReady
                        },
                        "ComfyUI exited before readiness".to_owned(),
                    ),
                    ReadinessError::TimedOut => (
                        FailureKind::ReadinessTimeout,
                        "ComfyUI readiness timed out".to_owned(),
                    ),
                };
                (
                    outcome.clone(),
                    Some(failure(kind, outcome.exit_code, &message)),
                )
            }
            First::Exited(outcome) => {
                // A stop that wins the race against the runner's exit is a cancellation, not a crash.
                let (kind, message) = if outcome.spawn_failed {
                    (FailureKind::Spawn, "ComfyUI exited before readiness")
                } else if cancel.is_cancelled() {
                    (FailureKind::Cancelled, "start cancelled")
                } else {
                    (
                        FailureKind::ExitBeforeReady,
                        "ComfyUI exited before readiness",
                    )
                };
                let code = outcome.exit_code;
                (outcome, Some(failure(kind, code, message)))
            }
        };
        if !outcome.cleanup_verified {
            failed = Some(failure(
                FailureKind::Termination,
                outcome.exit_code,
                "process-tree termination could not be verified; maintenance remains blocked",
            ));
            // A failed start retains its maintenance lease, rather than releasing it on an
            // unverified termination. Running-runtime failure is blocked by inventory/status.
            if let Some(lease) = lease.take() {
                *self
                    .inner
                    .quarantine
                    .lock()
                    .unwrap_or_else(|p| p.into_inner()) = Some(lease);
            }
        }
        if let Err(error) = log.finish()
            && failed.is_none()
        {
            failed = Some(failure(
                FailureKind::Persistence,
                outcome.exit_code,
                &error.to_string(),
            ));
        }
        if outcome.cleanup_verified && self.inner.store.save_session(&session, true).await.is_err()
        {
            failed = Some(failure(
                FailureKind::Persistence,
                outcome.exit_code,
                "session cleanup could not be persisted",
            ));
        }
        let stopped = cancel.is_cancelled()
            && outcome.cleanup_verified
            && failed
                .as_ref()
                .is_none_or(|f| f.kind == FailureKind::Cancelled);
        if stopped {
            self.publish(&id, Lifecycle::Stopped, None);
        } else {
            let failed = failed.get_or_insert_with(|| {
                failure(
                    FailureKind::UnexpectedExit,
                    outcome.exit_code,
                    "ComfyUI exited unexpectedly",
                )
            });
            self.publish(&id, Lifecycle::Crashed, Some(failed.clone()));
        }
        let status = self.inner.hub.status();
        let result = if stopped {
            Ok(status)
        } else {
            Err(RuntimeError::Failed(status.failure.as_ref().map_or_else(
                || "ComfyUI stopped".into(),
                |f| f.message.clone(),
            )))
        };
        self.finish_active(&id);
        drop(lease);
        if let Some(started) = started {
            let _ = started.send(result.clone());
        }
        let _ = done.send(Some(result));
    }
    async fn readiness(
        &self,
        session: &mut RuntimeSession,
        observer: &Observer,
        cancel: &CancelToken,
    ) -> Result<(), ReadinessError> {
        let probe = SystemStatsProbe::new()?;
        let deadline = tokio::time::Instant::now() + self.inner.readiness_timeout;
        let mut published = false;
        loop {
            if cancel.is_cancelled() {
                return Err(RuntimeError::Cancelled.into());
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(ReadinessError::TimedOut);
            }
            if let Some(owned) = observer.process() {
                if !owned.is_alive()? {
                    return Err(ReadinessError::Exited);
                }
                if !published {
                    session.identity = Some(owned.identity());
                    self.inner.store.save_session(session, false).await?;
                    {
                        let active = self.inner.active.lock().unwrap_or_else(|p| p.into_inner());
                        if active.as_ref().is_some_and(|a| a.id == session.id) {
                            let mut status = self.inner.hub.status();
                            status.session = Some(session.clone());
                            self.inner.hub.set_status(status);
                        }
                    }
                    published = true;
                }
                let port = session.options.port;
                let before = owned_listener(owned.as_ref(), port);
                if before
                    && probe.ready(port).await
                    && owned.is_alive()?
                    && owned_listener(owned.as_ref(), port)
                {
                    return Ok(());
                }
            }
            tokio::select! {
                _=tokio::time::sleep(Duration::from_millis(250))=>{},
                _=observer.spawned.notified()=>{},
            }
        }
    }
    fn finish_active(&self, id: &SessionId) {
        let mut active = self.inner.active.lock().unwrap_or_else(|p| p.into_inner());
        if active.as_ref().is_some_and(|a| &a.id == id) {
            *active = None;
        }
    }
}
enum ReadinessError {
    TimedOut,
    Exited,
    Runtime(RuntimeError),
}
impl From<RuntimeError> for ReadinessError {
    fn from(error: RuntimeError) -> Self {
        Self::Runtime(error)
    }
}
fn failure(kind: FailureKind, exit_code: Option<i32>, message: &str) -> RuntimeFailure {
    RuntimeFailure {
        kind,
        exit_code,
        message: crate::shared::logging::redact(message).into_owned(),
    }
}
fn completion(result: Result<ProcessCompletion, tokio::task::JoinError>) -> ProcessCompletion {
    result.unwrap_or(ProcessCompletion {
        exit_code: None,
        spawn_failed: false,
        cleanup_verified: false,
    })
}
fn owned_listener(owned: &dyn OwnedRuntimeProcess, port: Port) -> bool {
    let Ok(pids) = super::inventory::listener_pids(port) else {
        return false;
    };
    !pids.is_empty()
        && pids
            .into_iter()
            .all(|pid| owned.contains_pid(pid).unwrap_or(false))
}
#[async_trait]
impl RuntimeMaintenance for ComfyRuntimeSupervisor {
    async fn ensure_stopped(&self, root: &Path) -> Result<(), RuntimeBlock> {
        if !application::environment::same_location(root, &self.inner.root) {
            return Err(RuntimeBlock::CannotAssess);
        }
        match self.inventory(root).await {
            Ok(InventoryState::Stopped) => Ok(()),
            Ok(InventoryState::OwnedRunning | InventoryState::VerifiedSurvivor(_)) => {
                Err(RuntimeBlock::Running)
            }
            _ => Err(RuntimeBlock::CannotAssess),
        }
    }
}
#[async_trait]
impl RuntimeSupervisor for ComfyRuntimeSupervisor {
    async fn status(&self) -> RuntimeStatus {
        self.inner.hub.status()
    }
    async fn inventory(&self, root: &Path) -> Result<InventoryState, RuntimeError> {
        if !application::environment::same_location(root, &self.inner.root) {
            return Ok(InventoryState::Unknown(
                "runtime belongs to another install root".into(),
            ));
        }
        if self.active().is_some() {
            return Ok(InventoryState::OwnedRunning);
        }
        if self
            .inner
            .quarantine
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some()
            || self
                .inner
                .hub
                .status()
                .failure
                .is_some_and(|f| f.kind == FailureKind::Termination)
        {
            return Ok(InventoryState::Unknown(
                "runtime termination is unverified".into(),
            ));
        }
        super::inventory::inspect(root, self.inner.store.as_ref(), self.inner.snapshot).await
    }
    async fn start(
        &self,
        session: RuntimeSession,
        args: Vec<String>,
        lease: Box<dyn MaintenanceLease>,
    ) -> Result<RuntimeStatus, RuntimeError> {
        self.launch(session, LaunchArguments::Ready(args), lease)
            .await
    }
    async fn start_with_preflight(
        &self,
        session: RuntimeSession,
        env: PythonEnvironment,
        preflight: Arc<dyn LaunchPreflight>,
        lease: Box<dyn MaintenanceLease>,
    ) -> Result<RuntimeStatus, RuntimeError> {
        self.launch(
            session,
            LaunchArguments::Preflight {
                env: Box::new(env),
                preflight,
            },
            lease,
        )
        .await
    }
    async fn stop(&self) -> Result<RuntimeStatus, RuntimeError> {
        let Some(mut active) = self.active() else {
            return match self.inventory(&self.inner.root).await? {
                InventoryState::Stopped => Ok(self.inner.hub.status()),
                _ => Err(RuntimeError::RecoveryRequired(
                    "the surviving process cannot be safely terminated by this core".into(),
                )),
            };
        };
        active.cancel.cancel();
        self.publish(&active.id, Lifecycle::Stopping, None);
        loop {
            if let Some(result) = active.done.borrow().clone() {
                return result;
            }
            if active.done.changed().await.is_err() {
                return Err(RuntimeError::RecoveryRequired(
                    "runtime cleanup lost its completion signal".into(),
                ));
            }
        }
    }
}

/// CLI and actual-env capability validation. No package installation is performed at launch.
pub struct PythonLaunchPreflight {
    verifier: Arc<dyn EnvVerifier>,
}
impl PythonLaunchPreflight {
    pub fn new(verifier: Arc<dyn EnvVerifier>) -> Self {
        Self { verifier }
    }
}
#[async_trait]
impl LaunchPreflight for PythonLaunchPreflight {
    async fn capabilities(
        &self,
        root: &Path,
        env: &PythonEnvironment,
        cancel: &CancelToken,
    ) -> Result<LaunchCapabilities, RuntimeError> {
        let torch = env
            .pins
            .get(&DistName::new("torch").expect("static name"))
            .ok_or(RuntimeError::EnvironmentNotUsable)?;
        let cpu = torch.ends_with("+cpu");
        let ctx = StepContext {
            run_id: RunId::new("runtime-preflight").expect("static id"),
            step: StepId::FinalVerify,
            sink: Arc::new(NullSink),
            cancel: cancel.clone(),
            allow_source_builds: false,
            retained: Default::default(),
        };
        let expectations = VerifyExpectations {
            torch_version_prefix: torch.into(),
            require_cuda: !cpu,
            require_triton: env
                .pins
                .iter()
                .any(|(n, _)| matches!(n.as_str(), "triton" | "triton-windows")),
        };
        self.verifier
            .verify_final(root, &env.path, &expectations, &ctx)
            .await?;
        let layout = Layout::new(root);
        let python = venv_python(&env.path);
        let limits = RunLimits {
            timeout: Some(Duration::from_secs(30)),
            max_stdout_bytes: 1 << 20,
            ..Default::default()
        };
        let help = command(&layout, Tool::Python, &python)
            .cwd(layout.comfyui_dir())
            .args(["-s", "main.py", "--help"]);
        let result = process::run(&help, &process::DiscardSink, cancel, &limits)
            .await
            .map_err(|_| {
                RuntimeError::Failed("the selected checkout's CLI preflight failed".into())
            })?;
        if result.stdout.len() >= limits.max_stdout_bytes {
            return Err(RuntimeError::Failed(
                "CLI help exceeded the capture limit".into(),
            ));
        }
        let options = parse_options(&result.stdout);
        let script = r#"import importlib,json
def available(name):
    try:
        importlib.import_module(name)
        return True
    except Exception:
        return False
print('VJ-CAPS:'+json.dumps({name:available(name) for name in ['sageattention','flash_attn','comfyui_manager']}))
"#;
        let probe = command(&layout, Tool::Python, &python)
            .cwd(layout.comfyui_dir())
            .args(["-s", "-c", script]);
        let output = process::run(&probe, &process::DiscardSink, cancel, &limits)
            .await
            .map_err(|_| {
                RuntimeError::Failed("installed launch capabilities could not be verified".into())
            })?;
        let caps: serde_json::Value = output
            .stdout
            .lines()
            .find_map(|l| l.strip_prefix("VJ-CAPS:"))
            .and_then(|s| serde_json::from_str(s).ok())
            .ok_or(RuntimeError::InvalidRecord)?;
        let flag = |name: &str| {
            caps.get(name)
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        };
        Ok(LaunchCapabilities {
            options,
            sage: flag("sageattention"),
            flash: flag("flash_attn"),
            builtin_manager: flag("comfyui_manager"),
            legacy_manager: layout
                .comfyui_dir()
                .join("custom_nodes/ComfyUI-Manager")
                .exists(),
            cpu,
        })
    }
}
fn parse_options(help: &str) -> std::collections::BTreeSet<String> {
    help.match_indices("--")
        .filter_map(|(start, _)| {
            let name: String = help[start..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
                .collect();
            (name.len() > 2).then_some(name)
        })
        .collect()
}

#[cfg(test)]
mod tests;
