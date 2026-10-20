use std::{path::Path, path::PathBuf, sync::Arc};

use async_trait::async_trait;
use domain::environment::{
    DistName, EnvId, HardwareObservation, OsKind, PinSet, PythonEnvironment, PythonVersion,
    SetupProfile, VerifyExpectations,
};
use thiserror::Error;

use super::model::{
    ComfyRef, ConsentRequest, CuratedNodeSpec, InstallManifest, InstalledPackage, ManifestState,
    ProvisioningRun, PythonSourceInput, ResolvedGit, RunId, RunStatus, StepFailure, StepId,
    StepStatus,
};
use crate::{
    custom_nodes::{CommitDecision, NodeOutcome, PlanReview},
    shared::{CancelToken, MaintenanceLease},
};

// ---------------------------------------------------------------------------------------
// Progress
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupEvent {
    RunStatus {
        run_id: RunId,
        status: RunStatus,
    },
    Step {
        run_id: RunId,
        step: StepId,
        status: StepStatus,
        attempt: u32,
        message: Option<String>,
    },
    /// One already redacted output line.
    Log {
        run_id: RunId,
        step: StepId,
        line: String,
    },
    Progress {
        run_id: RunId,
        step: StepId,
        done_bytes: u64,
        total_bytes: Option<u64>,
    },
    Failure {
        run_id: RunId,
        failure: Box<StepFailure>,
    },
}

/// Receives setup events. Implementations must not block; a lost listener must not stop the
/// workflow.
pub trait SetupProgressSink: Send + Sync {
    fn emit(&self, event: SetupEvent);
}

/// A sink that drops every event.
pub struct NullSink;

impl SetupProgressSink for NullSink {
    fn emit(&self, _event: SetupEvent) {}
}

/// Everything an adapter needs while it runs one step.
#[derive(Clone)]
pub struct StepContext {
    pub run_id: RunId,
    pub step: StepId,
    pub sink: Arc<dyn SetupProgressSink>,
    pub cancel: CancelToken,
    /// Allows source distributions whose build hooks run third-party code.
    pub allow_source_builds: bool,
    pub retained: Retained,
}

impl StepContext {
    pub fn log(&self, line: impl Into<String>) {
        self.sink.emit(SetupEvent::Log {
            run_id: self.run_id.clone(),
            step: self.step,
            line: line.into(),
        });
    }

    pub fn progress(&self, done_bytes: u64, total_bytes: Option<u64>) {
        self.sink.emit(SetupEvent::Progress {
            run_id: self.run_id.clone(),
            step: self.step,
            done_bytes,
            total_bytes,
        });
    }
}

