use std::{
    ffi::OsString,
    time::{Duration, Instant},
};

use application::shared::CancelToken;

use super::*;
use crate::shared::{
    process::{CommandSpec, DiscardSink, RunError, RunLimits, run, system_baseline},
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

#[test]
fn processes_running_from_the_install_are_detected() {
    let root = root();
    let comfyui = root.join("ComfyUI");
    let envs = root.join("envs").join("env-1");

    let mut by_cwd = process(1, "python");
    by_cwd.cwd = Some(comfyui.join("custom_nodes"));
    assert_eq!(
        assess(&snapshot_with(vec![by_cwd]), &root, OWN_PID),
        Verdict::Running(1)
    );

    let mut by_exe = process(2, "python");
    by_exe.exe = Some(envs.join("Scripts").join("python.exe"));
    assert_eq!(
        assess(&snapshot_with(vec![by_exe]), &root, OWN_PID),
        Verdict::Running(2)
    );

    let mut by_cmd = process(3, "python");
    by_cmd.cmd = vec![
        "python".into(),
        comfyui.join("main.py").to_string_lossy().into_owned(),
    ];
    assert_eq!(
        assess(&snapshot_with(vec![by_cmd]), &root, OWN_PID),
        Verdict::Running(3)
    );

    // Downloads run from the tools folder and are not part of the maintenance gate.
    let mut tool = process(4, "hf");
    tool.exe = Some(root.join("tools").join("hf").join("Scripts").join("hf.exe"));
    assert_eq!(
        assess(&snapshot_with(vec![tool]), &root, OWN_PID),
        Verdict::Stopped
    );
}

#[test]
fn unrelated_neighbours_and_the_app_itself_do_not_block() {
    let root = root();
    let mut sibling = process(1, "python");
    sibling.cwd = Some(PathBuf::from(format!("{}-other/ComfyUI", root.display())));
    let mut prefix_only = process(2, "python");
    prefix_only.cwd = Some(PathBuf::from(format!("{}/ComfyUIExtra", root.display())));
    let mut app = process(OWN_PID, "app");
    app.cwd = Some(root.join("ComfyUI"));
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

#[cfg(windows)]
#[test]
fn windows_paths_compare_case_insensitively() {
    let root = root();
    let mut shouting = process(1, "python");
    shouting.cwd = Some(PathBuf::from(r"d:\APPS\comfyui desktop ไทย\COMFYUI"));
    assert_eq!(
        assess(&snapshot_with(vec![shouting]), &root, OWN_PID),
        Verdict::Running(1)
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
    let guard = ProcessRuntimeGuard::with_port(port_of(&server));

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

#[tokio::test]
async fn a_live_process_from_the_install_blocks_until_it_is_gone() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("install ไทย");
    let comfyui = root.join("ComfyUI");
    std::fs::create_dir_all(&comfyui).unwrap();
    let guard = ProcessRuntimeGuard::with_port(free_port());
    assert!(guard.ensure_stopped(&root).await.is_ok());

    let mut env: Vec<(OsString, OsString)> = system_baseline();
    let system_root = std::env::var("SystemRoot").unwrap_or_default();
    let spec = if cfg!(windows) {
        env.push(("PATH".into(), format!(r"{system_root}\System32").into()));
        CommandSpec::new(format!(r"{system_root}\System32\cmd.exe")).args([
            "/d",
            "/c",
            "ping -n 953 127.0.0.1 >nul",
        ])
    } else {
        env.push(("PATH".into(), "/usr/bin:/bin".into()));
        CommandSpec::new("/bin/sh").args(["-c", "sleep 953"])
    }
    .cwd(&comfyui)
    .env_vars(env);

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
        "a process running in ComfyUI/ must block maintenance"
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
