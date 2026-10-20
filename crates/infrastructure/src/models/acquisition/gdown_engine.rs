//! The only adapter that runs the isolated gdown tool (Google Drive downloads and resolution).
use super::{
    bandwidth::BandwidthLimiter,
    finalize::StagingArea,
    hf_engine::runtime_failure_summary,
    hf_proxy::{HfProxyConfig, HfRelay, ProxyUnavailable, RelayScope},
};
use crate::{
    environment::layout::Layout,
    shared::process::{self, CommandSpec, LineSink, RawOutputError, RunError, RunLimits},
};
use application::{models::acquisition::*, shared::CancelToken};
use async_trait::async_trait;
use domain::{models::acquisition::*, shared::model_values::SafeFileName};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const DRIVER: &str = include_str!("gdown_driver.py");
const LINE_TAG: &str = "@vj-gdown ";
const RESOLVE_TIMEOUT: Duration = Duration::from_secs(90);

/// One structured line of the driver; anything untagged or malformed is ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DriverLine {
    Progress { done: u64, total: Option<u64> },
    Name(String),
    Error(String),
}

fn parse_line(line: &str) -> Option<DriverLine> {
    let value: serde_json::Value = serde_json::from_str(line.strip_prefix(LINE_TAG)?).ok()?;
    let object = value.as_object()?;
    if let Some(done) = object.get("done").and_then(serde_json::Value::as_u64) {
        return Some(DriverLine::Progress {
            done,
            total: object.get("total").and_then(serde_json::Value::as_u64),
        });
    }
    if let Some(name) = object.get("name").and_then(serde_json::Value::as_str) {
        return Some(DriverLine::Name(name.to_owned()));
    }
    object
        .get("error")
        .and_then(serde_json::Value::as_str)
        .map(|kind| DriverLine::Error(kind.to_owned()))
}

fn error_code(kind: &str) -> DownloadErrorCode {
    match kind {
        "quota" => DownloadErrorCode::DriveQuota,
        "permission" => DownloadErrorCode::DrivePermission,
        "not_found" => DownloadErrorCode::NotFound,
        "network" => DownloadErrorCode::Network,
        "service" => DownloadErrorCode::Service,
        "disk" => DownloadErrorCode::Disk,
        _ => DownloadErrorCode::ToolRuntime,
    }
}

fn error_kind_in(tail: &[String]) -> Option<String> {
    tail.iter().rev().find_map(|line| match parse_line(line) {
        Some(DriverLine::Error(kind)) => Some(kind),
        _ => None,
    })
}

fn run_error_code(error: RunError, kind: Option<String>) -> DownloadErrorCode {
    match error {
        RunError::Spawn(_) | RunError::InputFailed { .. } => DownloadErrorCode::ToolLaunch,
        RunError::Cancelled { .. } => DownloadErrorCode::Cancelled,
        RunError::TimedOut { .. } => DownloadErrorCode::Network,
        RunError::CleanupFailed { .. } => DownloadErrorCode::ProcessCleanupFailed,
        RunError::Failed { tail, .. } => {
            let code = kind
                .as_deref()
                .map_or(DownloadErrorCode::ToolRuntime, error_code);
            if code == DownloadErrorCode::ToolRuntime {
                tracing::warn!(summary = %runtime_failure_summary(&tail), "Google Drive tool runtime failure");
            }
            code
        }
    }
}

/// The interpreter of the provisioned `tools/gdown` venv; a missing tool falls back to native.
fn provisioned_python(root: &Path) -> Result<PathBuf> {
    let layout = Layout::new(root);
    let python = layout.gdown_python();
    if python.is_file() && layout.gdown_dir().join(".vj-lock").is_file() {
        Ok(python)
    } else {
        Err(DownloadErrorCode::ToolLaunch.into())
    }
}

