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
    #[test]
    fn the_effective_engine_decides_whether_a_job_restarts_from_zero() {
        let mut fell_back = job(DownloadEngine::HfCli);
        fell_back.effective_engine = DownloadEngine::Native;
        assert!(!fell_back.restart_from_zero());
        let mut switched = job(DownloadEngine::Native);
        switched.effective_engine = DownloadEngine::Gdown;
        assert!(switched.restart_from_zero());
    }
    #[test]
    fn repo_types_name_themselves_in_singular_and_plural() {
        for (repo_type, singular, plural) in [
            (RepoType::Model, "model", "models"),
            (RepoType::Dataset, "dataset", "datasets"),
            (RepoType::Space, "space", "spaces"),
        ] {
            assert_eq!(repo_type.singular(), singular);
            assert_eq!(repo_type.plural(), plural);
        }
    }
    #[test]
    fn only_unfinished_states_are_active() {
        for state in [
            DownloadState::Queued,
            DownloadState::Resolving,
            DownloadState::Downloading,
            DownloadState::Verifying,
            DownloadState::Paused,
        ] {
            assert!(state.active(), "{}", state.as_str());
        }
        for state in [
            DownloadState::Completed,
            DownloadState::Failed,
            DownloadState::Cancelled,
        ] {
            assert!(!state.active(), "{}", state.as_str());
        }
    }
    #[test]
    fn download_states_have_stable_names() {
        for (state, name) in [
            (DownloadState::Queued, "queued"),
            (DownloadState::Resolving, "resolving"),
            (DownloadState::Downloading, "downloading"),
            (DownloadState::Verifying, "verifying"),
            (DownloadState::Paused, "paused"),
            (DownloadState::Completed, "completed"),
            (DownloadState::Failed, "failed"),
            (DownloadState::Cancelled, "cancelled"),
        ] {
            assert_eq!(state.as_str(), name);
        }
    }
    #[test]
    fn known_and_estimated_sizes_report_bytes_and_unknown_does_not() {
        assert_eq!(DownloadSize::Exact(5).bytes(), Some(5));
        assert_eq!(DownloadSize::Estimated(7).bytes(), Some(7));
        assert_eq!(DownloadSize::Unknown.bytes(), None);
    }
    fn destination(subdirectory: &str, filename: &str) -> Destination {
        Destination {
            root_id: "Root-1".into(),
            category: ModelCategory::new("checkpoints").unwrap(),
            subdirectory: RelativeModelPath::new(subdirectory).unwrap(),
            filename: SafeFileName::new(filename).unwrap(),
        }
    }
    #[test]
    fn destination_path_is_category_subdirectory_and_filename() {
        assert_eq!(
            destination("sdxl/base", "Model.safetensors")
                .relative_path()
                .as_string(),
            "checkpoints/sdxl/base/Model.safetensors"
        );
        assert_eq!(
            destination("", "Model.safetensors")
                .relative_path()
                .as_string(),
            "checkpoints/Model.safetensors"
        );
    }
    #[test]
    fn reservation_keys_are_root_scoped_and_case_insensitive() {
        assert_eq!(
            destination("SDXL", "Model.safetensors").reservation_key(),
            "Root-1:checkpoints/sdxl/model.safetensors"
        );
        assert_eq!(
            destination("sdxl", "MODEL.safetensors").reservation_key(),
            destination("SDXL", "model.safetensors").reservation_key()
        );
        let mut other_root = destination("sdxl", "model.safetensors");
        other_root.root_id = "Root-2".into();
        assert_ne!(
            other_root.reservation_key(),
            destination("sdxl", "model.safetensors").reservation_key()
        );
    }
    #[test]
    fn signed_url_expiry_is_resolved_again_once() {
        for retries in [0, 3, 255] {
            assert_eq!(
                retry_action(DownloadErrorCode::SignedUrlExpired, retries, false),
                RetryAction::ReResolve
            );
        }
        assert_eq!(
            retry_action(DownloadErrorCode::SignedUrlExpired, 0, true),
            RetryAction::Fatal
        );
    }
    #[test]
    fn service_errors_back_off_exponentially_then_give_up() {
        for (retries, delay) in [(0, 1), (1, 2), (2, 4)] {
            assert_eq!(
                retry_action(DownloadErrorCode::Service, retries, false),
                RetryAction::RetryAfter(delay)
            );
        }
        assert_eq!(
            retry_action(DownloadErrorCode::Service, 3, false),
            RetryAction::Fatal
        );
    }
    #[test]
    fn other_errors_are_never_retried() {
        for code in [
            DownloadErrorCode::InvalidToken,
            DownloadErrorCode::Disk,
            DownloadErrorCode::HashMismatch,
            DownloadErrorCode::NotFound,
        ] {
            assert_eq!(retry_action(code, 0, false), RetryAction::Fatal);
            assert_eq!(retry_action(code, 0, true), RetryAction::Fatal);
        }
    }
    fn job_in(state: DownloadState) -> DownloadJob {
        let mut value = job(DownloadEngine::Native);
        value.state = state;
        value
    }
    fn publication() -> PublicationIntent {
        PublicationIntent {
            phase: FinalizePhase::Verified,
            staged_key: "staged".into(),
            staged_identity: FileIdentity {
                size: 1,
                modified_ns: 1,
                platform_id: "id".into(),
                digest: None,
            },
            digest: Sha256::new(&"a".repeat(64)).unwrap(),
            external_verified: false,
            backup_name: SafeFileName::new("a.bak").unwrap(),
            published_identity: None,
        }
    }
    #[test]
    fn a_job_advances_through_the_download_pipeline() {
        let mut job = job_in(DownloadState::Queued);
        for next in [
            DownloadState::Resolving,
            DownloadState::Downloading,
            DownloadState::Verifying,
            DownloadState::Completed,
        ] {
            assert_eq!(job.transition(next), Ok(()));
            assert_eq!(job.state, next);
        }
    }
    #[test]
    fn paused_and_failed_jobs_can_be_queued_again() {
        for state in [DownloadState::Paused, DownloadState::Failed] {
            let mut job = job_in(state);
            assert_eq!(job.transition(DownloadState::Queued), Ok(()));
            assert_eq!(job.state, DownloadState::Queued);
        }
    }
    #[test]
    fn any_active_job_can_pause_fail_or_cancel() {
        for from in [
            DownloadState::Queued,
            DownloadState::Resolving,
            DownloadState::Downloading,
            DownloadState::Verifying,
            DownloadState::Paused,
        ] {
            for next in [
                DownloadState::Paused,
                DownloadState::Failed,
                DownloadState::Cancelled,
            ] {
                let mut job = job_in(from);
                assert_eq!(job.transition(next), Ok(()), "{from:?} -> {next:?}");
                assert_eq!(job.state, next);
            }
        }
    }
    #[test]
    fn finished_jobs_reject_transitions_and_keep_their_state() {
        let all = [
            DownloadState::Queued,
            DownloadState::Resolving,
            DownloadState::Downloading,
            DownloadState::Verifying,
            DownloadState::Paused,
            DownloadState::Completed,
            DownloadState::Failed,
            DownloadState::Cancelled,
        ];
        for from in [DownloadState::Completed, DownloadState::Cancelled] {
            for next in all {
                let mut job = job_in(from);
                assert_eq!(
                    job.transition(next),
                    Err(DownloadErrorCode::InvalidTransition),
                    "{from:?} -> {next:?}"
                );
                assert_eq!(job.state, from);
            }
        }
        for next in all.into_iter().filter(|s| *s != DownloadState::Queued) {
            let mut job = job_in(DownloadState::Failed);
            assert_eq!(
                job.transition(next),
                Err(DownloadErrorCode::InvalidTransition),
                "Failed -> {next:?}"
            );
            assert_eq!(job.state, DownloadState::Failed);
        }
    }
    #[test]
    fn pipeline_stages_cannot_be_skipped_or_reversed() {
        for (from, next) in [
            (DownloadState::Queued, DownloadState::Downloading),
            (DownloadState::Queued, DownloadState::Completed),
            (DownloadState::Resolving, DownloadState::Verifying),
            (DownloadState::Downloading, DownloadState::Completed),
            (DownloadState::Downloading, DownloadState::Queued),
            (DownloadState::Verifying, DownloadState::Downloading),
            (DownloadState::Paused, DownloadState::Downloading),
        ] {
            let mut job = job_in(from);
            assert_eq!(
                job.transition(next),
                Err(DownloadErrorCode::InvalidTransition),
                "{from:?} -> {next:?}"
            );
            assert_eq!(job.state, from);
        }
    }
    #[test]
    fn a_job_with_a_publication_cannot_be_cancelled_but_can_still_pause_or_fail() {
        let mut job = job_in(DownloadState::Verifying);
        job.publication = Some(publication());
        assert_eq!(
            job.transition(DownloadState::Cancelled),
            Err(DownloadErrorCode::InvalidTransition)
        );
        assert_eq!(job.state, DownloadState::Verifying);
        assert_eq!(job.transition(DownloadState::Paused), Ok(()));
        assert_eq!(job.transition(DownloadState::Failed), Ok(()));
        assert_eq!(job.state, DownloadState::Failed);
    }
}
