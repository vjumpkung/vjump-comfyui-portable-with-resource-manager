use thiserror::Error;

use super::env::EnvState;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EnvironmentError {
    #[error("invalid Python version `{0}`; an exact major.minor.patch is required")]
    InvalidPythonVersion(String),
    #[error("invalid compute capability")]
    InvalidComputeCapability,
    #[error("invalid CUDA version")]
    InvalidCudaVersion,
    #[error("unknown CUDA variant")]
    InvalidCudaVariant,
    #[error("unknown operating system")]
    InvalidOs,
    #[error("invalid distribution name")]
    InvalidDistributionName,
    #[error("invalid environment id")]
    InvalidEnvId,
    #[error("invalid setup profile: {0}")]
    InvalidProfile(&'static str),
    #[error("unknown setup mode")]
    InvalidSetupMode,
    #[error("unknown environment state")]
    InvalidEnvState,
    #[error("unknown environment ownership")]
    InvalidOwnership,
    #[error("unknown Python source")]
    InvalidPythonSource,
    #[error("environment cannot move from {from:?} to {to:?}")]
    InvalidTransition { from: EnvState, to: EnvState },
    #[error("only a verified environment can be activated")]
    NotActivatable,
    #[error("the environment is already the active environment")]
    AlreadyActive,
    #[error("adopted environments are never deleted")]
    AdoptedNotDeletable,
    #[error("the active environment cannot be deleted")]
    ActiveNotDeletable,
    #[error("rebuilding an adopted environment needs explicit confirmation")]
    AdoptedRebuildNeedsConfirmation,
    #[error("this setup mode needs an active environment")]
    ModeNeedsActiveEnvironment,
    #[error("first-run setup is not available while an environment is active")]
    FirstRunWithActiveEnvironment,
    #[error("this setup mode must keep the active environment's Python minor version")]
    ModeNeedsSamePythonMinor,
    #[error("this setup mode needs a different Python minor version")]
    ModeNeedsDifferentPythonMinor,
    #[error("the setup target resolves to the active environment")]
    TargetIsActiveEnvironment,
    #[error("this setup mode needs a custom virtual environment path")]
    ModeNeedsCustomVenv,
}
