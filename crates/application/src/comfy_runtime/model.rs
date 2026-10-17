use domain::{
    comfy_runtime::*,
    environment::{EnvId, PinSet},
};
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionId(String);
impl SessionId {
    pub fn new(value: impl Into<String>) -> Result<Self, RuntimeError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 64
            || !value
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(RuntimeError::InvalidRecord);
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessIdentity {
    pub pid: u32,
    /// OS creation identity, never just a recycled PID.
    pub created: String,
    pub scope: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeSession {
    pub id: SessionId,
    pub env_id: EnvId,
    pub interpreter: PathBuf,
    pub checkout: PathBuf,
    pub revision: Revision,
    pub options: LaunchOptions,
    pub started_at: String,
    pub identity: Option<ProcessIdentity>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeStatus {
    pub lifecycle: Lifecycle,
    pub session: Option<RuntimeSession>,
    pub failure: Option<RuntimeFailure>,
    pub pending_operation: Option<String>,
    pub recovery: Option<String>,
}
impl RuntimeStatus {
    pub fn origin(&self) -> Option<String> {
        (self.lifecycle == Lifecycle::Running)
            .then(|| {
                self.session
                    .as_ref()
                    .map(|s| format!("http://127.0.0.1:{}", s.options.port.get()))
            })
            .flatten()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InventoryState {
    Stopped,
    OwnedRunning,
    VerifiedSurvivor(ProcessIdentity),
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRoot {
    pub section: String,
    pub category: String,
    pub path: PathBuf,
    pub is_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeEvent {
    Snapshot {
        cursor: u64,
        status: Box<RuntimeStatus>,
        logs: Vec<LogChunk>,
        gap: bool,
    },
    State {
        cursor: u64,
        status: Box<RuntimeStatus>,
    },
    Log(LogChunk),
    Gap {
        cursor: u64,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogChunk {
    pub cursor: u64,
    pub session: SessionId,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdatePhase {
    Intent,
    SourceChanged,
    Dependencies,
    Verifying,
    Publishing,
    Complete,
}
impl UpdatePhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Intent => "intent",
            Self::SourceChanged => "source_changed",
            Self::Dependencies => "dependencies",
            Self::Verifying => "verifying",
            Self::Publishing => "publishing",
            Self::Complete => "complete",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrozenUpdate {
    pub commit: String,
    pub tag: Option<String>,
    pub fast_forward: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateRun {
    pub id: SessionId,
    pub env_id: EnvId,
    pub old_commit: String,
    pub target: FrozenUpdate,
    pub pins: PinSet,
    pub phase: UpdatePhase,
    pub failure: Option<String>,
    pub created_at: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionList {
    pub current: Revision,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeDiagnostics {
    pub os: String,
    pub gpus: Vec<String>,
    pub driver_version: Option<String>,
    pub env_id: Option<EnvId>,
    pub python_version: Option<String>,
    pub protected_packages: Vec<(String, String)>,
    pub revision: Revision,
    pub status: RuntimeStatus,
    pub log_tail: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RuntimeError {
    #[error("another maintenance operation is running")]
    Busy,
    #[error("ComfyUI must be stopped before continuing")]
    NeedsStop,
    #[error("interrupted custom-node work must be resolved before launch")]
    NodeRecoveryRequired,
    #[error("ComfyUI stopped, but custom nodes could not be rescanned; retry Rescan")]
    NodeRescanFailed,
    #[error("a verified active environment is required; repair the selected environment first")]
    EnvironmentNotUsable,
    #[error("installation recovery is required: {0}")]
    RecoveryRequired(String),
    #[error("port {requested:?} is occupied")]
    PortConflict {
        requested: Port,
        suggested: Option<Port>,
    },
    #[error(transparent)]
    Launch(#[from] LaunchError),
    #[error("update refused: {reason}")]
    UpdateRefused {
        reason: UpdateRefusal,
        status: String,
    },
    #[error("runtime storage is unavailable")]
    Storage,
    #[error("invalid saved runtime configuration; recover the stored record before continuing")]
    InvalidRecord,
    #[error("the operation was cancelled")]
    Cancelled,
    #[error("{0}")]
    Failed(String),
}
impl From<crate::environment::ProvisionError> for RuntimeError {
    fn from(error: crate::environment::ProvisionError) -> Self {
        use crate::environment::ProvisionError;
        match error {
            ProvisionError::Cancelled => Self::Cancelled,
            ProvisionError::Failed(s) | ProvisionError::Retryable(s) => Self::Failed(s),
            ProvisionError::CommandFailed(_) => {
                Self::Failed("a runtime maintenance command failed; inspect diagnostics".into())
            }
            ProvisionError::ConsentRequired(_) => Self::RecoveryRequired(
                "dependency work requires the environment workflow's explicit source-build consent"
                    .into(),
            ),
        }
    }
}
