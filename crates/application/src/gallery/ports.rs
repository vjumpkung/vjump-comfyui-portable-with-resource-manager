use super::model::*;
use crate::shared::{ExplorerTarget, GateError, Retained};
use async_trait::async_trait;
use domain::{gallery::GalleryLocation, shared::model_values::SafeFileName};
use std::path::Path;

/// Why a gallery operation did not complete. Maps to a sanitized IPC error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GalleryErrorCode {
    /// The ComfyUI folder does not exist yet.
    NotInstalled,
    NotFound,
    /// The entry changed since it was listed or planned.
    Stale,
    TargetExists,
    InvalidName,
    InvalidRequest,
    /// Another process holds the file open (Windows sharing violation, EBUSY).
    Locked,
    /// The operating system refused access, or the path crosses a link.
    AccessDenied,
    TrashFailed,
    Unsupported,
    /// The copy succeeded but the moved source could not be fully removed.
    SourceRetained,
    /// Another maintenance operation is running.
    Busy,
    Io,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("gallery operation failed: {0:?}")]
pub struct GalleryError(pub GalleryErrorCode);
impl From<GalleryErrorCode> for GalleryError {
    fn from(value: GalleryErrorCode) -> Self {
        Self(value)
    }
}
impl From<GateError> for GalleryError {
    fn from(value: GateError) -> Self {
        Self(match value {
            GateError::Busy => GalleryErrorCode::Busy,
            GateError::Unavailable => GalleryErrorCode::Io,
        })
    }
}
pub type Result<T> = std::result::Result<T, GalleryError>;

/// The file system below `<root>/ComfyUI/input` and `<root>/ComfyUI/output`. Adapters resolve
/// every location through pinned, non-following parents, never create `ComfyUI`, and never
/// permanently delete. Callers screen same-folder moves before asking the adapter.
#[async_trait]
pub trait GalleryFiles: Send + Sync {
    async fn list(&self, root: &Path, directory: &GalleryLocation) -> Result<GalleryListing>;
    /// Renames within the same folder without replacing anything.
    async fn rename(
        &self,
        root: &Path,
        entry: &GalleryEntryRef,
        new_name: &SafeFileName,
    ) -> Result<GalleryLocation>;
    /// Copies or moves one entry into an existing folder, suffixing the name on conflict.
    /// `operation` is a fresh id used to name the owned staging directory. `retained` is the
    /// maintenance gate's protection; the adapter keeps it alive until its blocking file work
    /// has returned, even when the caller stops waiting.
    async fn transfer(
        &self,
        root: &Path,
        entry: &GalleryEntryRef,
        destination: &GalleryLocation,
        mode: TransferMode,
        operation: &str,
        retained: Retained,
    ) -> ItemResult;
    async fn plan_delete(&self, root: &Path, entry: &GalleryEntryRef) -> Result<DeletePlanItem>;
    /// Moves a planned entry to the recycle bin after checking its fingerprint again.
    async fn trash(&self, root: &Path, item: &DeletePlanItem) -> Result<()>;
    async fn open_media(
        &self,
        root: &Path,
        location: &GalleryLocation,
    ) -> Result<Box<dyn GalleryMedia>>;
    async fn explorer_target(
        &self,
        root: &Path,
        location: &GalleryLocation,
    ) -> Result<ExplorerTarget>;
    /// Removes proven operation-owned staging copies in both areas; unrecognized contents stay
    /// and are reported.
    async fn cleanup_staging(&self, root: &Path) -> Vec<GalleryWarning>;
}

/// A pinned regular file opened for preview reads. `len` is the size at open time; callers
/// read by offset and never ask whether it is empty.
#[allow(clippy::len_without_is_empty)]
pub trait GalleryMedia: Send {
    fn len(&self) -> u64;
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> std::io::Result<usize>;
}
