//! File operations of the library over pinned, non-following parent directories. Containment is
//! re-established for every operation; no unchecked absolute path is ever mutated.
use super::roots::LibraryRoots;
use crate::{
    models::acquisition::stack::DEFAULT_MODEL_ROOT_ID,
    shared::{
        model_paths::{self, PinnedDirectory},
        trash,
    },
};
use application::{models::library::*, shared::ExplorerTarget};
use async_trait::async_trait;
use domain::{
    models::{acquisition::FileIdentity, library::ModelLocation},
    shared::model_values::{SafeFileName, Sha256},
};
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

static SCRATCH: AtomicU64 = AtomicU64::new(0);

pub struct LocalModelFiles {
    pub roots: Arc<LibraryRoots>,
}

/// Maps an operating-system error to a library code. Sharing violations are "locked"; access
/// denied stays distinct; a bare `PermissionDenied` kind is a containment violation raised by
/// the pinned traversal itself.
pub fn classify(error: &io::Error) -> LibraryErrorCode {
    match error.raw_os_error() {
        #[cfg(windows)]
        Some(32 | 33) => LibraryErrorCode::Locked,
        #[cfg(windows)]
        Some(5) => LibraryErrorCode::AccessDenied,
        #[cfg(unix)]
        Some(libc::EBUSY | libc::ETXTBSY) => LibraryErrorCode::Locked,
        #[cfg(unix)]
        Some(libc::EACCES | libc::EPERM) => LibraryErrorCode::AccessDenied,
        Some(_) if error.kind() == io::ErrorKind::PermissionDenied => {
            LibraryErrorCode::AccessDenied
        }
        _ => match error.kind() {
            io::ErrorKind::PermissionDenied | io::ErrorKind::InvalidInput => {
                LibraryErrorCode::OutsideRoot
            }
            io::ErrorKind::NotFound => LibraryErrorCode::NotFound,
            io::ErrorKind::AlreadyExists => LibraryErrorCode::TargetExists,
            _ => LibraryErrorCode::Io,
        },
    }
}
fn err(error: io::Error) -> LibraryError {
    LibraryError(classify(&error))
}
pub(crate) fn is_cross_volume(error: &io::Error) -> bool {
    error.raw_os_error() == Some(if cfg!(windows) { 17 } else { 18 })
}
fn same(live: &FileIdentity, expected: &FileIdentity) -> bool {
    live.size == expected.size
        && live.modified_ns == expected.modified_ns
        && live.platform_id == expected.platform_id
}
fn scratch_name(prefix: &str, scratch: &str, suffix: &str) -> io::Result<SafeFileName> {
    let n = SCRATCH.fetch_add(1, Ordering::SeqCst);
    SafeFileName::new(format!(".vjump-{prefix}-{scratch}-{n}{suffix}"))
        .map_err(|_| io::ErrorKind::InvalidInput.into())
}

impl LocalModelFiles {
    fn parent(
        &self,
        location: &ModelLocation,
        create: bool,
    ) -> io::Result<(Arc<PinnedDirectory>, SafeFileName)> {
        self.roots
            .authorized()
            .parent(&location.root_id, &location.path, create)
    }

    async fn blocking<T: Send + 'static>(
        &self,
        work: impl FnOnce(&LocalModelFiles) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let this = LocalModelFiles {
            roots: self.roots.clone(),
        };
        tokio::task::spawn_blocking(move || work(&this))
            .await
            .map_err(|_| LibraryError(LibraryErrorCode::Io))?
    }
}

#[async_trait]
impl ModelFiles for LocalModelFiles {
    async fn inspect(&self, location: &ModelLocation) -> Result<Option<FileIdentity>> {
        let location = location.clone();
        self.blocking(move |this| {
            let (parent, name) = match this.parent(&location, false) {
                Ok(found) => found,
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(err(e)),
            };
            match parent.open(&name, false, false) {
                Ok(file) => model_paths::identity(&file).map(Some).map_err(err),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(err(e)),
            }
        })
        .await
    }

