//! OS identity and listening socket ownership checks.
use crate::{
    environment::runtime_guard::{Verdict, assess},
    shared::procscan::SnapshotFn,
};
use application::comfy_runtime::{InventoryState, RuntimeError, RuntimeSession, RuntimeStore};
use domain::comfy_runtime::Port;
use std::path::Path;

/// No PID discovered here is termination authority. Only a live runner-owned scope can kill.
/// `snapshot` is the process-list source: the real one in production, a fixed one in tests.
pub async fn inspect(
    root: &Path,
    store: &dyn RuntimeStore,
    snapshot: SnapshotFn,
) -> Result<InventoryState, RuntimeError> {
    let root = root.to_path_buf();
    let verdict =
        tokio::task::spawn_blocking(move || assess(&snapshot(), &root, std::process::id()))
            .await
            .map_err(|_| RuntimeError::Failed("process inventory failed".into()))?;
    let sessions = store.unfinished_sessions().await?;
    match verdict {
        Verdict::Stopped => {
            for session in sessions {
                if recorded_process_exists(&session)? {
                    return Ok(InventoryState::Unknown("a recorded runtime process still exists; recover ownership before maintenance".into()));
                }
                store.save_session(&session, true).await?;
            }
            Ok(InventoryState::Stopped)
        }
        Verdict::Running(_) => Ok(InventoryState::Unknown(
            "a process is using this installation; its termination scope is not owned by this core"
                .into(),
        )),
        Verdict::Unreadable | Verdict::SnapshotFailed => Ok(InventoryState::Unknown(
            "process ownership could not be assessed; close the surviving process and retry".into(),
        )),
    }
}
fn recorded_process_exists(session: &RuntimeSession) -> Result<bool, RuntimeError> {
    let Some(identity) = &session.identity else {
        return Ok(false);
    };
    Ok(creation_identity(identity.pid)?.is_some_and(|created| created == identity.created))
}

#[cfg(windows)]
pub fn creation_identity(pid: u32) -> Result<Option<String>, RuntimeError> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, FILETIME, GetLastError},
        System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    };
    // SAFETY: OpenProcess takes a PID and returns an owned handle or null.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        // SAFETY: reads this thread's last error, immediately after OpenProcess.
        if unsafe { GetLastError() } == ERROR_INVALID_PARAMETER {
            return Ok(None);
        }
        return Err(RuntimeError::Failed(
            "the recorded process identity is inaccessible".into(),
        ));
    }
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: all output pointers and the handle remain valid for the call.
    let ok = unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) };
    // SAFETY: this function owns the handle and closes it once.
    unsafe { CloseHandle(handle) };
    if ok == 0 {
        return Err(RuntimeError::Failed(
            "process creation identity could not be read".into(),
        ));
    }
    Ok(Some(format!(
        "{}",
        (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime)
    )))
}
#[cfg(unix)]
pub fn creation_identity(pid: u32) -> Result<Option<String>, RuntimeError> {
    let stat = match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => stat,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => {
            return Err(RuntimeError::Failed(
                "process identity is inaccessible".into(),
            ));
        }
    };
    let (_, rest) = stat.rsplit_once(')').ok_or(RuntimeError::InvalidRecord)?;
    let start = rest
        .split_whitespace()
        .nth(19)
        .ok_or(RuntimeError::InvalidRecord)?;
    let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .map_err(|_| RuntimeError::InvalidRecord)?;
    Ok(Some(format!("{}:{start}", boot.trim())))
}

