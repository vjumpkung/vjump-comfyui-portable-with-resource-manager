use std::{
    ffi::OsString,
    path::Path,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

use super::*;
use crate::shared::{logging, procscan};

#[derive(Default)]
struct Lines(Mutex<Vec<String>>);

impl LineSink for Lines {
    fn line(&self, line: &str) {
        self.0.lock().unwrap().push(line.to_owned());
    }
}

impl Lines {
    fn all(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

/// Every test child starts in a throwaway directory, so a stray relative write can never land
/// in the repository.
fn scratch_dir() -> &'static Path {
    static DIR: OnceLock<tempfile::TempDir> = OnceLock::new();
    DIR.get_or_init(|| tempfile::tempdir().expect("scratch directory"))
        .path()
}

fn base_env() -> Vec<(OsString, OsString)> {
    let mut env = system_baseline();
    if cfg!(windows) {
        let root = std::env::var("SystemRoot").expect("SystemRoot");
        env.push(("PATH".into(), format!(r"{root}\System32;{root}").into()));
    } else {
        env.push(("PATH".into(), "/usr/bin:/bin".into()));
    }
    env
}

/// A shell one-liner as a command spec.
fn shell(script: &str) -> CommandSpec {
    let spec = if cfg!(windows) {
        let root = std::env::var("SystemRoot").unwrap();
        CommandSpec::new(format!(r"{root}\System32\cmd.exe")).args(["/d", "/c", script])
    } else {
        CommandSpec::new("/bin/sh").args(["-c", script])
    };
    spec.cwd(scratch_dir()).env_vars(base_env())
}

async fn run_default(spec: &CommandSpec) -> (Result<Output, RunError>, Vec<String>) {
    let sink = Lines::default();
    let result = run(spec, &sink, &CancelToken::new(), &RunLimits::default()).await;
    (result, sink.all())
}

#[test]
fn lines_split_on_lf_crlf_and_carriage_returns() {
    let mut assembler = LineAssembler::new(1024);
    let mut lines = Vec::new();
    let mut collect = |line: &[u8], progress: bool| {
        lines.push((String::from_utf8_lossy(line).into_owned(), progress))
    };
    assembler.push(b"first\nsecond\r\nthird\rfourth", &mut collect);
    assembler.push(b" part\r\n\r\nlast", &mut collect);
    assembler.finish(&mut collect);
    let expected = [
        ("first", false),
        ("second", false),
        ("third", true),
        ("fourth part", false),
        ("last", false),
    ];
    assert_eq!(
        lines,
        expected.map(|(text, progress)| (text.to_owned(), progress)),
        "lines ended by a lone CR are progress updates; CRLF is an ordinary line ending"
    );
    // A CR and the LF that completes it may arrive in different chunks.
    let mut split = Vec::new();
    let mut assembler = LineAssembler::new(1024);
    let mut collect = |line: &[u8], progress: bool| {
        split.push((String::from_utf8_lossy(line).into_owned(), progress))
    };
    assembler.push(b"windows line\r", &mut collect);
    assembler.push(b"\nnext", &mut collect);
    assembler.finish(&mut collect);
    assert_eq!(
        split,
        [
            ("windows line".to_owned(), false),
            ("next".to_owned(), false)
        ]
    );
}

#[test]
fn partial_utf8_sequences_survive_chunk_boundaries_and_long_lines() {
    let text = "ไทย-สวัสดี";
    let bytes = text.as_bytes();
    let mut assembler = LineAssembler::new(1024);
    let mut lines = Vec::new();
    let mut collect = |line: &[u8], _: bool| lines.push(String::from_utf8(line.to_vec()).unwrap());
    for byte in bytes {
        assembler.push(std::slice::from_ref(byte), &mut collect);
    }
    assembler.push(b"\n", &mut collect);
    assert_eq!(lines, [text]);

    // A line longer than the limit is split on a character boundary.
    let long = "ไ".repeat(40);
    let mut assembler = LineAssembler::new(64);
    let mut chunks = Vec::new();
    let mut collect = |line: &[u8], _: bool| {
        chunks.push(String::from_utf8(line.to_vec()).expect("never split inside a character"))
    };
    assembler.push(long.as_bytes(), &mut collect);
    assembler.finish(&mut collect);
    assert!(chunks.len() > 1);
    assert_eq!(chunks.concat(), long);
}

#[tokio::test]
async fn output_is_streamed_captured_and_the_exit_code_reported() {
    let (result, lines) = run_default(&shell("echo hello&& echo world")).await;
    let output = result.unwrap();
    assert_eq!(output.exit_code, 0);
    assert_eq!(lines, ["hello", "world"]);
    assert_eq!(output.stdout, "hello\nworld\n");
    assert_eq!(output.tail, ["hello", "world"]);

    let (result, _) = run_default(&shell("echo before&& exit 3")).await;
    match result {
        Err(RunError::Failed { exit_code, tail }) => {
            assert_eq!(exit_code, Some(3));
            assert_eq!(tail, ["before"]);
        }
        other => panic!("expected a failure, got {other:?}"),
    }
}

#[tokio::test]
async fn a_missing_program_is_a_spawn_error() {
    let spec = CommandSpec::new("definitely-not-a-program.exe").env_vars(base_env());
    let (result, _) = run_default(&spec).await;
    assert!(matches!(result, Err(RunError::Spawn(_))));
}

#[tokio::test]
async fn the_child_only_sees_the_given_environment() {
    // The test process certainly has PATH (and USERPROFILE or HOME); the child must not.
    let script = if cfg!(windows) { "set" } else { "env" };
    let spec = shell(script).env_vars({
        let mut env = base_env();
        env.push(("ALLOWED_VARIABLE".into(), "visible".into()));
        env
    });
    let (result, lines) = run_default(&spec).await;
    result.unwrap();
    let names: Vec<String> = lines
        .iter()
        .filter_map(|line| line.split_once('=').map(|(name, _)| name.to_uppercase()))
        .collect();
    assert!(names.contains(&"ALLOWED_VARIABLE".to_owned()), "{names:?}");
    if cfg!(windows) {
        for required in ["SYSTEMROOT", "SYSTEMDRIVE", "COMSPEC", "PROGRAMDATA"] {
            assert!(
                names.contains(&required.to_owned()),
                "{required} is part of the Windows baseline: {names:?}"
            );
        }
    }
    for inherited in [
        "USERPROFILE",
        "HOME",
        "APPDATA",
        "LOCALAPPDATA",
        "USERNAME",
        "TEMP",
    ] {
        assert!(
            !names.contains(&inherited.to_owned()),
            "{inherited} must not be inherited: {names:?}"
        );
    }
}

#[tokio::test]
async fn output_is_redacted_before_it_reaches_the_sink_the_tail_and_the_capture() {
    logging::register_secret("registered-secret-value-0001");
    let (result, lines) = run_default(&shell(
        "echo Authorization: Bearer abcdef0123456789abcdef&& echo token=supersecretvalue99&& echo registered-secret-value-0001&& echo https://user:hunter2password@host.example/path&& echo X-Amz-Signature=deadbeefdeadbeef&& echo ok",
    ))
    .await;
    let output = result.unwrap();
    for text in lines
        .iter()
        .chain(output.tail.iter())
        .chain(std::iter::once(&output.stdout))
    {
        for secret in [
            "abcdef0123456789abcdef",
            "supersecretvalue99",
            "registered-secret-value-0001",
            "hunter2password",
            "deadbeefdeadbeef",
        ] {
            assert!(!text.contains(secret), "{secret} leaked in {text:?}");
        }
    }
    assert!(lines.contains(&"ok".to_owned()));

    // A failing command keeps redacted diagnostics too.
    let (result, _) = run_default(&shell("echo token=anothersecretvalue1&& exit 1")).await;
    match result {
        Err(RunError::Failed { tail, .. }) => {
            assert!(
                tail.iter()
                    .all(|line| !line.contains("anothersecretvalue1"))
            );
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn the_command_display_is_redacted() {
    let spec = shell("x")
        .arg("--index-url")
        .arg("https://user:hunter2password@host.example/simple")
        .arg("token=abcdef0123456789");
    let display = spec.display();
    assert!(!display.contains("hunter2password"));
    assert!(!display.contains("abcdef0123456789"));
    assert!(display.starts_with("cmd.exe") || display.starts_with("sh"));
}

#[tokio::test]
async fn unicode_and_spaced_working_directories_and_program_paths_work() {
    let directory = tempfile::tempdir().unwrap();
    let cwd = directory.path().join("work ไทย dir");
    std::fs::create_dir_all(&cwd).unwrap();
    let (result, _) = run_default(&shell("echo created> out.txt").cwd(&cwd)).await;
    result.unwrap();
    assert!(
        cwd.join("out.txt").is_file(),
        "the child ran in the Unicode working directory"
    );

    // The program itself lives in a folder with spaces and Thai characters.
    let program_dir = directory.path().join("my tools ไทย");
    std::fs::create_dir_all(&program_dir).unwrap();
    let source = if cfg!(windows) {
        PathBuf::from(std::env::var("SystemRoot").unwrap()).join(r"System32\cmd.exe")
    } else {
        PathBuf::from("/bin/sh")
    };
    let program = program_dir.join(if cfg!(windows) {
        "cmd copy.exe"
    } else {
        "sh copy"
    });
    std::fs::copy(&source, &program).unwrap();
    let spec = if cfg!(windows) {
        CommandSpec::new(&program).args(["/d", "/c", "echo", "two words"])
    } else {
        CommandSpec::new(&program).args(["-c", "echo \"$0\"", "two words"])
    }
    .env_vars(base_env());
    let (result, lines) = run_default(&spec).await;
    result.unwrap();
    assert!(
        lines.iter().any(|line| line.contains("two words")),
        "{lines:?}"
    );
}

#[tokio::test]
async fn a_timeout_ends_the_command() {
    let sleeper = if cfg!(windows) {
        shell("ping -n 30 127.0.0.1 >nul")
    } else {
        shell("sleep 30")
    };
    let limits = RunLimits {
        timeout: Some(Duration::from_millis(300)),
        ..RunLimits::default()
    };
    let started = Instant::now();
    let result = run(&sleeper, &DiscardSink, &CancelToken::new(), &limits).await;
    assert!(
        matches!(result, Err(RunError::TimedOut { .. })),
        "{result:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(10));
}

fn marker_processes(marker: &str) -> Vec<procscan::ProcessInfo> {
    procscan::snapshot()
        .into_iter()
        .filter(|process| process.cmd.join(" ").contains(marker))
        .collect()
}

/// Spawns a shell whose child (and, below it, a long-sleeping grandchild) carries a unique
/// marker, cancels the run and proves that no descendant survives.
async fn assert_tree_is_killed(script: &str, marker: &str) {
    let spec = shell(script);
    let cancel = CancelToken::new();
    let watcher = {
        let cancel = cancel.clone();
        let marker = marker.to_owned();
        tokio::spawn(async move {
            // Wait until the descendant really exists, so the test cannot pass vacuously.
            let deadline = Instant::now() + Duration::from_secs(20);
            while Instant::now() < deadline {
                let found = tokio::task::spawn_blocking({
                    let marker = marker.clone();
                    move || marker_processes(&marker).len()
                })
                .await
                .unwrap();
                if found > 0 {
                    cancel.cancel();
                    return found;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            cancel.cancel();
            0
        })
    };
    let started = Instant::now();
    let limits = RunLimits {
        timeout: Some(Duration::from_secs(60)),
        ..RunLimits::default()
    };
    let result = run(&spec, &DiscardSink, &cancel, &limits).await;
    let seen_before_cancel = watcher.await.unwrap();
    assert!(seen_before_cancel > 0, "the grandchild never started");
    assert!(
        matches!(result, Err(RunError::Cancelled { .. })),
        "{result:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(40));
    let survivors = tokio::task::spawn_blocking({
        let marker = marker.to_owned();
        move || marker_processes(&marker)
    })
    .await
    .unwrap();
    assert!(
        survivors.is_empty(),
        "descendants survived cancellation: {survivors:?}"
    );
}

#[tokio::test]
async fn cancellation_kills_the_child_and_its_grandchild() {
    if cfg!(windows) {
        // cmd.exe is the child; each ping is a grandchild.
        assert_tree_is_killed(
            "ping -n 937 127.0.0.1 >nul & ping -n 938 127.0.0.1 >nul",
            "ping -n 937 127.0.0.1",
        )
        .await;
    } else {
        assert_tree_is_killed("sleep 937 & sleep 938; wait", "sleep 937").await;
    }
}

#[tokio::test]
async fn stragglers_that_outlive_the_main_process_are_cleaned_up() {
    // The shell exits immediately while a background grandchild keeps running with the pipes.
    let script = if cfg!(windows) {
        "start /b ping -n 941 127.0.0.1 >nul & echo started"
    } else {
        "sleep 941 & echo started"
    };
    let marker = if cfg!(windows) {
        "ping -n 941 127.0.0.1"
    } else {
        "sleep 941"
    };
    let started = Instant::now();
    let (result, lines) = run_default(&shell(script)).await;
    result.unwrap();
    assert!(lines.contains(&"started".to_owned()));
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "the run must not wait for the straggler's pipes"
    );
    let survivors = tokio::task::spawn_blocking(move || marker_processes(marker))
        .await
        .unwrap();
    assert!(survivors.is_empty(), "straggler survived: {survivors:?}");
}

#[tokio::test]
async fn concurrent_runs_do_not_interfere() {
    let sink = Arc::new(Lines::default());
    let mut handles = Vec::new();
    for index in 0..4 {
        let sink = sink.clone();
        handles.push(tokio::spawn(async move {
            let spec = shell(&format!("echo run{index}"));
            run(&spec, &*sink, &CancelToken::new(), &RunLimits::default())
                .await
                .map(|output| output.stdout)
        }));
    }
    for (index, handle) in handles.into_iter().enumerate() {
        assert_eq!(handle.await.unwrap().unwrap(), format!("run{index}\n"));
    }
}

#[derive(Default)]
struct ScopeObserver {
    scope: Mutex<Option<Arc<ProcessScope>>>,
    raw: Mutex<Vec<u8>>,
    own_pid_was_member: Mutex<Option<bool>>,
    foreign_pid_was_member: Mutex<Option<bool>>,
}

impl ProcessObserver for ScopeObserver {
    fn spawned(&self, scope: Arc<ProcessScope>) {
        *self.own_pid_was_member.lock().unwrap() = Some(scope.is_alive().unwrap());
        *self.foreign_pid_was_member.lock().unwrap() =
            Some(scope.contains_pid(std::process::id()).unwrap());
        *self.scope.lock().unwrap() = Some(scope);
    }
    fn chunk(&self, _: bool, bytes: &[u8]) {
        self.raw.lock().unwrap().extend_from_slice(bytes);
    }
}

#[tokio::test]
async fn observed_runs_share_the_owned_scope_and_preserve_raw_progress_bytes() {
    let observer = ScopeObserver::default();
    let spec = if cfg!(windows) {
        shell("echo observed&& ping -n 2 127.0.0.1 >nul")
    } else {
        shell("printf 'observed\\rprogress\\n'; sleep 1")
    };
    run_observed(
        &spec,
        &DiscardSink,
        &CancelToken::new(),
        &RunLimits::default(),
        &observer,
    )
    .await
    .unwrap();
    assert_eq!(*observer.own_pid_was_member.lock().unwrap(), Some(true));
    assert_eq!(
        *observer.foreign_pid_was_member.lock().unwrap(),
        Some(false)
    );
    let scope = observer.scope.lock().unwrap().clone().unwrap();
    assert!(!scope.is_alive().unwrap());
    let bytes = observer.raw.lock().unwrap();
    assert!(String::from_utf8_lossy(&bytes).contains("observed"));
    if cfg!(unix) {
        assert!(bytes.contains(&b'\r'));
    }
}

#[test]
fn the_windows_baseline_never_carries_user_specific_variables() {
    for (name, _) in system_baseline() {
        let name = name.to_string_lossy().to_uppercase();
        for forbidden in [
            "USERPROFILE",
            "APPDATA",
            "LOCALAPPDATA",
            "HOME",
            "TEMP",
            "TMP",
            "TOKEN",
        ] {
            assert!(!name.contains(forbidden), "{name}");
        }
    }
    #[cfg(windows)]
    {
        let names: Vec<String> = system_baseline()
            .iter()
            .map(|(name, _)| name.to_string_lossy().to_uppercase())
            .collect();
        assert!(names.contains(&"SYSTEMDRIVE".to_owned()));
        assert!(names.contains(&"SYSTEMROOT".to_owned()));
    }
}

/// Guard against children writing files into the crate directory (for example a literal
/// `%SystemDrive%` folder created when system variables are missing).
#[test]
fn tests_leave_no_files_outside_temp_directories() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let unexpected: Vec<String> = std::fs::read_dir(manifest_dir)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| !matches!(name.as_str(), "Cargo.toml" | "migrations" | "src" | "tests"))
        .collect();
    assert!(
        unexpected.is_empty(),
        "unexpected entries in the crate directory: {unexpected:?}"
    );
}

#[tokio::test]
async fn rapid_progress_updates_are_throttled_and_do_not_flood_the_tail() {
    // 300 progress updates in a burst, then a normal final line.
    // Platform shells make emitting a bare carriage return awkward, so drive the collector
    // directly with the same byte stream a tool such as git would produce.
    let sink = Lines::default();
    let limits = RunLimits::default();
    let collector = Collector {
        sink: &sink,
        limits: &limits,
        state: Mutex::new(State {
            tail: std::collections::VecDeque::new(),
            stdout: String::new(),
            tail_is_progress: false,
            last_progress: None,
        }),
    };
    let mut assembler = LineAssembler::new(1024);
    let mut bytes = Vec::new();
    for index in 0..300 {
        bytes.extend_from_slice(format!("Receiving objects: {index}%\r").as_bytes());
    }
    bytes.extend_from_slice(b"Receiving objects: 100%, done.\n");
    assembler.push(&bytes, |line, progress| {
        collector.push(Stream::Stderr, line, progress)
    });
    let forwarded = sink.all();
    assert!(
        forwarded.len() < 10,
        "a burst of 300 updates must be throttled, got {}",
        forwarded.len()
    );
    assert_eq!(forwarded.last().unwrap(), "Receiving objects: 100%, done.");
    let tail = collector.tail();
    assert_eq!(
        tail,
        ["Receiving objects: 299%", "Receiving objects: 100%, done."],
        "the tail keeps one progress update and the final line, not hundreds of updates"
    );
}

/// Reports its own destruction.
struct DropFlag(Arc<std::sync::atomic::AtomicBool>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

#[tokio::test]
async fn maintenance_lease_survives_cleanup_failure() {
    use application::shared::{GateError, MaintenanceGate};
    use std::sync::atomic::{AtomicBool, Ordering};
    let gate = crate::shared::gate::TokioMaintenanceGate::new();
    let lease = gate.acquire().await.unwrap();
    let alive = Arc::new(AtomicBool::new(true));
    CLEANUP_SURVIVORS.with(|slot| *slot.borrow_mut() = Some(alive.clone()));
    let spec = shell("echo done").retain(lease.retained());
    assert!(matches!(
        run_default(&spec).await.0,
        Err(RunError::CleanupFailed { .. })
    ));
    drop(spec);
    drop(lease);
    assert_eq!(gate.try_acquire().await.err(), Some(GateError::Busy));
    alive.store(false, Ordering::SeqCst);
    tokio::time::timeout(Duration::from_secs(5), gate.acquire())
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn command_retention_merges_lease_and_filesystem_pins() {
    use application::shared::{GateError, MaintenanceGate};
    use std::sync::atomic::{AtomicBool, Ordering};
    let gate = crate::shared::gate::TokioMaintenanceGate::new();
    let lease = gate.acquire().await.unwrap();
    let released = Arc::new(AtomicBool::new(false));
    let pin = Retained::new(Arc::new(DropFlag(released.clone())));
    let alive = Arc::new(AtomicBool::new(true));
    CLEANUP_SURVIVORS.with(|slot| *slot.borrow_mut() = Some(alive.clone()));
    let spec = shell("echo done").retain(pin);
    let ctx = application::environment::StepContext {
        run_id: application::environment::RunId::new("retained-test").unwrap(),
        step: application::environment::StepId::ComfyRequirements,
        sink: Arc::new(application::environment::NullSink),
        cancel: CancelToken::new(),
        allow_source_builds: false,
        retained: lease.retained(),
    };
    assert!(
        crate::environment::exec::run_step(&spec, &ctx, &RunLimits::default())
            .await
            .is_err()
    );
    drop(spec);
    drop(ctx);
    drop(lease);
    assert!(!released.load(Ordering::SeqCst));
    assert_eq!(gate.try_acquire().await.err(), Some(GateError::Busy));
    alive.store(false, Ordering::SeqCst);
    tokio::time::timeout(Duration::from_secs(5), gate.acquire())
        .await
        .unwrap()
        .unwrap();
    assert!(released.load(Ordering::SeqCst));
}

#[tokio::test]
async fn stdin_delivery_failure_cannot_report_success() {
    let spec = shell("exit 0").stdin(Arc::from(vec![b'x'; 16 << 20]));
    assert!(matches!(
        run_default(&spec).await.0,
        Err(RunError::InputFailed { .. })
    ));
}

#[tokio::test]
async fn stdin_writer_obeys_cancellation_and_timeout() {
    for cancelled in [true, false] {
        let spec = shell(if cfg!(windows) {
            "ping -n 999 127.0.0.1 >nul"
        } else {
            "sleep 999"
        })
        .stdin(Arc::from(vec![b'x'; 16 << 20]));
        let cancel = CancelToken::new();
        let limits = RunLimits {
            timeout: (!cancelled).then_some(Duration::from_millis(200)),
            kill_grace: Duration::from_secs(1),
            ..Default::default()
        };
        let future = run(&spec, &DiscardSink, &cancel, &limits);
        let trigger = async {
            if cancelled {
                tokio::time::sleep(Duration::from_millis(200)).await;
                cancel.cancel();
            }
        };
        let started = Instant::now();
        let (result, ()) = tokio::join!(future, trigger);
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(
            if cancelled {
                matches!(result, Err(RunError::Cancelled { .. }))
            } else {
                matches!(result, Err(RunError::TimedOut { .. }))
            },
            "{result:?}"
        );
    }
}

#[cfg(windows)]
#[tokio::test]
async fn windows_children_exclude_cwd_from_default_dll_search() {
    let directory = tempfile::tempdir().unwrap();
    let name = format!("vjump-dll-policy-{}.dll", std::process::id());
    std::fs::write(directory.path().join(&name), b"invalid DLL").unwrap();
    // The helper is started by the runner, like every tool, with the planted file as its cwd.
    let mut spec = CommandSpec::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "shared::process::tests::dll_lookup_child",
            "--ignored",
            "--nocapture",
        ])
        .cwd(directory.path())
        .env_vars(base_env())
        .arg("--test-threads=1");
    spec.env.push(("VJUMP_DLL_LOOKUP_NAME".into(), name.into()));
    run_default(&spec).await.0.unwrap();
}

/// The process-wide policy applies to the process that sets it. Windows 11 does not hand it
/// to child processes (verified with `CreateProcess` children): a process that must exclude its
/// working directory from the default DLL search has to run the initializer itself, as the app
/// and the runner's spawn path do.
#[cfg(windows)]
#[test]
#[ignore = "helper executed only by windows_children_exclude_cwd_from_default_dll_search"]
fn dll_lookup_child() {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::LibraryLoader::LoadLibraryW;
    const BAD_EXE_FORMAT: i32 = 193;
    const MOD_NOT_FOUND: i32 = 126;
    let name = std::env::var_os("VJUMP_DLL_LOOKUP_NAME").expect("parent supplies unique DLL name");
    let name: Vec<u16> = name.encode_wide().chain(Some(0)).collect();
    // A cwd hit reports ERROR_BAD_EXE_FORMAT; excluding cwd reports ERROR_MOD_NOT_FOUND.
    // SAFETY: the DLL name is terminated and valid throughout this call.
    let module = unsafe { LoadLibraryW(name.as_ptr()) };
    assert!(module.is_null());
    let before = io::Error::last_os_error().raw_os_error();
    assert!(
        matches!(before, Some(BAD_EXE_FORMAT | MOD_NOT_FOUND)),
        "unexpected error before the policy: {before:?}"
    );
    initialize_tool_policy().unwrap();
    // SAFETY: this repeats the same terminated DLL name after applying the policy.
    let module = unsafe { LoadLibraryW(name.as_ptr()) };
    assert!(module.is_null());
    assert_eq!(
        io::Error::last_os_error().raw_os_error(),
        Some(MOD_NOT_FOUND),
        "the policy must exclude cwd"
    );
}

#[tokio::test]
async fn cleanup_failure_retains_execution_resources_until_children_exit() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let released = Arc::new(AtomicBool::new(false));
    let survivors = Arc::new(AtomicBool::new(true));
    let retained = Retained::new(Arc::new(DropFlag(released.clone())));
    let alive = survivors.clone();
    retain_until_empty(retained.clone(), move || {
        let alive = alive.clone();
        async move { !alive.load(Ordering::SeqCst) }
    });
    // The caller drops its own copy, as it does when the failed command returns.
    drop(retained);
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert!(
        !released.load(Ordering::SeqCst),
        "protection ended while descendants were still running"
    );
    survivors.store(false, Ordering::SeqCst);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !released.load(Ordering::SeqCst) {
        assert!(Instant::now() < deadline, "protection was never released");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn stdin_bytes_reach_the_child_and_a_retained_copy_is_not_needed_afterwards() {
    let script = if cfg!(windows) { "more" } else { "cat" };
    let released = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let retained = Retained::new(Arc::new(DropFlag(released.clone())));
    let spec = shell(script)
        .stdin(Arc::from(
            &b"alpha
beta
"[..],
        ))
        .retain(retained.clone());
    let (result, lines) = run_default(&spec).await;
    assert_eq!(result.unwrap().exit_code, 0);
    assert!(
        lines.iter().any(|line| line.contains("alpha"))
            && lines.iter().any(|line| line.contains("beta")),
        "{lines:?}"
    );
    // A command which ended cleanly keeps no protection of its own.
    drop((spec, retained));
    assert!(released.load(std::sync::atomic::Ordering::SeqCst));
}
