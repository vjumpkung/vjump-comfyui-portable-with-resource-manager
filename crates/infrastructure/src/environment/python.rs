//! Managed CPython, virtual environments, Python validation and seeding.

use std::{
    collections::HashMap,
    ffi::OsString,
    fs,
    path::{Component, Path, PathBuf},
    time::Duration,
};

use application::{
    environment::{
        BlockedFeature, CheckId, ProvisionError, PythonProvisioner, PythonSourceInput,
        PythonValidationReport, StepContext, ValidationCheck, ValidationTarget, VenvInfo,
    },
    shared::CancelToken,
};
use async_trait::async_trait;
use domain::{
    environment::{OsKind, PythonEnvironment, PythonVersion},
    shared::model_values::SafeFileName,
};
use serde::Deserialize;

use super::{
    exec::{run_quiet, run_step},
    layout::{Layout, Tool, command, ensure_cache_dirs, venv_python, venv_scripts},
    pins::{MSVC_RUNTIME, PYTHON_DOWNLOADS_JSON},
};
use crate::shared::{fs::atomic_write, model_paths::pin_absolute, process::RunLimits};

const PROBE_MARKER: &str = "VJ-PROBE:";

const PROBE_SCRIPT: &str = r#"
import importlib.util, json, os, struct, sys, sysconfig
paths = sysconfig.get_paths()
include = sysconfig.get_config_var("INCLUDEPY") or paths.get("include") or ""
base = sys.base_prefix
major, minor, patch = sys.version_info[:3]
library = os.path.join(base, "libs", "python%d%d.lib" % (major, minor))
print("VJ-PROBE:" + json.dumps({
    "version": [major, minor, patch],
    "bits": struct.calcsize("P") * 8,
    "implementation": sys.implementation.name,
    "base_prefix": base,
    "header": bool(include) and os.path.isfile(os.path.join(include, "Python.h")),
    "import_library": os.path.isfile(library),
    "pip": importlib.util.find_spec("pip") is not None,
}))
"#;

const MSVC_SCRIPT: &str = r#"
import ctypes
for name in ("msvcp140.dll", "vcruntime140.dll", "vcruntime140_1.dll"):
    ctypes.WinDLL(name)
print("VJ-PROBE:ok")
"#;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Probe {
    pub version: [u8; 3],
    pub bits: u32,
    pub implementation: String,
    pub base_prefix: String,
    pub header: bool,
    pub import_library: bool,
    pub pip: bool,
}

pub fn parse_probe(stdout: &str) -> Option<Probe> {
    stdout
        .lines()
        .find_map(|line| line.trim().strip_prefix(PROBE_MARKER))
        .and_then(|json| serde_json::from_str(json).ok())
}

