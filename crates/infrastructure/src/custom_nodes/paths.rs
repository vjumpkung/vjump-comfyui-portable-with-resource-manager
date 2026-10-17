//! Retained, non-following parents for every custom-node filesystem operation.
use crate::shared::model_paths::{self, PinnedDirectory};
use application::{
    custom_nodes::{NodeAppError, NodeLocationGuard, TreeSnapshot},
    environment::Retained,
};
use domain::{
    custom_nodes::{InstalledNode, NodeId},
    shared::model_values::SafeFileName,
};
use std::{
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

pub(crate) type Entry = (Arc<PinnedDirectory>, SafeFileName);

pub(crate) struct NodePaths {
    pub(crate) root: Arc<PinnedDirectory>,
    _comfy: Arc<PinnedDirectory>,
    pub(crate) cn: Arc<PinnedDirectory>,
    pub(crate) disabled: Arc<PinnedDirectory>,
    hooks: Arc<PinnedDirectory>,
}

pub(crate) fn filesystem(error: io::Error) -> NodeAppError {
    if error.kind() == io::ErrorKind::NotADirectory
        || (error.kind() == io::ErrorKind::PermissionDenied && error.raw_os_error().is_none())
        || is_link_error(&error)
    {
        NodeAppError::UnsafeSource
    } else {
        NodeAppError::Filesystem
    }
}

fn is_link_error(error: &io::Error) -> bool {
    #[cfg(unix)]
    {
        error.raw_os_error() == Some(libc::ELOOP)
    }
    #[cfg(windows)]
    {
        let _ = error;
        false
    }
}

pub(crate) fn safe_name(name: impl Into<String>) -> SafeFileName {
    SafeFileName::new(name).expect("app-owned component is a safe filename")
}

pub(crate) fn entry_path(entry: &Entry) -> PathBuf {
    entry.0.anchored_path().join(entry.1.as_str())
}

pub(crate) fn exists(entry: &Entry) -> Result<bool, NodeAppError> {
    match entry.0.open_entry(&entry.1) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(filesystem(e)),
    }
}

pub(crate) fn rename(from: &Entry, to: &Entry) -> Result<(), NodeAppError> {
    let file = from.0.open_entry(&from.1).map_err(filesystem)?;
    model_paths::rename_no_replace(&from.0, &from.1, &file, &to.0, &to.1).map_err(|e| {
        if e.kind() == io::ErrorKind::AlreadyExists {
            NodeAppError::TargetExists
        } else {
            filesystem(e)
        }
    })
}

pub(crate) fn remove(entry: &Entry) -> Result<(), NodeAppError> {
    match entry.0.remove_owned_tree(&entry.1) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(filesystem(e)),
    }
}

impl NodePaths {
    pub(crate) fn open(root: &Path, create: bool) -> Result<Self, NodeAppError> {
        let root = model_paths::pin_absolute(root).map_err(filesystem)?;
        let comfy = root
            .child(&safe_name("ComfyUI"), false)
            .map_err(filesystem)?;
        let cn = comfy
            .child(&safe_name("custom_nodes"), create)
            .map_err(filesystem)?;
        let disabled = cn
            .child(&safe_name(".disabled"), create)
            .map_err(filesystem)?;
        let hooks = disabled
            .child(&safe_name(".vjump-no-hooks"), create)
            .map_err(filesystem)?;
        if fs::read_dir(hooks.anchored_path())
            .map_err(filesystem)?
            .next()
            .is_some()
        {
            return Err(NodeAppError::UnsafeSource);
        }
        let paths = Self {
            root,
            _comfy: comfy,
            cn,
            disabled,
            hooks,
        };
        #[cfg(test)]
        AFTER_OPEN.with(|boundary| {
            if let Some(boundary) = boundary.borrow_mut().take() {
                boundary();
            }
        });
        Ok(paths)
    }

    pub(crate) fn location(&self, node: &InstalledNode) -> Entry {
        self.named_location(&node.name, node.enabled)
    }

    pub(crate) fn named_location(&self, name: &SafeFileName, enabled: bool) -> Entry {
        (
            if enabled {
                self.cn.clone()
            } else {
                self.disabled.clone()
            },
            name.clone(),
        )
    }

