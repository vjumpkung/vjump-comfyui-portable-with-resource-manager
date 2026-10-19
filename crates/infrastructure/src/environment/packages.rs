//! uv-based package management with the torch pin guard.

use std::{
    ffi::OsString,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

use application::environment::{
    InstallSpec, InstalledPackage, PackageInstaller, ProvisionError, ResolutionPreview, StepContext,
};
use async_trait::async_trait;
use domain::environment::{DistName, EnvId, PinSet};
use serde::Deserialize;

use super::{
    exec::{failure_summary, run_quiet, run_step},
    layout::{Layout, Tool, command, ensure_cache_dirs, venv_python},
};
use crate::shared::{
    fs::atomic_write,
    process::{CommandSpec, RunLimits},
};

pub struct UvPackages;

impl UvPackages {
    pub fn new() -> Self {
        Self
    }
}

impl Default for UvPackages {
    fn default() -> Self {
        Self::new()
    }
}

fn uv(layout: &Layout) -> CommandSpec {
    command(layout, Tool::Uv, layout.uv_exe()).arg("--no-config")
}

/// The arguments of `uv pip install` for `spec` (without the program and `--no-config`).
pub fn install_args(python: &Path, spec: &InstallSpec, dry_run: bool) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec!["pip".into(), "install".into(), "--python".into()];
    args.push(python.as_os_str().to_owned());
    if dry_run {
        args.push("--dry-run".into());
    }
    if !spec.allow_source_builds {
        // Source distributions run build hooks (third-party code); they need explicit consent.
        args.extend(["--only-binary".into(), ":all:".into()]);
    }
    if let Some(index) = &spec.index_url {
        args.extend(["--index-url".into(), index.into()]);
    }
    if let Some(constraints) = &spec.constraint_file {
        args.extend(["-c".into(), constraints.as_os_str().to_owned()]);
    }
    if spec.upgrade {
        args.push("--upgrade".into());
    }
    for file in &spec.requirement_files {
        if spec
            .guarded
            .as_ref()
            .is_some_and(|guarded| guarded.path == *file)
        {
            // The protected snapshot arrives on standard input (see `with_guarded`).
            args.extend(["-r".into(), "-".into()]);
        } else {
            args.extend(["-r".into(), file.as_os_str().to_owned()]);
        }
    }
    args.extend(spec.requirements.iter().map(OsString::from));
    args
}

/// Attaches the guarded requirements snapshot of `spec` to a command built from `install_args`.
/// uv reads the reviewed bytes from standard input instead of reopening the file by name, with
/// the guarded directory as its working directory. That resolves relative `-r`/`-c` includes
/// and local paths exactly as `-r <directory>/requirements.txt` would, and the command keeps
/// the node location protected until the command and any survivor of its cleanup have ended.
pub fn with_guarded(command: CommandSpec, spec: &InstallSpec) -> CommandSpec {
    match &spec.guarded {
        Some(guarded) if spec.requirement_files.contains(&guarded.path) => command
            .cwd(&guarded.directory)
            .stdin(guarded.text.clone())
            .retain(guarded.retained.clone()),
        _ => command,
    }
}

fn preflight_refused() -> ProvisionError {
    ProvisionError::Failed("Requirements need external inspection before previewing; editable and source-directory requirements are not permitted.".into())
}

/// A deliberately conservative subset of requirements syntax. Anything not understood here
/// is refused before uv can ask a source tree's build backend for metadata.
fn preflight_requirements(spec: &InstallSpec) -> Result<(), ProvisionError> {
    let directory = match &spec.guarded {
        Some(guarded) => guarded.directory.clone(),
        None => std::env::current_dir().map_err(|_| preflight_refused())?,
    };
    let mut pending = Vec::new();
    for requirement in &spec.requirements {
        preflight_line(requirement, &directory, &mut pending)?;
    }
    for path in spec
        .requirement_files
        .iter()
        .chain(spec.constraint_file.iter())
    {
        if let Some(guarded) = &spec.guarded
            && path == &guarded.path
        {
            preflight_text(&guarded.text, &guarded.directory, &mut pending)?;
        } else {
            pending.push(path.clone());
        }
    }
    let mut seen = std::collections::HashSet::new();
    let mut total = 0usize;
    while let Some(path) = pending.pop() {
        let path = path.canonicalize().map_err(|_| preflight_refused())?;
        if !seen.insert(path.clone()) {
            continue;
        }
        if seen.len() > 64 {
            return Err(preflight_refused());
        }
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::{
                FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
            };
            options
                .share_mode(FILE_SHARE_READ)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        }
        let file = options.open(&path).map_err(|_| preflight_refused())?;
        let metadata = file.metadata().map_err(|_| preflight_refused())?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes()
                & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
                != 0
            {
                return Err(preflight_refused());
            }
        }
        if !metadata.is_file() || metadata.len() > 256 * 1024 {
            return Err(preflight_refused());
        }
        let mut text = Vec::new();
        file.take(256 * 1024 + 1)
            .read_to_end(&mut text)
            .map_err(|_| preflight_refused())?;
        total += text.len();
        if total > 1024 * 1024 {
            return Err(preflight_refused());
        }
        preflight_text(
            &text,
            path.parent().ok_or_else(preflight_refused)?,
            &mut pending,
        )?;
    }
    Ok(())
}

