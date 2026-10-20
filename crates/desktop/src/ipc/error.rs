use application::{
    comfy_runtime::{RuntimeBlock, RuntimeError},
    environment::{EnvRepoError, EnvironmentAppError, ProvisionError, RootSwitchError},
    models::{acquisition::DownloadError, library::LibraryError},
    settings::{CredentialStoreError, SettingsAppError, SettingsRepoError},
    shared::GateError,
};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::state::RecoveryInfo;

/// Sanitized error crossing the IPC boundary: no paths, secrets or raw backend text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct IpcError {
    pub code: String,
    pub message: String,
}

impl IpcError {
    pub(crate) fn new(code: &str, message: &str) -> Self {
        Self {
            code: code.to_owned(),
            message: message.to_owned(),
        }
    }

    /// The error of every command while the app runs without a usable install root.
    pub(crate) fn recovery(info: &RecoveryInfo) -> Self {
        Self::new(
            match info.code.as_str() {
                "root_missing" => "root_missing",
                "rebuild_required" => "rebuild_required",
                _ => "recovery_required",
            },
            &info.message,
        )
    }
}

impl From<SettingsAppError> for IpcError {
    fn from(error: SettingsAppError) -> Self {
        match error {
            SettingsAppError::Validation(error) => Self::new("validation", &error.to_string()),
            SettingsAppError::Repository(SettingsRepoError::Unavailable) => {
                Self::new("settings_unavailable", "Settings storage is unavailable.")
            }
            SettingsAppError::Repository(SettingsRepoError::InvalidPersisted(_)) => Self::new(
                "settings_invalid_persisted",
                "Stored settings are invalid and were not changed.",
            ),
            SettingsAppError::Credential(error) => error.into(),
        }
    }
}

impl From<CredentialStoreError> for IpcError {
    fn from(error: CredentialStoreError) -> Self {
        match error {
            CredentialStoreError::Unavailable => Self::new(
                "credential_unavailable",
                "Credential storage is unavailable.",
            ),
            CredentialStoreError::Locked => {
                Self::new("credential_locked", "Credential storage is locked.")
            }
            CredentialStoreError::UnknownPending | CredentialStoreError::KindMismatch => {
                Self::new("credential_rejected", "The credential could not be saved.")
            }
            CredentialStoreError::DeleteFailed => Self::new(
                "credential_delete_failed",
                "The stored credential could not be deleted.",
            ),
        }
    }
}

