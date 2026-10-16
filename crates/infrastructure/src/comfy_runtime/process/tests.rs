use super::*;
use crate::shared::gate::TokioMaintenanceGate;
use application::shared::MaintenanceGate;
use domain::environment::EnvId;
use std::sync::atomic::{AtomicBool, Ordering};
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
}
struct Runner {
    mode: Mode,
    cleanup: bool,
    gate: Arc<TokioMaintenanceGate>,
    gate_held_during_cleanup: AtomicBool,
    spawned: AtomicBool,
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
        if matches!(self.mode, Mode::Spawn) {
            return ProcessCompletion {
                exit_code: None,
                spawn_failed: true,
                cleanup_verified: true,
            };
        }
        let alive = Arc::new(AtomicBool::new(true));
        observer.spawned(Arc::new(Owned {
            alive: alive.clone(),
            member: !matches!(self.mode, Mode::Foreign),
        }));
        self.spawned.store(true, Ordering::SeqCst);
        observer.chunk(false, "ไทย token=sec".as_bytes());
        observer.chunk(false, b"ret-value\rready\n");
        if matches!(self.mode, Mode::Exit) {
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
        let listener = if matches!(self.mode, Mode::Ready | Mode::Foreign) {
            Some(TcpListener::bind(("127.0.0.1", port)).await.unwrap())
        } else {
            None
        };
        while !cancel.is_cancelled() {
            tokio::select! {
                _=tokio::time::sleep(Duration::from_millis(10))=>{},
                accepted=async{listener.as_ref().unwrap().accept().await},if listener.is_some()=>{
                    let (mut socket,_)=accepted.unwrap();let mut request=[0;4096];let _=socket.read(&mut request).await;
                    let body=r#"{"system":{},"devices":[]}"#;
                    let reply=format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
                    let _=socket.write_all(reply.as_bytes()).await;
                }
            }
        }
        self.gate_held_during_cleanup
            .store(self.gate.try_acquire().await.is_err(), Ordering::SeqCst);
        alive.store(false, Ordering::SeqCst);
        drop(listener);
        ProcessCompletion {
            exit_code: None,
            spawn_failed: false,
            cleanup_verified: self.cleanup,
        }
    }
}
struct Fixture {
    _directory: tempfile::TempDir,
    session: RuntimeSession,
    supervisor: ComfyRuntimeSupervisor,
    gate: Arc<TokioMaintenanceGate>,
    runner: Arc<Runner>,
    store: Arc<Store>,
}
fn fixture(mode: Mode, cleanup: bool) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    std::fs::create_dir(root.join("ComfyUI")).unwrap();
    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = Port::new(socket.local_addr().unwrap().port()).unwrap();
    drop(socket);
    let gate = Arc::new(TokioMaintenanceGate::new());
    let store = Arc::new(Store::default());
    let runner = Arc::new(Runner {
        mode,
        cleanup,
        gate: gate.clone(),
        gate_held_during_cleanup: AtomicBool::new(false),
        spawned: AtomicBool::new(false),
    });
    let supervisor = ComfyRuntimeSupervisor::with_readiness_timeout(
        root.clone(),
        runner.clone(),
        store.clone(),
        RuntimeLogHub::default(),
        Duration::from_millis(700),
    );
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
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
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
        let f = fixture(mode, true);
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
    while !f.runner.spawned.load(Ordering::SeqCst) {
        tokio::task::yield_now().await;
    }
    let stopped = f.supervisor.stop().await.unwrap();
    assert_eq!(stopped.lifecycle, Lifecycle::Stopped);
    assert_eq!(start.await.unwrap().unwrap().lifecycle, Lifecycle::Stopped);
    assert!(f.runner.gate_held_during_cleanup.load(Ordering::SeqCst));
}
#[tokio::test]
async fn failed_cleanup_quarantines_the_gate_and_m1_maintenance() {
    let f = fixture(Mode::NeverReady, false);
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
    while !f.runner.spawned.load(Ordering::SeqCst) {
        tokio::task::yield_now().await;
    }
    caller.abort();
    let _ = caller.await;
    assert_eq!(
        f.supervisor.stop().await.unwrap().lifecycle,
        Lifecycle::Stopped
    );
    assert!(f.runner.gate_held_during_cleanup.load(Ordering::SeqCst));
}