/// `mode` is `resolve` or `download`; `output` is the exact file the download writes.
fn command(
    python: &Path,
    root: &Path,
    mode: &str,
    file_id: &str,
    output: Option<&Path>,
) -> Result<CommandSpec> {
    if file_id.is_empty()
        || file_id.len() > 200
        || !file_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(DownloadErrorCode::InvalidSource.into());
    }
    let home = root.join("data/cache/gdown/home");
    let temp = root.join("data/tmp/gdown");
    std::fs::create_dir_all(&home).map_err(|_| DownloadErrorCode::Disk)?;
    std::fs::create_dir_all(&temp).map_err(|_| DownloadErrorCode::Disk)?;
    let mut env = process::system_baseline();
    env.extend([
        (OsString::from("USERPROFILE"), home.as_os_str().into()),
        ("HOME".into(), home.into_os_string()),
        ("TMP".into(), temp.as_os_str().into()),
        ("TEMP".into(), temp.as_os_str().into()),
        ("TMPDIR".into(), temp.into_os_string()),
    ]);
    let mut spec = CommandSpec::new(python)
        .args(["-I", "-B", "-X", "utf8", "-c", DRIVER, mode, file_id])
        .cwd(root)
        .env_vars(env);
    if let Some(output) = output {
        spec = spec.arg(output.as_os_str());
    }
    Ok(spec)
}

struct Speed {
    last: u64,
    at: Instant,
    speed: f64,
}

/// Turns the driver's progress lines into samples and remembers its last error kind.
struct GdownLines<'a> {
    job: &'a DownloadJob,
    sink: &'a dyn ProgressSink,
    speed: Mutex<Speed>,
    error: Mutex<Option<String>>,
}
impl<'a> GdownLines<'a> {
    fn new(job: &'a DownloadJob, sink: &'a dyn ProgressSink) -> Self {
        Self {
            job,
            sink,
            speed: Mutex::new(Speed {
                last: 0,
                at: Instant::now(),
                speed: 0.0,
            }),
            error: Mutex::new(None),
        }
    }
    fn error(&self) -> Option<String> {
        self.error.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}
impl LineSink for GdownLines<'_> {
    fn line(&self, line: &str) {
        match parse_line(line) {
            Some(DriverLine::Progress { done, total }) => {
                let total = total.map_or(self.job.size, DownloadSize::Exact);
                let mut state = self.speed.lock().unwrap_or_else(|p| p.into_inner());
                let now = Instant::now();
                let elapsed = now.duration_since(state.at).as_secs_f64();
                let sample = done.saturating_sub(state.last) as f64 / elapsed.max(0.001);
                state.speed = if state.speed == 0.0 {
                    sample
                } else {
                    state.speed * 0.7 + sample * 0.3
                };
                state.last = done;
                state.at = now;
                let speed = state.speed;
                drop(state);
                self.sink.sample(
                    &self.job.id,
                    self.job.generation,
                    TransferProgress {
                        bytes: done,
                        total,
                        bytes_per_second: speed,
                        eta_seconds: total.bytes().filter(|n| *n >= done).and_then(|n| {
                            (speed > 0.0).then_some(((n - done) as f64 / speed) as u64)
                        }),
                        estimated: false,
                    },
                );
            }
            Some(DriverLine::Error(kind)) => {
                *self.error.lock().unwrap_or_else(|p| p.into_inner()) = Some(kind);
            }
            _ => {}
        }
    }
}

pub struct GdownDownloader {
    pub bandwidth: Arc<BandwidthLimiter>,
    pub proxy: std::result::Result<HfProxyConfig, ProxyUnavailable>,
    pub root: PathBuf,
    pub staging: Arc<StagingArea>,
}
impl GdownDownloader {
    async fn relay(&self) -> Result<HfRelay> {
        HfRelay::start_scoped(
            self.bandwidth.clone(),
            self.proxy
                .clone()
                .map_err(|_| DownloadErrorCode::BandwidthProxyUnavailable)?,
            RelayScope::GoogleDrive,
        )
        .await
        .map_err(|_| DownloadErrorCode::BandwidthProxyUnavailable.into())
    }

