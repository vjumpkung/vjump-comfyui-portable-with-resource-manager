use std::{
    ffi::OsString,
    time::{Duration, Instant},
};

use application::shared::CancelToken;

use super::*;
use crate::shared::{
    process::{CommandSpec, DiscardSink, RunError, RunLimits, run, system_baseline},
    procscan::test_snapshot,
    test_http::{Route, TestServer},
};

const OWN_PID: u32 = 4242;

fn process(pid: u32, name: &str) -> ProcessInfo {
    ProcessInfo {
        pid,
        name: name.to_owned(),
        exe: Some(PathBuf::from("/usr/bin").join(name)),
        cwd: Some(PathBuf::from("/home/user")),
        cmd: vec![name.to_owned()],
        parent: None,
    }
}

fn root() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"D:\Apps\ComfyUI Desktop ไทย")
    } else {
        PathBuf::from("/opt/comfy desktop")
    }
}

fn snapshot_with(extra: Vec<ProcessInfo>) -> Vec<ProcessInfo> {
    let mut all = vec![process(OWN_PID, "vjumpkung-comfyui-desktop")];
    all.extend(extra);
    all
}

/// A process whose command line is `python <args>` with `cwd` as its working directory.
fn python_running(pid: u32, cwd: &Path, args: &[&Path]) -> ProcessInfo {
    let mut python = process(pid, "python");
    python.cwd = Some(cwd.to_owned());
    python.cmd = vec!["python".to_owned()];
    python
        .cmd
        .extend(args.iter().map(|arg| arg.to_string_lossy().into_owned()));
    python
}

#[test]
fn only_this_installations_main_py_counts() {
    let root = root();
    let comfyui = root.join("ComfyUI");
    let main = Path::new("main.py");

    let relative = python_running(1, &comfyui, &[main]);
    assert_eq!(
        assess(&snapshot_with(vec![relative]), &root, OWN_PID),
        Verdict::Running(1)
    );

    let from_root = python_running(
        2,
        &root,
        &[Path::new("-s"), &Path::new("ComfyUI").join(main)],
    );
    assert_eq!(
        assess(&snapshot_with(vec![from_root]), &root, OWN_PID),
        Verdict::Running(2)
    );

    let absolute = python_running(3, Path::new("/home/user"), &[&comfyui.join(main)]);
    assert_eq!(
        assess(&snapshot_with(vec![absolute]), &root, OWN_PID),
        Verdict::Running(3)
    );

    let parent_relative = python_running(
        4,
        &root.join("tools"),
        &[&Path::new("..").join("ComfyUI").join(main)],
    );
    assert_eq!(
        assess(&snapshot_with(vec![parent_relative]), &root, OWN_PID),
        Verdict::Running(4)
    );

    let dotted = python_running(5, &comfyui, &[&Path::new(".").join(main)]);
    assert_eq!(
        assess(&snapshot_with(vec![dotted]), &root, OWN_PID),
        Verdict::Running(5)
    );
}

#[test]
fn programs_that_only_use_the_folders_do_not_count() {
    let root = root();
    let comfyui = root.join("ComfyUI");

    let mut terminal = process(1, "cmd.exe");
    terminal.cwd = Some(comfyui.join("custom_nodes").join("x"));
    terminal.cmd = vec!["cmd.exe".into()];

    let mut explorer = process(2, "explorer.exe");
    explorer.cmd = vec![
        "explorer.exe".into(),
        comfyui.join("output").to_string_lossy().into_owned(),
    ];

    let mut viewer = process(3, "Photos.exe");
    viewer.cmd = vec![
        "Photos.exe".into(),
        comfyui
            .join("output")
            .join("x.png")
            .to_string_lossy()
            .into_owned(),
    ];

    let node_script = python_running(
        4,
        &comfyui,
        &[&Path::new("custom_nodes").join("x").join("main.py")],
    );

    let mut env_python = process(5, "python");
    env_python.exe = Some(
        root.join("envs")
            .join("env-1")
            .join("Scripts")
            .join("python.exe"),
    );
    env_python.cmd = vec!["python".into(), "script.py".into()];

    // Downloads run from the tools folder and are not part of the maintenance gate.
    let mut tool = process(6, "hf");
    tool.exe = Some(root.join("tools").join("hf").join("Scripts").join("hf.exe"));

    for other in [terminal, explorer, viewer, node_script, env_python, tool] {
        let pid = other.pid;
        assert_eq!(
            assess(&snapshot_with(vec![other]), &root, OWN_PID),
            Verdict::Stopped,
            "process {pid}"
        );
    }
}

