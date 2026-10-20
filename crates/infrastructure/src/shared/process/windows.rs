//! Windows process-tree ownership with a Job Object.
//!
//! The child is created suspended, assigned to a job with `KILL_ON_JOB_CLOSE`, and only then
//! resumed, so it cannot start a descendant before the job owns it.

use std::{ffi::c_void, io, mem::size_of, time::Duration};

use tokio::process::{Child, Command};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, WAIT_TIMEOUT},
    System::{
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectBasicAccountingInformation,
            JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
            TerminateJobObject,
        },
        LibraryLoader::SetDllDirectoryW,
        Threading::{
            CREATE_NO_WINDOW, CREATE_SUSPENDED, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_SYNCHRONIZE, WaitForSingleObject,
        },
    },
};

/// Remove cwd from the default DLL search for this process only;
/// Windows does not pass it to children (residual 6).
pub(super) fn initialize_dll_policy() -> io::Result<()> {
    static POLICY: std::sync::OnceLock<Result<(), i32>> = std::sync::OnceLock::new();
    match POLICY.get_or_init(|| {
        // SAFETY: a terminated empty UTF-16 string removes cwd; it is valid for the call.
        if unsafe { SetDllDirectoryW([0u16].as_ptr()) } == 0 {
            Err(io::Error::last_os_error().raw_os_error().unwrap_or(1))
        } else {
            Ok(())
        }
    }) {
        Ok(()) => Ok(()),
        Err(code) => Err(io::Error::from_raw_os_error(*code)),
    }
}

// `windows-sys` does not expose `NtResumeProcess`. It resumes every thread of a process that
// was started with CREATE_SUSPENDED; `std` and `tokio` close the primary thread handle, so
// `ResumeThread` is not reachable.
#[link(name = "ntdll", kind = "raw-dylib")]
unsafe extern "system" {
    fn NtResumeProcess(process: HANDLE) -> i32;
}

pub(super) struct Tree {
    job: HANDLE,
}

// A job handle is a plain kernel handle that any thread may use.
unsafe impl Send for Tree {}
unsafe impl Sync for Tree {}

impl Tree {
    pub(super) fn contains_pid(&self, pid: u32) -> io::Result<bool> {
        // SAFETY: Opens a read-only query handle for this PID, not a termination handle.
        let process = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                pid,
            )
        };
        if process.is_null() {
            let error = io::Error::last_os_error();
            return if error.raw_os_error() == Some(87) {
                Ok(false)
            } else {
                Err(error)
            };
        }
        let mut member = 0;
        // SAFETY: zero timeout only queries whether the process has terminated.
        let alive = unsafe { WaitForSingleObject(process, 0) } == WAIT_TIMEOUT;
        // SAFETY: both handles remain valid and the output pointer is initialized.
        let success = unsafe { IsProcessInJob(process, self.job, &mut member) };
        let error = (success == 0).then(io::Error::last_os_error);
        // SAFETY: this function owns the query handle and closes it once.
        unsafe { CloseHandle(process) };
        match error {
            Some(error) => Err(error),
            None => Ok(alive && member != 0),
        }
    }

    fn new() -> io::Result<Self> {
        // SAFETY: null attributes and name create an anonymous job with default security.
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the structure is plain data and zero is a valid initial state.
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: `info` outlives the call and the length matches its type.
        let configured = unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&raw const info).cast::<c_void>(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if configured == 0 {
            let error = io::Error::last_os_error();
            // SAFETY: `job` is a valid handle owned here.
            unsafe { CloseHandle(job) };
            return Err(error);
        }
        Ok(Self { job })
    }

    /// Terminates every process in the job. The force flag only matters on Unix.
    pub(super) fn kill(&self, _force: bool) {
        // SAFETY: `self.job` is a valid job handle for the lifetime of `self`.
        unsafe { TerminateJobObject(self.job, 1) };
    }

    fn active_processes(&self) -> Option<u32> {
        // SAFETY: plain data structure, zero is valid.
        let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: `info` outlives the call and the length matches its type.
        let ok = unsafe {
            QueryInformationJobObject(
                self.job,
                JobObjectBasicAccountingInformation,
                (&raw mut info).cast::<c_void>(),
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                std::ptr::null_mut(),
            )
        };
        (ok != 0).then_some(info.ActiveProcesses)
    }

    /// True once no process of the job is left.
    pub(super) async fn wait_empty(&self, timeout: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            match self.active_processes() {
                Some(0) => return true,
                None => return false,
                Some(_) => {}
            }
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        // SAFETY: the handle is valid and closed exactly once. KILL_ON_JOB_CLOSE ends any
        // process still in the job.
        unsafe { CloseHandle(self.job) };
    }
}

pub(super) fn spawn(command: &mut Command) -> io::Result<(Child, Tree)> {
    initialize_dll_policy()?;
    let tree = Tree::new()?;
    command.creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW);
    let mut child = command.spawn()?;
    let handle = child
        .raw_handle()
        .ok_or_else(|| io::Error::other("the process handle is unavailable"))?
        as HANDLE;
    let attached = (|| {
        // SAFETY: both handles are valid; the process is still suspended.
        if unsafe { AssignProcessToJobObject(tree.job, handle) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the handle has full access rights because std created the process.
        if unsafe { NtResumeProcess(handle) } < 0 {
            return Err(io::Error::other("the process could not be resumed"));
        }
        Ok(())
    })();
    if let Err(error) = attached {
        let _ = child.start_kill();
        return Err(error);
    }
    Ok((child, tree))
}