// ---------------------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandFailure {
    /// The redacted command line.
    pub command: String,
    pub exit_code: Option<i32>,
    /// The last redacted output lines.
    pub tail: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProvisionError {
    /// Cancellation was honoured and the whole process tree is gone.
    #[error("the operation was cancelled")]
    Cancelled,
    #[error("protected packages could not be restored or verified: {0}")]
    PinDrift(String),
    #[error("a command failed")]
    CommandFailed(CommandFailure),
    /// A transient condition such as rate limiting or a dropped connection.
    #[error("{0}")]
    Retryable(String),
    /// A sanitized failure description.
    #[error("{0}")]
    Failed(String),
    #[error("explicit consent is required")]
    ConsentRequired(ConsentRequest),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum EnvRepoError {
    #[error("environment storage is unavailable")]
    Unavailable,
    #[error("environment storage holds an invalid record")]
    Corrupt,
    #[error("a conflicting record exists")]
    Conflict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ManifestError {
    #[error("install.json could not be written")]
    WriteFailed,
    #[error("install.json could not be read")]
    ReadFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RootSwitchError {
    #[error("the install root is fixed by a debug override")]
    FixedByOverride,
    #[error("the selected folder cannot be used: {0}")]
    InvalidDestination(String),
    #[error("the install root could not be switched")]
    Failed,
    #[error("the selected installation is missing")]
    Missing,
    #[error("ComfyUI is running")]
    ComfyUiRunning,
    #[error("ComfyUI process ownership could not be established")]
    ComfyUiStateUnknown,
    #[error("maintenance is in progress")]
    MaintenanceBusy,
    #[error("relocation journals require recovery")]
    RelocationRecoveryRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum RelocationError {
    #[error("relocation journals require recovery")]
    RecoveryRequired,
    #[error("relocation storage is unavailable")]
    Storage,
}

#[async_trait]
pub trait InstallationProbe: Send + Sync {
    async fn inspect(
        &self,
        current_root: &Path,
        manifest: &InstallManifest,
        active: &PythonEnvironment,
    ) -> Result<super::InstallationHealth, ProvisionError>;

    fn invalidate(&self) {}

    async fn validate_rebuild_checkout(&self, _root: &Path) -> Result<(), ProvisionError> {
        Ok(())
    }
}

#[async_trait]
pub trait RelocationStore: Send + Sync {
    async fn check(&self, _old_root: &Path, _new_root: &Path) -> Result<(), RelocationError> {
        Ok(())
    }
    /// The caller holds the maintenance lease and has proved both roots stopped.
    async fn accept(&self, old_root: &Path, new_root: &Path) -> Result<(), RelocationError>;
}

#[async_trait]
pub trait ExistingInstallOpener: Send + Sync {
    async fn open_existing(&self, path: &Path) -> Result<(), RootSwitchError>;
    async fn select_new(&self, path: &Path) -> Result<(), RootSwitchError>;
}

// ---------------------------------------------------------------------------------------
// Hardware, profiles and Python validation
// ---------------------------------------------------------------------------------------

#[async_trait]
pub trait HardwareProbe: Send + Sync {
    /// Observes the machine. Unknown values are carried as `None`, never guessed.
    async fn probe(&self, install_root: &Path, env_path: Option<&Path>) -> HardwareObservation;
}

pub trait ProfileCatalog: Send + Sync {
    fn list(&self) -> Vec<SetupProfile>;
    fn get(&self, id: &str) -> Option<SetupProfile>;
    fn get_revision(&self, id: &str, revision: u32) -> Option<SetupProfile> {
        self.get(id).filter(|profile| profile.revision == revision)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckId {
    TargetLocation,
    VersionSupported,
    Is64Bit,
    PackageInstaller,
    DevelopmentFiles,
    MsvcRuntime,
}

impl CheckId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TargetLocation => "target_location",
            Self::VersionSupported => "version_supported",
            Self::Is64Bit => "is_64_bit",
            Self::PackageInstaller => "package_installer",
            Self::DevelopmentFiles => "development_files",
            Self::MsvcRuntime => "msvc_runtime",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockedFeature {
    /// The whole setup cannot use this Python.
    Setup,
    /// Triton (and therefore the accelerator stack) cannot use this Python.
    Triton,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationCheck {
    pub id: CheckId,
    pub passed: bool,
    pub detail: String,
    /// What a failure blocks; empty for passed checks.
    pub blocks: Vec<BlockedFeature>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationTarget {
    /// An empty (or not yet created) folder in which the app creates an app-owned venv.
    EmptyFolder,
    /// An existing venv; it is adopted and never deleted.
    ExistingVenv,
    /// A Python executable from which the app creates a new venv under `envs/`.
    Interpreter,
    /// A non-empty folder that is not a venv.
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonValidationReport {
    pub target: ValidationTarget,
    /// The canonical location of the selected path.
    pub resolved_path: PathBuf,
    pub python_version: Option<PythonVersion>,
    pub checks: Vec<ValidationCheck>,
}

impl PythonValidationReport {
    pub fn check(&self, id: CheckId) -> Option<&ValidationCheck> {
        self.checks.iter().find(|check| check.id == id)
    }

    /// True when no failed check blocks the whole setup.
    pub fn usable(&self) -> bool {
        self.target != ValidationTarget::Rejected
            && !self
                .checks
                .iter()
                .any(|check| !check.passed && check.blocks.contains(&BlockedFeature::Setup))
    }

    /// `None` when the development files were not checked (for example a managed Python).
    pub fn development_files_ok(&self) -> Option<bool> {
        self.check(CheckId::DevelopmentFiles)
            .map(|check| check.passed)
    }
}

// ---------------------------------------------------------------------------------------
// Provisioning ports
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolInfo {
    pub name: String,
    pub version: String,
}

#[async_trait]
pub trait ToolProvisioner: Send + Sync {
    async fn ensure_uv(&self, root: &Path, ctx: &StepContext) -> Result<ToolInfo, ProvisionError>;
    async fn ensure_git(&self, root: &Path, ctx: &StepContext) -> Result<ToolInfo, ProvisionError>;
    /// Prepares the isolated, hash-locked `hf` tool venv on the pinned managed Python.
    async fn ensure_hf_tool(
        &self,
        root: &Path,
        ctx: &StepContext,
    ) -> Result<ToolInfo, ProvisionError>;

    async fn recreate_hf_tool(
        &self,
        root: &Path,
        ctx: &StepContext,
    ) -> Result<ToolInfo, ProvisionError> {
        self.ensure_hf_tool(root, ctx).await
    }

    /// Prepares the isolated, hash-locked `gdown` tool venv used for Google Drive downloads.
    async fn ensure_gdown_tool(
        &self,
        root: &Path,
        ctx: &StepContext,
    ) -> Result<ToolInfo, ProvisionError>;

    async fn recreate_gdown_tool(
        &self,
        root: &Path,
        ctx: &StepContext,
    ) -> Result<ToolInfo, ProvisionError> {
        self.ensure_gdown_tool(root, ctx).await
    }

    /// False when a download tool venv is missing, so Repair re-runs the Tools step.
    fn download_tools_ready(&self, _root: &Path) -> bool {
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VenvInfo {
    pub version: PythonVersion,
    pub base_interpreter: Option<PathBuf>,
}

#[async_trait]
pub trait PythonProvisioner: Send + Sync {
    /// Installs the pinned managed CPython and returns its interpreter. Idempotent.
    async fn install_managed_python(
        &self,
        root: &Path,
        version: PythonVersion,
        ctx: &StepContext,
    ) -> Result<PathBuf, ProvisionError>;
    /// Creates a venv at its final path from `interpreter`. Idempotent for a matching venv;
    /// a non-empty folder that is not a venv is an error and is left untouched.
    async fn create_venv(
        &self,
        root: &Path,
        interpreter: &Path,
        target: &Path,
        ctx: &StepContext,
    ) -> Result<VenvInfo, ProvisionError>;
    async fn inspect_venv(&self, root: &Path, venv: &Path) -> Result<VenvInfo, ProvisionError>;
    async fn validate(
        &self,
        root: &Path,
        source: &PythonSourceInput,
    ) -> Result<PythonValidationReport, ProvisionError>;
    /// Seeds pip, setuptools and wheel (plus the MSVC runtime on Windows) and checks that the
    /// runtime DLLs actually load.
    async fn seed(
        &self,
        root: &Path,
        venv: &Path,
        os: OsKind,
        ctx: &StepContext,
    ) -> Result<(), ProvisionError>;
    /// Removes an app-owned environment directory after checking it is inside its authorized
    /// location.
    async fn remove_environment(&self, env: &PythonEnvironment) -> Result<(), ProvisionError>;
}

pub use crate::shared::Retained;

/// A requirements file read once through a guarded node location. Adapters must take the
/// requirements from `text` rather than reopening `path`, and resolve relative `-r`/`-c` includes
/// and local paths against `directory`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardedRequirements {
    /// The logical file, one of `InstallSpec::requirement_files`, that `text` stands for.
    pub path: PathBuf,
    pub directory: PathBuf,
    pub text: Arc<[u8]>,
    pub retained: Retained,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeRequirementSet {
    pub node_id: domain::custom_nodes::NodeId,
    pub name: domain::shared::model_values::SafeFileName,
    /// The exact source and current content, from `InstalledNode::dependency_identity`.
    pub source_identity: String,
    pub file: Option<GuardedRequirements>,
    pub extra: Vec<String>,
}

#[async_trait]
pub trait NodeRequirementSource: Send + Sync {
    /// Retains the execution lease during scans and in every returned guarded input.
    async fn requirement_sets(
        &self,
        root: &Path,
        retained: Retained,
    ) -> Result<Vec<NodeRequirementSet>, ProvisionError>;
    async fn record_outcome(
        &self,
        environment: &EnvId,
        set: &NodeRequirementSet,
        state: domain::custom_nodes::DependencyState,
    ) -> Result<(), ProvisionError>;
    async fn needs_repair(
        &self,
        environment: &EnvId,
        root: &Path,
        retained: Retained,
    ) -> Result<bool, ProvisionError>;
}

/// The curated custom nodes offered by first-run setup (FR-SETUP-9), in installer order.
pub trait CuratedNodeCatalog: Send + Sync {
    fn list(&self) -> Vec<CuratedNodeSpec>;
}

/// Why a setup-node operation stopped. The class decides whether setup pauses for the user,
/// prepares again, or must refuse to continue because the candidate environment was changed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SetupNodeError {
    /// Nothing of the candidate changed; the step can be retried or skipped.
    #[error("{0}")]
    NotApplied(String),
    /// The reviewed plan is missing or no longer matches; prepare again.
    #[error("the reviewed plan is no longer valid")]
    PlanStale,
    /// The decision does not cover the exact review.
    #[error("explicit consent does not cover this review")]
    ConsentRequired,
    /// Files or dependencies of the candidate were changed and failed. Skip cannot hide this.
    #[error("{0}")]
    Damaged(String),
    #[error("protected packages could not be restored or verified: {0}")]
    PinDrift(String),
    #[error("the operation was cancelled")]
    Cancelled,
}

/// Custom-node installation against the setup candidate, implemented by the node service. It is
/// injected next to the environment dependencies so the two contexts stay free of a construction
/// cycle. Every call runs under the lease the setup run already holds and resolves the target
/// environment from the persisted run, never from caller-supplied paths.
#[async_trait]
pub trait SetupNodes: Send + Sync {
    /// Prepares a reviewable plan for the exact catalog commit and requires the resolved source
    /// to equal the entry.
    async fn prepare(
        &self,
        run: &RunId,
        entry: &CuratedNodeSpec,
        ctx: &StepContext,
        lease: &mut dyn MaintenanceLease,
    ) -> Result<PlanReview, SetupNodeError>;

    /// Commits one reviewed plan. Setup never runs `install.py`, never builds from source and
    /// never remembers trust, whatever the decision says.
    async fn commit(
        &self,
        run: &RunId,
        plan: &str,
        decision: CommitDecision,
        ctx: &StepContext,
        lease: &mut dyn MaintenanceLease,
    ) -> Result<NodeOutcome, SetupNodeError>;

    /// The reviewed plans of this run that exist in this process.
    fn pending(&self, run: &RunId) -> Result<Vec<PlanReview>, SetupNodeError>;

    /// True if publication started for the journal operation, including uncertain storage reads.
    async fn publication_started(&self, plan: &str) -> bool;

    /// True when the entry's exact source is installed and its files still match the record, with
    /// a successful dependency outcome for the run's candidate environment.
    async fn installed(
        &self,
        run: &RunId,
        entry: &CuratedNodeSpec,
        lease: &mut dyn MaintenanceLease,
    ) -> Result<bool, SetupNodeError>;
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstallSpec {
    pub requirements: Vec<String>,
    pub requirement_files: Vec<PathBuf>,
    pub index_url: Option<String>,
    pub constraint_file: Option<PathBuf>,
    pub upgrade: bool,
    pub allow_source_builds: bool,
    /// Replaces the on-disk content of one requirement file with a protected snapshot.
    pub guarded: Option<GuardedRequirements>,
    pub no_deps: bool,
}

#[async_trait]
pub trait WheelFetcher: Send + Sync {
    async fn fetch(
        &self,
        root: &Path,
        spec: &domain::environment::AcceleratorSpec,
        ctx: &StepContext,
    ) -> Result<PathBuf, ProvisionError>;
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolutionPreview {
    pub installs: Vec<InstalledPackage>,
    pub removals: Vec<InstalledPackage>,
}

#[async_trait]
pub trait PackageInstaller: Send + Sync {
    async fn install(
        &self,
        root: &Path,
        env_path: &Path,
        spec: &InstallSpec,
        ctx: &StepContext,
    ) -> Result<(), ProvisionError>;
    async fn uninstall(
        &self,
        root: &Path,
        env_path: &Path,
        names: &[DistName],
        ctx: &StepContext,
    ) -> Result<(), ProvisionError>;
    /// Resolves without mutating the environment (`--dry-run`).
    async fn preview_install(
        &self,
        root: &Path,
        env_path: &Path,
        spec: &InstallSpec,
        ctx: &StepContext,
    ) -> Result<ResolutionPreview, ProvisionError>;
    async fn list(
        &self,
        root: &Path,
        env_path: &Path,
        ctx: Option<&StepContext>,
    ) -> Result<Vec<InstalledPackage>, ProvisionError>;
    /// Writes the constraints file for an environment and returns its path.
    async fn write_constraints(
        &self,
        root: &Path,
        env: &EnvId,
        pins: &PinSet,
        extra: &[String],
    ) -> Result<PathBuf, ProvisionError>;
    async fn clean_cache(&self, root: &Path, ctx: &StepContext) -> Result<(), ProvisionError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TorchReport {
    pub version: String,
    pub cuda_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationReport {
    pub python_version: String,
    pub torch_version: String,
    pub cuda_available: bool,
    pub cuda_device: Option<String>,
    pub triton_version: Option<String>,
    pub comfy_path: Option<String>,
    /// Optional accelerator modules and whether they are importable.
    pub accelerators: Vec<(String, bool)>,
}

impl VerificationReport {
    pub fn summary(&self) -> String {
        format!(
            "Python {}, torch {} (CUDA {}), triton {}",
            self.python_version,
            self.torch_version,
            if self.cuda_available {
                "available"
            } else {
                "unavailable"
            },
            self.triton_version.as_deref().unwrap_or("absent"),
        )
    }
}

#[async_trait]
pub trait EnvVerifier: Send + Sync {
    /// Imports one optional module in a separate bounded subprocess.
    async fn verify_accelerator(
        &self,
        root: &Path,
        env_path: &Path,
        module: &str,
        ctx: &StepContext,
    ) -> Result<(), ProvisionError>;
    async fn verify_torch(
        &self,
        root: &Path,
        env_path: &Path,
        expectations: &VerifyExpectations,
        ctx: &StepContext,
    ) -> Result<TorchReport, ProvisionError>;
    /// Returns the Triton version after importing it.
    async fn verify_triton(
        &self,
        root: &Path,
        env_path: &Path,
        ctx: &StepContext,
    ) -> Result<String, ProvisionError>;
    /// The reference verification: `import sys, torch, triton, comfy`, plus a CUDA tensor
    /// operation when the profile requires CUDA.
    async fn verify_final(
        &self,
        root: &Path,
        env_path: &Path,
        expectations: &VerifyExpectations,
        ctx: &StepContext,
    ) -> Result<VerificationReport, ProvisionError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checkout {
    Absent,
    Repo {
        commit: String,
    },
    /// The folder exists but is not a git checkout; it must not be altered.
    NotGit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequirementFiles {
    pub comfyui: PathBuf,
}

#[async_trait]
pub trait ComfySource: Send + Sync {
    /// Resolves a reference once. `Latest` uses the GitHub latest-release API and peels the tag
    /// to a commit; rate limiting is a retryable failure.
    async fn resolve(
        &self,
        root: &Path,
        reference: &ComfyRef,
    ) -> Result<ResolvedGit, ProvisionError>;
    async fn inspect_checkout(&self, root: &Path) -> Result<Checkout, ProvisionError>;
    /// Clones into a staging folder, checks out the resolved commit and publishes the folder.
    async fn clone_comfyui(
        &self,
        root: &Path,
        resolved: &ResolvedGit,
        ctx: &StepContext,
    ) -> Result<(), ProvisionError>;
    async fn requirement_files(&self, root: &Path) -> Result<RequirementFiles, ProvisionError>;
    async fn resolve_manager(&self, root: &Path) -> Result<ResolvedGit, ProvisionError>;
    /// Installs ComfyUI-Manager at the approved commit and returns its requirement files.
    async fn install_manager(
        &self,
        root: &Path,
        resolved: &ResolvedGit,
        ctx: &StepContext,
    ) -> Result<Vec<PathBuf>, ProvisionError>;
}

// ---------------------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------------------

#[async_trait]
pub trait EnvironmentRepository: Send + Sync {
    async fn list(&self) -> Result<Vec<PythonEnvironment>, EnvRepoError>;
    async fn get(&self, id: &EnvId) -> Result<Option<PythonEnvironment>, EnvRepoError>;
    async fn active(&self) -> Result<Option<PythonEnvironment>, EnvRepoError>;
    /// Inserts or updates the environment record. It never changes the active selection.
    async fn save(&self, env: &PythonEnvironment) -> Result<(), EnvRepoError>;
    async fn load_components(
        &self,
        id: &EnvId,
    ) -> Result<super::model::InstalledComponents, EnvRepoError>;
    async fn save_components(
        &self,
        id: &EnvId,
        components: &super::model::InstalledComponents,
    ) -> Result<(), EnvRepoError>;
    async fn delete(&self, id: &EnvId) -> Result<(), EnvRepoError>;
}

#[async_trait]
pub trait ProvisioningRunRepository: Send + Sync {
    async fn insert(&self, run: &ProvisioningRun) -> Result<(), EnvRepoError>;
    async fn save(&self, run: &ProvisioningRun) -> Result<(), EnvRepoError>;
    async fn get(&self, id: &RunId) -> Result<Option<ProvisioningRun>, EnvRepoError>;
    /// The most recently created run, if any.
    async fn latest(&self) -> Result<Option<ProvisioningRun>, EnvRepoError>;
    /// Runs that are not in a final state or whose activation is incomplete.
    async fn unfinished(&self) -> Result<Vec<ProvisioningRun>, EnvRepoError>;
}

#[async_trait]
pub trait ActivationStore: Send + Sync {
    /// In one transaction: make `new_active` the only active environment and persist `run`
    /// (whose activation phase is `Committed`).
    async fn commit_activation(
        &self,
        run: &ProvisioningRun,
        new_active: &EnvId,
    ) -> Result<(), EnvRepoError>;
}

#[async_trait]
pub trait InstallManifestStore: Send + Sync {
    async fn read(&self, root: &Path) -> Result<ManifestState, ManifestError>;
    /// Writes and flushes a temporary file beside `install.json`, then atomically replaces it.
    async fn publish(&self, manifest: &InstallManifest) -> Result<(), ManifestError>;
}

#[async_trait]
pub trait WizardDraftStore: Send + Sync {
    async fn load(&self) -> Result<Option<String>, EnvRepoError>;
    async fn save(&self, draft: &str) -> Result<(), EnvRepoError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootSource {
    Default,
    Bootstrap,
    DebugOverride,
}

impl RootSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Bootstrap => "bootstrap",
            Self::DebugOverride => "debug_override",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootInfo {
    pub root: PathBuf,
    pub source: RootSource,
}

#[async_trait]
pub trait RootSwitcher: Send + Sync {
    fn current(&self) -> RootInfo;
    /// Validates the destination, creates its database, transfers settings and the wizard draft,
    /// then atomically publishes the bootstrap pointer. The app restarts to use the new root.
    async fn switch_root(
        &self,
        new_root: &Path,
        draft: Option<&str>,
    ) -> Result<(), RootSwitchError>;
}
