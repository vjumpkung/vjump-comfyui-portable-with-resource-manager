//! The ComfyUI source: release resolution through the GitHub API and Git subprocesses for
//! tags, clones and checkouts. Fresh clones are staged and published into empty scaffolds;
//! existing user content is never replaced.

use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use application::environment::{
    Checkout, ComfyRef, ComfySource, ProvisionError, RequirementFiles, ResolvedGit, StepContext,
};
use async_trait::async_trait;
use serde::Deserialize;

use super::{
    exec::{failure_summary, looks_transient, run_quiet, run_step},
    layout::{Layout, Tool, command},
    pins::{COMFYUI_RELEASES_API, COMFYUI_URL, MANAGER_URL},
    tools::git_program,
};
use crate::shared::{
    download::Downloader,
    process::{CommandSpec, RunLimits},
};

mod publish;

#[derive(Debug, Clone)]
pub struct Endpoints {
    pub comfyui_url: String,
    pub manager_url: String,
    pub releases_api: String,
}

impl Endpoints {
    pub fn pinned() -> Self {
        Self {
            comfyui_url: COMFYUI_URL.to_owned(),
            manager_url: MANAGER_URL.to_owned(),
            releases_api: COMFYUI_RELEASES_API.to_owned(),
        }
    }
}

pub struct GitSource {
    downloader: Downloader,
    endpoints: Endpoints,
    git_override: Option<PathBuf>,
    #[cfg(test)]
    publish_fault: std::sync::Mutex<Option<publish::Fault>>,
}

impl GitSource {
    pub fn new() -> Self {
        Self::with(Downloader::new(), Endpoints::pinned(), None)
    }

    pub fn with(
        downloader: Downloader,
        endpoints: Endpoints,
        git_override: Option<PathBuf>,
    ) -> Self {
        Self {
            downloader,
            endpoints,
            git_override,
            #[cfg(test)]
            publish_fault: Default::default(),
        }
    }

    fn git(&self, layout: &Layout) -> Result<PathBuf, ProvisionError> {
        self.git_override
            .clone()
            .or_else(|| git_program(layout))
            .ok_or_else(|| {
                failed("Git is not available yet. Setup provisions it in the tools step.")
            })
    }

    fn git_command(&self, layout: &Layout) -> Result<CommandSpec, ProvisionError> {
        Ok(command(layout, Tool::Git, self.git(layout)?).args([
            "-c",
            "core.longpaths=true",
            "-c",
            "advice.detachedHead=false",
        ]))
    }
}

impl Default for GitSource {
    fn default() -> Self {
        Self::new()
    }
}

fn failed(message: impl Into<String>) -> ProvisionError {
    ProvisionError::Failed(message.into())
}

/// Tag and branch names that are safe to pass to Git: no option injection, no revision syntax.
pub fn valid_ref_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 200
        && !name.starts_with(['-', '/', '.'])
        && !name.ends_with(['/', '.'])
        && !name.contains("..")
        && !name.contains("//")
        && !name.contains("@{")
        && !name.ends_with(".lock")
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'/' | b'-'))
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
}

/// The refs printed by `git ls-remote`: `<sha>\t<ref>`.
pub fn parse_ls_remote(stdout: &str) -> Vec<(String, String)> {
    stdout
        .lines()
        .filter_map(|line| {
            let (sha, name) = line.split_once('\t')?;
            (sha.len() >= 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit()))
                .then(|| (sha.to_owned(), name.trim().to_owned()))
        })
        .collect()
}

impl GitSource {
    async fn ls_remote(
        &self,
        layout: &Layout,
        url: &str,
        patterns: &[String],
    ) -> Result<Vec<(String, String)>, ProvisionError> {
        let spec = self
            .git_command(layout)?
            .arg("ls-remote")
            .arg(url)
            .args(patterns.iter().map(String::as_str));
        let limits = RunLimits {
            timeout: Some(Duration::from_secs(120)),
            ..RunLimits::default()
        };
        match run_quiet(&spec, &application::shared::CancelToken::new(), &limits).await {
            Ok(output) => Ok(parse_ls_remote(&output.stdout)),
            Err(ProvisionError::CommandFailed(failure)) if looks_transient(&failure.tail) => {
                Err(ProvisionError::Retryable(
                    "The repository could not be reached. Check the connection and retry.".into(),
                ))
            }
            Err(error) => Err(failure_summary(error)),
        }
    }