/// The checks derived from a probe. Pure so every branch is testable.
pub fn checks_from_probe(
    probe: &Probe,
    os: OsKind,
    msvc_runtime_loads: Option<bool>,
) -> Vec<ValidationCheck> {
    let version = PythonVersion::new(probe.version[0], probe.version[1], probe.version[2]);
    let supported = version.is_supported() && probe.implementation == "cpython";
    let mut checks = vec![
        ValidationCheck {
            id: CheckId::VersionSupported,
            passed: supported,
            detail: if supported {
                format!("CPython {version}")
            } else {
                format!(
                    "{} {version} is outside the supported range (CPython 3.12 to 3.14)",
                    probe.implementation
                )
            },
            blocks: if supported {
                Vec::new()
            } else {
                vec![BlockedFeature::Setup]
            },
        },
        ValidationCheck {
            id: CheckId::Is64Bit,
            passed: probe.bits == 64,
            detail: format!("{}-bit interpreter", probe.bits),
            blocks: if probe.bits == 64 {
                Vec::new()
            } else {
                vec![BlockedFeature::Setup]
            },
        },
        ValidationCheck {
            id: CheckId::PackageInstaller,
            passed: true,
            detail: if probe.pip {
                "pip is available; uv installs the packages".into()
            } else {
                "pip is not installed; uv installs the packages".into()
            },
            blocks: Vec::new(),
        },
    ];
    let development_ok = probe.header && (os == OsKind::Linux || probe.import_library);
    checks.push(ValidationCheck {
        id: CheckId::DevelopmentFiles,
        passed: development_ok,
        detail: match (probe.header, probe.import_library, os) {
            (true, _, OsKind::Linux) => "Python.h is present".into(),
            (true, true, OsKind::Windows) => "Python.h and the import library are present".into(),
            (false, _, _) => "Python.h was not found; Triton cannot compile kernels".into(),
            (true, false, OsKind::Windows) => {
                "the Python import library was not found; Triton cannot compile kernels".into()
            }
        },
        blocks: if development_ok {
            Vec::new()
        } else {
            vec![BlockedFeature::Triton]
        },
    });
    if os == OsKind::Windows {
        let loads = msvc_runtime_loads.unwrap_or(false);
        checks.push(ValidationCheck {
            id: CheckId::MsvcRuntime,
            passed: loads,
            detail: if loads {
                "the MSVC runtime DLLs load".into()
            } else {
                "the MSVC runtime DLLs do not load; install the Microsoft Visual C++ Redistributable"
                    .into()
            },
            blocks: if loads {
                Vec::new()
            } else {
                vec![BlockedFeature::Setup]
            },
        });
    }
    checks
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VenvConfig {
    pub major: u8,
    pub minor: u8,
    /// uv records only `major.minor` when a venv is created from a minor-version link.
    pub patch: Option<u8>,
    pub home: PathBuf,
}

impl VenvConfig {
    pub fn version(&self) -> Option<PythonVersion> {
        self.patch
            .map(|patch| PythonVersion::new(self.major, self.minor, patch))
    }
}

/// Reads the interpreter version and base directory from a `pyvenv.cfg`.
pub fn parse_pyvenv_cfg(text: &str) -> Option<VenvConfig> {
    let mut version: Option<(u8, u8, Option<u8>)> = None;
    let mut home = None;
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        match key {
            "version_info" | "version" if version.is_none() => {
                let mut parts = value.split('.').map(|part| part.parse::<u8>().ok());
                if let (Some(Some(major)), Some(Some(minor))) = (parts.next(), parts.next()) {
                    version = Some((major, minor, parts.next().flatten()));
                }
            }
            "home" => home = Some(PathBuf::from(value)),
            _ => {}
        }
    }
    let (major, minor, patch) = version?;
    Some(VenvConfig {
        major,
        minor,
        patch,
        home: home?,
    })
}

/// A canonical, display-friendly path. Missing tails are appended to the nearest existing
/// ancestor; the Windows verbatim prefix is removed.
pub fn normalize_path(path: &Path) -> PathBuf {
    let mut missing = Vec::new();
    let mut existing = path;
    while !existing.exists() {
        match (existing.file_name(), existing.parent()) {
            (Some(name), Some(parent)) => {
                missing.push(name.to_owned());
                existing = parent;
            }
            _ => break,
        }
    }
    let mut base = fs::canonicalize(existing).unwrap_or_else(|_| existing.to_owned());
    if cfg!(windows) {
        let text = base.to_string_lossy().into_owned();
        if let Some(stripped) = text.strip_prefix(r"\\?\")
            && !stripped.starts_with("UNC\\")
        {
            base = PathBuf::from(stripped);
        }
    }
    for name in missing.into_iter().rev() {
        base.push(name);
    }
    base
}

fn failed(message: impl Into<String>) -> ProvisionError {
    ProvisionError::Failed(message.into())
}

#[derive(Deserialize)]
struct PinnedPython {
    name: String,
    arch: PinnedArch,
    os: String,
    libc: String,
    major: u8,
    minor: u8,
    patch: u8,
    variant: Option<String>,
}

#[derive(Deserialize)]
struct PinnedArch {
    family: String,
}

/// The folder name of uv's minor-version link for the pinned Python, derived from the pinned
/// download metadata (`cpython-3.13-windows-x86_64-none`, `cpython-3.13-linux-x86_64-gnu`).
fn minor_link_name(
    version: PythonVersion,
    os: &str,
    arch: &str,
) -> Result<SafeFileName, ProvisionError> {
    let unknown = || failed("The pinned Python download metadata has no entry for this platform.");
    let entries: HashMap<String, PinnedPython> =
        serde_json::from_str(PYTHON_DOWNLOADS_JSON).map_err(|_| unknown())?;
    let mut matching = entries.values().filter(|entry| {
        (entry.major, entry.minor, entry.patch) == (version.major, version.minor, version.patch)
            && entry.os == os
            && entry.arch.family == arch
            && entry.variant.is_none()
    });
    let (Some(entry), None) = (matching.next(), matching.next()) else {
        return Err(unknown());
    };
    SafeFileName::new(format!(
        "{}-{}.{}-{}-{}-{}",
        entry.name, entry.major, entry.minor, entry.os, entry.arch.family, entry.libc
    ))
    .map_err(|_| unknown())
}

fn minor_link_names(os: &str, arch: &str) -> Result<Vec<SafeFileName>, ProvisionError> {
    let unknown = || failed("The pinned Python download metadata has no entry for this platform.");
    let entries: HashMap<String, PinnedPython> =
        serde_json::from_str(PYTHON_DOWNLOADS_JSON).map_err(|_| unknown())?;
    let mut names = entries
        .values()
        .filter(|entry| entry.os == os && entry.arch.family == arch && entry.variant.is_none())
        .map(|entry| {
            SafeFileName::new(format!(
                "{}-{}.{}-{}-{}-{}",
                entry.name, entry.major, entry.minor, entry.os, entry.arch.family, entry.libc
            ))
            .map_err(|_| unknown())
        })
        .collect::<Result<Vec<_>, _>>()?;
    names.sort();
    names.dedup();
    Ok(names)
}

/// A symbolic link or, on Windows, any reparse point (a junction is one).
fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    metadata.file_type().is_symlink()
}