    /// The file name Drive reports for `source`; only a tool that fails to launch falls back.
    pub async fn resolve(&self, source: &SourceRef) -> Result<ResolvedFile> {
        let SourceRef::GoogleDrive { file_id } = source else {
            return Err(DownloadErrorCode::InvalidSource.into());
        };
        let python = provisioned_python(&self.root)?;
        self.resolve_with(&python, source, file_id).await
    }

    async fn resolve_with(
        &self,
        python: &Path,
        source: &SourceRef,
        file_id: &str,
    ) -> Result<ResolvedFile> {
        let mut spec = command(python, &self.root, "resolve", file_id, None)?;
        let relay = self.relay().await?;
        relay.configure_child(&mut spec.env);
        let limits = RunLimits {
            timeout: Some(RESOLVE_TIMEOUT),
            ..RunLimits::default()
        };
        let output = process::run_raw_stdout(&spec, &CancelToken::new(), &limits).await;
        // Resolution abandons the body mid-stream, which the relay reports as a failure.
        let _ = relay.shutdown().await;
        let stdout = match output {
            Ok(bytes) => bytes,
            Err(RawOutputError::Run(error)) => {
                let kind = match &error {
                    RunError::Failed { tail, .. } => error_kind_in(tail),
                    _ => None,
                };
                return Err(run_error_code(error, kind).into());
            }
            Err(RawOutputError::LimitExceeded | RawOutputError::Incomplete) => {
                return Err(DownloadErrorCode::ToolRuntime.into());
            }
        };
        let text = String::from_utf8(stdout).map_err(|_| DownloadErrorCode::ToolRuntime)?;
        let name = text
            .lines()
            .find_map(|line| match parse_line(line.trim_end()) {
                Some(DriverLine::Name(name)) => Some(name),
                _ => None,
            })
            .ok_or(DownloadErrorCode::ToolRuntime)?;
        let filename = SafeFileName::new(name)
            .ok()
            .or_else(|| SafeFileName::new(format!("{file_id}.bin")).ok())
            .ok_or(DownloadErrorCode::InvalidMetadata)?;
        Ok(ResolvedFile {
            source: source.clone(),
            identity: ProviderIdentity(format!("gdrive:{file_id}")),
            filename,
            size: DownloadSize::Unknown,
            sha256: None,
            suggested_category: None,
            primary: true,
        })
    }

