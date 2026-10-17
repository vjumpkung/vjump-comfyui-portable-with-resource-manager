use super::*;
use crate::{
    environment::{InstallManifest, ManifestState, RunId, StepContext},
    shared::{CancelToken, GateError, MaintenanceGate, MaintenanceLease},
    test_support::block_on,
};
use async_trait::async_trait;
use domain::{
    comfy_runtime::*,
    environment::{EnvState, PythonEnvironment, PythonVersion},
};
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
#[allow(dead_code, clippy::duplicate_mod)]
#[path = "../environment/tests/fakes.rs"]
mod env_fakes;
use env_fakes::World;

struct Machine {
    env: Arc<World>,
    status: Mutex<RuntimeStatus>,
    options: Mutex<LaunchOptions>,
    update: Mutex<Option<UpdateRun>>,
    revision: Mutex<Revision>,
    repo: Mutex<RepoState>,
    inventory_checked: AtomicBool,
    mutation_saw_repair: AtomicBool,
    node_journal_failure: AtomicBool,
    node_rescan_failure: AtomicBool,
    stop_held: Mutex<Vec<bool>>,
    hold_start: AtomicBool,
    start_lease: Mutex<Option<Box<dyn MaintenanceLease>>>,
    start_waker: Mutex<Option<std::task::Waker>>,
}
impl Machine {
    fn new() -> Arc<Self> {
        let env = World::new();
        let active = env.seed_active_env("selected", PythonVersion::new(3, 13, 16));
        *env.manifest.lock().unwrap() = ManifestState::Valid(InstallManifest {
            schema_version: 1,
            install_root: env_fakes::ROOT.into(),
            generation: 1,
            env_id: active.id,
            env_path: active.path,
            ownership: active.ownership,
            python_version: active.python_version,
            profile_id: active.profile_id,
            profile_revision: active.profile_revision,
            comfyui_reference: Some("v1".into()),
            comfyui_commit: Some("a".repeat(40)),
            run_id: RunId::new("setup-1").unwrap(),
            activated_at: "t".into(),
        });
        Arc::new(Self {
            env,
            status: Mutex::default(),
            options: Mutex::default(),
            update: Mutex::default(),
            revision: Mutex::new(Revision {
                commit: "a".repeat(40),
                tag: Some("v1".into()),
                branch: None,
            }),
            repo: Mutex::default(),
            inventory_checked: AtomicBool::new(false),
            mutation_saw_repair: AtomicBool::new(false),
            node_journal_failure: AtomicBool::new(false),
            node_rescan_failure: AtomicBool::new(false),
            stop_held: Mutex::default(),
            hold_start: AtomicBool::new(false),
            start_lease: Mutex::default(),
            start_waker: Mutex::default(),
        })
    }
    fn deps(self: &Arc<Self>) -> RuntimeDeps {
        RuntimeDeps {
            environment: self.env.deps(),
            nodes: self.clone(),
            supervisor: self.clone(),
            store: self.clone(),
            repository: self.clone(),
            ports: self.clone(),
            preflight: self.clone(),
            extra_paths: self.clone(),
        }
    }
    fn request() -> UpdateRequest {
        UpdateRequest {
            target: UpdateTarget::Tag(Tag::new("v2").unwrap()),
            stop_running: false,
            retry: None,
        }
    }
}
#[async_trait]
impl NodeLifecycle for Machine {
    async fn before_start(&self, _: &mut dyn MaintenanceLease) -> Result<(), RuntimeError> {
        assert!(self.env.gate.busy.load(Ordering::SeqCst));
        self.env.call("nodes_before_start");
        if self.node_journal_failure.load(Ordering::SeqCst) {
            Err(RuntimeError::NodeRecoveryRequired)
        } else {
            Ok(())
        }
    }
    async fn after_stop(&self, _: &mut dyn MaintenanceLease) -> Result<(), RuntimeError> {
        assert!(self.env.gate.busy.load(Ordering::SeqCst));
        assert_eq!(self.status.lock().unwrap().lifecycle, Lifecycle::Stopped);
        self.env.call("nodes_after_stop");
        if self.node_rescan_failure.load(Ordering::SeqCst) {
            Err(RuntimeError::NodeRescanFailed)
        } else {
            Ok(())
        }
    }
}
#[async_trait]
impl RuntimeStore for Machine {
    async fn launch_options(&self) -> Result<LaunchOptions, RuntimeError> {
        Ok(self.options.lock().unwrap().clone())
    }
    async fn save_launch_options(&self, o: &LaunchOptions) -> Result<(), RuntimeError> {
        *self.options.lock().unwrap() = o.clone();
        Ok(())
    }
    async fn save_session(&self, _: &RuntimeSession, _: bool) -> Result<(), RuntimeError> {
        Ok(())
    }
    async fn unfinished_sessions(&self) -> Result<Vec<RuntimeSession>, RuntimeError> {
        Ok(Vec::new())
    }
    async fn save_update(&self, r: &UpdateRun) -> Result<(), RuntimeError> {
        assert!(self.env.gate.busy.load(Ordering::SeqCst));
        if r.phase == UpdatePhase::Dependencies {
            self.mutation_saw_repair.store(
                self.env.env("selected").state() == EnvState::RepairRequired,
                Ordering::SeqCst,
            );
        }
        *self.update.lock().unwrap() = Some(r.clone());
        Ok(())
    }
    async fn unfinished_update(&self) -> Result<Option<UpdateRun>, RuntimeError> {
        Ok(self
            .update
            .lock()
            .unwrap()
            .clone()
            .filter(|r| r.phase != UpdatePhase::Complete))
    }
}
#[async_trait]
impl RuntimeMaintenance for Machine {
    async fn ensure_stopped(&self, _: &Path) -> Result<(), RuntimeBlock> {
        if self.status.lock().unwrap().lifecycle == Lifecycle::Running {
            Err(RuntimeBlock::Running)
        } else {
            Ok(())
        }
    }
}
#[async_trait]
impl RuntimeSupervisor for Machine {
    async fn status(&self) -> RuntimeStatus {
        self.status.lock().unwrap().clone()
    }
    async fn inventory(&self, _: &Path) -> Result<InventoryState, RuntimeError> {
        assert!(
            self.env.gate.busy.load(Ordering::SeqCst),
            "gate must precede process inspection"
        );
        self.inventory_checked.store(true, Ordering::SeqCst);
        self.env.call("runtime_inventory");
        Ok(
            if self.status.lock().unwrap().lifecycle == Lifecycle::Running {
                InventoryState::OwnedRunning
            } else {
                InventoryState::Stopped
            },
        )
    }
    async fn start(
        &self,
        s: RuntimeSession,
        _: Vec<String>,
        lease: Box<dyn MaintenanceLease>,
    ) -> Result<RuntimeStatus, RuntimeError> {
        assert!(self.env.gate.busy.load(Ordering::SeqCst));
        self.env.call("runtime_start");
        if self.hold_start.load(Ordering::SeqCst) {
            self.status.lock().unwrap().lifecycle = Lifecycle::Starting;
            *self.start_lease.lock().unwrap() = Some(lease);
            return std::future::poll_fn(|cx| {
                if self.start_lease.lock().unwrap().is_none() {
                    std::task::Poll::Ready(Err(RuntimeError::Cancelled))
                } else {
                    *self.start_waker.lock().unwrap() = Some(cx.waker().clone());
                    std::task::Poll::Pending
                }
            })
            .await;
        }
        let status = RuntimeStatus {
            lifecycle: Lifecycle::Running,
            session: Some(s),
            ..Default::default()
        };
        *self.status.lock().unwrap() = status.clone();
        Ok(status)
    }
    async fn stop(&self) -> Result<RuntimeStatus, RuntimeError> {
        self.env.call("runtime_stop");
        self.stop_held
            .lock()
            .unwrap()
            .push(self.env.gate.busy.load(Ordering::SeqCst));
        self.start_lease.lock().unwrap().take();
        if let Some(waker) = self.start_waker.lock().unwrap().take() {
            waker.wake();
        }
        let mut status = self.status.lock().unwrap();
        status.lifecycle = Lifecycle::Stopped;
        Ok(status.clone())
    }
}

