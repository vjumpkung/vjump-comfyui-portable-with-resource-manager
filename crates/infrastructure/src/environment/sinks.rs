//! Setup event sinks: redaction before anything leaves the backend, and the per-run
//! `data/logs/setup-<run-id>.log` file.

use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use application::environment::{ConsentRequest, SetupEvent, SetupProgressSink, StepFailure};

use crate::shared::{clock::rfc3339_utc, logging::redact};

fn clean(text: &str) -> String {
    redact(text).into_owned()
}

fn clean_failure(failure: &StepFailure) -> StepFailure {
    StepFailure {
        step: failure.step,
        kind: failure.kind,
        message: clean(&failure.message),
        command: failure.command.as_deref().map(clean),
        exit_code: failure.exit_code,
        tail: failure.tail.iter().map(|line| clean(line)).collect(),
        consent: failure.consent.as_ref().map(|consent| ConsentRequest {
            source: clean(&consent.source),
            commit: consent.commit.clone(),
        }),
    }
}

/// Redacts every free-text field of an event.
pub fn redact_event(event: SetupEvent) -> SetupEvent {
    match event {
        SetupEvent::Log { run_id, step, line } => SetupEvent::Log {
            run_id,
            step,
            line: clean(&line),
        },
        SetupEvent::Step {
            run_id,
            step,
            status,
            attempt,
            message,
        } => SetupEvent::Step {
            run_id,
            step,
            status,
            attempt,
            message: message.as_deref().map(clean),
        },
        SetupEvent::Failure { run_id, failure } => SetupEvent::Failure {
            run_id,
            failure: Box::new(clean_failure(&failure)),
        },
        other => other,
    }
}

/// Redacts events before they reach the wrapped sink (for example the UI channel).
pub struct RedactingSink {
    inner: Arc<dyn SetupProgressSink>,
}

impl RedactingSink {
    pub fn new(inner: Arc<dyn SetupProgressSink>) -> Self {
        Self { inner }
    }
}

impl SetupProgressSink for RedactingSink {
    fn emit(&self, event: SetupEvent) {
        self.inner.emit(redact_event(event));
    }
}

/// Appends events to `setup-<run-id>.log` and forwards them. A failing log file never stops setup.
pub struct FileLogSink {
    path: PathBuf,
    file: Mutex<Option<File>>,
    inner: Arc<dyn SetupProgressSink>,
}

impl FileLogSink {
    pub fn new(logs_dir: &Path, run_id: &str, inner: Arc<dyn SetupProgressSink>) -> Self {
        let path = logs_dir.join(format!("setup-{run_id}.log"));
        let file = fs::create_dir_all(logs_dir).ok().and_then(|()| {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .ok()
        });
        Self {
            path,
            file: Mutex::new(file),
            inner,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn write_lines(&self, lines: &[String]) {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs());
        let stamp = rfc3339_utc(seconds);
        let mut guard = self.file.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(file) = guard.as_mut() {
            for line in lines {
                let _ = writeln!(file, "{stamp} {}", clean(line));
            }
            let _ = file.flush();
        }
    }
}

impl SetupProgressSink for FileLogSink {
    fn emit(&self, event: SetupEvent) {
        match &event {
            SetupEvent::Log { step, line, .. } => {
                self.write_lines(&[format!("[{}] {line}", step.as_str())]);
            }
            SetupEvent::Step {
                step,
                status,
                attempt,
                message,
                ..
            } => {
                let mut line = format!(
                    "== {} {} (attempt {attempt})",
                    step.as_str(),
                    status.as_str()
                );
                if let Some(message) = message {
                    line.push_str(": ");
                    line.push_str(message);
                }
                self.write_lines(&[line]);
            }
            SetupEvent::RunStatus { status, .. } => {
                self.write_lines(&[format!("== run {}", status.as_str())]);
            }
            SetupEvent::Failure { failure, .. } => {
                let mut lines = vec![format!(
                    "!! {} failed ({}): {}",
                    failure.step.as_str(),
                    failure.kind.as_str(),
                    failure.message
                )];
                if let Some(command) = &failure.command {
                    lines.push(format!("!! command: {command}"));
                }
                if let Some(code) = failure.exit_code {
                    lines.push(format!("!! exit code: {code}"));
                }
                lines.extend(failure.tail.iter().map(|line| format!("!! | {line}")));
                self.write_lines(&lines);
            }
            SetupEvent::Progress { .. } => {}
        }
        self.inner.emit(event);
    }
}

/// The sink of one run: events are redacted first, then logged to the run's file, then handed
/// to `inner` (the UI channel). Returns the sink and the log file's path.
pub fn run_sink(
    logs_dir: &Path,
    run_id: &str,
    inner: Arc<dyn SetupProgressSink>,
) -> (Arc<dyn SetupProgressSink>, PathBuf) {
    let file_sink = FileLogSink::new(logs_dir, run_id, inner);
    let path = file_sink.path().to_owned();
    (Arc::new(RedactingSink::new(Arc::new(file_sink))), path)
}

#[cfg(test)]
mod tests;
