use crate::shared::model_values::SafeFileName;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NodeError {
    #[error("invalid node identity or source")]
    InvalidIdentity,
    #[error("consent does not cover this exact plan")]
    ConsentMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NodeId(String);
impl NodeId {
    pub fn new(value: impl Into<String>) -> Result<Self, NodeError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 128
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        {
            return Err(NodeError::InvalidIdentity);
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeSource {
    Registry {
        id: String,
        publisher: Option<String>,
        version: String,
        repository: Option<String>,
    },
    Git {
        repository: String,
        commit: String,
        branch: Option<String>,
    },
    Manual,
}
impl NodeSource {
    /// Length framing prevents concatenated metadata from aliasing another source.
    pub fn identity(&self) -> String {
        fn frame(parts: &[&str]) -> String {
            parts.iter().map(|s| format!("{}:{s}", s.len())).collect()
        }
        match self {
            Self::Registry {
                id,
                publisher,
                version,
                repository,
            } => frame(&[
                "registry",
                id,
                publisher.as_deref().unwrap_or(""),
                version,
                repository.as_deref().unwrap_or(""),
            ]),
            Self::Git {
                repository,
                commit,
                branch,
            } => frame(&["git", repository, commit, branch.as_deref().unwrap_or("")]),
            Self::Manual => "manual".into(),
        }
    }
    /// True when `observed` (read from disk) names the same origin as `self` (the stored record):
    /// the same registry node, the same Git remote or both manual. Versions and commits may differ
    /// because Manager or a manual `git pull` can change them outside the app.
    pub fn same_origin(&self, observed: &Self) -> bool {
        match (self, observed) {
            (Self::Registry { id: a, .. }, Self::Registry { id: b, .. }) => a == b,
            (Self::Git { repository: a, .. }, Self::Git { repository: b, .. }) => a == b,
            (Self::Manual, Self::Manual) => true,
            _ => false,
        }
    }
    /// The stored record with the observed exact version, commit or branch applied. Stored
    /// publisher and repository metadata are kept because the disk cannot supply them.
    pub fn with_observed_revision(&self, observed: &Self) -> Self {
        match (self, observed) {
            (
                Self::Registry {
                    id,
                    publisher,
                    repository,
                    ..
                },
                Self::Registry { version, .. },
            ) => Self::Registry {
                id: id.clone(),
                publisher: publisher.clone(),
                version: version.clone(),
                repository: repository.clone(),
            },
            (Self::Git { repository, .. }, Self::Git { commit, branch, .. }) => Self::Git {
                repository: repository.clone(),
                commit: commit.clone(),
                branch: branch.clone(),
            },
            _ => self.clone(),
        }
    }
    /// Repository normalization is performed by the infrastructure URL parser before construction.
    pub fn trust_namespace(&self) -> Option<String> {
        match self {
            Self::Registry {
                id,
                publisher: Some(publisher),
                repository: Some(repository),
                ..
            } => Some(format!(
                "registry:{}:{id}{}:{publisher}{}:{repository}",
                id.len(),
                publisher.len(),
                repository.len()
            )),
            Self::Git { repository, .. } => Some(format!("git:{}:{repository}", repository.len())),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProvenanceOrigin {
    App,
    ExternallyObserved,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyState {
    NotRun,
    Ok,
    Failed,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledNode {
    pub id: NodeId,
    pub name: SafeFileName,
    pub source: NodeSource,
    pub origin: ProvenanceOrigin,
    pub enabled: bool,
    pub single_file: bool,
    pub tree_digest: String,
    pub dependencies: DependencyState,
    pub conflict: bool,
    pub dirty: Option<bool>,
}

impl InstalledNode {
    pub fn dependency_identity(&self) -> String {
        let source = self.source.identity();
        format!(
            "{}:{source}{}:{}",
            source.len(),
            self.tree_digest.len(),
            self.tree_digest
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry(id: &str, version: &str) -> NodeSource {
        NodeSource::Registry {
            id: id.into(),
            publisher: Some("p".into()),
            version: version.into(),
            repository: Some("https://github.com/p/r".into()),
        }
    }

    #[test]
    fn node_ids_reject_path_like_values() {
        for bad in ["", "a/b", "a b", "..", r"a\b", &"x".repeat(129)] {
            assert!(NodeId::new(bad).is_err(), "{bad}");
        }
        assert!(NodeId::new("node-0f_1").is_ok());
    }

    #[test]
    fn source_identity_is_length_framed() {
        let a = NodeSource::Git {
            repository: "ab".into(),
            commit: "c".into(),
            branch: None,
        };
        let b = NodeSource::Git {
            repository: "a".into(),
            commit: "bc".into(),
            branch: None,
        };
        assert_ne!(a.identity(), b.identity());
    }

    #[test]
    fn dependency_identity_frames_the_exact_source_and_content() {
        let mut node = InstalledNode {
            id: NodeId::new("node").unwrap(),
            name: SafeFileName::new("node").unwrap(),
            source: registry("nodes", "1.0.0"),
            origin: ProvenanceOrigin::App,
            enabled: true,
            single_file: false,
            tree_digest: "contents".into(),
            dependencies: DependencyState::NotRun,
            conflict: false,
            dirty: None,
        };
        let source = node.source.identity();
        assert_eq!(
            node.dependency_identity(),
            format!("{}:{source}8:contents", source.len())
        );
        let identity = node.dependency_identity();
        node.tree_digest = "changed".into();
        assert_ne!(node.dependency_identity(), identity);
        node.tree_digest = "contents".into();
        node.source = registry("nodes", "1.1.0");
        assert_ne!(node.dependency_identity(), identity);
    }

    #[test]
    fn observed_revision_changes_do_not_change_the_origin() {
        let stored = registry("nodes", "1.0.0");
        let observed = NodeSource::Registry {
            id: "nodes".into(),
            publisher: None,
            version: "1.1.0".into(),
            repository: None,
        };
        assert!(stored.same_origin(&observed));
        assert_eq!(
            stored.with_observed_revision(&observed),
            registry("nodes", "1.1.0")
        );
        assert!(!stored.same_origin(&registry("other", "1.0.0")));
        assert!(!stored.same_origin(&NodeSource::Manual));
        let git = NodeSource::Git {
            repository: "https://h/r".into(),
            commit: "a".into(),
            branch: Some("main".into()),
        };
        let moved = NodeSource::Git {
            repository: "https://h/r".into(),
            commit: "b".into(),
            branch: Some("main".into()),
        };
        let other = NodeSource::Git {
            repository: "https://h/x".into(),
            commit: "a".into(),
            branch: None,
        };
        assert!(git.same_origin(&moved));
        assert!(!git.same_origin(&other));
    }

    #[test]
    fn trust_is_namespaced_by_kind_publisher_node_and_repository() {
        let a = registry("nodes", "1");
        let mut other_publisher = a.clone();
        if let NodeSource::Registry { publisher, .. } = &mut other_publisher {
            *publisher = Some("evil".into());
        }
        assert_ne!(a.trust_namespace(), other_publisher.trust_namespace());
        let git = NodeSource::Git {
            repository: "https://github.com/p/r".into(),
            commit: "a".into(),
            branch: None,
        };
        assert_ne!(a.trust_namespace(), git.trust_namespace());
        let no_repository = NodeSource::Registry {
            id: "n".into(),
            publisher: Some("p".into()),
            version: "1".into(),
            repository: None,
        };
        assert_eq!(no_repository.trust_namespace(), None);
        assert_eq!(NodeSource::Manual.trust_namespace(), None);
    }
}