#[test]
fn unrelated_neighbours_and_the_app_itself_do_not_block() {
    let root = root();
    let other_install = PathBuf::from(format!("{}-other", root.display()));
    let sibling = python_running(
        1,
        Path::new("/home/user"),
        &[&other_install.join("ComfyUI").join("main.py")],
    );
    let prefix_only = python_running(
        2,
        Path::new("/home/user"),
        &[&root.join("ComfyUIExtra").join("main.py")],
    );
    let mut app = python_running(OWN_PID, &root.join("ComfyUI"), &[Path::new("main.py")]);
    app.name = "app".into();
    let mut all = snapshot_with(vec![sibling, prefix_only, process(3, "notepad")]);
    all[0] = app;
    assert_eq!(assess(&all, &root, OWN_PID), Verdict::Stopped);
}

#[test]
fn an_unreadable_python_is_flagged_and_an_empty_snapshot_cannot_be_trusted() {
    let root = root();
    let hidden = ProcessInfo {
        pid: 9,
        name: "python.exe".into(),
        exe: None,
        cwd: None,
        cmd: Vec::new(),
        parent: None,
    };
    assert_eq!(
        assess(&snapshot_with(vec![hidden]), &root, OWN_PID),
        Verdict::Unreadable
    );
    let hidden_other = ProcessInfo {
        pid: 9,
        name: "svchost.exe".into(),
        exe: None,
        cwd: None,
        cmd: Vec::new(),
        parent: None,
    };
    assert_eq!(
        assess(&snapshot_with(vec![hidden_other]), &root, OWN_PID),
        Verdict::Stopped
    );
    assert_eq!(assess(&[], &root, OWN_PID), Verdict::SnapshotFailed);
    assert_eq!(
        assess(&[process(1, "x")], &root, OWN_PID),
        Verdict::SnapshotFailed
    );
}

#[test]
fn a_relative_main_py_with_an_unreadable_cwd_is_unknown_only_on_windows() {
    let root = root();
    // On Windows a readable command line with an unreadable cwd is an elevated process; on Linux
    // another user's `python main.py` is the normal case and must not block anything.
    let expected = if cfg!(windows) {
        Verdict::Unreadable
    } else {
        Verdict::Stopped
    };
    for name in ["python", "uv"] {
        let mut hidden_cwd = process(9, name);
        hidden_cwd.cwd = None;
        hidden_cwd.cmd = vec![name.to_owned(), "main.py".into()];
        assert_eq!(
            assess(&snapshot_with(vec![hidden_cwd]), &root, OWN_PID),
            expected,
            "{name}"
        );
    }

    // An absolute path needs no working directory to be placed.
    let mut other_install = process(9, "python");
    other_install.cwd = None;
    other_install.cmd = vec![
        "python".into(),
        root.with_file_name("elsewhere")
            .join("main.py")
            .to_string_lossy()
            .into_owned(),
    ];
    assert_eq!(
        assess(&snapshot_with(vec![other_install]), &root, OWN_PID),
        Verdict::Stopped
    );
}

#[cfg(windows)]
#[test]
fn windows_paths_compare_case_insensitively() {
    let root = root();
    let shouting = python_running(
        1,
        Path::new(r"C:\Users"),
        &[Path::new(r"D:\APPS\COMFYUI DESKTOP ไทย\COMFYUI\MAIN.PY")],
    );
    assert_eq!(
        assess(&snapshot_with(vec![shouting]), &root, OWN_PID),
        Verdict::Running(1)
    );
}

fn env_dir() -> PathBuf {
    root().join("envs").join("env-1")
}

/// A python from the environment's `Scripts` folder, started by `parent`.
fn python_in_env(pid: u32, parent: Option<u32>) -> ProcessInfo {
    let mut python = process(pid, "python.exe");
    python.exe = Some(env_dir().join("Scripts").join("python.exe"));
    python.parent = parent;
    python
}

#[test]
fn environment_user_names_a_program_running_from_the_environment() {
    let mut elsewhere = process(1, "notepad.exe");
    elsewhere.exe = Some(root().join("tools").join("notepad.exe"));
    let all = snapshot_with(vec![elsewhere, python_in_env(7, Some(1))]);
    assert_eq!(
        environment_user(&all, &env_dir(), OWN_PID),
        Some(EnvironmentUser {
            pid: 7,
            name: "python.exe".into()
        })
    );
    assert_eq!(
        environment_user(&snapshot_with(Vec::new()), &env_dir(), OWN_PID),
        None
    );
}

#[test]
fn environment_user_matches_whole_path_components_only() {
    let mut sibling = process(7, "python.exe");
    sibling.exe = Some(
        root()
            .join("envs")
            .join("env-10")
            .join("Scripts")
            .join("python.exe"),
    );
    assert_eq!(
        environment_user(&snapshot_with(vec![sibling]), &env_dir(), OWN_PID),
        None
    );
}

