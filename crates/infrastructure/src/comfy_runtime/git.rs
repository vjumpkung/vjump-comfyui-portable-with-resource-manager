//! Exact, frozen Git updates through the M1 subprocess runner.
use crate::{
    environment::{
        layout::{Layout, Tool, command},
        tools::git_program,
    },
    shared::process::{self, CommandSpec, RunLimits},
};
use application::{
    comfy_runtime::*,
    environment::{ComfyRef, ComfySource, StepContext},
    shared::CancelToken,
};
use async_trait::async_trait;
use domain::comfy_runtime::*;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

pub struct GitRepository {
    source: Arc<dyn ComfySource>,
    git_override: Option<PathBuf>,
}
impl GitRepository {
    pub fn new(source: Arc<dyn ComfySource>) -> Self {
        Self {
            source,
            git_override: None,
        }
    }
    pub fn with_git(source: Arc<dyn ComfySource>, git: PathBuf) -> Self {
        Self {
            source,
            git_override: Some(git),
        }
    }
    fn command(&self, root: &Path, args: &[&str]) -> Result<CommandSpec, RuntimeError> {
        let layout = Layout::new(root);
        let git = self
            .git_override
            .clone()
            .or_else(|| git_program(&layout))
            .ok_or_else(|| {
                RuntimeError::Failed("Git is unavailable; complete setup first".into())
            })?;
        Ok(command(&layout, Tool::Git, git)
            .args([
                "-c",
                "core.longpaths=true",
                "-c",
                "advice.detachedHead=false",
            ])
            .arg("-C")
            .arg(layout.comfyui_dir())
            .args(args.iter().copied()))
    }
    async fn run(
        &self,
        root: &Path,
        args: &[&str],
        cancel: &CancelToken,
    ) -> Result<String, RuntimeError> {
        let spec = self.command(root, args)?;
        let limits = RunLimits {
            timeout: Some(Duration::from_secs(120)),
            ..Default::default()
        };
        match process::run(&spec, &process::DiscardSink, cancel, &limits).await {
            Ok(output) if output.stdout.len() < limits.max_stdout_bytes => Ok(output.stdout),
            Ok(_) => Err(RuntimeError::Failed(
                "Git output exceeded the safe capture limit".into(),
            )),
            Err(process::RunError::Cancelled { .. }) => Err(RuntimeError::Cancelled),
            Err(_) => Err(RuntimeError::Failed(
                "Git command failed; inspect repository status and retry".into(),
            )),
        }
    }
    async fn read(&self, root: &Path, args: &[&str]) -> Result<String, RuntimeError> {
        self.run(root, args, &CancelToken::new()).await
    }
    async fn commit(&self, root: &Path, reference: &str) -> Result<String, RuntimeError> {
        let value = self
            .read(
                root,
                &["rev-parse", "--verify", "--end-of-options", reference],
            )
            .await?;
        let value = value.trim();
        if super::sqlite::oid(value) {
            Ok(value.into())
        } else {
            Err(RuntimeError::Failed(
                "Git did not resolve an exact commit".into(),
            ))
        }
    }
}

