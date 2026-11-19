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
    path::{Path, PathBuf},
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
    applied: Mutex<Vec<FrozenUpdate>>,
    latest_detached: AtomicBool,
    inventory_checked: AtomicBool,
    mutation_saw_repair: AtomicBool,
    node_journal_failure: AtomicBool,
    reconcile_failure: AtomicBool,
    stop_held: Mutex<Vec<bool>>,
    /// Whether the gate was held each time a scan was requested.
    scan_requested_under_gate: Mutex<Vec<bool>>,
    /// What `scans_held_off` answers: closing holds scans off.
    held_off: AtomicBool,
    /// What `scan_settling` answers: a scan is settling under the gate.
    settling: AtomicBool,
    /// Makes the configured port unavailable.
    port_taken: AtomicBool,
    /// Makes every stop fail.
    fail_stop: AtomicBool,
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
            components: Default::default(),
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
            applied: Mutex::default(),
            latest_detached: AtomicBool::new(false),
            inventory_checked: AtomicBool::new(false),
            mutation_saw_repair: AtomicBool::new(false),
            node_journal_failure: AtomicBool::new(false),
            reconcile_failure: AtomicBool::new(false),
            stop_held: Mutex::default(),
            scan_requested_under_gate: Mutex::default(),
            held_off: AtomicBool::new(false),
            settling: AtomicBool::new(false),
            port_taken: AtomicBool::new(false),
            fail_stop: AtomicBool::new(false),
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
    async fn reconcile_after_stop(&self, _: &mut dyn MaintenanceLease) -> Result<(), RuntimeError> {
        assert!(self.env.gate.busy.load(Ordering::SeqCst));
        assert_eq!(self.status.lock().unwrap().lifecycle, Lifecycle::Stopped);
        self.env.call("nodes_reconcile_after_stop");
        if self.reconcile_failure.load(Ordering::SeqCst) {
            Err(RuntimeError::NodeRecoveryRequired)
        } else {
            Ok(())
        }
    }
    fn note_start(&self) {
        assert!(self.env.gate.busy.load(Ordering::SeqCst));
        self.env.call("nodes_note_start");
    }
    fn start_finished(&self) {
        self.env.call("nodes_start_finished");
    }
    fn request_scan(&self) {
        self.env.call("nodes_request_scan");
        self.scan_requested_under_gate
            .lock()
            .unwrap()
            .push(self.env.gate.busy.load(Ordering::SeqCst));
    }
    fn scans_held_off(&self) -> bool {
        self.held_off.load(Ordering::SeqCst)
    }
    fn scan_settling(&self) -> bool {
        self.settling.load(Ordering::SeqCst)
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
        if self.fail_stop.load(Ordering::SeqCst) {
            return Err(RuntimeError::Storage);
        }
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

    async fn acquire_within(
        &self,
        wait: std::time::Duration,
    ) -> Result<Box<dyn MaintenanceLease>, GateError> {
        self.inner.acquire_within(wait).await
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
fn restart_stops_and_starts_with_one_acquisition() {
    let m = Machine::new();
    m.status.lock().unwrap().lifecycle = Lifecycle::Running;
    let status = block_on(RestartComfy::new(m.deps()).execute()).unwrap();
    assert_eq!(status.lifecycle, Lifecycle::Running);
    assert_eq!(m.env.gate.try_acquires.load(Ordering::SeqCst), 1);
    assert_eq!(m.env.gate.acquires.load(Ordering::SeqCst), 0);
    assert_eq!(m.env.gate.acquire_withins.load(Ordering::SeqCst), 0);
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
            "nodes_note_start",
            "nodes_before_start",
            "runtime_inventory",
            "runtime_start",
            "nodes_start_finished",
            "nodes_request_scan",
        ]
    );
}

#[test]
fn restart_requests_a_scan_after_the_start_returns() {
    for start_fails in [false, true] {
        let m = Machine::new();
        m.status.lock().unwrap().lifecycle = Lifecycle::Running;
        m.node_journal_failure.store(start_fails, Ordering::SeqCst);

        let result = block_on(RestartComfy::new(m.deps()).execute());

        assert_eq!(result.is_err(), start_fails);
        // One request, after the start attempt, and the gate is free again by then.
        assert_eq!(*m.scan_requested_under_gate.lock().unwrap(), [false]);
        assert!(m.env.position("runtime_stop") < m.env.position("nodes_before_start"));
        assert!(m.env.position("nodes_before_start") < m.env.position("nodes_request_scan"));
        assert_eq!(m.env.count("runtime_start"), usize::from(!start_fails));
        assert!(!m.env.gate.busy.load(Ordering::SeqCst));
    }
}

