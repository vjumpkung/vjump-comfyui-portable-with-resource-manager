use std::{
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
};

use application::environment::RootSource;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub root: PathBuf,
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    pub logs_dir: PathBuf,
}

impl AppPaths {
    /// The platform default install root (spec section 3.2). The debug override and the
    /// bootstrap pointer are applied by [`resolve_root`].
    pub fn default_root() -> io::Result<PathBuf> {
        dirs::data_local_dir()
            .map(|directory| directory.join("vjumpkung-comfyui"))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "local data directory is unavailable",
                )
            })
    }

    pub fn from_root(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref().to_owned();
        let data_dir = root.join("data");
        let db_path = data_dir.join("app.db");
        let logs_dir = data_dir.join("logs");
        Self {
            root,
            data_dir,
            db_path,
            logs_dir,
        }
    }

    pub fn ensure_dirs(&self) -> io::Result<()> {
        fs::create_dir_all(&self.data_dir)?;
        fs::create_dir_all(&self.logs_dir)
    }
}

/// The only bootstrap pointer schema this build reads.
pub const BOOTSTRAP_SCHEMA_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct BootstrapDoc {
    schema_version: u32,
    install_root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RootError {
    #[error("the bootstrap pointer is unreadable or invalid")]
    BootstrapCorrupt,
    #[error("the bootstrap pointer was written by a newer version of the app")]
    BootstrapNewer,
    #[error("the bootstrap pointer could not be read")]
    BootstrapInaccessible,
    #[error("the local data directory is unavailable")]
    NoLocalDataDirectory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRoot {
    pub root: PathBuf,
    pub source: RootSource,
}

/// `<OS config dir>/vjumpkung-comfyui/bootstrap.json`.
pub fn bootstrap_path() -> Result<PathBuf, RootError> {
    dirs::config_dir()
        .map(|directory| directory.join("vjumpkung-comfyui").join("bootstrap.json"))
        .ok_or(RootError::NoLocalDataDirectory)
}

/// Reads the bootstrap pointer. A missing file is `Ok(None)`; anything unreadable, invalid or
/// newer is an error so the caller never silently falls back to another root.
pub fn read_bootstrap(path: &Path) -> Result<Option<PathBuf>, RootError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) if error.kind() == io::ErrorKind::InvalidData => {
            return Err(RootError::BootstrapCorrupt);
        }
        Err(_) => return Err(RootError::BootstrapInaccessible),
    };
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|_| RootError::BootstrapCorrupt)?;
    match value
        .get("schema_version")
        .and_then(|version| version.as_u64())
    {
        Some(version) if version > u64::from(BOOTSTRAP_SCHEMA_VERSION) => {
            return Err(RootError::BootstrapNewer);
        }
        Some(version) if version == u64::from(BOOTSTRAP_SCHEMA_VERSION) => {}
        _ => return Err(RootError::BootstrapCorrupt),
    }
    let doc: BootstrapDoc =
        serde_json::from_value(value).map_err(|_| RootError::BootstrapCorrupt)?;
    if !doc.install_root.is_absolute() {
        return Err(RootError::BootstrapCorrupt);
    }
    Ok(Some(doc.install_root))
}

/// Atomically publishes the bootstrap pointer: write and flush a temporary file, then replace.
pub fn write_bootstrap(path: &Path, root: &Path) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no parent directory"))?;
    fs::create_dir_all(parent)?;
    let doc = BootstrapDoc {
        schema_version: BOOTSTRAP_SCHEMA_VERSION,
        install_root: root.to_owned(),
    };
    let text = serde_json::to_string_pretty(&doc)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    crate::shared::fs::atomic_write(path, text.as_bytes())
}

/// Resolves the install root in order: debug-only override, bootstrap pointer, platform default.
pub fn resolve_root_with(
    debug_override: Option<OsString>,
    bootstrap_file: &Path,
    default_root: PathBuf,
) -> Result<ResolvedRoot, RootError> {
    if let Some(root) = debug_override.filter(|root| !root.is_empty()) {
        return Ok(ResolvedRoot {
            root: PathBuf::from(root),
            source: RootSource::DebugOverride,
        });
    }
    match read_bootstrap(bootstrap_file)? {
        Some(root) => Ok(ResolvedRoot {
            root,
            source: RootSource::Bootstrap,
        }),
        None => Ok(ResolvedRoot {
            root: default_root,
            source: RootSource::Default,
        }),
    }
}

