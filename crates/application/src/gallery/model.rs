use super::ports::GalleryError;
use domain::{
    gallery::{GalleryEntryKind, GalleryLocation, GalleryMediaKind},
    shared::model_values::SafeFileName,
};

/// Opaque, adapter-defined fingerprint of an entry. Equal revisions mean the entry is unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryRevision(pub String);

/// An entry as the caller last saw it. Adapters refuse it as stale when the revision differs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GalleryEntryRef {
    pub location: GalleryLocation,
    pub revision: EntryRevision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GalleryEntry {
    pub location: GalleryLocation,
    pub revision: EntryRevision,
    pub name: SafeFileName,
    pub kind: GalleryEntryKind,
    /// `Other` for directories.
    pub media: GalleryMediaKind,
    /// `None` for directories.
    pub size_bytes: Option<u64>,
    pub modified_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GalleryWarningCode {
    /// The name cannot be addressed safely and is not listed.
    UnsafeName,
    /// A symbolic link or reparse point; links are never followed.
    Link,
    /// Neither a regular file nor a directory.
    Special,
    Unreadable,
    /// An unrecognized entry in the app's staging folder was left in place.
    StagingLeftover,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GalleryWarning {
    /// A lossy display name; never a path.
    pub name: Option<String>,
    pub code: GalleryWarningCode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GalleryListing {
    pub directory: GalleryLocation,
    pub entries: Vec<GalleryEntry>,
    pub warnings: Vec<GalleryWarning>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferMode {
    Copy,
    Move,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemOutcome {
    Succeeded,
    /// A move onto the folder the entry already lives in.
    Unchanged,
    /// The copy is complete and verified, but part or all of the moved source remains.
    SourceRetained,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemResult {
    pub source: GalleryLocation,
    pub destination: Option<GalleryLocation>,
    pub outcome: ItemOutcome,
    pub error: Option<GalleryError>,
}

/// One planned recycle. The fingerprint is adapter-defined; for a directory it covers the whole
/// tree, so a change anywhere below invalidates the plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletePlanItem {
    pub entry: GalleryEntryRef,
    pub kind: GalleryEntryKind,
    pub bytes: u64,
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletePlan {
    pub id: String,
    pub items: Vec<DeletePlanItem>,
    pub total_bytes: u64,
}
