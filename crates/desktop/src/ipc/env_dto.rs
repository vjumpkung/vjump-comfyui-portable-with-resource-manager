//! IPC DTOs of the environment context. Byte counts are `f64` because JavaScript numbers are
//! IEEE doubles (exact up to 9 PB) and the generated bindings must not use `bigint`.

use std::path::PathBuf;

use application::environment::{
    ApplyOutcome, BlockedFeature, CheckId, ComfyRef, EnvironmentInfo, EnvironmentSummary,
    FailureKind, InstalledPackage, PackageOperation, PackagePlan, PlanResolution, ProfileOffer,
    ProvisioningRun, PythonSourceInput, PythonValidationReport, RecoveryReason, RunId, RunOutcome,
    RunStatus, SetupAction, SetupEvent, SetupPhase, SetupRequest, SetupStatus, StepFailure, StepId,
    StepStatus, ValidationTarget,
};
use domain::environment::{
    Eligibility, EnvState, GpuProbe, HardwareObservation, IneligibleReason, OsKind, Ownership,
    PinChange, ProfileStatus, PythonSourceKind, SetupMode, SetupProfile,
};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::error::IpcError;

// ---------------------------------------------------------------------------- small enums

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SetupModeDto {
    FirstRun,
    Repair,
    ChangeTorch,
    ChangePython,
    SwitchVenv,
}

impl From<SetupModeDto> for SetupMode {
    fn from(value: SetupModeDto) -> Self {
        match value {
            SetupModeDto::FirstRun => Self::FirstRun,
            SetupModeDto::Repair => Self::Repair,
            SetupModeDto::ChangeTorch => Self::ChangeTorch,
            SetupModeDto::ChangePython => Self::ChangePython,
            SetupModeDto::SwitchVenv => Self::SwitchVenv,
        }
    }
}