/// The production resolution. `VJUMPKUNG_COMFYUI_HOME` is honored only in debug builds.
pub fn resolve_root() -> Result<ResolvedRoot, RootError> {
    #[cfg(debug_assertions)]
    let debug_override = std::env::var_os("VJUMPKUNG_COMFYUI_HOME");
    #[cfg(not(debug_assertions))]
    let debug_override = None;
    let default_root = AppPaths::default_root().map_err(|_| RootError::NoLocalDataDirectory)?;
    resolve_root_with(debug_override, &bootstrap_path()?, default_root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_and_directory_creation_support_spaces_and_unicode() {
        let directory = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(directory.path().join("test ไทย root"));
        assert_eq!(paths.db_path, paths.root.join("data/app.db"));
        assert_eq!(paths.logs_dir, paths.root.join("data/logs"));
        paths.ensure_dirs().unwrap();
        paths.ensure_dirs().unwrap();
        assert!(paths.data_dir.is_dir());
        assert!(paths.logs_dir.is_dir());
        assert!(!paths.db_path.exists());
    }

    fn resolve(
        override_root: Option<&str>,
        file: &Path,
        default: &str,
    ) -> Result<ResolvedRoot, RootError> {
        resolve_root_with(
            override_root.map(OsString::from),
            file,
            PathBuf::from(default),
        )
    }

    #[test]
    fn root_resolution_order_is_override_then_bootstrap_then_default() {
        let directory = tempfile::tempdir().unwrap();
        let pointer = directory.path().join("config ไทย").join("bootstrap.json");
        let default = directory.path().join("default root");

        let resolved = resolve(None, &pointer, default.to_str().unwrap()).unwrap();
        assert_eq!(resolved.source, RootSource::Default);
        assert_eq!(resolved.root, default);

        let chosen = directory.path().join("chosen ราก root");
        write_bootstrap(&pointer, &chosen).unwrap();
        let resolved = resolve(None, &pointer, default.to_str().unwrap()).unwrap();
        assert_eq!(resolved.source, RootSource::Bootstrap);
        assert_eq!(resolved.root, chosen);

        let resolved =
            resolve(Some("/override/root"), &pointer, default.to_str().unwrap()).unwrap();
        assert_eq!(resolved.source, RootSource::DebugOverride);
        assert_eq!(resolved.root, PathBuf::from("/override/root"));
        // An empty override is ignored.
        assert_eq!(
            resolve(Some(""), &pointer, default.to_str().unwrap())
                .unwrap()
                .source,
            RootSource::Bootstrap
        );
    }

    #[test]
    fn an_unusable_pointer_is_an_error_never_a_silent_fallback() {
        let directory = tempfile::tempdir().unwrap();
        let pointer = directory.path().join("bootstrap.json");
        let default = directory.path().join("default");
        let default = default.to_str().unwrap();
        let abs = directory.path().join("root");
        for (content, expected) in [
            ("not json", RootError::BootstrapCorrupt),
            ("{}", RootError::BootstrapCorrupt),
            ("[]", RootError::BootstrapCorrupt),
            (
                "{\"schema_version\":1,\"install_root\":\"relative/path\"}",
                RootError::BootstrapCorrupt,
            ),
            ("{\"schema_version\":1}", RootError::BootstrapCorrupt),
            (
                "{\"schema_version\":0,\"install_root\":\"/x\"}",
                RootError::BootstrapCorrupt,
            ),
            (
                &format!(
                    "{{\"schema_version\":2,\"install_root\":{:?},\"extra\":true}}",
                    abs
                ),
                RootError::BootstrapNewer,
            ),
        ] {
            fs::write(&pointer, content).unwrap();
            assert_eq!(resolve(None, &pointer, default), Err(expected), "{content}");
        }
        // The debug override stays fixed even when the pointer is broken.
        assert_eq!(
            resolve(Some("/override"), &pointer, default)
                .unwrap()
                .source,
            RootSource::DebugOverride
        );
        // A directory where the file should be is inaccessible, not absent.
        fs::remove_file(&pointer).unwrap();
        fs::create_dir(&pointer).unwrap();
        assert_eq!(
            resolve(None, &pointer, default),
            Err(RootError::BootstrapInaccessible)
        );
    }

    #[test]
    fn publishing_the_pointer_replaces_it_atomically() {
        let directory = tempfile::tempdir().unwrap();
        let pointer = directory.path().join("bootstrap.json");
        write_bootstrap(&pointer, &directory.path().join("first")).unwrap();
        write_bootstrap(&pointer, &directory.path().join("second")).unwrap();
        assert_eq!(
            read_bootstrap(&pointer).unwrap(),
            Some(directory.path().join("second"))
        );
        let names: Vec<_> = fs::read_dir(directory.path())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name())
            .collect();
        assert_eq!(
            names.len(),
            1,
            "no temporary file is left behind: {names:?}"
        );
    }
}
