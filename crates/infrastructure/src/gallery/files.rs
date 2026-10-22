//! Gallery file operations over pinned, non-following parent directories. `ComfyUI/input` and
//! `ComfyUI/output` are resolved from the install root on every call and are never canonicalized;
//! a link or reparse point anywhere in the chain is refused, never followed. Copies are staged
//! below `<area>/.vjump-gallery-staging/<operation>/` and published with a no-replace rename, so
//! an incomplete copy is never visible. Deletion only ever goes to the recycle bin.
use crate::{
    custom_nodes::tree::is_link,
    models::library::file_ops::is_cross_volume,
    shared::{
        model_paths::{PinnedDirectory, digest, hex, identity, pin_absolute, rename_no_replace},
        trash::recycle_entry,
    },
};
use application::{
    gallery::*,
    shared::{ExplorerTarget, Retained},
};
use async_trait::async_trait;
use domain::{
    gallery::{GalleryArea, GalleryEntryKind, GalleryLocation, STAGING_DIR, classify_media},
    models::acquisition::FileIdentity,
    shared::model_values::SafeFileName,
};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::UNIX_EPOCH,
};

/// The ownership marker inside every staging operation directory.
const OWNER: &str = ".owner";
const OWNER_PREFIX: &str = "vjump-gallery-staging:";
/// Deepest folder nesting that is fingerprinted. A copy stops one level short, because its
/// staging directory adds a level that removing the staged tree must still be able to reach.
const MAX_DEPTH: usize = 64;
/// Highest `name (n).ext` suffix tried before giving up on a conflict.
const MAX_SUFFIX: u32 = 999;

/// Fault injection for tests. A request takes the faults armed on its calling thread, so they
/// reach the blocking worker that runs it.
#[derive(Default)]
pub(crate) struct Faults {
    /// Skips the same-volume rename so a move takes the staged copy path.
    pub force_cross_device: bool,
    pub fail_before_publish: bool,
    /// Fails right after publication, before any source is removed.
    pub fail_after_publish: bool,
    /// Fails the first removal of the source cleanup.
    pub fail_during_cleanup: bool,
    /// Runs on the worker after publication, to change the source behind the operation's back.
    pub after_publish: Option<Box<dyn FnOnce() + Send>>,
}

#[cfg(test)]
thread_local! {
    static FAULTS: std::cell::Cell<Faults> = std::cell::Cell::new(Faults::default());
}

#[cfg(test)]
pub(crate) fn inject(faults: Faults) {
    FAULTS.with(|slot| slot.set(faults));
}

fn take_faults() -> Faults {
    #[cfg(test)]
    {
        FAULTS.with(std::cell::Cell::take)
    }
    #[cfg(not(test))]
    {
        Faults::default()
    }
}

/// Maps an operating-system error to a gallery code. Sharing violations are "locked"; a bare
/// `PermissionDenied` kind is a refusal raised by the pinned traversal itself (a link, a
/// reparse point or a special file).
fn classify(error: &io::Error) -> GalleryErrorCode {
    match error.raw_os_error() {
        #[cfg(windows)]
        Some(32 | 33) => GalleryErrorCode::Locked,
        #[cfg(unix)]
        Some(libc::EBUSY | libc::ETXTBSY) => GalleryErrorCode::Locked,
        #[cfg(unix)]
        Some(libc::ELOOP) => GalleryErrorCode::AccessDenied,
        _ => match error.kind() {
            io::ErrorKind::PermissionDenied => GalleryErrorCode::AccessDenied,
            io::ErrorKind::InvalidInput | io::ErrorKind::NotADirectory => {
                GalleryErrorCode::InvalidRequest
            }
            io::ErrorKind::InvalidFilename => GalleryErrorCode::InvalidName,
            io::ErrorKind::NotFound => GalleryErrorCode::NotFound,
            io::ErrorKind::AlreadyExists => GalleryErrorCode::TargetExists,
            _ => GalleryErrorCode::Io,
        },
    }
}
fn err(error: io::Error) -> GalleryError {
    GalleryError(classify(&error))
}
/// For entries met while walking a tree: a refusal of the pinned open means the tree holds a
/// link or special file, and a vanished entry means the tree changed.
fn tree_error(error: io::Error) -> GalleryError {
    GalleryError(match classify(&error) {
        GalleryErrorCode::NotFound => GalleryErrorCode::Stale,
        GalleryErrorCode::AccessDenied if is_refusal(&error) => GalleryErrorCode::Unsupported,
        code => code,
    })
}
/// A bare `PermissionDenied` (no OS code) or `ELOOP`: the pinned open refused a link or special
/// file, as opposed to the OS denying access.
fn is_refusal(error: &io::Error) -> bool {
    #[cfg(unix)]
    {
        error.raw_os_error().is_none() || error.raw_os_error() == Some(libc::ELOOP)
    }
    #[cfg(not(unix))]
    {
        error.raw_os_error().is_none()
    }
}
fn trash_error(error: &io::Error) -> GalleryError {
    GalleryError(if classify(error) == GalleryErrorCode::Locked {
        GalleryErrorCode::Locked
    } else {
        GalleryErrorCode::TrashFailed
    })
}