#[test]
fn stop_requests_a_scan_after_releasing_the_gate() {
    let m = Machine::new();
    m.status.lock().unwrap().lifecycle = Lifecycle::Running;

    let status = block_on(StopComfy::new(m.deps()).execute()).unwrap();

    assert_eq!(status.lifecycle, Lifecycle::Stopped);
    assert_eq!(*m.stop_held.lock().unwrap(), [false, true]);
    assert_eq!(*m.scan_requested_under_gate.lock().unwrap(), [false]);
    assert!(m.env.position("runtime_stop") < m.env.position("nodes_request_scan"));
    // Stop neither settles nor records a start: that waits for the next start or scan.
    assert_eq!(m.env.count("nodes_reconcile_after_stop"), 0);
    assert_eq!(m.env.count("nodes_before_start"), 0);
    assert!(!m.env.gate.busy.load(Ordering::SeqCst));
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
    assert_eq!(m.env.count("nodes_request_scan"), 1);
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
    assert_eq!(m.env.count("nodes_request_scan"), 1);
    assert_eq!(m.env.gate.acquires.load(Ordering::SeqCst), 1);
    assert_eq!(m.env.gate.try_acquires.load(Ordering::SeqCst), 0);
}

#[test]
fn update_with_stop_reconciles_under_its_existing_lease() {
    let m = Machine::new();
    m.status.lock().unwrap().lifecycle = Lifecycle::Running;
    let mut request = Machine::request();
    request.stop_running = true;
    let run = block_on(UpdateComfy::new(m.deps()).execute(request, CancelToken::new())).unwrap();
    assert_eq!(run.phase, UpdatePhase::Complete);
    assert_eq!(*m.stop_held.lock().unwrap(), [true]);
    assert_eq!(m.env.gate.try_acquires.load(Ordering::SeqCst), 1);
    assert_eq!(m.env.gate.acquires.load(Ordering::SeqCst), 0);
    assert_eq!(m.env.gate.acquire_withins.load(Ordering::SeqCst), 0);
    assert_eq!(m.env.count("nodes_reconcile_after_stop"), 1);
    assert_eq!(m.env.count("nodes_request_scan"), 1);
    // Settling comes first; the scan is requested right after it, before the update goes on.
    assert!(m.env.position("runtime_stop") < m.env.position("nodes_reconcile_after_stop"));
    assert!(m.env.position("nodes_reconcile_after_stop") < m.env.position("nodes_request_scan"));
    assert!(m.env.position("nodes_request_scan") < m.env.position("runtime_fetch"));
    assert!(m.env.position("nodes_request_scan") < m.env.position("runtime_apply"));
}

#[test]
fn a_failed_reconcile_after_stop_blocks_the_update() {
    let m = Machine::new();
    m.status.lock().unwrap().lifecycle = Lifecycle::Running;
    m.reconcile_failure.store(true, Ordering::SeqCst);
    let mut request = Machine::request();
    request.stop_running = true;

    let error =
        block_on(UpdateComfy::new(m.deps()).execute(request, CancelToken::new())).unwrap_err();

    assert_eq!(error, RuntimeError::NodeRecoveryRequired);
    assert_eq!(m.status.lock().unwrap().lifecycle, Lifecycle::Stopped);
    assert_eq!(m.env.count("nodes_reconcile_after_stop"), 1);
    // The scan is requested even though settling failed, and only after it.
    assert_eq!(m.env.count("nodes_request_scan"), 1);
    assert!(m.env.position("nodes_reconcile_after_stop") < m.env.position("nodes_request_scan"));
    assert_eq!(m.env.count("runtime_fetch"), 0);
    assert_eq!(m.env.count("runtime_apply"), 0);
    assert!(m.env.installs.lock().unwrap().is_empty());
    assert!(!m.env.gate.busy.load(Ordering::SeqCst));
}

const GATED_OPERATIONS: [&str; 3] = ["start", "restart", "update"];

