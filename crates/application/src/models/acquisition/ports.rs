use crate::shared::CancelToken;
use async_trait::async_trait;
use domain::{
    models::{acquisition::*, library::ModelCategory},
    shared::model_values::{JobId, SafeFileName, Sha256},
};
use std::{sync::Arc, time::Duration};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("download operation failed: {0:?}")]
pub struct DownloadError(pub DownloadErrorCode);
impl From<DownloadErrorCode> for DownloadError {
    fn from(value: DownloadErrorCode) -> Self {
        Self(value)
    }
}
pub type Result<T> = std::result::Result<T, DownloadError>;

#[derive(Debug, Clone)]
pub struct ResolvedFile {
    pub source: SourceRef,
    pub identity: ProviderIdentity,
    pub filename: SafeFileName,
    pub size: DownloadSize,
    pub sha256: Option<Sha256>,
    pub suggested_category: Option<ModelCategory>,
    pub primary: bool,
}
#[derive(Debug, Clone)]
pub struct Resolution {
    pub id: u64,
    pub files: Vec<ResolvedFile>,
    pub selection_required: bool,
}
#[async_trait]
pub trait SourceResolver: Send + Sync {
    async fn resolve(&self, source: &SourceRef) -> Result<Resolution>;
    async fn selected(&self, resolution_id: u64, index: usize) -> Result<ResolvedFile>;
}

#[derive(Debug, Clone)]
pub struct EnqueueRequest {
    pub job: DownloadJob,
    pub force: bool,
    pub catalog_match: Option<Destination>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnqueueOutcome {
    Queued(JobId),
    Duplicate(JobId),
    AlreadyDownloaded(Destination),
    Conflict {
        destination: Destination,
        current: Option<FileIdentity>,
    },
    Skipped,
    Invalid(DownloadErrorCode),
}
#[async_trait]
pub trait JobRepository: Send + Sync {
    /// Atomic active-identity check, destination reservation, and insertion.
    async fn enqueue(&self, request: EnqueueRequest) -> Result<EnqueueOutcome>;
    async fn get(&self, id: &JobId) -> Result<DownloadJob>;
    async fn list(&self, before: Option<u64>, limit: u32) -> Result<Vec<DownloadJob>>;
    async fn unfinished(&self) -> Result<Vec<DownloadJob>>;
    /// Compare-and-swap generation; stale worker writes are rejected.
    async fn save(&self, job: &DownloadJob, expected_generation: u64) -> Result<()>;
    /// Increments generation and claims a queued job in one transaction.
    async fn claim(&self, now: u64) -> Result<Option<DownloadJob>>;
    async fn catalog_matches(&self, hash: &Sha256) -> Result<Vec<(Destination, FileIdentity)>>;
    /// Catalog upsert and phase=Cataloged are committed together.
    async fn catalog_and_mark(&self, job: &DownloadJob) -> Result<DownloadJob>;
}

#[derive(Debug, Clone)]
pub struct TransferProgress {
    pub bytes: u64,
    pub total: DownloadSize,
    pub bytes_per_second: f64,
    pub eta_seconds: Option<u64>,
    pub estimated: bool,
}
pub trait ProgressSink: Send + Sync {
    fn sample(&self, id: &JobId, generation: u64, progress: TransferProgress);
}
/// Application owns the 4 Hz policy. Adapters use this monotonic interval.
pub const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
#[derive(Debug, Clone)]
pub struct TransferResult {
    pub staged_key: String,
    pub validator: Option<ResumeValidator>,
    pub size: u64,
}
#[async_trait]
pub trait Downloader: Send + Sync {
    async fn transfer(
        &self,
        job: &DownloadJob,
        sink: Arc<dyn ProgressSink>,
        cancel: CancelToken,
    ) -> Result<TransferResult>;
}
#[async_trait]
pub trait DownloadFiles: Send + Sync {
    async fn inspect(&self, destination: &Destination) -> Result<Option<FileIdentity>>;
    async fn verify(
        &self,
        staged_key: &str,
        expected: Option<&Sha256>,
        size: DownloadSize,
    ) -> Result<(FileIdentity, Sha256)>;
    /// Lease and containment checks surround every operation. Returns existing publication on recovery.
    async fn publish(&self, job: &DownloadJob) -> Result<FileIdentity>;
    async fn recycle_backup(&self, job: &DownloadJob) -> Result<()>;
    async fn cleanup(&self, job: &DownloadJob, preserve_native: bool) -> Result<()>;
    async fn clean_orphans(&self, keep: &[DownloadJob]) -> Result<()>;
}
#[async_trait]
pub trait DownloadControl: Send + Sync {
    /// Returns only once transfer teardown has completed. Publication is reconciled instead of cancelled.
    async fn stop(&self, id: &JobId, intent: StopIntent) -> Result<DownloadJob>;
    fn wake(&self);
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadCursor {
    pub epoch: String,
    pub sequence: u64,
}
#[derive(Debug, Clone)]
pub enum DownloadEventKind {
    Snapshot(Vec<DownloadJob>),
    State(Box<DownloadJob>),
    Progress {
        id: JobId,
        generation: u64,
        progress: TransferProgress,
    },
    Gap,
}
#[derive(Debug, Clone)]
pub struct DownloadEvent {
    pub cursor: DownloadCursor,
    pub kind: DownloadEventKind,
}
pub trait DownloadEvents: Send + Sync {
    fn state(&self, job: &DownloadJob);
}

#[derive(Debug, Clone)]
pub struct PackRow {
    pub name: String,
    pub source: SourceRef,
    pub category: ModelCategory,
}
#[derive(Debug, Clone)]
pub struct PackPreviewRow {
    pub name: String,
    pub category: ModelCategory,
    pub resolved: Result<Resolution>,
}
#[derive(Debug, Clone)]
pub struct PackExport {
    pub json: String,
    pub excluded: Vec<JobId>,
}
#[async_trait]
pub trait ModelPacks: Send + Sync {
    async fn parse(&self, json_or_ingress: &str, remote: bool) -> Result<Vec<PackRow>>;
    fn export(&self, jobs: &[DownloadJob]) -> Result<PackExport>;
}
