//! Complete diagnostics for support (REQUIREMENTS 6.7 "Copy diagnostics").
//!
//! The snapshot reuses the existing runtime and environment queries. It never runs setup,
//! never imports custom node code and never fails as a whole: a section that cannot be read is
//! left out and named in the availability warnings. Redaction of the rendered report belongs to
//! the composition root, which sees the finished text.

use std::{fmt::Write, sync::Arc};

use async_trait::async_trait;
use domain::environment::GpuProbe;
use thiserror::Error;

use crate::{
    comfy_runtime::{GetComfyStatus, RuntimeDeps, RuntimeLogReader, RuntimeStatus},
    environment::{
        AcceleratorResult, ComponentStatus, DetectHardware, EnvironmentAppError, EnvironmentInfo,
        GetEnvironmentInfo, InstalledComponents, ManifestState,
    },
};

/// How many log lines each log section of the report carries.
pub const LOG_TAIL_LINES: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DiagnosticsError {
    #[error("the application log is unavailable")]
    Unavailable,
}

/// The tail of the application's own log file.
#[async_trait]
pub trait AppLogTail: Send + Sync {
    /// At most `limit` of the newest lines, oldest first.
    async fn last_lines(&self, limit: usize) -> Result<Vec<String>, DiagnosticsError>;
}

/// A tool whose version is pinned by the application rather than observed on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedTool {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentSection {
    pub id: String,
    pub ownership: String,
    pub state: String,
    pub python: String,
    pub python_source: String,
    pub profile_id: String,
    pub profile_revision: u32,
    pub profile_label: Option<String>,
    pub cuda_variant: Option<String>,
    pub torch: Option<String>,
    pub triton: Option<String>,
    pub protected_pins: Vec<(String, String)>,
    /// The stored outcome of every accelerator component, one line each; empty when the profile
    /// has none or none were recorded.
    pub accelerators: Vec<String>,
}

impl From<&EnvironmentInfo> for EnvironmentSection {
    fn from(info: &EnvironmentInfo) -> Self {
        let env = &info.env;
        Self {
            id: env.id.to_string(),
            ownership: env.ownership.as_str().to_owned(),
            state: env.state.as_str().to_owned(),
            python: env.python_version.to_string(),
            python_source: env.source.as_str().to_owned(),
            profile_id: env.profile_id.clone(),
            profile_revision: env.profile_revision,
            profile_label: info.profile_label.clone(),
            cuda_variant: info.cuda_variant.clone(),
            torch: info.torch_version.clone(),
            triton: info.triton_version.clone(),
            protected_pins: env
                .pins
                .iter()
                .map(|(name, version)| (name.to_string(), version.to_owned()))
                .collect(),
            accelerators: info
                .components
                .accelerators
                .iter()
                .map(describe_accelerator)
                .collect(),
        }
    }
}

/// Installation and import outcomes stay separate; an untested import is not a success.
fn describe_accelerator(result: &AcceleratorResult) -> String {
    let mut text = format!(
        "{}: {}, {}, import {}",
        result.id,
        if result.selected {
            "selected"
        } else {
            "not selected"
        },
        match result.status {
            ComponentStatus::Installed => "installed",
            ComponentStatus::Failed => "failed",
            ComponentStatus::Skipped => "skipped",
        },
        match result.importable {
            Some(true) => "ok",
            Some(false) => "failed",
            None => "untested",
        }
    );
    if let Some(message) = &result.message {
        let _ = write!(text, " - {message}");
    }
    text
}

/// The profile recorded in `install.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestSection {
    pub profile_id: String,
    pub profile_revision: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ComfySection {
    /// Commit, tag and branch of the checkout, when it can be read.
    pub revision: Option<String>,
    pub status: Option<String>,
}

/// Everything the report states, as plain data. Sections that could not be read are `None` or
/// empty and are named in `warnings`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiagnosticsSnapshot {
    pub app_version: String,
    pub os: String,
    pub gpus: Vec<String>,
    pub driver_version: Option<String>,
    pub driver_cuda: Option<String>,
    pub environment: Option<EnvironmentSection>,
    pub manifest: Option<ManifestSection>,
    pub comfyui: ComfySection,
    pub tools: Vec<PinnedTool>,
    pub app_log: Vec<String>,
    pub runtime_log: Vec<String>,
    pub warnings: Vec<String>,
}

pub struct CollectDiagnostics {
    deps: RuntimeDeps,
    app_log: Arc<dyn AppLogTail>,
    runtime_log: Arc<dyn RuntimeLogReader>,
    app_version: String,
    tools: Vec<PinnedTool>,
}