/// Copy tools that follow links turn uv's minor-version links into real folders, and uv then
/// fails to replace them. uv recreates the minor links of all managed installations on every
/// install, so every such copied folder is removed. A real link is left for uv and never followed.
fn clear_copied_minor_link(
    layout: &Layout,
    name: &SafeFileName,
    ctx: &StepContext,
) -> Result<(), ProvisionError> {
    let text = name.as_str();
    let cache = layout.managed_python_dir();
    let metadata = match fs::symlink_metadata(cache.join(text)) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => {
            return Err(failed(format!(
                "The app's Python cache item named {text} could not be inspected. Close programs that use it and retry."
            )));
        }
    };
    if is_link(&metadata) {
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err(failed(format!(
            "The app's Python cache contains an unexpected item named {text}. Remove it and retry."
        )));
    }
    ctx.log(format!(
        "Removing a copied Python version folder ({text}); uv recreates its link."
    ));
    pin_absolute(&cache)
        .and_then(|pinned| pinned.remove_owned_tree(name))
        .map_err(|_| {
            failed(format!(
                "A copied Python version folder ({text}) in the app's Python cache could not be removed. Close programs that use it and retry."
            ))
        })
}

/// The `uv python install` arguments. The interpreter is neither linked into a bin folder nor
/// registered with Windows (PEP 514), so the `py` launcher and IDEs never list it.
fn managed_install_args(version: PythonVersion, metadata: &Path) -> Vec<OsString> {
    vec![
        "--no-config".into(),
        "python".into(),
        "install".into(),
        version.to_string().into(),
        "--no-bin".into(),
        "--no-registry".into(),
        "--python-downloads-json-url".into(),
        metadata.as_os_str().to_owned(),
    ]
}

/// Installs the pinned managed CPython through uv, using the pinned download metadata so uv
/// verifies every archive against its SHA-256. Idempotent. Returns the interpreter path.
pub(crate) async fn ensure_managed_python(
    layout: &Layout,
    version: PythonVersion,
    ctx: &StepContext,
) -> Result<PathBuf, ProvisionError> {
    ensure_cache_dirs(layout).map_err(|_| failed("the cache folders could not be created"))?;
    let metadata = layout.cache_dir().join("python-downloads.json");
    let current = fs::read_to_string(&metadata).ok();
    if current.as_deref() != Some(PYTHON_DOWNLOADS_JSON) {
        atomic_write(&metadata, PYTHON_DOWNLOADS_JSON.as_bytes())
            .map_err(|_| failed("the Python download metadata could not be written"))?;
    }
    let version_text = version.to_string();
    let limits = RunLimits {
        timeout: Some(Duration::from_secs(1800)),
        ..RunLimits::default()
    };
    // Release 1 targets Windows x64 and Linux x64 only.
    let os = if cfg!(windows) { "windows" } else { "linux" };
    minor_link_name(version, os, "x86_64")?;
    for name in minor_link_names(os, "x86_64")? {
        clear_copied_minor_link(layout, &name, ctx)?;
    }
    let install =
        command(layout, Tool::Uv, layout.uv_exe()).args(managed_install_args(version, &metadata));
    run_step(&install, ctx, &limits).await?;
    let find = command(layout, Tool::Uv, layout.uv_exe())
        .arg("--no-config")
        .args(["python", "find", version_text.as_str()])
        .args(["--managed-python", "--no-project"])
        .arg("--python-downloads-json-url")
        .arg(&metadata);
    let output = run_step(&find, ctx, &RunLimits::default()).await?;
    let path = output
        .stdout
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .ok_or_else(|| failed("the managed Python was installed but could not be located"))?;
    Ok(path)
}

