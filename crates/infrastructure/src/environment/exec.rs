//! Glue between the process runner and the application's step context.

use application::{
    environment::{CommandFailure, ProvisionError, StepContext},
    shared::CancelToken,
};

use crate::shared::process::{self, CommandSpec, LineSink, Output, RunError, RunLimits};

/// Forwards redacted lines to the step's progress sink.
pub struct StepSink<'a>(pub &'a StepContext);

impl LineSink for StepSink<'_> {
    fn line(&self, line: &str) {
        self.0.log(line);
    }
}

fn program_name(spec: &CommandSpec) -> String {
    spec.program.file_name().map_or_else(
        || "the tool".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// Runs a command for a setup step. Failures carry the redacted command line, the exit code and
/// the last output lines. Cancellation returns only after the process tree is gone.
pub async fn run_step(
    spec: &CommandSpec,
    ctx: &StepContext,
    limits: &RunLimits,
) -> Result<Output, ProvisionError> {
    let sink = StepSink(ctx);
    map(spec, process::run(spec, &sink, &ctx.cancel, limits).await)
}

/// Runs a command without a step (probes), still honouring a cancellation token.
pub async fn run_quiet(
    spec: &CommandSpec,
    cancel: &CancelToken,
    limits: &RunLimits,
) -> Result<Output, ProvisionError> {
    map(
        spec,
        process::run(spec, &process::DiscardSink, cancel, limits).await,
    )
}

fn map(spec: &CommandSpec, result: Result<Output, RunError>) -> Result<Output, ProvisionError> {
    match result {
        Ok(output) => Ok(output),
        Err(RunError::Cancelled { .. }) => Err(ProvisionError::Cancelled),
        Err(RunError::Spawn(_)) => Err(ProvisionError::Failed(format!(
            "{} could not be started.",
            program_name(spec)
        ))),
        Err(RunError::CleanupFailed { .. }) => Err(ProvisionError::Failed(
            "The process tree could not be confirmed stopped. Close the surviving process before retrying.".into(),
        )),
        Err(RunError::TimedOut { tail }) => Err(ProvisionError::CommandFailed(CommandFailure {
            command: spec.display(),
            exit_code: None,
            tail,
        })),
        Err(RunError::Failed { exit_code, tail }) => {
            Err(ProvisionError::CommandFailed(CommandFailure {
                command: spec.display(),
                exit_code,
                tail,
            }))
        }
    }
}

/// True when the failure text points at connectivity rather than a permanent problem.
pub fn looks_transient(tail: &[String]) -> bool {
    tail.iter().any(|line| {
        let line = line.to_lowercase();
        [
            "could not resolve host",
            "unable to access",
            "timed out",
            "connection reset",
            "connection refused",
            "network is unreachable",
            "temporary failure",
            "tls",
            "ssl",
            "http 5",
            "429",
        ]
        .iter()
        .any(|needle| line.contains(needle))
    })
}

/// Turns a failed resolver or listing command into a sanitized failure message built from the
/// meaningful output lines.
pub fn failure_summary(error: ProvisionError) -> ProvisionError {
    match error {
        ProvisionError::CommandFailed(failure) => {
            let lines: Vec<&str> = failure
                .tail
                .iter()
                .map(String::as_str)
                .filter(|line| {
                    let line = line.trim();
                    !line.is_empty() && !line.starts_with("Using ") && !line.starts_with("Resolved")
                })
                .collect();
            let start = lines.len().saturating_sub(6);
            let mut text = lines[start..].join(" ");
            if text.len() > 600 {
                text.truncate(text.floor_char_boundary(600));
            }
            if text.is_empty() {
                text = "the command failed".into();
            }
            ProvisionError::Failed(text)
        }
        other => other,
    }
}