    async fn list_directory(&self, directory: &ModelLocation) -> Result<Vec<SafeFileName>> {
        let directory = directory.clone();
        self.blocking(move |this| {
            let pinned = this.roots.pinned_directory(&directory).map_err(err)?;
            let mut names = Vec::new();
            for entry in std::fs::read_dir(&pinned.path).map_err(err)? {
                let entry = entry.map_err(err)?;
                if entry.file_type().map_err(err)?.is_file()
                    && let Ok(name) = SafeFileName::new(entry.file_name().to_string_lossy())
                {
                    names.push(name);
                }
            }
            Ok(names)
        })
        .await
    }

    async fn check_unlocked(&self, location: &ModelLocation) -> Result<()> {
        let location = location.clone();
        self.blocking(move |this| {
            let (parent, name) = this.parent(&location, false).map_err(err)?;
            match parent.open(&name, false, false) {
                Ok(_) => Ok(()),
                Err(e) => Err(err(e)),
            }
        })
        .await
    }

    async fn relocate(
        &self,
        from: &ModelLocation,
        to: &ModelLocation,
        expected: Option<&FileIdentity>,
        scratch: &str,
    ) -> Result<Relocated> {
        let (from, to, expected, scratch) = (
            from.clone(),
            to.clone(),
            expected.cloned(),
            scratch.to_owned(),
        );
        self.blocking(move |this| {
            let (source_parent, source_name) = this.parent(&from, false).map_err(err)?;
            let (target_parent, target_name) = this.parent(&to, true).map_err(err)?;
            let mut file = source_parent
                .open(&source_name, false, false)
                .map_err(err)?;
            if let Some(expected) = &expected
                && !same(&model_paths::identity(&file).map_err(err)?, expected)
            {
                return Err(LibraryErrorCode::Stale.into());
            }
            match model_paths::rename_no_replace(
                &source_parent,
                &source_name,
                &file,
                &target_parent,
                &target_name,
            ) {
                Ok(()) => Ok(Relocated::Renamed),
                Err(e) if is_cross_volume(&e) => copy_across_volumes(
                    &source_parent,
                    &source_name,
                    &mut file,
                    &target_parent,
                    &target_name,
                    &scratch,
                ),
                Err(e) => Err(err(e)),
            }
        })
        .await
    }

    async fn trash(&self, location: &ModelLocation, expected: &FileIdentity) -> Result<()> {
        let (location, expected) = (location.clone(), expected.clone());
        self.blocking(move |this| {
            let (parent, name) = this.parent(&location, false).map_err(err)?;
            let file = parent.open(&name, false, false).map_err(err)?;
            if !same(&model_paths::identity(&file).map_err(err)?, &expected) {
                return Err(LibraryErrorCode::Stale.into());
            }
            drop(file);
            trash::recycle(&parent.path.join(name.as_str()))
                .map_err(|_| LibraryError(LibraryErrorCode::TrashFailed))
        })
        .await
    }

    async fn delete_permanently(
        &self,
        location: &ModelLocation,
        expected: &FileIdentity,
    ) -> Result<()> {
        let (location, expected) = (location.clone(), expected.clone());
        self.blocking(move |this| {
            let (parent, name) = this.parent(&location, false).map_err(err)?;
            let file = parent.open(&name, false, false).map_err(err)?;
            if !same(&model_paths::identity(&file).map_err(err)?, &expected) {
                return Err(LibraryErrorCode::Stale.into());
            }
            drop(file);
            parent.remove_owned_file(&name).map_err(err)
        })
        .await
    }

    async fn hash(&self, location: &ModelLocation, expected: &FileIdentity) -> Result<Sha256> {
        let (location, expected) = (location.clone(), expected.clone());
        self.blocking(move |this| {
            let (parent, name) = this.parent(&location, false).map_err(err)?;
            let mut file = parent.open(&name, false, false).map_err(err)?;
            let before = model_paths::identity(&file).map_err(err)?;
            if !same(&before, &expected) {
                return Err(LibraryErrorCode::Stale.into());
            }
            let digest = model_paths::digest(&mut file).map_err(err)?;
            if !same(&model_paths::identity(&file).map_err(err)?, &before) {
                return Err(LibraryErrorCode::Stale.into());
            }
            Ok(digest)
        })
        .await
    }

