//! Pinned tool acquisition: uv, MinGit (Windows), system Git detection (Linux) and the isolated,
//! hash-locked `hf` tool environment.

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use application::environment::{ProvisionError, StepContext, ToolInfo, ToolProvisioner};
use async_trait::async_trait;
use domain::environment::PythonVersion;

use super::{
    exec::{run_quiet, run_step},
    layout::{Layout, Tool, command, ensure_cache_dirs},
    pins::{
        HF_TOOL_PYTHON, HF_VERSION, MIN_SYSTEM_GIT, MINGIT_GIT_VERSION, MINGIT_SHA256, MINGIT_URL,
        UV_VERSION, hf_requirements, uv_artifact,
    },
    python::ensure_managed_python,
};
use crate::shared::{
    archive::{ArchiveError, ArchiveKind, ExtractLimits, extract},
    download::{DownloadError, Downloader, sha256_file, sha256_hex},
    fs::atomic_write,
    process::RunLimits,
};

/// One pinned downloadable tool archive.
#[derive(Debug, Clone)]
pub struct ToolSource {
    pub url: String,
    pub sha256: String,
    pub kind: ArchiveKind,
    /// The executable's path inside the archive.
    pub executable: String,
    /// Arguments that make the executable print its version.
    pub version_args: Vec<OsString>,
    /// Text the version output must contain.
    pub version_contains: String,
}

#[derive(Debug, Clone)]
pub struct ToolSources {
    pub uv: ToolSource,
    pub mingit: ToolSource,
}

impl ToolSources {
    /// The pinned production sources.
    pub fn pinned() -> Self {
        let uv = uv_artifact();
        Self {
            uv: ToolSource {
                url: uv.url.to_owned(),
                sha256: uv.sha256.to_owned(),
                kind: if cfg!(windows) {
                    ArchiveKind::Zip
                } else {
                    ArchiveKind::TarGz
                },
                executable: uv.executable.to_owned(),
                version_args: vec!["--version".into()],
                version_contains: format!("uv {UV_VERSION} "),
            },
            mingit: ToolSource {
                url: MINGIT_URL.to_owned(),
                sha256: MINGIT_SHA256.to_owned(),
                kind: ArchiveKind::Zip,
                executable: "cmd/git.exe".to_owned(),
                version_args: vec!["--version".into()],
                version_contains: MINGIT_GIT_VERSION.to_owned(),
            },
        }
    }
}

const MAX_TOOL_BYTES: u64 = 512 << 20;

pub struct Tools {
    downloader: Downloader,
    sources: ToolSources,
}

impl Tools {
    pub fn new() -> Self {
        Self::with(Downloader::new(), ToolSources::pinned())
    }

    pub fn with(downloader: Downloader, sources: ToolSources) -> Self {
        Self {
            downloader,
            sources,
        }
    }
}

impl Default for Tools {
    fn default() -> Self {
        Self::new()
    }
}

fn failed(message: impl Into<String>) -> ProvisionError {
    ProvisionError::Failed(message.into())
}

fn download_error(error: DownloadError) -> ProvisionError {
    match error {
        DownloadError::Cancelled => ProvisionError::Cancelled,
        DownloadError::HashMismatch => {
            failed("The downloaded file does not match its pinned SHA-256 and was discarded.")
        }
        other if other.is_retryable() => ProvisionError::Retryable(format!(
            "The download failed ({other}). Check the connection and retry."
        )),
        other => failed(format!("The download failed: {other}.")),
    }
}

fn archive_error(error: ArchiveError) -> ProvisionError {
    failed(format!("The downloaded archive was rejected: {error}."))
}