fn preflight_text(
    text: &[u8],
    directory: &Path,
    pending: &mut Vec<PathBuf>,
) -> Result<(), ProvisionError> {
    if text.len() > 256 * 1024 {
        return Err(preflight_refused());
    }
    for line in std::str::from_utf8(text)
        .map_err(|_| preflight_refused())?
        .trim_start_matches('\u{feff}')
        .lines()
    {
        preflight_line(line, directory, pending)?;
    }
    Ok(())
}

fn preflight_line(
    line: &str,
    directory: &Path,
    pending: &mut Vec<PathBuf>,
) -> Result<(), ProvisionError> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return Ok(());
    }
    let line = line.split_once(" #").map_or(line, |(line, _)| line).trim();
    for prefix in [
        "--requirement=",
        "--constraint=",
        "--requirement ",
        "--constraint ",
        "-r",
        "-c",
    ] {
        if let Some(path) = line.strip_prefix(prefix) {
            let path = path.trim();
            if path.is_empty()
                || path.contains(['\'', '"', '$', '\0'])
                || path.chars().any(char::is_whitespace)
                || path.contains("://")
            {
                return Err(preflight_refused());
            }
            pending.push(directory.join(path));
            return Ok(());
        }
    }
    // An environment marker only decides whether the requirement applies. Its alphabet cannot
    // name a path or a URL, so it cannot turn a plain requirement into a source build.
    let line = match line.split_once(';') {
        Some((requirement, marker)) => {
            if marker.trim().is_empty()
                || !marker
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || " _.'\"()<>=!~,*+-".contains(c))
            {
                return Err(preflight_refused());
            }
            requirement.trim()
        }
        None => line,
    };
    if line.is_empty()
        || line.starts_with('-')
        || line.contains(['$', '\\', '\0'])
        || !line.is_ascii()
        || directory.join(line).is_dir()
    {
        return Err(preflight_refused());
    }
    let target = line
        .split_once('@')
        .map_or(line, |(_, target)| target.trim());
    if target.ends_with(".whl") {
        if target.chars().any(char::is_whitespace) {
            return Err(preflight_refused());
        }
        if target.starts_with("https://") || directory.join(target).is_file() {
            return Ok(());
        }
        return Err(preflight_refused());
    }
    if line.contains('@') || !line.as_bytes()[0].is_ascii_alphanumeric() {
        return Err(preflight_refused());
    }
    let name_end = line
        .find(|c: char| !c.is_ascii_alphanumeric() && !matches!(c, '-' | '_' | '.'))
        .unwrap_or(line.len());
    let mut rest = line[name_end..].trim();
    if let Some(extras) = rest.strip_prefix('[') {
        let (extras, tail) = extras.split_once(']').ok_or_else(preflight_refused)?;
        if extras.is_empty()
            || !extras
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ','))
        {
            return Err(preflight_refused());
        }
        rest = tail.trim();
    }
    if !rest.is_empty()
        && (!rest.starts_with(['=', '<', '>', '!', '~'])
            || !rest.chars().all(|c| {
                c.is_ascii_alphanumeric()
                    || matches!(
                        c,
                        '=' | '<' | '>' | '!' | '~' | ',' | '.' | '*' | '+' | '-' | '_' | ' '
                    )
            }))
    {
        return Err(preflight_refused());
    }
    Ok(())
}

#[derive(Deserialize)]
struct ListEntry {
    name: String,
    version: String,
}

pub fn parse_list(stdout: &str) -> Result<Vec<InstalledPackage>, ProvisionError> {
    let start = stdout.find('[').unwrap_or(0);
    let entries: Vec<ListEntry> = serde_json::from_str(&stdout[start..])
        .map_err(|_| ProvisionError::Failed("the package list could not be read".into()))?;
    Ok(entries
        .into_iter()
        .map(|entry| InstalledPackage {
            name: entry.name,
            version: entry.version,
        })
        .collect())
}

/// Parses the ` + name==version` / ` - name==version` lines of `uv pip install --dry-run`.
pub fn parse_dry_run(lines: &[String]) -> ResolutionPreview {
    let mut preview = ResolutionPreview::default();
    for line in lines {
        let trimmed = line.trim();
        let (target, rest) = if let Some(rest) = trimmed.strip_prefix("+ ") {
            (&mut preview.installs, rest)
        } else if let Some(rest) = trimmed.strip_prefix("- ") {
            (&mut preview.removals, rest)
        } else {
            continue;
        };
        if let Some((name, version)) = rest.split_once("==") {
            let name = name.trim();
            let version = version.split_whitespace().next().unwrap_or("");
            if !name.is_empty() && !version.is_empty() && !name.contains(' ') {
                target.push(InstalledPackage {
                    name: name.to_owned(),
                    version: version.to_owned(),
                });
            }
        }
    }
    preview
}