impl CollectDiagnostics {
    pub fn new(
        deps: RuntimeDeps,
        app_log: Arc<dyn AppLogTail>,
        runtime_log: Arc<dyn RuntimeLogReader>,
        app_version: impl Into<String>,
        tools: Vec<PinnedTool>,
    ) -> Self {
        Self {
            deps,
            app_log,
            runtime_log,
            app_version: app_version.into(),
            tools,
        }
    }

    pub async fn execute(&self) -> DiagnosticsSnapshot {
        let environment = &self.deps.environment;
        let root = environment.root.current().root;
        let mut snapshot = DiagnosticsSnapshot {
            app_version: self.app_version.clone(),
            tools: self.tools.clone(),
            ..DiagnosticsSnapshot::default()
        };

        let info = GetEnvironmentInfo::new(environment.clone()).execute().await;
        let env_path = info.as_ref().ok().map(|info| info.env.path.clone());
        match info {
            Ok(info) => {
                if info.components == InstalledComponents::default() {
                    snapshot
                        .warnings
                        .push("Accelerator outcomes are not recorded for this environment.".into());
                }
                snapshot.environment = Some(EnvironmentSection::from(&info));
            }
            Err(EnvironmentAppError::NoActiveEnvironment) => snapshot.warnings.push(
                "No environment is active yet (first run): environment details are not available."
                    .into(),
            ),
            Err(_) => snapshot
                .warnings
                .push("The active environment could not be read.".into()),
        }

        let hardware = DetectHardware::new(environment.clone())
            .execute(&root, env_path.as_deref())
            .await;
        snapshot.os = hardware.os_version;
        snapshot.gpus = describe_gpus(&hardware.gpus);
        snapshot.driver_version = hardware.driver_version;
        snapshot.driver_cuda = hardware.driver_cuda.map(|cuda| cuda.to_string());

        match environment.manifests.read(&root).await {
            Ok(ManifestState::Valid(manifest)) => {
                snapshot.manifest = Some(ManifestSection {
                    profile_id: manifest.profile_id,
                    profile_revision: manifest.profile_revision,
                });
            }
            Ok(ManifestState::Absent) => snapshot.warnings.push(
                "install.json does not exist yet: the manifest profile is not available.".into(),
            ),
            _ => snapshot.warnings.push(
                "install.json could not be read: the manifest profile is not available.".into(),
            ),
        }

        match self.deps.repository.revision(&root).await {
            Ok(revision) => {
                let mut text = revision.commit;
                if let Some(tag) = revision.tag {
                    let _ = write!(text, " (tag {tag})");
                }
                if let Some(branch) = revision.branch {
                    let _ = write!(text, " (branch {branch})");
                }
                snapshot.comfyui.revision = Some(text);
            }
            Err(_) => snapshot.warnings.push(
                "The ComfyUI checkout could not be read: its revision is not available.".into(),
            ),
        }
        match GetComfyStatus::new(self.deps.clone()).execute().await {
            Ok(status) => snapshot.comfyui.status = Some(describe_status(&status)),
            Err(_) => snapshot
                .warnings
                .push("The ComfyUI runtime status could not be read.".into()),
        }

        match self.app_log.last_lines(LOG_TAIL_LINES).await {
            Ok(lines) => snapshot.app_log = newest(lines),
            Err(_) => snapshot
                .warnings
                .push("The application log could not be read.".into()),
        }
        snapshot.runtime_log = newest(self.runtime_log.last_lines(LOG_TAIL_LINES));
        snapshot
    }
}

/// The last [`LOG_TAIL_LINES`] lines, whatever the port returned.
fn newest(mut lines: Vec<String>) -> Vec<String> {
    let excess = lines.len().saturating_sub(LOG_TAIL_LINES);
    lines.drain(..excess);
    lines
}

fn describe_gpus(probe: &GpuProbe) -> Vec<String> {
    match probe {
        GpuProbe::Detected(gpus) => gpus
            .iter()
            .map(|gpu| {
                format!(
                    "{} ({} MiB)",
                    gpu.name,
                    gpu.vram_mib
                        .map_or_else(|| "unknown".into(), |mib| mib.to_string())
                )
            })
            .collect(),
        GpuProbe::NoNvidiaGpu => vec!["No NVIDIA GPU detected".into()],
        GpuProbe::Unavailable => vec!["GPU observation unavailable".into()],
    }
}

