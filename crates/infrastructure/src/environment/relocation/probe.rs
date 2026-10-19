use std::{
    fs,
    future::Future,
    io,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use application::{
    environment::{
        InstallManifest, InstallationHealth, InstallationProbe, ProvisionError, RebuildReason,
        same_location,
    },
    shared::CancelToken,
};
use async_trait::async_trait;
use domain::environment::{PythonEnvironment, PythonSourceKind, PythonVersion};
use serde::Deserialize;
use tokio::sync::Mutex;

use crate::{
    environment::{
        exec::run_quiet,
        layout::{Layout, Tool, command, venv_python},
        python::parse_pyvenv_cfg,
    },
    shared::process::RunLimits,
};

const PROBE_MARKER: &str = "VJ-INSTALLATION:";
const PROBE_SCRIPT: &str = r#"
import json, struct, sys
print("VJ-INSTALLATION:" + json.dumps({
    "implementation": sys.implementation.name,
    "version": list(sys.version_info[:3]),
    "bits": struct.calcsize("P") * 8,
    "executable": sys.executable,
    "prefix": sys.prefix,
    "base_prefix": sys.base_prefix,
}))
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProbeKey {
    root: PathBuf,
    manifest: InstallManifest,
    active: PythonEnvironment,
}

struct CachedProbe {
    generation: u64,
    key: ProbeKey,
    result: Result<InstallationHealth, ProvisionError>,
}

/// The mutex covers the process as well as the result. Polling shares one probe,
/// including when an invalidation occurs while an earlier probe is running.
#[derive(Default)]
pub struct FsInstallationProbe {
    generation: AtomicU64,
    cache: Mutex<Option<CachedProbe>>,
}

impl FsInstallationProbe {
    pub fn new() -> Self {
        Self::default()
    }

    async fn inspect_cached(
        &self,
        key: ProbeKey,
        probe: impl Future<Output = Result<InstallationHealth, ProvisionError>>,
    ) -> Result<InstallationHealth, ProvisionError> {
        let mut cache = self.cache.lock().await;
        let generation = self.generation.load(Ordering::Acquire);
        if let Some(cached) = cache.as_ref()
            && cached.generation == generation
            && cached.key == key
        {
            return cached.result.clone();
        }
        let result = probe.await;
        if self.generation.load(Ordering::Acquire) == generation {
            *cache = Some(CachedProbe {
                generation,
                key,
                result: result.clone(),
            });
        }
        result
    }
}

#[async_trait]
impl InstallationProbe for FsInstallationProbe {
    async fn inspect(
        &self,
        current_root: &Path,
        manifest: &InstallManifest,
        active: &PythonEnvironment,
    ) -> Result<InstallationHealth, ProvisionError> {
        let key = ProbeKey {
            root: current_root.to_owned(),
            manifest: manifest.clone(),
            active: active.clone(),
        };
        self.inspect_cached(key, inspect(current_root, manifest, active))
            .await
    }

    fn invalidate(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
    }

    async fn validate_rebuild_checkout(&self, root: &Path) -> Result<(), ProvisionError> {
        use crate::shared::model_paths::pin_absolute;
        use domain::shared::model_values::SafeFileName;
        let validate = || -> io::Result<()> {
            let name = |value: &str| SafeFileName::new(value).map_err(io::Error::other);
            let root = pin_absolute(root)?;
            let checkout = root.child(&name("ComfyUI")?, false)?;
            let _git = checkout.child(&name(".git")?, false)?;
            let _main = checkout.open_read_pin(&name("main.py")?)?;
            let _requirements = checkout.open_read_pin(&name("requirements.txt")?)?;
            Ok(())
        };
        validate().map_err(|_| {
            ProvisionError::Failed("The existing ComfyUI checkout is unsafe or incomplete.".into())
        })
    }
}

fn normalize_components(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push(component);
                }
            }
            other => normalized.push(other),
        }
    }
    normalized
}

fn canonical_or_missing(path: &Path) -> io::Result<PathBuf> {
    match fs::canonicalize(path) {
        Ok(path) => Ok(normalize_components(&path)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(normalize_components(path)),
        Err(error) => Err(error),
    }
}

/// The recorded root as an absolute component path. An unreadable one (vanished, offline share,
/// access denied) is compared by its normalized spelling instead of failing.
fn recorded_root(recorded: &Path) -> PathBuf {
    canonical_or_missing(recorded).unwrap_or_else(|_| normalize_components(recorded))
}

/// Whether `current` is not the root the manifest records. A roots-only comparison: it spawns no
/// interpreter and never touches the environment.
pub fn root_moved(current: &Path, recorded: &Path) -> bool {
    let current = fs::canonicalize(current).unwrap_or_else(|_| current.to_owned());
    !same_location(&normalize_components(&current), &recorded_root(recorded))
}

fn same_existing_path(left: &Path, right: &Path) -> bool {
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => same_location(&left, &right),
        _ => false,
    }
}

