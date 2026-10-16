use std::{path::Path, path::PathBuf, sync::Arc};

use async_trait::async_trait;
use domain::environment::{
    DistName, EnvId, HardwareObservation, OsKind, PinSet, PythonEnvironment, PythonVersion,
    SetupProfile, VerifyExpectations,
};
use thiserror::Error;

use super::model::{
    ComfyRef, ConsentRequest, InstallManifest, InstalledPackage, ManifestState, ProvisioningRun,
    PythonSourceInput, ResolvedGit, RunId, RunStatus, StepFailure, StepId, StepStatus,
};
use crate::shared::CancelToken;

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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstallSpec {
    pub requirements: Vec<String>,
    pub requirement_files: Vec<PathBuf>,
    pub index_url: Option<String>,
    pub constraint_file: Option<PathBuf>,
    pub upgrade: bool,
    pub allow_source_builds: bool,
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
    /// `requirements.txt` files of installed custom nodes.
    pub nodes: Vec<PathBuf>,
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