    async fn transfer_with(
        &self,
        python: &Path,
        job: &DownloadJob,
        file_id: &str,
        sink: Arc<dyn ProgressSink>,
        cancel: CancelToken,
    ) -> Result<TransferResult> {
        let directory = self
            .staging
            .gdrive_directory(job, true)
            .map_err(|_| DownloadErrorCode::Disk)?;
        let output = directory.path.join("payload");
        let mut spec = command(python, &self.root, "download", file_id, Some(&output))?;
        let relay = self.relay().await?;
        relay.configure_child(&mut spec.env);
        let lines = GdownLines::new(job, sink.as_ref());
        let outcome = process::run(&spec, &lines, &cancel, &RunLimits::default()).await;
        let proxy_failed = relay.shutdown().await;
        let result: Result<()> = match outcome {
            Ok(_) => Ok(()),
            Err(error) => Err(run_error_code(error, lines.error()).into()),
        };
        if proxy_failed
            && result.as_ref().is_err_and(|error: &DownloadError| {
                !matches!(
                    error.0,
                    DownloadErrorCode::Cancelled | DownloadErrorCode::ProcessCleanupFailed
                )
            })
        {
            return Err(DownloadErrorCode::BandwidthProxyUnavailable.into());
        }
        result?;
        let key = format!("gdrive/{}/payload", job.id.as_str());
        let (_, _, file) = self
            .staging
            .open_key(&key, true, false)
            .map_err(|_| DownloadErrorCode::Disk)?;
        let size = file.metadata().map_err(|_| DownloadErrorCode::Disk)?.len();
        file.sync_all().map_err(|_| DownloadErrorCode::Disk)?;
        if matches!(job.size, DownloadSize::Exact(n) if n != size) {
            return Err(DownloadErrorCode::LengthMismatch.into());
        }
        Ok(TransferResult {
            staged_key: key,
            validator: None,
            size,
        })
    }
}
#[async_trait]
impl Downloader for GdownDownloader {
    async fn transfer(
        &self,
        job: &DownloadJob,
        sink: Arc<dyn ProgressSink>,
        cancel: CancelToken,
    ) -> Result<TransferResult> {
        let SourceRef::GoogleDrive { file_id } = &job.source else {
            return Err(DownloadErrorCode::InvalidSource.into());
        };
        // A missing tool must fall back before the relay starts or anything is created.
        let python = provisioned_python(&self.root)?;
        self.transfer_with(&python, job, file_id, sink, cancel)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{
        models::library::ModelCategory,
        shared::model_values::{JobId, RelativeModelPath},
    };

    const REAL_FILE: &str = "1l_5RK28JRL19wpT22B-DY9We3TVXnnQQ";
    const REAL_SIZE: u64 = 498_881_336;

    fn drive_job(file_id: &str) -> DownloadJob {
        DownloadJob {
            id: JobId::new("gdrive-home").unwrap(),
            source: SourceRef::GoogleDrive {
                file_id: file_id.into(),
            },
            identity: ProviderIdentity(format!("gdrive:{file_id}")),
            destination: Destination {
                root_id: "models".into(),
                category: ModelCategory::new("checkpoints").unwrap(),
                subdirectory: RelativeModelPath::default(),
                filename: SafeFileName::new("payload.bin").unwrap(),
            },
            state: DownloadState::Queued,
            requested_engine: DownloadEngine::Gdown,
            effective_engine: DownloadEngine::Gdown,
            generation: 0,
            size: DownloadSize::Unknown,
            expected_sha256: None,
            bytes_done: 0,
            validator: None,
            retry_count: 0,
            retry_at: None,
            stop_intent: None,
            error: None,
            conflict: None,
            decision_id: None,
            observed: None,
            publication: None,
            created_at: 1,
            updated_at: 1,
        }
    }

    #[test]
    fn gdown_command_isolates_python_and_home() {
        let root = tempfile::tempdir().unwrap();
        let python = root.path().join("python-under-test");
        let output = root.path().join("staging/payload");
        let spec = command(&python, root.path(), "download", "AbC-123_x", Some(&output)).unwrap();
        assert_eq!(spec.program, python);
        let expected: Vec<OsString> = vec![
            "-I".into(),
            "-B".into(),
            "-X".into(),
            "utf8".into(),
            "-c".into(),
            DRIVER.into(),
            "download".into(),
            "AbC-123_x".into(),
            output.clone().into_os_string(),
        ];
        assert_eq!(spec.args, expected);
        assert_eq!(output.file_name().unwrap(), "payload");
        assert_eq!(spec.cwd.as_deref(), Some(root.path()));
        let home = root.path().join("data/cache/gdown/home");
        assert!(home.is_dir());
        assert!(root.path().join("data/tmp/gdown").is_dir());
        for key in ["USERPROFILE", "HOME"] {
            let values: Vec<_> = spec.env.iter().filter(|(k, _)| k == key).collect();
            assert_eq!(values.len(), 1, "{key}");
            assert_eq!(values[0].1, home.as_os_str());
        }
        for (key, _) in &spec.env {
            let key = key.to_string_lossy().to_ascii_uppercase();
            assert!(
                !matches!(
                    key.as_str(),
                    "HF_TOKEN"
                        | "HOMEDRIVE"
                        | "HOMEPATH"
                        | "PYTHONPATH"
                        | "HTTP_PROXY"
                        | "HTTPS_PROXY"
                        | "ALL_PROXY"
                        | "NO_PROXY"
                ) && !key.starts_with("PYTHON"),
                "{key}"
            );
        }
        let resolve = command(&python, root.path(), "resolve", "AbC-123_x", None).unwrap();
        assert_eq!(resolve.args.len(), 8);
        for bad in ["", "a b", "../x", "a/b", "id;rm"] {
            assert_eq!(
                command(&python, root.path(), "resolve", bad, None)
                    .unwrap_err()
                    .0,
                DownloadErrorCode::InvalidSource,
                "{bad:?}"
            );
        }
    }

    #[test]
    fn gdown_command_requires_the_provisioned_tool() {
        let root = tempfile::tempdir().unwrap();
        let layout = Layout::new(root.path());
        assert_eq!(
            provisioned_python(root.path()).unwrap_err().0,
            DownloadErrorCode::ToolLaunch
        );
        std::fs::create_dir_all(layout.gdown_python().parent().unwrap()).unwrap();
        std::fs::write(layout.gdown_python(), b"python").unwrap();
        assert_eq!(
            provisioned_python(root.path()).unwrap_err().0,
            DownloadErrorCode::ToolLaunch,
            "the interpreter alone is not a provisioned tool"
        );
        std::fs::write(layout.gdown_dir().join(".vj-lock"), b"marker").unwrap();
        assert_eq!(
            provisioned_python(root.path()).unwrap(),
            layout.gdown_python()
        );
    }

    #[test]
    fn driver_lines_parse_and_errors_map() {
        assert_eq!(
            parse_line(r#"@vj-gdown {"done":10,"total":100}"#),
            Some(DriverLine::Progress {
                done: 10,
                total: Some(100)
            })
        );
        assert_eq!(
            parse_line(r#"@vj-gdown {"done":10,"total":null}"#),
            Some(DriverLine::Progress {
                done: 10,
                total: None
            })
        );
        assert_eq!(
            parse_line(r#"@vj-gdown {"name":"mödel.safetensors"}"#),
            Some(DriverLine::Name("m\u{f6}del.safetensors".into()))
        );
        assert_eq!(
            parse_line(r#"@vj-gdown {"error":"quota","detail":"x"}"#),
            Some(DriverLine::Error("quota".into()))
        );
        for line in [
            "",
            "plain output",
            r#"{"done":1,"total":2}"#,
            r#"@vj-gdown {"done":"#,
            r#"@vj-gdown [1,2]"#,
            r#"@vj-gdown {"unknown":1}"#,
        ] {
            assert_eq!(parse_line(line), None, "{line:?}");
        }
        for (kind, code) in [
            ("quota", DownloadErrorCode::DriveQuota),
            ("permission", DownloadErrorCode::DrivePermission),
            ("not_found", DownloadErrorCode::NotFound),
            ("network", DownloadErrorCode::Network),
            ("service", DownloadErrorCode::Service),
            ("disk", DownloadErrorCode::Disk),
            ("other", DownloadErrorCode::ToolRuntime),
            ("surprise", DownloadErrorCode::ToolRuntime),
        ] {
            assert_eq!(error_code(kind), code, "{kind}");
        }
        let failed = |kind: Option<&str>| {
            run_error_code(
                RunError::Failed {
                    exit_code: Some(1),
                    tail: Vec::new(),
                },
                kind.map(str::to_owned),
            )
        };
        assert_eq!(
            failed(Some("permission")),
            DownloadErrorCode::DrivePermission
        );
        assert_eq!(failed(None), DownloadErrorCode::ToolRuntime);
        assert_eq!(
            run_error_code(RunError::Spawn(std::io::ErrorKind::NotFound), None),
            DownloadErrorCode::ToolLaunch
        );
        assert_eq!(
            run_error_code(RunError::Cancelled { tail: Vec::new() }, None),
            DownloadErrorCode::Cancelled
        );
        assert_eq!(
            run_error_code(RunError::TimedOut { tail: Vec::new() }, None),
            DownloadErrorCode::Network
        );
        assert_eq!(
            run_error_code(RunError::CleanupFailed { tail: Vec::new() }, None),
            DownloadErrorCode::ProcessCleanupFailed
        );
        let tail = vec![
            "@vj-gdown {\"done\":1,\"total\":2}".to_owned(),
            "@vj-gdown {\"error\":\"service\",\"detail\":\"x\"}".to_owned(),
        ];
        assert_eq!(error_kind_in(&tail).as_deref(), Some("service"));
        assert_eq!(error_kind_in(&tail[..1]), None);
    }

    #[test]
    fn driver_never_persists_cookies() {
        assert_eq!(DRIVER.matches("use_cookies=False").count(), 2);
        assert!(!DRIVER.contains("use_cookies=True"));
    }

    struct LastSample(Mutex<Option<TransferProgress>>);
    impl ProgressSink for LastSample {
        fn sample(
            &self,
            _: &domain::shared::model_values::JobId,
            _: u64,
            progress: TransferProgress,
        ) {
            *self.0.lock().unwrap() = Some(progress);
        }
    }

    #[test]
    fn progress_lines_become_exact_samples() {
        let job = drive_job("AbC-123_x");
        let sink = LastSample(Mutex::new(None));
        let lines = GdownLines::new(&job, &sink);
        lines.line(r#"@vj-gdown {"done":50,"total":200}"#);
        let sample = sink.0.lock().unwrap().take().unwrap();
        assert_eq!(sample.bytes, 50);
        assert_eq!(sample.total, DownloadSize::Exact(200));
        assert!(!sample.estimated);
        lines.line(r#"@vj-gdown {"error":"quota","detail":"x"}"#);
        lines.line("noise");
        assert!(sink.0.lock().unwrap().is_none());
        assert_eq!(lines.error().as_deref(), Some("quota"));
    }

    /// Set VJ_REAL_GDOWN_PYTHON to the absolute path of a venv Python with gdown 6.4.2.
    #[tokio::test]
    #[ignore = "requires a real gdown 6.4.2 venv, network and a 498,881,336-byte download"]
    async fn real_gdown_resolves_and_downloads_through_the_relay() {
        let python = PathBuf::from(std::env::var_os("VJ_REAL_GDOWN_PYTHON").unwrap());
        assert!(python.is_absolute() && python.is_file());
        let root = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
        let staging = Arc::new(StagingArea::new(&root.path().join("downloads")).unwrap());
        let downloader = GdownDownloader {
            bandwidth: Arc::new(BandwidthLimiter::new(None)),
            proxy: HfProxyConfig::capture(),
            root: root.path().to_owned(),
            staging,
        };

        let source = SourceRef::GoogleDrive {
            file_id: REAL_FILE.into(),
        };
        let resolved = downloader
            .resolve_with(&python, &source, REAL_FILE)
            .await
            .unwrap();
        assert_eq!(resolved.filename.as_str(), "fcn8s_from_caffe.npz");

        // Drive the tool exactly as `transfer` does, to observe the relay's failure flag.
        let job = drive_job(REAL_FILE);
        let directory = downloader.staging.gdrive_directory(&job, true).unwrap();
        let output = directory.path.join("payload");
        let mut spec = command(&python, root.path(), "download", REAL_FILE, Some(&output)).unwrap();
        let relay = downloader.relay().await.unwrap();
        relay.configure_child(&mut spec.env);
        let sink = LastSample(Mutex::new(None));
        let lines = GdownLines::new(&job, &sink);
        let result = process::run(
            &spec,
            &lines,
            &CancelToken::new(),
            &RunLimits {
                timeout: Some(Duration::from_secs(900)),
                ..RunLimits::default()
            },
        )
        .await;
        assert!(
            !relay.shutdown().await,
            "the relay must not report a failure"
        );
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(std::fs::metadata(&output).unwrap().len(), REAL_SIZE);
        let last = sink.0.lock().unwrap().take().unwrap();
        assert_eq!(last.total, DownloadSize::Exact(REAL_SIZE));

        let denied = drive_job("1AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
        let error = downloader
            .transfer_with(
                &python,
                &denied,
                "1AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                Arc::new(LastSample(Mutex::new(None))),
                CancelToken::new(),
            )
            .await
            .unwrap_err();
        assert_eq!(error.0, DownloadErrorCode::DrivePermission);
    }
}
