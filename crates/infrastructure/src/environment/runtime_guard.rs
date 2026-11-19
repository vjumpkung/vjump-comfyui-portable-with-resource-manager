//! The M1 runtime-maintenance adapter: a real process-state preflight. Maintenance is refused
//! while ComfyUI of this installation runs (a process whose command line runs
//! `<root>/ComfyUI/main.py`) or while a ComfyUI answers on the configured port and cannot be
//! shown to belong to another installation. Programs that only use the folders, including Pythons
//! from `envs/`, do not count; `ProcessEnvironmentUsers` separately guards package changes. M2
//! replaces the refusal with an owned-runtime stop.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Duration,
};

use application::{
    comfy_runtime::{RuntimeBlock, RuntimeMaintenance},
    environment::{EnvironmentUser, EnvironmentUsers, normalized_path},
};
use async_trait::async_trait;

use super::layout::Layout;
use crate::shared::procscan::{ProcessInfo, SnapshotFn, snapshot};

/// The default ComfyUI port.
pub const DEFAULT_PORT: u16 = 8188;

pub struct ProcessRuntimeGuard {
    port: u16,
    /// The process-list source; always the real one outside tests.
    snapshot: SnapshotFn,
}

impl ProcessRuntimeGuard {
    pub fn new() -> Self {
        Self::with_port(DEFAULT_PORT)
    }

    pub fn with_port(port: u16) -> Self {
        Self { port, snapshot }
    }
}

impl Default for ProcessRuntimeGuard {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Stopped,
    Running(u32),
    /// A python process whose location is unreadable (for example an elevated one).
    Unreadable,
    /// The process list does not even contain this app, so it cannot be trusted.
    SnapshotFailed,
}

enum MainArg {
    Absent,
    This,
    Unresolvable,
}

/// Some argument is this install's main.py: absolute, or relative to the process's cwd.
fn main_arg(process: &ProcessInfo, main: &Path) -> MainArg {
    let mut unresolvable = false;
    for part in &process.cmd {
        let part = Path::new(part);
        let is_main_py = part.file_name().is_some_and(|name| {
            if cfg!(windows) {
                name.eq_ignore_ascii_case("main.py")
            } else {
                name == "main.py"
            }
        });
        if !is_main_py {
            continue;
        }
        let resolved = if part.is_absolute() {
            part.to_path_buf()
        } else if let Some(cwd) = process.cwd.as_deref() {
            cwd.join(part)
        } else {
            unresolvable = true;
            continue;
        };
        if normalized_path(&resolved) == main {
            return MainArg::This;
        }
    }
    if unresolvable {
        MainArg::Unresolvable
    } else {
        MainArg::Absent
    }
}

/// Classifies a process snapshot against an install root. Only a process whose command line runs
/// this installation's `ComfyUI/main.py` is `Running`; programs that merely sit in or use its
/// folders are not. A python whose location cannot be read at all makes the state `Unreadable`,
/// and so, on Windows only, does a relative `main.py` whose working directory cannot be read: a
/// readable command line with an unreadable cwd is an elevated process of the same user
/// (`ProcessCommandLineInformation`), possibly this installation's ComfyUI run as administrator.
/// On Linux `/proc/<pid>/cwd` needs ptrace access while `/proc/<pid>/cmdline` is world-readable,
/// so another user's (or a root Docker container's) `python main.py` would otherwise block every
/// operation; there the unresolvable argument is ignored. Pure, so every rule is testable.
pub fn assess(processes: &[ProcessInfo], root: &Path, own_pid: u32) -> Verdict {
    if !processes.iter().any(|process| process.pid == own_pid) {
        return Verdict::SnapshotFailed;
    }
    let main = normalized_path(&Layout::new(root).comfyui_dir().join("main.py"));
    let mut unreadable = false;
    for process in processes.iter().filter(|process| process.pid != own_pid) {
        match main_arg(process, &main) {
            MainArg::This => return Verdict::Running(process.pid),
            MainArg::Unresolvable => unreadable |= cfg!(windows),
            MainArg::Absent => {}
        }
        let python_like = process.name.to_lowercase().contains("python");
        if python_like && process.cwd.is_none() && process.exe.is_none() && process.cmd.is_empty() {
            unreadable = true;
        }
    }
    if unreadable {
        Verdict::Unreadable
    } else {
        Verdict::Stopped
    }
}

