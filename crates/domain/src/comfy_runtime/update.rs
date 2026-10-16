use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag(String);
impl Tag {
    pub fn new(value: impl Into<String>) -> Result<Self, UpdateRefusal> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 200
            || value.starts_with(['-', '/', '.'])
            || value.ends_with(['/', '.'])
            || value.contains("..")
            || value.contains("//")
            || value.contains("@{")
            || value
                .split('/')
                .any(|part| part.starts_with('.') || part.ends_with(".lock"))
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'/' | b'-'))
        {
            Err(UpdateRefusal::InvalidTag)
        } else {
            Ok(Self(value))
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateTarget {
    LatestRelease,
    Tag(Tag),
    BranchFfOnly,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revision {
    pub commit: String,
    pub tag: Option<String>,
    pub branch: Option<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepoState {
    pub dirty: bool,
    pub unfinished_operation: bool,
    pub ahead: u64,
    pub behind: u64,
    pub has_upstream: bool,
    pub status: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum UpdateRefusal {
    #[error("the working tree has local changes")]
    Dirty,
    #[error("finish the pending merge, rebase or cherry-pick first")]
    UnfinishedOperation,
    #[error("the branch has diverged from its upstream")]
    Diverged,
    #[error("the checkout is detached; select a release tag")]
    Detached,
    #[error("the branch has no upstream")]
    MissingUpstream,
    #[error("invalid release tag")]
    InvalidTag,
}
pub fn can_update(
    state: &RepoState,
    revision: &Revision,
    target: &UpdateTarget,
) -> Result<(), UpdateRefusal> {
    if state.dirty {
        return Err(UpdateRefusal::Dirty);
    }
    if state.unfinished_operation {
        return Err(UpdateRefusal::UnfinishedOperation);
    }
    if state.ahead > 0 && state.behind > 0 {
        return Err(UpdateRefusal::Diverged);
    }
    if matches!(target, UpdateTarget::BranchFfOnly) {
        if revision.branch.is_none() {
            return Err(UpdateRefusal::Detached);
        }
        if !state.has_upstream {
            return Err(UpdateRefusal::MissingUpstream);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refusal_policy_and_safe_refs() {
        for tag in ["-x", "v1^{commit}", "a/../b", "x.lock/y", ".x/y"] {
            assert!(Tag::new(tag).is_err());
        }
        let rev = Revision {
            commit: "a".repeat(40),
            branch: None,
            tag: None,
        };
        assert_eq!(
            can_update(&RepoState::default(), &rev, &UpdateTarget::BranchFfOnly),
            Err(UpdateRefusal::Detached)
        );
        assert!(
            can_update(
                &RepoState::default(),
                &rev,
                &UpdateTarget::Tag(Tag::new("v1").unwrap())
            )
            .is_ok()
        );
        assert_eq!(
            can_update(
                &RepoState {
                    dirty: true,
                    ..Default::default()
                },
                &rev,
                &UpdateTarget::LatestRelease
            ),
            Err(UpdateRefusal::Dirty)
        );
    }
}
