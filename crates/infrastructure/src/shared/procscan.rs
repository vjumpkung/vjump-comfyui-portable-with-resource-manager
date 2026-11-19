use std::path::PathBuf;

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub exe: Option<PathBuf>,
    pub cwd: Option<PathBuf>,
    pub cmd: Vec<String>,
    pub parent: Option<u32>,
}

/// A blocking snapshot of the running processes with their executable, working directory and
/// command line (where the OS lets us read them).
pub fn snapshot() -> Vec<ProcessInfo> {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_cmd(UpdateKind::Always)
            .with_cwd(UpdateKind::Always)
            .with_exe(UpdateKind::Always),
    );
    system
        .processes()
        .iter()
        .map(|(pid, process)| ProcessInfo {
            pid: pid.as_u32(),
            name: process.name().to_string_lossy().into_owned(),
            exe: process.exe().map(PathBuf::from),
            cwd: process.cwd().map(PathBuf::from),
            cmd: process
                .cmd()
                .iter()
                .map(|part| part.to_string_lossy().into_owned())
                .collect(),
            parent: process.parent().map(|pid| pid.as_u32()),
        })
        .collect()
}

/// A process-list source. Production code passes [`snapshot`]; tests whose assertions are not
/// about the rest of the machine inject [`test_snapshot`], so they cannot be failed by it.
pub type SnapshotFn = fn() -> Vec<ProcessInfo>;

/// The real snapshot minus processes whose location cannot be read and so cannot be assessed: a
/// python caught while starting or exiting (the parallel suite spawns many) briefly has no
/// readable cwd, exe or command line, and on Windows a process with a relative `main.py` argument
/// but no readable cwd cannot be placed either (other OSes ignore that case). Mirrors both
/// Unreadable rules of `assess`.
#[cfg(test)]
pub fn test_snapshot() -> Vec<ProcessInfo> {
    snapshot()
        .into_iter()
        .filter(|process| {
            let hidden_python = process.name.to_lowercase().contains("python")
                && process.cwd.is_none()
                && process.exe.is_none()
                && process.cmd.is_empty();
            let relative_main_without_cwd = cfg!(windows)
                && process.cwd.is_none()
                && process.cmd.iter().any(|part| {
                    let part = std::path::Path::new(part);
                    part.is_relative()
                        && part
                            .file_name()
                            .is_some_and(|name| name.eq_ignore_ascii_case("main.py"))
                });
            !(hidden_python || relative_main_without_cwd)
        })
        .collect()
}