fn contained_existing_path(path: &Path, parent: &Path) -> bool {
    let (Ok(path), Ok(parent)) = (fs::canonicalize(path), fs::canonicalize(parent)) else {
        return false;
    };
    path.ancestors()
        .any(|ancestor| same_location(ancestor, &parent))
}

#[derive(Deserialize)]
struct InterpreterIdentity {
    implementation: String,
    version: [u8; 3],
    bits: u32,
    executable: PathBuf,
    prefix: PathBuf,
    base_prefix: PathBuf,
}

fn identity_matches(
    identity: &InterpreterIdentity,
    active: &PythonEnvironment,
    layout: &Layout,
) -> bool {
    identity.implementation == "cpython"
        && identity.bits == 64
        && PythonVersion::new(
            identity.version[0],
            identity.version[1],
            identity.version[2],
        ) == active.python_version
        && same_existing_path(&identity.executable, &venv_python(&active.path))
        && same_existing_path(&identity.prefix, &active.path)
        && identity.base_prefix.is_dir()
        && !same_existing_path(&identity.prefix, &identity.base_prefix)
        && (active.source != PythonSourceKind::Managed
            || contained_existing_path(&identity.base_prefix, &layout.managed_python_dir()))
}

async fn inspect(
    current_root: &Path,
    manifest: &InstallManifest,
    active: &PythonEnvironment,
) -> Result<InstallationHealth, ProvisionError> {
    let current = fs::canonicalize(current_root)
        .map_err(|_| ProvisionError::Failed("The installation root cannot be read.".into()))?;
    if !same_location(
        &normalize_components(&current),
        &recorded_root(&manifest.install_root),
    ) {
        return Ok(InstallationHealth::RebuildRequired(RebuildReason::Moved));
    }
    let broken = InstallationHealth::RebuildRequired(RebuildReason::InterpreterBroken);
    if manifest.env_id != active.id
        || manifest.python_version != active.python_version
        || !same_existing_path(&manifest.env_path, &active.path)
    {
        return Ok(broken);
    }
    let layout = Layout::new(current_root);
    let Some(config) = fs::read_to_string(active.path.join("pyvenv.cfg"))
        .ok()
        .and_then(|text| parse_pyvenv_cfg(&text))
    else {
        return Ok(broken);
    };
    if config.major != active.python_version.major
        || config.minor != active.python_version.minor
        || config
            .patch
            .is_some_and(|patch| patch != active.python_version.patch)
        || (active.source == PythonSourceKind::Managed
            && !contained_existing_path(&config.home, &layout.managed_python_dir()))
    {
        return Ok(broken);
    }
    let spec =
        command(&layout, Tool::Python, venv_python(&active.path)).args(["-I", "-c", PROBE_SCRIPT]);
    let limits = RunLimits {
        timeout: Some(Duration::from_secs(10)),
        max_stdout_bytes: 16 * 1024,
        ..RunLimits::default()
    };
    let Ok(output) = run_quiet(&spec, &CancelToken::new(), &limits).await else {
        return Ok(broken);
    };
    let identity: Option<InterpreterIdentity> = output
        .stdout
        .lines()
        .find_map(|line| line.trim().strip_prefix(PROBE_MARKER))
        .and_then(|text| serde_json::from_str(text).ok());
    if identity.is_some_and(|identity| identity_matches(&identity, active, &layout)) {
        Ok(InstallationHealth::Healthy)
    } else {
        Ok(broken)
    }
}

#[cfg(test)]
mod tests {
    use application::environment::{InstalledComponents, RunId};
    use domain::environment::{EnvId, NewEnvironment, Ownership};

    use super::*;

    #[tokio::test]
    async fn rebuild_checkout_refuses_git_indirection_and_missing_entrypoint() {
        let directory = tempfile::tempdir().unwrap();
        let checkout = directory.path().join("ComfyUI");
        fs::create_dir(&checkout).unwrap();
        fs::write(checkout.join("requirements.txt"), "").unwrap();
        fs::write(checkout.join(".git"), "gitdir: external-checkout").unwrap();
        let probe = FsInstallationProbe::new();
        assert!(
            probe
                .validate_rebuild_checkout(directory.path())
                .await
                .is_err()
        );
        fs::remove_file(checkout.join(".git")).unwrap();
        fs::create_dir(checkout.join(".git")).unwrap();
        assert!(
            probe
                .validate_rebuild_checkout(directory.path())
                .await
                .is_err()
        );
        fs::write(checkout.join("main.py"), "# local checkout\n").unwrap();
        probe
            .validate_rebuild_checkout(directory.path())
            .await
            .unwrap();
    }