#[test]
fn environment_user_ignores_this_app_and_everything_it_started() {
    let mut app = python_in_env(OWN_PID, None);
    app.name = "app".into();
    let child = python_in_env(10, Some(OWN_PID));
    let grandchild = python_in_env(11, Some(10));
    let mut all = snapshot_with(vec![child, grandchild]);
    all[0] = app;
    assert_eq!(environment_user(&all, &env_dir(), OWN_PID), None);
}

#[test]
fn environment_user_still_reports_an_unrelated_process_tree() {
    let child = python_in_env(10, Some(OWN_PID));
    let launcher = python_in_env(20, Some(21));
    let all = snapshot_with(vec![child, launcher, python_in_env(21, Some(22))]);
    assert_eq!(
        environment_user(&all, &env_dir(), OWN_PID).map(|user| user.pid),
        Some(20)
    );
}

#[test]
fn environment_user_ignores_processes_whose_executable_cannot_be_read() {
    let mut hidden = python_in_env(7, None);
    hidden.exe = None;
    assert_eq!(
        environment_user(&snapshot_with(vec![hidden]), &env_dir(), OWN_PID),
        None
    );
}

#[test]
fn environment_user_terminates_on_parent_cycles() {
    let all = snapshot_with(vec![
        python_in_env(7, Some(8)),
        python_in_env(8, Some(7)),
        python_in_env(9, Some(9)),
    ]);
    assert_eq!(
        environment_user(&all, &env_dir(), OWN_PID).map(|user| user.pid),
        Some(7)
    );
    let parents = HashMap::from([(7, 8), (8, 7)]);
    assert!(!descends_from(7, OWN_PID, &parents));
}

#[cfg(windows)]
#[test]
fn environment_user_compares_paths_case_insensitively() {
    let mut shouting = process(7, "PYTHON.EXE");
    shouting.exe = Some(PathBuf::from(
        r"D:\APPS\COMFYUI DESKTOP ไทย\ENVS\ENV-1\SCRIPTS\PYTHON.EXE",
    ));
    assert_eq!(
        environment_user(&snapshot_with(vec![shouting]), &env_dir(), OWN_PID).map(|user| user.pid),
        Some(7)
    );
}
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn port_of(server: &TestServer) -> u16 {
    server.base_url.rsplit(':').next().unwrap().parse().unwrap()
}

#[tokio::test]
async fn a_machine_without_comfyui_is_stopped() {
    let directory = tempfile::tempdir().unwrap();
    let guard = ProcessRuntimeGuard::with_port(free_port());
    guard
        .ensure_stopped(&directory.path().join("install"))
        .await
        .unwrap();
}