    pub(crate) fn staging(&self, operation: &NodeId) -> Entry {
        (
            self.disabled.clone(),
            safe_name(format!(".vjump-staging-{}", operation.as_str())),
        )
    }

    pub(crate) fn staging_zip(&self, operation: &NodeId) -> Entry {
        (
            self.disabled.clone(),
            safe_name(format!(".vjump-staging-{}.zip", operation.as_str())),
        )
    }

    pub(crate) fn staging_hoist(&self, operation: &NodeId) -> Entry {
        (
            self.disabled.clone(),
            safe_name(format!(".vjump-staging-{}-hoist", operation.as_str())),
        )
    }

    pub(crate) fn backup(&self, operation: &NodeId) -> Entry {
        (
            self.disabled.clone(),
            safe_name(format!(".vjump-backup-{}", operation.as_str())),
        )
    }

    pub(crate) fn hooks(&self) -> PathBuf {
        self.hooks.anchored_path()
    }

    pub(crate) fn discard(&self, operation: &NodeId) -> Result<(), NodeAppError> {
        for entry in [
            self.staging(operation),
            self.staging_zip(operation),
            self.staging_hoist(operation),
        ] {
            remove(&entry)?;
        }
        Ok(())
    }
}

/// A root input as read: its bytes (`None` above the size limit) and the handle that pins it.
type Input = (Option<Vec<u8>>, File);

/// Reads one root input of a node through a non-following read pin. A missing file is `None`; a
/// link or special file is refused. The bytes are `None` above `limit`. The handle is returned
/// so the pin outlives the read.
pub(crate) fn read_input(
    directory: &PinnedDirectory,
    name: &str,
    limit: u64,
) -> Result<Option<Input>, NodeAppError> {
    let mut file = match directory.open_read_pin(&safe_name(name)) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(filesystem(e)),
    };
    let mut bytes = Vec::new();
    (&mut file)
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(filesystem)?;
    let bytes = (bytes.len() as u64 <= limit).then_some(bytes);
    Ok(Some((bytes, file)))
}

/// Everything a guarded location keeps pinned. Surviving child processes share ownership of it
/// (through `Retained`), so it is released only once the last of them has exited.
struct Resources {
    _paths: Arc<NodePaths>,
    _directory: Arc<PinnedDirectory>,
    _inputs: Vec<File>,
}

/// A pinned node directory with snapshots of its root requirements and install script.
pub(crate) struct PinnedLocation {
    directory: PathBuf,
    requirements: Option<Arc<[u8]>>,
    script: Option<Arc<[u8]>>,
    snapshot: TreeSnapshot,
    resources: Arc<Resources>,
}

impl PinnedLocation {
    pub(crate) fn new(
        paths: Arc<NodePaths>,
        directory: Arc<PinnedDirectory>,
        inputs: Vec<File>,
        requirements: Option<Vec<u8>>,
        script: Option<Vec<u8>>,
        snapshot: TreeSnapshot,
    ) -> Self {
        Self {
            // Windows: the pinned path. Linux: `/proc/<pid>/fd/<fd>/.`, which keeps naming this
            // directory even when an ancestor is renamed or replaced.
            directory: directory.anchored_path(),
            requirements: requirements.map(Arc::from),
            script: script.map(Arc::from),
            snapshot,
            resources: Arc::new(Resources {
                _paths: paths,
                _directory: directory,
                _inputs: inputs,
            }),
        }
    }
}

impl NodeLocationGuard for PinnedLocation {
    fn directory(&self) -> &Path {
        &self.directory
    }
    fn requirements(&self) -> Option<Arc<[u8]>> {
        self.requirements.clone()
    }
    fn install_script(&self) -> Option<Arc<[u8]>> {
        self.script.clone()
    }
    fn snapshot(&self) -> &TreeSnapshot {
        &self.snapshot
    }
    fn retained(&self) -> Retained {
        Retained::new(self.resources.clone())
    }
}

#[cfg(test)]
thread_local! {
    static AFTER_OPEN: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn after_open(boundary: impl FnOnce() + 'static) {
    AFTER_OPEN.with(|slot| {
        assert!(slot.borrow().is_none());
        *slot.borrow_mut() = Some(Box::new(boundary));
    });
}