/// The constraints file content: exact protected pins plus the profile's extra constraints.
/// Names are normalized distribution names; no wildcards are ever emitted.
pub fn render_constraints(pins: &PinSet, extra: &[String]) -> String {
    let mut text = String::from(
        "# Generated by vjumpkung-comfyui-desktop. These versions are the torch pin guard.\n",
    );
    for (name, version) in pins.iter() {
        text.push_str(&format!("{name}=={version}\n"));
    }
    for line in extra {
        text.push_str(line.trim());
        text.push('\n');
    }
    text
}

#[async_trait]
impl PackageInstaller for UvPackages {
    async fn install(
        &self,
        root: &Path,
        env_path: &Path,
        spec: &InstallSpec,
        ctx: &StepContext,
    ) -> Result<(), ProvisionError> {
        let layout = Layout::new(root);
        ensure_cache_dirs(&layout)
            .map_err(|_| ProvisionError::Failed("the cache folders could not be created".into()))?;
        let command = with_guarded(
            uv(&layout).args(install_args(&venv_python(env_path), spec, false)),
            spec,
        );
        let limits = RunLimits {
            timeout: Some(Duration::from_secs(3 * 3600)),
            ..RunLimits::default()
        };
        run_step(&command, ctx, &limits).await.map(|_| ())
    }

    async fn uninstall(
        &self,
        root: &Path,
        env_path: &Path,
        names: &[DistName],
        ctx: &StepContext,
    ) -> Result<(), ProvisionError> {
        let layout = Layout::new(root);
        let command = uv(&layout)
            .args(["pip", "uninstall", "--python"])
            .arg(venv_python(env_path))
            .args(names.iter().map(|name| name.as_str()));
        let limits = RunLimits {
            timeout: Some(Duration::from_secs(900)),
            ..RunLimits::default()
        };
        run_step(&command, ctx, &limits).await.map(|_| ())
    }

    async fn preview_install(
        &self,
        root: &Path,
        env_path: &Path,
        spec: &InstallSpec,
        ctx: &StepContext,
    ) -> Result<ResolutionPreview, ProvisionError> {
        preflight_requirements(spec)?;
        let layout = Layout::new(root);
        let command = with_guarded(
            uv(&layout).args(install_args(&venv_python(env_path), spec, true)),
            spec,
        );
        let limits = RunLimits {
            max_tail_lines: 4000,
            timeout: Some(Duration::from_secs(900)),
            ..RunLimits::default()
        };
        match run_step(&command, ctx, &limits).await {
            Ok(output) => {
                let lines: Vec<String> = output.tail;
                Ok(parse_dry_run(&lines))
            }
            Err(error) => Err(failure_summary(error)),
        }
    }

    async fn list(
        &self,
        root: &Path,
        env_path: &Path,
        ctx: Option<&StepContext>,
    ) -> Result<Vec<InstalledPackage>, ProvisionError> {
        let layout = Layout::new(root);
        let command = uv(&layout)
            .args(["pip", "list", "--format", "json", "--python"])
            .arg(venv_python(env_path));
        let limits = RunLimits {
            timeout: Some(Duration::from_secs(120)),
            max_stdout_bytes: 16 << 20,
            ..RunLimits::default()
        };
        let result = match ctx {
            Some(ctx) => run_step(&command, ctx, &limits).await,
            None => run_quiet(&command, &application::shared::CancelToken::new(), &limits).await,
        };
        let output = result.map_err(failure_summary)?;
        parse_list(&output.stdout)
    }

    async fn write_constraints(
        &self,
        root: &Path,
        env: &EnvId,
        pins: &PinSet,
        extra: &[String],
    ) -> Result<PathBuf, ProvisionError> {
        let layout = Layout::new(root);
        let path = layout.constraints_file(env);
        let content = render_constraints(pins, extra);
        let target = path.clone();
        tokio::task::spawn_blocking(move || {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            atomic_write(&target, content.as_bytes())
        })
        .await
        .map_err(|_| ProvisionError::Failed("the constraints file could not be written".into()))?
        .map_err(|_| ProvisionError::Failed("the constraints file could not be written".into()))?;
        Ok(path)
    }

    async fn clean_cache(&self, root: &Path, ctx: &StepContext) -> Result<(), ProvisionError> {
        let layout = Layout::new(root);
        let command = uv(&layout).args(["cache", "clean"]);
        let limits = RunLimits {
            timeout: Some(Duration::from_secs(600)),
            ..RunLimits::default()
        };
        run_step(&command, ctx, &limits).await.map(|_| ())
    }
}

#[cfg(test)]
mod tests;