#[derive(Debug, Clone, Copy)]
pub struct PythonAdapter;

impl PythonAdapter {
    pub fn new() -> Self {
        Self
    }

    async fn probe(
        &self,
        layout: &Layout,
        python: &Path,
        cancel: &CancelToken,
    ) -> Result<Option<Probe>, ProvisionError> {
        let spec = command(layout, Tool::Python, python).args(["-I", "-c", PROBE_SCRIPT]);
        let limits = RunLimits {
            timeout: Some(Duration::from_secs(60)),
            ..RunLimits::default()
        };
        match run_quiet(&spec, cancel, &limits).await {
            Ok(output) => Ok(parse_probe(&output.stdout)),
            Err(ProvisionError::Cancelled) => Err(ProvisionError::Cancelled),
            Err(_) => Ok(None),
        }
    }

    async fn msvc_loads(
        &self,
        layout: &Layout,
        python: &Path,
        cancel: &CancelToken,
        retained: application::shared::Retained,
    ) -> bool {
        let spec = command(layout, Tool::Python, python)
            .args(["-I", "-c", MSVC_SCRIPT])
            .retain(retained);
        let limits = RunLimits {
            timeout: Some(Duration::from_secs(60)),
            ..RunLimits::default()
        };
        run_quiet(&spec, cancel, &limits).await.is_ok()
    }

    async fn report_for_python(
        &self,
        layout: &Layout,
        python: &Path,
        target: ValidationTarget,
        resolved_path: PathBuf,
    ) -> Result<PythonValidationReport, ProvisionError> {
        let cancel = CancelToken::new();
        let os = if cfg!(windows) {
            OsKind::Windows
        } else {
            OsKind::Linux
        };
        let Some(probe) = self.probe(layout, python, &cancel).await? else {
            return Ok(PythonValidationReport {
                target: ValidationTarget::Rejected,
                resolved_path,
                python_version: None,
                checks: vec![ValidationCheck {
                    id: CheckId::VersionSupported,
                    passed: false,
                    detail: "the selected Python could not be run".into(),
                    blocks: vec![BlockedFeature::Setup],
                }],
            });
        };
        let msvc = if os == OsKind::Windows {
            Some(
                self.msvc_loads(layout, python, &cancel, Default::default())
                    .await,
            )
        } else {
            None
        };
        Ok(PythonValidationReport {
            target,
            resolved_path,
            python_version: Some(PythonVersion::new(
                probe.version[0],
                probe.version[1],
                probe.version[2],
            )),
            checks: checks_from_probe(&probe, os, msvc),
        })
    }
}

impl Default for PythonAdapter {
    fn default() -> Self {
        Self::new()
    }
}

fn rejected(path: PathBuf, detail: &str) -> PythonValidationReport {
    PythonValidationReport {
        target: ValidationTarget::Rejected,
        resolved_path: path,
        python_version: None,
        checks: vec![ValidationCheck {
            id: CheckId::TargetLocation,
            passed: false,
            detail: detail.into(),
            blocks: vec![BlockedFeature::Setup],
        }],
    }
}

fn dir_is_empty(path: &Path) -> bool {
    fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_none())
}

fn read_config(venv: &Path) -> Result<VenvConfig, ProvisionError> {
    let text = fs::read_to_string(venv.join("pyvenv.cfg"))
        .map_err(|_| failed("the folder is not a virtual environment"))?;
    parse_pyvenv_cfg(&text)
        .ok_or_else(|| failed("the virtual environment configuration is not readable"))
}

