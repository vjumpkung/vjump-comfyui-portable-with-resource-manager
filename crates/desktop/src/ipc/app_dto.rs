//! IPC DTOs of the application-level commands: signed self-update and diagnostics. Byte counts
//! are `f64` because JavaScript numbers are exact up to 2^53 and no installer is that large.

use application::settings::updates::{UpdateError, UpdateOffer, UpdateProgress};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::error::IpcError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatusDto {
    /// False when this build carries no complete signed-update configuration.
    pub configured: bool,
    pub current_version: String,
}

/// A newer release for the user to review. `update_id` must be echoed back to install exactly
/// this release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateOfferDto {
    pub update_id: String,
    pub version: String,
    pub date: Option<String>,
    /// Plain text from the update server; never markup.
    pub notes: Option<String>,
}

impl From<UpdateOffer> for UpdateOfferDto {
    fn from(offer: UpdateOffer) -> Self {
        Self {
            update_id: offer.update_id,
            version: offer.version,
            date: offer.date,
            notes: offer.notes,
        }
    }
}

/// Installation progress. `failed` carries the stable error code of the failure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum UpdateProgressDto {
    #[serde(rename_all = "camelCase")]
    Downloading {
        downloaded_bytes: f64,
        total_bytes: Option<f64>,
    },
    Installing,
    Restarting,
    Failed {
        code: String,
    },
}

impl From<UpdateProgress> for UpdateProgressDto {
    fn from(progress: UpdateProgress) -> Self {
        match progress {
            UpdateProgress::Downloading { downloaded, total } => Self::Downloading {
                downloaded_bytes: downloaded as f64,
                total_bytes: total.map(|total| total as f64),
            },
            UpdateProgress::Installing => Self::Installing,
            UpdateProgress::Restarting => Self::Restarting,
            UpdateProgress::Failed(error) => Self::Failed {
                code: error.code().to_owned(),
            },
        }
    }
}

/// The redacted report and whether it reached the system clipboard. When `copied` is false the
/// UI shows `text` for manual selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CopyDiagnosticsDto {
    pub text: String,
    pub copied: bool,
}

impl From<UpdateError> for IpcError {
    fn from(error: UpdateError) -> Self {
        use application::{comfy_runtime::RuntimeBlock, shared::GateError};
        let message = match error {
            UpdateError::NotConfigured => "This build has no signed update source configured.",
            UpdateError::Stale => {
                "The reviewed update is no longer available. Check for updates again."
            }
            UpdateError::CheckFailed => "The update server could not be checked.",
            UpdateError::SignatureInvalid => {
                "The downloaded update failed signature verification and was not installed."
            }
            UpdateError::InstallFailed => {
                "The update could not be downloaded or installed. Restart the app before trying again."
            }
            UpdateError::Gate(GateError::Busy) => {
                "Another maintenance operation is running. Wait for it to finish."
            }
            UpdateError::Gate(GateError::Unavailable) => "The maintenance gate is unavailable.",
            UpdateError::Runtime(RuntimeBlock::Running) => {
                "ComfyUI from this installation is running. Stop it first."
            }
            UpdateError::Runtime(RuntimeBlock::CannotAssess) => {
                "The state of ComfyUI could not be determined, so nothing was changed."
            }
        };
        Self::new(error.code(), message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_serializes_as_the_documented_union() {
        let json = |progress: UpdateProgress| {
            serde_json::to_value(UpdateProgressDto::from(progress)).unwrap()
        };
        assert_eq!(
            json(UpdateProgress::Downloading {
                downloaded: 5,
                total: None
            }),
            serde_json::json!({ "kind": "downloading", "downloadedBytes": 5.0, "totalBytes": null })
        );
        assert_eq!(
            json(UpdateProgress::Installing),
            serde_json::json!({ "kind": "installing" })
        );
        assert_eq!(
            json(UpdateProgress::Restarting),
            serde_json::json!({ "kind": "restarting" })
        );
        assert_eq!(
            json(UpdateProgress::Failed(UpdateError::SignatureInvalid)),
            serde_json::json!({ "kind": "failed", "code": "update_signature_invalid" })
        );
    }

    #[test]
    fn every_update_error_has_its_code_and_a_sanitized_message() {
        for error in [
            UpdateError::NotConfigured,
            UpdateError::Stale,
            UpdateError::CheckFailed,
            UpdateError::SignatureInvalid,
            UpdateError::InstallFailed,
            UpdateError::Gate(application::shared::GateError::Busy),
            UpdateError::Runtime(application::comfy_runtime::RuntimeBlock::Running),
        ] {
            let ipc = IpcError::from(error);
            assert_eq!(ipc.code, error.code());
            assert!(!ipc.message.is_empty());
        }
    }
}