#[async_trait]
impl ComfyRepository for GitRepository {
    async fn revision(&self, root: &Path) -> Result<Revision, RuntimeError> {
        let commit = self.commit(root, "HEAD^{commit}").await?;
        let branch = self
            .read(root, &["symbolic-ref", "--quiet", "--short", "HEAD"])
            .await
            .ok()
            .map(|s| s.trim().to_owned());
        let tag = self
            .read(root, &["describe", "--tags", "--exact-match", "HEAD"])
            .await
            .ok()
            .map(|s| s.trim().to_owned());
        Ok(Revision {
            commit,
            branch,
            tag,
        })
    }
    async fn status(&self, root: &Path) -> Result<RepoState, RuntimeError> {
        let porcelain = self
            .read(
                root,
                &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
            )
            .await?;
        let dirty = porcelain_dirty(&porcelain)?;
        let status = self
            .read(root, &["status", "--untracked-files=all"])
            .await?;
        let mut unfinished_operation = false;
        for marker in [
            "MERGE_HEAD",
            "CHERRY_PICK_HEAD",
            "REVERT_HEAD",
            "rebase-apply",
            "rebase-merge",
            "sequencer",
        ] {
            let path = self
                .read(root, &["rev-parse", "--git-path", marker])
                .await?;
            let path = Path::new(path.trim());
            let resolved = if path.is_absolute() {
                path.to_path_buf()
            } else {
                root.join("ComfyUI").join(path)
            };
            if resolved
                .try_exists()
                .map_err(|_| RuntimeError::Failed("cannot assess pending Git operation".into()))?
            {
                unfinished_operation = true;
            }
        }
        let upstream = self
            .read(
                root,
                &[
                    "rev-parse",
                    "--verify",
                    "--symbolic-full-name",
                    "@{upstream}",
                ],
            )
            .await;
        let has_upstream = upstream.is_ok();
        let (ahead, behind) = if has_upstream {
            let counts = self
                .read(
                    root,
                    &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"],
                )
                .await?;
            parse_counts(&counts)?
        } else {
            (0, 0)
        };
        Ok(RepoState {
            dirty,
            unfinished_operation,
            ahead,
            behind,
            has_upstream,
            status,
        })
    }
    async fn fetch(&self, root: &Path, ctx: &StepContext) -> Result<(), RuntimeError> {
        self.run(root, &["fetch", "--tags"], &ctx.cancel)
            .await
            .map(|_| ())
    }
    async fn freeze(
        &self,
        root: &Path,
        target: &UpdateTarget,
    ) -> Result<FrozenUpdate, RuntimeError> {
        let tag = match target {
            UpdateTarget::Tag(tag) => Some(tag.clone()),
            UpdateTarget::LatestRelease => {
                let resolved = self.source.resolve(root, &ComfyRef::Latest).await?;
                let tag = Tag::new(resolved.reference).map_err(|_| RuntimeError::InvalidRecord)?;
                let commit = self
                    .commit(root, &format!("refs/tags/{}^{{commit}}", tag.as_str()))
                    .await?;
                if commit != resolved.commit {
                    return Err(RuntimeError::Failed(
                        "the fetched release tag differs from the resolved release; retry fetch"
                            .into(),
                    ));
                }
                return Ok(FrozenUpdate {
                    commit,
                    tag: Some(tag.as_str().into()),
                    fast_forward: false,
                });
            }
            UpdateTarget::BranchFfOnly => None,
        };
        if let Some(tag) = tag {
            let commit = self
                .commit(root, &format!("refs/tags/{}^{{commit}}", tag.as_str()))
                .await?;
            Ok(FrozenUpdate {
                commit,
                tag: Some(tag.as_str().into()),
                fast_forward: false,
            })
        } else {
            let rev = self.revision(root).await?;
            let state = self.status(root).await?;
            can_update(&state, &rev, target).map_err(|reason| RuntimeError::UpdateRefused {
                reason,
                status: state.status,
            })?;
            Ok(FrozenUpdate {
                commit: self.commit(root, "@{upstream}^{commit}").await?,
                tag: None,
                fast_forward: true,
            })
        }
    }
    async fn apply(
        &self,
        root: &Path,
        frozen: &FrozenUpdate,
        ctx: &StepContext,
    ) -> Result<(), RuntimeError> {
        if !super::sqlite::oid(&frozen.commit) {
            return Err(RuntimeError::InvalidRecord);
        }
        let state = self.status(root).await?;
        let target = if frozen.fast_forward {
            UpdateTarget::BranchFfOnly
        } else {
            UpdateTarget::Tag(
                Tag::new(frozen.tag.clone().ok_or(RuntimeError::InvalidRecord)?)
                    .map_err(|_| RuntimeError::InvalidRecord)?,
            )
        };
        let revision = self.revision(root).await?;
        can_update(&state, &revision, &target).map_err(|reason| RuntimeError::UpdateRefused {
            reason,
            status: state.status,
        })?;
        let args = if frozen.fast_forward {
            vec!["merge", "--ff-only", frozen.commit.as_str()]
        } else {
            vec!["checkout", "--detach", frozen.commit.as_str(), "--"]
        };
        self.run(root, &args, &ctx.cancel).await?;
        if self.revision(root).await?.commit != frozen.commit {
            return Err(RuntimeError::Failed(
                "Git did not reach the exact frozen target".into(),
            ));
        }
        Ok(())
    }
    async fn versions(&self, root: &Path) -> Result<VersionList, RuntimeError> {
        let tags = self
            .read(root, &["tag", "--list", "v*", "--sort=-v:refname"])
            .await?
            .lines()
            .map(str::to_owned)
            .collect();
        Ok(VersionList {
            current: self.revision(root).await?,
            tags,
        })
    }
}
pub(crate) fn porcelain_dirty(output: &str) -> Result<bool, RuntimeError> {
    // M1's bounded line capture adds a final LF to a NUL-terminated record.
    let output = output.strip_suffix('\n').unwrap_or(output);
    if output.is_empty() {
        return Ok(false);
    }
    if !output.ends_with('\0') || output.len() < 4 {
        return Err(RuntimeError::Failed(
            "Git status output is incomplete".into(),
        ));
    }
    Ok(true)
}
pub(crate) fn parse_counts(text: &str) -> Result<(u64, u64), RuntimeError> {
    let values: Vec<_> = text.split_whitespace().collect();
    if values.len() != 2 {
        return Err(RuntimeError::InvalidRecord);
    }
    Ok((
        values[0].parse().map_err(|_| RuntimeError::InvalidRecord)?,
        values[1].parse().map_err(|_| RuntimeError::InvalidRecord)?,
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nul_status_counts_as_dirty_even_for_newlines_and_renames() {
        assert!(!porcelain_dirty("").unwrap());
        for text in ["?? a\n b\0\n", "R  new\0old\0\n", "?? token=secret\0\n"] {
            assert!(porcelain_dirty(text).unwrap());
        }
        assert!(porcelain_dirty("?? truncated").is_err());
        assert_eq!(parse_counts("2\t3\n").unwrap(), (2, 3));
    }
}