    fn installation(root: &Path, path: &Path) -> (InstallManifest, PythonEnvironment) {
        let active = PythonEnvironment::provisioning(NewEnvironment {
            id: EnvId::new("adopted").unwrap(),
            path: path.to_owned(),
            ownership: Ownership::Adopted,
            source: PythonSourceKind::CustomVenv,
            base_interpreter: None,
            python_version: PythonVersion::new(3, 13, 16),
            profile_id: "profile".into(),
            profile_revision: 1,
            authorized_location: path.to_owned(),
            now: "now".into(),
        });
        let manifest = InstallManifest {
            schema_version: 2,
            install_root: root.to_owned(),
            generation: 1,
            env_id: active.id.clone(),
            env_path: path.to_owned(),
            ownership: active.ownership,
            python_version: active.python_version,
            profile_id: active.profile_id.clone(),
            profile_revision: active.profile_revision,
            comfyui_reference: None,
            comfyui_commit: None,
            run_id: RunId::new("recorded").unwrap(),
            activated_at: "now".into(),
            components: InstalledComponents::default(),
        };
        (manifest, active)
    }

    #[tokio::test]
    async fn broken_adopted_interpreter_offers_owned_replacement_without_mutation() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("root");
        let adopted = directory.path().join("adopted");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(adopted.join("Scripts")).unwrap();
        fs::write(adopted.join("keep.txt"), b"adopted user data").unwrap();
        let config = b"home = /unavailable\nversion = 3.13.16\n";
        fs::write(adopted.join("pyvenv.cfg"), config).unwrap();
        let python = venv_python(&adopted);
        fs::create_dir_all(python.parent().unwrap()).unwrap();
        fs::write(&python, b"broken executable").unwrap();
        let (manifest, active) = installation(&root, &adopted);

        let health = FsInstallationProbe::new()
            .inspect(&root, &manifest, &active)
            .await
            .unwrap();