struct AcquisitionHookGate {
    inner: Arc<env_fakes::FakeGate>,
    before_acquire: Option<Box<dyn Fn() + Send + Sync>>,
    before_try_acquire: Option<Box<dyn Fn() + Send + Sync>>,
}

#[async_trait]
impl MaintenanceGate for AcquisitionHookGate {
    async fn acquire(&self) -> Result<Box<dyn MaintenanceLease>, GateError> {
        if let Some(hook) = &self.before_acquire {
            hook();
        }
        self.inner.acquire().await
    }

    async fn try_acquire(&self) -> Result<Box<dyn MaintenanceLease>, GateError> {
        if let Some(hook) = &self.before_try_acquire {
            hook();
        }
        self.inner.try_acquire().await
    }
}

#[test]
fn start_checks_node_recovery_under_the_maintenance_lease() {
    let m = Machine::new();
    m.node_journal_failure.store(true, Ordering::SeqCst);
    assert_eq!(
        block_on(StartComfy::new(m.deps()).execute()),
        Err(RuntimeError::NodeRecoveryRequired)
    );
    assert_eq!(m.env.count("nodes_before_start"), 1);
    assert!(!m.inventory_checked.load(Ordering::SeqCst));
    assert_eq!(m.env.count("runtime_start"), 0);
    assert!(!m.env.gate.busy.load(Ordering::SeqCst));

    m.node_journal_failure.store(false, Ordering::SeqCst);
    block_on(StartComfy::new(m.deps()).execute()).unwrap();
    assert!(m.env.position("nodes_before_start") < m.env.position("runtime_inventory"));
    assert!(m.env.position("runtime_inventory") < m.env.position("runtime_start"));
}