#[cfg(windows)]
pub fn listener_pids(port: Port) -> Result<Vec<u32>, RuntimeError> {
    use windows_sys::Win32::{
        Foundation::{ERROR_INSUFFICIENT_BUFFER, NO_ERROR},
        NetworkManagement::IpHelper::{
            GetExtendedTcpTable, MIB_TCPROW_OWNER_PID, MIB_TCPTABLE_OWNER_PID,
            TCP_TABLE_OWNER_PID_LISTENER,
        },
        Networking::WinSock::AF_INET,
    };
    let mut size = 0u32;
    // SAFETY: null probes the required buffer size.
    let first = unsafe {
        GetExtendedTcpTable(
            std::ptr::null_mut(),
            &mut size,
            0,
            u32::from(AF_INET),
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        )
    };
    if first != ERROR_INSUFFICIENT_BUFFER && first != NO_ERROR {
        return Err(RuntimeError::Failed(
            "listening socket ownership is unavailable".into(),
        ));
    }
    for _ in 0..4 {
        if size as usize > 16 << 20 {
            return Err(RuntimeError::InvalidRecord);
        }
        // u64 allocation guarantees sufficient alignment for the Windows table and rows.
        let mut buffer = vec![0u64; (size as usize).div_ceil(8).max(1)];
        let capacity = buffer.len() * 8;
        size = capacity as u32;
        // SAFETY: the allocated buffer is aligned and has the supplied byte capacity.
        let code = unsafe {
            GetExtendedTcpTable(
                buffer.as_mut_ptr().cast(),
                &mut size,
                0,
                u32::from(AF_INET),
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        if code == ERROR_INSUFFICIENT_BUFFER {
            continue;
        }
        if code != NO_ERROR {
            return Err(RuntimeError::Failed(
                "listening socket table could not be read".into(),
            ));
        }
        let offset = std::mem::offset_of!(MIB_TCPTABLE_OWNER_PID, table);
        if (size as usize) < offset {
            return Err(RuntimeError::InvalidRecord);
        }
        // SAFETY: successful API call initialized the count at the aligned buffer start.
        let count = unsafe { *(buffer.as_ptr().cast::<u32>()) } as usize;
        if offset + count * std::mem::size_of::<MIB_TCPROW_OWNER_PID>() > size as usize {
            return Err(RuntimeError::InvalidRecord);
        }
        // SAFETY: row count and total byte length have just been checked against the buffer.
        let rows = unsafe {
            std::slice::from_raw_parts(
                buffer
                    .as_ptr()
                    .cast::<u8>()
                    .add(offset)
                    .cast::<MIB_TCPROW_OWNER_PID>(),
                count,
            )
        };
        return Ok(rows
            .iter()
            .filter(|r| {
                u16::from_be(r.dwLocalPort as u16) == port.get()
                    && matches!(r.dwLocalAddr.to_ne_bytes(), [0, 0, 0, 0] | [127, 0, 0, 1])
            })
            .map(|r| r.dwOwningPid)
            .collect());
    }
    Err(RuntimeError::Failed(
        "the listening socket table kept changing".into(),
    ))
}

#[cfg(unix)]
pub fn listener_pids(port: Port) -> Result<Vec<u32>, RuntimeError> {
    use std::collections::HashSet;
    let text = std::fs::read_to_string("/proc/net/tcp")
        .map_err(|_| RuntimeError::Failed("listening socket inventory is inaccessible".into()))?;
    let mut inodes = HashSet::new();
    for line in text.lines().skip(1) {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() < 10 || fields[3] != "0A" {
            continue;
        }
        let Some((address, number)) = fields[1].split_once(':') else {
            continue;
        };
        if matches!(address, "0100007F" | "00000000")
            && u16::from_str_radix(number, 16).ok() == Some(port.get())
        {
            inodes.insert(format!("socket:[{}]", fields[9]));
        }
    }
    if inodes.is_empty() {
        return Ok(Vec::new());
    }
    let mut pids = Vec::new();
    let mut found = HashSet::new();
    for entry in std::fs::read_dir("/proc")
        .map_err(|_| RuntimeError::InvalidRecord)?
        .flatten()
    {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let Ok(fds) = std::fs::read_dir(entry.path().join("fd")) else {
            continue;
        };
        for fd in fds.flatten() {
            if let Ok(target) = std::fs::read_link(fd.path()) {
                let target = target.to_string_lossy().into_owned();
                if inodes.contains(&target) {
                    found.insert(target);
                    pids.push(pid);
                    break;
                }
            }
        }
    }
    if found.len() != inodes.len() {
        return Err(RuntimeError::Failed(
            "a listener exists but its process owner is inaccessible".into(),
        ));
    }
    pids.sort_unstable();
    pids.dedup();
    Ok(pids)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identifies_the_current_process_and_own_listener() {
        assert!(creation_identity(std::process::id()).unwrap().is_some());
        let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let pids = listener_pids(Port::new(socket.local_addr().unwrap().port()).unwrap()).unwrap();
        assert!(pids.contains(&std::process::id()), "{pids:?}");
    }
}