impl From<SetupMode> for SetupModeDto {
    fn from(value: SetupMode) -> Self {
        match value {
            SetupMode::FirstRun => Self::FirstRun,
            SetupMode::Repair => Self::Repair,
            SetupMode::ChangeTorch => Self::ChangeTorch,
            SetupMode::ChangePython => Self::ChangePython,
            SetupMode::SwitchVenv => Self::SwitchVenv,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OsDto {
    Windows,
    Linux,
}

impl From<OsKind> for OsDto {
    fn from(value: OsKind) -> Self {
        match value {
            OsKind::Windows => Self::Windows,
            OsKind::Linux => Self::Linux,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OwnershipDto {
    AppOwned,
    Adopted,
}

impl From<Ownership> for OwnershipDto {
    fn from(value: Ownership) -> Self {
        match value {
            Ownership::AppOwned => Self::AppOwned,
            Ownership::Adopted => Self::Adopted,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum EnvStateDto {
    Provisioning,
    Verified,
    Failed,
    RepairRequired,
}

impl From<EnvState> for EnvStateDto {
    fn from(value: EnvState) -> Self {
        match value {
            EnvState::Provisioning => Self::Provisioning,
            EnvState::Verified => Self::Verified,
            EnvState::Failed => Self::Failed,
            EnvState::RepairRequired => Self::RepairRequired,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PythonSourceKindDto {
    Managed,
    CustomVenv,
    CustomInterpreter,
}

impl From<PythonSourceKind> for PythonSourceKindDto {
    fn from(value: PythonSourceKind) -> Self {
        match value {
            PythonSourceKind::Managed => Self::Managed,
            PythonSourceKind::CustomVenv => Self::CustomVenv,
            PythonSourceKind::CustomInterpreter => Self::CustomInterpreter,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RunStatusDto {
    Registered,
    Running,
    Failed,
    Cancelled,
    Succeeded,
    Interrupted,
}

impl From<RunStatus> for RunStatusDto {
    fn from(value: RunStatus) -> Self {
        match value {
            RunStatus::Registered => Self::Registered,
            RunStatus::Running => Self::Running,
            RunStatus::Failed => Self::Failed,
            RunStatus::Cancelled => Self::Cancelled,
            RunStatus::Succeeded => Self::Succeeded,
            RunStatus::Interrupted => Self::Interrupted,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum StepStatusDto {
    Pending,
    Running,
    Succeeded,
    Warning,
    Failed,
    Skipped,
    Interrupted,
}

impl From<StepStatus> for StepStatusDto {
    fn from(value: StepStatus) -> Self {
        match value {
            StepStatus::Pending => Self::Pending,
            StepStatus::Running => Self::Running,
            StepStatus::Succeeded => Self::Succeeded,
            StepStatus::Warning => Self::Warning,
            StepStatus::Failed => Self::Failed,
            StepStatus::Skipped => Self::Skipped,
            StepStatus::Interrupted => Self::Interrupted,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum StepPolicyDto {
    Required,
    Optional,
    BestEffort,
}

impl From<application::environment::StepPolicy> for StepPolicyDto {
    fn from(value: application::environment::StepPolicy) -> Self {
        match value {
            application::environment::StepPolicy::Required => Self::Required,
            application::environment::StepPolicy::Optional => Self::Optional,
            application::environment::StepPolicy::BestEffort => Self::BestEffort,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum FailureKindDto {
    Failed,
    CommandFailed,
    Retryable,
    ConsentRequired,
    Interrupted,
}

impl From<FailureKind> for FailureKindDto {
    fn from(value: FailureKind) -> Self {
        match value {
            FailureKind::Failed => Self::Failed,
            FailureKind::CommandFailed => Self::CommandFailed,
            FailureKind::Retryable => Self::Retryable,
            FailureKind::ConsentRequired => Self::ConsentRequired,
            FailureKind::Interrupted => Self::Interrupted,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SetupPhaseDto {
    NeedsSetup,
    Ready,
    RepairRequired,
    RunInProgress,
    RunFailed,
    RecoveryRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RootSourceDto {
    Default,
    Bootstrap,
    DebugOverride,
}

impl From<application::environment::RootSource> for RootSourceDto {
    fn from(value: application::environment::RootSource) -> Self {
        match value {
            application::environment::RootSource::Default => Self::Default,
            application::environment::RootSource::Bootstrap => Self::Bootstrap,
            application::environment::RootSource::DebugOverride => Self::DebugOverride,
        }
    }
}

// ----------------------------------------------------------------------------- run snapshot

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConsentDto {
    pub source: String,
    pub commit: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StepFailureDto {
    pub step: String,
    pub kind: FailureKindDto,
    pub message: String,
    pub command: Option<String>,
    pub exit_code: Option<i32>,
    pub tail: Vec<String>,
    pub consent: Option<ConsentDto>,
}

impl From<&StepFailure> for StepFailureDto {
    fn from(value: &StepFailure) -> Self {
        Self {
            step: value.step.as_str().to_owned(),
            kind: value.kind.into(),
            message: value.message.clone(),
            command: value.command.clone(),
            exit_code: value.exit_code,
            tail: value.tail.clone(),
            consent: value.consent.as_ref().map(|consent| ConsentDto {
                source: consent.source.clone(),
                commit: consent.commit.clone(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StepDto {
    pub step: String,
    pub number: u8,
    pub label: String,
    pub policy: StepPolicyDto,
    pub status: StepStatusDto,
    pub attempts: u32,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RunSnapshotDto {
    pub id: String,
    pub mode: SetupModeDto,
    pub status: RunStatusDto,
    pub profile_id: String,
    pub target_env_id: String,
    pub steps: Vec<StepDto>,
    pub failure: Option<StepFailureDto>,
    pub cancel_requested: bool,
    pub activation_phase: String,
    pub created_at: String,
    pub updated_at: String,
}

impl From<&ProvisioningRun> for RunSnapshotDto {
    fn from(run: &ProvisioningRun) -> Self {
        Self {
            id: run.id.to_string(),
            mode: run.request.mode.into(),
            status: run.status.into(),
            profile_id: run.profile.id.clone(),
            target_env_id: run.target_env.to_string(),
            steps: run
                .steps
                .iter()
                .map(|record| StepDto {
                    step: record.step.as_str().to_owned(),
                    number: record.step.number(),
                    label: record.step.label().to_owned(),
                    policy: record.step.policy().into(),
                    status: record.status.into(),
                    attempts: record.attempts,
                    message: record.message.clone(),
                })
                .collect(),
            failure: run.failure.as_ref().map(StepFailureDto::from),
            cancel_requested: run.cancel_requested,
            activation_phase: run.activation.as_str().to_owned(),
            created_at: run.created_at.clone(),
            updated_at: run.updated_at.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PinDto {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentSummaryDto {
    pub id: String,
    pub path: String,
    pub ownership: OwnershipDto,
    pub source: PythonSourceKindDto,
    pub python_version: String,
    pub profile_id: String,
    pub profile_revision: u32,
    pub state: EnvStateDto,
    pub pins: Vec<PinDto>,
    pub verification: Option<String>,
}

impl From<&EnvironmentSummary> for EnvironmentSummaryDto {
    fn from(env: &EnvironmentSummary) -> Self {
        Self {
            id: env.id.to_string(),
            path: env.path.display().to_string(),
            ownership: env.ownership.into(),
            source: env.source.into(),
            python_version: env.python_version.to_string(),
            profile_id: env.profile_id.clone(),
            profile_revision: env.profile_revision,
            state: env.state.into(),
            pins: env
                .pins
                .iter()
                .map(|(name, version)| PinDto {
                    name: name.to_string(),
                    version: version.to_owned(),
                })
                .collect(),
            verification: env.verification.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryDto {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupStatusDto {
    pub phase: SetupPhaseDto,
    pub recovery: Option<RecoveryDto>,
    pub install_root: String,
    pub root_source: RootSourceDto,
    pub active_env: Option<EnvironmentSummaryDto>,
    pub run: Option<RunSnapshotDto>,
    pub draft: Option<String>,
}

fn recovery_message(reason: RecoveryReason) -> &'static str {
    match reason {
        RecoveryReason::ManifestCorrupt => {
            "install.json is unreadable. It was not changed. Restore it from a backup or move it aside to start setup again."
        }
        RecoveryReason::ManifestNewer => {
            "install.json was written by a newer version of this app. Update the app."
        }
        RecoveryReason::ManifestMissing => {
            "install.json is missing although an environment is recorded. Restore the file; a guided rebuild arrives in a later milestone."
        }
        RecoveryReason::ManifestMismatch => {
            "install.json and the app database disagree about the active environment. Nothing was changed."
        }
    }
}

impl From<&SetupStatus> for SetupStatusDto {
    fn from(status: &SetupStatus) -> Self {
        let (phase, recovery) = match status.phase {
            SetupPhase::NeedsSetup => (SetupPhaseDto::NeedsSetup, None),
            SetupPhase::Ready => (SetupPhaseDto::Ready, None),
            SetupPhase::RepairRequired => (SetupPhaseDto::RepairRequired, None),
            SetupPhase::RunInProgress => (SetupPhaseDto::RunInProgress, None),
            SetupPhase::RunFailed => (SetupPhaseDto::RunFailed, None),
            SetupPhase::Recovery(reason) => (
                SetupPhaseDto::RecoveryRequired,
                Some(RecoveryDto {
                    code: reason.code().to_owned(),
                    message: recovery_message(reason).to_owned(),
                }),
            ),
        };
        Self {
            phase,
            recovery,
            install_root: status.root.root.display().to_string(),
            root_source: status.root.source.into(),
            active_env: status.active_env.as_ref().map(EnvironmentSummaryDto::from),
            run: status.run.as_ref().map(RunSnapshotDto::from),
            draft: status.draft.clone(),
        }
    }
}

impl SetupStatusDto {
    /// The status of an app that started without a usable install root.
    pub fn recovery(info: &crate::state::RecoveryInfo) -> Self {
        Self {
            phase: SetupPhaseDto::RecoveryRequired,
            recovery: Some(RecoveryDto {
                code: info.code.clone(),
                message: info.message.clone(),
            }),
            install_root: String::new(),
            root_source: RootSourceDto::Default,
            active_env: None,
            run: None,
            draft: None,
        }
    }
}

// ---------------------------------------------------------------------- hardware, profiles

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GpuDto {
    pub index: u32,
    pub name: String,
    pub compute_capability: Option<String>,
    pub vram_mib: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum GpuStatusDto {
    Detected,
    NoNvidiaGpu,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HardwareDto {
    pub gpu_status: GpuStatusDto,
    pub gpus: Vec<GpuDto>,
    pub driver_version: Option<String>,
    pub driver_cuda: Option<String>,
    pub os: OsDto,
    pub os_version: String,
    pub install_volume_free_bytes: Option<f64>,
    pub env_volume_free_bytes: Option<f64>,
    pub env_on_separate_volume: bool,
    pub long_paths_enabled: Option<bool>,
}

impl From<&HardwareObservation> for HardwareDto {
    fn from(hardware: &HardwareObservation) -> Self {
        let (gpu_status, gpus) = match &hardware.gpus {
            GpuProbe::Detected(gpus) => (
                GpuStatusDto::Detected,
                gpus.iter()
                    .map(|gpu| GpuDto {
                        index: gpu.index,
                        name: gpu.name.clone(),
                        compute_capability: gpu.compute_capability.map(|cc| cc.to_string()),
                        vram_mib: gpu.vram_mib.map(|mib| mib as f64),
                    })
                    .collect(),
            ),
            GpuProbe::NoNvidiaGpu => (GpuStatusDto::NoNvidiaGpu, Vec::new()),
            GpuProbe::Unavailable => (GpuStatusDto::Unavailable, Vec::new()),
        };
        Self {
            gpu_status,
            gpus,
            driver_version: hardware.driver_version.clone(),
            driver_cuda: hardware.driver_cuda.map(|version| version.to_string()),
            os: hardware.os.into(),
            os_version: hardware.os_version.clone(),
            install_volume_free_bytes: hardware.install_volume_free.map(|bytes| bytes as f64),
            env_volume_free_bytes: hardware.env_volume_free.map(|bytes| bytes as f64),
            env_on_separate_volume: hardware.env_on_separate_volume,
            long_paths_enabled: hardware.long_paths_enabled,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ReasonDto {
    pub code: String,
    pub message: String,
}

fn reason_code(reason: &IneligibleReason) -> &'static str {
    match reason {
        IneligibleReason::WrongOs { .. } => "wrong_os",
        IneligibleReason::NoNvidiaGpu => "no_nvidia_gpu",
        IneligibleReason::GpuProbeFailed => "gpu_probe_failed",
        IneligibleReason::DriverUnknown => "driver_unknown",
        IneligibleReason::DriverTooOld { .. } => "driver_too_old",
        IneligibleReason::ComputeCapabilityUnknown { .. } => "compute_capability_unknown",
        IneligibleReason::ArchTooOld { .. } => "arch_too_old",
        IneligibleReason::ArchNotInWheels { .. } => "arch_not_in_wheels",
        IneligibleReason::DiskUnknown { .. } => "disk_unknown",
        IneligibleReason::InsufficientDisk { .. } => "insufficient_disk",
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TorchDto {
    pub torch: String,
    pub torchvision: String,
    pub torchaudio: Option<String>,
    pub cuda: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ProfileStatusDto {
    Stable,
    Experimental,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProfileDto {
    pub id: String,
    pub revision: u32,
    pub label: String,
    pub status: ProfileStatusDto,
    pub recommended: bool,
    pub os: OsDto,
    pub python: String,
    pub torch: TorchDto,
    pub triton: String,
    pub min_driver_cuda: String,
    pub min_compute_capability: String,
    pub estimated_install_bytes: f64,
    pub estimate_calibrated: bool,
    pub enabled: bool,
    pub reasons: Vec<ReasonDto>,
    pub provenance: Vec<String>,
}

impl From<&ProfileOffer> for ProfileDto {
    fn from(offer: &ProfileOffer) -> Self {
        let profile: &SetupProfile = &offer.profile;
        let (enabled, reasons) = match &offer.eligibility {
            Eligibility::Eligible => (true, Vec::new()),
            Eligibility::Disabled(reasons) => (
                false,
                reasons
                    .iter()
                    .map(|reason| ReasonDto {
                        code: reason_code(reason).to_owned(),
                        message: reason.to_string(),
                    })
                    .collect(),
            ),
        };
        Self {
            id: profile.id.clone(),
            revision: profile.revision,
            label: profile.label.clone(),
            status: match profile.status {
                ProfileStatus::Stable => ProfileStatusDto::Stable,
                ProfileStatus::Experimental => ProfileStatusDto::Experimental,
            },
            recommended: profile.recommended,
            os: profile.os.into(),
            python: profile.python.to_string(),
            torch: TorchDto {
                torch: profile.torch.torch.clone(),
                torchvision: profile.torch.torchvision.clone(),
                torchaudio: profile.torch.torchaudio.clone(),
                cuda: profile.torch.cuda.as_str().to_owned(),
            },
            triton: profile.triton.requirement(),
            min_driver_cuda: profile.min_driver_cuda.to_string(),
            min_compute_capability: profile.min_cc.to_string(),
            estimated_install_bytes: profile.disk.required_on_install_volume(true) as f64,
            estimate_calibrated: profile.disk.calibrated,
            enabled,
            reasons,
            provenance: profile.provenance.clone(),
        }
    }
}

// -------------------------------------------------------------------- Python validation

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PythonSourceDto {
    Managed,
    CustomVenv { path: String },
    CustomInterpreter { path: String },
}

impl From<PythonSourceDto> for PythonSourceInput {
    fn from(value: PythonSourceDto) -> Self {
        match value {
            PythonSourceDto::Managed => Self::Managed,
            PythonSourceDto::CustomVenv { path } => Self::CustomVenv(PathBuf::from(path)),
            PythonSourceDto::CustomInterpreter { path } => {
                Self::CustomInterpreter(PathBuf::from(path))
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ValidationTargetDto {
    EmptyFolder,
    ExistingVenv,
    Interpreter,
    Rejected,
}

impl From<ValidationTarget> for ValidationTargetDto {
    fn from(value: ValidationTarget) -> Self {
        match value {
            ValidationTarget::EmptyFolder => Self::EmptyFolder,
            ValidationTarget::ExistingVenv => Self::ExistingVenv,
            ValidationTarget::Interpreter => Self::Interpreter,
            ValidationTarget::Rejected => Self::Rejected,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum BlockedFeatureDto {
    Setup,
    Triton,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CheckDto {
    pub id: String,
    pub passed: bool,
    pub detail: String,
    pub blocks: Vec<BlockedFeatureDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PythonValidationDto {
    pub target: ValidationTargetDto,
    pub resolved_path: String,
    pub python_version: Option<String>,
    pub usable: bool,
    pub checks: Vec<CheckDto>,
}

impl From<&PythonValidationReport> for PythonValidationDto {
    fn from(report: &PythonValidationReport) -> Self {
        Self {
            target: report.target.into(),
            resolved_path: report.resolved_path.display().to_string(),
            python_version: report.python_version.map(|version| version.to_string()),
            usable: report.usable(),
            checks: report
                .checks
                .iter()
                .map(|check| CheckDto {
                    id: CheckId::as_str(check.id).to_owned(),
                    passed: check.passed,
                    detail: check.detail.clone(),
                    blocks: check
                        .blocks
                        .iter()
                        .map(|feature| match feature {
                            BlockedFeature::Setup => BlockedFeatureDto::Setup,
                            BlockedFeature::Triton => BlockedFeatureDto::Triton,
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

// ------------------------------------------------------------------------ setup actions

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ComfyRefDto {
    Latest,
    Tag { value: String },
    Branch { value: String },
    KeepCurrent,
}

impl From<ComfyRefDto> for ComfyRef {
    fn from(value: ComfyRefDto) -> Self {
        match value {
            ComfyRefDto::Latest => Self::Latest,
            ComfyRefDto::Tag { value } => Self::Tag(value),
            ComfyRefDto::Branch { value } => Self::Branch(value),
            ComfyRefDto::KeepCurrent => Self::KeepCurrent,
        }
    }
}

/// The install root is never taken from the UI: it is always the app's current root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupRequestDto {
    pub mode: SetupModeDto,
    pub python_source: PythonSourceDto,
    pub profile_id: String,
    pub comfyui_ref: ComfyRefDto,
    pub install_manager: bool,
    pub clean_cache: bool,
    pub confirm_adopted_changes: bool,
    pub allow_source_builds: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SetupActionDto {
    New {
        request: SetupRequestDto,
    },
    #[serde(rename_all = "camelCase")]
    Retry {
        run_id: String,
        manager_consent: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    SkipStep {
        run_id: String,
        step: String,
    },
}

impl SetupActionDto {
    pub fn into_application(self, install_root: &std::path::Path) -> Result<SetupAction, IpcError> {
        let invalid = || IpcError::new("invalid_identifier", "The identifier is not valid.");
        Ok(match self {
            Self::New { request } => SetupAction::New(Box::new(SetupRequest {
                mode: request.mode.into(),
                install_root: install_root.to_owned(),
                python_source: request.python_source.into(),
                profile_id: request.profile_id,
                comfyui_ref: request.comfyui_ref.into(),
                install_manager: request.install_manager,
                clean_cache: request.clean_cache,
                confirm_adopted_changes: request.confirm_adopted_changes,
                allow_source_builds: request.allow_source_builds,
            })),
            Self::Retry {
                run_id,
                manager_consent,
            } => SetupAction::Retry {
                run_id: RunId::new(run_id).map_err(|_| invalid())?,
                manager_consent,
            },
            Self::SkipStep { run_id, step } => SetupAction::SkipStep {
                run_id: RunId::new(run_id).map_err(|_| invalid())?,
                step: step.parse::<StepId>().map_err(|_| invalid())?,
            },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupStartedDto {
    pub run_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RunOutcomeDto {
    Succeeded,
    NothingToRepair,
    Failed,
    Cancelled,
    /// The workflow could not run (for example the maintenance gate was unavailable).
    Error,
}

impl From<&RunOutcome> for RunOutcomeDto {
    fn from(outcome: &RunOutcome) -> Self {
        match outcome {
            RunOutcome::Succeeded => Self::Succeeded,
            RunOutcome::NothingToRepair => Self::NothingToRepair,
            RunOutcome::Failed(_) => Self::Failed,
            RunOutcome::Cancelled => Self::Cancelled,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SetupEventDto {
    #[serde(rename_all = "camelCase")]
    RunStatus {
        run_id: String,
        status: RunStatusDto,
    },
    #[serde(rename_all = "camelCase")]
    Step {
        run_id: String,
        step: String,
        status: StepStatusDto,
        attempt: u32,
        message: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Log {
        run_id: String,
        step: String,
        line: String,
    },
    #[serde(rename_all = "camelCase")]
    Progress {
        run_id: String,
        step: String,
        done_bytes: f64,
        total_bytes: Option<f64>,
    },
    #[serde(rename_all = "camelCase")]
    Failure {
        run_id: String,
        failure: StepFailureDto,
    },
    /// Always the last event of a run's channel.
    #[serde(rename_all = "camelCase")]
    Finished {
        run_id: String,
        outcome: RunOutcomeDto,
    },
}

impl From<&SetupEvent> for SetupEventDto {
    fn from(event: &SetupEvent) -> Self {
        match event {
            SetupEvent::RunStatus { run_id, status } => Self::RunStatus {
                run_id: run_id.to_string(),
                status: (*status).into(),
            },
            SetupEvent::Step {
                run_id,
                step,
                status,
                attempt,
                message,
            } => Self::Step {
                run_id: run_id.to_string(),
                step: step.as_str().to_owned(),
                status: (*status).into(),
                attempt: *attempt,
                message: message.clone(),
            },
            SetupEvent::Log { run_id, step, line } => Self::Log {
                run_id: run_id.to_string(),
                step: step.as_str().to_owned(),
                line: line.clone(),
            },
            SetupEvent::Progress {
                run_id,
                step,
                done_bytes,
                total_bytes,
            } => Self::Progress {
                run_id: run_id.to_string(),
                step: step.as_str().to_owned(),
                done_bytes: *done_bytes as f64,
                total_bytes: total_bytes.map(|bytes| bytes as f64),
            },
            SetupEvent::Failure { run_id, failure } => Self::Failure {
                run_id: run_id.to_string(),
                failure: StepFailureDto::from(failure.as_ref()),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RootSelectionDto {
    /// The new root is used after the app restarts.
    pub restart_required: bool,
}

// ---------------------------------------------------------------------------- environment

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentInfoDto {
    pub env: EnvironmentSummaryDto,
    pub profile_label: Option<String>,
    pub cuda_variant: Option<String>,
    pub torch_version: Option<String>,
    pub triton_version: Option<String>,
    pub comfyui_reference: Option<String>,
    pub comfyui_commit: Option<String>,
    pub accelerators: Vec<String>,
}

impl From<&EnvironmentInfo> for EnvironmentInfoDto {
    fn from(info: &EnvironmentInfo) -> Self {
        Self {
            env: EnvironmentSummaryDto::from(&info.env),
            profile_label: info.profile_label.clone(),
            cuda_variant: info.cuda_variant.clone(),
            torch_version: info.torch_version.clone(),
            triton_version: info.triton_version.clone(),
            comfyui_reference: info.comfyui_reference.clone(),
            comfyui_commit: info.comfyui_commit.clone(),
            accelerators: info.accelerators.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PackageDto {
    pub name: String,
    pub version: String,
}

impl From<&InstalledPackage> for PackageDto {
    fn from(package: &InstalledPackage) -> Self {
        Self {
            name: package.name.clone(),
            version: package.version.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PipOperationKindDto {
    Install,
    Upgrade,
    Uninstall,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PipOperationDto {
    pub kind: PipOperationKindDto,
    pub specs: Vec<String>,
}

impl From<PipOperationDto> for PackageOperation {
    fn from(value: PipOperationDto) -> Self {
        match value.kind {
            PipOperationKindDto::Install => Self::Install(value.specs),
            PipOperationKindDto::Upgrade => Self::Upgrade(value.specs),
            PipOperationKindDto::Uninstall => Self::Uninstall(value.specs),
        }
    }
}

impl From<&PackageOperation> for PipOperationDto {
    fn from(value: &PackageOperation) -> Self {
        match value {
            PackageOperation::Install(specs) => Self {
                kind: PipOperationKindDto::Install,
                specs: specs.clone(),
            },
            PackageOperation::Upgrade(specs) => Self {
                kind: PipOperationKindDto::Upgrade,
                specs: specs.clone(),
            },
            PackageOperation::Uninstall(specs) => Self {
                kind: PipOperationKindDto::Uninstall,
                specs: specs.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PinChangeDto {
    pub name: String,
    pub from: Option<String>,
    pub to: Option<String>,
}

impl From<&PinChange> for PinChangeDto {
    fn from(change: &PinChange) -> Self {
        Self {
            name: change.name.to_string(),
            from: change.from.clone(),
            to: change.to.clone(),
        }
    }
}

impl PinChangeDto {
    pub fn into_domain(self) -> Result<PinChange, IpcError> {
        Ok(PinChange {
            name: domain::environment::DistName::new(&self.name).map_err(|_| {
                IpcError::new("package_spec_invalid", "The package request is not valid.")
            })?,
            from: self.from,
            to: self.to,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PlanResolutionDto {
    Constrained,
    Unconstrained,
    Removal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PipPlanDto {
    pub plan_id: String,
    pub env_id: String,
    pub operation: PipOperationDto,
    pub resolution: PlanResolutionDto,
    pub installs: Vec<PackageDto>,
    pub removals: Vec<PackageDto>,
    pub protected_changes: Vec<PinChangeDto>,
    pub requires_confirmation: bool,
}

impl From<&PackagePlan> for PipPlanDto {
    fn from(plan: &PackagePlan) -> Self {
        Self {
            plan_id: plan.id.as_str().to_owned(),
            env_id: plan.env_id.to_string(),
            operation: PipOperationDto::from(&plan.operation),
            resolution: match plan.resolution {
                PlanResolution::Constrained => PlanResolutionDto::Constrained,
                PlanResolution::Unconstrained => PlanResolutionDto::Unconstrained,
                PlanResolution::Removal => PlanResolutionDto::Removal,
            },
            installs: plan.installs.iter().map(PackageDto::from).collect(),
            removals: plan.removals.iter().map(PackageDto::from).collect(),
            protected_changes: plan
                .protected_changes
                .iter()
                .map(PinChangeDto::from)
                .collect(),
            requires_confirmation: plan.requires_confirmation(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PipApplyInput {
    pub plan_id: String,
    /// Must equal the plan's protected changes when the plan requires confirmation.
    pub confirmed_changes: Vec<PinChangeDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PipApplyResultDto {
    pub env_state: EnvStateDto,
    pub protected_changes: Vec<PinChangeDto>,
}

impl From<&ApplyOutcome> for PipApplyResultDto {
    fn from(outcome: &ApplyOutcome) -> Self {
        Self {
            env_state: outcome.env_state.into(),
            protected_changes: outcome
                .protected_changes
                .iter()
                .map(PinChangeDto::from)
                .collect(),
        }
    }
}
