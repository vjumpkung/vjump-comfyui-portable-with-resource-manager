//! Subprocess runner for uv, git and Python.
//!
//! * the child gets exactly the environment it is given (`env_clear`, then an allow-list);
//! * the whole process tree is owned: a Windows Job Object (child started suspended, assigned to
//!   the job, then resumed, so no descendant can escape) or a Unix process group;
//! * output is read as raw bytes, split on `\n`, `\r\n` and lone `\r` (progress bars), redacted
//!   and bounded before it reaches the sink, the tail ring or the captured stdout;
//! * cancellation tears the tree down and returns only after the descendants are gone.

use std::{
    collections::VecDeque,
    ffi::OsString,
    future::Future,
    io,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

use application::{environment::Retained, shared::CancelToken};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::Command,
};

use super::logging::redact;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
use unix as platform;
#[cfg(windows)]
use windows as platform;

/// Establish process-wide tool launch policy before workers are started.
pub fn initialize_tool_policy() -> io::Result<()> {
    #[cfg(windows)]
    windows::initialize_dll_policy()?;
    Ok(())
}

pub(crate) fn default_working_directory() -> io::Result<PathBuf> {
    static DIRECTORY: OnceLock<Result<PathBuf, io::ErrorKind>> = OnceLock::new();
    match DIRECTORY.get_or_init(|| {
        std::env::current_exe()
            .map_err(|error| error.kind())?
            .parent()
            .map(Path::to_path_buf)
            .ok_or(io::ErrorKind::NotFound)
    }) {
        Ok(directory) => Ok(directory.clone()),
        Err(kind) => Err((*kind).into()),
    }
}

/// Read-only membership in the process scope owned by this runner. This never
/// grants termination authority to a PID read from persistent storage.
pub struct ProcessScope {
    pid: u32,
    tree: Arc<platform::Tree>,
}

impl ProcessScope {
    pub fn pid(&self) -> u32 {
        self.pid
    }
    pub fn is_alive(&self) -> io::Result<bool> {
        self.tree.contains_pid(self.pid)
    }
    pub fn contains_pid(&self, pid: u32) -> io::Result<bool> {
        self.tree.contains_pid(pid)
    }
}

/// Infrastructure-only observation of long-lived processes. Observers redact raw
/// output before persisting it or forwarding it beyond infrastructure.
pub trait ProcessObserver: Send + Sync {
    fn spawned(&self, scope: Arc<ProcessScope>);
    fn chunk(&self, stderr: bool, bytes: &[u8]);
}

