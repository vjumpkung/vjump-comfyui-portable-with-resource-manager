//! The bundled curated custom nodes of first-run setup (FR-SETUP-9): a versioned JSON document of
//! GitHub repositories, each pinned to an exact commit, in the order of the reference installer.

use application::environment::{CuratedNodeCatalog, CuratedNodeSpec};
use serde::Deserialize;
use thiserror::Error;

use super::source::valid_ref_name;
use crate::custom_nodes::provenance::{repository_folder, validate_remote};

const BUNDLED: &str = include_str!("data/curated_nodes.json");
const SCHEMA_VERSION: u32 = 1;
const MAX_UNAVAILABLE_BYTES: usize = 300;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EntryDoc {
    repository: String,
    branch: String,
    commit: String,
    /// Why the entry cannot be installed at its pinned commit; the entry stays listed but unselectable.
    #[serde(default)]
    unavailable: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogDoc {
    schema_version: u32,
    provenance: String,
    nodes: Vec<EntryDoc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CuratedError {
    #[error("the curated node catalog is not valid JSON for this schema")]
    Parse,
    #[error("the curated node catalog has an unsupported schema version")]
    Schema,
    #[error("a curated node repository is not a GitHub owner/name pair")]
    Repository,
    #[error("a curated node branch is not a valid branch name")]
    Branch,
    #[error("a curated node commit is not a full commit hash")]
    Commit,
    #[error("the curated node catalog has no provenance or no nodes")]
    Empty,
    #[error("two curated nodes have the same id")]
    Duplicate,
    #[error("a curated node has an invalid unavailability reason")]
    Unavailable,
}

/// `owner/name` with the characters GitHub allows, and no `.git` suffix.
fn valid_repository(repository: &str) -> bool {
    let valid_part = |part: &str| {
        !part.is_empty()
            && part.len() <= 100
            && !part.starts_with('.')
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    };
    match repository.split_once('/') {
        Some((owner, name)) => {
            valid_part(owner) && valid_part(name) && !name.ends_with(".git") && !name.contains('/')
        }
        None => false,
    }
}

/// A reason shown to the user: trimmed, not empty, short and free of control characters.
fn valid_unavailable(reason: &str) -> bool {
    !reason.is_empty()
        && reason.len() <= MAX_UNAVAILABLE_BYTES
        && reason == reason.trim()
        && !reason.chars().any(char::is_control)
}

/// The catalog the app ships with. Ids derive from the repository names and are unique.
#[derive(Debug, Clone)]
pub struct BundledCuratedNodes {
    nodes: Vec<CuratedNodeSpec>,
}

impl BundledCuratedNodes {
    pub fn load() -> Result<Self, CuratedError> {
        Self::parse(BUNDLED)
    }

    pub fn parse(json: &str) -> Result<Self, CuratedError> {
        let doc: CatalogDoc = serde_json::from_str(json).map_err(|_| CuratedError::Parse)?;
        if doc.schema_version != SCHEMA_VERSION {
            return Err(CuratedError::Schema);
        }
        if doc.provenance.trim().is_empty() || doc.nodes.is_empty() {
            return Err(CuratedError::Empty);
        }
        let mut nodes: Vec<CuratedNodeSpec> = Vec::new();
        for entry in doc.nodes {
            if !valid_repository(&entry.repository) {
                return Err(CuratedError::Repository);
            }
            if !valid_ref_name(&entry.branch) {
                return Err(CuratedError::Branch);
            }
            if entry.commit.len() != 40
                || !entry
                    .commit
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                return Err(CuratedError::Commit);
            }
            if entry
                .unavailable
                .as_deref()
                .is_some_and(|reason| !valid_unavailable(reason))
            {
                return Err(CuratedError::Unavailable);
            }
            let url = format!("https://github.com/{}.git", entry.repository);
            // The node installs into the folder named after the repository.
            let name = validate_remote(&url, false)
                .and_then(|remote| repository_folder(&remote.identity))
                .ok_or(CuratedError::Repository)?;
            let id = name.as_str().to_ascii_lowercase();
            if nodes.iter().any(|existing| existing.id == id) {
                return Err(CuratedError::Duplicate);
            }
            nodes.push(CuratedNodeSpec {
                id,
                name: name.as_str().to_owned(),
                repository: url,
                branch: entry.branch,
                commit: entry.commit,
                provenance: doc.provenance.clone(),
                unavailable: entry.unavailable,
            });
        }
        Ok(Self { nodes })
    }
}

impl CuratedNodeCatalog for BundledCuratedNodes {
    fn list(&self) -> Vec<CuratedNodeSpec> {
        self.nodes.clone()
    }
}

#[cfg(test)]
mod tests;