#[test]
fn a_journal_failure_created_before_lease_acquisition_blocks_start() {
    let m = Machine::new();
    let mut deps = m.deps();
    let journal = m.clone();
    deps.environment.gate = Arc::new(AcquisitionHookGate {
        inner: m.env.gate.clone(),
        before_acquire: None,
        before_try_acquire: Some(Box::new(move || {
            assert!(!journal.env.gate.busy.load(Ordering::SeqCst));
            journal.node_journal_failure.store(true, Ordering::SeqCst);
        })),
    });
    assert_eq!(
        block_on(StartComfy::new(deps).execute()),
        Err(RuntimeError::NodeRecoveryRequired)
    );
    assert_eq!(m.env.count("nodes_before_start"), 1);
    assert_eq!(m.env.count("runtime_inventory"), 0);
    assert_eq!(m.env.count("runtime_start"), 0);
}

#[test]
fn restart_stops_rescans_and_starts_with_one_acquisition() {
    let m = Machine::new();
    m.status.lock().unwrap().lifecycle = Lifecycle::Running;
    let status = block_on(RestartComfy::new(m.deps()).execute()).unwrap();
    assert_eq!(status.lifecycle, Lifecycle::Running);
    assert_eq!(m.env.gate.try_acquires.load(Ordering::SeqCst), 1);
    assert_eq!(m.env.gate.acquires.load(Ordering::SeqCst), 0);
    assert_eq!(*m.stop_held.lock().unwrap(), [true]);
    let calls: Vec<_> = m
        .env
        .calls()
        .into_iter()
        .filter(|c| c.starts_with("runtime_") || c.starts_with("nodes_"))
        .collect();
    assert_eq!(
        calls,
        [
            "runtime_stop",
            "nodes_after_stop",
            "nodes_before_start",
            "runtime_inventory",
            "runtime_start",
        ]
    );
}

#[test]
fn stop_cancels_start_before_waiting_for_the_gate() {
    use std::{future::Future, pin::pin, task::Context};

    let m = Machine::new();
    m.hold_start.store(true, Ordering::SeqCst);
    let start = StartComfy::new(m.deps());
    let mut starting = pin!(start.execute());
    assert!(
        starting
            .as_mut()
            .poll(&mut Context::from_waker(std::task::Waker::noop()))
            .is_pending()
    );
    assert!(m.env.gate.busy.load(Ordering::SeqCst));
    assert_eq!(m.status.lock().unwrap().lifecycle, Lifecycle::Starting);
    let stopped = block_on(StopComfy::new(m.deps()).execute()).unwrap();
    assert_eq!(stopped.lifecycle, Lifecycle::Stopped);
    assert_eq!(block_on(starting), Err(RuntimeError::Cancelled));
    assert_eq!(m.env.gate.acquires.load(Ordering::SeqCst), 1);
    assert_eq!(*m.stop_held.lock().unwrap(), [true, true]);
    assert_eq!(m.env.count("nodes_after_stop"), 1);
    assert!(!m.env.gate.busy.load(Ordering::SeqCst));
}

#[test]
fn stop_rechecks_termination_after_acquiring_the_gate() {
    let m = Machine::new();
    let mut deps = m.deps();
    let competing_start = m.clone();
    deps.environment.gate = Arc::new(AcquisitionHookGate {
        inner: m.env.gate.clone(),
        before_acquire: Some(Box::new(move || {
            assert_eq!(competing_start.env.count("runtime_stop"), 1);
            competing_start.status.lock().unwrap().lifecycle = Lifecycle::Running;
        })),
        before_try_acquire: None,
    });
    let status = block_on(StopComfy::new(deps).execute()).unwrap();
    assert_eq!(status.lifecycle, Lifecycle::Stopped);
    assert_eq!(*m.stop_held.lock().unwrap(), [false, true]);
    assert_eq!(m.env.count("runtime_stop"), 2);
    assert_eq!(m.env.count("nodes_after_stop"), 1);
    assert_eq!(m.env.gate.acquires.load(Ordering::SeqCst), 1);
    assert_eq!(m.env.gate.try_acquires.load(Ordering::SeqCst), 0);
}