        assert_eq!(
            health,
            InstallationHealth::RebuildRequired(RebuildReason::InterpreterBroken)
        );
        assert_eq!(fs::read(&python).unwrap(), b"broken executable");
        assert_eq!(fs::read(adopted.join("pyvenv.cfg")).unwrap(), config);
        assert_eq!(
            fs::read(adopted.join("keep.txt")).unwrap(),
            b"adopted user data"
        );
        assert!(!root.join("envs").exists());
        assert!(!root.join("data").exists());
    }

    #[tokio::test]
    async fn relocation_is_detected_with_the_original_root_present_or_vanished() {
        let directory = tempfile::tempdir().unwrap();
        let old = directory.path().join("old");
        let new = directory.path().join("new");
        fs::create_dir_all(&new).unwrap();
        let (manifest, active) = installation(&old, &old.join("envs/adopted"));
        let probe = FsInstallationProbe::new();
        for old_exists in [false, true] {
            if old_exists {
                fs::create_dir_all(&old).unwrap();
                probe.invalidate();
            }
            assert_eq!(
                probe.inspect(&new, &manifest, &active).await.unwrap(),
                InstallationHealth::RebuildRequired(RebuildReason::Moved)
            );
        }
        assert_eq!(fs::read_dir(&new).unwrap().count(), 0);
    }

    #[test]
    fn managed_python_home_requires_component_containment_in_the_layout_directory() {
        let directory = tempfile::tempdir().unwrap();
        let layout = Layout::new(directory.path());
        let managed = layout.managed_python_dir();
        let legitimate = managed.join("cpython-3.13/bin");
        let sibling = layout.cache_dir().join("uv-python-external/bin");
        fs::create_dir_all(&legitimate).unwrap();
        fs::create_dir_all(&sibling).unwrap();
        assert!(contained_existing_path(&legitimate, &managed));
        assert!(!contained_existing_path(&sibling, &managed));
    }

    #[test]
    fn interpreter_identity_requires_the_recorded_version_and_venv_prefix() {
        let directory = tempfile::tempdir().unwrap();
        let layout = Layout::new(directory.path());
        let path = directory.path().join("venv");
        let base = directory.path().join("custom-python");
        let python = venv_python(&path);
        fs::create_dir_all(python.parent().unwrap()).unwrap();
        fs::create_dir_all(&base).unwrap();
        fs::write(&python, b"interpreter").unwrap();
        let (_, active) = installation(directory.path(), &path);
        let mut identity = InterpreterIdentity {
            implementation: "cpython".into(),
            version: [3, 13, 16],
            bits: 64,
            executable: python,
            prefix: path,
            base_prefix: base,
        };
        assert!(identity_matches(&identity, &active, &layout));
        identity.version[2] = 15;
        assert!(!identity_matches(&identity, &active, &layout));
        identity.version[2] = 16;
        identity.prefix = identity.base_prefix.clone();
        assert!(!identity_matches(&identity, &active, &layout));
        identity.prefix = active.path.clone();
        identity.implementation = "pypy".into();
        assert!(!identity_matches(&identity, &active, &layout));
    }

    #[test]
    fn component_normalization_preserves_root_and_resolves_relative_segments() {
        assert_eq!(
            normalize_components(Path::new("/install/./envs/../data")),
            PathBuf::from("/install/data")
        );
    }

    #[tokio::test]
    async fn status_probe_shares_in_flight_result_and_invalidation_rechecks() {
        use std::sync::atomic::AtomicUsize;

        let directory = tempfile::tempdir().unwrap();
        let (manifest, active) = installation(directory.path(), &directory.path().join("env"));
        let key = ProbeKey {
            root: directory.path().to_owned(),
            manifest,
            active,
        };
        let probe = FsInstallationProbe::new();
        let starts = AtomicUsize::new(0);
        let running = AtomicUsize::new(0);
        let maximum = AtomicUsize::new(0);
        let work = || async {
            starts.fetch_add(1, Ordering::SeqCst);
            let count = running.fetch_add(1, Ordering::SeqCst) + 1;
            maximum.fetch_max(count, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(10)).await;
            running.fetch_sub(1, Ordering::SeqCst);
            Ok(InstallationHealth::Healthy)
        };
        let (first, second) = tokio::join!(
            probe.inspect_cached(key.clone(), work()),
            probe.inspect_cached(key.clone(), work()),
        );
        assert_eq!(first.unwrap(), InstallationHealth::Healthy);
        assert_eq!(second.unwrap(), InstallationHealth::Healthy);
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        assert_eq!(maximum.load(Ordering::SeqCst), 1);

        probe.invalidate();
        probe.inspect_cached(key, work()).await.unwrap();
        assert_eq!(starts.load(Ordering::SeqCst), 2);
        assert_eq!(maximum.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn invalidation_during_a_probe_never_caches_its_old_result_or_overlaps() {
        use std::sync::atomic::AtomicUsize;

        let directory = tempfile::tempdir().unwrap();
        let (manifest, active) = installation(directory.path(), &directory.path().join("env"));
        let key = ProbeKey {
            root: directory.path().to_owned(),
            manifest,
            active,
        };
        let probe = FsInstallationProbe::new();
        let starts = AtomicUsize::new(0);
        let running = AtomicUsize::new(0);
        let work = || async {
            starts.fetch_add(1, Ordering::SeqCst);
            assert_eq!(running.fetch_add(1, Ordering::SeqCst), 0);
            tokio::task::yield_now().await;
            running.fetch_sub(1, Ordering::SeqCst);
            Ok(InstallationHealth::Healthy)
        };
        let first = probe.inspect_cached(key.clone(), work());
        let invalidate = async {
            while running.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
            probe.invalidate();
            probe.inspect_cached(key.clone(), work()).await.unwrap();
        };
        let (first, ()) = tokio::join!(first, invalidate);
        first.unwrap();
        assert_eq!(starts.load(Ordering::SeqCst), 2);
        probe.inspect_cached(key, work()).await.unwrap();
        assert_eq!(starts.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn every_environment_source_and_ownership_is_probed() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let path = root.join("venv");
        fs::create_dir_all(&path).unwrap();
        fs::write(
            path.join("pyvenv.cfg"),
            b"home = /missing\nversion = 3.13.16\n",
        )
        .unwrap();
        let (mut manifest, mut active) = installation(root, &path);
        let probe = FsInstallationProbe::new();
        for source in [
            PythonSourceKind::Managed,
            PythonSourceKind::CustomVenv,
            PythonSourceKind::CustomInterpreter,
        ] {
            for ownership in [Ownership::Adopted, Ownership::AppOwned] {
                active.source = source;
                active.ownership = ownership;
                manifest.ownership = ownership;
                assert_eq!(
                    probe.inspect(root, &manifest, &active).await.unwrap(),
                    InstallationHealth::RebuildRequired(RebuildReason::InterpreterBroken),
                    "{source:?} {ownership:?}"
                );
            }
        }
        assert!(!root.join("data").exists());
    }
}