fn describe_status(status: &RuntimeStatus) -> String {
    let mut text = format!("{:?}", status.lifecycle).to_lowercase();
    if let Some(failure) = &status.failure {
        let _ = write!(text, "; last failure: {:?}", failure.kind);
        if let Some(code) = failure.exit_code {
            let _ = write!(text, " (exit code {code})");
        }
        let _ = write!(text, ": {}", failure.message);
    }
    if let Some(operation) = &status.pending_operation {
        let _ = write!(text, "; pending operation: {operation}");
    }
    if let Some(recovery) = &status.recovery {
        let _ = write!(text, "; recovery: {recovery}");
    }
    text
}

impl DiagnosticsSnapshot {
    /// The plain-text report. Redaction is applied by the caller to this finished text.
    pub fn render(&self) -> String {
        const UNAVAILABLE: &str = "unavailable";
        let mut out = String::new();
        let _ = writeln!(out, "vjumpkung ComfyUI Desktop diagnostics");
        let _ = writeln!(out, "App version: {}", self.app_version);
        let _ = writeln!(out, "OS: {}", self.os);
        for gpu in &self.gpus {
            let _ = writeln!(out, "GPU: {gpu}");
        }
        let _ = writeln!(
            out,
            "NVIDIA driver: {}",
            self.driver_version.as_deref().unwrap_or(UNAVAILABLE)
        );
        let _ = writeln!(
            out,
            "Driver CUDA: {}",
            self.driver_cuda.as_deref().unwrap_or(UNAVAILABLE)
        );

        let _ = writeln!(out, "\n== Environment ==");
        match &self.environment {
            Some(env) => {
                let _ = writeln!(out, "ID: {}", env.id);
                let _ = writeln!(out, "Ownership: {}", env.ownership);
                let _ = writeln!(out, "State: {}", env.state);
                let _ = writeln!(out, "Python: {} ({})", env.python, env.python_source);
                let _ = write!(
                    out,
                    "Profile: {} revision {}",
                    env.profile_id, env.profile_revision
                );
                if let Some(label) = &env.profile_label {
                    let _ = write!(out, " ({label})");
                }
                let _ = writeln!(out);
                for (label, value) in [
                    ("CUDA variant", &env.cuda_variant),
                    ("PyTorch", &env.torch),
                    ("Triton", &env.triton),
                ] {
                    let _ = writeln!(out, "{label}: {}", value.as_deref().unwrap_or(UNAVAILABLE));
                }
                let _ = writeln!(out, "Protected packages:");
                if env.protected_pins.is_empty() {
                    let _ = writeln!(out, "  none");
                }
                for (name, version) in &env.protected_pins {
                    let _ = writeln!(out, "  {name}=={version}");
                }
                let _ = writeln!(out, "Accelerators:");
                if env.accelerators.is_empty() {
                    let _ = writeln!(out, "  none recorded");
                }
                for accelerator in &env.accelerators {
                    let _ = writeln!(out, "  {accelerator}");
                }
            }
            None => {
                let _ = writeln!(out, "{UNAVAILABLE}");
            }
        }

        let _ = writeln!(out, "\n== Install manifest ==");
        match &self.manifest {
            Some(manifest) => {
                let _ = writeln!(
                    out,
                    "Profile: {} revision {}",
                    manifest.profile_id, manifest.profile_revision
                );
            }
            None => {
                let _ = writeln!(out, "{UNAVAILABLE}");
            }
        }

        let _ = writeln!(out, "\n== ComfyUI ==");
        let _ = writeln!(
            out,
            "Revision: {}",
            self.comfyui.revision.as_deref().unwrap_or(UNAVAILABLE)
        );
        let _ = writeln!(
            out,
            "Status: {}",
            self.comfyui.status.as_deref().unwrap_or(UNAVAILABLE)
        );

        let _ = writeln!(out, "\n== Tool versions ==");
        for tool in &self.tools {
            let _ = writeln!(
                out,
                "{} {} (pinned by this app, not probed)",
                tool.name, tool.version
            );
        }

        let _ = writeln!(out, "\n== Availability warnings ==");
        if self.warnings.is_empty() {
            let _ = writeln!(out, "none");
        }
        for warning in &self.warnings {
            let _ = writeln!(out, "- {warning}");
        }

        for (title, lines) in [
            ("Application log", &self.app_log),
            ("ComfyUI runtime log", &self.runtime_log),
        ] {
            let _ = writeln!(out, "\n== {title} (last {LOG_TAIL_LINES} lines) ==");
            if lines.is_empty() {
                let _ = writeln!(out, "(no lines)");
            }
            for line in lines {
                let _ = writeln!(out, "{line}");
            }
        }
        out
    }
}

#[cfg(test)]
mod tests;
