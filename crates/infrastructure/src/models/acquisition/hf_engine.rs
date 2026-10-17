//! The only adapter allowed to construct an HF child containing HF_TOKEN.
use super::{
    bandwidth::BandwidthLimiter,
    finalize::StagingArea,
    hf_proxy::{HfProxyConfig, HfRelay, ProxyUnavailable},
};
use crate::{
    settings::secrets::CredentialVault,
    shared::process::{self, CommandSpec, DiscardSink, RunError, RunLimits},
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
        let temp = root.join("data/tmp/hf");
        std::fs::create_dir_all(&cache).map_err(|_| DownloadErrorCode::Disk)?;
        std::fs::create_dir_all(&temp).map_err(|_| DownloadErrorCode::Disk)?;
        env.extend([
            (OsString::from("HF_HOME"), cache.into_os_string()),
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
                        Err(RunError::Spawn(_)) => Err(DownloadErrorCode::ToolLaunch.into()),
                        Err(RunError::Cancelled { .. }) => Err(DownloadErrorCode::Cancelled.into()),
                        Err(RunError::TimedOut { .. }) => Err(DownloadErrorCode::Network.into()),
                        Err(RunError::CleanupFailed { .. }) => Err(DownloadErrorCode::ProcessCleanupFailed.into()),
                        Err(RunError::Failed { tail, .. }) => Err(classify_stderr(&tail.join("\n")).into()),
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
