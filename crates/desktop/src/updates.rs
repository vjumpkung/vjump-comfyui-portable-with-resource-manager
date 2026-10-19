//! The signed self-updater: complete-or-nothing build-time configuration and the adapter over
//! `tauri-plugin-updater`.
//!
//! The plugin is used from Rust only (no capability grants its commands to a webview) and is
//! registered only when the build carries a public key and an HTTPS endpoint. The adapter
//! retains the exact release the user reviewed behind an opaque id; installing consumes that id
//! and can never resolve, download or install any other release. Windows installers end the
//! process from inside `install()`, so the application's own background work is stopped before
//! `install()` is called.

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use application::{
    settings::updates::{AppUpdater, UpdateError, UpdateOffer, UpdateProgress, UpdateProgressSink},
    shared::IdGenerator,
};
use async_trait::async_trait;
use infrastructure::shared::{
    clock::RandomIds,
    logging::{self, LoggingGuard},
};
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_updater::UpdaterExt;

use crate::state::AppState;

/// At most one progress event per interval, plus the final one.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// The public key and HTTPS endpoint compiled into this build (`VJUMP_UPDATER_PUBKEY` and
/// `VJUMP_UPDATER_ENDPOINT`). Either one alone, or an invalid one, means "not configured".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdaterBuildConfig {
    pubkey: String,
    endpoint: tauri::Url,
}

impl UpdaterBuildConfig {
    pub fn from_build_env() -> Option<Self> {
        Self::parse(
            option_env!("VJUMP_UPDATER_PUBKEY"),
            option_env!("VJUMP_UPDATER_ENDPOINT"),
        )
    }

    /// The public key must be the base64 text of a minisign key; the endpoint must be an HTTPS
    /// URL with a host.
    pub fn parse(pubkey: Option<&str>, endpoint: Option<&str>) -> Option<Self> {
        let pubkey = pubkey?.trim();
        if pubkey.is_empty()
            || !pubkey
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'='))
        {
            return None;
        }
        let endpoint: tauri::Url = endpoint?.trim().parse().ok()?;
        if endpoint.scheme() != "https" || endpoint.host_str().is_none() {
            return None;
        }
        Some(Self {
            pubkey: pubkey.to_owned(),
            endpoint,
        })
    }

    /// The `plugins.updater` configuration the plugin reads when it is set up. Everything else
    /// keeps its default: HTTPS only, no downgrades, signatures always verified.
    pub fn plugin_config(&self) -> serde_json::Value {
        serde_json::json!({
            "pubkey": self.pubkey,
            "endpoints": [self.endpoint.as_str()],
        })
    }
}

/// Why a plugin step failed, as far as the adapter distinguishes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseFault {
    /// The package does not verify against the build's public key.
    Signature,
    Other,
}

/// What the user is shown about a release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseSummary {
    pub version: String,
    pub date: Option<String>,
    pub notes: Option<String>,
}

/// Looks for a newer release on the configured endpoint.
#[async_trait]
pub trait ReleaseChannel: Send + Sync {
    async fn check(&self) -> Result<Option<Box<dyn Release>>, ReleaseFault>;
}

/// One release found by a check, retained as it was found.
#[async_trait]
pub trait Release: Send + Sync {
    fn summary(&self) -> ReleaseSummary;

    /// Downloads the package and verifies its signature. `on_chunk` receives the size of each
    /// chunk and the announced total, when there is one.
    async fn download(
        &self,
        on_chunk: &mut (dyn FnMut(u64, Option<u64>) + Send),
    ) -> Result<Vec<u8>, ReleaseFault>;

    /// Installs a verified package. On Windows this ends the process after starting the
    /// installer, which then relaunches the application.
    fn install(&self, package: Vec<u8>) -> Result<(), ReleaseFault>;

    /// Restarts the application after an installation that returned (not on Windows). The real
    /// restart does not return, so the caller keeps the maintenance gate until the process ends.
    fn restart(&self);
}

/// Stops the application's own background work before an installer may replace it or end the
/// process.
#[async_trait]
pub trait Quiesce: Send + Sync {
    async fn stop_downloads(&self);
    async fn stop_library(&self);
    fn flush_logs(&self);
    async fn restore(&self);
}

struct NoQuiesce;

