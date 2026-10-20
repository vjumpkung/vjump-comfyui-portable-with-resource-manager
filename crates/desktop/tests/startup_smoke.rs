#![cfg(all(target_os = "windows", debug_assertions))]

use std::{
    fs::{self, File},
    io,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_FILE_NOT_FOUND, GetLastError, HWND, LPARAM},
    System::Threading::{OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE},
    UI::WindowsAndMessaging::{
        EnumWindows, FindWindowW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
        PostMessageW, WM_CLOSE,
    },
};

const SETUP: &str = "application setup finished";
const SHUTDOWN: &str = "application shutdown finished";

struct RunningApp {
    child: Child,
    log: PathBuf,
    stdout: PathBuf,
    stderr: PathBuf,
}

impl Drop for RunningApp {
    fn drop(&mut self) {
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[test]
#[ignore = "native: needs WebView2, an interactive desktop and opens a visible window; run explicitly"]
fn fresh_root_starts_and_closes_cleanly() {
    assert_single_instance_absent().unwrap_or_else(|error| panic!("{error}"));
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path().join("install");
    let stdout = scratch.path().join("stdout.log");
    let stderr = scratch.path().join("stderr.log");
    let child = Command::new(env!("CARGO_BIN_EXE_vjumpkung-comfyui-desktop"))
        .env("VJUMPKUNG_COMFYUI_HOME", &root)
        .env("RUST_BACKTRACE", "1")
        .env("RUST_LOG", "info")
        .stdin(Stdio::null())
        .stdout(File::create(&stdout).unwrap())
        .stderr(File::create(&stderr).unwrap())
        .spawn()
        .expect("could not spawn the desktop smoke-test child");
    let mut app = RunningApp {
        child,
        log: root.join("data/logs/app.log"),
        stdout,
        stderr,
    };

    if let Err(error) = check_startup_and_close(&mut app) {
        let before_cleanup = app.child.try_wait();
        let cleanup_status = if matches!(before_cleanup, Ok(Some(_))) {
            None
        } else {
            let _ = app.child.kill();
            Some(app.child.wait())
        };
        panic!(
            "{error}\nexit status: {before_cleanup:?}; cleanup exit status: {cleanup_status:?}\n\
             stderr tail:\n{}\nstdout tail:\n{}",
            tail(&app.stderr),
            tail(&app.stdout),
        );
    }
}

fn check_startup_and_close(app: &mut RunningApp) -> Result<(), String> {
    let startup_deadline = Instant::now() + Duration::from_secs(60);
    loop {
        require_alive(&mut app.child, "before the setup marker")?;
        if read_capture(&app.log).contains(SETUP) || read_capture(&app.stdout).contains(SETUP) {
            break;
        }
        if Instant::now() >= startup_deadline {
            return Err("setup marker was not observed within 60 s".into());
        }
        thread::sleep(Duration::from_millis(100));
    }

    let window = loop {
        require_alive(&mut app.child, "while finding the manager window")?;
        if let Some(window) = find_manager_window(app.child.id())? {
            break window;
        }
        if Instant::now() >= startup_deadline {
            return Err(
                "visible manager window for the child PID was not found within 60 s".into(),
            );
        }
        thread::sleep(Duration::from_millis(100));
    };
    thread::sleep(Duration::from_secs(3));
    require_alive(&mut app.child, "3 s after setup and window discovery")?;

    if unsafe { PostMessageW(window, WM_CLOSE, 0, 0) } == 0 {
        return Err(format!("WM_CLOSE failed: {}", io::Error::last_os_error()));
    }
    let close_deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        if let Some(status) = child_status(&mut app.child)? {
            break status;
        }
        if Instant::now() >= close_deadline {
            return Err(
                "graceful close did not exit within 15 s; forced cleanup is a failure".into(),
            );
        }
        thread::sleep(Duration::from_millis(100));
    };
    if status.code() != Some(0) {
        return Err(format!("graceful close returned {status}"));
    }
    let log = read_capture(&app.log);
    if !log
        .find(SETUP)
        .is_some_and(|setup| log[setup + SETUP.len()..].contains(SHUTDOWN))
    {
        return Err("log must contain the shutdown marker after the setup marker".into());
    }
    Ok(())
}

fn child_status(child: &mut Child) -> Result<Option<ExitStatus>, String> {
    child
        .try_wait()
        .map_err(|error| format!("could not read child exit status: {error}"))
}

fn require_alive(child: &mut Child, phase: &str) -> Result<(), String> {
    if let Some(status) = child_status(child)? {
        return Err(format!("child exited {phase}: {status}"));
    }
    Ok(())
}

fn read_capture(path: &Path) -> String {
    String::from_utf8_lossy(&fs::read(path).unwrap_or_default()).into_owned()
}

fn tail(path: &Path) -> String {
    let text = read_capture(path);
    let mut lines: Vec<_> = text.lines().rev().take(40).collect();
    lines.reverse();
    lines.join("\n")
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn assert_single_instance_absent() -> Result<(), String> {
    // Names from tauri-plugin-single-instance 2.5.3 (the semver feature is disabled).
    let class = wide("dev.vjump.comfyui-desktop-sic");
    let title = wide("dev.vjump.comfyui-desktop-siw");
    if !unsafe { FindWindowW(class.as_ptr(), title.as_ptr()) }.is_null() {
        return Err("precondition failed: an app instance already owns the single-instance window; close it before running this test".into());
    }
    let name = wide("dev.vjump.comfyui-desktop-sim");
    let mutex = unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, 0, name.as_ptr()) };
    if !mutex.is_null() {
        unsafe { CloseHandle(mutex) };
        return Err("precondition failed: an app instance already owns the single-instance mutex; close it before running this test".into());
    }
    let error = unsafe { GetLastError() };
    if error != ERROR_FILE_NOT_FOUND {
        return Err(format!(
            "precondition failed: could not establish that the single-instance mutex is absent: {}",
            io::Error::from_raw_os_error(error as i32)
        ));
    }
    Ok(())
}

struct WindowSearch {
    pid: u32,
    window: HWND,
}

unsafe extern "system" fn inspect_window(window: HWND, data: LPARAM) -> i32 {
    // EnumWindows passes the live WindowSearch for this synchronous enumeration.
    let search = unsafe { &mut *(data as *mut WindowSearch) };
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(window, &mut pid) };
    if pid == search.pid && unsafe { IsWindowVisible(window) } != 0 {
        let mut title = [0; 256];
        let length = unsafe { GetWindowTextW(window, title.as_mut_ptr(), title.len() as i32) };
        if String::from_utf16_lossy(&title[..length as usize]) == "vjumpkung ComfyUI" {
            search.window = window;
            return 0;
        }
    }
    1
}

fn find_manager_window(pid: u32) -> Result<Option<HWND>, String> {
    let mut search = WindowSearch {
        pid,
        window: std::ptr::null_mut(),
    };
    let result = unsafe { EnumWindows(Some(inspect_window), &mut search as *mut _ as LPARAM) };
    if !search.window.is_null() {
        Ok(Some(search.window))
    } else if result == 0 {
        Err(format!(
            "EnumWindows failed: {}",
            io::Error::last_os_error()
        ))
    } else {
        Ok(None)
    }
}