impl From<EnvironmentAppError> for IpcError {
    fn from(error: EnvironmentAppError) -> Self {
        use EnvironmentAppError as E;
        match error {
            E::Domain(error) => Self::new("validation", &error.to_string()),
            E::Repository(EnvRepoError::Unavailable) => Self::new(
                "environment_storage_unavailable",
                "Environment storage is unavailable.",
            ),
            E::Repository(EnvRepoError::Corrupt) => Self::new(
                "environment_storage_corrupt",
                "Stored environment data is invalid and was not changed.",
            ),
            E::Repository(EnvRepoError::Conflict) => Self::new(
                "environment_storage_conflict",
                "The environment data changed in the meantime. Try again.",
            ),
            E::Gate(GateError::Busy) => Self::new(
                "maintenance_busy",
                "Another maintenance operation is running. Wait for it to finish.",
            ),
            E::Gate(GateError::Unavailable) => Self::new(
                "maintenance_unavailable",
                "The maintenance gate is unavailable.",
            ),
            E::Runtime(RuntimeBlock::Running) => Self::new(
                "comfyui_running",
                "ComfyUI from this installation is running. Stop it first.",
            ),
            E::Runtime(RuntimeBlock::CannotAssess) => Self::new(
                "comfyui_state_unknown",
                "The state of ComfyUI could not be determined, so nothing was changed.",
            ),
            E::Provision(ProvisionError::Cancelled) => {
                Self::new("cancelled", "The operation was cancelled.")
            }
            E::Provision(ProvisionError::PinDrift(_)) => Self::new(
                "pin_drift",
                "Protected packages could not be restored or verified.",
            ),
            E::Provision(error) => Self::new("operation_failed", &describe(&error)),
            E::Manifest(_) => Self::new(
                "manifest_unavailable",
                "install.json could not be read or written.",
            ),
            E::RootSwitch(RootSwitchError::FixedByOverride) => Self::new(
                "root_fixed_by_override",
                "The install root is fixed by a debug override.",
            ),
            E::RootSwitch(RootSwitchError::InvalidDestination(reason)) => Self::new(
                "root_invalid",
                &format!("The selected folder cannot be used: {reason}."),
            ),
            E::RootSwitch(RootSwitchError::Failed) => Self::new(
                "root_switch_failed",
                "The install root could not be switched.",
            ),
            E::RootSwitch(RootSwitchError::Missing) => Self::new(
                "root_missing",
                "The selected installation folder is missing.",
            ),
            E::RootSwitch(RootSwitchError::ComfyUiRunning) => Self::new(
                "comfyui_running",
                "Stop ComfyUI before changing the installation.",
            ),
            E::RootSwitch(RootSwitchError::ComfyUiStateUnknown) => Self::new(
                "comfyui_state_unknown",
                "ComfyUI process ownership could not be established.",
            ),
            E::RootSwitch(RootSwitchError::MaintenanceBusy) => {
                Self::new("maintenance_busy", "Maintenance is in progress.")
            }
            E::RootSwitch(RootSwitchError::RelocationRecoveryRequired)
            | E::Relocation(application::environment::RelocationError::RecoveryRequired) => {
                Self::new(
                    "relocation_recovery_required",
                    "Interrupted publication requires recovery before relocation.",
                )
            }
            E::Relocation(application::environment::RelocationError::Storage) => Self::new(
                "environment_storage_unavailable",
                "Relocation storage is unavailable.",
            ),
            E::RebuildRequired => Self::new(
                "rebuild_required",
                "The installation requires a guided rebuild.",
            ),
            E::RebuildCheckoutMissing => Self::new(
                "rebuild_checkout_missing",
                "The existing ComfyUI checkout is missing.",
            ),
            E::RebuildCheckoutInvalid => Self::new(
                "rebuild_checkout_invalid",
                "The existing ComfyUI checkout is unsafe or is not Git.",
            ),
            E::AcceleratorSelectionInvalid => Self::new(
                "accelerator_selection_invalid",
                "Accelerator ids must be unique and belong to the selected profile.",
            ),
            E::CuratedSelectionInvalid => Self::new(
                "curated_selection_invalid",
                "Curated node ids must be unique, belong to the catalog and be chosen for a first run.",
            ),
            E::ThirdPartyConsentInvalid => Self::new(
                "third_party_consent_invalid",
                "Third-party consent can only be given for a first-run setup.",
            ),
            E::ProfileRevisionUnavailable => Self::new(
                "profile_revision_unavailable",
                "The recorded setup profile revision is unavailable.",
            ),
            E::ProfileNotFound => Self::new("profile_not_found", "Unknown setup profile."),
            E::ProfileDisabled(reasons) => {
                let text: Vec<String> = reasons.iter().map(ToString::to_string).collect();
                Self::new(
                    "profile_disabled",
                    &format!("This profile is not available: {}.", text.join("; ")),
                )
            }
            E::SetupInProgress => {
                Self::new("setup_in_progress", "A setup run is already in progress.")
            }
            E::RunNotFound => Self::new("run_not_found", "Unknown setup run."),
            E::NotRetryable => Self::new(
                "not_retryable",
                "The run cannot be retried in its current state.",
            ),
            E::StepNotSkippable => Self::new(
                "step_not_skippable",
                "Only a failed optional step can be skipped.",
            ),
            E::NotCancellable => Self::new(
                "not_cancellable",
                "The run cannot be cancelled in its current state.",
            ),
            E::NotRunnable => Self::new(
                "not_runnable",
                "The run cannot be started in its current state.",
            ),
            E::InvalidRunId | E::InvalidStep => {
                Self::new("invalid_identifier", "The identifier is not valid.")
            }
            E::PythonSourceRejected(reason) => Self::new(
                "python_source_rejected",
                &format!("The Python source was rejected: {reason}."),
            ),
            E::RefChangeNotAllowed => Self::new(
                "ref_change_not_allowed",
                "A re-setup keeps the existing ComfyUI checkout. Updating it is a separate action.",
            ),
            E::RootChangeNotAllowed => Self::new(
                "root_change_not_allowed",
                "The install root cannot be changed after setup.",
            ),
            E::RecoveryRequired => Self::new(
                "recovery_required",
                "install.json or the bootstrap pointer needs recovery before setup can continue.",
            ),
            E::EnvironmentNotFound => Self::new("environment_not_found", "Unknown environment."),
            E::NoActiveEnvironment => {
                Self::new("no_active_environment", "No environment is active.")
            }
            E::EnvironmentNotUsable => Self::new(
                "environment_not_usable",
                "The environment is not in a state this operation accepts.",
            ),
            E::EnvironmentInUse => Self::new(
                "environment_in_use",
                "A setup run still targets this environment.",
            ),
            E::PlanNotFound => Self::new("plan_not_found", "Unknown or expired package plan."),
            E::PlanStale => Self::new(
                "plan_stale",
                "The environment changed after the plan was made. Preview the change again.",
            ),
            E::ConfirmationMismatch => Self::new(
                "confirmation_mismatch",
                "The confirmation does not match the protected package changes of the plan.",
            ),
            E::PackageSpecInvalid => {
                Self::new("package_spec_invalid", "The package request is not valid.")
            }
            E::ResolutionFailed(reason) => Self::new(
                "resolution_failed",
                &format!("The packages could not be resolved: {reason}"),
            ),
            E::PackageOperationFailed => Self::new(
                "package_operation_failed",
                "The package operation failed and the environment needs repair.",
            ),
            E::PinDrift => Self::new(
                "pin_drift",
                "Protected packages changed unexpectedly and the environment needs repair.",
            ),
        }
    }
}