fn constant(name: &str) -> SafeFileName {
    SafeFileName::new(name).expect("constant is a safe file name")
}
/// Opaque revision text: any change of identity, size or modification time changes it.
fn revision_of(identity: &FileIdentity) -> String {
    format!(
        "{}:{}:{}",
        identity.platform_id, identity.size, identity.modified_ns
    )
}
fn modified_ms(meta: &fs::Metadata) -> Option<i64> {
    let since = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(since.as_millis()).ok()
}
/// The volume (device) part of a platform identity.
fn volume(file: &File) -> io::Result<String> {
    let id = identity(file)?.platform_id;
    Ok(id.split(':').next().unwrap_or_default().to_owned())
}

/// `<root>/ComfyUI/<area>`. A missing install is `NotInstalled`; `ComfyUI` is never created.
fn area_root(root: &Path, area: GalleryArea, create: bool) -> Result<Arc<PinnedDirectory>> {
    let missing = |error: io::Error| {
        if error.kind() == io::ErrorKind::NotFound {
            GalleryError(GalleryErrorCode::NotInstalled)
        } else {
            err(error)
        }
    };
    let install = pin_absolute(root).map_err(missing)?;
    let comfy = install
        .child(&constant("ComfyUI"), false)
        .map_err(missing)?;
    comfy.child(&constant(area.dir_name()), create).map_err(err)
}
fn resolve_directory(
    root: &Path,
    location: &GalleryLocation,
    create: bool,
) -> Result<Arc<PinnedDirectory>> {
    let mut directory = area_root(root, location.area(), create)?;
    for part in location.path().components() {
        directory = directory.child(part, false).map_err(err)?;
    }
    Ok(directory)
}
fn resolve_parent(
    root: &Path,
    location: &GalleryLocation,
    create: bool,
) -> Result<(Arc<PinnedDirectory>, SafeFileName)> {
    let name = location
        .name()
        .ok_or(GalleryErrorCode::InvalidRequest)?
        .clone();
    let parent = location.parent().ok_or(GalleryErrorCode::InvalidRequest)?;
    Ok((resolve_directory(root, &parent, create)?, name))
}
/// Opens the entry without following links and refuses it when it is not the one the caller saw.
fn current_entry(
    parent: &PinnedDirectory,
    name: &SafeFileName,
    revision: &EntryRevision,
) -> Result<File> {
    let file = parent.open_entry(name).map_err(err)?;
    if revision_of(&identity(&file).map_err(err)?) != revision.0 {
        return Err(GalleryErrorCode::Stale.into());
    }
    Ok(file)
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| GalleryError(GalleryErrorCode::Io))?
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LocalGalleryFiles;

impl LocalGalleryFiles {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl GalleryFiles for LocalGalleryFiles {
    async fn list(&self, root: &Path, directory: &GalleryLocation) -> Result<GalleryListing> {
        let (root, directory) = (root.to_owned(), directory.clone());
        blocking(move || list_directory(&root, &directory)).await
    }

    async fn rename(
        &self,
        root: &Path,
        entry: &GalleryEntryRef,
        new_name: &SafeFileName,
    ) -> Result<GalleryLocation> {
        let (root, entry, new_name) = (root.to_owned(), entry.clone(), new_name.clone());
        blocking(move || rename_entry(&root, &entry, &new_name)).await
    }

    async fn transfer(
        &self,
        root: &Path,
        entry: &GalleryEntryRef,
        destination: &GalleryLocation,
        mode: TransferMode,
        operation: &str,
        retained: Retained,
    ) -> ItemResult {
        let source = entry.location.clone();
        let job = Job {
            root: root.to_owned(),
            entry: entry.clone(),
            destination: destination.clone(),
            mode,
            operation: operation.to_owned(),
            faults: take_faults(),
        };
        let outcome = tokio::task::spawn_blocking(move || {
            // The gate's protection outlives the file work, even when the caller stops waiting.
            let _retained = retained;
            place(job)
        })
        .await
        .map_err(|_| GalleryError(GalleryErrorCode::Io))
        .and_then(|placed| placed);
        match outcome {
            Ok(placed) => ItemResult {
                source,
                destination: Some(placed.location),
                outcome: if placed.retained {
                    ItemOutcome::SourceRetained
                } else {
                    ItemOutcome::Succeeded
                },
                error: placed
                    .retained
                    .then(|| GalleryErrorCode::SourceRetained.into()),
            },
            Err(error) => ItemResult {
                source,
                destination: None,
                outcome: ItemOutcome::Failed,
                error: Some(error),
            },
        }
    }