    async fn peel_tag(
        &self,
        layout: &Layout,
        url: &str,
        tag: &str,
    ) -> Result<String, ProvisionError> {
        if !valid_ref_name(tag) {
            return Err(failed("The tag name is not valid."));
        }
        let refs = self
            .ls_remote(
                layout,
                url,
                &[format!("refs/tags/{tag}"), format!("refs/tags/{tag}^{{}}")],
            )
            .await?;
        let peeled = format!("refs/tags/{tag}^{{}}");
        let direct = format!("refs/tags/{tag}");
        refs.iter()
            .find(|(_, name)| *name == peeled)
            .or_else(|| refs.iter().find(|(_, name)| *name == direct))
            .map(|(sha, _)| sha.clone())
            .ok_or_else(|| failed(format!("The tag {tag} was not found in the repository.")))
    }

    async fn latest_tag(&self) -> Result<String, ProvisionError> {
        if !self.downloader.url_allowed(&self.endpoints.releases_api) {
            return Err(failed("The release endpoint must use HTTPS."));
        }
        let response = self
            .downloader
            .client()
            .get(&self.endpoints.releases_api)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|_| {
                ProvisionError::Retryable(
                    "GitHub could not be reached to find the latest ComfyUI release. Retry later."
                        .into(),
                )
            })?;
        let status = response.status();
        let rate_limited = status.as_u16() == 429
            || (status.as_u16() == 403
                && (response
                    .headers()
                    .get("x-ratelimit-remaining")
                    .is_some_and(|value| value == "0")
                    || response.headers().contains_key("retry-after")));
        if rate_limited {
            return Err(ProvisionError::Retryable(
                "GitHub's rate limit was reached while resolving the latest ComfyUI release. \
                 Retry later; the requested source was not changed."
                    .into(),
            ));
        }
        if status.is_server_error() {
            return Err(ProvisionError::Retryable(format!(
                "GitHub answered HTTP {} while resolving the latest release. Retry later.",
                status.as_u16()
            )));
        }
        if !status.is_success() {
            return Err(failed(format!(
                "GitHub answered HTTP {} while resolving the latest ComfyUI release.",
                status.as_u16()
            )));
        }
        let release: Release = response
            .json()
            .await
            .map_err(|_| failed("The latest release answer from GitHub was not understood."))?;
        Ok(release.tag_name)
    }

    async fn clone_checkout(
        &self,
        layout: &Layout,
        url: &str,
        resolved: &ResolvedGit,
        final_dir: &Path,
        stage_name: &str,
        ctx: &StepContext,
    ) -> Result<(), ProvisionError> {
        let parent = final_dir
            .parent()
            .ok_or_else(|| failed("the destination has no parent folder"))?;
        fs::create_dir_all(parent)
            .map_err(|_| failed("the destination folder could not be created"))?;
        let stage = parent.join(format!(".{stage_name}.cloning"));
        let publication = publish::Publication::open(final_dir, stage_name)
            .map_err(|_| failed(publish::INTERRUPTED))?;
        if self.recover_publication(&publication, layout, ctx).await? {
            return Ok(());
        }
        publication.discard_stage().map_err(|_| {
            failed("The previous staging folder could not be removed. It was left for inspection.")
        })?;
        let result = self
            .clone_into_stage(layout, url, resolved, &stage, ctx)
            .await;
        if let Err(error) = result {
            let _ = publication.discard_stage();
            return Err(error);
        }
        match fs::symlink_metadata(final_dir) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if fs::rename(&stage, final_dir).is_err() {
                    let _ = publication.discard_stage();
                    return Err(failed(publish::CONFLICT));
                }
                Ok(())
            }
            Ok(_) => self.publish_scaffold(&publication, layout, ctx).await,
            Err(_) => {
                let _ = publication.discard_stage();
                Err(failed("The ComfyUI folder could not be inspected."))
            }
        }
    }

    async fn clone_into_stage(
        &self,
        layout: &Layout,
        url: &str,
        resolved: &ResolvedGit,
        stage: &Path,
        ctx: &StepContext,
    ) -> Result<(), ProvisionError> {
        let limits = RunLimits {
            timeout: Some(Duration::from_secs(3600)),
            ..RunLimits::default()
        };
        let clone = self
            .git_command(layout)?
            .args(["clone", "--progress"])
            .arg(url)
            .arg(stage);
        run_step(&clone, ctx, &limits).await?;
        let checkout = match &resolved.branch {
            Some(branch) => self.git_command(layout)?.arg("-C").arg(stage).args([
                "checkout",
                "-B",
                branch.as_str(),
                "--track",
                &format!("origin/{branch}"),
            ]),
            None => self.git_command(layout)?.arg("-C").arg(stage).args([
                "checkout",
                "--detach",
                resolved.commit.as_str(),
            ]),
        };
        run_step(&checkout, ctx, &limits).await?;
        let head = self.head_commit(layout, stage, ctx).await?;
        if resolved.branch.is_none() && head != resolved.commit {
            return Err(failed(
                "The checkout does not match the resolved commit, so it was discarded.",
            ));
        }
        Ok(())
    }

    async fn head_commit(
        &self,
        layout: &Layout,
        repository: &Path,
        ctx: &StepContext,
    ) -> Result<String, ProvisionError> {
        self.head_commit_with_limits(layout, repository, ctx, &ctx.cancel, &RunLimits::default())
            .await
    }

    async fn head_commit_with_limits(
        &self,
        layout: &Layout,
        repository: &Path,
        ctx: &StepContext,
        cancel: &application::shared::CancelToken,
        limits: &RunLimits,
    ) -> Result<String, ProvisionError> {
        let spec = self
            .git_command(layout)?
            .arg("-C")
            .arg(repository)
            .args(["rev-parse", "HEAD"]);
        let output = run_quiet(&spec.clone().retain(ctx.retained.clone()), cancel, limits).await?;
        output
            .stdout
            .lines()
            .map(str::trim)
            .find(|line| line.len() >= 40 && line.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .map(str::to_owned)
            .ok_or_else(|| failed("The checked-out commit could not be read."))
    }
}

