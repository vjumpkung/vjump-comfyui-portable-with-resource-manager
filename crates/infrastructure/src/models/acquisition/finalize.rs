use crate::shared::{
    model_paths::{self, AuthorizedModelRoots, PinnedDirectory},
    trash,
};
use application::{models::acquisition::*, shared::path_locks::ModelPathLocks};
use async_trait::async_trait;
use domain::{
    models::acquisition::*,
    shared::model_values::{RelativeModelPath, SafeFileName, Sha256},
};
use std::{
    fs::{self, File},
    io,
    path::Path,
    sync::Arc,
};

pub struct StagingArea {
    pub root: Arc<PinnedDirectory>,
}
impl StagingArea {
    pub fn new(path: &Path) -> io::Result<Self> {
        fs::create_dir_all(path)?;
        Ok(Self {
            root: model_paths::pin_absolute(&fs::canonicalize(path)?)?,
        })
    }
    pub fn part_key(job: &DownloadJob) -> String {
        format!("{}.part", job.id.as_str())
    }
    pub fn open_key(
        &self,
        key: &str,
        write: bool,
        create: bool,
    ) -> io::Result<(Arc<PinnedDirectory>, SafeFileName, File)> {
        let relative = RelativeModelPath::new(key).map_err(|_| io::ErrorKind::PermissionDenied)?;
        let (name, parents) = relative
            .components()
            .split_last()
            .ok_or(io::ErrorKind::InvalidInput)?;
        let mut parent = self.root.clone();
        for p in parents {
            parent = parent.child(p, create)?;
        }
        let file = match parent.open(name, write, create) {
            Err(e) if create && e.kind() == io::ErrorKind::AlreadyExists => {
                parent.open(name, write, false)?
            }
            r => r?,
        };
        Ok((parent, name.clone(), file))
    }
    pub fn hf_directory(
        &self,
        job: &DownloadJob,
        create: bool,
    ) -> io::Result<Arc<PinnedDirectory>> {
        self.job_directory("hf", job, create)
    }
    pub fn gdrive_directory(
        &self,
        job: &DownloadJob,
        create: bool,
    ) -> io::Result<Arc<PinnedDirectory>> {
        self.job_directory("gdrive", job, create)
    }
    fn job_directory(
        &self,
        group: &str,
        job: &DownloadJob,
        create: bool,
    ) -> io::Result<Arc<PinnedDirectory>> {
        self.root
            .child(&SafeFileName::new(group).unwrap(), create)?
            .child(
                &SafeFileName::new(job.id.as_str()).map_err(|_| io::ErrorKind::InvalidInput)?,
                create,
            )
    }
    fn remove_tree(directory: &Arc<PinnedDirectory>) -> io::Result<()> {
        // Enumerate only inside a pinned owned directory; reject links rather than following them.
        for entry in fs::read_dir(&directory.path)? {
            let entry = entry?;
            let name = SafeFileName::new(entry.file_name().to_string_lossy().into_owned())
                .map_err(|_| io::ErrorKind::PermissionDenied)?;
            if entry.file_type()?.is_dir() {
                let child = directory.child(&name, false)?;
                Self::remove_tree(&child)?;
                drop(child);
                fs::remove_dir(directory.path.join(name.as_str()))?;
            } else {
                let file = directory.open(&name, false, false)?;
                drop(file);
                directory.remove_owned_file(&name)?;
            }
        }
        Ok(())
    }
}
pub struct LocalDownloadFiles {
    pub roots: Arc<AuthorizedModelRoots>,
    pub staging: Arc<StagingArea>,
    pub locks: Arc<dyn ModelPathLocks>,
}
fn disk(_: io::Error) -> DownloadError {
    DownloadErrorCode::Disk.into()
}
fn path_error(e: io::Error) -> DownloadError {
    if matches!(
        e.kind(),
        io::ErrorKind::PermissionDenied | io::ErrorKind::InvalidInput
    ) {
        DownloadErrorCode::Containment.into()
    } else if e.kind() == io::ErrorKind::AlreadyExists {
        DownloadErrorCode::Conflict.into()
    } else {
        disk(e)
    }
}
#[async_trait]
impl DownloadFiles for LocalDownloadFiles {
    async fn inspect(&self, destination: &Destination) -> Result<Option<FileIdentity>> {
        let roots = self.roots.clone();
        let d = destination.clone();
        tokio::task::spawn_blocking(move || {
            let (parent, name) = match roots.parent(&d.root_id, &d.relative_path(), false) {
                Ok(p) => p,
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(path_error(e)),
            };
            match parent.open(&name, false, false) {
                Ok(file) => model_paths::identity(&file).map(Some).map_err(path_error),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(path_error(e)),
            }
        })
        .await
        .map_err(|_| DownloadErrorCode::Disk)?
    }
    async fn verify(
        &self,
        staged_key: &str,
        expected: Option<&Sha256>,
        size: DownloadSize,
    ) -> Result<(FileIdentity, Sha256)> {
        let staging = self.staging.clone();
        let key = staged_key.to_owned();
        let expected = expected.cloned();
        tokio::task::spawn_blocking(move || {
            let (_, _, mut file) = staging.open_key(&key, true, false).map_err(path_error)?;
            file.sync_all().map_err(disk)?;
            let before = model_paths::identity(&file).map_err(disk)?;
            if matches!(size,DownloadSize::Exact(n) if n!=before.size) {
                return Err(DownloadErrorCode::LengthMismatch.into());
            }
            let hash = model_paths::digest(&mut file).map_err(disk)?;
            if model_paths::identity(&file).map_err(disk)? != before {
                return Err(DownloadErrorCode::Containment.into());
            }
            if expected.as_ref().is_some_and(|e| e != &hash) {
                return Err(DownloadErrorCode::HashMismatch.into());
            }
            Ok((before, hash))
        })
        .await
        .map_err(|_| DownloadErrorCode::Disk)?
    }
    async fn publish(&self, job: &DownloadJob) -> Result<FileIdentity> {
        let _lease = self.locks.acquire(&job.destination.reservation_key()).await;
        let roots = self.roots.clone();
        let staging = self.staging.clone();
        let job = job.clone();
        tokio::task::spawn_blocking(move || publish(&roots, &staging, &job))
            .await
            .map_err(|_| DownloadErrorCode::Disk)?
    }
    async fn recycle_backup(&self, job: &DownloadJob) -> Result<()> {
        let _lease = self.locks.acquire(&job.destination.reservation_key()).await;
        let roots = self.roots.clone();
        let job = job.clone();
        tokio::task::spawn_blocking(move || {
            let intent = job
                .publication
                .as_ref()
                .ok_or(DownloadErrorCode::InvalidTransition)?;
            let (parent, _) = roots
                .parent(
                    &job.destination.root_id,
                    &job.destination.relative_path(),
                    false,
                )
                .map_err(path_error)?;
            match parent.open(&intent.backup_name, false, false) {
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(path_error(e)),
                Ok(file) => {
                    if let Some(ConflictChoice::Overwrite { approved }) = &job.conflict {
                        if &model_paths::identity(&file).map_err(disk)? != approved {
                            return Err(DownloadErrorCode::Conflict.into());
                        }
                    } else {
                        return Err(DownloadErrorCode::Conflict.into());
                    }
                    drop(file);
                    trash::recycle(&parent.path.join(intent.backup_name.as_str()))
                        .map_err(|_| DownloadErrorCode::TrashFailed.into())
                }
            }
        })
        .await
        .map_err(|_| DownloadErrorCode::Disk)?
    }
    async fn cleanup(&self, job: &DownloadJob, preserve_native: bool) -> Result<()> {
        let staging = self.staging.clone();
        let job = job.clone();
        tokio::task::spawn_blocking(move || {
            if !preserve_native {
                let name = SafeFileName::new(StagingArea::part_key(&job))
                    .map_err(|_| DownloadErrorCode::Containment)?;
                match staging.root.remove_owned_file(&name) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(path_error(e)),
                }
            }
            for directory in [
                staging.hf_directory(&job, false),
                staging.gdrive_directory(&job, false),
            ] {
                match directory {
                    Ok(dir) => {
                        StagingArea::remove_tree(&dir).map_err(path_error)?;
                        let path = dir.path.clone();
                        drop(dir);
                        fs::remove_dir(path).map_err(path_error)?;
                    }
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(path_error(e)),
                }
            }
            Ok(())
        })
        .await
        .map_err(|_| DownloadErrorCode::Disk)?
    }
    async fn clean_orphans(&self, keep: &[DownloadJob]) -> Result<()> {
        let staging = self.staging.clone();
        let keep = keep.to_vec();
        tokio::task::spawn_blocking(move || {
            for entry in fs::read_dir(&staging.root.path).map_err(disk)? {
                let entry = entry.map_err(disk)?;
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.ends_with(".part") && !keep.iter().any(|j| StagingArea::part_key(j) == name)
                {
                    // Only the strict job-id namespace is owned; unrelated files are untouched.
                    if domain::shared::model_values::JobId::new(name.trim_end_matches(".part"))
                        .is_ok()
                    {
                        let name =
                            SafeFileName::new(name).map_err(|_| DownloadErrorCode::Containment)?;
                        let file = staging.root.open(&name, false, false).map_err(path_error)?;
                        drop(file);
                        staging.root.remove_owned_file(&name).map_err(path_error)?;
                    }
                }
            }
            for group in ["hf", "gdrive"] {
                let group = match staging
                    .root
                    .child(&SafeFileName::new(group).unwrap(), false)
                {
                    Ok(d) => d,
                    Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                    Err(e) => return Err(path_error(e)),
                };
                for entry in fs::read_dir(&group.path).map_err(disk)? {
                    let entry = entry.map_err(disk)?;
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if keep
                        .iter()
                        .any(|j| j.id.as_str() == name && j.publication.is_some())
                    {
                        continue;
                    }
                    if domain::shared::model_values::JobId::new(name.clone()).is_err() {
                        continue;
                    }
                    let name =
                        SafeFileName::new(name).map_err(|_| DownloadErrorCode::Containment)?;
                    let dir = group.child(&name, false).map_err(path_error)?;
                    StagingArea::remove_tree(&dir).map_err(path_error)?;
                    let path = dir.path.clone();
                    drop(dir);
                    fs::remove_dir(path).map_err(path_error)?;
                }
            }
            Ok(())
        })
        .await
        .map_err(|_| DownloadErrorCode::Disk)?
    }
}
fn publish(
    roots: &AuthorizedModelRoots,
    staging: &StagingArea,
    job: &DownloadJob,
) -> Result<FileIdentity> {
    let intent = job
        .publication
        .as_ref()
        .ok_or(DownloadErrorCode::InvalidTransition)?;
    let (parent, name) = roots
        .parent(
            &job.destination.root_id,
            &job.destination.relative_path(),
            true,
        )
        .map_err(path_error)?;
    let stage = staging.open_key(&intent.staged_key, false, false);
    match parent.open(&name, false, false) {
        Ok(mut existing) => {
            let identity = model_paths::identity(&existing).map_err(disk)?;
            // Recover a rename/copy that succeeded before the published DB update.
            if model_paths::digest(&mut existing).map_err(disk)? == intent.digest
                && (stage.is_err()
                    || identity.platform_id == intent.staged_identity.platform_id
                    || parent.open(&intent.backup_name, false, false).is_ok())
            {
                return Ok(identity);
            }
            let approved = match &job.conflict {
                Some(ConflictChoice::Overwrite { approved }) => approved,
                _ => return Err(DownloadErrorCode::Conflict.into()),
            };
            if &identity != approved {
                return Err(DownloadErrorCode::Conflict.into());
            }
            if parent.open(&intent.backup_name, false, false).is_ok() {
                return Err(DownloadErrorCode::Conflict.into());
            }
            model_paths::rename_no_replace(&parent, &name, &existing, &parent, &intent.backup_name)
                .map_err(path_error)?;
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(path_error(e)),
    }
    let (stage_parent, stage_name, mut file) = stage.map_err(path_error)?;
    if model_paths::identity(&file).map_err(disk)? != intent.staged_identity
        || model_paths::digest(&mut file).map_err(disk)? != intent.digest
    {
        return Err(DownloadErrorCode::HashMismatch.into());
    }
    match model_paths::rename_no_replace(&stage_parent, &stage_name, &file, &parent, &name) {
        Ok(()) => {}
        Err(e) if is_cross_volume(&e) => {
            let temporary = SafeFileName::new(format!(
                ".download-{}-{}.publish",
                job.id.as_str(),
                job.generation
            ))
            .map_err(|_| DownloadErrorCode::Containment)?;
            let mut copy = match parent.open(&temporary, true, true) {
                Ok(mut target) => {
                    use std::io::{Seek, SeekFrom};
                    file.seek(SeekFrom::Start(0)).map_err(disk)?;
                    io::copy(&mut file, &mut target).map_err(disk)?;
                    target.sync_all().map_err(disk)?;
                    target
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    parent.open(&temporary, true, false).map_err(path_error)?
                }
                Err(e) => return Err(path_error(e)),
            };
            if model_paths::digest(&mut copy).map_err(disk)? != intent.digest {
                return Err(DownloadErrorCode::HashMismatch.into());
            }
            model_paths::rename_no_replace(&parent, &temporary, &copy, &parent, &name)
                .map_err(path_error)?;
        }
        Err(e) => return Err(path_error(e)),
    }
    model_paths::identity(&parent.open(&name, false, false).map_err(path_error)?).map_err(disk)
}
fn is_cross_volume(e: &io::Error) -> bool {
    e.raw_os_error() == Some(if cfg!(windows) { 17 } else { 18 })
}
