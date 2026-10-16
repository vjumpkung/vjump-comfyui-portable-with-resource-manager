//! IPC DTOs of the custom-node context. Counters are `f64` because JavaScript numbers are IEEE
//! doubles. No DTO carries an absolute path, a transfer URL or a credential; the UI refers to
//! prepared plans by opaque IDs and never supplies resolved metadata or paths.

use application::custom_nodes::{
    CachedPage, NodeAppError, NodeOperation, NodeOutcome, OperationKind, OperationPhase,
    PlanReview, RegistryNode,
};
use domain::custom_nodes::{
    DependencyState, InstalledNode, NodeId, NodeSource, ProvenanceOrigin, Support,
};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::{env_dto::PinChangeDto, error::IpcError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SupportDto {
    Compatible,
    Incompatible,
    Unknown,
}
impl From<Support> for SupportDto {
    fn from(value: Support) -> Self {
        match value {
            Support::Compatible => Self::Compatible,
            Support::Incompatible => Self::Incompatible,
            Support::Unknown => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NodeSourceDto {
    #[serde(rename_all = "camelCase")]
    Registry {
        id: String,
        publisher: Option<String>,
        version: String,
        repository: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Git {
        repository: String,
        commit: String,
        branch: Option<String>,
    },
    Manual,
}
impl From<&NodeSource> for NodeSourceDto {
    fn from(value: &NodeSource) -> Self {
        match value {
            NodeSource::Registry {
                id,
                publisher,
                version,
                repository,
            } => Self::Registry {
                id: id.clone(),
                publisher: publisher.clone(),
                version: version.clone(),
                repository: repository.clone(),
            },
            NodeSource::Git {
                repository,
                commit,
                branch,
            } => Self::Git {
                repository: repository.clone(),
                commit: commit.clone(),
                branch: branch.clone(),
            },
            NodeSource::Manual => Self::Manual,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DependencyStateDto {
    NotRun,
    Ok,
    Failed,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OriginDto {
    App,
    Observed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InstalledNodeDto {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub single_file: bool,
    pub source: NodeSourceDto,
    pub origin: OriginDto,
    pub dependencies: DependencyStateDto,
    /// The provenance on disk disagrees with the stored record.
    pub conflict: bool,
    /// Local changes of a Git node; unknown when Git could not be asked.
    pub dirty: Option<bool>,
}
impl From<&InstalledNode> for InstalledNodeDto {
    fn from(node: &InstalledNode) -> Self {
        Self {
            id: node.id.as_str().to_owned(),
            name: node.name.as_str().to_owned(),
            enabled: node.enabled,
            single_file: node.single_file,
            source: (&node.source).into(),
            origin: match node.origin {
                ProvenanceOrigin::App => OriginDto::App,
                ProvenanceOrigin::ExternallyObserved => OriginDto::Observed,
            },
            dependencies: match node.dependencies {
                DependencyState::NotRun => DependencyStateDto::NotRun,
                DependencyState::Ok => DependencyStateDto::Ok,
                DependencyState::Failed => DependencyStateDto::Failed,
            },
            conflict: node.conflict,
            dirty: node.dirty,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OperationKindDto {
    Install,
    Update,
    RetryDependencies,
    Enable,
    Disable,
    Uninstall,
}
impl From<OperationKind> for OperationKindDto {
    fn from(value: OperationKind) -> Self {
        match value {
            OperationKind::Install => Self::Install,
            OperationKind::Update => Self::Update,
            OperationKind::RetryDependencies => Self::RetryDependencies,
            OperationKind::Enable => Self::Enable,
            OperationKind::Disable => Self::Disable,
            OperationKind::Uninstall => Self::Uninstall,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OperationDto {
    pub id: String,
    pub kind: OperationKindDto,
    /// One of the journal phases, for example `publish_intent` or `completed`.
    pub phase: String,
    pub node_name: String,
    pub failure: Option<String>,
    pub log_tail: Vec<String>,
}
impl OperationDto {
    pub fn new(op: &NodeOperation, log_tail: Vec<String>) -> Self {
        Self {
            id: op.id.as_str().to_owned(),
            kind: op.kind.into(),
            phase: op.phase.as_str().to_owned(),
            node_name: op.node.name.as_str().to_owned(),
            failure: op.failure.clone(),
            log_tail,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NodeListDto {
    pub nodes: Vec<InstalledNodeDto>,
    /// Operations that are not completed or failed; a start is blocked while one is unresolved.
    pub unresolved: Vec<OperationDto>,
    /// The selected environment needs repair; most node operations are limited until then.
    pub repair_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RegistrySearchInput {
    pub search: String,
    pub page: u32,
    pub limit: u32,
    /// Bypass the cache lifetime for this query.
    pub refresh: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RegistryNodeDto {
    pub id: String,
    pub name: String,
    pub publisher: Option<String>,
    pub description: Option<String>,
    pub downloads: Option<f64>,
    /// GitHub stars as reported by the registry; unknown stays null.
    pub github_stars: Option<f64>,
    pub latest_version: Option<String>,
    pub repository: Option<String>,
    pub os: SupportDto,
    pub accelerator: SupportDto,
    pub comfyui: SupportDto,
    pub flagged: bool,
    pub deprecated: bool,
    pub warnings: Vec<String>,
}
impl From<&RegistryNode> for RegistryNodeDto {
    fn from(node: &RegistryNode) -> Self {
        Self {
            id: node.id.clone(),
            name: node.name.clone(),
            publisher: node.publisher.clone(),
            description: node.description.clone(),
            downloads: node.downloads.map(|v| v as f64),
            github_stars: node.github_stars.map(|v| v as f64),
            latest_version: node.latest_version.clone(),
            repository: node.repository.clone(),
            os: node.evaluated.os.into(),
            accelerator: node.evaluated.accelerator.into(),
            comfyui: node.evaluated.comfyui.into(),
            flagged: node.evaluated.flagged,
            deprecated: node.evaluated.deprecated,
            warnings: node.evaluated.warnings(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RegistryPageDto {
    pub nodes: Vec<RegistryNodeDto>,
    pub page: u32,
    pub limit: u32,
    pub total: f64,
    pub total_pages: u32,
    pub fetched_at: String,
    /// Served from the cache because the registry could not be reached.
    pub stale: bool,
}
impl From<&CachedPage> for RegistryPageDto {
    fn from(value: &CachedPage) -> Self {
        Self {
            nodes: value.page.nodes.iter().map(Into::into).collect(),
            page: value.page.page,
            limit: value.page.limit,
            total: value.page.total as f64,
            total_pages: value.page.total_pages,
            fetched_at: value.fetched_at.clone(),
            stale: value.stale,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PrepareRegistryInput {
    /// Client-chosen key that `node_cancel` accepts while the preparation runs.
    pub request_id: String,
    pub id: String,
    /// Absent means the latest release, resolved once and shown in the review.
    pub version: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PrepareGitInput {
    pub request_id: String,
    pub repository: String,
    /// Branch, tag or commit; absent means the default branch.
    pub reference: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PrepareUpdateInput {
    pub request_id: String,
    pub node_id: String,
    /// Required for detached or upstream-less Git nodes; a tag, branch or commit.
    pub reference: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PrepareRetryInput {
    pub request_id: String,
    pub node_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlanReviewDto {
    pub id: String,
    pub binding: String,
    pub kind: OperationKindDto,
    pub name: String,
    pub source: NodeSourceDto,
    /// The source before an update.
    pub prior_source: Option<NodeSourceDto>,
    pub requirements: Vec<String>,
    pub artifact_digest: Option<String>,
    pub tree_digest: String,
    pub hook_digest: Option<String>,
    pub protected_changes: Vec<PinChangeDto>,
    pub warnings: Vec<String>,
    pub trusted_source: bool,
    pub trusted_install_py: bool,
    pub has_requirements: bool,
    pub has_install_py: bool,
}
impl From<&PlanReview> for PlanReviewDto {
    fn from(review: &PlanReview) -> Self {
        Self {
            id: review.id.clone(),
            binding: review.binding.clone(),
            kind: review.kind.into(),
            name: review.name.clone(),
            source: (&review.source).into(),
            prior_source: review.prior_source.as_ref().map(Into::into),
            requirements: review.requirements.clone(),
            artifact_digest: review.artifact_digest.clone(),
            tree_digest: review.tree_digest.clone(),
            hook_digest: review.hook_digest.clone(),
            protected_changes: review.protected_changes.iter().map(Into::into).collect(),
            warnings: review.warnings.clone(),
            trusted_source: review.trusted_source,
            trusted_install_py: review.trusted_install_py,
            has_requirements: review.has_requirements,
            has_install_py: review.has_install_py,
        }
    }
}

/// The user's decision for one reviewed plan. `install.py` defaults to unchecked in the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CommitInput {
    pub plan_id: String,
    pub binding: String,
    pub execute_node_code: bool,
    pub run_install_py: bool,
    pub acknowledge_warnings: bool,
    pub remember_source: bool,
    pub remember_install_py: bool,
    pub confirmed_changes: Vec<PinChangeDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAllInput {
    pub items: Vec<CommitInput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NodeIdInput {
    pub node_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetEnabledInput {
    pub node_id: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NodeOutcomeDto {
    pub operation_id: String,
    pub node: Option<InstalledNodeDto>,
    /// A retained backup could not be moved to the Recycle Bin yet.
    pub cleanup_pending: bool,
}
impl From<&NodeOutcome> for NodeOutcomeDto {
    fn from(outcome: &NodeOutcome) -> Self {
        Self {
            operation_id: outcome.operation.as_str().to_owned(),
            node: Some((&outcome.node).into()),
            cleanup_pending: outcome.cleanup_pending,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BatchItemDto {
    pub plan_id: String,
    pub outcome: Option<NodeOutcomeDto>,
    pub error: Option<IpcError>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BatchOutcomeDto {
    pub items: Vec<BatchItemDto>,
    /// The batch stopped because the environment now needs repair.
    pub stopped_for_repair: bool,
}

/// Events of one subscription. Sequence numbers increase by one per event; a `gap` means events
/// were dropped for a slow subscriber and the UI must refetch the list and operation state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NodeEventDto {
    #[serde(rename_all = "camelCase")]
    Snapshot { epoch: String, sequence: f64 },
    #[serde(rename_all = "camelCase")]
    Changed { epoch: String, sequence: f64 },
    #[serde(rename_all = "camelCase")]
    Log {
        epoch: String,
        sequence: f64,
        operation_id: String,
        line: String,
    },
    #[serde(rename_all = "camelCase")]
    Gap { epoch: String, sequence: f64 },
}

pub fn node_id(text: &str) -> Result<NodeId, IpcError> {
    NodeId::new(text)
        .map_err(|_| IpcError::new("invalid_identifier", "The identifier is not valid."))
}

/// Request keys are client chosen; keep them short and harmless.
pub fn request_key(text: &str) -> Result<String, IpcError> {
    let valid = !text.is_empty()
        && text.len() <= 64
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'));
    if valid {
        Ok(text.to_owned())
    } else {
        Err(IpcError::new(
            "invalid_identifier",
            "The identifier is not valid.",
        ))
    }
}

pub fn describe(error: NodeAppError) -> (&'static str, String) {
    use NodeAppError as E;
    let fixed = |code: &'static str, message: &str| (code, message.to_owned());
    match error {
        E::Busy => fixed(
            "maintenance_busy",
            "Another maintenance operation is running. Wait for it to finish.",
        ),
        E::RuntimeRunning => fixed(
            "comfyui_running",
            "ComfyUI must be stopped before changing custom nodes. Stop it and retry.",
        ),
        E::EnvironmentUnavailable => fixed(
            "environment_not_usable",
            "A verified environment is required. Repair the environment first.",
        ),
        E::RecoveryRequired => fixed(
            "node_recovery_required",
            "An interrupted custom node operation must be resolved first. Rescan the custom nodes.",
        ),
        E::StalePlan => fixed(
            "node_plan_stale",
            "The reviewed plan is no longer current. Prepare it again.",
        ),
        E::ConsentRequired => fixed(
            "node_consent_required",
            "The confirmation does not cover this exact plan.",
        ),
        E::Cancelled => fixed("cancelled", "The operation was cancelled."),
        E::PinDrift => fixed(
            "pin_drift",
            "Protected packages changed unexpectedly and the environment needs repair.",
        ),
        E::DependenciesFailed => fixed(
            "node_dependencies_failed",
            "Installing the node's dependencies failed. The environment needs repair.",
        ),
        E::NotFound => fixed("node_not_found", "The custom node was not found."),
        E::TargetExists => fixed(
            "node_target_exists",
            "A custom node with that folder name already exists.",
        ),
        E::UnsafeSource => fixed(
            "node_unsafe_source",
            "The source, archive or folder was rejected by the safety checks.",
        ),
        E::GitRefused => fixed(
            "node_git_refused",
            "The repository has local changes, an unfinished operation, a diverged branch or no selected upstream, so it was not changed.",
        ),
        E::RegistryStatus(code) => (
            "registry_refused",
            format!("The registry answered with HTTP {code}."),
        ),
        E::Offline => fixed(
            "registry_offline",
            "The registry or repository could not be reached.",
        ),
        E::InvalidRegistry => fixed(
            "registry_invalid",
            "The registry answered with data that was not valid or did not match the request.",
        ),
        E::Storage => fixed(
            "node_storage_unavailable",
            "Custom node storage is unavailable.",
        ),
        E::Filesystem => fixed(
            "node_filesystem",
            "A custom node file operation failed. Nothing was replaced.",
        ),
        E::ResolutionFailed => fixed(
            "node_resolution_failed",
            "The dependencies could not be resolved. No package was changed.",
        ),
        E::UpToDate => fixed("node_up_to_date", "The node is already up to date."),
        E::InvalidRequest => fixed("validation", "The request is not valid."),
        E::AmbiguousReference => fixed(
            "node_ambiguous_reference",
            "The name matches both a branch and a tag. Use a commit to choose exactly.",
        ),
        E::Failed(message) => ("node_failed", message),
    }
}

impl From<NodeAppError> for IpcError {
    fn from(error: NodeAppError) -> Self {
        let (code, message) = describe(error);
        Self {
            code: code.to_owned(),
            message,
        }
    }
}

/// Whether a phase string means the operation is finished.
pub fn is_final(phase: OperationPhase) -> bool {
    matches!(phase, OperationPhase::Completed | OperationPhase::Failed)
}
