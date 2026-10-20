use std::{path::PathBuf, str::FromStr};

use domain::environment::{
    EnvId, EnvState, Ownership, PinSet, PythonSourceKind, PythonVersion, SetupMode,
};

use super::error::EnvironmentAppError;

/// The identifier of a provisioning run: lowercase letters, digits and `-`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RunId(String);

impl RunId {
    pub fn new(value: impl Into<String>) -> Result<Self, EnvironmentAppError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
        if valid {
            Ok(Self(value))
        } else {
            Err(EnvironmentAppError::InvalidRunId)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for RunId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepPolicy {
    /// A failure stops the run and leaves the previous environment active.
    Required,
    /// A failure pauses the run; the user may retry, skip the step or abort.
    Optional,
    /// A failure is recorded as a warning and the run continues.
    BestEffort,
}

/// The ordered setup steps. `Preflight` is step 0; the others are FR-SETUP-6 steps 1 to 13.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StepId {
    Preflight,
    Tools,
    PythonVenv,
    Seed,
    CloneComfyUi,
    InstallTorch,
    VerifyTorch,
    Triton,
    Accelerators,
    ComfyRequirements,
    Manager,
    CuratedNodes,
    FinalVerify,
    CacheClean,
}

impl StepId {
    pub const ALL: [StepId; 14] = [
        Self::Preflight,
        Self::Tools,
        Self::PythonVenv,
        Self::Seed,
        Self::CloneComfyUi,
        Self::InstallTorch,
        Self::VerifyTorch,
        Self::Triton,
        Self::Accelerators,
        Self::ComfyRequirements,
        Self::Manager,
        Self::CuratedNodes,
        Self::FinalVerify,
        Self::CacheClean,
    ];

    pub fn number(self) -> u8 {
        Self::ALL
            .iter()
            .position(|step| *step == self)
            .expect("every step is listed") as u8
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Preflight => "preflight",
            Self::Tools => "tools",
            Self::PythonVenv => "python_venv",
            Self::Seed => "seed",
            Self::CloneComfyUi => "clone_comfyui",
            Self::InstallTorch => "install_torch",
            Self::VerifyTorch => "verify_torch",
            Self::Triton => "triton",
            Self::Accelerators => "accelerators",
            Self::ComfyRequirements => "comfy_requirements",
            Self::Manager => "manager",
            Self::CuratedNodes => "curated_nodes",
            Self::FinalVerify => "final_verify",
            Self::CacheClean => "cache_clean",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Preflight => "Preflight checks",
            Self::Tools => "Fetch and verify tools",
            Self::PythonVenv => "Prepare Python and the virtual environment",
            Self::Seed => "Seed pip, setuptools and wheel",
            Self::CloneComfyUi => "Clone ComfyUI",
            Self::InstallTorch => "Install PyTorch",
            Self::VerifyTorch => "Verify PyTorch and CUDA",
            Self::Triton => "Install Triton",
            Self::Accelerators => "Install optional accelerators",
            Self::ComfyRequirements => "Install ComfyUI requirements",
            Self::Manager => "Install ComfyUI-Manager",
            Self::CuratedNodes => "Install curated custom nodes",
            Self::FinalVerify => "Final verification",
            Self::CacheClean => "Clean caches",
        }
    }

    pub fn policy(self) -> StepPolicy {
        match self {
            Self::Manager | Self::CuratedNodes => StepPolicy::Optional,
            Self::Accelerators | Self::CacheClean => StepPolicy::BestEffort,
            _ => StepPolicy::Required,
        }
    }
}

impl FromStr for StepId {
    type Err = EnvironmentAppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .iter()
            .copied()
            .find(|step| step.as_str() == value)
            .ok_or(EnvironmentAppError::InvalidStep)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepStatus {
    Pending,
    Running,
    Succeeded,
    Warning,
    Failed,
    Skipped,
    Interrupted,
}

impl StepStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Warning => "warning",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::Interrupted => "interrupted",
        }
    }
}

impl FromStr for StepStatus {
    type Err = EnvironmentAppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        [
            Self::Pending,
            Self::Running,
            Self::Succeeded,
            Self::Warning,
            Self::Failed,
            Self::Skipped,
            Self::Interrupted,
        ]
        .into_iter()
        .find(|status| status.as_str() == value)
        .ok_or(EnvironmentAppError::InvalidStep)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepRecord {
    pub step: StepId,
    pub status: StepStatus,
    pub attempts: u32,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStatus {
    /// Persisted and waiting for the workflow to start.
    Registered,
    Running,
    /// Stopped at a failed step; the user may retry, skip an optional step or abort.
    Failed,
    Cancelled,
    Succeeded,
    /// The app stopped while the run was active; it can be retried.
    Interrupted,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Registered => "registered",
            Self::Running => "running",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Succeeded => "succeeded",
            Self::Interrupted => "interrupted",
        }
    }

    pub fn is_active(self) -> bool {
        matches!(self, Self::Registered | Self::Running)
    }

    pub fn is_retryable(self) -> bool {
        matches!(self, Self::Failed | Self::Interrupted)
    }
}