struct TreeCleanup {
    tree: Arc<platform::Tree>,
    armed: bool,
}
impl Drop for TreeCleanup {
    fn drop(&mut self) {
        if self.armed {
            self.tree.kill(true);
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CommandSpec {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub cwd: Option<PathBuf>,
    /// The complete environment of the child. Nothing is inherited.
    pub env: Vec<(OsString, OsString)>,
    /// Bytes written to the child's standard input (which is otherwise empty).
    pub stdin: Option<Arc<[u8]>>,
    /// Protection the command's inputs depend on. It stays alive until no process of the command
    /// is left, even when the caller drops its own copy after a failed cleanup.
    pub retained: Retained,
}

impl CommandSpec {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            ..Self::default()
        }
    }

    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    pub fn env_vars(mut self, env: Vec<(OsString, OsString)>) -> Self {
        self.env = env;
        self
    }

    pub fn stdin(mut self, bytes: Arc<[u8]>) -> Self {
        self.stdin = Some(bytes);
        self
    }

    pub fn retain(mut self, retained: Retained) -> Self {
        self.retained = self.retained.merge(retained);
        self
    }

    /// The redacted command line shown in failures and logs.
    pub fn display(&self) -> String {
        let mut text = self.program.file_name().map_or_else(
            || "command".to_owned(),
            |name| quote(&name.to_string_lossy()),
        );
        for arg in &self.args {
            text.push(' ');
            text.push_str(&quote(&arg.to_string_lossy()));
        }
        redact(&text).into_owned()
    }
}

fn quote(value: &str) -> String {
    if value.is_empty() || value.contains(char::is_whitespace) {
        format!("\"{value}\"")
    } else {
        value.to_owned()
    }
}

#[derive(Debug, Clone)]
pub struct RunLimits {
    pub max_tail_lines: usize,
    pub max_line_bytes: usize,
    /// Text stdout is truncated beyond this many bytes; raw capture refuses overflow.
    pub max_stdout_bytes: usize,
    pub timeout: Option<Duration>,
    /// How long a Unix process group gets between SIGTERM and SIGKILL.
    pub kill_grace: Duration,
    /// How long to wait for the output pipes to close after the process ended.
    pub drain_grace: Duration,
}

impl Default for RunLimits {
    fn default() -> Self {
        Self {
            max_tail_lines: 40,
            max_line_bytes: 16 * 1024,
            max_stdout_bytes: 4 << 20,
            timeout: None,
            kill_grace: Duration::from_secs(3),
            drain_grace: Duration::from_secs(5),
        }
    }
}

/// Receives each redacted output line.
pub trait LineSink: Send + Sync {
    fn line(&self, line: &str);
}

pub struct DiscardSink;

impl LineSink for DiscardSink {
    fn line(&self, _line: &str) {}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub exit_code: i32,
    /// Redacted stdout, bounded by `max_stdout_bytes`.
    pub stdout: String,
    pub tail: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunError {
    Spawn(io::ErrorKind),
    Cancelled {
        tail: Vec<String>,
    },
    TimedOut {
        tail: Vec<String>,
    },
    InputFailed {
        tail: Vec<String>,
    },
    CleanupFailed {
        tail: Vec<String>,
    },
    Failed {
        exit_code: Option<i32>,
        tail: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RawOutputError {
    Run(RunError),
    LimitExceeded,
    Incomplete,
}

#[derive(Default)]
struct RawCapture {
    bytes: Vec<u8>,
    limit_exceeded: bool,
    complete: bool,
}

impl RawCapture {
    fn push(&mut self, bytes: &[u8], limit: usize) {
        if self.limit_exceeded {
            return;
        }
        if bytes.len() > limit.saturating_sub(self.bytes.len()) {
            self.limit_exceeded = true;
            return;
        }
        self.bytes.extend_from_slice(bytes);
    }

    fn finish(self) -> Result<Vec<u8>, RawOutputError> {
        if self.limit_exceeded {
            Err(RawOutputError::LimitExceeded)
        } else if !self.complete {
            Err(RawOutputError::Incomplete)
        } else {
            Ok(self.bytes)
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stream {
    Stdout,
    Stderr,
}

struct State {
    tail: VecDeque<String>,
    stdout: String,
    /// The last tail entry was a progress update and may be replaced by the next one.
    tail_is_progress: bool,
    last_progress: Option<Instant>,
}

/// Progress updates (lines ended by a lone carriage return) are forwarded at most this often.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

struct Collector<'a> {
    sink: &'a dyn LineSink,
    limits: &'a RunLimits,
    state: Mutex<State>,
}

impl Collector<'_> {
    fn push(&self, stream: Stream, raw: &[u8], progress: bool) {
        let text = String::from_utf8_lossy(raw);
        let line = redact(text.trim_end_matches(['\r', '\n'])).into_owned();
        if line.trim().is_empty() {
            return;
        }
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if progress {
            let now = Instant::now();
            let due = state
                .last_progress
                .is_none_or(|last| now.duration_since(last) >= PROGRESS_INTERVAL);
            // The tail keeps only the newest progress line, so diagnostics are not flooded.
            if state.tail_is_progress {
                state.tail.pop_back();
            }
            state.tail.push_back(line.clone());
            state.tail_is_progress = true;
            if !due {
                return;
            }
            state.last_progress = Some(now);
            drop(state);
            self.sink.line(&line);
            return;
        }
        if state.tail_is_progress {
            state.tail_is_progress = false;
        }
        if state.tail.len() >= self.limits.max_tail_lines {
            state.tail.pop_front();
        }
        state.tail.push_back(line.clone());
        if stream == Stream::Stdout && state.stdout.len() < self.limits.max_stdout_bytes {
            state.stdout.push_str(&line);
            state.stdout.push('\n');
        }
        drop(state);
        self.sink.line(&line);
    }

    fn tail(&self) -> Vec<String> {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .tail
            .iter()
            .cloned()
            .collect()
    }
}

/// Splits a byte stream into lines on `\n`, `\r\n` and lone `\r`, keeping partial UTF-8
/// sequences together and bounding the length of a single line. The second argument of the
/// callback is true for progress updates (a line ended by a lone carriage return).
pub(crate) struct LineAssembler {
    buffer: Vec<u8>,
    max_line: usize,
    /// A line ended by `\r` waits to learn whether `\n` follows (a normal line) or not
    /// (a progress update).
    pending_cr: Option<Vec<u8>>,
}

impl LineAssembler {
    pub fn new(max_line: usize) -> Self {
        Self {
            buffer: Vec::new(),
            max_line: max_line.max(16),
            pending_cr: None,
        }
    }

    pub fn push(&mut self, bytes: &[u8], mut emit: impl FnMut(&[u8], bool)) {
        for &byte in bytes {
            if let Some(line) = self.pending_cr.take() {
                if byte == b'\n' {
                    if !line.is_empty() {
                        emit(&line, false);
                    }
                    continue;
                }
                if !line.is_empty() {
                    emit(&line, true);
                }
            }
            match byte {
                b'\n' => self.flush(&mut emit),
                b'\r' => self.pending_cr = Some(std::mem::take(&mut self.buffer)),
                _ => {
                    self.buffer.push(byte);
                    if self.buffer.len() >= self.max_line {
                        self.split_long(&mut emit);
                    }
                }
            }
        }
    }

    pub fn finish(&mut self, mut emit: impl FnMut(&[u8], bool)) {
        if let Some(line) = self.pending_cr.take()
            && !line.is_empty()
        {
            emit(&line, false);
        }
        self.flush(&mut emit);
    }

    fn flush(&mut self, emit: &mut impl FnMut(&[u8], bool)) {
        if !self.buffer.is_empty() {
            emit(&self.buffer, false);
            self.buffer.clear();
        }
    }

    /// Emits a full buffer without cutting a UTF-8 sequence in half.
    fn split_long(&mut self, emit: &mut impl FnMut(&[u8], bool)) {
        let mut cut = self.buffer.len();
        while cut > 0 && (self.buffer[cut - 1] & 0xC0) == 0x80 {
            cut -= 1;
        }
        if cut > 0 && self.buffer[cut - 1] >= 0xC0 {
            cut -= 1;
        }
        if cut == 0 {
            cut = self.buffer.len();
        }
        emit(&self.buffer[..cut], false);
        self.buffer.drain(..cut);
    }
}

async fn read_stream<R: AsyncRead + Unpin>(
    mut reader: R,
    stream: Stream,
    collector: &Collector<'_>,
    observer: Option<&dyn ProcessObserver>,
) -> bool {
    let mut assembler = LineAssembler::new(collector.limits.max_line_bytes);
    let mut chunk = [0u8; 8192];
    let complete = loop {
        match reader.read(&mut chunk).await {
            Ok(0) => break true,
            Err(_) => break false,
            Ok(count) => {
                if let Some(observer) = observer {
                    observer.chunk(stream == Stream::Stderr, &chunk[..count]);
                }
                assembler.push(&chunk[..count], |line, progress| {
                    collector.push(stream, line, progress)
                });
            }
        }
    };
    assembler.finish(|line, progress| collector.push(stream, line, progress));
    complete
}

async fn read_raw_stdout<R: AsyncRead + Unpin>(
    mut reader: R,
    capture: &Mutex<RawCapture>,
    limit: usize,
) -> bool {
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk).await {
            Ok(0) => return true,
            Err(_) => return false,
            Ok(count) => capture
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push(&chunk[..count], limit),
        }
    }
}

async fn drain_output(pump: impl Future<Output = bool>, grace: Duration) -> bool {
    tokio::time::timeout(grace, pump).await.unwrap_or(false)
}

/// Keeps `retained` alive until `empty` reports that no process of the command is left.
fn retain_until_empty<F, Fut>(retained: Retained, mut empty: F)
where
    F: FnMut() -> Fut + Send + 'static,
    Fut: Future<Output = bool> + Send + 'static,
{
    if retained.is_empty() {
        return;
    }
    tokio::spawn(async move {
        while !empty().await {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        drop(retained);
    });
}

enum Ended {
    Exited(io::Result<std::process::ExitStatus>),
    Cancelled,
    TimedOut,
    InputFailed,
}

#[cfg(test)]
thread_local! {
    static CLEANUP_SURVIVORS: std::cell::RefCell<Option<Arc<std::sync::atomic::AtomicBool>>> = const { std::cell::RefCell::new(None) };
}

/// Runs `spec` to completion, streaming redacted lines to `sink`.
pub async fn run(
    spec: &CommandSpec,
    sink: &dyn LineSink,
    cancel: &CancelToken,
    limits: &RunLimits,
) -> Result<Output, RunError> {
    run_inner(spec, sink, cancel, limits, None, None).await
}

pub async fn run_observed(
    spec: &CommandSpec,
    sink: &dyn LineSink,
    cancel: &CancelToken,
    limits: &RunLimits,
    observer: &dyn ProcessObserver,
) -> Result<Output, RunError> {
    run_inner(spec, sink, cancel, limits, Some(observer), None).await
}

/// Captures exact stdout bytes without forwarding them to diagnostics or observers.
pub(crate) async fn run_raw_stdout(
    spec: &CommandSpec,
    cancel: &CancelToken,
    limits: &RunLimits,
) -> Result<Vec<u8>, RawOutputError> {
    let capture = Mutex::new(RawCapture::default());
    run_inner(spec, &DiscardSink, cancel, limits, None, Some(&capture))
        .await
        .map_err(RawOutputError::Run)?;
    capture
        .into_inner()
        .unwrap_or_else(|p| p.into_inner())
        .finish()
}

async fn run_inner(
    spec: &CommandSpec,
    sink: &dyn LineSink,
    cancel: &CancelToken,
    limits: &RunLimits,
    observer: Option<&dyn ProcessObserver>,
    raw_stdout: Option<&Mutex<RawCapture>>,
) -> Result<Output, RunError> {
    #[cfg(test)]
    let cleanup_survivors = CLEANUP_SURVIVORS.with(|slot| slot.borrow_mut().take());
    let mut command = Command::new(&spec.program);
    command
        .args(&spec.args)
        .env_clear()
        .envs(spec.env.iter().map(|(key, value)| (key, value)))
        .stdin(if spec.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // A supplementary safeguard only: the tree guard below does the real cleanup.
        .kill_on_drop(true);
    let cwd = match &spec.cwd {
        Some(cwd) => cwd.clone(),
        None => default_working_directory().map_err(|error| RunError::Spawn(error.kind()))?,
    };
    command.current_dir(cwd);
    let (mut child, tree) =
        platform::spawn(&mut command).map_err(|error| RunError::Spawn(error.kind()))?;
    let tree = Arc::new(tree);
    let mut cleanup = TreeCleanup {
        tree: tree.clone(),
        armed: true,
    };
    if let Some(observer) = observer {
        observer.spawned(Arc::new(ProcessScope {
            pid: child.id().expect("newly spawned child has a process id"),
            tree: tree.clone(),
        }));
    }
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdin = child.stdin.take();

    let collector = Collector {
        sink,
        limits,
        state: Mutex::new(State {
            tail: VecDeque::new(),
            stdout: String::new(),
            tail_is_progress: false,
            last_progress: None,
        }),
    };
    let pump = async {
        let (stdout_complete, stderr_complete) = tokio::join!(
            async {
                if let Some(stdout) = stdout {
                    if let Some(capture) = raw_stdout {
                        read_raw_stdout(stdout, capture, limits.max_stdout_bytes).await
                    } else {
                        read_stream(stdout, Stream::Stdout, &collector, observer).await
                    }
                } else {
                    false
                }
            },
            async {
                if let Some(stderr) = stderr {
                    read_stream(stderr, Stream::Stderr, &collector, observer).await
                } else {
                    false
                }
            }
        );
        stdout_complete && stderr_complete
    };
    tokio::pin!(pump);

    let started = Instant::now();
    let mut ticker = tokio::time::interval(Duration::from_millis(40));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut pump_done = false;
    let mut pump_complete = false;
    let ended = {
        let input = async move {
            if let Some(bytes) = &spec.stdin {
                let mut stdin = stdin.ok_or(io::ErrorKind::BrokenPipe)?;
                stdin.write_all(bytes).await?;
                stdin.shutdown().await?;
            }
            Ok::<_, io::Error>(())
        };
        tokio::pin!(input);
        let mut input_done = false;
        loop {
            if cancel.is_cancelled() {
                break Ended::Cancelled;
            }
            if limits
                .timeout
                .is_some_and(|timeout| started.elapsed() >= timeout)
            {
                break Ended::TimedOut;
            }
            tokio::select! {
                biased;
                result = &mut input, if !input_done => {
                    input_done = true;
                    if result.is_err() {
                        break Ended::InputFailed;
                    }
                }
                status = child.wait() => {
                    break if input_done { Ended::Exited(status) } else { Ended::InputFailed };
                }
                _ = ticker.tick() => {
                    if cancel.is_cancelled() {
                        break Ended::Cancelled;
                    }
                    if limits.timeout.is_some_and(|timeout| started.elapsed() >= timeout) {
                        break Ended::TimedOut;
                    }
                }
                complete = &mut pump, if !pump_done => {
                    pump_done = true;
                    pump_complete = complete;
                },
            }
        }
    };

    match &ended {
        Ended::Exited(_) => {
            // Stragglers that outlive the main process must not keep the pipes open.
            tree.kill(true);
        }
        Ended::Cancelled | Ended::TimedOut | Ended::InputFailed => {
            tree.kill(false);
            if tokio::time::timeout(limits.kill_grace, child.wait())
                .await
                .is_err()
            {
                tree.kill(true);
                let _ = child.wait().await;
            }
            tree.kill(true);
        }
    }
    if !pump_done {
        pump_complete = drain_output(&mut pump, limits.drain_grace).await;
    }
    if let Some(capture) = raw_stdout {
        capture.lock().unwrap_or_else(|p| p.into_inner()).complete = pump_complete;
    }
    // Ownership of the tree ends only when no descendant is left.
    let mut cleanup_verified = tree.wait_empty(limits.kill_grace).await;
    if !cleanup_verified {
        tree.kill(true);
        cleanup_verified = tree.wait_empty(limits.kill_grace).await;
    }

    let tail = collector.tail();
    #[cfg(test)]
    let cleanup_verified = cleanup_verified
        && cleanup_survivors
            .as_ref()
            .is_none_or(|alive| !alive.load(std::sync::atomic::Ordering::SeqCst));
    if !cleanup_verified {
        // Survivors may still use the command's inputs: hand the protection to a task that
        // releases it only once the tree is empty.
        let (survivors, grace) = (tree.clone(), limits.kill_grace);
        retain_until_empty(spec.retained.clone(), move || {
            let tree = survivors.clone();
            #[cfg(test)]
            let injected = cleanup_survivors.clone();
            async move {
                let empty = tree.wait_empty(grace).await;
                #[cfg(test)]
                let empty = empty
                    && injected
                        .as_ref()
                        .is_none_or(|alive| !alive.load(std::sync::atomic::Ordering::SeqCst));
                empty
            }
        });
        return Err(RunError::CleanupFailed { tail });
    }
    cleanup.armed = false;
    match ended {
        Ended::Cancelled => Err(RunError::Cancelled { tail }),
        Ended::TimedOut => Err(RunError::TimedOut { tail }),
        Ended::InputFailed => Err(RunError::InputFailed { tail }),
        Ended::Exited(Err(_)) => Err(RunError::Failed {
            exit_code: None,
            tail,
        }),
        Ended::Exited(Ok(status)) => {
            let exit_code = status.code();
            if status.success() {
                let stdout = std::mem::take(
                    &mut collector
                        .state
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .stdout,
                );
                Ok(Output {
                    exit_code: 0,
                    stdout,
                    tail,
                })
            } else {
                Err(RunError::Failed { exit_code, tail })
            }
        }
    }
}

/// The variables Windows components need to start and resolve their own paths. `env_clear`
/// removes them; without `SystemDrive` and friends, `%SystemDrive%\ProgramData\...` style
/// registry paths are not expanded and Windows writes caches into a literal `%SystemDrive%`
/// folder under the working directory. Only variables the parent actually has are copied, and
/// nothing user-specific (profile, tokens, proxies) is included here.
#[cfg(windows)]
pub fn system_baseline() -> Vec<(OsString, OsString)> {
    const NAMES: [&str; 17] = [
        "SystemRoot",
        "SystemDrive",
        "windir",
        "ComSpec",
        "PATHEXT",
        "ProgramData",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramW6432",
        "CommonProgramFiles",
        "CommonProgramFiles(x86)",
        "CommonProgramW6432",
        "ALLUSERSPROFILE",
        "PUBLIC",
        "PROCESSOR_ARCHITECTURE",
        "NUMBER_OF_PROCESSORS",
        "OS",
    ];
    let mut variables: Vec<(OsString, OsString)> = NAMES
        .iter()
        .filter_map(|name| std::env::var_os(name).map(|value| (OsString::from(*name), value)))
        .collect();
    let has = |variables: &[(OsString, OsString)], name: &str| {
        variables
            .iter()
            .any(|(key, _)| key.to_string_lossy().eq_ignore_ascii_case(name))
    };
    if !has(&variables, "SystemDrive")
        && let Some((_, root)) = variables
            .iter()
            .find(|(key, _)| key.to_string_lossy().eq_ignore_ascii_case("SystemRoot"))
    {
        let drive: String = root.to_string_lossy().chars().take(2).collect();
        if drive.ends_with(':') {
            variables.push(("SystemDrive".into(), drive.into()));
        }
    }
    variables
}

#[cfg(not(windows))]
pub fn system_baseline() -> Vec<(OsString, OsString)> {
    Vec::new()
}

#[cfg(test)]
mod tests;