/// The exact interpreter version, asked of the venv's own Python.
async fn exact_version(layout: &Layout, venv: &Path) -> Result<PythonVersion, ProvisionError> {
    let spec = command(layout, Tool::Python, venv_python(venv)).args([
        "-I",
        "-c",
        "import sys; print('.'.join(str(part) for part in sys.version_info[:3]))",
    ]);
    let limits = RunLimits {
        timeout: Some(Duration::from_secs(60)),
        ..RunLimits::default()
    };
    let output = run_quiet(&spec, &CancelToken::new(), &limits).await?;
    output
        .stdout
        .lines()
        .find_map(|line| line.trim().parse::<PythonVersion>().ok())
        .ok_or_else(|| failed("the environment's Python version could not be read"))
}

async fn venv_info(layout: &Layout, venv: &Path) -> Result<VenvInfo, ProvisionError> {
    let config = read_config(venv)?;
    let version = match config.version() {
        Some(version) => version,
        None => exact_version(layout, venv).await?,
    };
    let interpreter = config.home.join(if cfg!(windows) {
        "python.exe"
    } else {
        "python3"
    });
    Ok(VenvInfo {
        version,
        base_interpreter: interpreter.is_file().then_some(interpreter),
    })
}

#[async_trait]
impl PythonProvisioner for PythonAdapter {
    async fn install_managed_python(
        &self,
        root: &Path,
        version: PythonVersion,
        ctx: &StepContext,
    ) -> Result<PathBuf, ProvisionError> {
        ensure_managed_python(&Layout::new(root), version, ctx).await
    }

    async fn create_venv(
        &self,
        root: &Path,
        interpreter: &Path,
        target: &Path,
        ctx: &StepContext,
    ) -> Result<VenvInfo, ProvisionError> {
        let layout = Layout::new(root);
        if target.join("pyvenv.cfg").is_file() {
            // A venv we already created: keep it so the step is idempotent.
            return venv_info(&layout, target).await;
        }
        if target.exists() && (!target.is_dir() || !dir_is_empty(target)) {
            return Err(failed(
                "The environment folder is not empty and is not a virtual environment. It was left unchanged.",
            ));
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|_| failed("the environment's parent folder could not be created"))?;
        }
        let spec = command(&layout, Tool::Uv, layout.uv_exe())
            .arg("--no-config")
            .arg("venv")
            .arg("--no-project")
            .arg("--python")
            .arg(interpreter)
            .arg(target);
        let limits = RunLimits {
            timeout: Some(Duration::from_secs(300)),
            ..RunLimits::default()
        };
        run_step(&spec, ctx, &limits).await?;
        venv_info(&layout, target).await
    }

    async fn inspect_venv(&self, root: &Path, venv: &Path) -> Result<VenvInfo, ProvisionError> {
        venv_info(&Layout::new(root), venv).await
    }

    async fn validate(
        &self,
        root: &Path,
        source: &PythonSourceInput,
    ) -> Result<PythonValidationReport, ProvisionError> {
        let layout = Layout::new(root);
        match source {
            PythonSourceInput::Managed => Ok(PythonValidationReport {
                target: ValidationTarget::Interpreter,
                resolved_path: layout.envs_dir(),
                python_version: None,
                checks: Vec::new(),
            }),
            PythonSourceInput::CustomInterpreter(path) => {
                let resolved = normalize_path(path);
                if !resolved.is_file() {
                    return Ok(rejected(
                        resolved,
                        "the selected Python executable does not exist",
                    ));
                }
                self.report_for_python(
                    &layout,
                    &resolved,
                    ValidationTarget::Interpreter,
                    resolved.clone(),
                )
                .await
            }
            PythonSourceInput::CustomVenv(path) => {
                let resolved = normalize_path(path);
                if resolved.is_file() {
                    return Ok(rejected(
                        resolved,
                        "the selected location is a file, not a folder",
                    ));
                }
                if !resolved.exists() || dir_is_empty(&resolved) {
                    return Ok(PythonValidationReport {
                        target: ValidationTarget::EmptyFolder,
                        resolved_path: resolved,
                        python_version: None,
                        checks: vec![ValidationCheck {
                            id: CheckId::TargetLocation,
                            passed: true,
                            detail: "an empty folder: a new app-owned virtual environment is created here"
                                .into(),
                            blocks: Vec::new(),
                        }],
                    });
                }
                if !resolved.join("pyvenv.cfg").is_file() {
                    return Ok(rejected(
                        resolved,
                        "the folder is not empty and is not a virtual environment",
                    ));
                }
                let python = venv_python(&resolved);
                if !python.is_file() {
                    return Ok(rejected(
                        resolved,
                        "the virtual environment has no Python executable",
                    ));
                }
                self.report_for_python(&layout, &python, ValidationTarget::ExistingVenv, resolved)
                    .await
            }
        }
    }

    async fn seed(
        &self,
        root: &Path,
        venv: &Path,
        os: OsKind,
        ctx: &StepContext,
    ) -> Result<(), ProvisionError> {
        let layout = Layout::new(root);
        let python = venv_python(venv);
        let mut spec = command(&layout, Tool::Uv, layout.uv_exe())
            .arg("--no-config")
            .args(["pip", "install", "--python"])
            .arg(&python)
            .args(["--upgrade", "pip", "setuptools", "wheel"]);
        if os == OsKind::Windows {
            spec = spec.arg(MSVC_RUNTIME);
        }
        let limits = RunLimits {
            timeout: Some(Duration::from_secs(900)),
            ..RunLimits::default()
        };
        run_step(&spec, ctx, &limits).await?;
        if os != OsKind::Windows {
            return Ok(());
        }
        // The MSVC runtime package drops its DLLs into the venv's Scripts folder. A managed
        // Python lives inside the install root, so they are copied beside its python.exe, where
        // extension modules find them. Interpreters outside the install root are never touched.
        if let Ok(config) = read_config(venv)
            && let home = config.home.as_path()
            && home.starts_with(layout.cache_dir())
        {
            copy_runtime_dlls(&venv_scripts(venv), home)
                .map_err(|_| failed("the MSVC runtime DLLs could not be copied"))?;
        }
        if self
            .msvc_loads(&layout, &python, &ctx.cancel, ctx.retained.clone())
            .await
        {
            ctx.log("The MSVC runtime DLLs load.");
            Ok(())
        } else {
            Err(failed(
                "The MSVC runtime DLLs (msvcp140.dll, vcruntime140.dll, vcruntime140_1.dll) do not load. \
                 Install the Microsoft Visual C++ Redistributable or choose the managed Python.",
            ))
        }
    }

    async fn remove_environment(&self, env: &PythonEnvironment) -> Result<(), ProvisionError> {
        let path = env.path.clone();
        let authorized = env.authorized_location.clone();
        let removable =
            path == authorized && env.ownership == domain::environment::Ownership::AppOwned;
        if !removable {
            return Err(failed(
                "This environment is not app-owned, so it is never deleted.",
            ));
        }
        tokio::task::spawn_blocking(move || remove_venv_dir(&path, &authorized))
            .await
            .map_err(|_| failed("the environment could not be deleted"))?
    }
}

