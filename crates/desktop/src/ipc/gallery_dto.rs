//! IPC DTOs of the gallery context. Counters are `f64` because JavaScript numbers are IEEE
//! doubles. A location is an area plus a relative path with `/` separators; no DTO carries an
//! absolute path, and an entry revision is an opaque token the UI hands back unchanged.

use application::gallery::{
    DeletePlan, EntryRevision, GalleryEntry, GalleryEntryRef, GalleryErrorCode, GalleryListing,
    GalleryWarning, GalleryWarningCode, ItemOutcome, ItemResult,
};
use domain::{
    gallery::{GalleryArea, GalleryEntryKind, GalleryLocation, GalleryMediaKind},
    shared::model_values::RelativeModelPath,
};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::error::IpcError;

/// The longest relative path accepted from the UI, in bytes.
const MAX_PATH_BYTES: usize = 4096;
/// The longest revision token accepted from the UI, in characters.
const MAX_REVISION_CHARS: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum GalleryAreaDto {
    Input,
    Output,
}
impl From<GalleryArea> for GalleryAreaDto {
    fn from(value: GalleryArea) -> Self {
        match value {
            GalleryArea::Input => Self::Input,
            GalleryArea::Output => Self::Output,
        }
    }
}
impl From<GalleryAreaDto> for GalleryArea {
    fn from(value: GalleryAreaDto) -> Self {
        match value {
            GalleryAreaDto::Input => Self::Input,
            GalleryAreaDto::Output => Self::Output,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GalleryLocationDto {
    pub area: GalleryAreaDto,
    /// Relative to the area, with `/` separators. Empty for the area root.
    pub path: String,
}
impl From<&GalleryLocation> for GalleryLocationDto {
    fn from(value: &GalleryLocation) -> Self {
        Self {
            area: value.area().into(),
            path: value.path().as_string(),
        }
    }
}
impl GalleryLocationDto {
    pub fn into_domain(self) -> Result<GalleryLocation, IpcError> {
        let invalid = || IpcError::new("validation", "The gallery location is not valid.");
        let path = self.path.trim_matches('/');
        if path.len() > MAX_PATH_BYTES {
            return Err(invalid());
        }
        let path = RelativeModelPath::new(path).map_err(|_| invalid())?;
        GalleryLocation::new(self.area.into(), path).map_err(|_| invalid())
    }
}

/// An entry as the UI last saw it; the revision lets the backend refuse a stale reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GalleryEntryRefDto {
    pub location: GalleryLocationDto,
    pub revision: String,
}
impl From<&GalleryEntryRef> for GalleryEntryRefDto {
    fn from(value: &GalleryEntryRef) -> Self {
        Self {
            location: (&value.location).into(),
            revision: value.revision.0.clone(),
        }
    }
}
impl GalleryEntryRefDto {
    pub fn into_domain(self) -> Result<GalleryEntryRef, IpcError> {
        let length = self.revision.chars().count();
        if length == 0 || length > MAX_REVISION_CHARS {
            return Err(IpcError::new(
                "validation",
                "The gallery entry is not valid.",
            ));
        }
        Ok(GalleryEntryRef {
            location: self.location.into_domain()?,
            revision: EntryRevision(self.revision),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum GalleryEntryKindDto {
    File,
    Directory,
}
impl From<GalleryEntryKind> for GalleryEntryKindDto {
    fn from(value: GalleryEntryKind) -> Self {
        match value {
            GalleryEntryKind::File => Self::File,
            GalleryEntryKind::Directory => Self::Directory,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum GalleryMediaKindDto {
    Image,
    Video,
    #[serde(rename = "model3d")]
    Model3d,
    Other,
}
impl From<GalleryMediaKind> for GalleryMediaKindDto {
    fn from(value: GalleryMediaKind) -> Self {
        match value {
            GalleryMediaKind::Image => Self::Image,
            GalleryMediaKind::Video => Self::Video,
            GalleryMediaKind::Model3d => Self::Model3d,
            GalleryMediaKind::Other => Self::Other,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GalleryEntryDto {
    pub location: GalleryLocationDto,
    pub revision: String,
    pub name: String,
    pub kind: GalleryEntryKindDto,
    pub media: GalleryMediaKindDto,
    /// Absent for folders.
    pub size_bytes: Option<f64>,
    /// Milliseconds since the Unix epoch.
    pub modified_ms: Option<f64>,
}
impl From<&GalleryEntry> for GalleryEntryDto {
    fn from(value: &GalleryEntry) -> Self {
        Self {
            location: (&value.location).into(),
            revision: value.revision.0.clone(),
            name: value.name.as_str().to_owned(),
            kind: value.kind.into(),
            media: value.media.into(),
            size_bytes: value.size_bytes.map(|size| size as f64),
            modified_ms: value.modified_ms.map(|modified| modified as f64),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum GalleryWarningCodeDto {
    UnsafeName,
    Link,
    Special,
    Unreadable,
    StagingLeftover,
}
impl From<GalleryWarningCode> for GalleryWarningCodeDto {
    fn from(value: GalleryWarningCode) -> Self {
        match value {
            GalleryWarningCode::UnsafeName => Self::UnsafeName,
            GalleryWarningCode::Link => Self::Link,
            GalleryWarningCode::Special => Self::Special,
            GalleryWarningCode::Unreadable => Self::Unreadable,
            GalleryWarningCode::StagingLeftover => Self::StagingLeftover,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GalleryWarningDto {
    /// A display name only; never a path.
    pub name: Option<String>,
    pub code: GalleryWarningCodeDto,
}
impl From<&GalleryWarning> for GalleryWarningDto {
    fn from(value: &GalleryWarning) -> Self {
        Self {
            name: value.name.clone(),
            code: value.code.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GalleryListingDto {
    pub directory: GalleryLocationDto,
    pub entries: Vec<GalleryEntryDto>,
    pub warnings: Vec<GalleryWarningDto>,
}
impl From<&GalleryListing> for GalleryListingDto {
    fn from(value: &GalleryListing) -> Self {
        Self {
            directory: (&value.directory).into(),
            entries: value.entries.iter().map(Into::into).collect(),
            warnings: value.warnings.iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GalleryRenameInput {
    pub entry: GalleryEntryRefDto,
    pub new_name: String,
}

/// Input of a move or a copy: the entries and the existing folder they go to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GalleryTransferInput {
    pub items: Vec<GalleryEntryRefDto>,
    pub destination: GalleryLocationDto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum GalleryItemOutcomeDto {
    Succeeded,
    Unchanged,
    SourceRetained,
    Failed,
}
impl From<ItemOutcome> for GalleryItemOutcomeDto {
    fn from(value: ItemOutcome) -> Self {
        match value {
            ItemOutcome::Succeeded => Self::Succeeded,
            ItemOutcome::Unchanged => Self::Unchanged,
            ItemOutcome::SourceRetained => Self::SourceRetained,
            ItemOutcome::Failed => Self::Failed,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GalleryItemResultDto {
    pub source: GalleryLocationDto,
    pub destination: Option<GalleryLocationDto>,
    pub outcome: GalleryItemOutcomeDto,
    pub error: Option<IpcError>,
}
impl From<&ItemResult> for GalleryItemResultDto {
    fn from(value: &ItemResult) -> Self {
        Self {
            source: (&value.source).into(),
            destination: value.destination.as_ref().map(Into::into),
            outcome: value.outcome.into(),
            error: value.error.map(IpcError::from),
        }
    }
}
/// One result per entry, in request order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GalleryBatchReportDto {
    pub results: Vec<GalleryItemResultDto>,
}
impl From<&[ItemResult]> for GalleryBatchReportDto {
    fn from(value: &[ItemResult]) -> Self {
        Self {
            results: value.iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GalleryDeletePlanItemDto {
    pub entry: GalleryEntryRefDto,
    pub kind: GalleryEntryKindDto,
    /// The size of the entry; for a folder, of everything below it.
    pub bytes: f64,
}
/// A confirmation bound to the fingerprints of the listed entries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GalleryDeletePlanDto {
    pub plan_id: String,
    pub items: Vec<GalleryDeletePlanItemDto>,
    pub total_bytes: f64,
}
impl From<&DeletePlan> for GalleryDeletePlanDto {
    fn from(value: &DeletePlan) -> Self {
        Self {
            plan_id: value.id.clone(),
            items: value
                .items
                .iter()
                .map(|item| GalleryDeletePlanItemDto {
                    entry: (&item.entry).into(),
                    kind: item.kind.into(),
                    bytes: item.bytes as f64,
                })
                .collect(),
            total_bytes: value.total_bytes as f64,
        }
    }
}

/// Sanitized text for every failure: no paths, no raw OS text.
pub fn describe(code: GalleryErrorCode) -> (&'static str, &'static str) {
    use GalleryErrorCode as E;
    match code {
        E::NotInstalled => ("gallery_not_installed", "ComfyUI is not installed yet."),
        E::NotFound => ("gallery_not_found", "The file or folder no longer exists."),
        E::Stale => (
            "gallery_stale",
            "The file or folder changed. Refresh and try again.",
        ),
        E::TargetExists => (
            "gallery_target_exists",
            "An item with that name already exists.",
        ),
        E::InvalidName => ("gallery_invalid_name", "That name is not allowed."),
        E::InvalidRequest => (
            "gallery_invalid_request",
            "That operation is not allowed here.",
        ),
        E::Locked => (
            "gallery_locked",
            "Another program is using the item. Close it, or stop ComfyUI, then retry.",
        ),
        E::AccessDenied => (
            "gallery_access_denied",
            "The operating system denied access.",
        ),
        E::TrashFailed => (
            "gallery_trash_failed",
            "The item could not be moved to the Recycle Bin or Trash. It was not deleted.",
        ),
        E::Unsupported => (
            "gallery_unsupported",
            "This operation is not supported for this item.",
        ),
        E::SourceRetained => (
            "gallery_source_retained",
            "The copy succeeded but some of the original could not be removed; it was kept.",
        ),
        E::Busy => (
            "gallery_busy",
            "Another maintenance operation is running. Try again when it finishes.",
        ),
        E::Io => (
            "gallery_io",
            "The operation failed because of a file system error.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use application::gallery::GalleryError;
    use std::collections::HashSet;

    fn location(area: GalleryAreaDto, path: &str) -> GalleryLocationDto {
        GalleryLocationDto {
            area,
            path: path.into(),
        }
    }

    #[test]
    fn locations_trim_the_outer_slashes_and_reject_hostile_paths() {
        let ok = location(GalleryAreaDto::Output, "/2026/ไทย folder/").into_domain();
        assert_eq!(ok.unwrap().path().as_string(), "2026/ไทย folder");
        let root = location(GalleryAreaDto::Input, "").into_domain().unwrap();
        assert!(root.is_root());
        assert_eq!(GalleryLocationDto::from(&root).area, GalleryAreaDto::Input);
        let too_long = "a/".repeat(MAX_PATH_BYTES);
        for path in [
            "../x",
            "a/../b",
            "C:/x",
            "a\\b",
            "a//b",
            ".vjump-gallery-staging",
            "a/.VJUMP-Gallery-Staging/b",
            too_long.as_str(),
        ] {
            let error = location(GalleryAreaDto::Output, path)
                .into_domain()
                .unwrap_err();
            assert_eq!(error.code, "validation", "{path:?}");
            assert_eq!(error.message, "The gallery location is not valid.");
        }
    }

    #[test]
    fn entry_references_need_a_bounded_revision_and_a_valid_location() {
        let entry = |revision: &str, path: &str| GalleryEntryRefDto {
            location: location(GalleryAreaDto::Output, path),
            revision: revision.into(),
        };
        let ok = entry("rev", "a.png").into_domain().unwrap();
        assert_eq!(ok.revision, EntryRevision("rev".into()));
        let longest = "r".repeat(MAX_REVISION_CHARS);
        assert!(entry(&longest, "a.png").into_domain().is_ok());
        for bad in [
            entry("", "a.png"),
            entry(&"r".repeat(MAX_REVISION_CHARS + 1), "a.png"),
            entry("rev", "../a.png"),
        ] {
            assert_eq!(bad.into_domain().unwrap_err().code, "validation");
        }
    }

    #[test]
    fn wire_names_follow_the_ipc_conventions() {
        let entry = GalleryEntryDto {
            location: location(GalleryAreaDto::Output, "a.glb"),
            revision: "r".into(),
            name: "a.glb".into(),
            kind: GalleryEntryKindDto::File,
            media: GalleryMediaKindDto::Model3d,
            size_bytes: Some(3.0),
            modified_ms: None,
        };
        assert_eq!(
            serde_json::to_value(&entry).unwrap(),
            serde_json::json!({
                "location": { "area": "output", "path": "a.glb" },
                "revision": "r",
                "name": "a.glb",
                "kind": "file",
                "media": "model3d",
                "sizeBytes": 3.0,
                "modifiedMs": null
            })
        );
        assert_eq!(
            serde_json::to_value([
                GalleryItemOutcomeDto::SourceRetained,
                GalleryItemOutcomeDto::Unchanged
            ])
            .unwrap(),
            serde_json::json!(["source_retained", "unchanged"])
        );
        assert_eq!(
            serde_json::to_value(GalleryWarningCodeDto::StagingLeftover).unwrap(),
            serde_json::json!("staging_leftover")
        );
    }

    #[test]
    fn a_failed_item_carries_the_sanitized_error() {
        let source = GalleryLocationDto {
            area: GalleryAreaDto::Input,
            path: "a.png".into(),
        }
        .into_domain()
        .unwrap();
        let result = ItemResult {
            source,
            destination: None,
            outcome: ItemOutcome::Failed,
            error: Some(GalleryErrorCode::Locked.into()),
        };
        let dto = GalleryItemResultDto::from(&result);
        assert_eq!(dto.outcome, GalleryItemOutcomeDto::Failed);
        assert_eq!(dto.error.unwrap().code, "gallery_locked");
        assert_eq!(dto.destination, None);
    }

    #[test]
    fn every_error_code_has_a_distinct_sanitized_description() {
        let all = [
            GalleryErrorCode::NotInstalled,
            GalleryErrorCode::NotFound,
            GalleryErrorCode::Stale,
            GalleryErrorCode::TargetExists,
            GalleryErrorCode::InvalidName,
            GalleryErrorCode::InvalidRequest,
            GalleryErrorCode::Locked,
            GalleryErrorCode::AccessDenied,
            GalleryErrorCode::TrashFailed,
            GalleryErrorCode::Unsupported,
            GalleryErrorCode::SourceRetained,
            GalleryErrorCode::Busy,
            GalleryErrorCode::Io,
        ];
        let mut codes = HashSet::new();
        for code in all {
            let (name, message) = describe(code);
            assert!(name.starts_with("gallery_"), "{name}");
            assert!(codes.insert(name), "{name} is used twice");
            assert!(!message.is_empty());
            assert!(
                !message.contains(['/', '\\']),
                "{message} must not look like a path"
            );
            let error = IpcError::from(GalleryError(code));
            assert_eq!(
                (error.code.as_str(), error.message.as_str()),
                (name, message)
            );
        }
    }
}