    async fn plan_delete(&self, root: &Path, entry: &GalleryEntryRef) -> Result<DeletePlanItem> {
        let (root, entry) = (root.to_owned(), entry.clone());
        blocking(move || {
            let (parent, name) = resolve_parent(&root, &entry.location, true)?;
            let described = describe(&parent, &name)?;
            if revision_of(&described.identity) != entry.revision.0 {
                return Err(GalleryErrorCode::Stale.into());
            }
            Ok(DeletePlanItem {
                entry,
                kind: described.kind,
                bytes: described.bytes,
                fingerprint: described.fingerprint,
            })
        })
        .await
    }

    async fn trash(&self, root: &Path, item: &DeletePlanItem) -> Result<()> {
        let (root, item) = (root.to_owned(), item.clone());
        blocking(move || {
            let (parent, name) = resolve_parent(&root, &item.entry.location, true)?;
            let described = describe(&parent, &name)?;
            if revision_of(&described.identity) != item.entry.revision.0
                || described.kind != item.kind
                || described.fingerprint != item.fingerprint
            {
                return Err(GalleryErrorCode::Stale.into());
            }
            recycle_entry(&parent, &name, &described.identity).map_err(|e| trash_error(&e))
        })
        .await
    }

    async fn open_media(
        &self,
        root: &Path,
        location: &GalleryLocation,
    ) -> Result<Box<dyn GalleryMedia>> {
        let (root, location) = (root.to_owned(), location.clone());
        blocking(move || {
            let (parent, name) = resolve_parent(&root, &location, true)?;
            let entry = parent.open_entry(&name).map_err(err)?;
            let regular = entry.metadata().map_err(err)?.is_file();
            // The entry handle allows deletion, which the read pin below must not coexist with.
            drop(entry);
            if !regular {
                return Err(GalleryErrorCode::InvalidRequest.into());
            }
            let file = parent.open_read_pin(&name).map_err(err)?;
            let len = file.metadata().map_err(err)?.len();
            Ok(Box::new(LocalMedia { file, len }) as Box<dyn GalleryMedia>)
        })
        .await
    }