fn copy_runtime_dlls(scripts: &Path, home: &Path) -> std::io::Result<()> {
    for entry in fs::read_dir(scripts)?.flatten() {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        let is_runtime = name.ends_with(".dll")
            && (name.starts_with("msvcp140")
                || name.starts_with("concrt140")
                || name.starts_with("vcruntime140"));
        if is_runtime && entry.path().is_file() {
            let destination = home.join(entry.file_name());
            if !destination.exists() {
                fs::copy(entry.path(), destination)?;
            }
        }
    }
    Ok(())
}

/// Deletes a venv directory after checking it is the authorized location, looks like a venv, is
/// not a link and is deep enough not to be a drive or home root.
pub(crate) fn remove_venv_dir(path: &Path, authorized: &Path) -> Result<(), ProvisionError> {
    let depth = path
        .components()
        .filter(|component| matches!(component, Component::Normal(_)))
        .count();
    if path != authorized || !path.is_absolute() || depth < 3 {
        return Err(failed("The environment location is not safe to delete."));
    }
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(failed("The environment folder could not be inspected.")),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(failed(
            "The environment folder is a link or not a folder, so it was left unchanged.",
        ));
    }
    if !path.join("pyvenv.cfg").is_file() && !dir_is_empty(path) {
        return Err(failed(
            "The folder does not look like a virtual environment, so it was left unchanged.",
        ));
    }
    fs::remove_dir_all(path).map_err(|_| {
        failed("The environment could not be deleted. Close programs that use it and retry.")
    })
}

#[cfg(test)]
mod tests;