impl FromStr for RunStatus {
    type Err = EnvironmentAppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        [
            Self::Registered,
            Self::Running,
            Self::Failed,
            Self::Cancelled,
            Self::Succeeded,
            Self::Interrupted,
        ]
        .into_iter()
        .find(|status| status.as_str() == value)
        .ok_or(EnvironmentAppError::InvalidStep)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationPhase {
    None,
    /// Intent recorded with the old and new environment ids.
    Intent,
    /// `install.json` atomically replaced.
    ManifestPublished,
    /// The active selection switched in one transaction.
    Committed,
}

impl ActivationPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Intent => "intent",
            Self::ManifestPublished => "manifest_published",
            Self::Committed => "committed",
        }
    }

    pub fn is_incomplete(self) -> bool {
        matches!(self, Self::Intent | Self::ManifestPublished)
    }
}

impl FromStr for ActivationPhase {
    type Err = EnvironmentAppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        [
            Self::None,
            Self::Intent,
            Self::ManifestPublished,
            Self::Committed,
        ]
        .into_iter()
        .find(|phase| phase.as_str() == value)
        .ok_or(EnvironmentAppError::InvalidStep)
    }
}

/// Where the interpreter comes from, as chosen in the wizard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PythonSourceInput {
    Managed,
    CustomVenv(PathBuf),
    CustomInterpreter(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComfyRef {
    /// The latest published GitHub release, resolved once per run.
    Latest,
    Tag(String),
    Branch(String),
    /// Re-setup keeps the existing checkout at its recorded commit.
    KeepCurrent,
}

/// The immutable input of a setup run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupRequest {
    pub mode: SetupMode,
    pub install_root: PathBuf,
    pub python_source: PythonSourceInput,
    pub profile_id: String,
    pub comfyui_ref: ComfyRef,
    pub install_manager: bool,
    /// Explicit profile component ids. An empty list selects no accelerators.
    pub accelerators: Vec<String>,
    /// Catalog ids of the curated custom nodes to install (first run only). A first-run request
    /// may carry the user's consent, given on the wizard's review stage, for ComfyUI-Manager and
    /// the selected curated nodes (`third_party_consent`); without it every install pauses.
    pub curated_nodes: Vec<String>,
    pub clean_cache: bool,
    /// Explicit confirmation that setup may install packages into an existing (adopted) venv.
    pub confirm_adopted_changes: bool,
    /// Allows source distributions (build hooks run third-party code) in dependency installs.
    pub allow_source_builds: bool,
    /// The user confirmed on the review stage that setup may run third-party code (first run
    /// only).
    pub third_party_consent: bool,
}

/// The environment a run builds, decided from the validated Python source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonTarget {
    pub ownership: Ownership,
    pub source: PythonSourceKind,
    pub env_path: PathBuf,
    /// The custom interpreter to create the venv from, when the user supplied one.
    pub interpreter: Option<PathBuf>,
    pub create_venv: bool,
    /// `Some(false)` when the interpreter lacks the headers Triton needs.
    pub development_files_ok: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedGit {
    pub url: String,
    /// The tag or branch name that was resolved, or `current` for a kept checkout.
    pub reference: String,
    /// The branch to check out and track; `None` for tags and kept checkouts.
    pub branch: Option<String>,
    pub commit: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolvedSources {
    pub comfyui: Option<ResolvedGit>,
    pub manager: Option<ResolvedGit>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsentRequest {
    pub source: String,
    pub commit: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    Failed,
    CommandFailed,
    Retryable,
    ConsentRequired,
    Interrupted,
}

impl FailureKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Failed => "failed",
            Self::CommandFailed => "command_failed",
            Self::Retryable => "retryable",
            Self::ConsentRequired => "consent_required",
            Self::Interrupted => "interrupted",
        }
    }
}

impl FromStr for FailureKind {
    type Err = EnvironmentAppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        [
            Self::Failed,
            Self::CommandFailed,
            Self::Retryable,
            Self::ConsentRequired,
            Self::Interrupted,
        ]
        .into_iter()
        .find(|kind| kind.as_str() == value)
        .ok_or(EnvironmentAppError::InvalidStep)
    }
}

/// A sanitized, redacted description of why a step failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepFailure {
    pub step: StepId,
    pub kind: FailureKind,
    pub message: String,
    pub command: Option<String>,
    pub exit_code: Option<i32>,
    pub tail: Vec<String>,
    pub consent: Option<ConsentRequest>,
    /// Decided by the backend: false once the failure damaged the candidate environment (a
    /// published dependency failure or protected-package drift), so Skip cannot hide it.
    pub can_skip: bool,
}

/// A run record. It is persisted after every state change, so the UI can reconstruct it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvisioningRun {
    pub id: RunId,
    pub request: SetupRequest,
    pub profile: domain::environment::SetupProfile,
    pub target_env: EnvId,
    pub previous_env: Option<EnvId>,
    pub python_target: PythonTarget,
    pub resolved: ResolvedSources,
    pub components: InstalledComponents,
    pub steps: Vec<StepRecord>,
    pub status: RunStatus,
    pub cancel_requested: bool,
    pub failure: Option<StepFailure>,
    pub activation: ActivationPhase,
    /// The Manager commit whose code execution the user approved.
    pub manager_consent: Option<String>,
    /// One entry per selected curated node, in catalog order.
    pub curated: Vec<CuratedProgress>,
    pub created_at: String,
    pub updated_at: String,
}