/// A fresh machine for the operation: Restart and Update (which stops it) find ComfyUI running.
fn machine_for(operation: &str) -> Arc<Machine> {
    let m = Machine::new();
    if operation != "start" {
        m.status.lock().unwrap().lifecycle = Lifecycle::Running;
    }
    m
}

fn run_runtime_operation(m: &Arc<Machine>, operation: &str) -> Result<(), RuntimeError> {
    match operation {
        "start" => block_on(StartComfy::new(m.deps()).execute()).map(|_| ()),
        "restart" => block_on(RestartComfy::new(m.deps()).execute()).map(|_| ()),
        _ => {
            let mut request = Machine::request();
            request.stop_running = true;
            block_on(UpdateComfy::new(m.deps()).execute(request, CancelToken::new())).map(|_| ())
        }
    }
}

/// What a Start, Restart or Update that was refused must not have called.
const NOTHING_RAN: [&str; 10] = [
    "nodes_note_start",
    "nodes_start_finished",
    "nodes_before_start",
    "nodes_reconcile_after_stop",
    "nodes_request_scan",
    "runtime_stop",
    "runtime_inventory",
    "runtime_start",
    "runtime_fetch",
    "runtime_apply",
];

#[test]
fn closing_wins_over_a_start_restart_or_update_that_took_the_gate() {
    for operation in GATED_OPERATIONS {
        let m = machine_for(operation);
        m.held_off.store(true, Ordering::SeqCst);

        assert_eq!(
            run_runtime_operation(&m, operation),
            Err(RuntimeError::Busy),
            "{operation}"
        );

        for call in NOTHING_RAN {
            assert_eq!(m.env.count(call), 0, "{operation}: {call}");
        }
        assert!(!m.env.gate.busy.load(Ordering::SeqCst));
        drop(block_on(m.env.gate.try_acquire()).expect("the gate is free again"));
    }
}

#[test]
fn start_restart_and_update_are_busy_while_the_gate_is_held() {
    for operation in GATED_OPERATIONS {
        let m = machine_for(operation);
        let held = block_on(m.env.gate.try_acquire()).unwrap();

        assert_eq!(
            run_runtime_operation(&m, operation),
            Err(RuntimeError::Busy),
            "{operation}"
        );

        // Nobody waits for the holder: no scan is settling, so there is nothing to wait for.
        assert_eq!(m.env.gate.acquires.load(Ordering::SeqCst), 0);
        assert_eq!(m.env.gate.acquire_withins.load(Ordering::SeqCst), 0);
        for call in NOTHING_RAN {
            assert_eq!(m.env.count(call), 0, "{operation}: {call}");
        }
        assert!(m.env.gate.busy.load(Ordering::SeqCst));
        drop(held);
    }
}

#[test]
fn start_restart_and_update_wait_briefly_for_a_scan_that_is_settling() {
    for operation in GATED_OPERATIONS {
        let m = machine_for(operation);
        m.settling.store(true, Ordering::SeqCst);
        let held = block_on(m.env.gate.try_acquire()).unwrap();

        // The settling scan does not let go in time: one bounded wait, then Busy, nothing else.
        assert_eq!(
            run_runtime_operation(&m, operation),
            Err(RuntimeError::Busy),
            "{operation}"
        );

        assert_eq!(m.env.gate.try_acquires.load(Ordering::SeqCst), 2);
        assert_eq!(m.env.gate.acquire_withins.load(Ordering::SeqCst), 1);
        assert_eq!(m.env.gate.acquires.load(Ordering::SeqCst), 0);
        for call in NOTHING_RAN {
            assert_eq!(m.env.count(call), 0, "{operation}: {call}");
        }
        assert!(m.env.gate.busy.load(Ordering::SeqCst));
        drop(held);
    }
}

#[test]
fn a_settling_scan_does_not_delay_an_operation_that_finds_the_gate_free() {
    let m = machine_for("start");
    m.settling.store(true, Ordering::SeqCst);

    block_on(StartComfy::new(m.deps()).execute()).unwrap();

    assert_eq!(m.env.gate.acquire_withins.load(Ordering::SeqCst), 0);
}

