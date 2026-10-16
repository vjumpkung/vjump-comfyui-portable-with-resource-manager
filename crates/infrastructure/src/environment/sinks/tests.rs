use application::environment::{FailureKind, RunId, RunStatus, StepFailure, StepId, StepStatus};

use super::*;
use crate::shared::logging;

#[derive(Default)]
struct Collect(Mutex<Vec<SetupEvent>>);

impl SetupProgressSink for Collect {
    fn emit(&self, event: SetupEvent) {
        self.0.lock().unwrap().push(event);
    }
}

fn run_id() -> RunId {
    RunId::new("run-abc").unwrap()
}

const SECRET: &str = "super-secret-token-value-12345";

fn secret_failure() -> StepFailure {
    StepFailure {
        step: StepId::InstallTorch,
        kind: FailureKind::CommandFailed,
        message: format!("failed with token={SECRET}"),
        command: Some(format!(
            "uv pip install --index-url https://user:{SECRET}@host/simple torch"
        )),
        exit_code: Some(1),
        tail: vec![
            "Authorization: Bearer abcdef0123456789abcdef".into(),
            "X-Amz-Signature=0123456789abcdef0123456789abcdef".into(),
            "plain line".into(),
        ],
        consent: Some(ConsentRequest {
            source: format!("https://x:{SECRET}@example.test/repo.git"),
            commit: "a".repeat(40),
        }),
    }
}

fn all_events() -> Vec<SetupEvent> {
    vec![
        SetupEvent::RunStatus {
            run_id: run_id(),
            status: RunStatus::Running,
        },
        SetupEvent::Step {
            run_id: run_id(),
            step: StepId::Tools,
            status: StepStatus::Running,
            attempt: 1,
            message: Some(format!("using token={SECRET}")),
        },
        SetupEvent::Log {
            run_id: run_id(),
            step: StepId::Tools,
            line: format!("Authorization: Bearer {SECRET}"),
        },
        SetupEvent::Progress {
            run_id: run_id(),
            step: StepId::Tools,
            done_bytes: 1,
            total_bytes: Some(2),
        },
        SetupEvent::Failure {
            run_id: run_id(),
            failure: Box::new(secret_failure()),
        },
    ]
}

fn debug_text(events: &[SetupEvent]) -> String {
    format!("{events:?}")
}

#[test]
fn redaction_covers_every_free_text_field() {
    logging::register_secret(SECRET);
    let redacted: Vec<SetupEvent> = all_events().into_iter().map(redact_event).collect();
    let text = debug_text(&redacted);
    for secret in [
        SECRET,
        "abcdef0123456789abcdef",
        "0123456789abcdef0123456789abcdef",
    ] {
        assert!(!text.contains(secret), "{secret} leaked: {text}");
    }
    assert!(text.contains("plain line"));
    assert!(text.contains("[REDACTED]"));
}

#[test]
fn the_redacting_sink_scrubs_before_the_channel_sees_anything() {
    logging::register_secret(SECRET);
    let channel = Arc::new(Collect::default());
    let sink = RedactingSink::new(channel.clone());
    for event in all_events() {
        sink.emit(event);
    }
    let received = channel.0.lock().unwrap().clone();
    assert_eq!(received.len(), 5);
    assert!(!debug_text(&received).contains(SECRET));
}

#[test]
fn the_run_log_file_is_redacted_appended_and_named_after_the_run() {
    logging::register_secret(SECRET);
    let directory = tempfile::tempdir().unwrap();
    let logs = directory.path().join("data ไทย").join("logs");
    let channel = Arc::new(Collect::default());
    let (sink, path) = run_sink(&logs, "run-abc", channel.clone());
    assert_eq!(path, logs.join("setup-run-abc.log"));
    for event in all_events() {
        sink.emit(event);
    }
    let text = fs::read_to_string(&path).unwrap();
    assert!(!text.contains(SECRET), "{text}");
    assert!(!text.contains("abcdef0123456789abcdef"));
    assert!(text.contains("[tools]"));
    assert!(text.contains("== tools running (attempt 1)"));
    assert!(text.contains("== run running"));
    assert!(text.contains("!! install_torch failed (command_failed)"));
    assert!(text.contains("!! exit code: 1"));
    assert!(text.contains("!! | plain line"));
    assert!(
        !text.contains("done_bytes"),
        "progress events are not logged"
    );
    assert_eq!(channel.0.lock().unwrap().len(), 5);

    // A retry appends to the same file instead of replacing it.
    let (retry, same_path) = run_sink(&logs, "run-abc", Arc::new(Collect::default()));
    assert_eq!(same_path, path);
    retry.emit(SetupEvent::Log {
        run_id: run_id(),
        step: StepId::Seed,
        line: "second attempt".into(),
    });
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("== tools running"));
    assert!(text.contains("[seed] second attempt"));
    // Every line is timestamped.
    assert!(
        text.lines()
            .all(|line| line.len() > 20 && line.as_bytes()[4] == b'-')
    );
}

#[test]
fn an_unwritable_log_never_stops_the_events() {
    let directory = tempfile::tempdir().unwrap();
    // A file where the logs directory should be.
    let blocker = directory.path().join("logs");
    fs::write(&blocker, b"not a directory").unwrap();
    let channel = Arc::new(Collect::default());
    let (sink, _) = run_sink(&blocker, "run-abc", channel.clone());
    sink.emit(SetupEvent::Log {
        run_id: run_id(),
        step: StepId::Tools,
        line: "still delivered".into(),
    });
    assert_eq!(channel.0.lock().unwrap().len(), 1);
}