#[async_trait]
impl Quiesce for NoQuiesce {
    async fn stop_downloads(&self) {}
    async fn stop_library(&self) {}
    fn flush_logs(&self) {}
    async fn restore(&self) {}
}

pub struct SignedUpdater {
    channel: Option<Box<dyn ReleaseChannel>>,
    quiesce: Arc<dyn Quiesce>,
    ids: Arc<dyn IdGenerator>,
    /// The one release the user may install: the latest successful check.
    reviewed: Mutex<Option<(String, Box<dyn Release>)>>,
}

impl SignedUpdater {
    /// `make_channel` runs only for a complete configuration, so an unconfigured updater owns
    /// nothing that could reach the network.
    pub fn new(
        config: Option<UpdaterBuildConfig>,
        make_channel: impl FnOnce(UpdaterBuildConfig) -> Box<dyn ReleaseChannel>,
        quiesce: Arc<dyn Quiesce>,
        ids: Arc<dyn IdGenerator>,
    ) -> Self {
        Self {
            channel: config.map(make_channel),
            quiesce,
            ids,
            reviewed: Mutex::new(None),
        }
    }

    /// The updater of a build without configuration.
    pub fn disabled() -> Self {
        Self::new(
            None,
            |_| unreachable!("a disabled updater has no configuration"),
            Arc::new(NoQuiesce),
            Arc::new(RandomIds::default()),
        )
    }

    fn reviewed(&self) -> std::sync::MutexGuard<'_, Option<(String, Box<dyn Release>)>> {
        self.reviewed
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Takes the reviewed release if `update_id` names it. Any other id leaves it in place.
    fn take_reviewed(&self, update_id: &str) -> Result<Box<dyn Release>, UpdateError> {
        let mut reviewed = self.reviewed();
        match reviewed.take() {
            Some((id, release)) if id == update_id => Ok(release),
            other => {
                *reviewed = other;
                Err(UpdateError::Stale)
            }
        }
    }
}

fn install_error(fault: ReleaseFault) -> UpdateError {
    match fault {
        ReleaseFault::Signature => UpdateError::SignatureInvalid,
        ReleaseFault::Other => UpdateError::InstallFailed,
    }
}

#[async_trait]
impl AppUpdater for SignedUpdater {
    fn configured(&self) -> bool {
        self.channel.is_some()
    }

    async fn check(&self) -> Result<Option<UpdateOffer>, UpdateError> {
        let channel = self.channel.as_ref().ok_or(UpdateError::NotConfigured)?;
        let release = channel
            .check()
            .await
            .map_err(|_| UpdateError::CheckFailed)?;
        let Some(release) = release else {
            *self.reviewed() = None;
            return Ok(None);
        };
        let summary = release.summary();
        let update_id = self.ids.new_id();
        *self.reviewed() = Some((update_id.clone(), release));
        Ok(Some(UpdateOffer {
            update_id,
            version: summary.version,
            date: summary.date,
            notes: summary.notes,
        }))
    }

    async fn install(
        &self,
        update_id: &str,
        sink: Arc<dyn UpdateProgressSink>,
    ) -> Result<(), UpdateError> {
        if self.channel.is_none() {
            return Err(UpdateError::NotConfigured);
        }
        // The id is consumed: a failure needs a new check and a new review.
        let release = self.take_reviewed(update_id)?;

        let mut downloaded = 0u64;
        let mut last_report: Option<Instant> = None;
        let report = sink.clone();
        let package = release
            .download(&mut |chunk, total| {
                downloaded += chunk;
                let finished = total.is_some_and(|total| downloaded >= total);
                let now = Instant::now();
                if finished || last_report.is_none_or(|at| now - at >= PROGRESS_INTERVAL) {
                    last_report = Some(now);
                    report.report(UpdateProgress::Downloading { downloaded, total });
                }
            })
            .await
            .map_err(install_error)?;

        // Cleanup precedes the installer: on Windows `install` does not return on success.
        self.quiesce.stop_downloads().await;
        self.quiesce.stop_library().await;
        sink.report(UpdateProgress::Installing);
        self.quiesce.flush_logs();
        if let Err(error) = release.install(package) {
            self.quiesce.restore().await;
            return Err(install_error(error));
        }
        sink.report(UpdateProgress::Restarting);
        release.restart();
        Ok(())
    }
}