impl ProvisioningRun {
    pub fn step(&self, step: StepId) -> &StepRecord {
        &self.steps[usize::from(step.number())]
    }

    pub fn step_mut(&mut self, step: StepId) -> &mut StepRecord {
        &mut self.steps[usize::from(step.number())]
    }

    pub fn fresh_steps() -> Vec<StepRecord> {
        StepId::ALL
            .iter()
            .map(|step| StepRecord {
                step: *step,
                status: StepStatus::Pending,
                attempts: 0,
                message: None,
            })
            .collect()
    }
}

/// One curated custom node of the catalog (FR-SETUP-9): the latest commit of a repository's
/// default branch at setup time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CuratedNodeSpec {
    /// Derived from the repository name; unique within the catalog.
    pub id: String,
    pub name: String,
    pub repository: String,
    pub provenance: String,
    /// Why this entry cannot be installed. It stays listed but cannot be selected.
    pub unavailable: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CuratedStatus {
    /// Nothing is prepared yet.
    Pending,
    /// A reviewed plan waits for the user's decision. Plans live only in this process.
    Prepared,
    /// Publication started, but the reviewed install or candidate dependencies failed.
    Failed,
    /// Installed at the exact commit with successful dependency outcomes for the candidate.
    Installed,
    /// The user skipped the step before this node was installed.
    Skipped,
}

impl CuratedStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Prepared => "prepared",
            Self::Failed => "failed",
            Self::Installed => "installed",
            Self::Skipped => "skipped",
        }
    }
}

impl FromStr for CuratedStatus {
    type Err = EnvironmentAppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        [
            Self::Pending,
            Self::Prepared,
            Self::Failed,
            Self::Installed,
            Self::Skipped,
        ]
        .into_iter()
        .find(|status| status.as_str() == value)
        .ok_or(EnvironmentAppError::InvalidStep)
    }
}

/// The persisted progress of one curated node. The plan and journal operation share one id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CuratedProgress {
    pub id: String,
    pub status: CuratedStatus,
    /// The reviewed plan of the latest preparation; obsolete after a backend restart.
    pub plan: Option<String>,
    /// The journal operation that installed the node, saved before the next node starts.
    pub operation: Option<String>,
    pub message: Option<String>,
}

impl CuratedProgress {
    pub fn pending(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            status: CuratedStatus::Pending,
            plan: None,
            operation: None,
            message: None,
        }
    }
}

/// The `install.json` marker. It identifies the active environment so the database can be
/// validated and reconciled against it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallManifest {
    pub schema_version: u32,
    pub install_root: PathBuf,
    pub generation: u64,
    pub env_id: EnvId,
    pub env_path: PathBuf,
    pub ownership: Ownership,
    pub python_version: PythonVersion,
    pub profile_id: String,
    pub profile_revision: u32,
    pub comfyui_reference: Option<String>,
    pub comfyui_commit: Option<String>,
    pub run_id: RunId,
    pub activated_at: String,
    pub components: InstalledComponents,
}

pub const MANIFEST_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentStatus {
    Installed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceleratorResult {
    pub id: String,
    pub selected: bool,
    pub status: ComponentStatus,
    /// None means no import test ran.
    pub importable: Option<bool>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagerResult {
    pub selected: bool,
    pub status: ComponentStatus,
    pub source_commit: Option<String>,
    pub message: Option<String>,
}

/// None for Manager and an empty accelerator list represent unknown legacy metadata.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstalledComponents {
    pub manager: Option<ManagerResult>,
    pub accelerators: Vec<AcceleratorResult>,
}

impl InstalledComponents {
    pub fn selected(request: &SetupRequest, profile: &domain::environment::SetupProfile) -> Self {
        Self {
            manager: Some(ManagerResult {
                selected: request.install_manager,
                status: ComponentStatus::Skipped,
                source_commit: None,
                message: None,
            }),
            accelerators: profile
                .accelerators
                .iter()
                .map(|spec| AcceleratorResult {
                    id: spec.id.clone(),
                    selected: request.accelerators.contains(&spec.id),
                    status: ComponentStatus::Skipped,
                    importable: None,
                    message: None,
                })
                .collect(),
        }
    }
}

/// What reading `install.json` found. A short-lived value, so the size difference between
/// the variants does not matter.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)]
pub enum ManifestState {
    Absent,
    Valid(InstallManifest),
    /// Unreadable or structurally invalid. It is never silently replaced.
    Corrupt,
    /// Written by a newer version of the app.
    Newer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPackage {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentSummary {
    pub id: EnvId,
    pub path: PathBuf,
    pub ownership: Ownership,
    pub source: PythonSourceKind,
    pub python_version: PythonVersion,
    pub profile_id: String,
    pub profile_revision: u32,
    pub state: EnvState,
    pub pins: PinSet,
    pub verification: Option<String>,
}
