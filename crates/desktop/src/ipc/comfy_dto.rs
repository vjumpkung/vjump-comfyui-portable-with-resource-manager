//! IPC DTOs of the ComfyUI runtime context. Cursors are `f64` because JavaScript numbers are
//! IEEE doubles and the generated bindings must not use `bigint`.

use application::comfy_runtime::{
    LogChunk, ModelRoot, RuntimeDiagnostics, RuntimeEvent, RuntimeSession, RuntimeStatus,
    UpdateRun, VersionList,
};
use domain::comfy_runtime::{
    Attention, FailureKind, LaunchOptions, Lifecycle, Port, PreviewMethod, Revision, Tag,
    UpdateTarget, VramMode,
};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::error::IpcError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleDto {
    Stopped,
    Starting,
    Running,
    Stopping,
    Crashed,
}

impl From<Lifecycle> for LifecycleDto {
    fn from(value: Lifecycle) -> Self {
        match value {
            Lifecycle::Stopped => Self::Stopped,
            Lifecycle::Starting => Self::Starting,
            Lifecycle::Running => Self::Running,
            Lifecycle::Stopping => Self::Stopping,
            Lifecycle::Crashed => Self::Crashed,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeFailureKindDto {
    Spawn,
    ExitBeforeReady,
    ReadinessTimeout,
    UnexpectedExit,
    Termination,
    Cancelled,
    Persistence,
}

impl From<FailureKind> for RuntimeFailureKindDto {
    fn from(value: FailureKind) -> Self {
        match value {
            FailureKind::Spawn => Self::Spawn,
            FailureKind::ExitBeforeReady => Self::ExitBeforeReady,
            FailureKind::ReadinessTimeout => Self::ReadinessTimeout,
            FailureKind::UnexpectedExit => Self::UnexpectedExit,
            FailureKind::Termination => Self::Termination,
            FailureKind::Cancelled => Self::Cancelled,
            FailureKind::Persistence => Self::Persistence,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeFailureDto {
    pub kind: RuntimeFailureKindDto,
    pub exit_code: Option<i32>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RevisionDto {
    pub commit: String,
    pub tag: Option<String>,
    pub branch: Option<String>,
}

impl From<&Revision> for RevisionDto {
    fn from(value: &Revision) -> Self {
        Self {
            commit: value.commit.clone(),
            tag: value.tag.clone(),
            branch: value.branch.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum VramModeDto {
    Auto,
    Low,
    High,
    NoVram,
}

impl From<VramMode> for VramModeDto {
    fn from(value: VramMode) -> Self {
        match value {
            VramMode::Auto => Self::Auto,
            VramMode::Low => Self::Low,
            VramMode::High => Self::High,
            VramMode::NoVram => Self::NoVram,
        }
    }
}

impl From<VramModeDto> for VramMode {
    fn from(value: VramModeDto) -> Self {
        match value {
            VramModeDto::Auto => Self::Auto,
            VramModeDto::Low => Self::Low,
            VramModeDto::High => Self::High,
            VramModeDto::NoVram => Self::NoVram,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AttentionDto {
    Default,
    Sage,
    Flash,
    Pytorch,
    Split,
    Quad,
}

impl From<Attention> for AttentionDto {
    fn from(value: Attention) -> Self {
        match value {
            Attention::Default => Self::Default,
            Attention::Sage => Self::Sage,
            Attention::Flash => Self::Flash,
            Attention::Pytorch => Self::Pytorch,
            Attention::Split => Self::Split,
            Attention::Quad => Self::Quad,
        }
    }
}

impl From<AttentionDto> for Attention {
    fn from(value: AttentionDto) -> Self {
        match value {
            AttentionDto::Default => Self::Default,
            AttentionDto::Sage => Self::Sage,
            AttentionDto::Flash => Self::Flash,
            AttentionDto::Pytorch => Self::Pytorch,
            AttentionDto::Split => Self::Split,
            AttentionDto::Quad => Self::Quad,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PreviewMethodDto {
    Auto,
    Latent2Rgb,
    Taesd,
    None,
}

impl From<PreviewMethod> for PreviewMethodDto {
    fn from(value: PreviewMethod) -> Self {
        match value {
            PreviewMethod::Auto => Self::Auto,
            PreviewMethod::Latent2Rgb => Self::Latent2Rgb,
            PreviewMethod::Taesd => Self::Taesd,
            PreviewMethod::None => Self::None,
        }
    }
}

impl From<PreviewMethodDto> for PreviewMethod {
    fn from(value: PreviewMethodDto) -> Self {
        match value {
            PreviewMethodDto::Auto => Self::Auto,
            PreviewMethodDto::Latent2Rgb => Self::Latent2Rgb,
            PreviewMethodDto::Taesd => Self::Taesd,
            PreviewMethodDto::None => Self::None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LaunchOptionsDto {
    pub port: u16,
    pub vram: VramModeDto,
    pub attention: AttentionDto,
    pub preview: PreviewMethodDto,
    pub enable_manager: bool,
    pub listen_lan: bool,
    pub extra_args: Vec<String>,
}

impl From<&LaunchOptions> for LaunchOptionsDto {
    fn from(value: &LaunchOptions) -> Self {
        Self {
            port: value.port.get(),
            vram: value.vram.into(),
            attention: value.attention.into(),
            preview: value.preview.into(),
            enable_manager: value.enable_manager,
            listen_lan: value.listen_lan,
            extra_args: value.extra_args.clone(),
        }
    }
}

impl LaunchOptionsDto {
    pub fn into_domain(self) -> Result<LaunchOptions, IpcError> {
        let port = Port::new(self.port)
            .map_err(|error| IpcError::new("validation", &error.to_string()))?;
        let options = LaunchOptions {
            port,
            vram: self.vram.into(),
            attention: self.attention.into(),
            preview: self.preview.into(),
            enable_manager: self.enable_manager,
            listen_lan: self.listen_lan,
            extra_args: self.extra_args,
        };
        options
            .validate()
            .map_err(|error| IpcError::new("validation", &error.to_string()))?;
        Ok(options)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionDto {
    pub id: String,
    pub env_id: String,
    pub revision: RevisionDto,
    /// The options this session actually runs with, not the ones saved for the next launch.
    pub options: LaunchOptionsDto,
    pub started_at: String,
    pub pid: Option<u32>,
}

impl From<&RuntimeSession> for SessionDto {
    fn from(value: &RuntimeSession) -> Self {
        Self {
            id: value.id.as_str().to_owned(),
            env_id: value.env_id.to_string(),
            revision: (&value.revision).into(),
            options: (&value.options).into(),
            started_at: value.started_at.clone(),
            pid: value.identity.as_ref().map(|identity| identity.pid),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ComfyStatusDto {
    pub lifecycle: LifecycleDto,
    pub session: Option<SessionDto>,
    pub failure: Option<RuntimeFailureDto>,
    pub pending_operation: Option<String>,
    pub recovery: Option<String>,
    /// The loopback origin of the running session; only set while it is running.
    pub origin: Option<String>,
    pub pending_close_request: Option<String>,
}

impl ComfyStatusDto {
    pub fn new(status: &RuntimeStatus, pending_close_request: Option<String>) -> Self {
        Self {
            lifecycle: status.lifecycle.into(),
            session: status.session.as_ref().map(SessionDto::from),
            failure: status.failure.as_ref().map(|failure| RuntimeFailureDto {
                kind: failure.kind.into(),
                exit_code: failure.exit_code,
                message: failure.message.clone(),
            }),
            pending_operation: status.pending_operation.clone(),
            recovery: status.recovery.clone(),
            origin: status.origin(),
            pending_close_request,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum StartResultDto {
    Started {
        status: ComfyStatusDto,
    },
    /// The requested port is occupied. Nothing was started; the suggestion needs acceptance.
    PortConflict {
        requested: u16,
        suggested: Option<u16>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LogChunkDto {
    pub cursor: f64,
    pub session_id: String,
    pub text: String,
}

impl From<&LogChunk> for LogChunkDto {
    fn from(value: &LogChunk) -> Self {
        Self {
            cursor: value.cursor as f64,
            session_id: value.session.as_str().to_owned(),
            text: value.text.clone(),
        }
    }
}

/// Events of one subscription. The first event is always an authoritative snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ComfyEventDto {
    #[serde(rename_all = "camelCase")]
    Snapshot {
        cursor: f64,
        status: ComfyStatusDto,
        logs: Vec<LogChunkDto>,
        gap: bool,
    },
    #[serde(rename = "comfy:state", rename_all = "camelCase")]
    State { cursor: f64, status: ComfyStatusDto },
    #[serde(rename = "comfy:log", rename_all = "camelCase")]
    Log { chunk: LogChunkDto },
    /// The subscriber fell behind; subscribe again for a fresh snapshot.
    #[serde(rename_all = "camelCase")]
    Gap { cursor: f64 },
    /// A manager close request awaits the answer of the user (or was resolved when `None`).
    #[serde(rename_all = "camelCase")]
    CloseRequested { request_id: Option<String> },
}

impl ComfyEventDto {
    pub fn from_event(event: &RuntimeEvent, pending_close_request: Option<String>) -> Self {
        match event {
            RuntimeEvent::Snapshot {
                cursor,
                status,
                logs,
                gap,
            } => Self::Snapshot {
                cursor: *cursor as f64,
                status: ComfyStatusDto::new(status, pending_close_request),
                logs: logs.iter().map(LogChunkDto::from).collect(),
                gap: *gap,
            },
            RuntimeEvent::State { cursor, status } => Self::State {
                cursor: *cursor as f64,
                status: ComfyStatusDto::new(status, pending_close_request),
            },
            RuntimeEvent::Log(chunk) => Self::Log {
                chunk: chunk.into(),
            },
            RuntimeEvent::Gap { cursor } => Self::Gap {
                cursor: *cursor as f64,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UpdateTargetDto {
    LatestRelease,
    Tag { name: String },
    BranchFfOnly,
}

impl UpdateTargetDto {
    pub fn into_domain(self) -> Result<UpdateTarget, IpcError> {
        Ok(match self {
            Self::LatestRelease => UpdateTarget::LatestRelease,
            Self::BranchFfOnly => UpdateTarget::BranchFfOnly,
            Self::Tag { name } => UpdateTarget::Tag(
                Tag::new(name).map_err(|error| IpcError::new("validation", &error.to_string()))?,
            ),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInput {
    pub target: UpdateTargetDto,
    /// Explicit authorization to stop a running ComfyUI first.
    pub stop_running: bool,
    /// Retries the pending interrupted update with this id instead of starting a new one.
    pub retry: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRunDto {
    pub id: String,
    pub old_commit: String,
    pub target_commit: String,
    pub target_tag: Option<String>,
    pub phase: String,
    pub failure: Option<String>,
}

impl From<&UpdateRun> for UpdateRunDto {
    fn from(value: &UpdateRun) -> Self {
        Self {
            id: value.id.as_str().to_owned(),
            old_commit: value.old_commit.clone(),
            target_commit: value.target.commit.clone(),
            target_tag: value.target.tag.clone(),
            phase: value.phase.as_str().to_owned(),
            failure: value.failure.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UpdateResultDto {
    Updated {
        run: UpdateRunDto,
    },
    /// ComfyUI is running; resubmit with explicit authorization to stop it.
    NeedsStop,
    /// Dirty, diverged or otherwise unsafe: nothing was changed. `status` is `git status`.
    Refused {
        reason: String,
        status: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct VersionListDto {
    pub current: RevisionDto,
    pub tags: Vec<String>,
    /// An interrupted update that must be retried or repaired before ComfyUI can start.
    pub pending_update: Option<UpdateRunDto>,
}

impl VersionListDto {
    pub fn new(value: &VersionList, pending_update: Option<&UpdateRun>) -> Self {
        Self {
            current: (&value.current).into(),
            tags: value.tags.clone(),
            pending_update: pending_update.map(UpdateRunDto::from),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelRootDto {
    pub section: String,
    pub category: String,
    pub path: String,
    pub is_default: bool,
}

impl From<&ModelRoot> for ModelRootDto {
    fn from(value: &ModelRoot) -> Self {
        Self {
            section: value.section.clone(),
            category: value.category.clone(),
            path: value.path.display().to_string(),
            is_default: value.is_default,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PackagePinDto {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsDto {
    pub os: String,
    pub gpus: Vec<String>,
    pub driver_version: Option<String>,
    pub env_id: Option<String>,
    pub python_version: Option<String>,
    pub protected_packages: Vec<PackagePinDto>,
    pub revision: RevisionDto,
    pub status: ComfyStatusDto,
    pub log_tail: Vec<String>,
}

impl DiagnosticsDto {
    pub fn new(value: &RuntimeDiagnostics, pending_close_request: Option<String>) -> Self {
        Self {
            os: value.os.clone(),
            gpus: value.gpus.clone(),
            driver_version: value.driver_version.clone(),
            env_id: value.env_id.as_ref().map(ToString::to_string),
            python_version: value.python_version.clone(),
            protected_packages: value
                .protected_packages
                .iter()
                .map(|(name, version)| PackagePinDto {
                    name: name.clone(),
                    version: version.clone(),
                })
                .collect(),
            revision: (&value.revision).into(),
            status: ComfyStatusDto::new(&value.status, pending_close_request),
            log_tail: value.log_tail.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CloseChoiceDto {
    StopAndExit,
    KeepRunning,
    Cancel,
}

impl From<CloseChoiceDto> for application::comfy_runtime::CloseChoice {
    fn from(value: CloseChoiceDto) -> Self {
        match value {
            CloseChoiceDto::StopAndExit => Self::StopAndExit,
            CloseChoiceDto::KeepRunning => Self::KeepRunning,
            CloseChoiceDto::Cancel => Self::Cancel,
        }
    }
}
