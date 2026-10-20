//! IPC DTOs of the model acquisition context. Counters are `f64` because JavaScript numbers are
//! IEEE doubles and the generated bindings must not use `bigint`. No DTO carries a token, a
//! signed transfer URL or a raw provider response.

use application::{
    models::acquisition::{
        DownloadCursor, DownloadEvent, DownloadEventKind, EnqueueOutcome, PackExport,
        PackPreviewRow, Resolution, ResolvedFile, TransferProgress,
    },
    settings::credential_tester::{CredentialTestResult, CredentialTestStatus},
};
use domain::{
    models::acquisition::{
        ConflictChoice, Destination, DownloadEngine, DownloadErrorCode, DownloadJob, DownloadSize,
        DownloadState, FileIdentity, SourceRef,
    },
    shared::model_values::Sha256,
};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::error::IpcError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DownloadStateDto {
    Queued,
    Resolving,
    Downloading,
    Verifying,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

impl From<DownloadState> for DownloadStateDto {
    fn from(value: DownloadState) -> Self {
        match value {
            DownloadState::Queued => Self::Queued,
            DownloadState::Resolving => Self::Resolving,
            DownloadState::Downloading => Self::Downloading,
            DownloadState::Verifying => Self::Verifying,
            DownloadState::Paused => Self::Paused,
            DownloadState::Completed => Self::Completed,
            DownloadState::Failed => Self::Failed,
            DownloadState::Cancelled => Self::Cancelled,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ProviderDto {
    Civitai,
    Huggingface,
    GoogleDrive,
    Direct,
}

impl From<&SourceRef> for ProviderDto {
    fn from(value: &SourceRef) -> Self {
        match value {
            SourceRef::CivitAi { .. } => Self::Civitai,
            SourceRef::HuggingFace { .. } => Self::Huggingface,
            SourceRef::GoogleDrive { .. } => Self::GoogleDrive,
            SourceRef::Direct { .. } => Self::Direct,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum EngineDto {
    Native,
    HfCli,
    Gdown,
}

impl From<DownloadEngine> for EngineDto {
    fn from(value: DownloadEngine) -> Self {
        match value {
            DownloadEngine::Native => Self::Native,
            DownloadEngine::HfCli => Self::HfCli,
            DownloadEngine::Gdown => Self::Gdown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SizeKindDto {
    Exact,
    Estimated,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SizeDto {
    pub kind: SizeKindDto,
    pub bytes: Option<f64>,
}

impl From<DownloadSize> for SizeDto {
    fn from(value: DownloadSize) -> Self {
        match value {
            DownloadSize::Exact(n) => Self {
                kind: SizeKindDto::Exact,
                bytes: Some(n as f64),
            },
            DownloadSize::Estimated(n) => Self {
                kind: SizeKindDto::Estimated,
                bytes: Some(n as f64),
            },
            DownloadSize::Unknown => Self {
                kind: SizeKindDto::Unknown,
                bytes: None,
            },
        }
    }
}

/// The identity of an existing file, echoed back to approve an overwrite of exactly that file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FileIdentityDto {
    pub size: f64,
    pub modified_ns: String,
    pub platform_id: String,
    pub digest: Option<String>,
}

impl From<&FileIdentity> for FileIdentityDto {
    fn from(value: &FileIdentity) -> Self {
        Self {
            size: value.size as f64,
            modified_ns: value.modified_ns.to_string(),
            platform_id: value.platform_id.clone(),
            digest: value.digest.as_ref().map(|d| d.as_str().to_owned()),
        }
    }
}

impl FileIdentityDto {
    pub fn into_domain(self) -> Result<FileIdentity, IpcError> {
        let invalid = || IpcError::new("validation", "The file identity is not valid.");
        if !self.size.is_finite() || self.size < 0.0 {
            return Err(invalid());
        }
        Ok(FileIdentity {
            size: self.size as u64,
            modified_ns: self.modified_ns.parse().map_err(|_| invalid())?,
            platform_id: self.platform_id,
            digest: self
                .digest
                .map(|d| Sha256::new(&d))
                .transpose()
                .map_err(|_| invalid())?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConflictChoiceDto {
    Skip,
    KeepBoth,
    Overwrite { approved: Option<FileIdentityDto> },
}

impl ConflictChoiceDto {
    /// `Overwrite` without an approved identity maps to `None`; the caller decides whether to
    /// complete it.
    pub fn into_domain(self) -> Result<Option<ConflictChoice>, IpcError> {
        Ok(match self {
            Self::Skip => Some(ConflictChoice::Skip),
            Self::KeepBoth => Some(ConflictChoice::KeepBoth),
            Self::Overwrite { approved } => approved
                .map(|identity| {
                    identity
                        .into_domain()
                        .map(|approved| ConflictChoice::Overwrite { approved })
                })
                .transpose()?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DestinationDto {
    pub root_id: String,
    pub category: String,
    pub subdirectory: String,
    pub filename: String,
    pub relative_path: String,
}

impl From<&Destination> for DestinationDto {
    fn from(value: &Destination) -> Self {
        Self {
            root_id: value.root_id.clone(),
            category: value.category.as_str().to_owned(),
            subdirectory: value.subdirectory.as_string(),
            filename: value.filename.as_str().to_owned(),
            relative_path: value.relative_path().as_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DownloadErrorDto {
    pub code: String,
    pub message: String,
}

/// The stable code and the user-facing sentence of a download failure.
pub fn describe(code: DownloadErrorCode) -> (&'static str, &'static str) {
    use DownloadErrorCode as E;
    match code {
        E::InvalidSource => (
            "invalid_source",
            "The link is not a supported download source.",
        ),
        E::InvalidMetadata => (
            "invalid_metadata",
            "The provider returned metadata that could not be used.",
        ),
        E::SelectionRequired => (
            "selection_required",
            "Choose which file to download or resolve the conflict first.",
        ),
        E::SelectionNotFound => (
            "selection_not_found",
            "The selected file is no longer available. Resolve the link again.",
        ),
        E::TokenRequired => (
            "token_required",
            "This download needs an access token. Add one in Settings.",
        ),
        E::InvalidToken => (
            "invalid_token",
            "The provider rejected the saved token. Check it in Settings.",
        ),
        E::EarlyAccessOrPaid => (
            "early_access_or_paid",
            "This version is paid or in early access and cannot be downloaded.",
        ),
        E::GatedLicense => (
            "gated_license",
            "This repository requires accepting its license on the provider website first.",
        ),
        E::RepositoryUnavailable => (
            "repository_unavailable",
            "The repository is private or missing. Check the link and your token.",
        ),
        E::FileNotFound => (
            "file_not_found",
            "The file was not found in the repository.",
        ),
        E::RevisionNotFound => ("revision_not_found", "The revision was not found."),
        E::NotFound => ("not_found", "The download was not found."),
        E::DriveQuota => (
            "drive_quota",
            "Google Drive refused the download because its quota is exceeded.",
        ),
        E::DrivePermission => (
            "drive_permission",
            "Google Drive denied access or the file does not exist. The file must be shared with \"Anyone with the link\".",
        ),
        E::DriveFolderUnsupported => (
            "drive_folder_unsupported",
            "Google Drive folders are not supported. Use a link to a single file.",
        ),
        E::UnexpectedConfirmationPage => (
            "unexpected_confirmation_page",
            "Google Drive answered with a page this app cannot handle.",
        ),
        E::HttpConfirmationRequired => (
            "http_confirmation_required",
            "This link is not encrypted (http). Confirm to download it anyway.",
        ),
        E::SourceRefreshRequired => (
            "source_refresh_required",
            "This signed link cannot be reused after a restart. Add the download again with a fresh link.",
        ),
        E::Network => ("network", "The network connection failed."),
        E::Service => ("service", "The provider reported a temporary error."),
        E::HttpClient => ("http_client", "The provider refused the request."),
        E::SignedUrlExpired => ("signed_url_expired", "The download link expired."),
        E::Disk => ("disk", "The file could not be written to disk."),
        E::HashMismatch => (
            "hash_mismatch",
            "The downloaded file does not match its published checksum and was discarded.",
        ),
        E::LengthMismatch => (
            "length_mismatch",
            "The downloaded file has an unexpected size and was discarded.",
        ),
        E::InvalidRange => (
            "invalid_range",
            "The server answered a resume request incorrectly.",
        ),
        E::ToolLaunch => ("tool_launch", "The download tool could not be started."),
        E::BandwidthProxyUnavailable => (
            "bandwidth_proxy_unavailable",
            "The bandwidth relay is unavailable. Check proxy settings and retry.",
        ),
        E::ToolRuntime => ("tool_runtime", "The download tool reported an error."),
        E::ProcessCleanupFailed => (
            "process_cleanup_failed",
            "The download process could not be stopped cleanly.",
        ),
        E::NativeUnsupportedUseHf => (
            "native_unsupported_use_hf",
            "The built-in engine cannot download this file. Switch to the Hugging Face CLI engine in Settings.",
        ),
        E::Conflict => ("conflict", "A file with that name already exists."),
        E::StaleDecision => (
            "stale_decision",
            "The files changed since you decided. Review the conflict again.",
        ),
        E::Containment => (
            "containment",
            "The destination is outside the allowed model folder.",
        ),
        E::TrashFailed => (
            "trash_failed",
            "The replaced file could not be moved to the Recycle Bin. It was kept as a backup.",
        ),
        E::Cancelled => ("cancelled", "The download was cancelled."),
        E::Storage => ("storage", "Download storage is unavailable."),
        E::InvalidTransition => (
            "invalid_transition",
            "The download cannot do that in its current state.",
        ),
        E::SubscriptionGap => ("subscription_gap", "Download events were missed."),
    }
}

impl From<DownloadErrorCode> for DownloadErrorDto {
    fn from(value: DownloadErrorCode) -> Self {
        let (code, message) = describe(value);
        Self {
            code: code.to_owned(),
            message: message.to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DownloadJobDto {
    pub id: String,
    pub state: DownloadStateDto,
    pub provider: ProviderDto,
    /// A non-secret description of the source.
    pub label: String,
    pub destination: DestinationDto,
    pub size: SizeDto,
    pub bytes_done: f64,
    pub requested_engine: EngineDto,
    pub effective_engine: EngineDto,
    /// True when progress is derived from partial files (Hugging Face CLI).
    pub estimated: bool,
    /// True when pausing or restarting loses the partial file (Hugging Face CLI, gdown).
    pub restarts_from_zero: bool,
    pub generation: f64,
    pub retry_count: u32,
    pub retry_at: Option<f64>,
    pub error: Option<DownloadErrorDto>,
    /// Set while the job waits for a bound conflict decision.
    pub decision_id: Option<String>,
    /// The destination file observed when the conflict prompt was raised; echoed back to
    /// approve overwriting exactly that file.
    pub conflict_current: Option<FileIdentityDto>,
    /// True while the staged file is being verified and published.
    pub finalizing: bool,
    pub expected_sha256: Option<String>,
    pub verified: bool,
    pub created_at: f64,
    pub updated_at: f64,
}

fn label(source: &SourceRef) -> String {
    match source {
        SourceRef::CivitAi {
            model_id,
            version_id,
            file_id,
            ..
        } => match (version_id, file_id, model_id) {
            (Some(v), Some(f), _) => format!("CivitAI version {v}, file {f}"),
            (Some(v), None, _) => format!("CivitAI version {v}"),
            (None, _, Some(m)) => format!("CivitAI model {m}"),
            _ => "CivitAI".to_owned(),
        },
        SourceRef::HuggingFace {
            repo,
            revision,
            path,
            ..
        } => {
            let revision: String = revision.chars().take(12).collect();
            match path {
                Some(path) => format!("{repo}@{revision}:{}", path.as_string()),
                None => format!("{repo}@{revision}"),
            }
        }
        SourceRef::GoogleDrive { file_id } => format!("Google Drive {file_id}"),
        SourceRef::Direct { locator, .. } => locator.clone(),
    }
}

impl From<&DownloadJob> for DownloadJobDto {
    fn from(job: &DownloadJob) -> Self {
        Self {
            id: job.id.as_str().to_owned(),
            state: job.state.into(),
            provider: (&job.source).into(),
            label: label(&job.source),
            destination: (&job.destination).into(),
            size: job.size.into(),
            bytes_done: job.bytes_done as f64,
            requested_engine: job.requested_engine.into(),
            effective_engine: job.effective_engine.into(),
            estimated: job.effective_engine == DownloadEngine::HfCli,
            restarts_from_zero: job.restart_from_zero(),
            generation: job.generation as f64,
            retry_count: u32::from(job.retry_count),
            retry_at: job.retry_at.map(|t| t as f64),
            error: job.error.map(Into::into),
            decision_id: job.decision_id.clone(),
            conflict_current: job.observed.as_ref().map(Into::into),
            finalizing: job.publication.is_some(),
            expected_sha256: job.expected_sha256.as_ref().map(|h| h.as_str().to_owned()),
            verified: job
                .publication
                .as_ref()
                .is_some_and(|p| p.external_verified),
            created_at: job.created_at as f64,
            updated_at: job.updated_at as f64,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProgressDto {
    pub id: String,
    pub generation: f64,
    pub bytes: f64,
    pub total: SizeDto,
    pub bytes_per_second: f64,
    pub eta_seconds: Option<f64>,
    pub estimated: bool,
}

impl ProgressDto {
    fn new(id: &str, generation: u64, progress: &TransferProgress) -> Self {
        Self {
            id: id.to_owned(),
            generation: generation as f64,
            bytes: progress.bytes as f64,
            total: progress.total.into(),
            bytes_per_second: if progress.bytes_per_second.is_finite() {
                progress.bytes_per_second
            } else {
                0.0
            },
            eta_seconds: progress.eta_seconds.map(|s| s as f64),
            estimated: progress.estimated,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DownloadCursorDto {
    pub epoch: String,
    pub sequence: f64,
}

impl From<&DownloadCursor> for DownloadCursorDto {
    fn from(value: &DownloadCursor) -> Self {
        Self {
            epoch: value.epoch.clone(),
            sequence: value.sequence as f64,
        }
    }
}

impl DownloadCursorDto {
    pub fn into_domain(self) -> Option<DownloadCursor> {
        (self.sequence.is_finite() && self.sequence >= 0.0).then_some(DownloadCursor {
            epoch: self.epoch,
            sequence: self.sequence as u64,
        })
    }
}

/// Events of one subscription. The first event is always an authoritative snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DownloadEventDto {
    #[serde(rename_all = "camelCase")]
    Snapshot {
        cursor: DownloadCursorDto,
        jobs: Vec<DownloadJobDto>,
        gap: bool,
    },
    #[serde(rename = "job:state", rename_all = "camelCase")]
    State {
        cursor: DownloadCursorDto,
        job: Box<DownloadJobDto>,
    },
    #[serde(rename = "job:progress", rename_all = "camelCase")]
    Progress {
        cursor: DownloadCursorDto,
        progress: ProgressDto,
    },
    /// The subscriber fell behind; subscribe again for a fresh snapshot.
    #[serde(rename_all = "camelCase")]
    Gap { cursor: DownloadCursorDto },
}

impl DownloadEventDto {
    pub fn from_event(event: &DownloadEvent, replay_gap: bool) -> Self {
        let cursor = DownloadCursorDto::from(&event.cursor);
        match &event.kind {
            DownloadEventKind::Snapshot(jobs) => Self::Snapshot {
                cursor,
                jobs: jobs.iter().map(Into::into).collect(),
                gap: replay_gap,
            },
            DownloadEventKind::State(job) => Self::State {
                cursor,
                job: Box::new(job.as_ref().into()),
            },
            DownloadEventKind::Progress {
                id,
                generation,
                progress,
            } => Self::Progress {
                cursor,
                progress: ProgressDto::new(id.as_str(), *generation, progress),
            },
            DownloadEventKind::Gap => Self::Gap { cursor },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedFileDto {
    pub index: u32,
    pub provider: ProviderDto,
    pub label: String,
    pub filename: String,
    pub size: SizeDto,
    pub sha256: Option<String>,
    pub suggested_category: Option<String>,
    /// The provider's preferred file; preselected when a choice is needed.
    pub primary: bool,
}

impl ResolvedFileDto {
    fn new(index: usize, file: &ResolvedFile) -> Self {
        Self {
            index: u32::try_from(index).unwrap_or(u32::MAX),
            provider: (&file.source).into(),
            label: label(&file.source),
            filename: file.filename.as_str().to_owned(),
            size: file.size.into(),
            sha256: file.sha256.as_ref().map(|h| h.as_str().to_owned()),
            suggested_category: file
                .suggested_category
                .as_ref()
                .map(|c| c.as_str().to_owned()),
            primary: file.primary,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ResolutionDto {
    /// Process-local handle for `dl_enqueue`; it carries no transfer URL.
    pub id: f64,
    pub selection_required: bool,
    pub files: Vec<ResolvedFileDto>,
}

impl From<&Resolution> for ResolutionDto {
    fn from(value: &Resolution) -> Self {
        Self {
            id: value.id as f64,
            selection_required: value.selection_required,
            files: value
                .files
                .iter()
                .enumerate()
                .map(|(index, file)| ResolvedFileDto::new(index, file))
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ResolveInput {
    pub url: String,
    /// The user confirmed that an unencrypted http link may be used.
    pub http_confirmed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueItemInput {
    pub resolution_id: f64,
    pub file_index: u32,
    pub category: String,
    pub subdirectory: String,
    pub filename: Option<String>,
    pub conflict: Option<ConflictChoiceDto>,
    /// Download again although an identical file is already in the catalog.
    pub force: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EnqueueOutcomeDto {
    #[serde(rename_all = "camelCase")]
    Queued {
        job_id: String,
    },
    #[serde(rename_all = "camelCase")]
    Duplicate {
        job_id: String,
    },
    AlreadyDownloaded {
        destination: DestinationDto,
    },
    Conflict {
        destination: DestinationDto,
        current: Option<FileIdentityDto>,
    },
    Skipped,
    Invalid {
        error: DownloadErrorDto,
    },
}

impl From<&EnqueueOutcome> for EnqueueOutcomeDto {
    fn from(value: &EnqueueOutcome) -> Self {
        match value {
            EnqueueOutcome::Queued(id) => Self::Queued {
                job_id: id.as_str().to_owned(),
            },
            EnqueueOutcome::Duplicate(id) => Self::Duplicate {
                job_id: id.as_str().to_owned(),
            },
            EnqueueOutcome::AlreadyDownloaded(destination) => Self::AlreadyDownloaded {
                destination: destination.into(),
            },
            EnqueueOutcome::Conflict {
                destination,
                current,
            } => Self::Conflict {
                destination: destination.into(),
                current: current.as_ref().map(Into::into),
            },
            EnqueueOutcome::Skipped => Self::Skipped,
            EnqueueOutcome::Invalid(code) => Self::Invalid {
                error: (*code).into(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ResumeDecisionDto {
    pub decision_id: String,
    pub choice: ConflictChoiceDto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PackSourceDto {
    /// The text of a resource list or pack index.
    Json,
    /// An https link to a resource list or pack index.
    Url,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportPackInput {
    pub source: PackSourceDto,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PackPreviewRowDto {
    pub name: String,
    pub category: String,
    pub resolution: Option<ResolutionDto>,
    pub error: Option<DownloadErrorDto>,
}

impl From<&PackPreviewRow> for PackPreviewRowDto {
    fn from(value: &PackPreviewRow) -> Self {
        Self {
            name: value.name.clone(),
            category: value.category.as_str().to_owned(),
            resolution: value.resolved.as_ref().ok().map(Into::into),
            error: value.resolved.as_ref().err().map(|e| e.0.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExportInput {
    /// Exports exactly these jobs; `None` exports every completed download.
    pub job_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PackExportDto {
    pub json: String,
    /// Jobs left out because their source cannot be reproduced from a stable link.
    pub excluded: Vec<String>,
}

impl From<&PackExport> for PackExportDto {
    fn from(value: &PackExport) -> Self {
        Self {
            json: value.json.clone(),
            excluded: value
                .excluded
                .iter()
                .map(|id| id.as_str().to_owned())
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CredentialTestResultDto {
    Valid,
    Invalid,
    Absent,
    BackendUnavailable,
    BackendLocked,
    NetworkFailure,
}

impl From<CredentialTestResult> for CredentialTestResultDto {
    fn from(value: CredentialTestResult) -> Self {
        match value {
            CredentialTestResult::Valid => Self::Valid,
            CredentialTestResult::InvalidOrInsufficientAccess => Self::Invalid,
            CredentialTestResult::Absent => Self::Absent,
            CredentialTestResult::BackendUnavailable => Self::BackendUnavailable,
            CredentialTestResult::BackendLocked => Self::BackendLocked,
            CredentialTestResult::NetworkOrServiceFailure => Self::NetworkFailure,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CredentialTestDto {
    pub result: CredentialTestResultDto,
    /// Unix seconds.
    pub tested_at: f64,
}

impl From<CredentialTestStatus> for CredentialTestDto {
    fn from(value: CredentialTestStatus) -> Self {
        Self {
            result: value.result.into(),
            tested_at: value.tested_at as f64,
        }
    }
}