fn is_empty_dir(path: &Path) -> bool {
    fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_none())
}

#[async_trait]
impl ComfySource for GitSource {
    async fn resolve(
        &self,
        root: &Path,
        reference: &ComfyRef,
    ) -> Result<ResolvedGit, ProvisionError> {
        let layout = Layout::new(root);
        let url = self.endpoints.comfyui_url.clone();
        match reference {
            ComfyRef::Latest => {
                let tag = self.latest_tag().await?;
                let commit = self.peel_tag(&layout, &url, &tag).await?;
                Ok(ResolvedGit {
                    url,
                    reference: tag,
                    branch: None,
                    commit,
                })
            }
            ComfyRef::Tag(tag) => {
                let commit = self.peel_tag(&layout, &url, tag).await?;
                Ok(ResolvedGit {
                    url,
                    reference: tag.clone(),
                    branch: None,
                    commit,
                })
            }
            ComfyRef::Branch(branch) => {
                if !valid_ref_name(branch) {
                    return Err(failed("The branch name is not valid."));
                }
                let name = format!("refs/heads/{branch}");
                let refs = self
                    .ls_remote(&layout, &url, std::slice::from_ref(&name))
                    .await?;
                let commit = refs
                    .iter()
                    .find(|(_, found)| *found == name)
                    .map(|(sha, _)| sha.clone())
                    .ok_or_else(|| failed(format!("The branch {branch} was not found.")))?;
                Ok(ResolvedGit {
                    url,
                    reference: branch.clone(),
                    branch: Some(branch.clone()),
                    commit,
                })
            }
            ComfyRef::KeepCurrent => Err(failed(
                "There is no remote source to resolve for a kept checkout.",
            )),
        }
    }