    async fn explorer_target(
        &self,
        root: &Path,
        location: &GalleryLocation,
    ) -> Result<ExplorerTarget> {
        let (root, location) = (root.to_owned(), location.clone());
        blocking(move || {
            // Looking at a folder never creates one: a missing area folder is simply not found.
            if location.is_root() {
                let directory = area_root(&root, location.area(), false)?;
                return Ok(ExplorerTarget::OpenFolder(directory.path.clone()));
            }
            let (parent, name) = resolve_parent(&root, &location, false)?;
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

    async fn cleanup_staging(&self, root: &Path) -> Vec<GalleryWarning> {
        let root = root.to_owned();
        tokio::task::spawn_blocking(move || clean_staging(&root))
            .await
            .unwrap_or_default()
    }
}

/// A pinned regular file for positional preview reads.
struct LocalMedia {
    file: File,
    len: u64,
}
impl GalleryMedia for LocalMedia {
    fn len(&self) -> u64 {
        self.len
    }
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        #[cfg(unix)]
        {
            std::os::unix::fs::FileExt::read_at(&self.file, buf, offset)
        }
        #[cfg(windows)]
        {
            std::os::windows::fs::FileExt::seek_read(&self.file, buf, offset)
        }
    }
}

// ---- listing ----

fn list_directory(root: &Path, directory: &GalleryLocation) -> Result<GalleryListing> {
    let pinned = resolve_directory(root, directory, true)?;
    let mut entries = Vec::new();
    let mut warnings = Vec::new();
    for item in fs::read_dir(pinned.anchored_path()).map_err(err)? {
        let Ok(item) = item else {
            warnings.push(GalleryWarning {
                name: None,
                code: GalleryWarningCode::Unreadable,
            });
            continue;
        };
        let raw = item.file_name();
        let lossy = raw.to_string_lossy().into_owned();
        // The app's own staging folder is never part of the gallery.
        if lossy.eq_ignore_ascii_case(STAGING_DIR) {
            continue;
        }
        let name = raw.to_str().and_then(|text| SafeFileName::new(text).ok());
        let listed = match name {
            Some(name) => list_entry(&pinned, directory, &item, name),
            None => Err(GalleryWarningCode::UnsafeName),
        };
        match listed {
            Ok(Some(entry)) => entries.push(entry),
            Ok(None) => {}
            Err(code) => warnings.push(GalleryWarning {
                name: Some(lossy),
                code,
            }),
        }
    }
    entries.sort_by(|a, b| {
        let directory = |entry: &GalleryEntry| entry.kind == GalleryEntryKind::Directory;
        directory(b)
            .cmp(&directory(a))
            .then_with(|| {
                a.name
                    .as_str()
                    .to_lowercase()
                    .cmp(&b.name.as_str().to_lowercase())
            })
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(GalleryListing {
        directory: directory.clone(),
        entries,
        warnings,
    })
}

/// `Ok(None)` is an entry that vanished while listing; `Err` is the warning to report instead.
fn list_entry(
    pinned: &PinnedDirectory,
    directory: &GalleryLocation,
    item: &fs::DirEntry,
    name: SafeFileName,
) -> std::result::Result<Option<GalleryEntry>, GalleryWarningCode> {
    let listed = match item.metadata() {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(GalleryWarningCode::Unreadable),
    };
    if is_link(&listed) {
        return Err(GalleryWarningCode::Link);
    }
    if !listed.is_file() && !listed.is_dir() {
        return Err(GalleryWarningCode::Special);
    }
    let file = match pinned.open_entry(&name) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(GalleryWarningCode::Unreadable),
    };
    let (Ok(meta), Ok(id)) = (file.metadata(), identity(&file)) else {
        return Err(GalleryWarningCode::Unreadable);
    };
    let location = directory
        .join(name.clone())
        .map_err(|_| GalleryWarningCode::UnsafeName)?;
    let is_directory = meta.is_dir();
    Ok(Some(GalleryEntry {
        location,
        revision: EntryRevision(revision_of(&id)),
        kind: if is_directory {
            GalleryEntryKind::Directory
        } else {
            GalleryEntryKind::File
        },
        media: if is_directory {
            domain::gallery::GalleryMediaKind::Other
        } else {
            classify_media(&name)
        },
        size_bytes: (!is_directory).then_some(meta.len()),
        modified_ms: modified_ms(&meta),
        name,
    }))
}

// ---- rename ----

fn rename_entry(
    root: &Path,
    entry: &GalleryEntryRef,
    new_name: &SafeFileName,
) -> Result<GalleryLocation> {
    if new_name.as_str().eq_ignore_ascii_case(STAGING_DIR) {
        return Err(GalleryErrorCode::InvalidName.into());
    }
    let (parent, name) = resolve_parent(root, &entry.location, true)?;
    let file = current_entry(&parent, &name, &entry.revision)?;
    let case_only = name != *new_name && name.as_str().eq_ignore_ascii_case(new_name.as_str());
    let attempt = rename_no_replace(&parent, &name, &file, &parent, new_name);
    if case_only {
        // Whatever the attempt reported, the volume decides: a case-insensitive one may treat the
        // new spelling as the entry itself, a case-sensitive one may hold a different entry
        // under it. Success needs the entry's own respelling to be visible on disk.
        if !respelled(&parent, &file, &name, new_name) {
            return Err(match attempt {
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    GalleryErrorCode::TargetExists.into()
                }
                Err(e) => err(e),
                Ok(()) => GalleryErrorCode::Unsupported.into(),
            });
        }
    } else {
        attempt.map_err(err)?;
    }
    drop(file);
    entry
        .location
        .parent()
        .ok_or(GalleryErrorCode::InvalidRequest)?
        .join(new_name.clone())
        .map_err(|_| GalleryErrorCode::InvalidName.into())
}

/// After a case-only rename: the new spelling opens the very entry that was renamed, is listed
/// with exactly that spelling, and the old spelling is no longer listed.
fn respelled(
    parent: &PinnedDirectory,
    file: &File,
    old: &SafeFileName,
    new: &SafeFileName,
) -> bool {
    let (Ok(renamed), Ok(found)) = (
        identity(file),
        parent.open_entry(new).and_then(|entry| identity(&entry)),
    ) else {
        return false;
    };
    if renamed.platform_id != found.platform_id {
        return false;
    }
    let Ok(listing) = fs::read_dir(parent.anchored_path()) else {
        return false;
    };
    let (mut new_listed, mut old_listed) = (false, false);
    for item in listing.flatten() {
        let listed = item.file_name();
        new_listed |= listed.to_str() == Some(new.as_str());
        old_listed |= listed.to_str() == Some(old.as_str());
    }
    new_listed && !old_listed
}

// ---- copy and move ----

struct Job {
    root: PathBuf,
    entry: GalleryEntryRef,
    destination: GalleryLocation,
    mode: TransferMode,
    operation: String,
    faults: Faults,
}

struct Placed {
    location: GalleryLocation,
    /// A moved source that could not be fully removed after the copy was published.
    retained: bool,
}

/// One source file or directory that was copied, as seen when it was read.
struct Record {
    /// From the moved entry's own name down.
    path: Vec<SafeFileName>,
    directory: bool,
    identity: FileIdentity,
}

fn place(job: Job) -> Result<Placed> {
    let Job {
        root,
        entry,
        destination,
        mode,
        operation,
        faults,
    } = job;
    if entry.location.is_root() || entry.location.contains(&destination) {
        return Err(GalleryErrorCode::InvalidRequest.into());
    }
    let (source_parent, name) = resolve_parent(&root, &entry.location, true)?;
    let handle = current_entry(&source_parent, &name, &entry.revision)?;
    let is_directory = handle.metadata().map_err(err)?.is_dir();
    let target_dir = resolve_directory(&root, &destination, true)?;
    if mode == TransferMode::Move && !faults.force_cross_device {
        match publish(&source_parent, &name, &handle, &target_dir, &name) {
            Ok(target) => {
                return Ok(Placed {
                    location: destination
                        .join(target)
                        .map_err(|_| GalleryErrorCode::InvalidName)?,
                    retained: false,
                });
            }
            Err(e) if is_cross_volume(&e) => {}
            Err(e) => return Err(err(e)),
        }
    }
    // The entry handle allows deletion, which the read pin of the copy must not coexist with.
    drop(handle);
    Transfer {
        root,
        source_parent,
        name,
        is_directory,
        revision: entry.revision.0,
        target_dir,
        destination,
        mode,
        operation,
        faults,
    }
    .staged()
}

/// Renames without replacing. A taken name is retried as `name (1)`, `name (2)`, ... so nothing
/// is ever merged or overwritten.
fn publish(
    source: &PinnedDirectory,
    name: &SafeFileName,
    file: &File,
    target: &PinnedDirectory,
    wanted: &SafeFileName,
) -> io::Result<SafeFileName> {
    for attempt in 0..=MAX_SUFFIX {
        let candidate = if attempt == 0 {
            wanted.clone()
        } else {
            wanted
                .with_suffix(attempt)
                .map_err(|_| io::Error::from(io::ErrorKind::InvalidFilename))?
        };
        match rename_no_replace(source, name, file, target, &candidate) {
            Ok(()) => return Ok(candidate),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Err(io::ErrorKind::AlreadyExists.into())
}

struct Transfer {
    root: PathBuf,
    source_parent: Arc<PinnedDirectory>,
    name: SafeFileName,
    is_directory: bool,
    /// The revision the caller saw.
    revision: String,
    target_dir: Arc<PinnedDirectory>,
    destination: GalleryLocation,
    mode: TransferMode,
    operation: String,
    faults: Faults,
}

impl Transfer {
    /// Copy into the destination area's staging folder, publish, then (for a move) remove what
    /// was copied from the source.
    fn staged(mut self) -> Result<Placed> {
        if self.name.as_str().eq_ignore_ascii_case(OWNER) {
            return Err(GalleryErrorCode::Unsupported.into());
        }
        let operation = SafeFileName::new(self.operation.as_str())
            .map_err(|_| GalleryErrorCode::InvalidRequest)?;
        let area = area_root(&self.root, self.destination.area(), true)?;
        let staging = area.child(&constant(STAGING_DIR), true).map_err(err)?;
        let directory = staging.create_new_child(&operation).map_err(|e| {
            if e.kind() == io::ErrorKind::AlreadyExists {
                GalleryError(GalleryErrorCode::Io)
            } else {
                err(e)
            }
        })?;
        let staged = self.copy_and_publish(&directory, &operation);
        drop(directory);
        // After publication only the marker is left; after a failure, a partial copy. Both are
        // owned by this operation. A leftover keeps its marker and is settled by the next
        // staging cleanup.
        let _ = remove_operation(&staging, &operation);
        let (target, records) = staged?;
        let location = self
            .destination
            .join(target)
            .map_err(|_| GalleryErrorCode::InvalidName)?;
        if self.mode == TransferMode::Copy {
            return Ok(Placed {
                location,
                retained: false,
            });
        }
        if let Some(hook) = self.faults.after_publish.take() {
            hook();
        }
        let retained = self.faults.fail_after_publish
            || remove_moved_source(
                &self.source_parent,
                &records,
                &mut self.faults.fail_during_cleanup,
            );
        Ok(Placed { location, retained })
    }

    fn copy_and_publish(
        &self,
        directory: &Arc<PinnedDirectory>,
        operation: &SafeFileName,
    ) -> Result<(SafeFileName, Vec<Record>)> {
        let mut marker = directory.open(&constant(OWNER), true, true).map_err(err)?;
        marker
            .write_all(format!("{OWNER_PREFIX}{}", operation.as_str()).as_bytes())
            .and_then(|()| marker.sync_all())
            .map_err(err)?;
        drop(marker);
        // A rename cannot cross filesystems, so the staged copy must already be on the
        // destination's filesystem.
        if volume(&directory.file).map_err(err)? != volume(&self.target_dir.file).map_err(err)? {
            return Err(GalleryErrorCode::Unsupported.into());
        }
        let mut records = Vec::new();
        if self.is_directory {
            copy_directory(
                &self.source_parent,
                directory,
                &self.name,
                &[],
                &mut records,
            )?;
        } else {
            copy_file(
                &self.source_parent,
                directory,
                &self.name,
                &[],
                &mut records,
            )?;
        }
        // What was copied must be the entry the caller saw.
        if records
            .first()
            .map(|record| revision_of(&record.identity))
            .as_deref()
            != Some(self.revision.as_str())
        {
            return Err(GalleryErrorCode::Stale.into());
        }
        if self.faults.fail_before_publish {
            return Err(GalleryErrorCode::Io.into());
        }
        let staged = directory.open_entry(&self.name).map_err(err)?;
        let target =
            publish(directory, &self.name, &staged, &self.target_dir, &self.name).map_err(err)?;
        Ok((target, records))
    }
}

fn copy_file(
    source_parent: &PinnedDirectory,
    target_parent: &PinnedDirectory,
    name: &SafeFileName,
    parents: &[SafeFileName],
    records: &mut Vec<Record>,
) -> Result<()> {
    let mut source = source_parent.open_read_pin(name).map_err(tree_error)?;
    let before = identity(&source).map_err(err)?;
    let mut target = target_parent.open(name, true, true).map_err(err)?;
    io::copy(&mut source, &mut target).map_err(err)?;
    #[cfg(unix)]
    copy_mode(&source, &target, 0)?;
    target.sync_all().map_err(err)?;
    let copied = identity(&target).map_err(err)?;
    if copied.size != before.size
        || digest(&mut source).map_err(err)? != digest(&mut target).map_err(err)?
    {
        return Err(GalleryErrorCode::Io.into());
    }
    if identity(&source).map_err(err)? != before {
        return Err(GalleryErrorCode::Stale.into());
    }
    let mut path = parents.to_vec();
    path.push(name.clone());
    records.push(Record {
        path,
        directory: false,
        identity: before,
    });
    Ok(())
}

/// Recursive copy with non-following traversal. Any link, special file or unsafe name aborts the
/// whole item, so an incomplete tree is never published.
fn copy_directory(
    source_parent: &Arc<PinnedDirectory>,
    target_parent: &Arc<PinnedDirectory>,
    name: &SafeFileName,
    parents: &[SafeFileName],
    records: &mut Vec<Record>,
) -> Result<()> {
    // The staging directory sits above the copy, so one level less than `MAX_DEPTH` keeps every
    // staged tree within what `remove_owned_tree` can walk.
    if parents.len() >= MAX_DEPTH - 1 {
        return Err(GalleryErrorCode::Unsupported.into());
    }
    let source = source_parent.child(name, false).map_err(tree_error)?;
    let target = target_parent.create_new_child(name).map_err(err)?;
    let mut path = parents.to_vec();
    path.push(name.clone());
    records.push(Record {
        path: path.clone(),
        directory: true,
        identity: identity(&source.file).map_err(err)?,
    });
    for item in fs::read_dir(source.anchored_path()).map_err(err)? {
        let item = item.map_err(err)?;
        let child = item
            .file_name()
            .to_str()
            .and_then(|text| SafeFileName::new(text).ok())
            .ok_or(GalleryErrorCode::Unsupported)?;
        let is_directory = source
            .open_entry(&child)
            .and_then(|entry| entry.metadata())
            .map_err(tree_error)?
            .is_dir();
        if is_directory {
            copy_directory(&source, &target, &child, &path, records)?;
        } else {
            copy_file(&source, &target, &child, &path, records)?;
        }
    }
    // Applied once the contents are in place: a folder the owner cannot write could not be filled.
    // The owner always keeps full access to the copy, so the staged tree can still be published
    // (renaming a folder to a new parent needs write access) and removed.
    #[cfg(unix)]
    {
        copy_mode(&source.file, &target.file, 0o700)?;
        target.file.sync_all().map_err(err)?;
    }
    Ok(())
}

/// Gives a copy the permission bits of its source: the staged handles are created with `0600`
/// (files) and `0755` (folders) whatever the source had. `keep` bits are always set.
#[cfg(unix)]
fn copy_mode(source: &File, target: &File, keep: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = (source.metadata().map_err(err)?.permissions().mode() & 0o777) | keep;
    target
        .set_permissions(fs::Permissions::from_mode(mode))
        .map_err(err)
}

/// Removes from the source only what was copied and is still exactly as it was copied: files
/// whose identity matches, then the folders left empty, deepest first. Anything changed, new or
/// refused stays. Returns whether anything was left behind. `fail_first` fails one removal.
fn remove_moved_source(
    top: &Arc<PinnedDirectory>,
    records: &[Record],
    fail_first: &mut bool,
) -> bool {
    let mut retained = false;
    let mut directories: Vec<&Record> = Vec::new();
    for record in records {
        if record.directory {
            directories.push(record);
        } else if std::mem::take(fail_first) || remove_recorded_file(top, record).is_err() {
            retained = true;
        }
    }
    directories.sort_by_key(|record| std::cmp::Reverse(record.path.len()));
    for record in directories {
        if std::mem::take(fail_first) || remove_recorded_directory(top, record).is_err() {
            retained = true;
        }
    }
    retained
}

/// The parent of a recorded entry, below the directory that holds the moved entry.
fn recorded_parent(
    top: &Arc<PinnedDirectory>,
    record: &Record,
) -> io::Result<Arc<PinnedDirectory>> {
    let mut directory = top.clone();
    for part in &record.path[..record.path.len() - 1] {
        directory = directory.child(part, false)?;
    }
    Ok(directory)
}
fn remove_recorded_file(top: &Arc<PinnedDirectory>, record: &Record) -> io::Result<()> {
    let name = record.path.last().expect("a record names an entry");
    let parent = match recorded_parent(top, record) {
        Ok(parent) => parent,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    let current = match parent.open_entry(name) {
        Ok(current) => current,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    // The whole recorded identity must still hold, then the removal is by handle and platform
    // identity: a file swapped in after this check is refused, never deleted by name.
    if identity(&current)? != record.identity {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    drop(current);
    match parent.remove_checked_entry(name, &record.identity.platform_id) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}
fn remove_recorded_directory(top: &Arc<PinnedDirectory>, record: &Record) -> io::Result<()> {
    let name = record.path.last().expect("a record names an entry");
    let parent = match recorded_parent(top, record) {
        Ok(parent) => parent,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    // Only an empty folder of the same identity can go; the OS refuses anything else.
    match parent.remove_checked_entry(name, &record.identity.platform_id) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

// ---- delete ----

struct Described {
    kind: GalleryEntryKind,
    bytes: u64,
    fingerprint: String,
    identity: FileIdentity,
}

/// What a recycle would take, as of now. A file's fingerprint is its revision; a directory's
/// covers every descendant, links included but never followed.
fn describe(parent: &Arc<PinnedDirectory>, name: &SafeFileName) -> Result<Described> {
    let entry = parent.open_entry(name).map_err(err)?;
    let meta = entry.metadata().map_err(err)?;
    let id = identity(&entry).map_err(err)?;
    drop(entry);
    if !meta.is_dir() {
        return Ok(Described {
            kind: GalleryEntryKind::File,
            bytes: meta.len(),
            fingerprint: revision_of(&id),
            identity: id,
        });
    }
    let directory = parent.child(name, false).map_err(tree_error)?;
    let mut walk = Walk::default();
    walk.visit(&directory, "", 0)?;
    walk.lines.sort();
    let mut hasher = Sha256::new();
    for line in &walk.lines {
        hasher.update(line.as_bytes());
        hasher.update(b"\n");
    }
    Ok(Described {
        kind: GalleryEntryKind::Directory,
        bytes: walk.bytes,
        fingerprint: hex(&hasher.finalize()),
        identity: id,
    })
}

#[derive(Default)]
struct Walk {
    bytes: u64,
    /// One `path NUL kind NUL identity` line per descendant.
    lines: Vec<String>,
}
impl Walk {
    fn visit(
        &mut self,
        directory: &Arc<PinnedDirectory>,
        prefix: &str,
        depth: usize,
    ) -> Result<()> {
        if depth >= MAX_DEPTH {
            return Err(GalleryErrorCode::Unsupported.into());
        }
        for item in fs::read_dir(directory.anchored_path()).map_err(err)? {
            let item = item.map_err(err)?;
            let raw = item.file_name();
            let path = format!("{prefix}{}", raw.to_string_lossy());
            let listed = item.metadata().map_err(tree_error)?;
            let stamp = |meta: &fs::Metadata| {
                format!(
                    "{}:{}",
                    meta.len(),
                    meta.modified()
                        .ok()
                        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                        .map_or(0, |since| since.as_nanos())
                )
            };
            if is_link(&listed) || (!listed.is_file() && !listed.is_dir()) {
                // Recorded by what the directory entry says; never opened or followed.
                self.lines
                    .push(format!("{path}\0other\0{}", stamp(&listed)));
                continue;
            }
            let Some(name) = raw.to_str().and_then(|text| SafeFileName::new(text).ok()) else {
                if listed.is_dir() {
                    return Err(GalleryErrorCode::Unsupported.into());
                }
                self.bytes = self.bytes.saturating_add(listed.len());
                self.lines.push(format!("{path}\0file\0{}", stamp(&listed)));
                continue;
            };
            if listed.is_dir() {
                let child = directory.child(&name, false).map_err(tree_error)?;
                let id = identity(&child.file).map_err(err)?;
                self.lines
                    .push(format!("{path}\0dir\0{}", revision_of(&id)));
                self.visit(&child, &format!("{path}/"), depth + 1)?;
            } else {
                let file = directory.open_entry(&name).map_err(tree_error)?;
                let id = identity(&file).map_err(err)?;
                self.bytes = self.bytes.saturating_add(id.size);
                self.lines
                    .push(format!("{path}\0file\0{}", revision_of(&id)));
            }
        }
        Ok(())
    }
}

// ---- staging cleanup ----

/// Removes the operation directories of both areas that carry a valid ownership marker and
/// reports every other entry. The staging folder itself goes when it ends up empty.
fn clean_staging(root: &Path) -> Vec<GalleryWarning> {
    let mut warnings = Vec::new();
    for area in [GalleryArea::Input, GalleryArea::Output] {
        let Ok(area_directory) = area_root(root, area, false) else {
            continue;
        };
        let staging_name = constant(STAGING_DIR);
        let Ok(staging) = area_directory.child(&staging_name, false) else {
            continue;
        };
        if let Ok(children) = fs::read_dir(staging.anchored_path()) {
            for child in children.flatten() {
                let raw = child.file_name();
                let leftover = GalleryWarning {
                    name: Some(raw.to_string_lossy().into_owned()),
                    code: GalleryWarningCode::StagingLeftover,
                };
                let removed = raw
                    .to_str()
                    .and_then(|text| SafeFileName::new(text).ok())
                    .is_some_and(|name| {
                        if owns_operation(&staging, &name) {
                            remove_operation(&staging, &name).is_ok()
                        } else {
                            remove_empty_directory(&staging, &name).is_ok()
                        }
                    });
                if !removed {
                    warnings.push(leftover);
                }
            }
        }
        let platform_id = identity(&staging.file).map(|id| id.platform_id);
        drop(staging);
        if let Ok(platform_id) = platform_id {
            // An empty folder only; the OS refuses anything that still holds entries.
            let _ = area_directory.remove_checked_entry(&staging_name, &platform_id);
        }
    }
    warnings
}

/// Removes an operation directory so that a leftover can always be recognized again: what the
/// operation staged first, then the `.owner` marker, then the emptied directory itself. Stops at
/// the first refusal, so the marker survives for as long as anything it vouches for does.
fn remove_operation(staging: &Arc<PinnedDirectory>, name: &SafeFileName) -> io::Result<()> {
    let directory = staging.child(name, false)?;
    let marker = constant(OWNER);
    let mut staged = Vec::new();
    for item in fs::read_dir(directory.anchored_path())? {
        let child = item?
            .file_name()
            .into_string()
            .ok()
            .and_then(|text| SafeFileName::new(text).ok())
            .ok_or(io::ErrorKind::PermissionDenied)?;
        if child != marker {
            staged.push(child);
        }
    }
    for child in &staged {
        directory.remove_owned_tree(child)?;
    }
    // A marker that was never written leaves nothing to keep.
    match directory.remove_owned_tree(&marker) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        other => other?,
    }
    let platform_id = identity(&directory.file)?.platform_id;
    drop(directory);
    staging.remove_checked_entry(name, &platform_id)
}

/// Removes a directory that holds nothing; the OS refuses one that does. Files, links and
/// special entries are never removed here.
fn remove_empty_directory(staging: &Arc<PinnedDirectory>, name: &SafeFileName) -> io::Result<()> {
    let entry = staging.open_entry(name)?;
    if !entry.metadata()?.is_dir() {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    let platform_id = identity(&entry)?.platform_id;
    drop(entry);
    staging.remove_checked_entry(name, &platform_id)
}

/// A real directory whose `.owner` is a regular file naming exactly this operation.
fn owns_operation(staging: &Arc<PinnedDirectory>, name: &SafeFileName) -> bool {
    let Ok(directory) = staging.child(name, false) else {
        return false;
    };
    let Ok(marker) = directory.open(&constant(OWNER), false, false) else {
        return false;
    };
    let expected = format!("{OWNER_PREFIX}{}", name.as_str());
    let mut found = Vec::new();
    marker
        .take(expected.len() as u64 + 1)
        .read_to_end(&mut found)
        .is_ok()
        && found == expected.as_bytes()
}
