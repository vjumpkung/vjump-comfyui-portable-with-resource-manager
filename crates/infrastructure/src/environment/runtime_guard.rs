//! The M1 runtime-maintenance adapter: a real process-state preflight. Maintenance is refused
//! while a process of this installation (ComfyUI or anything running from its folders) is
//! alive, or while a ComfyUI answers on the configured port and cannot be shown to belong to
//! another installation. M2 replaces the refusal with an owned-runtime stop.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use application::comfy_runtime::{RuntimeBlock, RuntimeMaintenance};
use async_trait::async_trait;

use super::layout::Layout;
use crate::shared::procscan::{ProcessInfo, snapshot};

/// The default ComfyUI port.
pub const DEFAULT_PORT: u16 = 8188;

pub struct ProcessRuntimeGuard {
    port: u16,
}

impl ProcessRuntimeGuard {
    pub fn new() -> Self {
        Self::with_port(DEFAULT_PORT)
    }

    pub fn with_port(port: u16) -> Self {
        Self { port }
    }
}

impl Default for ProcessRuntimeGuard {
    fn default() -> Self {
        Self::new()
    }
}

fn comparable(path: &Path) -> Vec<String> {
    path.components()
        .map(|component| {
            let text = component.as_os_str().to_string_lossy().into_owned();
            if cfg!(windows) {
                text.to_lowercase()
            } else {
                text
            }
        })
        .collect()
}

fn is_under(path: &Path, directory: &[String]) -> bool {
    let path = comparable(path);
    path.len() >= directory.len() && path[..directory.len()] == directory[..]
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

/// Classifies a process snapshot against an install root. Pure, so every rule is testable.
pub fn assess(processes: &[ProcessInfo], root: &Path, own_pid: u32) -> Verdict {
    if !processes.iter().any(|process| process.pid == own_pid) {
        return Verdict::SnapshotFailed;
    }
    let layout = Layout::new(root);
    let comfyui = comparable(&layout.comfyui_dir());
    let envs = comparable(&layout.envs_dir());
    let mut unreadable = false;
    for process in processes.iter().filter(|process| process.pid != own_pid) {
        let in_folders = process
            .cwd
            .as_deref()
            .is_some_and(|cwd| is_under(cwd, &comfyui))
            || process
                .exe
                .as_deref()
                .is_some_and(|exe| is_under(exe, &envs))
            || process
                .cmd
                .iter()
                .any(|part| is_under(Path::new(part), &comfyui));
        if in_folders {
            return Verdict::Running(process.pid);
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

#[cfg(test)]
mod tests;