/// True when something on loopback answers like ComfyUI. It cannot be proven to be this
/// installation's process, so it counts as running.
async fn comfyui_answers(port: u16) -> bool {
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_millis(1500))
        .redirect(reqwest::redirect::Policy::none())
        .build()
    else {
        return false;
    };
    let Ok(response) = client
        .get(format!("http://127.0.0.1:{port}/system_stats"))
        .send()
        .await
    else {
        return false;
    };
    let Ok(body) = response.text().await else {
        return false;
    };
    body.contains("comfyui_version")
        || (body.contains("\"system\"") && body.contains("\"devices\""))
}

#[async_trait]
impl RuntimeMaintenance for ProcessRuntimeGuard {
    async fn ensure_stopped(&self, install_root: &Path) -> Result<(), RuntimeBlock> {
        let root: PathBuf = install_root.to_owned();
        let own_pid = std::process::id();
        let snapshot = self.snapshot;
        let verdict = tokio::task::spawn_blocking(move || assess(&snapshot(), &root, own_pid))
            .await
            .unwrap_or(Verdict::SnapshotFailed);
        match verdict {
            Verdict::Running(_) => return Err(RuntimeBlock::Running),
            Verdict::SnapshotFailed => return Err(RuntimeBlock::CannotAssess),
            Verdict::Stopped | Verdict::Unreadable => {}
        }
        if comfyui_answers(self.port).await {
            return Err(RuntimeBlock::Running);
        }
        if verdict == Verdict::Unreadable && port_is_open(self.port).await {
            return Err(RuntimeBlock::CannotAssess);
        }
        Ok(())
    }
}

async fn port_is_open(port: u16) -> bool {
    tokio::time::timeout(
        Duration::from_millis(500),
        tokio::net::TcpStream::connect(("127.0.0.1", port)),
    )
    .await
    .is_ok_and(|connection| connection.is_ok())
}

/// The first process outside `own_pid`'s process tree whose executable is inside `env`. Pure.
pub fn environment_user(
    processes: &[ProcessInfo],
    env: &Path,
    own_pid: u32,
) -> Option<EnvironmentUser> {
    let env = normalized_path(env);
    let parents: HashMap<u32, u32> = processes
        .iter()
        .filter_map(|process| process.parent.map(|parent| (process.pid, parent)))
        .collect();
    processes
        .iter()
        .filter(|process| process.pid != own_pid)
        .find(|process| {
            process
                .exe
                .as_deref()
                .is_some_and(|exe| normalized_path(exe).starts_with(&env))
                && !descends_from(process.pid, own_pid, &parents)
        })
        .map(|process| EnvironmentUser {
            pid: process.pid,
            name: process.name.clone(),
        })
}

/// True when `ancestor` is on the parent chain of `pid`. Bounded, so a parent cycle ends.
fn descends_from(pid: u32, ancestor: u32, parents: &HashMap<u32, u32>) -> bool {
    let mut current = pid;
    for _ in 0..64 {
        match parents.get(&current) {
            Some(&parent) if parent != current => {
                if parent == ancestor {
                    return true;
                }
                current = parent;
            }
            _ => return false,
        }
    }
    false
}

/// Finds programs outside this app that run an executable from an environment's folder.
pub struct ProcessEnvironmentUsers {
    /// The process-list source; always the real one outside tests.
    snapshot: SnapshotFn,
}

impl ProcessEnvironmentUsers {
    pub fn new() -> Self {
        Self { snapshot }
    }
}

impl Default for ProcessEnvironmentUsers {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EnvironmentUsers for ProcessEnvironmentUsers {
    async fn first_user(&self, env: &Path) -> Option<EnvironmentUser> {
        let env = env.to_owned();
        let snapshot = self.snapshot;
        tokio::task::spawn_blocking(move || environment_user(&snapshot(), &env, std::process::id()))
            .await
            .unwrap_or(None)
    }
}

#[cfg(test)]
mod tests;
