use super::ports::*;
use crate::{
    environment::{EnvironmentDeps, PackagePlan},
    shared::CancelToken,
};
use domain::{
    custom_nodes::*,
    environment::{EnvId, PinChange, PinSet},
    shared::model_values::SafeFileName,
};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NodeAppError {
    #[error("another maintenance operation is running")]
    Busy,
    #[error("ComfyUI must be stopped before node maintenance")]
    RuntimeRunning,
    #[error("a verified environment is required")]
    EnvironmentUnavailable,
    #[error("node recovery must finish before this operation")]
    RecoveryRequired,
    #[error("the prepared plan is missing or stale; prepare again")]
    StalePlan,
    #[error("explicit consent does not cover this plan")]
    ConsentRequired,
    #[error("the operation was cancelled")]
    Cancelled,
    #[error("protected packages drifted")]
    PinDrift,
    #[error("node dependency mutation failed; environment repair is required")]
    DependenciesFailed,
    #[error("node not found")]
    NotFound,
    #[error("the destination already exists")]
    TargetExists,
    #[error("unsafe node path, archive or source")]
    UnsafeSource,
    #[error("node repository has local changes, conflicts, divergence or no selected upstream")]
    GitRefused,
    #[error("registry request failed with HTTP {0}")]
    RegistryStatus(u16),
    #[error("registry is unavailable")]
    Offline,
    #[error("invalid registry response")]
    InvalidRegistry,
    #[error("node storage is unavailable")]
    Storage,
    #[error("node filesystem operation failed")]
    Filesystem,
    #[error("dependency resolution failed before package mutation")]
    ResolutionFailed,
    #[error("the node is already at the selected version")]
    UpToDate,
    #[error("the request is not valid")]
    InvalidRequest,
    #[error("the reference names both a branch and a tag")]
    AmbiguousReference,
    #[error("{0}")]
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryNode {
    pub id: String,
    pub name: String,
    pub publisher: Option<String>,
    pub description: Option<String>,
    pub downloads: Option<u64>,
    pub github_stars: Option<u64>,
    pub latest_version: Option<String>,
    pub repository: Option<String>,
    pub compatibility: CompatibilityMetadata,
    pub evaluated: Compatibility,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryPage {
    pub nodes: Vec<RegistryNode>,
    pub page: u32,
    pub limit: u32,
    pub total: u64,
    pub total_pages: u32,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryQuery {
    pub search: String,
    pub page: u32,
    pub limit: u32,
}
impl Default for RegistryQuery {
    fn default() -> Self {
        Self {
            search: String::new(),
            page: 1,
            limit: 50,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedPage {
    pub page: RegistryPage,
    pub fetched_at: String,
    pub stale: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryRelease {
    pub node_id: String,
    pub version: String,
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrepareRequest {
    Registry {
        id: String,
        version: Option<String>,
    },
    Git {
        repository: String,
        reference: Option<String>,
    },
    Update {
        node: NodeId,
        reference: Option<String>,
    },
    RetryDependencies {
        node: NodeId,
    },
    SetEnabled {
        node: NodeId,
        enabled: bool,
    },
    Uninstall {
        node: NodeId,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationKind {
    Install,
    Update,
    RetryDependencies,
    Enable,
    Disable,
    Uninstall,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationPhase {
    Prepared,
    PublishIntent,
    OldBackedUp,
    Published,
    DependenciesInProgress,
    Verified,
    CleanupPending,
    Completed,
    Failed,
}
impl OperationPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::PublishIntent => "publish_intent",
            Self::OldBackedUp => "old_backed_up",
            Self::Published => "published",
            Self::DependenciesInProgress => "dependencies_in_progress",
            Self::Verified => "verified",
            Self::CleanupPending => "cleanup_pending",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
    /// Phases whose filesystem or dependency state is not yet reconciled. Cleanup of a retained
    /// backup does not endanger ComfyUI, so it never blocks a start.
    pub fn blocks_start(self) -> bool {
        matches!(
            self,
            Self::PublishIntent
                | Self::OldBackedUp
                | Self::Published
                | Self::DependenciesInProgress
        )
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeOperation {
    pub id: NodeId,
    pub kind: OperationKind,
    pub phase: OperationPhase,
    pub node: InstalledNode,
    pub prior: Option<InstalledNode>,
    pub environment: EnvId,
    pub pins: PinSet,
    pub binding: String,
    pub artifact_digest: Option<String>,
    pub requirements_digest: Option<String>,
    pub hook_digest: Option<String>,
    pub consent_identity: Option<String>,
    pub failure: Option<String>,
    /// Derived relative locations; no caller-provided absolute paths are trusted.
    pub staging: String,
    pub backup: String,
    pub dependency_requirements: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanReview {
    pub id: String,
    pub binding: String,
    pub kind: OperationKind,
    pub source: NodeSource,
    pub name: String,
    pub prior_source: Option<NodeSource>,
    /// Registry dependencies followed by the lines of the node's root requirements.txt.
    pub requirements: Vec<String>,
    pub artifact_digest: Option<String>,
    pub tree_digest: String,
    pub requirements_digest: Option<String>,
    pub hook_digest: Option<String>,
    pub environment: EnvId,
    pub pins: PinSet,
    pub protected_changes: Vec<PinChange>,
    pub warnings: Vec<String>,
    pub trusted_source: bool,
    pub trusted_install_py: bool,
    pub has_requirements: bool,
    pub has_install_py: bool,
}
#[derive(Debug, Clone)]
pub struct StagedNode {
    pub source: NodeSource,
    pub name: SafeFileName,
    pub tree_digest: String,
    pub artifact_digest: Option<String>,
    pub requirement_lines: Vec<String>,
    pub requirements_digest: Option<String>,
    pub hook_digest: Option<String>,
    pub dependencies: Vec<String>,
    pub warnings: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeSnapshot {
    pub digest: String,
    pub requirements_digest: Option<String>,
    pub hook_digest: Option<String>,
    pub requirement_lines: Vec<String>,
}
/// What reconciliation concluded about one interrupted operation's files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryAction {
    /// Only owned, unpublished staging existed and was removed.
    Discarded,
    /// The previous state is back in place.
    RolledBack,
    /// The new state is in place and cleanup finished.
    RolledForward,
    /// The new state is in place but a retained backup could not be recycled yet.
    CleanupPending,
    /// The disk state is ambiguous; every copy is preserved and the operation stays unresolved.
    Blocked,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trust {
    pub namespace: String,
    pub allow_install_py: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyOutcome {
    pub node: NodeId,
    pub environment: EnvId,
    pub source_identity: String,
    pub state: DependencyState,
}
#[derive(Debug, Clone)]
pub struct CommitDecision {
    pub consent: InstallConsent,
    pub protected_changes: Vec<PinChange>,
}
#[derive(Debug, Clone)]
pub struct NodeOutcome {
    pub operation: NodeId,
    pub node: InstalledNode,
    pub cleanup_pending: bool,
}
#[derive(Debug, Clone)]
pub struct BatchOutcome {
    pub results: Vec<(String, Result<NodeOutcome, NodeAppError>)>,
    pub stopped_for_repair: bool,
}

#[derive(Clone)]
pub(crate) struct PreparedPlan {
    pub review: PlanReview,
    pub operation: NodeOperation,
    pub root: PathBuf,
    pub package: Option<PackagePlan>,
}
#[derive(Default)]
pub struct NodePlans(pub(crate) Mutex<HashMap<String, PreparedPlan>>);
#[derive(Clone)]
pub struct NodeDeps {
    pub environment: EnvironmentDeps,
    pub registry: Arc<dyn RegistryClient>,
    pub store: Arc<dyn NodeRepository>,
    pub files: Arc<dyn NodeFiles>,
    pub hook: Arc<dyn NodeHook>,
    pub plans: Arc<NodePlans>,
}
impl NodeDeps {
    pub(crate) fn take_plan(&self, id: &str) -> Result<PreparedPlan, NodeAppError> {
        self.plans
            .0
            .lock()
            .map_err(|_| NodeAppError::Storage)?
            .remove(id)
            .ok_or(NodeAppError::StalePlan)
    }
}
pub struct NodeContext {
    pub cancel: CancelToken,
    pub sink: Arc<dyn crate::environment::SetupProgressSink>,
}
impl Default for NodeContext {
    fn default() -> Self {
        Self {
            cancel: CancelToken::new(),
            sink: Arc::new(crate::environment::NullSink),
        }
    }
}
