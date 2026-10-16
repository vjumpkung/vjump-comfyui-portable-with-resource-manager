use async_trait::async_trait;
use domain::{
    models::{
        acquisition::FileIdentity,
        library::{ModelCategory, ModelFile, ModelLocation, RenameError},
    },
    shared::model_values::{SafeFileName, Sha256},
};

/// Why a library operation did not complete. Maps to a sanitized IPC error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryErrorCode {
    /// Another process holds the file open (Windows sharing violation, EBUSY).
    Locked,
    TargetExists,
    OutsideRoot,
    /// The operating system refused access (distinct from a sharing violation).
    AccessDenied,
    TrashFailed,
    AmbiguousSidecar,
    /// A download or another operation owns the destination.
    BusyReserved,
    NotFound,
    /// The file changed since it was listed or planned.
    Stale,
    InvalidName,
    ExtensionChanged,
    Unchanged,
    /// A multi-file operation could not be fully completed or restored.
    RecoveryRequired,
    /// The copy succeeded but the source could not be removed; both files remain.
    SourceRetained,
    StaleDecision,
    Storage,
    Io,
    Network,
    CredentialRequired,
    InvalidResponse,
    InvalidRequest,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("library operation failed: {0:?}")]
pub struct LibraryError(pub LibraryErrorCode);
impl From<LibraryErrorCode> for LibraryError {
    fn from(value: LibraryErrorCode) -> Self {
        Self(value)
    }
}
impl From<RenameError> for LibraryError {
    fn from(value: RenameError) -> Self {
        Self(match value {
            RenameError::Invalid => LibraryErrorCode::InvalidName,
            RenameError::Unchanged => LibraryErrorCode::Unchanged,
            RenameError::ExtensionChanged => LibraryErrorCode::ExtensionChanged,
        })
    }
}
pub type Result<T> = std::result::Result<T, LibraryError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootState {
    Ready,
    Missing,
    Inaccessible,
}
/// One authorized model root. The default root holds category folders; an extra root is a
/// single category folder from `extra_model_paths.yaml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootInfo {
    pub id: String,
    pub label: String,
    pub path: String,
    pub category: Option<ModelCategory>,
    pub state: RootState,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatcherState {
    Native,
    /// Native watching is unavailable; a 30 second poll runs instead.
    Polling,
    Unavailable,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanWarningCode {
    RootMissing,
    RootInaccessible,
    InvalidName,
    UnsafeLink,
    Unreadable,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanWarning {
    pub root_id: Option<String>,
    pub code: ScanWarningCode,
    pub detail: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryStatus {
    pub sequence: u64,
    pub roots: Vec<RootInfo>,
    pub warnings: Vec<ScanWarning>,
    pub scanning: bool,
    pub watcher: WatcherState,
}
#[derive(Debug, Clone, PartialEq)]
pub struct LibrarySnapshot {
    pub revision: u64,
    pub models: Vec<ModelFile>,
    pub status: LibraryStatus,
}

#[async_trait]
pub trait ModelCatalog: Send + Sync {
    async fn list(&self) -> Result<Vec<ModelFile>>;
    async fn revision(&self) -> Result<u64>;
    async fn get(&self, location: &ModelLocation) -> Result<Option<ModelFile>>;
    /// Whether `location` is a valid place for a model (a registered root; below a category
    /// folder for the default root).
    async fn accepts(&self, location: &ModelLocation) -> bool;
    /// Moves the row and its hash-cache entry, and finishes the journal entry `op_id`, in one
    /// transaction. Tolerates a row that is already at `to`.
    async fn apply_operation(
        &self,
        op_id: &str,
        from: &ModelLocation,
        to: &ModelLocation,
    ) -> Result<()>;
    /// Removes the row after the file was deleted.
    async fn remove(&self, location: &ModelLocation) -> Result<()>;
    async fn cached_hash(
        &self,
        location: &ModelLocation,
        identity: &FileIdentity,
    ) -> Result<Option<Sha256>>;
    /// Stores the digest while `identity` still matches the row.
    async fn record_hash(
        &self,
        location: &ModelLocation,
        identity: &FileIdentity,
        sha256: &Sha256,
    ) -> Result<()>;
    /// Stores the CivitAI version of the file while `identity` still matches the row.
    async fn record_identification(
        &self,
        location: &ModelLocation,
        identity: &FileIdentity,
        sha256: &Sha256,
        civitai_version_id: u64,
    ) -> Result<()>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relocated {
    Renamed,
    /// Copied across volumes, verified, and the source removed.
    Copied,
    /// Copied and verified but the source could not be removed.
    CopiedSourceRetained,
}
#[async_trait]
pub trait ModelFiles: Send + Sync {
    /// The live identity, or `None` when the file does not exist. Fails with `OutsideRoot`
    /// when the location cannot be reached without leaving its root.
    async fn inspect(&self, location: &ModelLocation) -> Result<Option<FileIdentity>>;
    /// Names of the regular files in a directory (empty path = the root itself).
    async fn list_directory(&self, directory: &ModelLocation) -> Result<Vec<SafeFileName>>;
    /// Fails with `Locked` when another process holds the file open without delete sharing.
    async fn check_unlocked(&self, location: &ModelLocation) -> Result<()>;
    /// No-replace placement. A cross-volume placement copies into a private temporary file in the
    /// destination directory, flushes and verifies it, publishes it, then removes the source.
    async fn relocate(
        &self,
        from: &ModelLocation,
        to: &ModelLocation,
        expected: Option<&FileIdentity>,
        scratch: &str,
    ) -> Result<Relocated>;
    async fn trash(&self, location: &ModelLocation, expected: &FileIdentity) -> Result<()>;
    async fn delete_permanently(
        &self,
        location: &ModelLocation,
        expected: &FileIdentity,
    ) -> Result<()>;
    /// Streams the file through SHA-256 off the caller's thread. The identity is checked before
    /// and after; a changed input is rejected with `Stale`.
    async fn hash(&self, location: &ModelLocation, expected: &FileIdentity) -> Result<Sha256>;
    /// Exclusive creation. With `replace`, an existing file is replaced.
    async fn write_sidecar(
        &self,
        location: &ModelLocation,
        bytes: &[u8],
        replace: bool,
    ) -> Result<()>;
    /// The sidecar file content (bounded) or `None`.
    async fn read_sidecar(&self, location: &ModelLocation, limit: usize)
    -> Result<Option<Vec<u8>>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationKind {
    Rename,
    Move,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationPhase {
    Intent,
    RecoveryRequired,
}
/// One file of a rename or move group: the model itself or one of its sidecars.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationMember {
    pub from: ModelLocation,
    pub to: ModelLocation,
    /// The recorded intermediate name of a case-only rename.
    pub via: Option<ModelLocation>,
    pub model: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationRecord {
    pub id: String,
    pub kind: OperationKind,
    pub phase: OperationPhase,
    pub members: Vec<OperationMember>,
    pub created_at: String,
}
impl OperationRecord {
    pub fn locations(&self) -> Vec<ModelLocation> {
        let mut all = Vec::new();
        for member in &self.members {
            all.push(member.from.clone());
            all.push(member.to.clone());
            all.extend(member.via.clone());
        }
        all
    }
}
#[async_trait]
pub trait OperationJournal: Send + Sync {
    async fn begin(&self, record: &OperationRecord) -> Result<()>;
    async fn set_phase(&self, id: &str, phase: OperationPhase) -> Result<()>;
    /// Deletes the entry (the operation was completed or fully compensated).
    async fn complete(&self, id: &str) -> Result<()>;
    async fn unfinished(&self) -> Result<Vec<OperationRecord>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CivitaiVersion {
    pub version_id: u64,
    pub model_id: u64,
    pub model_name: String,
    pub version_name: String,
    pub base_model: Option<String>,
    pub trigger_words: Vec<String>,
    /// Transient; never persisted.
    pub image_url: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CivitaiMatch {
    pub version: CivitaiVersion,
    /// The curated `.civitai.info` document: ids, names, base model and trigger words only.
    pub info_json: Vec<u8>,
}
#[async_trait]
pub trait CivitaiLookup: Send + Sync {
    /// `None` for an unknown hash (HTTP 404), which is a normal "not identified" result.
    async fn by_hash(&self, sha256: &Sha256) -> Result<Option<CivitaiMatch>>;
    /// A real PNG re-encoded from the first still image, or `None` when there is none.
    async fn preview_png(&self, version: &CivitaiVersion) -> Result<Option<Vec<u8>>>;
}

/// What the details pane shows, read from the sidecars.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModelDetails {
    pub civitai: Option<CivitaiVersion>,
    pub preview_png: Option<Vec<u8>>,
}
#[async_trait]
pub trait ModelDetailsReader: Send + Sync {
    async fn read(&self, location: &ModelLocation) -> Result<ModelDetails>;
}

/// The scan supervisor seen from the application: status, manual rescan and change notices.
#[async_trait]
pub trait LibraryControl: Send + Sync {
    /// Requests a full scan and waits for it to finish.
    async fn rescan(&self) -> Result<()>;
    fn status(&self) -> LibraryStatus;
    /// The catalog changed outside a scan (rename, delete, identification).
    fn changed(&self);
}