    async fn write_sidecar(
        &self,
        location: &ModelLocation,
        bytes: &[u8],
        replace: bool,
    ) -> Result<()> {
        let (location, bytes) = (location.clone(), bytes.to_vec());
        self.blocking(move |this| {
            let (parent, name) = this.parent(&location, false).map_err(err)?;
            let temporary = scratch_name("sidecar", "w", ".tmp").map_err(err)?;
            let mut file = parent.open(&temporary, true, true).map_err(err)?;
            let written = file
                .write_all(&bytes)
                .and_then(|()| file.sync_all())
                .map_err(err);
            let published = written.and_then(|()| {
                let backup = if replace {
                    match parent.open(&name, false, false) {
                        Ok(existing) => {
                            let backup = scratch_name("sidecar", "old", ".bak").map_err(err)?;
                            model_paths::rename_no_replace(
                                &parent, &name, &existing, &parent, &backup,
                            )
                            .map_err(err)?;
                            Some(backup)
                        }
                        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
                        Err(e) => return Err(err(e)),
                    }
                } else {
                    None
                };
                match model_paths::rename_no_replace(&parent, &temporary, &file, &parent, &name) {
                    Ok(()) => {
                        if let Some(backup) = backup {
                            let _ = parent.remove_owned_file(&backup);
                        }
                        Ok(())
                    }
                    Err(e) => {
                        if let Some(backup) = backup
                            && let Ok(old) = parent.open(&backup, false, false)
                        {
                            let _ = model_paths::rename_no_replace(
                                &parent, &backup, &old, &parent, &name,
                            );
                        }
                        Err(err(e))
                    }
                }
            });
            if published.is_err() {
                drop(file);
                let _ = parent.remove_owned_file(&temporary);
            }
            published
        })
        .await
    }

