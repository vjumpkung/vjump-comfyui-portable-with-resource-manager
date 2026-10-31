//! IPC DTOs of the custom-node context. Counters are `f64` because JavaScript numbers are IEEE
//! doubles. No DTO carries an absolute path, a transfer URL or a credential; the UI refers to
//! prepared plans by opaque IDs and never supplies resolved metadata or paths.

use application::custom_nodes::{
    CachedPage, NodeAppError, NodeGitStatus, NodeOperation, NodeOutcome, OperationKind, PlanReview,
    RegistryNode,
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
    pub node_id: String,
    pub blocks_start: bool,
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
            node_id: op.node.id.as_str().to_owned(),
            blocks_start: op.blocks_start(),
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
    /// Unfinished operations; ordinary cleanup does not block launch. See each `blocksStart`.
    pub unresolved: Vec<OperationDto>,
    pub pending_reviews: Vec<PlanReviewDto>,
    /// The newest 16 terminal operations, newest first.
    pub recent: Vec<OperationDto>,
    /// The selected environment needs repair; most node operations are limited until then.
    pub repair_required: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NodeGitStatusDto {
    pub commit: String,
    pub branch: Option<String>,
    pub dirty: bool,
    pub unfinished_operation: bool,
    pub ahead: f64,
    pub behind: f64,
    pub has_upstream: bool,
    pub status: String,
}
impl From<&NodeGitStatus> for NodeGitStatusDto {
    fn from(value: &NodeGitStatus) -> Self {
        Self {
            commit: value.revision.commit.clone(),
            branch: value.revision.branch.clone(),
            dirty: value.state.dirty,
            unfinished_operation: value.state.unfinished_operation,
            ahead: value.state.ahead as f64,
            behind: value.state.behind as f64,
            has_upstream: value.state.has_upstream,
            status: value.state.status.clone(),
        }
    }
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
    pub supported_os: Option<Vec<String>>,
    pub supported_accelerators: Option<Vec<String>>,
    pub supported_comfyui_version: Option<String>,
    pub status: Option<String>,
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
            supported_os: node.compatibility.supported_os.clone(),
            supported_accelerators: node.compatibility.supported_accelerators.clone(),
            supported_comfyui_version: node.compatibility.comfyui_expression.clone(),
            status: node.compatibility.status.clone(),
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

/// The fixed refusal text followed by the bounded status the adapter retained. The adapter already
/// redacted it and keeps it node-relative; control characters are dropped here as a last guard.
fn git_refusal_message(status: &str) -> String {
    const REFUSAL: &str = "The repository has local changes, an unfinished operation, a diverged branch or no selected upstream, so it was not changed.";
    let status: String = status
        .chars()
        .filter(|c| *c == '\n' || !c.is_control())
        .take(2000)
        .collect();
    // The leading column of a short status line is significant, so only the end is trimmed.
    let status = status.trim_end();
    if status.trim().is_empty() {
        REFUSAL.to_owned()
    } else {
        format!("{REFUSAL}\n\nDetails:\n{}", status.trim_start_matches('\n'))
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
            "ComfyUI must be stopped before changing custom nodes. If it is already stopped, close other programs that use the ComfyUI folder, such as a terminal opened there, and retry.",
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
        E::GitRefused { status } => ("node_git_refused", git_refusal_message(&status)),
        E::GitUnavailable => fixed(
            "node_git_unavailable",
            "Git is unavailable. Complete tool setup on Windows or install Git on Linux.",
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
            "The dependencies could not be resolved under the protected-package constraints. No package was changed.",
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

#[cfg(test)]
mod tests {
    use super::*;
    use domain::custom_nodes::{Compatibility, CompatibilityMetadata};

    #[test]
    fn registry_dto_preserves_raw_metadata_and_unknown_values() {
        let mut node = RegistryNode {
            id: "pack".into(),
            name: "Pack".into(),
            publisher: None,
            description: None,
            downloads: Some(0),
            github_stars: None,
            latest_version: None,
            repository: None,
            compatibility: CompatibilityMetadata {
                supported_os: Some(vec!["Mystery OS".into()]),
                supported_accelerators: Some(vec!["Future accelerator".into()]),
                comfyui_expression: Some(">=0.3.50".into()),
                status: Some("unknown-status".into()),
                deprecated: None,
            },
            evaluated: Compatibility {
                os: Support::Unknown,
                accelerator: Support::Unknown,
                comfyui: Support::Compatible,
                flagged: true,
                deprecated: false,
            },
            node_flagged: false,
            node_deprecated: false,
        };
        let dto = RegistryNodeDto::from(&node);
        assert_eq!(dto.supported_os, node.compatibility.supported_os);
        assert_eq!(
            dto.supported_accelerators,
            node.compatibility.supported_accelerators
        );
        assert_eq!(
            dto.supported_comfyui_version,
            node.compatibility.comfyui_expression
        );
        assert_eq!(dto.status, node.compatibility.status);
        assert_eq!(dto.os, SupportDto::Unknown);
        assert_eq!(dto.accelerator, SupportDto::Unknown);
        assert_eq!(dto.comfyui, SupportDto::Compatible);
        assert!(dto.flagged && !dto.deprecated);
        assert_eq!(dto.warnings, node.evaluated.warnings());
        let json = serde_json::to_value(&dto).unwrap();
        assert_eq!(json["downloads"], 0.0);
        assert!(json["githubStars"].is_null());
        assert_eq!(json["supportedComfyuiVersion"], ">=0.3.50");
        node.compatibility = CompatibilityMetadata::default();
        let json = serde_json::to_value(RegistryNodeDto::from(&node)).unwrap();
        for field in [
            "supportedOs",
            "supportedAccelerators",
            "supportedComfyuiVersion",
            "status",
        ] {
            assert!(json[field].is_null(), "{field}");
        }
    }

    #[test]
    fn git_status_dto_and_unavailable_error_preserve_the_contract() {
        let status = NodeGitStatus {
            revision: domain::comfy_runtime::Revision {
                commit: "a".repeat(40),
                branch: None,
                tag: None,
            },
            state: domain::comfy_runtime::RepoState {
                dirty: true,
                unfinished_operation: true,
                ahead: 2,
                behind: 3,
                has_upstream: false,
                status: " M requirements.txt\n".into(),
            },
        };
        let json = serde_json::to_value(NodeGitStatusDto::from(&status)).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "commit": "a".repeat(40), "branch": null, "dirty": true,
                "unfinishedOperation": true, "ahead": 2.0, "behind": 3.0,
                "hasUpstream": false, "status": " M requirements.txt\n",
            })
        );
        assert_eq!(
            IpcError::from(NodeAppError::GitUnavailable),
            IpcError::new(
                "node_git_unavailable",
                "Git is unavailable. Complete tool setup on Windows or install Git on Linux.",
            )
        );
    }

    #[test]
    fn git_refusals_keep_their_code_and_carry_only_the_bounded_status() {
        let refused = IpcError::from(NodeAppError::GitRefused {
            status: " M requirements.txt\n?? notes.txt\n".into(),
        });
        assert_eq!(refused.code, "node_git_refused");
        assert!(
            refused
                .message
                .starts_with("The repository has local changes, an unfinished operation,")
        );
        assert!(
            refused
                .message
                .ends_with("Details:\n M requirements.txt\n?? notes.txt")
        );
        let noisy = IpcError::from(NodeAppError::GitRefused {
            status: format!("a\u{1b}[31mb\n{}", "x".repeat(5000)),
        });
        assert!(!noisy.message.contains('\u{1b}'));
        assert!(noisy.message.chars().count() < 2300);
        let empty = IpcError::from(NodeAppError::GitRefused {
            status: "  \n".into(),
        });
        assert!(empty.message.ends_with("so it was not changed."));
    }

    #[test]
    fn resolution_failures_name_the_protected_package_constraints() {
        assert_eq!(
            IpcError::from(NodeAppError::ResolutionFailed),
            IpcError::new(
                "node_resolution_failed",
                "The dependencies could not be resolved under the protected-package constraints. No package was changed.",
            )
        );
    }
}