    async fn inspect_checkout(&self, root: &Path) -> Result<Checkout, ProvisionError> {
        let layout = Layout::new(root);
        let dir = layout.comfyui_dir();
        let meta = match fs::symlink_metadata(&dir) {
            Ok(meta) => meta,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Checkout::Absent);
            }
            Err(_) => return Err(failed("The ComfyUI folder could not be inspected.")),
        };
        if !publish::real_directory(&meta) {
            return Ok(Checkout::NotGit);
        }
        let pinned = crate::shared::model_paths::pin_absolute(&dir)
            .map_err(|_| failed("The ComfyUI folder could not be inspected."))?;
        match fs::symlink_metadata(pinned.anchored_path().join(".git")) {
            Ok(_) => {} // Keep Git's existing checkout validation ahead of scaffold classification.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let pending = publish::Publication::open(&dir, "ComfyUI")
                    .is_ok_and(|publication| publication.pending());
                return Ok(if pending || publish::is_scaffold(&pinned) {
                    Checkout::Absent
                } else {
                    Checkout::NotGit
                });
            }
            Err(_) => return Err(failed("The ComfyUI folder could not be inspected.")),
        }
        let spec = self
            .git_command(&layout)?
            .arg("-C")
            .arg(&dir)
            .args(["rev-parse", "HEAD"]);
        let output = run_quiet(
            &spec,
            &application::shared::CancelToken::new(),
            &RunLimits::default(),
        )
        .await;
        match output {
            Ok(output) => output
                .stdout
                .lines()
                .map(str::trim)
                .find(|line| line.len() >= 40)
                .map(|commit| Checkout::Repo {
                    commit: commit.to_owned(),
                })
                .ok_or_else(|| failed("The ComfyUI checkout's commit could not be read.")),
            // A `.git` folder that Git cannot read is not a checkout we may alter.
            Err(ProvisionError::CommandFailed(_)) => Ok(Checkout::NotGit),
            Err(error) => Err(error),
        }
    }

    async fn clone_comfyui(
        &self,
        root: &Path,
        resolved: &ResolvedGit,
        ctx: &StepContext,
    ) -> Result<(), ProvisionError> {
        let layout = Layout::new(root);
        let url = if resolved.url.is_empty() {
            self.endpoints.comfyui_url.clone()
        } else {
            resolved.url.clone()
        };
        self.clone_checkout(
            &layout,
            &url,
            resolved,
            &layout.comfyui_dir(),
            "ComfyUI",
            ctx,
        )
        .await
    }

    async fn requirement_files(&self, root: &Path) -> Result<RequirementFiles, ProvisionError> {
        let layout = Layout::new(root);
        let comfyui = layout.comfyui_dir().join("requirements.txt");
        if !comfyui.is_file() {
            return Err(failed("ComfyUI's requirements.txt was not found."));
        }
        Ok(RequirementFiles { comfyui })
    }

    async fn resolve_manager(&self, root: &Path) -> Result<ResolvedGit, ProvisionError> {
        let layout = Layout::new(root);
        let url = self.endpoints.manager_url.clone();
        let refs = self.ls_remote(&layout, &url, &["HEAD".to_owned()]).await?;
        let commit = refs
            .iter()
            .find(|(_, name)| name == "HEAD")
            .map(|(sha, _)| sha.clone())
            .ok_or_else(|| failed("The ComfyUI-Manager repository has no HEAD."))?;
        Ok(ResolvedGit {
            url,
            reference: "HEAD".into(),
            branch: None,
            commit,
        })
    }

    async fn install_manager(
        &self,
        root: &Path,
        resolved: &ResolvedGit,
        ctx: &StepContext,
    ) -> Result<Vec<PathBuf>, ProvisionError> {
        let layout = Layout::new(root);
        let destination = layout
            .comfyui_dir()
            .join("custom_nodes")
            .join("ComfyUI-Manager");
        if destination.exists() && !is_empty_dir(&destination) {
            if !destination.join(".git").exists() {
                return Err(failed(
                    "custom_nodes/ComfyUI-Manager exists but is not a git checkout. It was left unchanged.",
                ));
            }
            let head = self.head_commit(&layout, &destination, ctx).await?;
            if head != resolved.commit {
                return Err(failed(
                    "The existing ComfyUI-Manager checkout is at another commit than the one you approved. It was left unchanged.",
                ));
            }
        } else {
            self.clone_checkout(
                &layout,
                &resolved.url,
                resolved,
                &destination,
                "ComfyUI-Manager",
                ctx,
            )
            .await?;
        }
        let requirements = destination.join("requirements.txt");
        Ok(if requirements.is_file() {
            vec![requirements]
        } else {
            Vec::new()
        })
    }
}

#[cfg(test)]
mod tests;
