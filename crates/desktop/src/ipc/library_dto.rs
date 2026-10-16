//! IPC DTOs of the model library context. Counters are `f64` because JavaScript numbers are IEEE
//! doubles. Locations are a root id plus a relative path; no DTO carries an absolute path of a
//! model file, a token or a signed transfer URL.

use application::models::library::{
    CivitaiVersion, DeleteItem, DeleteOutcome, DeletePlan, DeleteReport, IdentifyOutcome,
    LibraryErrorCode, LibrarySnapshot, LibraryStatus, ModelDetails, OperationKind, OperationPhase,
    OperationRecord, PreviewOutcome, RootInfo, RootState, ScanWarning, ScanWarningCode,
    WatcherState,
};
use domain::{
    models::library::{ModelFile, ModelLocation},
    shared::model_values::RelativeModelPath,
};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::error::IpcError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelLocationDto {
    pub root_id: String,
    /// Relative to the root, with `/` separators.
    pub path: String,
}
impl From<&ModelLocation> for ModelLocationDto {
    fn from(value: &ModelLocation) -> Self {
        Self {
            root_id: value.root_id.clone(),
            path: value.path.as_string(),
        }
    }
}
impl ModelLocationDto {
    pub fn into_domain(self) -> Result<ModelLocation, IpcError> {
        if self.root_id.is_empty() || self.root_id.len() > 100 || self.path.is_empty() {
            return Err(IpcError::new(
                "validation",
                "The model location is not valid.",
            ));
        }
        let path = RelativeModelPath::new(&self.path)
            .map_err(|_| IpcError::new("validation", "The model location is not valid."))?;
        Ok(ModelLocation::new(self.root_id, path))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RootStateDto {
    Ready,
    Missing,
    Inaccessible,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RootDto {
    pub id: String,
    pub label: String,
    pub path: String,
    /// Set for an extra folder bound to one category; the default root has none.
    pub category: Option<String>,
    pub state: RootStateDto,
}
impl From<&RootInfo> for RootDto {
    fn from(value: &RootInfo) -> Self {
        Self {
            id: value.id.clone(),
            label: value.label.clone(),
            path: value.path.clone(),
            category: value.category.as_ref().map(|c| c.as_str().to_owned()),
            state: match value.state {
                RootState::Ready => RootStateDto::Ready,
                RootState::Missing => RootStateDto::Missing,
                RootState::Inaccessible => RootStateDto::Inaccessible,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum WatcherStateDto {
    Native,
    Polling,
    Unavailable,
}
impl From<WatcherState> for WatcherStateDto {
    fn from(value: WatcherState) -> Self {
        match value {
            WatcherState::Native => Self::Native,
            WatcherState::Polling => Self::Polling,
            WatcherState::Unavailable => Self::Unavailable,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ScanWarningCodeDto {
    RootMissing,
    RootInaccessible,
    InvalidName,
    UnsafeLink,
    Unreadable,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScanWarningDto {
    pub root_id: Option<String>,
    pub code: ScanWarningCodeDto,
    pub detail: String,
}
impl From<&ScanWarning> for ScanWarningDto {
    fn from(value: &ScanWarning) -> Self {
        Self {
            root_id: value.root_id.clone(),
            code: match value.code {
                ScanWarningCode::RootMissing => ScanWarningCodeDto::RootMissing,
                ScanWarningCode::RootInaccessible => ScanWarningCodeDto::RootInaccessible,
                ScanWarningCode::InvalidName => ScanWarningCodeDto::InvalidName,
                ScanWarningCode::UnsafeLink => ScanWarningCodeDto::UnsafeLink,
                ScanWarningCode::Unreadable => ScanWarningCodeDto::Unreadable,
            },
            detail: value.detail.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LibraryStatusDto {
    /// Increases with every change; subscribers refetch the list when it moves.
    pub sequence: f64,
    pub roots: Vec<RootDto>,
    pub warnings: Vec<ScanWarningDto>,
    pub scanning: bool,
    pub watcher: WatcherStateDto,
}
impl From<&LibraryStatus> for LibraryStatusDto {
    fn from(value: &LibraryStatus) -> Self {
        Self {
            sequence: value.sequence as f64,
            roots: value.roots.iter().map(Into::into).collect(),
            warnings: value.warnings.iter().map(Into::into).collect(),
            scanning: value.scanning,
            watcher: value.watcher.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SidecarsDto {
    pub preview: bool,
    pub civitai_info: bool,
    pub json: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelEntryDto {
    pub location: ModelLocationDto,
    pub category: String,
    pub name: String,
    /// The folder below the category, empty when the file sits directly in it.
    pub subfolder: String,
    pub size: f64,
    /// Milliseconds since the Unix epoch.
    pub modified_ms: f64,
    pub sha256: Option<String>,
    /// The download job that produced the file, if any.
    pub source_job_id: Option<String>,
    pub civitai_version_id: Option<f64>,
    pub sidecars: SidecarsDto,
}
impl ModelEntryDto {
    fn new(model: &ModelFile, fixed_category: bool) -> Self {
        let directory = model.location.directory();
        let skip = usize::from(!fixed_category);
        let subfolder = directory
            .components()
            .iter()
            .skip(skip)
            .map(|c| c.as_str())
            .collect::<Vec<_>>()
            .join("/");
        Self {
            location: (&model.location).into(),
            category: model.category.as_str().to_owned(),
            name: model.name().to_owned(),
            subfolder,
            size: model.size as f64,
            modified_ms: (model.modified_ns / 1_000_000) as f64,
            sha256: model.sha256.as_ref().map(|h| h.as_str().to_owned()),
            source_job_id: model.source_job_id.as_ref().map(|j| j.as_str().to_owned()),
            civitai_version_id: model.civitai_version_id.map(|v| v as f64),
            sidecars: SidecarsDto {
                preview: model.sidecars.preview,
                civitai_info: model.sidecars.civitai_info,
                json: model.sidecars.json,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySnapshotDto {
    pub revision: f64,
    pub models: Vec<ModelEntryDto>,
    pub status: LibraryStatusDto,
}
impl From<&LibrarySnapshot> for LibrarySnapshotDto {
    fn from(value: &LibrarySnapshot) -> Self {
        let fixed = |root: &str| {
            value
                .status
                .roots
                .iter()
                .any(|r| r.id == root && r.category.is_some())
        };
        Self {
            revision: value.revision as f64,
            models: value
                .models
                .iter()
                .map(|m| ModelEntryDto::new(m, fixed(&m.location.root_id)))
                .collect(),
            status: (&value.status).into(),
        }
    }
}

/// Events of one subscription. The first event is an authoritative snapshot of the status.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ModelEventDto {
    #[serde(rename_all = "camelCase")]
    Snapshot {
        epoch: String,
        status: LibraryStatusDto,
    },
    /// Coalesced change notice: refetch `model_list`. Latest-value semantics mean no gap
    /// can occur, however slowly the subscriber reads.
    #[serde(rename = "models:changed", rename_all = "camelCase")]
    Changed {
        epoch: String,
        status: LibraryStatusDto,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CivitaiInfoDto {
    pub version_id: f64,
    pub model_id: f64,
    pub model_name: String,
    pub version_name: String,
    pub base_model: Option<String>,
    pub trigger_words: Vec<String>,
}
impl From<&CivitaiVersion> for CivitaiInfoDto {
    fn from(value: &CivitaiVersion) -> Self {
        Self {
            version_id: value.version_id as f64,
            model_id: value.model_id as f64,
            model_name: value.model_name.clone(),
            version_name: value.version_name.clone(),
            base_model: value.base_model.clone(),
            trigger_words: value.trigger_words.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelDetailsDto {
    pub civitai: Option<CivitaiInfoDto>,
    /// `data:image/png;base64,...` of the `.preview.png` sidecar (at most 10 MiB).
    pub preview_data_url: Option<String>,
}
impl From<&ModelDetails> for ModelDetailsDto {
    fn from(value: &ModelDetails) -> Self {
        Self {
            civitai: value.civitai.as_ref().map(Into::into),
            preview_data_url: value
                .preview_png
                .as_deref()
                .map(|bytes| format!("data:image/png;base64,{}", base64(bytes))),
        }
    }
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RenameInput {
    pub location: ModelLocationDto,
    pub new_name: String,
    /// The user saw the extension-change warning and accepted it.
    pub allow_extension_change: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MoveInput {
    pub location: ModelLocationDto,
    pub target_root_id: String,
    /// The target folder relative to the target root (for the default root: category first).
    pub target_directory: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IdentifyInput {
    pub location: ModelLocationDto,
    /// Replace existing `.civitai.info` / `.preview.png` files.
    pub replace_existing: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DeleteItemDto {
    pub location: ModelLocationDto,
    pub name: String,
    pub size: f64,
}
impl From<&DeleteItem> for DeleteItemDto {
    fn from(value: &DeleteItem) -> Self {
        Self {
            location: (&value.location).into(),
            name: value.name.clone(),
            size: value.size as f64,
        }
    }
}
/// A confirmation bound to the current identities of the listed files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DeletePlanDto {
    pub id: String,
    /// True for the separate confirmation that permanently deletes files the trash refused.
    pub permanent: bool,
    pub total_bytes: f64,
    pub items: Vec<DeleteItemDto>,
}
impl From<&DeletePlan> for DeletePlanDto {
    fn from(value: &DeletePlan) -> Self {
        Self {
            id: value.id.clone(),
            permanent: value.kind == application::models::library::DeleteKind::Permanent,
            total_bytes: value.total_bytes as f64,
            items: value.items.iter().map(Into::into).collect(),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DeleteOutcomeDto {
    Deleted,
    Locked,
    TrashFailed,
    Stale,
    Missing,
    Busy,
    Failed { error: LibraryErrorDto },
}
impl From<DeleteOutcome> for DeleteOutcomeDto {
    fn from(value: DeleteOutcome) -> Self {
        match value {
            DeleteOutcome::Deleted => Self::Deleted,
            DeleteOutcome::Locked => Self::Locked,
            DeleteOutcome::TrashFailed => Self::TrashFailed,
            DeleteOutcome::Stale => Self::Stale,
            DeleteOutcome::Missing => Self::Missing,
            DeleteOutcome::Busy => Self::Busy,
            DeleteOutcome::Failed(code) => Self::Failed { error: code.into() },
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DeleteResultDto {
    pub location: ModelLocationDto,
    pub outcome: DeleteOutcomeDto,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DeleteReportDto {
    pub results: Vec<DeleteResultDto>,
    /// Present when the trash refused some files; running it needs a separate confirmation.
    pub permanent_plan: Option<DeletePlanDto>,
}
impl From<&DeleteReport> for DeleteReportDto {
    fn from(value: &DeleteReport) -> Self {
        Self {
            results: value
                .results
                .iter()
                .map(|r| DeleteResultDto {
                    location: (&r.location).into(),
                    outcome: r.outcome.into(),
                })
                .collect(),
            permanent_plan: value.permanent_plan.as_ref().map(Into::into),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PreviewDto {
    Written,
    Missing,
    Failed,
}
impl From<PreviewOutcome> for PreviewDto {
    fn from(value: PreviewOutcome) -> Self {
        match value {
            PreviewOutcome::Written => Self::Written,
            PreviewOutcome::Missing => Self::Missing,
            PreviewOutcome::Failed(_) => Self::Failed,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IdentifyOutcomeDto {
    Identified {
        version: CivitaiInfoDto,
        sha256: String,
        preview: PreviewDto,
    },
    NotIdentified {
        sha256: String,
    },
    /// Sidecars exist; nothing was written. Repeat with `replaceExisting` after confirmation.
    SidecarsExist {
        version: CivitaiInfoDto,
        sha256: String,
    },
}
impl From<&IdentifyOutcome> for IdentifyOutcomeDto {
    fn from(value: &IdentifyOutcome) -> Self {
        match value {
            IdentifyOutcome::Identified {
                version,
                sha256,
                preview,
            } => Self::Identified {
                version: version.into(),
                sha256: sha256.as_str().to_owned(),
                preview: (*preview).into(),
            },
            IdentifyOutcome::NotIdentified { sha256 } => Self::NotIdentified {
                sha256: sha256.as_str().to_owned(),
            },
            IdentifyOutcome::SidecarsExist { version, sha256 } => Self::SidecarsExist {
                version: version.into(),
                sha256: sha256.as_str().to_owned(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OperationStatusDto {
    pub id: String,
    pub kind: String,
    /// `intent` (will be finished at the next rescan) or `recovery_required`.
    pub phase: String,
    pub files: u32,
}
impl From<&OperationRecord> for OperationStatusDto {
    fn from(value: &OperationRecord) -> Self {
        Self {
            id: value.id.clone(),
            kind: match value.kind {
                OperationKind::Rename => "rename",
                OperationKind::Move => "move",
            }
            .to_owned(),
            phase: match value.phase {
                OperationPhase::Intent => "intent",
                OperationPhase::RecoveryRequired => "recovery_required",
            }
            .to_owned(),
            files: u32::try_from(value.members.len()).unwrap_or(u32::MAX),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LibraryErrorDto {
    pub code: String,
    pub message: String,
}
impl From<LibraryErrorCode> for LibraryErrorDto {
    fn from(value: LibraryErrorCode) -> Self {
        let (code, message) = describe(value);
        Self {
            code: code.to_owned(),
            message: message.to_owned(),
        }
    }
}

/// Sanitized text for every failure: no paths, no raw provider text.
pub fn describe(code: LibraryErrorCode) -> (&'static str, &'static str) {
    use LibraryErrorCode as E;
    match code {
        E::Locked => (
            "model_locked",
            "Another program is using the file. Close it, or stop ComfyUI, then retry.",
        ),
        E::TargetExists => (
            "model_target_exists",
            "A file with that name already exists.",
        ),
        E::OutsideRoot => (
            "model_outside_root",
            "The location is outside the allowed model folders.",
        ),
        E::AccessDenied => (
            "model_access_denied",
            "The operating system denied access to the file.",
        ),
        E::TrashFailed => (
            "model_trash_failed",
            "The file could not be moved to the Recycle Bin or Trash. It was not deleted.",
        ),
        E::AmbiguousSidecar => (
            "model_ambiguous_sidecar",
            "Another model file shares this name, so its metadata files cannot be moved safely.",
        ),
        E::BusyReserved => (
            "model_busy",
            "A download or another operation is using that location.",
        ),
        E::NotFound => (
            "model_not_found",
            "The model file no longer exists. Rescan the library.",
        ),
        E::Stale => (
            "model_changed",
            "The file changed since it was listed. Rescan the library and try again.",
        ),
        E::InvalidName => ("model_invalid_name", "The file name is not valid."),
        E::ExtensionChanged => (
            "model_extension_changed",
            "The file extension would change. Confirm the extension change to continue.",
        ),
        E::Unchanged => ("model_unchanged", "Nothing would change."),
        E::RecoveryRequired => (
            "model_recovery_required",
            "An earlier file operation could not be completed or restored. Rescan to retry recovery.",
        ),
        E::SourceRetained => (
            "model_source_retained",
            "The file was copied but the original could not be removed, so both copies remain.",
        ),
        E::StaleDecision => (
            "model_stale_decision",
            "The files changed since you confirmed. Review the list again.",
        ),
        E::Storage => ("model_storage", "Library storage is unavailable."),
        E::Io => ("model_io", "The file operation failed."),
        E::Network => ("model_network", "CivitAI could not be reached."),
        E::CredentialRequired => (
            "model_credential_required",
            "CivitAI asked for a valid API token. Add one in Settings.",
        ),
        E::InvalidResponse => ("model_invalid_response", "CivitAI answered unexpectedly."),
        E::InvalidRequest => ("model_invalid_request", "The request is not valid."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard_alphabet_with_padding() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn locations_reject_traversal_and_empty_values() {
        for path in ["", "../x", "a/../b", "/abs", "C:/x", "a\\b"] {
            let dto = ModelLocationDto {
                root_id: "models".into(),
                path: path.into(),
            };
            assert!(dto.into_domain().is_err(), "{path:?}");
        }
        let ok = ModelLocationDto {
            root_id: "models".into(),
            path: "checkpoints/ไทย.safetensors".into(),
        };
        assert!(ok.into_domain().is_ok());
    }
}
