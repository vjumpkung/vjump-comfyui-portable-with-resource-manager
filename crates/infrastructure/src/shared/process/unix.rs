//! Unix process-tree ownership with a process group.

use std::{io, time::Duration};

use tokio::process::{Child, Command};

pub(super) struct Tree {
    pgid: i32,
}

impl Tree {
    pub(super) fn contains_pid(&self, pid: u32) -> io::Result<bool> {
        let pid = i32::try_from(pid).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        // SAFETY: getpgid only reads membership of the supplied positive PID.
        let group = unsafe { libc::getpgid(pid) };
        if group < 0 {
            let error = io::Error::last_os_error();
            return if error.raw_os_error() == Some(libc::ESRCH) {
                Ok(false)
            } else {
                Err(error)
            };
        }
        Ok(group == self.pgid)
    }

    /// Signals the whole group: SIGTERM first, SIGKILL when `force` is set.
    pub(super) fn kill(&self, force: bool) {
        let signal = if force { libc::SIGKILL } else { libc::SIGTERM };
        // SAFETY: killpg takes plain integers; a missing group only sets errno.
        unsafe { libc::killpg(self.pgid, signal) };
    }

    fn group_is_empty(&self) -> bool {
        // SAFETY: signal 0 only checks that the group exists.
        let result = unsafe { libc::killpg(self.pgid, 0) };
        result == -1 && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
    }

    pub(super) async fn wait_empty(&self, timeout: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if self.group_is_empty() {
                return true;
            }
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

pub(super) fn spawn(command: &mut Command) -> io::Result<(Child, Tree)> {
    // The child leads a new process group; its pid is the group id.
    command.process_group(0);
    let child = command.spawn()?;
    let pgid = child
        .id()
        .and_then(|pid| i32::try_from(pid).ok())
        .ok_or_else(|| io::Error::other("the process id is unavailable"))?;
    Ok((child, Tree { pgid }))
}