/// Classifies a plugin error: anything about the signature or the signed version is a
/// verification failure, everything else a plain failure.
fn fault(error: &tauri_plugin_updater::Error) -> ReleaseFault {
    use tauri_plugin_updater::Error;
    match error {
        Error::Minisign(_)
        | Error::Base64(_)
        | Error::SignatureUtf8(_)
        | Error::SignedVersionMismatch { .. }
        | Error::MissingSignedVersion => ReleaseFault::Signature,
        _ => ReleaseFault::Other,
    }
}

/// The plugin's endpoint check.
pub struct PluginChannel<R: Runtime>(AppHandle<R>);

impl<R: Runtime> PluginChannel<R> {
    /// The updater plugin must have been registered with a complete configuration.
    pub fn new(app: AppHandle<R>) -> Self {
        Self(app)
    }
}

#[async_trait]
impl<R: Runtime> ReleaseChannel for PluginChannel<R> {
    async fn check(&self) -> Result<Option<Box<dyn Release>>, ReleaseFault> {
        let updater = self.0.updater().map_err(|error| {
            tracing::warn!(%error, "the updater could not be prepared");
            fault(&error)
        })?;
        match updater.check().await {
            Ok(Some(update)) => Ok(Some(Box::new(PluginRelease {
                update,
                app: self.0.clone(),
            }))),
            Ok(None) => Ok(None),
            Err(error) => {
                tracing::warn!(%error, "the update check failed");
                Err(fault(&error))
            }
        }
    }
}

/// The exact `Update` the plugin returned.
struct PluginRelease<R: Runtime> {
    update: tauri_plugin_updater::Update,
    app: AppHandle<R>,
}

#[async_trait]
impl<R: Runtime> Release for PluginRelease<R> {
    fn summary(&self) -> ReleaseSummary {
        ReleaseSummary {
            version: self.update.version.clone(),
            date: self.update.date.map(|date| date.date().to_string()),
            notes: self.update.body.clone(),
        }
    }

    async fn download(
        &self,
        on_chunk: &mut (dyn FnMut(u64, Option<u64>) + Send),
    ) -> Result<Vec<u8>, ReleaseFault> {
        // The plugin verifies the package signature against the build's public key.
        self.update
            .download(|chunk, total| on_chunk(chunk as u64, total), || {})
            .await
            .map_err(|error| {
                tracing::warn!(%error, "the update could not be downloaded and verified");
                fault(&error)
            })
    }

    fn install(&self, package: Vec<u8>) -> Result<(), ReleaseFault> {
        self.update.install(package).map_err(|error| {
            // The tracing worker was flushed and stopped immediately before install().
            let state = self.app.state::<AppState>();
            let logs_dir = state
                .services()
                .ok()
                .map(|services| services.logs_dir.as_path());
            log_install_failure(logs_dir, &error.to_string());
            fault(&error)
        })
    }

    fn restart(&self) {
        self.app.restart();
    }
}

fn log_install_failure(logs_dir: Option<&std::path::Path>, error: &str) {
    use std::io::Write;

    let message = format!("ERROR the update could not be installed: {error}");
    let message = logging::redact(&message);
    if let Some(logs_dir) = logs_dir {
        let written = (|| -> std::io::Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(logs_dir.join("app.log"))?;
            writeln!(file, "{message}")?;
            file.flush()
        })();
        if written.is_ok() {
            return;
        }
    }
    eprintln!("{message}");
}

/// Stops the download and library writers and drains the app log before installation.
pub struct AppQuiesce<R: Runtime> {
    app: AppHandle<R>,
    logging_guard: Arc<Mutex<Option<LoggingGuard>>>,
}

impl<R: Runtime> AppQuiesce<R> {
    pub fn new(app: AppHandle<R>, logging_guard: Arc<Mutex<Option<LoggingGuard>>>) -> Self {
        Self { app, logging_guard }
    }
}

#[async_trait]
impl<R: Runtime> Quiesce for AppQuiesce<R> {
    async fn stop_downloads(&self) {
        self.app.state::<AppState>().shutdown_downloads().await;
    }

    async fn stop_library(&self) {
        self.app.state::<AppState>().shutdown_library().await;
    }

    fn flush_logs(&self) {
        let guard = self
            .logging_guard
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        drop(guard);
    }

    async fn restore(&self) {
        self.app.state::<AppState>().restore_library();
    }
}

#[cfg(test)]
mod tests;