    async fn read_sidecar(
        &self,
        location: &ModelLocation,
        limit: usize,
    ) -> Result<Option<Vec<u8>>> {
        let location = location.clone();
        self.blocking(move |this| {
            let (parent, name) = match this.parent(&location, false) {
                Ok(found) => found,
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(err(e)),
            };
            let file = match parent.open(&name, false, false) {
                Ok(file) => file,
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(err(e)),
            };
            let mut bytes = Vec::new();
            file.take(limit as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(err)?;
            if bytes.len() > limit {
                return Err(LibraryErrorCode::InvalidResponse.into());
            }
            Ok(Some(bytes))
        })
        .await
    }

    async fn explorer_target(&self, location: Option<&ModelLocation>) -> Result<ExplorerTarget> {
        let location = location.cloned();
        self.blocking(move |this| {
            let Some(location) = location else {
                let spec = this
                    .roots
                    .spec(DEFAULT_MODEL_ROOT_ID)
                    .ok_or(LibraryErrorCode::NotFound)?;
                let pinned = model_paths::pin_absolute(&spec.path).map_err(err)?;
                return Ok(ExplorerTarget::OpenFolder(pinned.path.clone()));
            };
            if location.path.components().is_empty() {
                let pinned = this.roots.pinned_directory(&location).map_err(err)?;
                return Ok(ExplorerTarget::OpenFolder(pinned.path.clone()));
            }
            let (parent, name) = this.parent(&location, false).map_err(err)?;
            let is_directory = parent
                .open_entry(&name)
                .and_then(|entry| entry.metadata())
                .map_err(err)?
                .is_dir();
            let path = parent.path.join(name.as_str());
            Ok(if is_directory {
                ExplorerTarget::OpenFolder(path)
            } else {
                ExplorerTarget::RevealItem(path)
            })
        })
        .await
    }
}

/// Copies into a private temporary file in the destination directory, flushes, verifies the
/// digest and the unchanged source, publishes without replacement, then removes the source.
fn copy_across_volumes(
    source_parent: &Arc<PinnedDirectory>,
    source_name: &SafeFileName,
    source: &mut File,
    target_parent: &Arc<PinnedDirectory>,
    target_name: &SafeFileName,
    scratch: &str,
) -> Result<Relocated> {
    let before = model_paths::identity(source).map_err(err)?;
    let temporary = scratch_name("move", scratch, ".tmp").map_err(err)?;
    let mut copy = target_parent.open(&temporary, true, true).map_err(err)?;
    let published = (|| -> Result<()> {
        source.seek(SeekFrom::Start(0)).map_err(err)?;
        io::copy(source, &mut copy).map_err(err)?;
        copy.sync_all().map_err(err)?;
        let wanted = model_paths::digest(source).map_err(err)?;
        if model_paths::digest(&mut copy).map_err(err)? != wanted {
            return Err(LibraryErrorCode::Io.into());
        }
        if !same(&model_paths::identity(source).map_err(err)?, &before) {
            return Err(LibraryErrorCode::Stale.into());
        }
        model_paths::rename_no_replace(target_parent, &temporary, &copy, target_parent, target_name)
            .map_err(err)
    })();
    if let Err(error) = published {
        drop(copy);
        let _ = target_parent.remove_owned_file(&temporary);
        return Err(error);
    }
    match source_parent.remove_owned_file(source_name) {
        Ok(()) => Ok(Relocated::Copied),
        Err(_) => Ok(Relocated::CopiedSourceRetained),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use application::comfy_runtime::{ExtraModelPaths, ModelRoot, RuntimeError};
    use domain::shared::model_values::RelativeModelPath;
    use std::path::Path;

    struct NoExtra;
    #[async_trait]
    impl ExtraModelPaths for NoExtra {
        async fn read(&self, _: &Path) -> std::result::Result<Vec<ModelRoot>, RuntimeError> {
            Ok(Vec::new())
        }
    }

    pub(crate) fn fixture(install: &Path) -> (Arc<LibraryRoots>, LocalModelFiles) {
        let roots =
            Arc::new(LibraryRoots::new(install, Arc::new(NoExtra), Arc::default()).unwrap());
        let files = LocalModelFiles {
            roots: roots.clone(),
        };
        (roots, files)
    }
    fn loc(path: &str) -> ModelLocation {
        ModelLocation::new("models", RelativeModelPath::new(path).unwrap())
    }
    fn models_dir(install: &Path) -> std::path::PathBuf {
        install.join("ComfyUI").join("models")
    }

    #[tokio::test]
    async fn relocate_never_replaces_and_reports_a_conflict() {
        let install = tempfile::tempdir().unwrap();
        let (_, files) = fixture(install.path());
        let dir = models_dir(install.path()).join("checkpoints");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.safetensors"), b"a").unwrap();
        std::fs::write(dir.join("b.safetensors"), b"b").unwrap();
        let error = files
            .relocate(
                &loc("checkpoints/a.safetensors"),
                &loc("checkpoints/b.safetensors"),
                None,
                "t",
            )
            .await
            .unwrap_err();
        assert_eq!(error.0, LibraryErrorCode::TargetExists);
        assert_eq!(std::fs::read(dir.join("b.safetensors")).unwrap(), b"b");
        files
            .relocate(
                &loc("checkpoints/a.safetensors"),
                &loc("checkpoints/ไทย.safetensors"),
                None,
                "t",
            )
            .await
            .unwrap();
        assert!(dir.join("ไทย.safetensors").exists());
        assert!(!dir.join("a.safetensors").exists());
    }

    #[tokio::test]
    async fn move_creates_the_target_folder_and_a_changed_source_is_stale() {
        let install = tempfile::tempdir().unwrap();
        let (_, files) = fixture(install.path());
        let dir = models_dir(install.path()).join("checkpoints");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.safetensors"), b"aaaa").unwrap();
        let identity = files
            .inspect(&loc("checkpoints/a.safetensors"))
            .await
            .unwrap()
            .unwrap();
        std::fs::write(dir.join("a.safetensors"), b"changed content").unwrap();
        let stale = files
            .relocate(
                &loc("checkpoints/a.safetensors"),
                &loc("loras/new folder/a.safetensors"),
                Some(&identity),
                "t",
            )
            .await
            .unwrap_err();
        assert_eq!(stale.0, LibraryErrorCode::Stale);
        files
            .relocate(
                &loc("checkpoints/a.safetensors"),
                &loc("loras/new folder/a.safetensors"),
                None,
                "t",
            )
            .await
            .unwrap();
        assert!(
            models_dir(install.path())
                .join("loras/new folder/a.safetensors")
                .exists()
        );
    }

    #[tokio::test]
    async fn locations_cannot_leave_their_root() {
        let install = tempfile::tempdir().unwrap();
        let (_, files) = fixture(install.path());
        std::fs::write(install.path().join("secret.txt"), b"s").unwrap();
        // Path values are validated, so the only escape routes left are unknown roots.
        let outside = ModelLocation::new("elsewhere", RelativeModelPath::new("x/a.pt").unwrap());
        assert_eq!(
            files.inspect(&outside).await.unwrap_err().0,
            LibraryErrorCode::OutsideRoot
        );
        assert!(RelativeModelPath::new("../secret.txt").is_err());
        assert!(RelativeModelPath::new("a/../../secret.txt").is_err());
        assert!(RelativeModelPath::new("C:/Windows/x").is_err());
        assert!(RelativeModelPath::new("\\\\server\\share\\x").is_err());
    }

    #[tokio::test]
    async fn hash_and_trash_refuse_a_changed_identity() {
        let install = tempfile::tempdir().unwrap();
        let (_, files) = fixture(install.path());
        let dir = models_dir(install.path()).join("vae");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("v.pt"), b"first").unwrap();
        let identity = files.inspect(&loc("vae/v.pt")).await.unwrap().unwrap();
        let hash = files.hash(&loc("vae/v.pt"), &identity).await.unwrap();
        assert_eq!(hash.as_str().len(), 64);
        std::fs::write(dir.join("v.pt"), b"second!").unwrap();
        assert_eq!(
            files.hash(&loc("vae/v.pt"), &identity).await.unwrap_err().0,
            LibraryErrorCode::Stale
        );
        assert_eq!(
            files
                .trash(&loc("vae/v.pt"), &identity)
                .await
                .unwrap_err()
                .0,
            LibraryErrorCode::Stale
        );
        assert!(dir.join("v.pt").exists());
        let live = files.inspect(&loc("vae/v.pt")).await.unwrap().unwrap();
        files
            .delete_permanently(&loc("vae/v.pt"), &live)
            .await
            .unwrap();
        assert!(!dir.join("v.pt").exists());
    }

    #[tokio::test]
    async fn sidecars_are_created_exclusively_and_replaced_only_on_request() {
        let install = tempfile::tempdir().unwrap();
        let (_, files) = fixture(install.path());
        let dir = models_dir(install.path()).join("loras");
        std::fs::create_dir_all(&dir).unwrap();
        let sidecar = loc("loras/x.civitai.info");
        files.write_sidecar(&sidecar, b"one", false).await.unwrap();
        assert_eq!(
            files
                .write_sidecar(&sidecar, b"two", false)
                .await
                .unwrap_err()
                .0,
            LibraryErrorCode::TargetExists
        );
        assert_eq!(std::fs::read(dir.join("x.civitai.info")).unwrap(), b"one");
        files.write_sidecar(&sidecar, b"three", true).await.unwrap();
        assert_eq!(std::fs::read(dir.join("x.civitai.info")).unwrap(), b"three");
        assert_eq!(
            std::fs::read_dir(&dir).unwrap().count(),
            1,
            "no temporary leftovers"
        );
        assert_eq!(
            files.read_sidecar(&sidecar, 100).await.unwrap().unwrap(),
            b"three"
        );
        assert!(files.read_sidecar(&sidecar, 2).await.is_err());
        assert!(
            files
                .read_sidecar(&loc("loras/none.json"), 2)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn a_file_held_without_delete_sharing_is_reported_as_locked() {
        use std::os::windows::fs::OpenOptionsExt;
        let install = tempfile::tempdir().unwrap();
        let (_, files) = fixture(install.path());
        let dir = models_dir(install.path()).join("checkpoints");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("held.safetensors");
        std::fs::write(&path, b"held").unwrap();
        // FILE_SHARE_READ only: another process reading the model without allowing delete.
        let _holder = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&path)
            .unwrap();
        let location = loc("checkpoints/held.safetensors");
        assert_eq!(
            files.check_unlocked(&location).await.unwrap_err().0,
            LibraryErrorCode::Locked
        );
        assert_eq!(
            files
                .relocate(&location, &loc("checkpoints/other.safetensors"), None, "t")
                .await
                .unwrap_err()
                .0,
            LibraryErrorCode::Locked
        );
        assert!(path.exists());
    }

    #[tokio::test]
    async fn explorer_targets_open_folders_and_reveal_files_of_the_default_root() {
        let install = tempfile::tempdir().unwrap();
        let (_, files) = fixture(install.path());
        let dir = models_dir(install.path());
        std::fs::create_dir_all(dir.join("loras/sub")).unwrap();
        std::fs::write(dir.join("loras/a.safetensors"), b"a").unwrap();
        // The roots are pinned from their canonical spelling.
        let root = std::fs::canonicalize(&dir).unwrap();
        assert_eq!(
            files.explorer_target(None).await.unwrap(),
            ExplorerTarget::OpenFolder(root.clone())
        );
        assert_eq!(
            files.explorer_target(Some(&loc(""))).await.unwrap(),
            ExplorerTarget::OpenFolder(root.clone())
        );
        assert_eq!(
            files.explorer_target(Some(&loc("loras"))).await.unwrap(),
            ExplorerTarget::OpenFolder(root.join("loras"))
        );
        assert_eq!(
            files
                .explorer_target(Some(&loc("loras/sub")))
                .await
                .unwrap(),
            ExplorerTarget::OpenFolder(root.join("loras").join("sub"))
        );
        assert_eq!(
            files
                .explorer_target(Some(&loc("loras/a.safetensors")))
                .await
                .unwrap(),
            ExplorerTarget::RevealItem(root.join("loras").join("a.safetensors"))
        );
        for missing in ["loras/none.pt", "nothing/deeper/x.pt"] {
            assert_eq!(
                files
                    .explorer_target(Some(&loc(missing)))
                    .await
                    .unwrap_err()
                    .0,
                LibraryErrorCode::NotFound,
                "{missing}"
            );
        }
        assert!(!dir.join("nothing").exists(), "nothing is created");
        let elsewhere = ModelLocation::new("elsewhere", RelativeModelPath::new("x").unwrap());
        assert_eq!(
            files.explorer_target(Some(&elsewhere)).await.unwrap_err().0,
            LibraryErrorCode::OutsideRoot
        );
    }

    #[tokio::test]
    async fn the_default_root_is_not_found_until_it_is_registered() {
        let install = tempfile::tempdir().unwrap();
        let roots = Arc::new(
            LibraryRoots::deferred(install.path(), Arc::new(NoExtra), Arc::default()).unwrap(),
        );
        let files = LocalModelFiles { roots };
        assert_eq!(
            files.explorer_target(None).await.unwrap_err().0,
            LibraryErrorCode::NotFound
        );
        assert!(!install.path().join("ComfyUI").exists());
    }

    #[tokio::test]
    async fn lists_only_regular_files_of_a_directory() {
        let install = tempfile::tempdir().unwrap();
        let (_, files) = fixture(install.path());
        let dir = models_dir(install.path()).join("loras");
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.pt"), b"a").unwrap();
        let listing = files
            .list_directory(&ModelLocation::new(
                "models",
                RelativeModelPath::new("loras").unwrap(),
            ))
            .await
            .unwrap();
        assert_eq!(listing.len(), 1);
    }
}