/// Runs `<executable> <version args>` and returns the first output line when it contains the
/// expected text.
async fn check_version(
    layout: &Layout,
    executable: &Path,
    source: &ToolSource,
    ctx: &StepContext,
) -> Result<String, ProvisionError> {
    let spec = command(layout, Tool::Plain, executable).args(source.version_args.clone());
    let limits = RunLimits {
        timeout: Some(Duration::from_secs(60)),
        ..RunLimits::default()
    };
    let output = run_step(&spec, ctx, &limits).await?;
    let first = output
        .stdout
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned();
    if output.stdout.contains(&source.version_contains) {
        Ok(first)
    } else {
        Err(failed(format!(
            "The tool reports an unexpected version: {first}"
        )))
    }
}

/// Parses `git version 2.43.0` or `git version 2.55.0.windows.3`.
pub fn parse_git_version(text: &str) -> Option<(u32, u32)> {
    let version = text.trim().strip_prefix("git version ")?;
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

pub fn system_git_candidates() -> [&'static str; 3] {
    ["/usr/bin/git", "/usr/local/bin/git", "/bin/git"]
}

/// The Git executable the app uses: MinGit under `tools/git` on Windows, the system Git on Linux.
pub fn git_program(layout: &Layout) -> Option<PathBuf> {
    if cfg!(windows) {
        let git = layout.mingit_exe();
        git.is_file().then_some(git)
    } else {
        system_git_candidates()
            .iter()
            .map(PathBuf::from)
            .find(|path| path.is_file())
    }
}

struct Staged {
    stage: PathBuf,
    archive: PathBuf,
}

impl Staged {
    fn cleanup(&self) {
        let _ = fs::remove_dir_all(&self.stage);
        let _ = fs::remove_file(&self.archive);
    }
}

impl Tools {
    /// Downloads, verifies and extracts `source` into a fresh staging directory.
    async fn fetch_and_extract(
        &self,
        layout: &Layout,
        source: &ToolSource,
        name: &str,
        ctx: &StepContext,
    ) -> Result<Staged, ProvisionError> {
        ensure_cache_dirs(layout).map_err(|_| failed("the cache folders could not be created"))?;
        let extension = match source.kind {
            ArchiveKind::Zip => "zip",
            ArchiveKind::TarGz => "tar.gz",
        };
        let archive = layout
            .downloads_dir()
            .join("tools")
            .join(format!("{name}-{}.{extension}", &source.sha256[..12]));
        let reusable = archive.is_file()
            && sha256_file(&archive).is_ok_and(|hash| hash.eq_ignore_ascii_case(&source.sha256));
        if !reusable {
            ctx.log(format!("Downloading {name}"));
            let progress_ctx = ctx.clone();
            self.downloader
                .fetch(
                    &source.url,
                    &archive,
                    &source.sha256,
                    MAX_TOOL_BYTES,
                    &ctx.cancel,
                    &move |done, total| progress_ctx.progress(done, total),
                )
                .await
                .map_err(download_error)?;
        }
        let stage = layout.tools_dir().join(format!(".stage-{name}"));
        let staged = Staged {
            stage: stage.clone(),
            archive: archive.clone(),
        };
        let _ = fs::remove_dir_all(&stage);
        let kind = source.kind;
        let extraction = tokio::task::spawn_blocking(move || {
            extract(&archive, kind, &stage, ExtractLimits::default())
        })
        .await
        .map_err(|_| failed("the archive could not be extracted"))?;
        if let Err(error) = extraction {
            staged.cleanup();
            return Err(archive_error(error));
        }
        Ok(staged)
    }

    async fn install_tool(
        &self,
        layout: &Layout,
        source: &ToolSource,
        name: &str,
        final_dir: &Path,
        flatten_executable_directory: bool,
        ctx: &StepContext,
    ) -> Result<String, ProvisionError> {
        let staged = self.fetch_and_extract(layout, source, name, ctx).await?;
        let result = self
            .publish_tool(
                layout,
                source,
                &staged,
                final_dir,
                flatten_executable_directory,
                ctx,
            )
            .await;
        staged.cleanup();
        result
    }

    async fn publish_tool(
        &self,
        layout: &Layout,
        source: &ToolSource,
        staged: &Staged,
        final_dir: &Path,
        flatten: bool,
        ctx: &StepContext,
    ) -> Result<String, ProvisionError> {
        let executable_in_stage = staged.stage.join(&source.executable);
        if !executable_in_stage.is_file() {
            return Err(failed(
                "The archive does not contain the expected executable.",
            ));
        }
        // `ready` holds the final directory content; it is verified before it replaces anything.
        let ready = if flatten {
            let ready = layout.tools_dir().join(format!(
                ".ready-{}",
                final_dir
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
            ));
            let _ = fs::remove_dir_all(&ready);
            fs::create_dir_all(&ready)
                .map_err(|_| failed("the tool folder could not be created"))?;
            let source_dir = executable_in_stage
                .parent()
                .ok_or_else(|| failed("the archive layout is unexpected"))?;
            for entry in fs::read_dir(source_dir)
                .map_err(|_| failed("the archive could not be read"))?
                .flatten()
            {
                if entry.path().is_file() {
                    fs::copy(entry.path(), ready.join(entry.file_name()))
                        .map_err(|_| failed("the tool files could not be copied"))?;
                    copy_executable_bit(&entry.path(), &ready.join(entry.file_name()));
                }
            }
            ready
        } else {
            staged.stage.clone()
        };
        let executable = if flatten {
            ready.join(
                Path::new(&source.executable)
                    .file_name()
                    .unwrap_or_default(),
            )
        } else {
            ready.join(&source.executable)
        };
        let version = match check_version(layout, &executable, source, ctx).await {
            Ok(version) => version,
            Err(error) => {
                if flatten {
                    let _ = fs::remove_dir_all(&ready);
                }
                return Err(error);
            }
        };
        if final_dir.exists() {
            fs::remove_dir_all(final_dir).map_err(|_| {
                failed(
                    "The previous tool folder could not be replaced. Close programs that use it.",
                )
            })?;
        }
        fs::rename(&ready, final_dir).map_err(|_| failed("the tool could not be published"))?;
        Ok(version)
    }

    async fn ensure_hf(
        &self,
        layout: &Layout,
        ctx: &StepContext,
    ) -> Result<ToolInfo, ProvisionError> {
        let version: PythonVersion = HF_TOOL_PYTHON
            .parse()
            .map_err(|_| failed("the hf tool Python pin is invalid"))?;
        let interpreter = ensure_managed_python(layout, version, ctx).await?;
        let requirements = hf_requirements();
        let marker_text = format!(
            "hf={HF_VERSION}\npython={HF_TOOL_PYTHON}\nlock={}\n",
            sha256_hex(requirements.as_bytes())
        );
        let marker = layout.hf_dir().join(".vj-lock");
        let hf = layout.hf_exe();
        if hf.is_file() && fs::read_to_string(&marker).is_ok_and(|text| text == marker_text) {
            return Ok(ToolInfo {
                name: "hf".into(),
                version: HF_VERSION.into(),
            });
        }
        self.check_cancel(ctx)?;
        let hf_dir = layout.hf_dir();
        if hf_dir.exists() {
            fs::remove_dir_all(&hf_dir).map_err(|_| {
                failed("The previous hf tool folder could not be replaced. Close programs that use it.")
            })?;
        }
        let limits = RunLimits {
            timeout: Some(Duration::from_secs(900)),
            ..RunLimits::default()
        };
        let venv = command(layout, Tool::Uv, layout.uv_exe())
            .arg("--no-config")
            .arg("venv")
            .arg("--no-project")
            .arg("--python")
            .arg(&interpreter)
            .arg(&hf_dir);
        run_step(&venv, ctx, &limits).await?;
        let lock = layout.cache_dir().join("hf-requirements.txt");
        atomic_write(&lock, requirements.as_bytes())
            .map_err(|_| failed("the hf lock file could not be written"))?;
        let sync = command(layout, Tool::Uv, layout.uv_exe())
            .arg("--no-config")
            .args(["pip", "sync", "--require-hashes", "--only-binary", ":all:"])
            .arg("--python")
            .arg(super::layout::venv_python(&hf_dir))
            .arg(&lock);
        run_step(&sync, ctx, &limits).await?;
        if !hf.is_file() {
            return Err(failed(
                "The hf tool was installed but its executable was not found.",
            ));
        }
        atomic_write(&marker, marker_text.as_bytes())
            .map_err(|_| failed("the hf tool marker could not be written"))?;
        Ok(ToolInfo {
            name: "hf".into(),
            version: HF_VERSION.into(),
        })
    }

    fn check_cancel(&self, ctx: &StepContext) -> Result<(), ProvisionError> {
        if ctx.cancel.is_cancelled() {
            Err(ProvisionError::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[cfg(unix)]
fn copy_executable_bit(from: &Path, to: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(metadata) = fs::metadata(from) {
        let _ = fs::set_permissions(
            to,
            fs::Permissions::from_mode(metadata.permissions().mode()),
        );
    }
}

#[cfg(not(unix))]
fn copy_executable_bit(_from: &Path, _to: &Path) {}

#[async_trait]
impl ToolProvisioner for Tools {
    async fn ensure_uv(&self, root: &Path, ctx: &StepContext) -> Result<ToolInfo, ProvisionError> {
        let layout = Layout::new(root);
        let source = &self.sources.uv;
        let executable = layout.uv_exe();
        if executable.is_file()
            && let Ok(version) = check_version(&layout, &executable, source, ctx).await
        {
            return Ok(ToolInfo {
                name: "uv".into(),
                version,
            });
        }
        let version = self
            .install_tool(&layout, source, "uv", &layout.uv_dir(), true, ctx)
            .await?;
        Ok(ToolInfo {
            name: "uv".into(),
            version,
        })
    }

    async fn ensure_git(&self, root: &Path, ctx: &StepContext) -> Result<ToolInfo, ProvisionError> {
        let layout = Layout::new(root);
        if cfg!(windows) {
            let source = &self.sources.mingit;
            let executable = layout.mingit_exe();
            if executable.is_file()
                && let Ok(version) = check_version(&layout, &executable, source, ctx).await
            {
                return Ok(ToolInfo {
                    name: "git".into(),
                    version,
                });
            }
            let version = self
                .install_tool(&layout, source, "mingit", &layout.git_dir(), false, ctx)
                .await?;
            return Ok(ToolInfo {
                name: "git".into(),
                version,
            });
        }
        let Some(git) = git_program(&layout) else {
            return Err(failed(format!(
                "Git {}.{} or newer is required but was not found. Install it with your package \
                 manager (for example `sudo apt install git`) and retry.",
                MIN_SYSTEM_GIT.0, MIN_SYSTEM_GIT.1
            )));
        };
        let spec = command(&layout, Tool::Git, &git).arg("--version");
        let output = run_quiet(
            &spec.clone().retain(ctx.retained.clone()),
            &ctx.cancel,
            &RunLimits::default(),
        )
        .await?;
        let line = output
            .stdout
            .lines()
            .next()
            .unwrap_or_default()
            .trim()
            .to_owned();
        match parse_git_version(&line) {
            Some(version) if version >= MIN_SYSTEM_GIT => Ok(ToolInfo {
                name: "git".into(),
                version: line,
            }),
            _ => Err(failed(format!(
                "The system Git ({line}) is older than {}.{}. Upgrade Git and retry.",
                MIN_SYSTEM_GIT.0, MIN_SYSTEM_GIT.1
            ))),
        }
    }

    async fn ensure_hf_tool(
        &self,
        root: &Path,
        ctx: &StepContext,
    ) -> Result<ToolInfo, ProvisionError> {
        self.ensure_hf(&Layout::new(root), ctx).await
    }
}

#[cfg(test)]
mod tests;
