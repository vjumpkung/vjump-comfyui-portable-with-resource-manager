use crate::{
    models::library::ModelCategory,
    shared::model_values::{JobId, RelativeModelPath, SafeFileName, Sha256},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoType {
    Model,
    Dataset,
    Space,
}
impl RepoType {
    pub fn singular(self) -> &'static str {
        match self {
            Self::Model => "model",
            Self::Dataset => "dataset",
            Self::Space => "space",
        }
    }
    pub fn plural(self) -> &'static str {
        match self {
            Self::Model => "models",
            Self::Dataset => "datasets",
            Self::Space => "spaces",
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CivitFilters {
    pub file_type: Option<String>,
    pub format: Option<String>,
    pub size: Option<String>,
    pub fp: Option<String>,
}
/// Contains only stable locators. Direct transfer secrets live behind ingress_id in infrastructure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceRef {
    CivitAi {
        model_id: Option<u64>,
        version_id: Option<u64>,
        file_id: Option<u64>,
        filters: CivitFilters,
    },
    HuggingFace {
        repo_type: RepoType,
        repo: String,
        revision: String,
        path: Option<RelativeModelPath>,
    },
    GoogleDrive {
        file_id: String,
    },
    Direct {
        locator: String,
        ingress_id: Option<u64>,
        requires_source_refresh: bool,
        insecure_http_confirmed: bool,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProviderIdentity(pub String);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadEngine {
    Native,
    HfCli,
    Gdown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadState {
    Queued,
    Resolving,
    Downloading,
    Verifying,
    Paused,
    Completed,
    Failed,
    Cancelled,
}
impl DownloadState {
    pub fn active(self) -> bool {
        !matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Resolving => "resolving",
            Self::Downloading => "downloading",
            Self::Verifying => "verifying",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadSize {
    Exact(u64),
    Estimated(u64),
    Unknown,
}
impl DownloadSize {
    pub fn bytes(self) -> Option<u64> {
        match self {
            Self::Exact(n) | Self::Estimated(n) => Some(n),
            Self::Unknown => None,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictChoice {
    Overwrite { approved: FileIdentity },
    KeepBoth,
    Skip,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileIdentity {
    pub size: u64,
    pub modified_ns: u128,
    pub platform_id: String,
    pub digest: Option<Sha256>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destination {
    pub root_id: String,
    pub category: ModelCategory,
    pub subdirectory: RelativeModelPath,
    pub filename: SafeFileName,
}
impl Destination {
    pub fn relative_path(&self) -> RelativeModelPath {
        RelativeModelPath::new(self.category.as_str())
            .expect("validated category")
            .join_path(&self.subdirectory)
            .join(self.filename.clone())
    }
    pub fn reservation_key(&self) -> String {
        format!(
            "{}:{}",
            self.root_id,
            self.relative_path().as_string().to_lowercase()
        )
    }
}
impl RelativeModelPath {
    fn join_path(&self, other: &Self) -> Self {
        let mut result = self.clone();
        for c in other.components() {
            result = result.join(c.clone());
        }
        result
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResumeValidator {
    StrongEtag(String),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinalizePhase {
    Verified,
    Published,
    Cataloged,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicationIntent {
    pub phase: FinalizePhase,
    pub staged_key: String,
    pub staged_identity: FileIdentity,
    pub digest: Sha256,
    pub external_verified: bool,
    pub backup_name: SafeFileName,
    pub published_identity: Option<FileIdentity>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopIntent {
    Pause,
    Cancel,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadJob {
    pub id: JobId,
    pub source: SourceRef,
    pub identity: ProviderIdentity,
    pub destination: Destination,
    pub state: DownloadState,
    pub requested_engine: DownloadEngine,
    pub effective_engine: DownloadEngine,
    pub generation: u64,
    pub size: DownloadSize,
    pub expected_sha256: Option<Sha256>,
    pub bytes_done: u64,
    pub validator: Option<ResumeValidator>,
    pub retry_count: u8,
    pub retry_at: Option<u64>,
    pub stop_intent: Option<StopIntent>,
    pub error: Option<DownloadErrorCode>,
    pub conflict: Option<ConflictChoice>,
    pub decision_id: Option<String>,
    /// The destination file observed when the conflict prompt was raised. The UI echoes it back
    /// to approve overwriting exactly that file.
    pub observed: Option<FileIdentity>,
    pub publication: Option<PublicationIntent>,
    pub created_at: u64,
    pub updated_at: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadErrorCode {
    InvalidSource,
    InvalidMetadata,
    SelectionRequired,
    SelectionNotFound,
    TokenRequired,
    InvalidToken,
    EarlyAccessOrPaid,
    GatedLicense,
    RepositoryUnavailable,
    FileNotFound,
    RevisionNotFound,
    NotFound,
    DriveQuota,
    DrivePermission,
    DriveFolderUnsupported,
    UnexpectedConfirmationPage,
    HttpConfirmationRequired,
    SourceRefreshRequired,
    Network,
    Service,
    HttpClient,
    SignedUrlExpired,
    Disk,
    HashMismatch,
    LengthMismatch,
    InvalidRange,
    ToolLaunch,
    BandwidthProxyUnavailable,
    ToolRuntime,
    ProcessCleanupFailed,
    NativeUnsupportedUseHf,
    Conflict,
    StaleDecision,
    Containment,
    TrashFailed,
    Cancelled,
    Storage,
    InvalidTransition,
    SubscriptionGap,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryAction {
    RetryAfter(u64),
    ReResolve,
    Fatal,
}
pub fn retry_action(code: DownloadErrorCode, retries: u8, refreshed: bool) -> RetryAction {
    match code {
        DownloadErrorCode::SignedUrlExpired if !refreshed => RetryAction::ReResolve,
        DownloadErrorCode::Network | DownloadErrorCode::Service if retries < 3 => {
            RetryAction::RetryAfter(1 << retries)
        }
        _ => RetryAction::Fatal,
    }
}
impl DownloadJob {
    pub fn transition(&mut self, next: DownloadState) -> Result<(), DownloadErrorCode> {
        use DownloadState::*;
        let valid = matches!(
            (self.state, next),
            (Queued, Resolving)
                | (Resolving, Downloading)
                | (Downloading, Verifying)
                | (Verifying, Completed)
                | (Paused, Queued)
                | (Failed, Queued)
        ) || (self.state.active() && matches!(next, Paused | Failed | Cancelled));
        if !valid || (self.publication.is_some() && matches!(next, Cancelled)) {
            return Err(DownloadErrorCode::InvalidTransition);
        }
        self.state = next;
        Ok(())
    }
    pub fn restart_from_zero(&self) -> bool {
        matches!(
            self.effective_engine,
            DownloadEngine::HfCli | DownloadEngine::Gdown
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retry_budget_is_initial_plus_three() {
        for n in 0..3 {
            assert_eq!(
                retry_action(DownloadErrorCode::Network, n, false),
                RetryAction::RetryAfter(1 << n)
            );
        }
        assert_eq!(
            retry_action(DownloadErrorCode::Network, 3, false),
            RetryAction::Fatal
        );
        assert_eq!(
            retry_action(DownloadErrorCode::SignedUrlExpired, 0, true),
            RetryAction::Fatal
        );
    }
    fn job(engine: DownloadEngine) -> DownloadJob {
        DownloadJob {
            id: JobId::new("job").unwrap(),
            source: SourceRef::Direct {
                locator: "https://example.test/a.bin".into(),
                ingress_id: None,
                requires_source_refresh: false,
                insecure_http_confirmed: false,
            },
            identity: ProviderIdentity("direct:job".into()),
            destination: Destination {
                root_id: "models".into(),
                category: ModelCategory::new("checkpoints").unwrap(),
                subdirectory: RelativeModelPath::default(),
                filename: SafeFileName::new("a.bin").unwrap(),
            },
            state: DownloadState::Queued,
            requested_engine: engine,
            effective_engine: engine,
            generation: 0,
            size: DownloadSize::Unknown,
            expected_sha256: None,
            bytes_done: 0,
            validator: None,
            retry_count: 0,
            retry_at: None,
            stop_intent: None,
            error: None,
            conflict: None,
            decision_id: None,
            observed: None,
            publication: None,
            created_at: 1,
            updated_at: 1,
        }
    }
    #[test]
    fn tool_engines_restart_from_zero_and_native_does_not() {
        assert!(job(DownloadEngine::HfCli).restart_from_zero());
        assert!(job(DownloadEngine::Gdown).restart_from_zero());
        assert!(!job(DownloadEngine::Native).restart_from_zero());
    }
}
