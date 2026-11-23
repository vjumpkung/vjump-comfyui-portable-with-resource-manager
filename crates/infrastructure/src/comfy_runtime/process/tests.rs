use super::*;
use crate::shared::gate::TokioMaintenanceGate;
use application::shared::MaintenanceGate;
use domain::environment::EnvId;
use std::{
    collections::VecDeque,
    ffi::OsString,
    sync::atomic::{AtomicBool, AtomicU16, AtomicUsize, Ordering},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

#[derive(Default)]
struct Store {
    sessions: Mutex<Vec<(RuntimeSession, bool)>>,
}
#[async_trait]
impl RuntimeStore for Store {
    async fn launch_options(&self) -> Result<LaunchOptions, RuntimeError> {
        Ok(LaunchOptions::default())
    }
    async fn save_launch_options(&self, _: &LaunchOptions) -> Result<(), RuntimeError> {
        Ok(())
    }
    async fn save_session(&self, s: &RuntimeSession, finished: bool) -> Result<(), RuntimeError> {
        let mut sessions = self.sessions.lock().unwrap();
        sessions.retain(|(old, _)| old.id != s.id);
        sessions.push((s.clone(), finished));
        Ok(())
    }
    async fn unfinished_sessions(&self) -> Result<Vec<RuntimeSession>, RuntimeError> {
        Ok(self
            .sessions
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, done)| !*done)
            .map(|(s, _)| s.clone())
            .collect())
    }
    async fn save_update(&self, _: &UpdateRun) -> Result<(), RuntimeError> {
        Ok(())
    }
    async fn unfinished_update(&self) -> Result<Option<UpdateRun>, RuntimeError> {
        Ok(None)
    }
}
#[derive(Clone, Copy)]
enum Mode {
    Ready,
    NeverReady,
    Foreign,
    Exit,
    Spawn,
    /// Exits 0 right away after asking for a Manager restart, like a prestartup restart.
    MarkerExit,
    /// Ready; once `exit_now` is set, exits 0, with or without the Manager restart marker.
    ReadyThenExit {
        marker: bool,
    },
    /// Ready; leaves a restart marker and exit code 0 behind when it is stopped.
    MarkerOnCancel,
}
struct Runner {
    mode: Mode,
    /// The modes of the first runs; later runs use `mode`.
    first: Mutex<VecDeque<Mode>>,
    cleanup: bool,
    gate: Arc<TokioMaintenanceGate>,
    gate_held_during_cleanup: AtomicBool,
    spawned: AtomicBool,
    runs: AtomicUsize,
    exit_now: AtomicBool,
    /// The `/system_stats` requests answered so far.
    probes: AtomicUsize,
    /// The session variable of the latest run's environment.
    session_variable: Mutex<Option<OsString>>,
}
fn session_variable(spec: &CommandSpec) -> Option<&OsString> {
    spec.env
        .iter()
        .find(|(name, _)| name == MANAGER_SESSION_VARIABLE)
        .map(|(_, value)| value)
}
/// What ComfyUI-Manager does under comfy-cli's protocol: an empty `<value>.reboot` file.
fn request_restart(spec: &CommandSpec) {
    let mut marker = session_variable(spec).expect("session variable").clone();
    marker.push(".reboot");
    std::fs::write(marker, b"").unwrap();
}
struct Owned {
    alive: Arc<AtomicBool>,
    member: bool,
}
impl OwnedRuntimeProcess for Owned {
    fn identity(&self) -> ProcessIdentity {
        ProcessIdentity {
            pid: std::process::id(),
            created: "test-created".into(),
            scope: "test-scope".into(),
        }
    }
    fn is_alive(&self) -> Result<bool, RuntimeError> {
        Ok(self.alive.load(Ordering::SeqCst))
    }
    fn contains_pid(&self, pid: u32) -> Result<bool, RuntimeError> {
        Ok(self.member && pid == std::process::id())
    }
}
#[async_trait]
impl RuntimeProcessRunner for Runner {
    async fn run(
        &self,
        spec: CommandSpec,
        observer: Arc<dyn RuntimeProcessObserver>,
        cancel: CancelToken,
    ) -> ProcessCompletion {
        let mode = self.first.lock().unwrap().pop_front().unwrap_or(self.mode);
        self.runs.fetch_add(1, Ordering::SeqCst);
        *self.session_variable.lock().unwrap() = session_variable(&spec).cloned();
        if matches!(mode, Mode::Spawn) {
            return ProcessCompletion {
                exit_code: None,
                spawn_failed: true,
                cleanup_verified: true,
            };
        }
        let alive = Arc::new(AtomicBool::new(true));
        observer.spawned(Arc::new(Owned {
            alive: alive.clone(),
            member: !matches!(mode, Mode::Foreign),
        }));
        self.spawned.store(true, Ordering::SeqCst);
        observer.chunk(false, "ไทย token=sec".as_bytes());
        observer.chunk(false, b"ret-value\rready\n");
        if matches!(mode, Mode::Exit | Mode::MarkerExit) {
            if matches!(mode, Mode::MarkerExit) {
                request_restart(&spec);
            }
            alive.store(false, Ordering::SeqCst);
            return ProcessCompletion {
                exit_code: Some(0),
                spawn_failed: false,
                cleanup_verified: true,
            };
        }
        let args: Vec<_> = spec.args.iter().map(|a| a.to_string_lossy()).collect();
        let port = args.windows(2).find(|w| w[0] == "--port").unwrap()[1]
            .parse::<u16>()
            .unwrap();
        let listener = if matches!(
            mode,
            Mode::Ready | Mode::Foreign | Mode::ReadyThenExit { .. } | Mode::MarkerOnCancel
        ) {
            Some(TcpListener::bind(("127.0.0.1", port)).await.unwrap())
        } else {
            None
        };
        let mut triggered = false;
        while !cancel.is_cancelled() {
            if matches!(mode, Mode::ReadyThenExit { .. })
                && self.exit_now.swap(false, Ordering::SeqCst)
            {
                triggered = true;
                break;
            }
            tokio::select! {
                _=tokio::time::sleep(Duration::from_millis(10))=>{},
                accepted=async{listener.as_ref().unwrap().accept().await},if listener.is_some()=>{
                    let (mut socket,_)=accepted.unwrap();let mut request=[0;4096];let _=socket.read(&mut request).await;
                    let body=r#"{"system":{},"devices":[]}"#;
                    let reply=format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
                    let _=socket.write_all(reply.as_bytes()).await;
                    self.probes.fetch_add(1, Ordering::SeqCst);
                }
            }
        }
        self.gate_held_during_cleanup
            .store(self.gate.try_acquire().await.is_err(), Ordering::SeqCst);
        // Like Manager, leave the marker behind before the process is gone.
        let exit_code = match mode {
            Mode::ReadyThenExit { marker } if triggered => {
                if marker {
                    request_restart(&spec);
                }
                Some(0)
            }
            Mode::MarkerOnCancel => {
                request_restart(&spec);
                Some(0)
            }
            _ => None,
        };
        alive.store(false, Ordering::SeqCst);
        drop(listener);
        ProcessCompletion {
            exit_code,
            spawn_failed: false,
            cleanup_verified: self.cleanup,
        }
    }
}
/// A free loopback port that the OS cannot hand to another test of this process. `bind(:0)` and
/// release would race every other test that binds port 0: the supervisor's own availability
/// probe then reports a conflict, the start fails before the runner spawns, and the lease is
/// dropped instead of held. The OS dynamic range starts at 49152 (Windows) or 32768 (Linux), so
/// ports from a counter below it are unique per fixture and still probed for outside owners.
fn reserve_port() -> Port {
    static NEXT: AtomicU16 = AtomicU16::new(0);
    let base = 20_000 + (std::process::id() % 300) as u16 * 32;
    for _ in 0..32 {
        let candidate = base + NEXT.fetch_add(1, Ordering::SeqCst) % 32;
        if std::net::TcpListener::bind(("127.0.0.1", candidate)).is_ok() {
            return Port::new(candidate).unwrap();
        }
    }
    panic!("no free test port in {base}..{}", base + 32)
}
/// The readiness timeout of tests that assert the timeout itself (they wait for it to elapse).
const TIMEOUT_UNDER_TEST: Duration = Duration::from_millis(700);
/// For every other test the timeout is never the thing under test, so it must not elapse by
/// accident when a loaded machine stalls the test for a while.
const TIMEOUT_NOT_UNDER_TEST: Duration = Duration::from_secs(60);
struct Fixture {
    _directory: tempfile::TempDir,
    session: RuntimeSession,
    supervisor: ComfyRuntimeSupervisor,
    gate: Arc<TokioMaintenanceGate>,
    runner: Arc<Runner>,
    store: Arc<Store>,
}
fn fixture(mode: Mode, cleanup: bool) -> Fixture {
    fixture_with_timeout(mode, cleanup, TIMEOUT_NOT_UNDER_TEST)
}
fn fixture_with_timeout(mode: Mode, cleanup: bool, readiness_timeout: Duration) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    std::fs::create_dir(root.join("ComfyUI")).unwrap();
    let port = reserve_port();
    let gate = Arc::new(TokioMaintenanceGate::new());
    let store = Arc::new(Store::default());
    let runner = Arc::new(Runner {
        mode,
        first: Mutex::new(VecDeque::new()),
        cleanup,
        gate: gate.clone(),
        gate_held_during_cleanup: AtomicBool::new(false),
        spawned: AtomicBool::new(false),
        runs: AtomicUsize::new(0),
        exit_now: AtomicBool::new(false),
        probes: AtomicUsize::new(0),
        session_variable: Mutex::new(None),
    });
    let mut supervisor = ComfyRuntimeSupervisor::with_readiness_timeout(
        root.clone(),
        runner.clone(),
        store.clone(),
        RuntimeLogHub::default(),
        readiness_timeout,
    );
    // The inventory is asserted against the processes these tests own, not against whatever else
    // the machine is running: a python caught starting or exiting by another parallel test has
    // an unreadable location, which production rightly treats as "cannot assess".
    Arc::get_mut(&mut supervisor.inner)
        .expect("a new supervisor is unshared")
        .snapshot = procscan::test_snapshot;
    let session = RuntimeSession {
        id: SessionId::new("session-a").unwrap(),
        env_id: EnvId::new("env-a").unwrap(),
        interpreter: std::env::current_exe().unwrap(),
        checkout: root.join("ComfyUI"),
        revision: Revision {
            commit: "a".repeat(40),
            tag: None,
            branch: None,
        },
        options: LaunchOptions {
            port,
            ..Default::default()
        },
        started_at: "2026-10-16T00:00:00Z".into(),
        identity: None,
    };
    Fixture {
        _directory: dir,
        session,
        supervisor,
        gate,
        runner,
        store,
    }
}
impl Fixture {
    /// Waits until the runner has spawned its process. Bounded, so a start that failed before
    /// spawning fails the test instead of spinning forever.
    async fn spawned(&self) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        while !self.runner.spawned.load(Ordering::SeqCst) {
            assert!(
                tokio::time::Instant::now() < deadline,
                "the runner was never spawned"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
    /// Makes the next run use `mode` instead of the fixture's.
    fn first(&self, mode: Mode) {
        self.runner.first.lock().unwrap().push_back(mode);
    }
    /// Where Manager leaves its restart request for this fixture's session.
    fn marker(&self) -> PathBuf {
        self._directory
            .path()
            .join("data/tmp/comfyui")
            .join(format!("{}.reboot", self.session.id.as_str()))
    }
    /// Waits until the status satisfies `done`. Bounded like `spawned`.
    async fn until(&self, what: &str, done: impl Fn(&RuntimeStatus) -> bool) -> RuntimeStatus {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        loop {
            let status = self.supervisor.status().await;
            if done(&status) {
                return status;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "the status never became {what}"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
    /// Waits until the runner has been run `runs` times. Bounded like `spawned`.
    async fn runs(&self, runs: usize) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        while self.runner.runs.load(Ordering::SeqCst) < runs {
            assert!(
                tokio::time::Instant::now() < deadline,
                "the runner was not run {runs} times"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
    async fn start(&self) -> Result<RuntimeStatus, RuntimeError> {
        self.supervisor
            .start(
                self.session.clone(),
                vec!["--port".into(), self.session.options.port.get().to_string()],
                self.gate.acquire().await.unwrap(),
            )
            .await
    }
}

#[test]
fn cli_options_are_exact() {
    let options =
        parse_options("usage: main.py [--listen [IP]] [--port PORT]\n  --lowvram, --highvram");
    assert!(options.contains("--listen"));
    assert!(!options.contains("--lis"));
    assert_eq!(options.len(), 4);
}

#[derive(Default)]
struct BridgeObservation {
    owned: Mutex<Option<Arc<dyn OwnedRuntimeProcess>>>,
    bytes: Mutex<Vec<u8>>,
}
impl RuntimeProcessObserver for BridgeObservation {
    fn spawned(&self, process: Arc<dyn OwnedRuntimeProcess>) {
        *self.owned.lock().unwrap() = Some(process);
    }
    fn chunk(&self, _: bool, bytes: &[u8]) {
        self.bytes.lock().unwrap().extend_from_slice(bytes);
    }
}
#[test]
#[ignore = "subprocess fixture, invoked only by the bridge ownership test"]
fn shared_bridge_grandchild() {
    loop {
        std::thread::sleep(Duration::from_millis(25));
    }
}
#[test]
#[ignore = "subprocess fixture, invoked only by the bridge ownership test"]
// The grandchild is deliberately left running: the owning job must reap it.
#[allow(clippy::zombie_processes)]
fn shared_bridge_child() {
    use std::io::Write;
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command.args([
        "--ignored",
        "--exact",
        "comfy_runtime::process::tests::shared_bridge_grandchild",
        "--nocapture",
    ]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    }
    let child = command.spawn().unwrap();
    print!("VJ-GRANDCHILD:{}\nไทย\rno-newline", child.id());
    std::io::stdout().flush().unwrap();
    loop {
        std::thread::sleep(Duration::from_millis(25));
    }
}
#[tokio::test]
async fn shared_runner_bridge_preserves_raw_output_and_owns_child_and_grandchild() {
    let dir = tempfile::tempdir().unwrap();
    let layout = Layout::new(dir.path());
    let spec = command(&layout, Tool::Python, std::env::current_exe().unwrap())
        .cwd(dir.path())
        .args([
            "--ignored",
            "--exact",
            "comfy_runtime::process::tests::shared_bridge_child",
            "--nocapture",
        ]);
    let observer = Arc::new(BridgeObservation::default());
    let cancel = CancelToken::new();
    let task_observer = observer.clone();
    let task_cancel = cancel.clone();
    let running = tokio::spawn(async move {
        SharedRuntimeRunner::new()
            .run(spec, task_observer, task_cancel)
            .await
    });
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let grandchild = loop {
        let bytes = observer.bytes.lock().unwrap().clone();
        let text = String::from_utf8_lossy(&bytes);
        if let Some(pid) = text.lines().find_map(|line| {
            line.strip_prefix("VJ-GRANDCHILD:")
                .and_then(|s| s.parse::<u32>().ok())
        }) {
            break pid;
        }
        if tokio::time::Instant::now() > deadline {
            cancel.cancel();
            let _ = running.await;
            panic!("fixture did not publish grandchild");
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    let owned = observer.owned.lock().unwrap().clone().unwrap();
    let identity = owned.identity();
    assert_eq!(
        super::super::inventory::creation_identity(identity.pid)
            .unwrap()
            .unwrap(),
        identity.created
    );
    assert!(owned.is_alive().unwrap());
    assert!(owned.contains_pid(identity.pid).unwrap());
    assert!(owned.contains_pid(grandchild).unwrap());
    assert!(!owned.contains_pid(std::process::id()).unwrap());
    cancel.cancel();
    let result = running.await.unwrap();
    assert!(result.cleanup_verified);
    assert!(!owned.is_alive().unwrap());
    // Kernel process teardown may trail the job's accounting briefly under load.
    let gone = tokio::time::Instant::now() + Duration::from_secs(5);
    while owned.contains_pid(grandchild).unwrap() {
        assert!(
            tokio::time::Instant::now() < gone,
            "grandchild survived cleanup"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let bytes = observer.bytes.lock().unwrap();
    assert!(String::from_utf8_lossy(&bytes).contains("ไทย\rno-newline"));
}

#[tokio::test]
async fn ready_owns_listener_releases_gate_and_stop_is_idempotent() {
    let f = fixture(Mode::Ready, true);
    let status = f.start().await.unwrap();
    assert_eq!(status.lifecycle, Lifecycle::Running);
    assert!(
        status
            .origin()
            .unwrap()
            .ends_with(&f.session.options.port.get().to_string())
    );
    assert!(f.gate.try_acquire().await.is_ok());
    let stopped = f.supervisor.stop().await.unwrap();
    assert_eq!(stopped.lifecycle, Lifecycle::Stopped);
    assert!(f.store.unfinished_sessions().await.unwrap().is_empty());
    assert_eq!(
        f.supervisor.stop().await.unwrap().lifecycle,
        Lifecycle::Stopped
    );
}
#[tokio::test]
async fn timeout_and_foreign_listener_cleanup_hold_the_start_lease() {
    for mode in [Mode::NeverReady, Mode::Foreign] {
        let f = fixture_with_timeout(mode, true, TIMEOUT_UNDER_TEST);
        assert!(f.start().await.is_err());
        assert_eq!(
            f.supervisor.status().await.failure.unwrap().kind,
            FailureKind::ReadinessTimeout
        );
        assert!(f.runner.gate_held_during_cleanup.load(Ordering::SeqCst));
        assert!(f.gate.try_acquire().await.is_ok());
    }
}
#[tokio::test]
async fn stop_cancels_start_without_recursively_acquiring_its_lease() {
    let f = fixture(Mode::NeverReady, true);
    let supervisor = f.supervisor.clone();
    let session = f.session.clone();
    let port = session.options.port.get().to_string();
    let lease = f.gate.acquire().await.unwrap();
    let start = tokio::spawn(async move {
        supervisor
            .start(session, vec!["--port".into(), port], lease)
            .await
    });
    f.spawned().await;
    let stopped = f.supervisor.stop().await.unwrap();
    assert_eq!(stopped.lifecycle, Lifecycle::Stopped);
    assert_eq!(start.await.unwrap().unwrap().lifecycle, Lifecycle::Stopped);
    assert!(f.runner.gate_held_during_cleanup.load(Ordering::SeqCst));
}
#[tokio::test]
async fn failed_cleanup_quarantines_the_gate_and_m1_maintenance() {
    let f = fixture_with_timeout(Mode::NeverReady, false, TIMEOUT_UNDER_TEST);
    assert!(f.start().await.is_err());
    assert!(f.gate.try_acquire().await.is_err());
    assert_eq!(
        f.supervisor.ensure_stopped(f._directory.path()).await,
        Err(RuntimeBlock::CannotAssess)
    );
    assert_eq!(
        f.supervisor.status().await.failure.unwrap().kind,
        FailureKind::Termination
    );
}
#[tokio::test]
async fn spawn_failure_and_clean_early_exit_are_distinct_crashes() {
    for (mode, kind) in [
        (Mode::Spawn, FailureKind::Spawn),
        (Mode::Exit, FailureKind::ExitBeforeReady),
    ] {
        let f = fixture(mode, true);
        assert!(f.start().await.is_err());
        assert_eq!(f.supervisor.status().await.failure.unwrap().kind, kind);
    }
}
#[tokio::test]
async fn disconnected_start_caller_does_not_abandon_core_cleanup() {
    let f = fixture(Mode::NeverReady, true);
    let supervisor = f.supervisor.clone();
    let session = f.session.clone();
    let port = session.options.port.get().to_string();
    let lease = f.gate.acquire().await.unwrap();
    let caller = tokio::spawn(async move {
        supervisor
            .start(session, vec!["--port".into(), port], lease)
            .await
    });
    f.spawned().await;
    caller.abort();
    let _ = caller.await;
    assert_eq!(
        f.supervisor.stop().await.unwrap().lifecycle,
        Lifecycle::Stopped
    );
    assert!(f.runner.gate_held_during_cleanup.load(Ordering::SeqCst));
}
#[tokio::test]
async fn manager_restart_after_readiness_relaunches_the_same_session() {
    let f = fixture(Mode::Ready, true);
    f.first(Mode::ReadyThenExit { marker: true });
    assert_eq!(f.start().await.unwrap().lifecycle, Lifecycle::Running);
    assert!(f.gate.try_acquire().await.is_ok());
    let probes = f.runner.probes.load(Ordering::SeqCst);
    f.runner.exit_now.store(true, Ordering::SeqCst);
    f.runs(2).await;
    // The relaunched ComfyUI answers a readiness probe of its own.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while f.runner.probes.load(Ordering::SeqCst) == probes {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the relaunched ComfyUI was never probed"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let status = f
        .until("Running", |s| s.lifecycle == Lifecycle::Running)
        .await;
    assert!(status.failure.is_none());
    assert_eq!(status.session.unwrap().id, f.session.id);
    assert!(!f.marker().exists());
    let unfinished = f.store.unfinished_sessions().await.unwrap();
    assert_eq!(unfinished.len(), 1);
    assert_eq!(unfinished[0].id, f.session.id);
    assert_eq!(f.runner.runs.load(Ordering::SeqCst), 2);
    assert_eq!(
        f.supervisor.stop().await.unwrap().lifecycle,
        Lifecycle::Stopped
    );
    assert!(f.store.unfinished_sessions().await.unwrap().is_empty());
}
#[tokio::test]
async fn a_relaunch_that_exits_before_readiness_is_a_crash() {
    let f = fixture(Mode::Exit, true);
    f.first(Mode::ReadyThenExit { marker: true });
    assert_eq!(f.start().await.unwrap().lifecycle, Lifecycle::Running);
    f.runner.exit_now.store(true, Ordering::SeqCst);
    let status = f
        .until("Crashed", |s| s.lifecycle == Lifecycle::Crashed)
        .await;
    assert_eq!(status.failure.unwrap().kind, FailureKind::ExitBeforeReady);
    assert_eq!(f.runner.runs.load(Ordering::SeqCst), 2);
    assert!(!f.marker().exists());
}
#[tokio::test]
async fn manager_restart_during_startup_is_part_of_the_start() {
    let f = fixture(Mode::Ready, true);
    f.first(Mode::MarkerExit);
    let status = f.start().await.unwrap();
    assert_eq!(status.lifecycle, Lifecycle::Running);
    assert_eq!(f.runner.runs.load(Ordering::SeqCst), 2);
    assert!(!f.marker().exists());
    let folder = f._directory.path().join("data/tmp/comfyui");
    assert!(folder.is_dir());
    // Every run gets the comfy-cli session variable, pointing at `<folder>/<session id>`.
    let value = f.runner.session_variable.lock().unwrap().clone().unwrap();
    assert_eq!(PathBuf::from(value), folder.join(f.session.id.as_str()));
    assert!(f.gate.try_acquire().await.is_ok());
    assert_eq!(
        f.supervisor.stop().await.unwrap().lifecycle,
        Lifecycle::Stopped
    );
}
#[tokio::test]
async fn clean_exit_without_a_restart_request_is_still_a_crash() {
    let f = fixture(Mode::Ready, true);
    f.first(Mode::ReadyThenExit { marker: false });
    assert_eq!(f.start().await.unwrap().lifecycle, Lifecycle::Running);
    f.runner.exit_now.store(true, Ordering::SeqCst);
    let status = f
        .until("Crashed", |s| s.lifecycle == Lifecycle::Crashed)
        .await;
    let failure = status.failure.unwrap();
    assert_eq!(failure.kind, FailureKind::UnexpectedExit);
    assert_eq!(failure.exit_code, Some(0));
    assert_eq!(f.runner.runs.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn stop_wins_over_a_pending_manager_restart() {
    let f = fixture(Mode::MarkerOnCancel, true);
    assert_eq!(f.start().await.unwrap().lifecycle, Lifecycle::Running);
    assert_eq!(
        f.supervisor.stop().await.unwrap().lifecycle,
        Lifecycle::Stopped
    );
    assert_eq!(f.runner.runs.load(Ordering::SeqCst), 1);
    assert!(!f.marker().exists());
}
#[tokio::test]
async fn manager_restarts_before_readiness_are_limited() {
    let f = fixture(Mode::MarkerExit, true);
    assert!(f.start().await.is_err());
    let failure = f.supervisor.status().await.failure.unwrap();
    assert_eq!(failure.kind, FailureKind::ExitBeforeReady);
    assert_eq!(failure.exit_code, Some(0));
    assert_eq!(
        f.runner.runs.load(Ordering::SeqCst),
        MANAGER_RESTARTS_BEFORE_READY + 1
    );
    assert!(!f.marker().exists());
    assert!(f.gate.try_acquire().await.is_ok());
}
