//! The facts behind `domain::comfy_runtime::checkout_placement`, read from a clone with the
//! caller's own Git command style. Probes are quiet: any failure answers "not known", which keeps
//! the checkout detached.

use std::time::Duration;

use application::shared::CancelToken;
use domain::comfy_runtime::{BranchFacts, CheckoutPlacement, checkout_placement, release_line};

use super::valid_ref_name;
use crate::shared::process::{self, CommandSpec, RunError, RunLimits};

const REMOTE_PREFIX: &str = "refs/remotes/origin/";

/// The caller's cancellation token fired while a probe ran.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Cancelled;

/// Decides whether `commit` is checked out on the remote's default branch, on the release branch
/// of `tag`, or detached. `git` builds a command for the repository from the arguments after
/// `git`. Nothing is modified.
pub(crate) async fn placement(
    git: impl Fn(&[&str]) -> CommandSpec,
    commit: &str,
    tag: Option<&str>,
    cancel: &CancelToken,
) -> Result<CheckoutPlacement, Cancelled> {
    let default = default_branch(&git, cancel).await?;
    placement_with_default(git, commit, tag, default.as_deref(), cancel).await
}

/// The validated name recorded by origin's symbolic default-branch reference.
pub(crate) async fn default_branch(
    git: impl Fn(&[&str]) -> CommandSpec,
    cancel: &CancelToken,
) -> Result<Option<String>, Cancelled> {
    Ok(probe(
        &git(&["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"]),
        cancel,
    )
    .await?
    .and_then(|text| {
        text.trim()
            .strip_prefix(REMOTE_PREFIX)
            .filter(|name| valid_ref_name(name))
            .map(str::to_owned)
    }))
}

/// Uses a previously frozen default branch instead of discovering origin/HEAD again.
pub(crate) async fn placement_with_default(
    git: impl Fn(&[&str]) -> CommandSpec,
    commit: &str,
    tag: Option<&str>,
    default: Option<&str>,
    cancel: &CancelToken,
) -> Result<CheckoutPlacement, Cancelled> {
    let Some(default) = default else {
        return Ok(checkout_placement(None, None, None));
    };
    let current = probe(
        &git(&["symbolic-ref", "--quiet", "--short", "HEAD"]),
        cancel,
    )
    .await?
    .map(|text| text.trim().to_owned())
    .filter(|name| !name.is_empty());
    let (default_on, default_local) = branch_facts(&git, default, commit, cancel).await?;
    // The release branch only matters when the default branch does not contain the target.
    let release = match tag
        .and_then(release_line)
        .filter(|line| valid_ref_name(line))
    {
        Some(line) if !default_on => {
            let (contains_target, local_only_commits) =
                branch_facts(&git, &line, commit, cancel).await?;
            Some((line, contains_target, local_only_commits))
        }
        _ => None,
    };
    Ok(checkout_placement(
        Some(BranchFacts {
            name: default,
            contains_target: default_on,
            local_only_commits: default_local,
        }),
        release
            .as_ref()
            .map(|(name, contains_target, local_only_commits)| BranchFacts {
                name,
                contains_target: *contains_target,
                local_only_commits: *local_only_commits,
            }),
        current.as_deref(),
    ))
}

/// Whether origin's branch `name` contains `commit`, and how many commits the local branch of
/// that name has that origin's does not. A missing remote branch does not contain the commit; a
/// missing local branch has none.
async fn branch_facts(
    git: &impl Fn(&[&str]) -> CommandSpec,
    name: &str,
    commit: &str,
    cancel: &CancelToken,
) -> Result<(bool, u64), Cancelled> {
    let remote = format!("{REMOTE_PREFIX}{name}");
    let local = format!("refs/heads/{name}");
    let contains_target = probe(
        &git(&["merge-base", "--is-ancestor", commit, &remote]),
        cancel,
    )
    .await?
    .is_some();
    let local_only = if probe(&git(&["rev-parse", "--verify", "--quiet", &local]), cancel)
        .await?
        .is_some()
    {
        let range = format!("{remote}..{local}");
        probe(&git(&["rev-list", "--count", &range]), cancel)
            .await?
            .and_then(|text| text.trim().parse().ok())
            // A branch whose unpublished commits cannot be counted is treated as having some.
            .unwrap_or(u64::MAX)
    } else {
        0
    };
    Ok((contains_target, local_only))
}

/// The output of a command that exited 0; `None` for any other outcome.
async fn probe(spec: &CommandSpec, cancel: &CancelToken) -> Result<Option<String>, Cancelled> {
    let limits = RunLimits {
        timeout: Some(Duration::from_secs(60)),
        ..RunLimits::default()
    };
    match process::run(spec, &process::DiscardSink, cancel, &limits).await {
        Ok(output) => Ok(Some(output.stdout)),
        Err(RunError::Cancelled { .. }) => Err(Cancelled),
        Err(_) => Ok(None),
    }
}