#[test]
fn start_and_restart_record_the_start_around_every_outcome() {
    #[derive(Clone, Copy)]
    enum Outcome {
        Started,
        JournalFailure,
        PortConflict,
        NeedsStop,
    }
    for (restart, outcome) in [
        (false, Outcome::Started),
        (false, Outcome::JournalFailure),
        (false, Outcome::PortConflict),
        (false, Outcome::NeedsStop),
        (true, Outcome::Started),
        (true, Outcome::JournalFailure),
        (true, Outcome::PortConflict),
    ] {
        let m = Machine::new();
        // Restart stops a running ComfyUI first; a plain Start refuses one.
        if restart || matches!(outcome, Outcome::NeedsStop) {
            m.status.lock().unwrap().lifecycle = Lifecycle::Running;
        }
        match outcome {
            Outcome::JournalFailure => m.node_journal_failure.store(true, Ordering::SeqCst),
            Outcome::PortConflict => m.port_taken.store(true, Ordering::SeqCst),
            _ => {}
        }

        let result = if restart {
            block_on(RestartComfy::new(m.deps()).execute()).map(|_| ())
        } else {
            block_on(StartComfy::new(m.deps()).execute()).map(|_| ())
        };

        let label = format!("restart={restart}");
        let expected = match outcome {
            Outcome::Started => Ok(()),
            Outcome::JournalFailure => Err(RuntimeError::NodeRecoveryRequired),
            Outcome::PortConflict => Err(RuntimeError::PortConflict {
                requested: Port::default(),
                suggested: None,
            }),
            Outcome::NeedsStop => Err(RuntimeError::NeedsStop),
        };
        assert_eq!(result, expected, "{label}");
        assert_eq!(m.env.count("nodes_note_start"), 1, "{label}");
        assert_eq!(m.env.count("nodes_start_finished"), 1, "{label}");
        assert!(
            m.env.position("nodes_note_start") < m.env.position("nodes_before_start"),
            "{label}"
        );
        assert!(
            m.env.position("nodes_before_start") < m.env.position("nodes_start_finished"),
            "{label}"
        );
        if result.is_ok() {
            // The runtime start is part of the recorded interval.
            assert!(
                m.env.position("runtime_start") < m.env.position("nodes_start_finished"),
                "{label}"
            );
        }
    }
}

#[test]
fn a_dropped_start_still_finishes_its_record() {
    use std::{future::Future, pin::pin, task::Context};

    let m = Machine::new();
    m.hold_start.store(true, Ordering::SeqCst);
    let start = StartComfy::new(m.deps());
    {
        let mut starting = pin!(start.execute());
        assert!(
            starting
                .as_mut()
                .poll(&mut Context::from_waker(std::task::Waker::noop()))
                .is_pending()
        );
        assert_eq!(m.env.count("nodes_note_start"), 1);
        assert_eq!(m.env.count("nodes_start_finished"), 0);
        // The caller disconnects: the future is dropped while the start is still pending.
    }
    assert_eq!(m.env.count("nodes_start_finished"), 1);
}

#[test]
fn a_failed_second_stop_still_requests_the_scan_and_a_failed_first_stop_does_not() {
    // The first stop succeeds and the gate cannot be taken: ComfyUI did stop, so a scan is asked for.
    let m = Machine::new();
    m.status.lock().unwrap().lifecycle = Lifecycle::Running;
    let mut deps = m.deps();
    deps.environment.gate = Arc::new(UnavailableGate);
    assert_eq!(
        block_on(StopComfy::new(deps).execute()),
        Err(RuntimeError::Busy)
    );
    assert_eq!(m.env.count("runtime_stop"), 1);
    assert_eq!(m.env.count("nodes_request_scan"), 1);

    // The first stop fails: nothing stopped, so there is nothing for a scan to see.
    let m = Machine::new();
    m.fail_stop.store(true, Ordering::SeqCst);
    assert_eq!(
        block_on(StopComfy::new(m.deps()).execute()),
        Err(RuntimeError::Storage)
    );
    assert_eq!(m.env.count("nodes_request_scan"), 0);
}

struct UnavailableGate;