#[tokio::test]
async fn a_comfyui_answering_on_the_configured_port_blocks_maintenance() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("install");
    let server = TestServer::start().await;
    let mut guard = ProcessRuntimeGuard::with_port(port_of(&server));
    // An open port is only trusted when the process list is assessable; a python caught starting
    // or exiting by another parallel test is not, and would turn "not ComfyUI" into a refusal.
    guard.snapshot = test_snapshot;

    server.route(
        "/system_stats",
        Route::ok(br#"{"system":{"comfyui_version":"0.40.0"},"devices":[]}"#.to_vec()),
    );
    assert_eq!(
        guard.ensure_stopped(&root).await,
        Err(RuntimeBlock::Running)
    );
    server.route(
        "/system_stats",
        Route::ok(br#"{"system":{},"devices":[]}"#.to_vec()),
    );
    assert_eq!(
        guard.ensure_stopped(&root).await,
        Err(RuntimeBlock::Running)
    );

    // Something else on the port is not ComfyUI.
    server.route(
        "/system_stats",
        Route::ok(b"hello from another service".to_vec()),
    );
    assert!(guard.ensure_stopped(&root).await.is_ok());
    server.route("/system_stats", Route::status(404, "nope"));
    assert!(guard.ensure_stopped(&root).await.is_ok());
}

/// A process that sleeps for a long time with `cwd` as its working directory. With `marker`, its
/// command line also carries a relative `main.py` argument, like `python main.py` would.
fn sleeper(cwd: &Path, marker: bool) -> CommandSpec {
    let mut env: Vec<(OsString, OsString)> = system_baseline();
    let system_root = std::env::var("SystemRoot").unwrap_or_default();
    let spec = if cfg!(windows) {
        env.push(("PATH".into(), format!(r"{system_root}\System32").into()));
        if marker {
            CommandSpec::new(format!(r"{system_root}\System32\cmd.exe")).args([
                "/d",
                "/c",
                "ping",
                "-n",
                "953",
                "127.0.0.1",
                ">nul",
                "&",
                "rem",
                "main.py",
            ])
        } else {
            CommandSpec::new(format!(r"{system_root}\System32\PING.EXE")).args([
                "-n",
                "953",
                "127.0.0.1",
            ])
        }
    } else {
        env.push(("PATH".into(), "/usr/bin:/bin".into()));
        if marker {
            CommandSpec::new("/bin/sh").args(["-c", "sleep 953; true", "main.py"])
        } else {
            CommandSpec::new("/bin/sleep").args(["953"])
        }
    };
    spec.cwd(cwd).env_vars(env)
}

#[tokio::test]
async fn a_live_comfyui_of_the_install_blocks_until_it_is_gone() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("install ไทย");
    let comfyui = root.join("ComfyUI");
    std::fs::create_dir_all(&comfyui).unwrap();
    let guard = ProcessRuntimeGuard::with_port(free_port());
    assert!(guard.ensure_stopped(&root).await.is_ok());

    let spec = sleeper(&comfyui, true);
    let cancel = CancelToken::new();
    let runner = {
        let cancel = cancel.clone();
        tokio::spawn(async move { run(&spec, &DiscardSink, &cancel, &RunLimits::default()).await })
    };
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut blocked = false;
    while Instant::now() < deadline {
        if guard.ensure_stopped(&root).await == Err(RuntimeBlock::Running) {
            blocked = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        blocked,
        "a process running ComfyUI/main.py must block maintenance"
    );
    cancel.cancel();
    assert!(matches!(
        runner.await.unwrap(),
        Err(RunError::Cancelled { .. })
    ));
    assert!(
        guard.ensure_stopped(&root).await.is_ok(),
        "after the process tree is gone maintenance is allowed again"
    );
}

#[tokio::test]
async fn a_live_terminal_in_the_folder_does_not_block() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("install ไทย");
    let comfyui = root.join("ComfyUI");
    std::fs::create_dir_all(&comfyui).unwrap();
    let guard = ProcessRuntimeGuard::with_port(free_port());

    let spec = sleeper(&comfyui, false);
    let cancel = CancelToken::new();
    let runner = {
        let cancel = cancel.clone();
        tokio::spawn(async move { run(&spec, &DiscardSink, &cancel, &RunLimits::default()).await })
    };
    let wanted = normalized_path(&comfyui);
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut visible = false;
    while Instant::now() < deadline {
        let processes = tokio::task::spawn_blocking(snapshot).await.unwrap();
        visible = processes.iter().any(|process| {
            process
                .cwd
                .as_deref()
                .is_some_and(|cwd| normalized_path(cwd) == wanted)
        });
        if visible {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(visible, "the process in ComfyUI/ must show in the snapshot");
    let verdict = guard.ensure_stopped(&root).await;
    cancel.cancel();
    assert!(matches!(
        runner.await.unwrap(),
        Err(RunError::Cancelled { .. })
    ));
    assert_eq!(
        verdict,
        Ok(()),
        "a program that only sits in ComfyUI/ is no ComfyUI"
    );
}

/// Kills the child when the test ends, however it ends.
struct KillOnDrop(std::process::Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
async fn a_live_child_of_this_app_is_not_an_environment_user() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("env")).unwrap();
    // Canonical, so the comparison does not depend on how the OS spells the temp folder (a
    // windows-latest TEMP is 8.3, such as RUNNER~1); every path below derives from this one value.
    let canonical = directory.path().join("env").canonicalize().unwrap();
    let env = PathBuf::from(canonical.to_string_lossy().trim_start_matches(r"\\?\"));
    let scripts = env.join("Scripts");
    std::fs::create_dir_all(&scripts).unwrap();
    let (original, copy, args): (PathBuf, PathBuf, &[&str]) = if cfg!(windows) {
        let system_root = std::env::var("SystemRoot").unwrap_or_default();
        (
            PathBuf::from(format!(r"{system_root}\System32\PING.EXE")),
            scripts.join("PING.EXE"),
            &["-n", "953", "127.0.0.1"],
        )
    } else {
        (PathBuf::from("/bin/sleep"), scripts.join("sleep"), &["953"])
    };
    std::fs::copy(&original, &copy).unwrap();
    let child = KillOnDrop(
        std::process::Command::new(&copy)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    let child_pid = child.0.id();

    let deadline = Instant::now() + Duration::from_secs(20);
    let mut seen = None;
    while Instant::now() < deadline {
        let processes = tokio::task::spawn_blocking(snapshot).await.unwrap();
        seen = environment_user(&processes, &env, u32::MAX);
        if seen.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    // Seen from another process, the child runs from the environment...
    assert_eq!(seen.map(|user| user.pid), Some(child_pid));
    // ...but it is this app's own child, so the app never counts it as a foreign user.
    assert_eq!(ProcessEnvironmentUsers::new().first_user(&env).await, None);
}