impl From<RootSwitchError> for IpcError {
    fn from(error: RootSwitchError) -> Self {
        EnvironmentAppError::RootSwitch(error).into()
    }
}

fn describe(error: &ProvisionError) -> String {
    match error {
        ProvisionError::Cancelled => "The operation was cancelled.".into(),
        ProvisionError::PinDrift(_) => {
            "Protected packages could not be restored or verified.".into()
        }
        ProvisionError::CommandFailed(failure) => match failure.exit_code {
            Some(code) => format!("A command failed with exit code {code}."),
            None => "A command failed.".into(),
        },
        ProvisionError::Retryable(message) | ProvisionError::Failed(message) => message.clone(),
        ProvisionError::ConsentRequired(_) => "Explicit consent is required.".into(),
    }
}

impl From<RuntimeError> for IpcError {
    fn from(error: RuntimeError) -> Self {
        match error {
            RuntimeError::Busy => Self::new(
                "maintenance_busy",
                "Another maintenance operation is running. Wait for it to finish.",
            ),
            RuntimeError::NeedsStop => Self::new(
                "comfyui_running",
                "ComfyUI from this installation is running. Stop it first.",
            ),
            RuntimeError::NodeRecoveryRequired => Self::new(
                "node_recovery_required",
                "interrupted custom-node work must be resolved before launch.",
            ),
            RuntimeError::NodeRescanFailed => Self::new(
                "node_rescan_failed",
                "ComfyUI stopped, but custom nodes could not be rescanned; retry Rescan.",
            ),
            RuntimeError::EnvironmentNotUsable => Self::new(
                "environment_not_usable",
                "A verified active environment is required. Repair the selected environment first.",
            ),
            RuntimeError::RecoveryRequired(message) => {
                Self::new("comfy_recovery_required", &message)
            }
            RuntimeError::PortConflict { .. } => {
                Self::new("port_conflict", "The requested port is already in use.")
            }
            RuntimeError::Launch(error) => Self::new("validation", &error.to_string()),
            RuntimeError::UpdateRefused { reason, .. } => {
                Self::new("update_refused", &reason.to_string())
            }
            RuntimeError::Storage => Self::new(
                "comfy_storage_unavailable",
                "ComfyUI runtime storage is unavailable.",
            ),
            RuntimeError::InvalidRecord => Self::new(
                "comfy_invalid_record",
                "Saved ComfyUI runtime configuration is invalid and was not changed.",
            ),
            RuntimeError::Cancelled => Self::new("cancelled", "The operation was cancelled."),
            RuntimeError::Failed(message) => Self::new("comfy_failed", &message),
            RuntimeError::ExtraPathsRejected(message) => {
                Self::new("extra_model_paths_invalid", &message)
            }
        }
    }
}

impl From<DownloadError> for IpcError {
    fn from(error: DownloadError) -> Self {
        let (code, message) = super::download_dto::describe(error.0);
        Self::new(code, message)
    }
}

impl From<LibraryError> for IpcError {
    fn from(error: LibraryError) -> Self {
        let (code, message) = super::library_dto::describe(error.0);
        Self::new(code, message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_node_failures_have_fixed_ipc_codes_and_messages() {
        assert_eq!(
            IpcError::from(RuntimeError::NodeRecoveryRequired),
            IpcError::new(
                "node_recovery_required",
                "interrupted custom-node work must be resolved before launch."
            )
        );
        assert_eq!(
            IpcError::from(RuntimeError::NodeRescanFailed),
            IpcError::new(
                "node_rescan_failed",
                "ComfyUI stopped, but custom nodes could not be rescanned; retry Rescan."
            )
        );
    }

    #[test]
    fn a_rejected_extra_model_path_keeps_its_message_under_a_fixed_code() {
        assert_eq!(
            IpcError::from(RuntimeError::ExtraPathsRejected(
                "Choose an absolute folder path on a local drive or network share.".into()
            )),
            IpcError::new(
                "extra_model_paths_invalid",
                "Choose an absolute folder path on a local drive or network share."
            )
        );
    }

    #[test]
    fn an_invalid_curated_selection_has_a_fixed_ipc_code() {
        assert_eq!(
            IpcError::from(EnvironmentAppError::CuratedSelectionInvalid).code,
            "curated_selection_invalid"
        );
    }

    #[test]
    fn invalid_third_party_consent_has_a_fixed_ipc_code_and_message() {
        assert_eq!(
            IpcError::from(EnvironmentAppError::ThirdPartyConsentInvalid),
            IpcError::new(
                "third_party_consent_invalid",
                "Third-party consent can only be given for a first-run setup."
            )
        );
    }
}