#[async_trait]
impl MaintenanceGate for UnavailableGate {
    async fn acquire(&self) -> Result<Box<dyn MaintenanceLease>, GateError> {
        Err(GateError::Unavailable)
    }
    async fn try_acquire(&self) -> Result<Box<dyn MaintenanceLease>, GateError> {
        Err(GateError::Unavailable)
    }
    async fn acquire_within(
        &self,
        _: std::time::Duration,
    ) -> Result<Box<dyn MaintenanceLease>, GateError> {
        Err(GateError::Unavailable)
    }
}
#[async_trait]
impl ComfyRepository for Machine {
    async fn revision(&self, _: &Path) -> Result<Revision, RuntimeError> {
        Ok(self.revision.lock().unwrap().clone())
    }
    async fn status(&self, _: &Path) -> Result<RepoState, RuntimeError> {
        Ok(self.repo.lock().unwrap().clone())
    }
    async fn fetch(&self, _: &Path, _: &UpdateTarget, _: &StepContext) -> Result<(), RuntimeError> {
        self.env.call("runtime_fetch");
        Ok(())
    }
    async fn freeze(&self, _: &Path, target: &UpdateTarget) -> Result<FrozenUpdate, RuntimeError> {
        self.env.call("runtime_freeze");
        Ok(FrozenUpdate {
            commit: "b".repeat(40),
            tag: (target != &UpdateTarget::LatestCommit).then(|| "v2".into()),
            branch: (target == &UpdateTarget::LatestCommit).then(|| "main".into()),
            fast_forward: false,
        })
    }
    async fn apply(&self, _: &Path, f: &FrozenUpdate, _: &StepContext) -> Result<(), RuntimeError> {
        self.env.call("runtime_apply");
        self.applied.lock().unwrap().push(f.clone());
        let mut revision = self.revision.lock().unwrap();
        revision.commit = f.commit.clone();
        if f.branch.is_some() {
            revision.tag = None;
            revision.branch = if self.latest_detached.load(Ordering::SeqCst) {
                None
            } else {
                f.branch.clone()
            };
        }
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
        Ok(!self.port_taken.load(Ordering::SeqCst))
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

type WriterCall = (&'static str, PathBuf, Option<PathBuf>);
#[derive(Default)]
struct RecordingWriter {
    calls: Mutex<Vec<WriterCall>>,
}
#[async_trait]
impl ExtraModelPathsWriter for RecordingWriter {
    async fn generate(&self, root: &Path, base: &Path) -> Result<(), RuntimeError> {
        self.calls
            .lock()
            .unwrap()
            .push(("generate", root.to_owned(), Some(base.to_owned())));
        if base.ends_with("rejected") {
            return Err(RuntimeError::ExtraPathsRejected("not here".into()));
        }
        Ok(())
    }
    async fn remove(&self, root: &Path) -> Result<(), RuntimeError> {
        self.calls
            .lock()
            .unwrap()
            .push(("remove", root.to_owned(), None));
        Ok(())
    }
}

#[test]
fn manage_extra_model_paths_uses_the_current_root() {
    let m = Machine::new();
    let writer = Arc::new(RecordingWriter::default());
    let manage = ManageExtraModelPaths::new(m.env.clone(), writer.clone());
    block_on(manage.generate(Path::new("/models"))).unwrap();
    assert_eq!(
        block_on(manage.generate(Path::new("/rejected"))),
        Err(RuntimeError::ExtraPathsRejected("not here".into()))
    );
    block_on(manage.remove()).unwrap();
    let root = PathBuf::from(env_fakes::ROOT);
    assert_eq!(
        *writer.calls.lock().unwrap(),
        vec![
            ("generate", root.clone(), Some(PathBuf::from("/models"))),
            ("generate", root.clone(), Some(PathBuf::from("/rejected"))),
            ("remove", root, None),
        ]
    );
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
fn update_keeps_the_recorded_profile_revision_constraints() {
    let m = Machine::new();
    let extras = vec!["transformers==5.5.0".to_owned()];
    {
        let mut profiles = m.env.profiles.lock().unwrap();
        assert_eq!(m.env.env("selected").profile_revision, profiles[0].revision);
        profiles[0].constraints = extras.clone();
        let mut current = profiles[0].clone();
        current.revision += 1;
        current.constraints = vec!["transformers==5.20.2".into()];
        profiles.insert(0, current);
    }
    let run = block_on(UpdateComfy::new(m.deps()).execute(Machine::request(), CancelToken::new()))
        .unwrap();
    assert_eq!(run.phase, UpdatePhase::Complete);
    assert_eq!(*m.env.constraint_extras.lock().unwrap(), [extras]);
}

#[test]
fn update_errors_when_the_recorded_profile_revision_is_missing() {
    let m = Machine::new();
    m.env.profiles.lock().unwrap()[0].revision += 1;
    assert!(matches!(
        block_on(UpdateComfy::new(m.deps()).execute(Machine::request(), CancelToken::new())),
        Err(RuntimeError::EnvironmentNotUsable)
    ));
    assert!(m.env.constraint_extras.lock().unwrap().is_empty());
    assert!(m.env.installs.lock().unwrap().is_empty());
    assert_eq!(m.env.env("selected").state(), EnvState::Verified);
}

#[test]
fn a_program_running_python_from_the_environment_refuses_the_update_before_it_starts() {
    let m = Machine::new();
    let user = crate::environment::EnvironmentUser {
        pid: 77,
        name: "python.exe".into(),
    };
    *m.env.env_user.lock().unwrap() = Some(user.clone());
    assert_eq!(
        block_on(UpdateComfy::new(m.deps()).execute(Machine::request(), CancelToken::new()))
            .unwrap_err(),
        RuntimeError::EnvironmentProcessRunning(user)
    );
    assert_eq!(m.env.count("runtime_fetch"), 0);
    assert_eq!(m.env.count("runtime_apply"), 0);
    assert!(m.update.lock().unwrap().is_none());
    assert!(m.env.installs.lock().unwrap().is_empty());
    assert_eq!(m.env.env("selected").state(), EnvState::Verified);
}

#[test]
fn a_program_running_python_from_the_environment_refuses_a_stopping_update_before_the_stop() {
    let m = Machine::new();
    m.status.lock().unwrap().lifecycle = Lifecycle::Running;
    let user = crate::environment::EnvironmentUser {
        pid: 77,
        name: "python.exe".into(),
    };
    *m.env.env_user.lock().unwrap() = Some(user.clone());
    let mut request = Machine::request();
    request.stop_running = true;
    assert_eq!(
        block_on(UpdateComfy::new(m.deps()).execute(request, CancelToken::new())).unwrap_err(),
        RuntimeError::EnvironmentProcessRunning(user)
    );
    assert_eq!(m.env.count("runtime_stop"), 0);
    assert_eq!(m.env.count("nodes_reconcile_after_stop"), 0);
    assert_eq!(m.env.count("runtime_fetch"), 0);
    assert_eq!(m.status.lock().unwrap().lifecycle, Lifecycle::Running);
    assert!(!m.env.gate.busy.load(Ordering::SeqCst));
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
fn switching_to_the_current_tag_re_places_a_detached_checkout() {
    let m = Machine::new();
    *m.revision.lock().unwrap() = Revision {
        commit: "b".repeat(40),
        tag: Some("v2".into()),
        branch: None,
    };
    let run = block_on(UpdateComfy::new(m.deps()).execute(Machine::request(), CancelToken::new()))
        .unwrap();
    assert_eq!(run.phase, UpdatePhase::Complete);
    assert_eq!(m.env.count("runtime_apply"), 1);
    assert_eq!(m.revision.lock().unwrap().commit, "b".repeat(40));
}
#[test]
fn switching_to_the_current_tag_on_a_branch_does_not_apply() {
    let m = Machine::new();
    *m.revision.lock().unwrap() = Revision {
        commit: "b".repeat(40),
        tag: Some("v2".into()),
        branch: Some("release/v0.2".into()),
    };
    let run = block_on(UpdateComfy::new(m.deps()).execute(Machine::request(), CancelToken::new()))
        .unwrap();
    assert_eq!(run.phase, UpdatePhase::Complete);
    assert_eq!(m.env.count("runtime_apply"), 0);
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

#[test]
fn frozen_update_policy_rejects_invalid_target_shapes() {
    let frozen = |fast_forward, tag: Option<&str>, branch: Option<&str>| FrozenUpdate {
        commit: "b".repeat(40),
        fast_forward,
        tag: tag.map(str::to_owned),
        branch: branch.map(str::to_owned),
    };
    assert_eq!(
        frozen(true, None, None).policy_target(),
        Ok(UpdateTarget::BranchFfOnly)
    );
    assert_eq!(
        frozen(false, Some("v1.0.0"), None).policy_target(),
        Ok(UpdateTarget::Tag(Tag::new("v1.0.0").unwrap()))
    );
    assert_eq!(
        frozen(false, None, Some("release/v1.0")).policy_target(),
        Ok(UpdateTarget::LatestCommit)
    );
    for (ff, tag, branch) in [
        (false, None, None),
        (false, Some("v1"), Some("main")),
        (true, Some("v1"), None),
        (true, None, Some("main")),
        (true, Some("v1"), Some("main")),
    ] {
        assert_eq!(
            frozen(ff, tag, branch).policy_target(),
            Err(RuntimeError::InvalidRecord)
        );
    }
    for reference in [
        "",
        "-main",
        "/main",
        "main/",
        ".main",
        "main.",
        "a..b",
        "a//b",
        "a@{b}",
        "a/.b",
        "a.lock/b",
        "a b",
        "main^{commit}",
        &"x".repeat(201),
    ] {
        assert_eq!(
            frozen(false, None, Some(reference)).policy_target(),
            Err(RuntimeError::InvalidRecord)
        );
        assert_eq!(
            frozen(false, Some(reference), None).policy_target(),
            Err(RuntimeError::InvalidRecord)
        );
    }
}

fn latest_commit_request() -> UpdateRequest {
    UpdateRequest {
        target: UpdateTarget::LatestCommit,
        stop_running: false,
        retry: None,
    }
}

#[test]
fn latest_commit_update_records_the_actual_reference() {
    for detached in [false, true] {
        let m = Machine::new();
        let pins = m.env.env("selected").pins;
        m.latest_detached.store(detached, Ordering::SeqCst);
        let run = block_on(
            UpdateComfy::new(m.deps()).execute(latest_commit_request(), CancelToken::new()),
        )
        .unwrap();
        assert_eq!(run.phase, UpdatePhase::Complete);
        assert_eq!(run.target.commit, "b".repeat(40));
        assert_eq!(run.target.branch.as_deref(), Some("main"));
        assert_eq!(run.target.tag, None);
        let ManifestState::Valid(manifest) = m.env.manifest.lock().unwrap().clone() else {
            panic!("valid manifest")
        };
        assert_eq!(manifest.comfyui_commit, Some(run.target.commit));
        assert_eq!(
            manifest.comfyui_reference,
            (!detached).then(|| "main".to_owned())
        );
        assert_eq!(m.env.env("selected").pins, pins);
        assert_eq!(*m.env.constraint_writes.lock().unwrap(), [pins]);
        let installs = m.env.installs.lock().unwrap();
        assert_eq!(installs.len(), 1);
        assert!(installs[0].constraint_file.is_some());
        assert!(!installs[0].allow_source_builds);
        assert_eq!(
            installs[0].requirement_files,
            [PathBuf::from(env_fakes::ROOT).join("ComfyUI/requirements.txt")]
        );
        assert_eq!(m.env.count("verify_final"), 1);
    }
    // A protected change is rejected during preview, before any package mutation.
    let m = Machine::new();
    m.env
        .previews
        .lock()
        .unwrap()
        .push_back(Ok(crate::environment::ResolutionPreview {
            installs: vec![env_fakes::package("torch", "99.0.0")],
            ..Default::default()
        }));
    assert!(matches!(
        block_on(UpdateComfy::new(m.deps()).execute(latest_commit_request(), CancelToken::new())),
        Err(RuntimeError::RecoveryRequired(_))
    ));
    assert!(m.env.installs.lock().unwrap().is_empty());
    assert_eq!(m.env.count("verify_final"), 0);
    // An unexpected installed pin is also refused before verification/publication.
    let m = Machine::new();
    m.env.put_installed("torch", "99.0.0");
    assert!(matches!(
        block_on(UpdateComfy::new(m.deps()).execute(latest_commit_request(), CancelToken::new())),
        Err(RuntimeError::RecoveryRequired(_))
    ));
    assert_eq!(m.env.count("verify_final"), 0);
    let ManifestState::Valid(manifest) = m.env.manifest.lock().unwrap().clone() else {
        panic!("valid manifest")
    };
    assert_eq!(manifest.comfyui_commit, Some("a".repeat(40)));
}

#[test]
fn latest_commit_retries_the_frozen_target() {
    use UpdatePhase::*;
    for (phase, at_target, branch, applies) in [
        (Intent, false, "release/v1.0", 1),
        (Intent, true, "release/v1.0", 1),
        (Intent, true, "frozen-default", 1),
        (SourceChanged, true, "frozen-default", 0),
        (Dependencies, true, "frozen-default", 0),
        (Verifying, true, "frozen-default", 0),
        (Publishing, true, "frozen-default", 0),
    ] {
        let m = Machine::new();
        let frozen = FrozenUpdate {
            commit: "c".repeat(40),
            tag: None,
            branch: Some("frozen-default".into()),
            fast_forward: false,
        };
        *m.revision.lock().unwrap() = Revision {
            commit: if at_target {
                frozen.commit.clone()
            } else {
                "a".repeat(40)
            },
            tag: None,
            branch: Some(branch.into()),
        };
        let pending = UpdateRun {
            id: SessionId::new("frozen-update").unwrap(),
            env_id: m.env.env("selected").id,
            old_commit: "a".repeat(40),
            target: frozen.clone(),
            pins: m.env.env("selected").pins,
            phase,
            failure: Some("interrupted".into()),
            created_at: "t".into(),
        };
        *m.update.lock().unwrap() = Some(pending.clone());
        let mut request = Machine::request(); // The retry's requested tag must be ignored.
        request.retry = Some(pending.id);
        let run =
            block_on(UpdateComfy::new(m.deps()).execute(request, CancelToken::new())).unwrap();
        assert_eq!(run.target, frozen);
        assert_eq!(run.phase, Complete);
        assert_eq!(m.env.count("runtime_fetch"), 0);
        assert_eq!(m.env.count("runtime_freeze"), 0);
        assert_eq!(m.env.count("runtime_apply"), applies);
        if applies > 0 {
            assert_eq!(
                m.applied.lock().unwrap().as_slice(),
                std::slice::from_ref(&frozen)
            );
        }
        assert_eq!(m.revision.lock().unwrap().commit, frozen.commit);
        assert_eq!(m.revision.lock().unwrap().branch, frozen.branch);
        assert_eq!(m.env.count("verify_final"), 1);
        assert_eq!(m.env.env("selected").state(), EnvState::Verified);
    }
    // A fresh same-SHA attached release must also establish the latest-commit placement.
    let m = Machine::new();
    *m.revision.lock().unwrap() = Revision {
        commit: "b".repeat(40),
        tag: Some("v2".into()),
        branch: Some("release/v2.0".into()),
    };
    block_on(UpdateComfy::new(m.deps()).execute(latest_commit_request(), CancelToken::new()))
        .unwrap();
    assert_eq!(m.env.count("runtime_apply"), 1);
    assert_eq!(m.revision.lock().unwrap().branch.as_deref(), Some("main"));

    let m = Machine::new();
    m.env.verify_final_fails.store(true, Ordering::SeqCst);
    assert!(
        block_on(UpdateComfy::new(m.deps()).execute(latest_commit_request(), CancelToken::new()))
            .is_err()
    );
    assert!(matches!(
        block_on(StartComfy::new(m.deps()).execute()),
        Err(RuntimeError::RecoveryRequired(_))
    ));
    let pending = m.update.lock().unwrap().clone().unwrap();
    let mut request = Machine::request();
    request.retry = Some(pending.id.clone());
    m.revision.lock().unwrap().commit = "d".repeat(40);
    assert!(matches!(
        block_on(UpdateComfy::new(m.deps()).execute(request.clone(), CancelToken::new())),
        Err(RuntimeError::RecoveryRequired(_))
    ));
    assert_eq!(m.env.count("runtime_apply"), 1);
    m.revision.lock().unwrap().commit = pending.target.commit.clone();
    m.env.verify_final_fails.store(false, Ordering::SeqCst);
    let done = block_on(UpdateComfy::new(m.deps()).execute(request, CancelToken::new())).unwrap();
    assert_eq!(done.target, pending.target);
    assert_eq!(done.phase, Complete);
    assert_eq!(m.env.count("runtime_fetch"), 1);
    assert_eq!(m.env.count("runtime_freeze"), 1);
    assert_eq!(m.env.count("runtime_apply"), 1);
    assert_eq!(m.env.count("verify_final"), 2);
    assert_eq!(m.env.env("selected").state(), EnvState::Verified);
}
