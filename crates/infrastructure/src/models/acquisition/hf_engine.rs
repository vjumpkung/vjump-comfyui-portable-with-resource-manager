//! The only adapter allowed to construct an HF child containing HF_TOKEN.
use super::{
    bandwidth::BandwidthLimiter,
    finalize::StagingArea,
    hf_proxy::{HfProxyConfig, HfRelay, ProxyUnavailable},
};
use crate::{
    settings::secrets::CredentialVault,
    shared::{
        logging,
        process::{self, CommandSpec, DiscardSink, RunError, RunLimits},
    },
};
use application::{models::acquisition::*, shared::CancelToken};
use async_trait::async_trait;
use domain::{models::acquisition::*, settings::CredentialKind};
use std::{ffi::OsString, path::PathBuf, sync::Arc, time::Instant};

pub struct HfDownloadCommand {
    spec: CommandSpec,
}
impl HfDownloadCommand {
    pub async fn for_job(
        root: &std::path::Path,
        job: &DownloadJob,
        local_dir: PathBuf,
        vault: &dyn CredentialVault,
    ) -> Result<Self> {
        let SourceRef::HuggingFace {
            repo_type,
            repo,
            revision,
            path: Some(path),
        } = &job.source
        else {
            return Err(DownloadErrorCode::InvalidSource.into());
        };
        if revision.len() != 40
            || !revision.bytes().all(|b| b.is_ascii_hexdigit())
            || repo.split('/').count() != 2
            || repo
                .split('/')
                .any(|p| domain::shared::model_values::SafeFileName::new(p).is_err())
            || path.components().is_empty()
        {
            return Err(DownloadErrorCode::InvalidSource.into());
        }
        let executable = root.join(if cfg!(windows) {
            "tools/hf/Scripts/hf.exe"
        } else {
            "tools/hf/bin/hf"
        });
        if !executable.is_absolute() || !local_dir.is_absolute() {
            return Err(DownloadErrorCode::ToolLaunch.into());
        }
        let snapshot = vault
            .read(CredentialKind::HuggingFace)
            .await
            .map_err(|_| DownloadErrorCode::TokenRequired)?;
        let mut env = process::system_baseline();
        let cache = root.join("data/cache/hf");
        let home = root.join("data/cache/hf/home");
        let temp = root.join("data/tmp/hf");
        std::fs::create_dir_all(&cache).map_err(|_| DownloadErrorCode::Disk)?;
        std::fs::create_dir_all(&home).map_err(|_| DownloadErrorCode::Disk)?;
        std::fs::create_dir_all(&temp).map_err(|_| DownloadErrorCode::Disk)?;
        env.extend([
            (OsString::from("HF_HOME"), cache.into_os_string()),
            ("USERPROFILE".into(), home.as_os_str().into()),
            ("HOME".into(), home.into_os_string()),
            ("HF_HUB_DISABLE_TELEMETRY".into(), "1".into()),
            ("PYTHONNOUSERSITE".into(), "1".into()),
            ("PYTHONUTF8".into(), "1".into()),
            ("TMP".into(), temp.as_os_str().into()),
            ("TEMP".into(), temp.as_os_str().into()),
            ("TMPDIR".into(), temp.into_os_string()),
        ]);
        if let Some(token) = snapshot.value {
            env.push(("HF_TOKEN".into(), token.expose().into()));
        }
        let spec = CommandSpec::new(executable)
            .args([
                "download".to_owned(),
                repo.clone(),
                path.as_string(),
                "--type".into(),
                repo_type.singular().into(),
                "--revision".into(),
                revision.clone(),
                "--local-dir".into(),
            ])
            .arg(local_dir.as_os_str())
            .args(["--format", "json"])
            .cwd(root)
            .env_vars(env);
        Ok(Self { spec })
    }
}
pub struct HfDownloader {
    pub bandwidth: Arc<BandwidthLimiter>,
    pub proxy: std::result::Result<HfProxyConfig, ProxyUnavailable>,
    pub root: PathBuf,
    pub staging: Arc<StagingArea>,
    pub vault: Arc<dyn CredentialVault>,
}
#[async_trait]
impl Downloader for HfDownloader {
    async fn transfer(
        &self,
        job: &DownloadJob,
        sink: Arc<dyn ProgressSink>,
        cancel: CancelToken,
    ) -> Result<TransferResult> {
        let directory = self
            .staging
            .hf_directory(job, true)
            .map_err(|_| DownloadErrorCode::Disk)?;
        let SourceRef::HuggingFace {
            path: Some(path), ..
        } = &job.source
        else {
            return Err(DownloadErrorCode::InvalidSource.into());
        };
        let relay = HfRelay::start(
            self.bandwidth.clone(),
            self.proxy
                .clone()
                .map_err(|_| DownloadErrorCode::BandwidthProxyUnavailable)?,
        )
        .await
        .map_err(|_| DownloadErrorCode::BandwidthProxyUnavailable)?;
        let result: Result<()> = async {
            let mut command = HfDownloadCommand::for_job(
                &self.root,
                job,
                directory.path.clone(),
                self.vault.as_ref(),
            )
            .await?;
            relay.configure_child(&mut command.spec.env);
            let limits = RunLimits::default();
            let line_sink = DiscardSink;
            let future = process::run(&command.spec, &line_sink, &cancel, &limits);
            tokio::pin!(future);
            let mut interval = tokio::time::interval(PROGRESS_INTERVAL);
            let mut last = 0;
            let mut last_at = Instant::now();
            let mut speed = 0.0;
            loop {
                tokio::select! {
                    result = &mut future => return match result {
                        Ok(_) => Ok(()),
                        Err(RunError::Spawn(_) | RunError::InputFailed { .. }) => Err(DownloadErrorCode::ToolLaunch.into()),
                        Err(RunError::Cancelled { .. }) => Err(DownloadErrorCode::Cancelled.into()),
                        Err(RunError::TimedOut { .. }) => Err(DownloadErrorCode::Network.into()),
                        Err(RunError::CleanupFailed { .. }) => Err(DownloadErrorCode::ProcessCleanupFailed.into()),
                        Err(RunError::Failed { tail, .. }) => {
                            let code = classify_stderr(&tail.join("\n"));
                            if code == DownloadErrorCode::ToolRuntime {
                                tracing::warn!(summary = %runtime_failure_summary(&tail), "Hugging Face tool runtime failure");
                            }
                            Err(code.into())
                        },
                    },
                    _ = interval.tick() => {
                        let dir = directory.path.clone();
                        let relative = path.as_string();
                        let polled = tokio::task::spawn_blocking(move || poll_bytes(&dir, &relative))
                            .await
                            .map_err(|_| DownloadError::from(DownloadErrorCode::Disk))
                            .and_then(|result| result);
                        let bytes = match polled {
                            Ok(bytes) => bytes,
                            Err(error) => {
                                // A polling failure still owns a live child. Reap it before
                                // closing and joining the attempt's relay connections.
                                cancel.cancel();
                                if matches!(future.await, Err(RunError::CleanupFailed { .. })) {
                                    return Err(DownloadErrorCode::ProcessCleanupFailed.into());
                                }
                                return Err(error);
                            }
                        };
                        let now = Instant::now();
                        let elapsed = now.duration_since(last_at).as_secs_f64();
                        let sample = bytes.saturating_sub(last) as f64 / elapsed.max(0.001);
                        speed = if speed == 0.0 { sample } else { speed * 0.7 + sample * 0.3 };
                        last = bytes;
                        last_at = now;
                        sink.sample(&job.id, job.generation, TransferProgress {
                            bytes,
                            total: job.size,
                            bytes_per_second: speed,
                            eta_seconds: job.size.bytes().filter(|n| *n >= bytes).and_then(|n| {
                                (speed > 0.0).then_some(((n - bytes) as f64 / speed) as u64)
                            }),
                            estimated: true,
                        });
                    }
                }
            }
        }
        .await;
        let proxy_failed = relay.shutdown().await;
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
        let key = format!("hf/{}/{}", job.id.as_str(), path.as_string());
        let (_, _, file) = self
            .staging
            .open_key(&key, true, false)
            .map_err(|_| DownloadErrorCode::Disk)?;
        let size = file.metadata().map_err(|_| DownloadErrorCode::Disk)?.len();
        file.sync_all().map_err(|_| DownloadErrorCode::Disk)?;
        if matches!(job.size,DownloadSize::Exact(n) if n!=size) {
            return Err(DownloadErrorCode::LengthMismatch.into());
        }
        Ok(TransferResult {
            staged_key: key,
            validator: None,
            size,
        })
    }
}
fn runtime_failure_summary(tail: &[String]) -> String {
    let mut summary = tail[tail.len().saturating_sub(20)..]
        .iter()
        .map(|line| logging::redact(line).chars().take(500).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    let mut end = summary.len().min(4 * 1024);
    while !summary.is_char_boundary(end) {
        end -= 1;
    }
    summary.truncate(end);
    summary
}

pub fn classify_stderr(text: &str) -> DownloadErrorCode {
    let text = text.to_ascii_lowercase();
    if text.contains("gatedrepo") || text.contains("accept the license") {
        DownloadErrorCode::GatedLicense
    } else if text.contains("401") || text.contains("invalid token") {
        DownloadErrorCode::InvalidToken
    } else if text.contains("entrynotfound") {
        DownloadErrorCode::FileNotFound
    } else if text.contains("revisionnotfound") {
        DownloadErrorCode::RevisionNotFound
    } else if text.contains("repositorynotfound") {
        DownloadErrorCode::RepositoryUnavailable
    } else if text.contains("no space left") || text.contains("disk full") {
        DownloadErrorCode::Disk
    } else if text.contains("timeout") || text.contains("connection") || text.contains("network") {
        DownloadErrorCode::Network
    } else if text.contains("502") || text.contains("503") || text.contains("500") {
        DownloadErrorCode::Service
    } else {
        DownloadErrorCode::ToolRuntime
    }
}
/// One selected file per attempt: use max(current incomplete, staged), never sum a rename.
pub fn poll_bytes(directory: &std::path::Path, selected: &str) -> Result<u64> {
    let staged = directory.join(selected);
    if let Ok(metadata) = std::fs::symlink_metadata(&staged) {
        if metadata.file_type().is_symlink() {
            return Err(DownloadErrorCode::Containment.into());
        }
        if metadata.is_file() {
            return Ok(metadata.len());
        }
    }
    let relative = std::path::Path::new(selected);
    let cache = directory
        .join(".cache/huggingface/download")
        .join(relative.parent().unwrap_or(std::path::Path::new("")));
    let prefix = format!(
        "{}.",
        relative.file_name().unwrap_or_default().to_string_lossy()
    );
    let entries = match std::fs::read_dir(cache) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(_) => return Err(DownloadErrorCode::Disk.into()),
    };
    let mut bytes = 0;
    for entry in entries {
        let entry = entry.map_err(|_| DownloadErrorCode::Disk)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(&prefix) && name.ends_with(".incomplete") {
            let metadata = entry.metadata().map_err(|_| DownloadErrorCode::Disk)?;
            if entry
                .file_type()
                .map_err(|_| DownloadErrorCode::Disk)?
                .is_symlink()
            {
                return Err(DownloadErrorCode::Containment.into());
            }
            bytes = bytes.max(metadata.len());
        }
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::secrets::CredentialSnapshot;
    use domain::{
        models::library::ModelCategory,
        shared::model_values::{JobId, RelativeModelPath, SafeFileName, Sha256},
    };

    const FILE: &str = "vae-ft-mse-840000-ema-pruned.safetensors";
    const SIZE: u64 = 334_641_190;
    const SHA256: &str = "735e4c3a447a3255760d7f86845f09f937809baa529c17370d83e4c3758f3c75";

    struct NoCredentials;

    #[async_trait]
    impl CredentialVault for NoCredentials {
        async fn read(
            &self,
            _: CredentialKind,
        ) -> std::result::Result<CredentialSnapshot, application::settings::CredentialStoreError>
        {
            Ok(CredentialSnapshot {
                generation: 0,
                value: None,
            })
        }

        async fn generation(
            &self,
            _: CredentialKind,
        ) -> std::result::Result<u64, application::settings::CredentialStoreError> {
            Ok(0)
        }
    }

    fn hf_job() -> DownloadJob {
        DownloadJob {
            id: JobId::new("hf-home").unwrap(),
            source: SourceRef::HuggingFace {
                repo_type: RepoType::Model,
                repo: "stabilityai/sd-vae-ft-mse-original".into(),
                revision: "629b3ad3030ce36e15e70c5db7d91df0d60c627f".into(),
                path: Some(RelativeModelPath::new(FILE).unwrap()),
            },
            identity: ProviderIdentity("hf-home".into()),
            destination: Destination {
                root_id: "models".into(),
                category: ModelCategory::new("vae").unwrap(),
                subdirectory: RelativeModelPath::default(),
                filename: SafeFileName::new(FILE).unwrap(),
            },
            state: DownloadState::Queued,
            requested_engine: DownloadEngine::HfCli,
            effective_engine: DownloadEngine::HfCli,
            generation: 0,
            size: DownloadSize::Exact(SIZE),
            expected_sha256: Some(Sha256::new(SHA256).unwrap()),
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

    #[tokio::test]
    async fn hf_spec_uses_an_app_owned_home() {
        let root = tempfile::tempdir().unwrap();
        let command = HfDownloadCommand::for_job(
            root.path(),
            &hf_job(),
            root.path().join("files"),
            &NoCredentials,
        )
        .await
        .unwrap();
        let home = root.path().join("data/cache/hf/home");
        assert!(home.is_dir());
        for key in ["USERPROFILE", "HOME"] {
            let values: Vec<_> = command.spec.env.iter().filter(|(k, _)| k == key).collect();
            assert_eq!(values.len(), 1);
            assert_eq!(values[0].1, home.as_os_str());
            assert_ne!(Some(values[0].1.clone()), std::env::var_os(key));
        }
    }

    #[test]
    fn hf_runtime_failure_summary_is_bounded_and_redacted() {
        let secret = "d4-registered-runtime-secret";
        let token = "hf_abcdefghijklmnopqrstuvwxyz123456";
        logging::register_secret(secret);
        let mut tail = vec!["discarded old line".into(); 5];
        tail.push(format!("{secret} {token}"));
        tail.extend((0..19).map(|index| format!("line {index}")));
        let summary = runtime_failure_summary(&tail);
        assert_eq!(summary.lines().count(), 20);
        assert!(!summary.contains("discarded old line"));
        assert!(!summary.contains(secret));
        assert!(!summary.contains(token));
        assert!(summary.contains("[REDACTED] [REDACTED]"));
        assert!(summary.ends_with("line 18"));

        let long = format!("{} {secret} {token}", "界".repeat(600));
        let summary = runtime_failure_summary(std::slice::from_ref(&long));
        assert_eq!(summary.chars().count(), 500);
        let summary = runtime_failure_summary(&vec![long; 30]);
        assert!(summary.len() <= 4 * 1024);
        assert!(summary.lines().all(|line| line.chars().count() <= 500));

        let boundary = format!("{}{secret}", "x".repeat(499));
        assert_eq!(
            runtime_failure_summary(&[boundary]),
            format!("{}[", "x".repeat(499))
        );
    }

    /// Set VJ_REAL_HF_EXE to the absolute path of the pinned hf CLI with hf-xet.
    #[tokio::test]
    #[ignore = "requires real pinned hf with hf-xet, network and a 334,641,190-byte download"]
    async fn real_hf_download_succeeds_without_user_profile_variables() {
        use std::{fs, time::Duration};

        let executable = PathBuf::from(std::env::var_os("VJ_REAL_HF_EXE").unwrap());
        assert!(executable.is_absolute() && executable.is_file());
        let root = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
        let local_dir = root.path().join("files");
        let mut command =
            HfDownloadCommand::for_job(root.path(), &hf_job(), local_dir.clone(), &NoCredentials)
                .await
                .unwrap();
        command.spec.program = executable;
        assert!(
            !command
                .spec
                .env
                .iter()
                .any(|(key, _)| key == "HOMEDRIVE" || key == "HOMEPATH" || key == "HF_TOKEN")
        );
        let result = process::run(
            &command.spec,
            &DiscardSink,
            &CancelToken::new(),
            &RunLimits {
                timeout: Some(Duration::from_secs(600)),
                ..RunLimits::default()
            },
        )
        .await;
        assert!(result.is_ok(), "hf must download with the app-owned home");
        let mut file = fs::File::open(local_dir.join(FILE)).unwrap();
        assert_eq!(file.metadata().unwrap().len(), SIZE);
        assert_eq!(
            crate::shared::model_paths::digest(&mut file)
                .unwrap()
                .as_str(),
            SHA256
        );
    }
}