#[test]
fn after_stop_failure_is_returned_and_prevents_restart() {
    for operation in ["stop", "restart", "update"] {
        let m = Machine::new();
        m.status.lock().unwrap().lifecycle = Lifecycle::Running;
        m.node_rescan_failure.store(true, Ordering::SeqCst);
        let error = match operation {
            "stop" => block_on(StopComfy::new(m.deps()).execute()).unwrap_err(),
            "restart" => block_on(RestartComfy::new(m.deps()).execute()).unwrap_err(),
            _ => {
                let mut request = Machine::request();
                request.stop_running = true;
                block_on(UpdateComfy::new(m.deps()).execute(request, CancelToken::new()))
                    .unwrap_err()
            }
        };
        assert_eq!(error, RuntimeError::NodeRescanFailed);
        assert_eq!(m.status.lock().unwrap().lifecycle, Lifecycle::Stopped);
        assert_eq!(m.env.count("nodes_after_stop"), 1);
        assert_eq!(m.env.count("nodes_before_start"), 0);
        assert_eq!(m.env.count("runtime_start"), 0);
        assert_eq!(m.env.count("runtime_fetch"), 0);
        assert_eq!(m.env.count("runtime_apply"), 0);
        assert!(m.env.installs.lock().unwrap().is_empty());
        assert!(!m.env.gate.busy.load(Ordering::SeqCst));
    }
}

