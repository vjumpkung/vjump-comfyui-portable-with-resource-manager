use std::path::PathBuf;

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub exe: Option<PathBuf>,
    pub cwd: Option<PathBuf>,
    pub cmd: Vec<String>,
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
        })
        .collect()
}
