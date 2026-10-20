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
    LatestCommit,
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

/// Where a checkout of a chosen commit is placed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckoutPlacement {
    /// On the default branch, or the tag's release branch, moved to the commit, so `git pull`
    /// keeps working.
    Branch(String),
    /// Detached at the commit, with the reason no branch was used.
    Detached(&'static str),
}

/// What a clone knows about one remote branch and its local counterpart: whether the remote
/// branch contains the target commit and how many commits the local branch has that the remote
/// branch does not (`u64::MAX` when they cannot be counted).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BranchFacts<'a> {
    pub name: &'a str,
    pub contains_target: bool,
    pub local_only_commits: u64,
}

/// `release/vMAJOR.MINOR` for a `vMAJOR.MINOR.PATCH` tag (where ComfyUI publishes patch releases).
pub fn release_line(tag: &str) -> Option<String> {
    let parts: Vec<_> = tag.strip_prefix('v')?.split('.').collect();
    (parts.len() == 3 && parts.iter().all(|part| all_digits(part)))
        .then(|| format!("release/v{}.{}", parts[0], parts[1]))
}

/// Whether `branch` is named `release/vMAJOR.MINOR`.
pub fn is_release_line(branch: &str) -> bool {
    branch.strip_prefix("release/v").is_some_and(|version| {
        let parts: Vec<_> = version.split('.').collect();
        parts.len() == 2 && parts.iter().all(|part| all_digits(part))
    })
}

fn all_digits(part: &str) -> bool {
    !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit())
}

/// Keeps a checkout on a branch that tracks `origin` unless that would lose or hide local work:
/// the remote's default branch when it contains the target, otherwise the tag's release branch.
/// `current_branch` is `None` for a detached HEAD.
pub fn checkout_placement(
    default: Option<BranchFacts<'_>>,
    release: Option<BranchFacts<'_>>,
    current_branch: Option<&str>,
) -> CheckoutPlacement {
    let Some(default) = default else {
        return CheckoutPlacement::Detached("the remote has no default branch");
    };
    if current_branch.is_some_and(|branch| branch != default.name && !is_release_line(branch)) {
        return CheckoutPlacement::Detached("HEAD is on another branch");
    }
    if default.contains_target {
        return if default.local_only_commits > 0 {
            CheckoutPlacement::Detached("the local default branch has unpublished commits")
        } else {
            CheckoutPlacement::Branch(default.name.to_owned())
        };
    }
    match release {
        Some(release) if release.contains_target => {
            if release.local_only_commits > 0 {
                CheckoutPlacement::Detached("the local release branch has unpublished commits")
            } else {
                CheckoutPlacement::Branch(release.name.to_owned())
            }
        }
        _ => CheckoutPlacement::Detached(
            "the target is on neither the default branch nor its release branch",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn latest_commit_update_has_the_tag_refusals() {
        let detached = Revision {
            commit: "a".repeat(40),
            branch: None,
            tag: Some("v1".into()),
        };
        for (state, refusal) in [
            (
                RepoState {
                    dirty: true,
                    ..Default::default()
                },
                UpdateRefusal::Dirty,
            ),
            (
                RepoState {
                    unfinished_operation: true,
                    ..Default::default()
                },
                UpdateRefusal::UnfinishedOperation,
            ),
            (
                RepoState {
                    ahead: 1,
                    behind: 1,
                    ..Default::default()
                },
                UpdateRefusal::Diverged,
            ),
        ] {
            for target in [
                UpdateTarget::LatestCommit,
                UpdateTarget::Tag(Tag::new("v2").unwrap()),
            ] {
                assert_eq!(can_update(&state, &detached, &target), Err(refusal.clone()));
            }
        }
        assert!(
            can_update(
                &RepoState::default(),
                &detached,
                &UpdateTarget::LatestCommit
            )
            .is_ok()
        );
        let attached = Revision {
            branch: Some("release/v1.0".into()),
            ..detached
        };
        assert!(
            can_update(
                &RepoState::default(),
                &attached,
                &UpdateTarget::LatestCommit
            )
            .is_ok()
        );
    }
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

    #[test]
    fn checkout_placement_attaches_only_when_nothing_local_is_lost() {
        use CheckoutPlacement::{Branch, Detached};
        fn on(name: &str, contains: bool, local: u64) -> BranchFacts<'_> {
            BranchFacts {
                name,
                contains_target: contains,
                local_only_commits: local,
            }
        }
        let release = "release/v0.42";
        // A detached HEAD, or one already on the default branch, is attached.
        assert_eq!(
            checkout_placement(Some(on("master", true, 0)), None, None),
            Branch("master".into())
        );
        assert_eq!(
            checkout_placement(Some(on("master", true, 0)), None, Some("master")),
            Branch("master".into())
        );
        // Each rule, in order.
        assert_eq!(
            checkout_placement(None, Some(on(release, true, 0)), Some("master")),
            Detached("the remote has no default branch")
        );
        assert_eq!(
            checkout_placement(
                Some(on("master", false, 0)),
                Some(on(release, true, 0)),
                Some("dev")
            ),
            Detached("HEAD is on another branch")
        );
        assert_eq!(
            checkout_placement(Some(on("master", true, 1)), None, None),
            Detached("the local default branch has unpublished commits")
        );
        // A target on the default branch never falls through to the release branch.
        assert_eq!(
            checkout_placement(
                Some(on("master", true, u64::MAX)),
                Some(on(release, true, 0)),
                Some("master")
            ),
            Detached("the local default branch has unpublished commits")
        );
        // A target off the default branch attaches to its release branch.
        for current in [None, Some("master"), Some("release/v0.41")] {
            assert_eq!(
                checkout_placement(
                    Some(on("master", false, 0)),
                    Some(on(release, true, 0)),
                    current
                ),
                Branch(release.into())
            );
        }
        assert_eq!(
            checkout_placement(Some(on("master", true, 0)), None, Some("release/v0.42")),
            Branch("master".into())
        );
        assert_eq!(
            checkout_placement(
                Some(on("master", false, 0)),
                Some(on(release, true, 2)),
                None
            ),
            Detached("the local release branch has unpublished commits")
        );
        assert_eq!(
            checkout_placement(
                Some(on("master", false, 0)),
                Some(on(release, false, 0)),
                None
            ),
            Detached("the target is on neither the default branch nor its release branch")
        );
        assert_eq!(
            checkout_placement(Some(on("master", false, 0)), None, Some("master")),
            Detached("the target is on neither the default branch nor its release branch")
        );
    }

    #[test]
    fn checkout_placement_derives_release_lines() {
        assert_eq!(release_line("v0.42.1").as_deref(), Some("release/v0.42"));
        assert_eq!(release_line("v0.42.0").as_deref(), Some("release/v0.42"));
        for tag in ["v2.0.0-side", "v0.42", "HEAD", "vx.1.2"] {
            assert_eq!(release_line(tag), None, "{tag}");
        }
        assert!(is_release_line("release/v0.42"));
        for branch in ["release/v0.18.3", "release/foo", "master"] {
            assert!(!is_release_line(branch), "{branch}");
        }
    }
}
