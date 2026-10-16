//! Git for custom nodes. HTTPS only, immutable commit resolution, staged clones with hooks
//! disabled and fast-forward-only updates. Nothing here resets, cleans or discards edits.
use super::provenance::{Remote, observed_identity, validate_remote};
use crate::{
    comfy_runtime::git::{parse_counts, porcelain_dirty},
    environment::{
        exec::{StepSink, looks_transient},
        layout::{Layout, Tool, command},
        source::valid_ref_name,
        tools::git_program,
    },
    shared::process::{self, CommandSpec, DiscardSink, RunError, RunLimits},
};
use application::{custom_nodes::NodeAppError, environment::StepContext, shared::CancelToken};
use domain::comfy_runtime::{RepoState, Revision, UpdateTarget, can_update};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    DefaultHead,
    Branch,
    Tag,
    Commit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub commit: String,
    pub branch: Option<String>,
    pub kind: RefKind,
}

#[derive(Debug, Clone)]
pub struct Inspection {
    pub state: RepoState,
    pub revision: Revision,
}

pub struct GitNodes {
    program: Option<PathBuf>,
    /// Tests only: accept local repositories as remotes.
    pub(crate) allow_local: bool,
}

pub fn is_object_id(text: &str) -> bool {
    matches!(text.len(), 40 | 64)
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn failed(message: impl Into<String>) -> NodeAppError {
    NodeAppError::Failed(message.into())
}

impl GitNodes {
    pub fn new() -> Self {
        Self {
            program: None,
            allow_local: false,
        }
    }

    pub fn with_program(program: PathBuf, allow_local: bool) -> Self {
        Self {
            program: Some(program),
            allow_local,
        }
    }

    #[cfg(test)]
    pub(crate) fn spec_for_test(&self, root: &Path) -> CommandSpec {
        self.spec(root, Path::new("/hooks"), None, &["status"])
            .unwrap()
    }

    pub fn validate(&self, url: &str) -> Result<Remote, NodeAppError> {
        validate_remote(url, self.allow_local).ok_or(NodeAppError::UnsafeSource)
    }

    pub fn available(&self, root: &Path) -> bool {
        self.program
            .clone()
            .or_else(|| git_program(&Layout::new(root)))
            .is_some()
    }

    fn spec(
        &self,
        root: &Path,
        hooks: &Path,
        cwd: Option<&Path>,
        args: &[&str],
    ) -> Result<CommandSpec, NodeAppError> {
        let layout = Layout::new(root);
        let program = self
            .program
            .clone()
            .or_else(|| git_program(&layout))
            .ok_or_else(|| failed("Git is not available; complete setup first"))?;
        let mut spec = command(&layout, Tool::Git, program)
            .args(["-c", "core.longpaths=true", "-c", "advice.detachedHead=false"])
            .args(["-c", "core.symlinks=false", "-c", "submodule.recurse=false"])
            .args(["-c", "protocol.allow=never", "-c", "protocol.https.allow=always"])
            .args(["-c", "credential.helper=", "-c", "core.fsmonitor=false"])
            .arg("-c")
            .arg(format!("core.hooksPath={}", hooks.display()));
        if self.allow_local {
            spec = spec.args(["-c", "protocol.file.allow=always"]);
        }
        if let Some(cwd) = cwd {
            spec = spec.arg("-C").arg(cwd);
        }
        Ok(spec.args(args.iter().copied()))
    }

    pub(crate) async fn run(
        &self,
        root: &Path,
        hooks: &Path,
        cwd: Option<&Path>,
        args: &[&str],
        cancel: &CancelToken,
        ctx: Option<&StepContext>,
        timeout: Duration,
    ) -> Result<String, NodeAppError> {
        let spec = self.spec(root, hooks, cwd, args)?;
        let limits = RunLimits {
            timeout: Some(timeout),
            ..Default::default()
        };
        let outcome = match ctx {
            Some(ctx) => process::run(&spec, &StepSink(ctx), cancel, &limits).await,
            None => process::run(&spec, &DiscardSink, cancel, &limits).await,
        };
        match outcome {
            Ok(output) if output.stdout.len() < limits.max_stdout_bytes => Ok(output.stdout),
            Ok(_) => Err(failed("Git output exceeded the safe capture limit")),
            Err(RunError::Cancelled { .. }) => Err(NodeAppError::Cancelled),
            Err(RunError::Failed { tail, .. }) | Err(RunError::TimedOut { tail }) => {
                if looks_transient(&tail) {
                    Err(NodeAppError::Offline)
                } else {
                    let start = tail.len().saturating_sub(3);
                    Err(failed(format!("Git failed: {}", tail[start..].join(" "))))
                }
            }
            Err(_) => Err(failed("Git could not be run")),
        }
    }

    async fn read(
        &self,
        root: &Path,
        hooks: &Path,
        dir: &Path,
        args: &[&str],
    ) -> Result<String, NodeAppError> {
        self.run(
            root,
            hooks,
            Some(dir),
            args,
            &CancelToken::new(),
            None,
            Duration::from_secs(120),
        )
        .await
    }

    /// Resolves the reference to one immutable commit. A name that matches both a branch and a
    /// tag is rejected instead of guessed.
    pub async fn resolve(
        &self,
        root: &Path,
        hooks: &Path,
        remote: &Remote,
        reference: Option<&str>,
        cancel: &CancelToken,
    ) -> Result<Resolved, NodeAppError> {
        let timeout = Duration::from_secs(120);
        match reference.map(str::trim).filter(|r| !r.is_empty()) {
            None => {
                let out = self
                    .run(
                        root,
                        hooks,
                        None,
                        &["ls-remote", "--symref", &remote.clone_url, "HEAD"],
                        cancel,
                        None,
                        timeout,
                    )
                    .await?;
                let mut branch = None;
                let mut commit = None;
                for line in out.lines() {
                    if let Some(rest) = line.strip_prefix("ref: ")
                        && let Some((name, target)) = rest.split_once('\t')
                        && target.trim() == "HEAD"
                    {
                        branch = name.strip_prefix("refs/heads/").map(str::to_owned);
                    } else if let Some((sha, name)) = line.split_once('\t')
                        && name.trim() == "HEAD"
                        && is_object_id(sha)
                    {
                        commit = Some(sha.to_owned());
                    }
                }
                let commit = commit.ok_or_else(|| failed("The repository has no default branch"))?;
                Ok(Resolved {
                    commit,
                    branch,
                    kind: RefKind::DefaultHead,
                })
            }
            Some(reference) if is_object_id(reference) => Ok(Resolved {
                commit: reference.to_owned(),
                branch: None,
                kind: RefKind::Commit,
            }),
            Some(reference) => {
                if !valid_ref_name(reference) {
                    return Err(NodeAppError::InvalidRequest);
                }
                let head = format!("refs/heads/{reference}");
                let tag = format!("refs/tags/{reference}");
                let peeled = format!("{tag}^{{}}");
                let out = self
                    .run(
                        root,
                        hooks,
                        None,
                        &["ls-remote", &remote.clone_url, &head, &tag, &peeled],
                        cancel,
                        None,
                        timeout,
                    )
                    .await?;
                let (mut branch, mut tag_direct, mut tag_peeled) = (None, None, None);
                for line in out.lines() {
                    let Some((sha, name)) = line.split_once('\t') else {
                        continue;
                    };
                    if !is_object_id(sha) {
                        continue;
                    }
                    match name.trim() {
                        n if n == head => branch = Some(sha.to_owned()),
                        n if n == tag => tag_direct = Some(sha.to_owned()),
                        n if n == peeled => tag_peeled = Some(sha.to_owned()),
                        _ => {}
                    }
                }
                let tag_commit = tag_peeled.or(tag_direct);
                match (branch, tag_commit) {
                    (Some(_), Some(_)) => Err(NodeAppError::AmbiguousReference),
                    (Some(commit), None) => Ok(Resolved {
                        commit,
                        branch: Some(reference.to_owned()),
                        kind: RefKind::Branch,
                    }),
                    (None, Some(commit)) => Ok(Resolved {
                        commit,
                        branch: None,
                        kind: RefKind::Tag,
                    }),
                    (None, None) => Err(failed("The branch or tag was not found")),
                }
            }
        }
    }

    /// Clones into the fresh `staging` directory without checking out, then checks out exactly the
    /// resolved commit. Branch installs keep tracking; tags and commits stay detached.
    pub async fn clone_checkout(
        &self,
        root: &Path,
        hooks: &Path,
        remote: &Remote,
        resolved: &Resolved,
        staging: &Path,
        ctx: &StepContext,
    ) -> Result<(), NodeAppError> {
        let timeout = Duration::from_secs(3600);
        let staging_text = staging.to_string_lossy().into_owned();
        self.run(
            root,
            hooks,
            None,
            &["clone", "--no-checkout", "--progress", &remote.clone_url, &staging_text],
            &ctx.cancel,
            Some(ctx),
            timeout,
        )
        .await?;
        // A pinned commit must exist in the clone before anything is checked out.
        self.run(
            root,
            hooks,
            Some(staging),
            &["cat-file", "-e", &format!("{}^{{commit}}", resolved.commit)],
            &ctx.cancel,
            None,
            timeout,
        )
        .await
        .map_err(|e| match e {
            NodeAppError::Cancelled | NodeAppError::Offline => e,
            _ => failed("The commit is not part of the cloned repository"),
        })?;
        match &resolved.branch {
            Some(branch) => {
                self.run(
                    root,
                    hooks,
                    Some(staging),
                    &["checkout", "-B", branch, &resolved.commit],
                    &ctx.cancel,
                    Some(ctx),
                    timeout,
                )
                .await?;
                // Tracking is best effort: a branch ref that moved is still a valid install.
                let _ = self
                    .run(
                        root,
                        hooks,
                        Some(staging),
                        &["branch", &format!("--set-upstream-to=origin/{branch}"), branch],
                        &ctx.cancel,
                        None,
                        timeout,
                    )
                    .await;
            }
            None => {
                self.run(
                    root,
                    hooks,
                    Some(staging),
                    &["checkout", "--detach", &resolved.commit],
                    &ctx.cancel,
                    Some(ctx),
                    timeout,
                )
                .await?;
            }
        }
        if self.head(root, hooks, staging).await? != resolved.commit {
            return Err(failed("The checkout does not match the resolved commit"));
        }
        Ok(())
    }

    pub async fn head(&self, root: &Path, hooks: &Path, dir: &Path) -> Result<String, NodeAppError> {
        let out = self
            .read(root, hooks, dir, &["rev-parse", "--verify", "HEAD^{commit}"])
            .await?;
        let head = out.trim();
        if is_object_id(head) {
            Ok(head.to_owned())
        } else {
            Err(failed("Git did not report an exact commit"))
        }
    }

    /// The text of `path` at `commit`, read from the object database without a checkout.
    pub async fn show_blob(
        &self,
        root: &Path,
        hooks: &Path,
        dir: &Path,
        commit: &str,
        path: &str,
    ) -> Option<String> {
        if !is_object_id(commit) {
            return None;
        }
        self.read(root, hooks, dir, &["show", &format!("{commit}:{path}")])
            .await
            .ok()
    }

    pub async fn origin(&self, root: &Path, hooks: &Path, dir: &Path) -> Result<Remote, NodeAppError> {
        let out = self
            .read(root, hooks, dir, &["config", "--get", "remote.origin.url"])
            .await?;
        validate_remote(out.trim(), self.allow_local).ok_or(NodeAppError::GitRefused)
    }

    pub async fn inspect(
        &self,
        root: &Path,
        hooks: &Path,
        dir: &Path,
    ) -> Result<Inspection, NodeAppError> {
        let porcelain = self
            .read(
                root,
                hooks,
                dir,
                &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
            )
            .await?;
        let dirty = porcelain_dirty(&porcelain).map_err(|_| failed("Git status was incomplete"))?;
        let git_dir = dir.join(".git");
        let unfinished_operation = [
            "MERGE_HEAD",
            "CHERRY_PICK_HEAD",
            "REVERT_HEAD",
            "rebase-apply",
            "rebase-merge",
            "sequencer",
        ]
        .iter()
        .any(|marker| git_dir.join(marker).exists());
        let upstream = self
            .read(
                root,
                hooks,
                dir,
                &["rev-parse", "--verify", "--symbolic-full-name", "@{upstream}"],
            )
            .await;
        let has_upstream = upstream.is_ok();
        let (ahead, behind) = if has_upstream {
            let counts = self
                .read(
                    root,
                    hooks,
                    dir,
                    &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"],
                )
                .await?;
            parse_counts(&counts).map_err(|_| failed("Git counts were incomplete"))?
        } else {
            (0, 0)
        };
        let status = self
            .read(root, hooks, dir, &["status", "--short", "--untracked-files=all"])
            .await
            .unwrap_or_default();
        let branch = self
            .read(root, hooks, dir, &["symbolic-ref", "--quiet", "--short", "HEAD"])
            .await
            .ok()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty());
        Ok(Inspection {
            state: RepoState {
                dirty,
                unfinished_operation,
                ahead,
                behind,
                has_upstream,
                status: status.chars().take(2000).collect(),
            },
            revision: Revision {
                commit: self.head(root, hooks, dir).await?,
                branch,
                tag: None,
            },
        })
    }

    pub async fn is_dirty(&self, root: &Path, hooks: &Path, dir: &Path) -> Option<bool> {
        let out = self
            .read(
                root,
                hooks,
                dir,
                &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
            )
            .await
            .ok()?;
        porcelain_dirty(&out).ok()
    }

    /// Fetches, then freezes the update target once. Returns the target commit.
    pub async fn freeze_update(
        &self,
        root: &Path,
        hooks: &Path,
        dir: &Path,
        reference: Option<&str>,
        ctx: &StepContext,
    ) -> Result<String, NodeAppError> {
        let before = self.inspect(root, hooks, dir).await?;
        let refused = |reason: domain::comfy_runtime::UpdateRefusal| {
            let _ = reason;
            NodeAppError::GitRefused
        };
        can_update(
            &before.state,
            &before.revision,
            if reference.is_some() {
                &UpdateTarget::LatestRelease
            } else {
                &UpdateTarget::BranchFfOnly
            },
        )
        .map_err(refused)?;
        // Only an HTTPS origin is ever contacted.
        self.origin(root, hooks, dir).await?;
        self.run(
            root,
            hooks,
            Some(dir),
            &["fetch", "--tags", "origin"],
            &ctx.cancel,
            Some(ctx),
            Duration::from_secs(1200),
        )
        .await?;
        // The tree may have changed while fetching: inspect again before choosing a target.
        let after = self.inspect(root, hooks, dir).await?;
        can_update(
            &after.state,
            &after.revision,
            if reference.is_some() {
                &UpdateTarget::LatestRelease
            } else {
                &UpdateTarget::BranchFfOnly
            },
        )
        .map_err(refused)?;
        let target = match reference.map(str::trim).filter(|r| !r.is_empty()) {
            None => {
                if after.state.ahead > 0 {
                    return Err(NodeAppError::GitRefused);
                }
                let out = self
                    .read(root, hooks, dir, &["rev-parse", "--verify", "@{upstream}^{commit}"])
                    .await?;
                out.trim().to_owned()
            }
            Some(reference) => self.local_target(root, hooks, dir, reference).await?,
        };
        if !is_object_id(&target) {
            return Err(failed("Git did not report an exact target commit"));
        }
        if target == after.revision.commit {
            return Err(NodeAppError::UpToDate);
        }
        if after.revision.branch.is_some() {
            // A branch only moves forward: the target must contain the current commit.
            let forward = self
                .read(
                    root,
                    hooks,
                    dir,
                    &["merge-base", "--is-ancestor", &after.revision.commit, &target],
                )
                .await
                .is_ok();
            if !forward {
                return Err(NodeAppError::GitRefused);
            }
        }
        Ok(target)
    }

    /// An explicit target must be a tag, a remote branch or a commit that exists locally after the
    /// fetch. A name that is both a tag and a branch is ambiguous.
    async fn local_target(
        &self,
        root: &Path,
        hooks: &Path,
        dir: &Path,
        reference: &str,
    ) -> Result<String, NodeAppError> {
        if is_object_id(reference) {
            let out = self
                .read(root, hooks, dir, &["rev-parse", "--verify", &format!("{reference}^{{commit}}")])
                .await
                .map_err(|_| failed("The commit was not found after fetching"))?;
            return Ok(out.trim().to_owned());
        }
        if !valid_ref_name(reference) {
            return Err(NodeAppError::InvalidRequest);
        }
        let tag = self
            .read(
                root,
                hooks,
                dir,
                &["rev-parse", "--verify", "--quiet", &format!("refs/tags/{reference}^{{commit}}")],
            )
            .await
            .ok();
        let branch = self
            .read(
                root,
                hooks,
                dir,
                &[
                    "rev-parse",
                    "--verify",
                    "--quiet",
                    &format!("refs/remotes/origin/{reference}^{{commit}}"),
                ],
            )
            .await
            .ok();
        match (tag, branch) {
            (Some(_), Some(_)) => Err(NodeAppError::AmbiguousReference),
            (Some(commit), None) | (None, Some(commit)) => Ok(commit.trim().to_owned()),
            (None, None) => Err(failed("The branch or tag was not found")),
        }
    }

    /// Applies the frozen target. Re-checks the tree, then fast-forwards a branch or moves a
    /// detached head. Local edits are never reset or discarded.
    pub async fn apply(
        &self,
        root: &Path,
        hooks: &Path,
        dir: &Path,
        expected_head: &str,
        target: &str,
        ctx: &StepContext,
    ) -> Result<(), NodeAppError> {
        if !is_object_id(target) {
            return Err(NodeAppError::InvalidRequest);
        }
        let now = self.inspect(root, hooks, dir).await?;
        if now.revision.commit != expected_head {
            return Err(NodeAppError::StalePlan);
        }
        can_update(&now.state, &now.revision, &UpdateTarget::LatestRelease)
            .map_err(|_| NodeAppError::GitRefused)?;
        let timeout = Duration::from_secs(600);
        if now.revision.branch.is_some() {
            self.run(
                root,
                hooks,
                Some(dir),
                &["merge", "--ff-only", target],
                &ctx.cancel,
                Some(ctx),
                timeout,
            )
            .await?;
        } else {
            self.run(
                root,
                hooks,
                Some(dir),
                &["checkout", "--detach", target],
                &ctx.cancel,
                Some(ctx),
                timeout,
            )
            .await?;
        }
        if self.head(root, hooks, dir).await? != target {
            return Err(failed("Git did not reach the exact frozen target"));
        }
        Ok(())
    }
}

impl Default for GitNodes {
    fn default() -> Self {
        Self::new()
    }
}

/// Remote identity of an observed repository, for comparison with a stored source.
pub fn identity_of(raw: &str) -> String {
    observed_identity(raw)
}
