use domain::environment::{EnvironmentError, IneligibleReason};
use thiserror::Error;

use super::ports::{EnvRepoError, ManifestError, ProvisionError, RootSwitchError};
use crate::{comfy_runtime::RuntimeBlock, shared::GateError};

#[derive(Debug, Error)]
pub enum EnvironmentAppError {
    #[error(transparent)]
    Domain(#[from] EnvironmentError),
    #[error(transparent)]
    Repository(#[from] EnvRepoError),
    #[error(transparent)]
    Gate(#[from] GateError),
    #[error(transparent)]
    Runtime(#[from] RuntimeBlock),
    #[error(transparent)]
    Provision(#[from] ProvisionError),
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    #[error(transparent)]
    RootSwitch(#[from] RootSwitchError),
    #[error(transparent)]
    Relocation(#[from] super::RelocationError),
    #[error("the installation requires a guided rebuild")]
    RebuildRequired,
    #[error("the existing ComfyUI checkout is missing")]
    RebuildCheckoutMissing,
    #[error("the existing ComfyUI checkout is unsafe or is not a Git checkout")]
    RebuildCheckoutInvalid,
    #[error("unknown setup profile")]
    ProfileNotFound,
    #[error("accelerator ids must be unique and belong to the selected profile")]
    AcceleratorSelectionInvalid,
    #[error("curated node ids must be unique, belong to the catalog and be used for a first run")]
    CuratedSelectionInvalid,
    #[error("the recorded setup profile revision is unavailable")]
    ProfileRevisionUnavailable,
    #[error("the selected profile is not available on this machine")]
    ProfileDisabled(Vec<IneligibleReason>),
    #[error("a setup run is already in progress")]
    SetupInProgress,
    #[error("unknown setup run")]
    RunNotFound,
    #[error("the run cannot be retried in its current state")]
    NotRetryable,
    #[error("only a failed optional step can be skipped")]
    StepNotSkippable,
    #[error("the run cannot be cancelled in its current state")]
    NotCancellable,
    #[error("the run cannot be started in its current state")]
    NotRunnable,
    #[error("invalid run identifier")]
    InvalidRunId,
    #[error("invalid step identifier")]
    InvalidStep,
    #[error("the Python source was rejected: {0}")]
    PythonSourceRejected(String),
    #[error(
        "a re-setup keeps the existing ComfyUI checkout; changing its version is a separate update"
    )]
    RefChangeNotAllowed,
    #[error("the install root cannot be changed after setup")]
    RootChangeNotAllowed,
    #[error("install.json or the bootstrap pointer needs recovery before setup can continue")]
    RecoveryRequired,
    #[error("unknown environment")]
    EnvironmentNotFound,
    #[error("no environment is active")]
    NoActiveEnvironment,
    #[error("the environment is not in a state this operation accepts")]
    EnvironmentNotUsable,
    #[error("a setup run still targets this environment")]
    EnvironmentInUse,
    #[error("unknown or expired package plan")]
    PlanNotFound,
    #[error("the environment changed after the plan was made; preview the change again")]
    PlanStale,
    #[error("the confirmation does not match the protected package changes of the plan")]
    ConfirmationMismatch,
    #[error("the package request is not valid")]
    PackageSpecInvalid,
    #[error("the packages could not be resolved: {0}")]
    ResolutionFailed(String),
    #[error("the package operation failed and the environment needs repair")]
    PackageOperationFailed,
    #[error("protected packages changed unexpectedly and the environment needs repair")]
    PinDrift,
}