#[test]
fn update_with_stop_rescans_under_its_existing_lease() {
    let m = Machine::new();
    m.status.lock().unwrap().lifecycle = Lifecycle::Running;
    let mut request = Machine::request();
    request.stop_running = true;
    let run = block_on(UpdateComfy::new(m.deps()).execute(request, CancelToken::new())).unwrap();
    assert_eq!(run.phase, UpdatePhase::Complete);
    assert_eq!(*m.stop_held.lock().unwrap(), [true]);
    assert_eq!(m.env.gate.try_acquires.load(Ordering::SeqCst), 1);
    assert_eq!(m.env.gate.acquires.load(Ordering::SeqCst), 0);
    assert_eq!(m.env.count("nodes_after_stop"), 1);
    assert!(m.env.position("runtime_stop") < m.env.position("nodes_after_stop"));
    assert!(m.env.position("nodes_after_stop") < m.env.position("runtime_fetch"));
    assert!(m.env.position("nodes_after_stop") < m.env.position("runtime_apply"));
}
#[async_trait]
impl ComfyRepository for Machine {
    async fn revision(&self, _: &Path) -> Result<Revision, RuntimeError> {
        Ok(self.revision.lock().unwrap().clone())
    }
    async fn status(&self, _: &Path) -> Result<RepoState, RuntimeError> {
        Ok(self.repo.lock().unwrap().clone())
    }
    async fn fetch(&self, _: &Path, _: &StepContext) -> Result<(), RuntimeError> {
        self.env.call("runtime_fetch");
        Ok(())
    }
    async fn freeze(&self, _: &Path, _: &UpdateTarget) -> Result<FrozenUpdate, RuntimeError> {
        self.env.call("runtime_freeze");
        Ok(FrozenUpdate {
            commit: "b".repeat(40),
            tag: Some("v2".into()),
            fast_forward: false,
        })
    }
    async fn apply(&self, _: &Path, f: &FrozenUpdate, _: &StepContext) -> Result<(), RuntimeError> {
        self.env.call("runtime_apply");
        self.revision.lock().unwrap().commit = f.commit.clone();
        Ok(())
    }
    async fn versions(&self, _: &Path) -> Result<VersionList, RuntimeError> {
        Ok(VersionList {
            current: self.revision.lock().unwrap().clone(),
            tags: vec!["v2".into()],
        })
    }
}
#[async_trait]
impl LaunchPreflight for Machine {
    async fn capabilities(
        &self,
        _: &Path,
        _: &PythonEnvironment,
        _: &CancelToken,
    ) -> Result<LaunchCapabilities, RuntimeError> {
        Ok(LaunchCapabilities {
            options: [
                "--listen",
                "--port",
                "--preview-method",
                "--disable-auto-launch",
                "--windows-standalone-build",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            ..Default::default()
        })
    }
}
#[async_trait]
impl PortProbe for Machine {
    async fn available(&self, _: Port, _: bool) -> Result<bool, RuntimeError> {
        Ok(true)
    }
    async fn next_free(&self, _: Port, _: bool) -> Result<Option<Port>, RuntimeError> {
        Ok(None)
    }
}
#[async_trait]
impl ExtraModelPaths for Machine {
    async fn read(&self, _: &Path) -> Result<Vec<ModelRoot>, RuntimeError> {
        Ok(Vec::new())
    }
}

#[test]
fn start_refuses_repair_and_acquires_gate_before_inspection() {
    let m = Machine::new();
    let mut env = m.env.env("selected");
    env.mark_repair_required("t").unwrap();
    m.env.envs.lock().unwrap().insert("selected".into(), env);
    assert_eq!(
        block_on(StartComfy::new(m.deps()).execute()),
        Err(RuntimeError::EnvironmentNotUsable)
    );
    assert!(m.inventory_checked.load(Ordering::SeqCst));
    m.env.gate.busy.store(true, Ordering::SeqCst);
    m.inventory_checked.store(false, Ordering::SeqCst);
    assert_eq!(
        block_on(StartComfy::new(m.deps()).execute()),
        Err(RuntimeError::Busy)
    );
    assert!(!m.inventory_checked.load(Ordering::SeqCst));
}
#[test]
fn restart_acquires_once_and_saves_options_apply_only_on_next_start() {
    let m = Machine::new();
    let deps = m.deps();
    let started = block_on(StartComfy::new(deps.clone()).execute()).unwrap();
    block_on(SetLaunchOptions::new(m.clone()).execute(LaunchOptions {
        port: Port::new(9001).unwrap(),
        ..Default::default()
    }))
    .unwrap();
    assert_eq!(started.session.unwrap().options.port, Port::default());
    let before = m.env.gate.try_acquires.load(Ordering::SeqCst);
    let status = block_on(RestartComfy::new(deps).execute()).unwrap();
    assert_eq!(m.env.gate.try_acquires.load(Ordering::SeqCst) - before, 1);
    assert_eq!(status.session.unwrap().options.port.get(), 9001);
}
#[test]
fn dirty_or_diverged_updates_change_neither_source_nor_packages() {
    for state in [
        RepoState {
            dirty: true,
            status: "dirty status".into(),
            ..Default::default()
        },
        RepoState {
            ahead: 1,
            behind: 1,
            ..Default::default()
        },
    ] {
        let m = Machine::new();
        *m.repo.lock().unwrap() = state;
        assert!(matches!(
            block_on(UpdateComfy::new(m.deps()).execute(Machine::request(), CancelToken::new())),
            Err(RuntimeError::UpdateRefused { .. })
        ));
        assert_eq!(m.env.count("runtime_apply"), 0);
        assert!(m.env.installs.lock().unwrap().is_empty());
    }
}
#[test]
fn update_failure_keeps_actual_source_and_blocks_start_then_retries_frozen_target() {
    let m = Machine::new();
    m.env.verify_final_fails.store(true, Ordering::SeqCst);
    assert!(
        block_on(UpdateComfy::new(m.deps()).execute(Machine::request(), CancelToken::new()))
            .is_err()
    );
    assert!(m.mutation_saw_repair.load(Ordering::SeqCst));
    assert_eq!(m.env.env("selected").state(), EnvState::RepairRequired);
    assert_eq!(m.revision.lock().unwrap().commit, "b".repeat(40));
    assert!(matches!(
        block_on(StartComfy::new(m.deps()).execute()),
        Err(RuntimeError::RecoveryRequired(_))
    ));
    let pending = m.update.lock().unwrap().clone().unwrap();
    m.env.verify_final_fails.store(false, Ordering::SeqCst);
    let mut retry = Machine::request();
    retry.retry = Some(pending.id);
    let done = block_on(UpdateComfy::new(m.deps()).execute(retry, CancelToken::new())).unwrap();
    assert_eq!(done.phase, UpdatePhase::Complete);
    assert_eq!(m.env.count("runtime_freeze"), 1);
    assert_eq!(m.env.count("runtime_apply"), 1);
    assert_eq!(m.env.env("selected").state(), EnvState::Verified);
    assert!(
        m.env
            .installs
            .lock()
            .unwrap()
            .iter()
            .all(|i| i.constraint_file.is_some() && !i.allow_source_builds)
    );
}
#[test]
fn an_update_needs_explicit_stop_authorization() {
    let m = Machine::new();
    m.status.lock().unwrap().lifecycle = Lifecycle::Running;
    assert_eq!(
        block_on(UpdateComfy::new(m.deps()).execute(Machine::request(), CancelToken::new())),
        Err(RuntimeError::NeedsStop)
    );
    assert_eq!(m.env.count("runtime_fetch"), 0);
}
