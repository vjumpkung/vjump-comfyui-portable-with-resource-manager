//! The model file aggregate and the validation rules of rename, move and delete requests.
use super::ModelCategory;
use crate::shared::model_values::{JobId, RelativeModelPath, SafeFileName, Sha256};

/// An authorized root plus validated relative components. Never an unchecked absolute path.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModelLocation {
    pub root_id: String,
    pub path: RelativeModelPath,
}
impl ModelLocation {
    pub fn new(root_id: impl Into<String>, path: RelativeModelPath) -> Self {
        Self {
            root_id: root_id.into(),
            path,
        }
    }
    /// The final component, or `None` for the root itself.
    pub fn file_name(&self) -> Option<&SafeFileName> {
        self.path.components().last()
    }
    /// The directory holding this entry.
    pub fn directory(&self) -> RelativeModelPath {
        let parts = self.path.components();
        let mut result = RelativeModelPath::default();
        for part in parts.iter().take(parts.len().saturating_sub(1)) {
            result = result.join(part.clone());
        }
        result
    }
    /// Another entry in the same directory.
    pub fn sibling(&self, name: SafeFileName) -> Self {
        Self {
            root_id: self.root_id.clone(),
            path: self.directory().join(name),
        }
    }
    /// The lock and comparison key; names compare case-insensitively on every platform so a
    /// case-insensitive volume can never be bypassed.
    pub fn key(&self) -> String {
        format!("{}:{}", self.root_id, self.path.as_string().to_lowercase())
    }
    pub fn display(&self) -> String {
        self.path.as_string()
    }
}

/// Which sidecar files exist beside a model.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SidecarSet {
    pub preview: bool,
    pub civitai_info: bool,
    pub json: bool,
}
impl SidecarSet {
    pub fn bits(self) -> u8 {
        u8::from(self.preview) | (u8::from(self.civitai_info) << 1) | (u8::from(self.json) << 2)
    }
    pub fn from_bits(bits: u8) -> Self {
        Self {
            preview: bits & 1 != 0,
            civitai_info: bits & 2 != 0,
            json: bits & 4 != 0,
        }
    }
}

/// One cataloged file. Hash and provenance are only kept while the file identity still matches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelFile {
    pub location: ModelLocation,
    pub category: ModelCategory,
    pub size: u64,
    pub modified_ns: u128,
    pub platform_id: String,
    pub sha256: Option<Sha256>,
    pub source_job_id: Option<JobId>,
    pub civitai_version_id: Option<u64>,
    pub sidecars: SidecarSet,
}
impl ModelFile {
    pub fn name(&self) -> &str {
        self.location.file_name().map_or("", SafeFileName::as_str)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RenameError {
    #[error("the new name is not a valid file name")]
    Invalid,
    #[error("the new name is the same as the current name")]
    Unchanged,
    #[error("the file extension would change")]
    ExtensionChanged,
}

/// The extension including its dot, or the empty string.
pub fn extension_of(name: &str) -> &str {
    match name.rfind('.') {
        Some(0) | None => "",
        Some(index) => &name[index..],
    }
}

/// True when two names differ only by letter case.
pub fn case_only_difference(a: &str, b: &str) -> bool {
    a != b && a.to_lowercase() == b.to_lowercase()
}

/// Validates a rename within the same folder. The extension is preserved by default; changing
/// it needs `allow_extension_change`, which the UI sets after warning the user.
pub fn validate_rename(
    current: &SafeFileName,
    new_name: &str,
    allow_extension_change: bool,
) -> Result<SafeFileName, RenameError> {
    let new = SafeFileName::new(new_name.trim()).map_err(|_| RenameError::Invalid)?;
    if new == *current {
        return Err(RenameError::Unchanged);
    }
    if !allow_extension_change && extension_of(new.as_str()) != extension_of(current.as_str()) {
        return Err(RenameError::ExtensionChanged);
    }
    Ok(new)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(value: &str) -> SafeFileName {
        SafeFileName::new(value).unwrap()
    }

    #[test]
    fn hostile_rename_targets_are_rejected() {
        let current = name("model.safetensors");
        for hostile in [
            "..\\..\\evil.dll",
            "../evil.safetensors",
            "CON.safetensors",
            "a:b",
            "dir/file.safetensors",
            "",
            "  ",
            "trailing.",
        ] {
            assert_eq!(
                validate_rename(&current, hostile, true),
                Err(RenameError::Invalid),
                "{hostile:?}"
            );
        }
    }

    #[test]
    fn extension_is_preserved_unless_the_user_allows_a_change() {
        let current = name("model.safetensors");
        assert_eq!(
            validate_rename(&current, "model.ckpt", false),
            Err(RenameError::ExtensionChanged)
        );
        assert_eq!(
            validate_rename(&current, "model.ckpt", true)
                .unwrap()
                .as_str(),
            "model.ckpt"
        );
        assert_eq!(
            validate_rename(&current, "better ไทย.safetensors", false)
                .unwrap()
                .as_str(),
            "better ไทย.safetensors"
        );
        assert_eq!(
            validate_rename(&current, "model.safetensors", false),
            Err(RenameError::Unchanged)
        );
    }

    #[test]
    fn case_only_changes_are_detected() {
        assert!(case_only_difference("Model.pt", "model.pt"));
        assert!(!case_only_difference("model.pt", "model.pt"));
        assert!(!case_only_difference("a.pt", "b.pt"));
        assert_eq!(extension_of(".hidden"), "");
        assert_eq!(extension_of("a.tar.gz"), ".gz");
    }

    #[test]
    fn locations_compare_case_insensitively_and_expose_siblings() {
        let a = ModelLocation::new(
            "models",
            RelativeModelPath::new("checkpoints/A.pt").unwrap(),
        );
        let b = ModelLocation::new(
            "models",
            RelativeModelPath::new("checkpoints/a.PT").unwrap(),
        );
        assert_eq!(a.key(), b.key());
        assert_eq!(a.sibling(name("x.pt")).display(), "checkpoints/x.pt");
        assert_eq!(a.directory().as_string(), "checkpoints");
    }

    #[test]
    fn sidecar_bits_round_trip() {
        let set = SidecarSet {
            preview: true,
            civitai_info: false,
            json: true,
        };
        assert_eq!(SidecarSet::from_bits(set.bits()), set);
    }
}
