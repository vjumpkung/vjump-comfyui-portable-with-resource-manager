//! Filesystem adapter for node installation. All paths derive from the install root plus validated
//! names and operation IDs; staging and backups live under `custom_nodes/.disabled/`, which ComfyUI
//! never loads. Mutations are no-replace renames inside that tree, never unchecked deletes.
use super::{
    git::{GitNodes, RefKind, Resolved, refused},
    paths::{
        Entry, NodePaths, PinnedLocation, exists as entry_exists, filesystem, read_input, remove,
        rename, safe_name,
    },
    provenance::{
        MARKER, TRACKING, read_manager_registry, read_marker, repository_folder, safe_folder,
    },
    registry::RegistryHttp,
    scan,
    tree::{self, is_link, read_text, sha256_file, validate_tree},
};
use crate::{
    custom_nodes::compat::evaluate,
    shared::{
        archive::ExtractLimits,
        download::sha256_hex,
        model_paths::{PinnedDirectory, identity},
    },
};
use application::{
    custom_nodes::*,
    environment::{Retained, StepContext},
};
use async_trait::async_trait;
use domain::{
    custom_nodes::*, models::acquisition::FileIdentity, shared::model_values::SafeFileName,
};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

/// Upper bounds for the root inputs a guarded location snapshots.
pub(super) const REQUIREMENTS_LIMIT: u64 = 256 * 1024;
const SCRIPT_LIMIT: u64 = 16 << 20;

pub type Recycler =
    Arc<dyn Fn(&PinnedDirectory, &SafeFileName, &FileIdentity) -> io::Result<()> + Send + Sync>;

pub struct LocalNodeFiles {
    pub registry: Arc<RegistryHttp>,
    pub git: Arc<GitNodes>,
    pub recycler: Recycler,
    pub limits: ExtractLimits,
}

/// NFR-SEC-4 limits for node archives: 20,000 entries, 2 GiB, 100x expansion.
pub fn node_limits() -> ExtractLimits {
    ExtractLimits {
        max_entries: 20_000,
        max_total_bytes: 2 << 30,
        max_ratio: 100,
    }
}

fn not_link(path: &Path) -> Result<(), NodeAppError> {
    match path.symlink_metadata() {
        Ok(meta) if is_link(&meta) => Err(NodeAppError::UnsafeSource),
        Ok(_) => Ok(()),
        Err(_) => Err(NodeAppError::NotFound),
    }
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, NodeAppError> + Send + 'static,
) -> Result<T, NodeAppError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| NodeAppError::Filesystem)?
}

fn parse_requirement_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .take(500)
        .map(|l| l.chars().take(300).collect())
        .collect()
}

fn remove_owned(entry: &Entry, expected: &FileIdentity) -> Result<(), NodeAppError> {
    let current = entry.0.open_entry(&entry.1).map_err(filesystem)?;
    if identity(&current).map_err(filesystem)?.platform_id != expected.platform_id {
        return Err(NodeAppError::UnsafeSource);
    }
    drop(current);
    remove(entry)
}

pub(crate) fn create_staging(entry: &Entry) -> Result<Arc<PinnedDirectory>, NodeAppError> {
    if entry_exists(entry)? {
        return Err(NodeAppError::UnsafeSource);
    }
    #[cfg(test)]
    STAGING_BOUNDARY.with(|slot| {
        if let Some(boundary) = slot.borrow_mut().take() {
            boundary();
        }
    });
    entry.0.create_new_child(&entry.1).map_err(|e| {
        if e.kind() == io::ErrorKind::AlreadyExists {
            NodeAppError::UnsafeSource
        } else {
            filesystem(e)
        }
    })
}

#[cfg(test)]
thread_local! {
    static REGISTRY_BOUNDARY: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
    static GIT_APPLY_BOUNDARY: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
    static STAGING_BOUNDARY: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn before_staging_create(boundary: impl FnOnce() + 'static) {
    STAGING_BOUNDARY.with(|slot| *slot.borrow_mut() = Some(Box::new(boundary)));
}

#[cfg(test)]
pub(crate) fn after_git_apply(boundary: impl FnOnce() + 'static) {
    GIT_APPLY_BOUNDARY.with(|slot| {
        assert!(slot.borrow().is_none());
        *slot.borrow_mut() = Some(Box::new(boundary));
    });
}

#[cfg(test)]
fn git_apply_boundary() {
    GIT_APPLY_BOUNDARY.with(|slot| {
        if let Some(boundary) = slot.borrow_mut().take() {
            boundary();
        }
    });
}

#[cfg(test)]
pub(crate) fn after_archive_open(boundary: impl FnOnce() + 'static) {
    REGISTRY_BOUNDARY.with(|slot| {
        assert!(slot.borrow().is_none());
        *slot.borrow_mut() = Some(Box::new(boundary));
    });
}

#[cfg(test)]
fn registry_boundary() {
    REGISTRY_BOUNDARY.with(|slot| {
        if let Some(boundary) = slot.borrow_mut().take() {
            boundary();
        }
    });
}

/// Exact-release compatibility with flags from that release or the top-level node.
fn review_warnings(mut release: Compatibility, node: &RegistryNode) -> Vec<String> {
    release.flagged |= node.node_flagged;
    release.deprecated |= node.node_deprecated;
    release.warnings()
}

/// The tree digest covers every file, tracked or not, except the app's own files, the top-level
/// `.git` and `__pycache__`; a Git node's exact commit is carried by its source instead.
fn read_tree(dir: &Arc<PinnedDirectory>, git_node: bool) -> Result<TreeSnapshot, NodeAppError> {
    let digest = if git_node {
        tree::tree_digest(dir)?
    } else {
        tree::digest_tree(dir, false)?
    };
    let requirements = read_input(dir, &safe_name("requirements.txt"), REQUIREMENTS_LIMIT)?
        .and_then(|(bytes, _)| bytes)
        .and_then(|bytes| String::from_utf8(bytes).ok());
    let hook = match sha256_file(dir, &safe_name("install.py")) {
        Ok(hash) => Some(hash),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(filesystem(e)),
    };
    Ok(TreeSnapshot {
        digest,
        requirements_digest: requirements.as_ref().map(|t| sha256_hex(t.as_bytes())),
        hook_digest: hook,
        requirement_lines: requirements
            .as_deref()
            .map(parse_requirement_lines)
            .unwrap_or_default(),
    })
}

/// Moves the only top-level directory of an extracted archive up one level, so GitHub-style
/// `repo-main/...` archives and flat registry archives install the same way.
fn hoist_single_root(paths: &NodePaths, op: &NodeId) -> Result<(), NodeAppError> {
    let staging = paths.staging(op);
    let directory = staging.0.child(&staging.1, false).map_err(filesystem)?;
    let entries = fs::read_dir(directory.anchored_path())
        .map_err(filesystem)?
        .collect::<io::Result<Vec<_>>>()
        .map_err(filesystem)?;
    let [only] = entries.as_slice() else {
        return Ok(());
    };
    let name = only
        .file_name()
        .into_string()
        .ok()
        .and_then(|n| SafeFileName::new(n).ok())
        .ok_or(NodeAppError::UnsafeSource)?;
    let file = directory.open_entry(&name).map_err(filesystem)?;
    if !file.metadata().map_err(filesystem)?.is_dir() {
        return Ok(());
    }
    let owned = identity(&file).map_err(filesystem)?;
    let hoist = paths.staging_hoist(op);
    rename(&(directory.clone(), name), &hoist)?;
    drop(file);
    let hoisted = hoist.0.child(&hoist.1, false).map_err(filesystem)?;
    if identity(&hoisted.file).map_err(filesystem)?.platform_id != owned.platform_id {
        return Err(NodeAppError::UnsafeSource);
    }
    for child in fs::read_dir(hoisted.anchored_path()).map_err(filesystem)? {
        let name = child
            .map_err(filesystem)?
            .file_name()
            .into_string()
            .ok()
            .and_then(|n| SafeFileName::new(n).ok())
            .ok_or(NodeAppError::UnsafeSource)?;
        rename(&(hoisted.clone(), name.clone()), &(directory.clone(), name))?;
    }
    drop(hoisted);
    remove_owned(&hoist, &owned)
}

fn write_marker(
    directory: &PinnedDirectory,
    name: &str,
    source: &NodeSource,
    digest: Option<&str>,
) -> Result<(), NodeAppError> {
    let document = serde_json::json!({
        "schema": 1, "name": name, "source": super::sqlite::source_to_json(source), "artifact_digest": digest,
    });
    let mut file = directory
        .open(&safe_name(MARKER), true, true)
        .map_err(filesystem)?;
    file.write_all(&serde_json::to_vec_pretty(&document).map_err(|_| NodeAppError::Filesystem)?)
        .and_then(|()| file.sync_all())
        .map_err(filesystem)
}

fn write_tracking(directory: &Arc<PinnedDirectory>) -> Result<(), NodeAppError> {
    let mut lines = Vec::new();
    tree::walk_files(directory, false, false, |_, _, relative, _| {
        if relative != MARKER && relative != TRACKING {
            lines.push(relative.to_owned());
        }
        Ok(())
    })?;
    let mut file = directory
        .open(&safe_name(TRACKING), true, true)
        .map_err(filesystem)?;
    file.write_all((lines.join("\n") + "\n").as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(filesystem)
}

fn recycle_backup(paths: &NodePaths, op: &NodeId, recycler: &Recycler) -> Result<(), NodeAppError> {
    let backup = paths.backup(op);
    if !entry_exists(&backup)? {
        return Ok(());
    }
    let file = backup.0.open_entry(&backup.1).map_err(filesystem)?;
    let expected = identity(&file).map_err(filesystem)?;
    drop(file);
    recycler(&backup.0, &backup.1, &expected).map_err(filesystem)?;
    if entry_exists(&backup)? {
        return Err(NodeAppError::Filesystem);
    }
    Ok(())
}

impl LocalNodeFiles {
    async fn prepare_registry(
        &self,
        root: &Path,
        op: &NodeId,
        id: &str,
        version: Option<&str>,
        prior: Option<&InstalledNode>,
        ctx: &StepContext,
    ) -> Result<StagedNode, NodeAppError> {
        let paths = Arc::new(NodePaths::open(root, true)?);
        let name = match prior {
            Some(prior) => prior.name.clone(),
            None => safe_folder(id).ok_or(NodeAppError::InvalidRequest)?,
        };
        if prior.is_none()
            && (entry_exists(&paths.named_location(&name, true))?
                || entry_exists(&paths.named_location(&name, false))?)
        {
            return Err(NodeAppError::TargetExists);
        }
        ctx.log(format!("Resolving {id} in the Comfy registry"));
        let first = self.registry.release(id, version).await?;
        // "Latest" is resolved once; every later request names the exact version.
        let info = if version.is_none() {
            let pinned = self
                .registry
                .release(id, Some(&first.release.version))
                .await?;
            if pinned.release.version != first.release.version {
                return Err(NodeAppError::InvalidRegistry);
            }
            pinned
        } else {
            first
        };
        let exact = info.release.version.clone();
        if let Some(NodeSource::Registry { version: old, .. }) = prior.map(|p| &p.source)
            && *old == exact
        {
            return Err(NodeAppError::UpToDate);
        }
        let url = info
            .download_url
            .clone()
            .ok_or(NodeAppError::InvalidRegistry)?;
        // Node-level metadata is part of the review: a refusal or an unreadable body aborts the
        // preparation before anything is downloaded, with the registry's own error.
        let node = self.registry.node(id).await?;
        let source = NodeSource::Registry {
            id: id.to_owned(),
            publisher: node.publisher.clone(),
            version: exact.clone(),
            repository: node.repository.clone(),
        };
        ctx.log(format!("Downloading {id} {exact}"));
        let zip = paths.staging_zip(op);
        let staging = paths.staging(op);
        let hoist = paths.staging_hoist(op);
        // Refuse foreign entries before acquiring anything owned by this attempt.
        if entry_exists(&staging)? || entry_exists(&hoist)? {
            return Err(NodeAppError::UnsafeSource);
        }
        let mut archive = zip.0.open(&zip.1, true, true).map_err(|e| {
            if e.kind() == io::ErrorKind::AlreadyExists {
                NodeAppError::UnsafeSource
            } else {
                filesystem(e)
            }
        })?;
        let archive_identity = identity(&archive).map_err(filesystem)?;
        #[cfg(test)]
        registry_boundary();
        let result = async {
            let artifact = self
                .registry
                .download(&url, &mut archive, &ctx.cancel)
                .await?;
            let limits = self.limits;
            let (layout, operation, marker_source, marker_name, digest) = (
                paths.clone(),
                op.clone(),
                source.clone(),
                name.as_str().to_owned(),
                artifact.sha256.clone(),
            );
            let tree = blocking(move || {
                let staging = layout.staging(&operation);
                let directory = create_staging(&staging)?;
                let owned = identity(&directory.file).map_err(filesystem)?;
                let extracted = (|| {
                    tree::extract_zip(archive, &directory, limits)?;
                    hoist_single_root(&layout, &operation)?;
                    if entry_exists(&(directory.clone(), safe_name(MARKER)))? {
                        return Err(NodeAppError::UnsafeSource);
                    }
                    validate_tree(&directory, false)?;
                    remove(&(directory.clone(), safe_name(TRACKING)))?;
                    let snapshot = read_tree(&directory, false)?;
                    write_tracking(&directory)?;
                    write_marker(&directory, &marker_name, &marker_source, Some(&digest))?;
                    Ok(snapshot)
                })();
                drop(directory);
                if extracted.is_err() {
                    remove_owned(&staging, &owned)?;
                }
                extracted
            })
            .await?;
            Ok::<_, NodeAppError>((artifact, tree))
        }
        .await;
        // Refuse a substituted entry and preserve protected copies for recovery.
        remove_owned(&zip, &archive_identity)?;
        let (artifact, tree) = result?;
        let warnings = review_warnings(evaluate(&info.metadata, &self.registry.host()), &node);
        Ok(StagedNode {
            source,
            name,
            tree_digest: tree.digest,
            artifact_digest: Some(artifact.sha256),
            requirement_lines: tree.requirement_lines,
            requirements_digest: tree.requirements_digest,
            hook_digest: tree.hook_digest,
            dependencies: info.release.dependencies,
            warnings,
        })
    }

    async fn prepare_git_install(
        &self,
        root: &Path,
        op: &NodeId,
        url: &str,
        reference: Option<&str>,
        ctx: &StepContext,
    ) -> Result<StagedNode, NodeAppError> {
        let paths = Arc::new(NodePaths::open(root, true)?);
        let remote = self.git.validate(url)?;
        let name = repository_folder(&remote.identity)
            .or_else(|| {
                // A local test remote is named by its folder.
                Path::new(&remote.identity)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .and_then(|n| safe_folder(n.strip_suffix(".git").unwrap_or(n)))
            })
            .ok_or(NodeAppError::UnsafeSource)?;
        if entry_exists(&paths.named_location(&name, true))?
            || entry_exists(&paths.named_location(&name, false))?
        {
            return Err(NodeAppError::TargetExists);
        }
        let hooks = paths.hooks();
        let git = self
            .git
            .bound(paths.clone(), vec![], ctx.retained.clone())?;
        ctx.log("Resolving the repository reference");
        let resolved = git
            .resolve(
                &paths.root.anchored_path(),
                &hooks,
                &remote,
                reference,
                &ctx.cancel,
            )
            .await?;
        let staged = paths.staging(op);
        let directory = create_staging(&staged)?;
        let owned = identity(&directory.file).map_err(filesystem)?;
        let staging = directory.anchored_path();
        let git = self
            .git
            .bound(paths.clone(), vec![directory.clone()], ctx.retained.clone())?;
        ctx.log(format!("Cloning at commit {}", resolved.commit));
        let cloned = async {
            git.clone_checkout(
                &paths.root.anchored_path(),
                &hooks,
                &remote,
                &resolved,
                &staging,
                ctx,
            )
            .await?;
            let directory = directory.clone();
            let source = git_source(&remote.identity, &resolved);
            let (marker_source, marker_name) = (source.clone(), name.as_str().to_owned());
            let tree = blocking(move || {
                tree::validate_fresh_git(&directory)?;
                // Keep the app's marker out of `git status`.
                let git = directory
                    .child(&safe_name(".git"), false)
                    .map_err(filesystem)?;
                let info = git.child(&safe_name("info"), true).map_err(filesystem)?;
                let exclude = safe_name("exclude");
                let mut text = read_text(&info, &exclude, 1 << 20).unwrap_or_default();
                text.push_str(&format!("\n{MARKER}\n{TRACKING}\n"));
                let mut file = match info.open(&exclude, true, false) {
                    Ok(file) => file,
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {
                        info.open(&exclude, true, true).map_err(filesystem)?
                    }
                    Err(e) => return Err(filesystem(e)),
                };
                file.set_len(0)
                    .and_then(|()| file.write_all(text.as_bytes()))
                    .and_then(|()| file.sync_all())
                    .map_err(filesystem)?;
                write_marker(&directory, &marker_name, &marker_source, None)?;
                read_tree(&directory, true)
            })
            .await?;
            // The reviewed tree is exactly the checkout of the resolved commit.
            git.require_clean(&paths.root.anchored_path(), &hooks, &staging)
                .await?;
            Ok::<_, NodeAppError>((source, tree))
        }
        .await;
        let (source, tree) = match cloned {
            Ok(value) => value,
            Err(error) => {
                drop(directory);
                if !matches!(error, NodeAppError::UnsafeSource) {
                    remove_owned(&staged, &owned)?;
                }
                return Err(error);
            }
        };
        let mut warnings = Vec::new();
        if staging.join(".gitmodules").is_file() {
            warnings.push("This repository declares submodules; they are not initialized".into());
        }
        match resolved.kind {
            RefKind::Tag | RefKind::Commit => {
                warnings.push("This install is detached; it will not track a branch".into());
            }
            _ => {}
        }
        Ok(StagedNode {
            source,
            name,
            tree_digest: tree.digest,
            artifact_digest: None,
            requirement_lines: tree.requirement_lines,
            requirements_digest: tree.requirements_digest,
            hook_digest: tree.hook_digest,
            dependencies: Vec::new(),
            warnings,
        })
    }

    async fn prepare_git_update(
        &self,
        root: &Path,
        op: &NodeId,
        prior: &InstalledNode,
        reference: Option<&str>,
        ctx: &StepContext,
    ) -> Result<StagedNode, NodeAppError> {
        let paths = Arc::new(NodePaths::open(root, true)?);
        let live = paths.location(prior);
        let live_directory = live.0.child(&live.1, false).map_err(filesystem)?;
        let dir = live_directory.anchored_path();
        let NodeSource::Git {
            repository, branch, ..
        } = &prior.source
        else {
            return Err(NodeAppError::InvalidRequest);
        };
        let _git = {
            let (dir, live_directory) = (dir.clone(), live_directory.clone());
            blocking(move || {
                not_link(&dir)?;
                pin_git_dir(&live_directory)
            })
            .await?
        };
        let hooks = paths.hooks();
        let git = self.git.bound(
            paths.clone(),
            vec![live_directory.clone()],
            ctx.retained.clone(),
        )?;
        ctx.log("Fetching and freezing the update target");
        let target = git
            .freeze_update(&paths.root.anchored_path(), &hooks, &dir, reference, ctx)
            .await?;
        // The reviewed tree comes from the repository the node was installed from.
        let remote = git
            .origin(&paths.root.anchored_path(), &hooks, &dir)
            .await?;
        if remote.identity != *repository {
            return Err(NodeAppError::UnsafeSource);
        }
        // The live update stays an in-place fast-forward; this complete checkout of the frozen
        // target is the evidence that is reviewed and later compared with the live result.
        let staged = paths.staging(op);
        let directory = create_staging(&staged)?;
        let owned = identity(&directory.file).map_err(filesystem)?;
        let staging = directory.anchored_path();
        let git = self
            .git
            .bound(paths.clone(), vec![directory.clone()], ctx.retained.clone())?;
        ctx.log(format!("Staging the complete tree at commit {target}"));
        let frozen = Resolved {
            commit: target.clone(),
            branch: None,
            kind: RefKind::Commit,
        };
        let cloned = async {
            git.clone_checkout(
                &paths.root.anchored_path(),
                &hooks,
                &remote,
                &frozen,
                &staging,
                ctx,
            )
            .await?;
            let checkout = directory.clone();
            let tree = blocking(move || {
                tree::validate_fresh_git(&checkout)?;
                read_tree(&checkout, true)
            })
            .await?;
            git.require_clean(&paths.root.anchored_path(), &hooks, &staging)
                .await?;
            Ok::<_, NodeAppError>(tree)
        }
        .await;
        let tree = match cloned {
            Ok(tree) => tree,
            Err(error) => {
                drop(directory);
                if !matches!(error, NodeAppError::UnsafeSource) {
                    remove_owned(&staged, &owned)?;
                }
                return Err(error);
            }
        };
        let mut warnings = Vec::new();
        if staging.join(".gitmodules").is_file() {
            warnings.push("This repository declares submodules; they are not updated".into());
        }
        Ok(StagedNode {
            source: NodeSource::Git {
                repository: repository.clone(),
                commit: target,
                branch: branch.clone(),
            },
            name: prior.name.clone(),
            tree_digest: tree.digest,
            artifact_digest: None,
            requirement_lines: tree.requirement_lines,
            requirements_digest: tree.requirements_digest,
            hook_digest: tree.hook_digest,
            dependencies: Vec::new(),
            warnings,
        })
    }

    async fn snapshot_entry(
        &self,
        paths: &Arc<NodePaths>,
        entry: Entry,
        node: &InstalledNode,
        require_clean: bool,
        retained: Retained,
    ) -> Result<TreeSnapshot, NodeAppError> {
        let expected = node.clone();
        let (snapshot, directory) = blocking(move || {
            if expected.single_file {
                let mut file = entry.0.open(&entry.1, false, false).map_err(filesystem)?;
                let digest = crate::shared::model_paths::digest(&mut file).map_err(filesystem)?;
                return Ok((
                    TreeSnapshot {
                        digest: format!("file:{}", digest.as_str()),
                        requirements_digest: None,
                        hook_digest: None,
                        requirement_lines: Vec::new(),
                    },
                    None,
                ));
            }
            let directory = entry.0.child(&entry.1, false).map_err(filesystem)?;
            if is_git(&expected) {
                pin_git_dir(&directory)?;
            }
            let snapshot = snapshot_directory(&directory, &expected)?;
            let marker = read_marker(&directory).or_else(|| read_manager_registry(&directory));
            let git_present = entry_exists(&(directory.clone(), safe_name(".git")))?;
            let matches = match &expected.source {
                NodeSource::Git { .. } => {
                    git_present
                        && marker
                            .as_ref()
                            .is_none_or(|m| expected.source.same_origin(m))
                }
                NodeSource::Registry { .. } => {
                    !git_present && marker.as_ref().is_some_and(|m| *m == expected.source)
                }
                NodeSource::Manual => {
                    !git_present && marker.as_ref().is_none_or(|m| *m == NodeSource::Manual)
                }
            };
            if !matches {
                return Err(NodeAppError::UnsafeSource);
            }
            Ok((snapshot, Some(directory)))
        })
        .await?;
        if let (
            NodeSource::Git {
                repository, commit, ..
            },
            Some(directory),
        ) = (&node.source, directory)
        {
            let _git = pin_git_dir(&directory)?;
            let git = self
                .git
                .bound(paths.clone(), vec![directory.clone()], retained)?;
            let (root, hooks, dir) = (
                paths.root.anchored_path(),
                paths.hooks(),
                directory.anchored_path(),
            );
            let inspection = if require_clean {
                git.require_clean(&root, &hooks, &dir).await?
            } else {
                git.inspect(&root, &hooks, &dir).await?
            };
            if inspection.revision.commit != *commit
                || git.origin(&root, &hooks, &dir).await?.identity != *repository
            {
                return Err(NodeAppError::StalePlan);
            }
        }
        Ok(snapshot)
    }

    async fn matches_entry(
        &self,
        paths: &Arc<NodePaths>,
        entry: &Entry,
        node: &InstalledNode,
        require_clean: bool,
        retained: Retained,
    ) -> bool {
        if node.tree_digest.starts_with("git:") {
            return false;
        }
        self.snapshot_entry(paths, entry.clone(), node, require_clean, retained)
            .await
            .is_ok_and(|snapshot| snapshot.digest == node.tree_digest)
    }

    /// Checks a live Git update against the review. Nothing is reset or cleaned on failure: edits
    /// stay in place and recovery keeps the operation unresolved until the files are the reviewed
    /// ones.
    async fn verify_applied(
        &self,
        paths: &Arc<NodePaths>,
        directory: &Arc<PinnedDirectory>,
        operation: &NodeOperation,
        retained: Retained,
    ) -> Result<(), NodeAppError> {
        let git = self
            .git
            .bound(paths.clone(), vec![directory.clone()], retained)?;
        let NodeSource::Git {
            repository, commit, ..
        } = &operation.node.source
        else {
            return Err(NodeAppError::InvalidRequest);
        };
        let (root, hooks, dir) = (
            paths.root.anchored_path(),
            paths.hooks(),
            directory.anchored_path(),
        );
        let _git = pin_git_dir(directory)?;
        let checked = directory.clone();
        let snapshot = blocking(move || read_tree(&checked, true)).await?;
        let applied = git.require_clean(&root, &hooks, &dir).await?;
        if applied.revision.commit != *commit {
            return Err(NodeAppError::Failed(
                "The repository is not at the reviewed commit".into(),
            ));
        }
        if git.origin(&root, &hooks, &dir).await?.identity != *repository {
            return Err(NodeAppError::UnsafeSource);
        }
        if snapshot.digest != operation.node.tree_digest {
            return Err(NodeAppError::StalePlan);
        }
        Ok(())
    }
}

fn git_source(identity: &str, resolved: &Resolved) -> NodeSource {
    NodeSource::Git {
        repository: identity.to_owned(),
        commit: resolved.commit.clone(),
        branch: resolved.branch.clone(),
    }
}

fn is_git(node: &InstalledNode) -> bool {
    matches!(node.source, NodeSource::Git { .. })
}

#[async_trait]
impl NodeFiles for LocalNodeFiles {
    async fn git_status(
        &self,
        root: &Path,
        node: &InstalledNode,
        retained: Retained,
    ) -> Result<NodeGitStatus, NodeAppError> {
        if node.single_file || !is_git(node) {
            return Err(NodeAppError::InvalidRequest);
        }
        let paths = Arc::new(NodePaths::open(root, true)?);
        let entry = paths.location(node);
        let directory = entry.0.child(&entry.1, false).map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                NodeAppError::NotFound
            } else {
                filesystem(error)
            }
        })?;
        let location = directory.anchored_path();
        let checked = directory.clone();
        blocking(move || {
            checked
                .child(&safe_name(".git"), false)
                .map_err(filesystem)?;
            validate_tree(&checked, true)
        })
        .await?;
        let git = self
            .git
            .bound(paths.clone(), vec![directory.clone()], retained)?;
        let mut inspection = git
            .inspect(&paths.root.anchored_path(), &paths.hooks(), &location)
            .await?;
        inspection.state.status = crate::shared::logging::redact(&inspection.state.status)
            .chars()
            .take(2000)
            .collect();
        Ok(NodeGitStatus {
            revision: inspection.revision,
            state: inspection.state,
        })
    }

    async fn prepare(
        &self,
        root: &Path,
        operation: &NodeId,
        request: &PrepareRequest,
        prior: Option<&InstalledNode>,
        ctx: &StepContext,
    ) -> Result<StagedNode, NodeAppError> {
        let outcome = match (request, prior) {
            (PrepareRequest::Registry { id, version }, None) => {
                self.prepare_registry(root, operation, id, version.as_deref(), None, ctx)
                    .await
            }
            (
                PrepareRequest::Git {
                    repository,
                    reference,
                },
                None,
            ) => {
                self.prepare_git_install(root, operation, repository, reference.as_deref(), ctx)
                    .await
            }
            (PrepareRequest::Update { reference, .. }, Some(prior)) => match &prior.source {
                NodeSource::Registry { id, .. } => {
                    self.prepare_registry(
                        root,
                        operation,
                        id,
                        reference.as_deref(),
                        Some(prior),
                        ctx,
                    )
                    .await
                }
                NodeSource::Git { .. } if !prior.single_file => {
                    self.prepare_git_update(root, operation, prior, reference.as_deref(), ctx)
                        .await
                }
                _ => Err(NodeAppError::InvalidRequest),
            },
            _ => Err(NodeAppError::InvalidRequest),
        };
        // Each preparation removes only the entries it created itself: a refused create-new
        // may have met a foreign entry, which must survive.
        outcome
    }

    async fn snapshot(
        &self,
        root: &Path,
        node: &InstalledNode,
        staged: Option<&NodeId>,
        retained: Retained,
    ) -> Result<TreeSnapshot, NodeAppError> {
        let paths = Arc::new(NodePaths::open(root, true)?);
        let entry = match staged {
            Some(op) => paths.staging(op),
            None => paths.location(node),
        };
        self.snapshot_entry(
            &paths,
            entry,
            node,
            staged.is_some() && is_git(node),
            retained,
        )
        .await
    }

    async fn pin_location(
        &self,
        root: &Path,
        node: &InstalledNode,
        staged: Option<&NodeId>,
    ) -> Result<Box<dyn NodeLocationGuard>, NodeAppError> {
        if node.single_file {
            return Err(NodeAppError::InvalidRequest);
        }
        let paths = Arc::new(NodePaths::open(root, true)?);
        let node = node.clone();
        let entry = match staged {
            Some(op) => paths.staging(op),
            None => paths.location(&node),
        };
        blocking(move || {
            let directory = entry.0.child(&entry.1, false).map_err(|error| {
                if error.kind() == io::ErrorKind::NotFound {
                    NodeAppError::NotFound
                } else {
                    filesystem(error)
                }
            })?;
            let mut inputs = Vec::new();
            let requirements = match read_input(
                &directory,
                &safe_name("requirements.txt"),
                REQUIREMENTS_LIMIT,
            )? {
                None => None,
                Some((None, _)) => return Err(NodeAppError::UnsafeSource),
                Some((Some(bytes), file)) => {
                    std::str::from_utf8(&bytes).map_err(|_| NodeAppError::UnsafeSource)?;
                    inputs.push(file);
                    Some(bytes)
                }
            };
            let script = match read_input(&directory, &safe_name("install.py"), SCRIPT_LIMIT)? {
                Some((Some(bytes), file)) => {
                    inputs.push(file);
                    Some(bytes)
                }
                Some(_) => return Err(NodeAppError::UnsafeSource),
                None => None,
            };
            let mut snapshot = snapshot_directory(&directory, &node)?;
            // Execution consumes the bytes just read, so the plan is bound to exactly those.
            let text = requirements
                .as_deref()
                .map(|bytes| String::from_utf8_lossy(bytes).into_owned());
            snapshot.requirements_digest = text.as_ref().map(|t| sha256_hex(t.as_bytes()));
            snapshot.requirement_lines = text
                .as_deref()
                .map(parse_requirement_lines)
                .unwrap_or_default();
            snapshot.hook_digest = script.as_ref().map(|bytes| sha256_hex(bytes));
            Ok(Box::new(PinnedLocation::new(
                paths,
                directory,
                inputs,
                requirements,
                script,
                snapshot,
            )) as Box<dyn NodeLocationGuard>)
        })
        .await
    }

    fn path(&self, root: &Path, node: &InstalledNode, staged: Option<&NodeId>) -> PathBuf {
        // This is a display/port path, not authority for mutations. Each adapter operation
        // acquires its own retained pins before using the filesystem.
        let cn = root.join("ComfyUI/custom_nodes");
        match staged {
            Some(op) => cn
                .join(".disabled")
                .join(format!(".vjump-staging-{}", op.as_str())),
            None if node.enabled => cn.join(node.name.as_str()),
            None => cn.join(".disabled").join(node.name.as_str()),
        }
    }

    async fn publish(
        &self,
        root: &Path,
        operation: &NodeOperation,
        retained: Retained,
    ) -> Result<(), NodeAppError> {
        let paths = Arc::new(NodePaths::open(root, true)?);
        let node = &operation.node;
        if let (Some(prior), true) = (&operation.prior, is_git(node)) {
            // Git updates fast-forward in place; there is nothing staged to move.
            let (NodeSource::Git { commit: head, .. }, NodeSource::Git { commit: target, .. }) =
                (&prior.source, &node.source)
            else {
                return Err(NodeAppError::InvalidRequest);
            };
            let entry = paths.location(prior);
            let directory = entry.0.child(&entry.1, false).map_err(filesystem)?;
            let _git = pin_git_dir(&directory)?;
            let dir = directory.anchored_path();
            let ctx = quiet_context(operation, retained.clone());
            let git = self
                .git
                .bound(paths.clone(), vec![directory.clone()], retained.clone())?;
            git.apply(
                &paths.root.anchored_path(),
                &paths.hooks(),
                &dir,
                head,
                target,
                &ctx,
            )
            .await?;
            #[cfg(test)]
            git_apply_boundary();
            // Before any dependency or hook runs, the live result must be the reviewed one.
            return self
                .verify_applied(&paths, &directory, operation, retained)
                .await;
        }
        let (staging, destination) = (paths.staging(&operation.id), paths.location(node));
        blocking(move || rename(&staging, &destination)).await
    }

    async fn backup(&self, root: &Path, operation: &NodeOperation) -> Result<(), NodeAppError> {
        let paths = Arc::new(NodePaths::open(root, true)?);
        let prior = operation.prior.as_ref().ok_or(NodeAppError::NotFound)?;
        if operation.kind == OperationKind::Update && is_git(prior) {
            return Ok(());
        }
        let (source, backup) = (paths.location(prior), paths.backup(&operation.id));
        blocking(move || rename(&source, &backup)).await
    }

    async fn move_enabled(
        &self,
        root: &Path,
        prior: &InstalledNode,
        enabled: bool,
    ) -> Result<(), NodeAppError> {
        let paths = Arc::new(NodePaths::open(root, true)?);
        let (from, to) = (
            paths.location(prior),
            paths.named_location(&prior.name, enabled),
        );
        blocking(move || rename(&from, &to)).await
    }

    async fn recycle(&self, root: &Path, operation: &NodeOperation) -> Result<(), NodeAppError> {
        let paths = Arc::new(NodePaths::open(root, true)?);
        let operation = operation.id.clone();
        let recycler = self.recycler.clone();
        blocking(move || recycle_backup(&paths, &operation, &recycler)).await
    }

    async fn discard_staging(&self, root: &Path, operation: &NodeId) -> Result<(), NodeAppError> {
        let paths = Arc::new(NodePaths::open(root, true)?);
        let operation = operation.clone();
        blocking(move || paths.discard(&operation)).await
    }

    async fn recover(
        &self,
        root: &Path,
        operation: &NodeOperation,
        retained: Retained,
    ) -> Result<RecoveryAction, NodeAppError> {
        let paths = Arc::new(NodePaths::open(root, true)?);
        let staging = paths.staging(&operation.id);
        let backup = paths.backup(&operation.id);
        let live = paths.location(&operation.node);
        if operation.phase == OperationPhase::Prepared {
            let op = operation.id.clone();
            return blocking(move || {
                paths.discard(&op)?;
                Ok(RecoveryAction::Discarded)
            })
            .await;
        }
        // A partially executed hook may have changed contents. Establish source and location,
        // preserve every copy, and let the application record the failed dependency mutation.
        if operation.phase == OperationPhase::DependenciesInProgress {
            return Ok(
                if self
                    .snapshot_entry(&paths, live, &operation.node, false, retained)
                    .await
                    .is_ok()
                {
                    RecoveryAction::RolledForward
                } else {
                    RecoveryAction::Blocked
                },
            );
        }
        let (f, b, s) = (
            entry_exists(&live)?,
            entry_exists(&backup)?,
            entry_exists(&staging)?,
        );
        let require_clean = !matches!(
            operation.phase,
            OperationPhase::Verified | OperationPhase::CleanupPending
        ) && matches!(
            operation.kind,
            OperationKind::Install | OperationKind::Update
        );
        let published = f
            && self
                .matches_entry(
                    &paths,
                    &live,
                    &operation.node,
                    require_clean,
                    retained.clone(),
                )
                .await;
        let mut prior_live = false;
        let mut prior_backup = false;
        if let Some(prior) = &operation.prior {
            let location = paths.location(prior);
            prior_live = entry_exists(&location)?
                && self
                    .matches_entry(&paths, &location, prior, false, retained.clone())
                    .await;
            prior_backup = b
                && self
                    .matches_entry(&paths, &backup, prior, false, retained.clone())
                    .await;
        }
        let staged = s
            && self
                .matches_entry(&paths, &staging, &operation.node, false, retained)
                .await;
        let evidence = RecoveryEvidence {
            live: f,
            backup: b,
            staging: s,
            published,
            prior_live,
            prior_backup,
            staged,
        };
        let op = operation.clone();
        let recycler = self.recycler.clone();
        blocking(move || recover_blocking(&paths, &op, evidence, &recycler)).await
    }

    async fn scan(
        &self,
        root: &Path,
        retained: Retained,
    ) -> Result<Vec<InstalledNode>, NodeAppError> {
        let paths = Arc::new(NodePaths::open(root, true)?);
        let cn = paths.cn.clone();
        let mut nodes = blocking(move || Ok(scan::scan(&cn))).await?;
        if self.git.available(&paths.root.anchored_path()) {
            let hooks = paths.hooks();
            for node in &mut nodes {
                if matches!(node.source, NodeSource::Git { .. })
                    && !node.single_file
                    && !node.conflict
                {
                    let entry = paths.location(node);
                    let directory = entry.0.child(&entry.1, false).map_err(filesystem)?;
                    let git =
                        self.git
                            .bound(paths.clone(), vec![directory.clone()], retained.clone())?;
                    node.dirty = git
                        .is_dirty(
                            &paths.root.anchored_path(),
                            &hooks,
                            &directory.anchored_path(),
                        )
                        .await;
                }
            }
        }
        Ok(nodes)
    }
}

/// The digests `NodeFiles::snapshot` reports for the pinned directory of a node.
fn snapshot_directory(
    directory: &Arc<PinnedDirectory>,
    node: &InstalledNode,
) -> Result<TreeSnapshot, NodeAppError> {
    read_tree(directory, is_git(node))
}

/// Pins a Git node's `.git` without following links. Missing repository metadata is refused with
/// an explanation; it is never replaced by a guess.
fn pin_git_dir(directory: &Arc<PinnedDirectory>) -> Result<Arc<PinnedDirectory>, NodeAppError> {
    directory.child(&safe_name(".git"), false).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            refused("", "The repository metadata (.git) is missing.")
        } else {
            filesystem(error)
        }
    })
}

fn quiet_context(operation: &NodeOperation, retained: Retained) -> StepContext {
    StepContext {
        run_id: application::environment::RunId::new(operation.id.as_str())
            .expect("operation IDs are valid run IDs"),
        step: application::environment::StepId::ComfyRequirements,
        sink: Arc::new(application::environment::NullSink),
        cancel: application::shared::CancelToken::new(),
        allow_source_builds: false,
        retained,
    }
}

struct RecoveryEvidence {
    live: bool,
    backup: bool,
    staging: bool,
    published: bool,
    prior_live: bool,
    prior_backup: bool,
    staged: bool,
}

/// Every copy is classified before any mutation. Ambiguity preserves all copies.
fn recover_blocking(
    paths: &NodePaths,
    op: &NodeOperation,
    evidence: RecoveryEvidence,
    recycler: &Recycler,
) -> Result<RecoveryAction, NodeAppError> {
    let RecoveryEvidence {
        live,
        backup,
        staging,
        published,
        prior_live,
        prior_backup,
        staged,
    } = evidence;
    let verified = matches!(
        op.phase,
        OperationPhase::Verified | OperationPhase::CleanupPending
    );
    if matches!(op.kind, OperationKind::Enable | OperationKind::Disable) {
        let prior = op.prior.as_ref().ok_or(NodeAppError::Storage)?;
        let from = entry_exists(&paths.location(prior))?;
        return Ok(match (from, live) {
            (true, false) if prior_live => RecoveryAction::RolledBack,
            (false, true) if published => RecoveryAction::RolledForward,
            _ => RecoveryAction::Blocked,
        });
    }
    if backup && !prior_backup {
        return Ok(RecoveryAction::Blocked);
    }
    if verified {
        if (op.kind == OperationKind::Uninstall && live)
            || (op.kind != OperationKind::Uninstall && !published)
        {
            return Ok(RecoveryAction::Blocked);
        }
        paths.discard(&op.id)?;
        return Ok(match recycle_backup(paths, &op.id, recycler) {
            Ok(()) => RecoveryAction::RolledForward,
            // H3 recycle refusals are ordinary cleanup failures once identity is established.
            Err(_) => RecoveryAction::CleanupPending,
        });
    }
    if op.kind == OperationKind::Uninstall {
        return Ok(match (live, backup) {
            (true, false) if prior_live => RecoveryAction::RolledBack,
            (false, true) => {
                let prior = op.prior.as_ref().ok_or(NodeAppError::Storage)?;
                if rename(&paths.backup(&op.id), &paths.location(prior)).is_ok() {
                    RecoveryAction::RolledBack
                } else {
                    RecoveryAction::Blocked
                }
            }
            _ => RecoveryAction::Blocked,
        });
    }
    if op.kind == OperationKind::RetryDependencies {
        return Ok(if published {
            RecoveryAction::RolledForward
        } else {
            RecoveryAction::Blocked
        });
    }
    if staging && !staged {
        return Ok(RecoveryAction::Blocked);
    }
    let git_update = op.kind == OperationKind::Update && is_git(&op.node);
    let action = match (op.prior.is_some(), live, backup, staging) {
        (false, false, false, _) => RecoveryAction::Discarded,
        (false, true, false, false) if published => RecoveryAction::RolledForward,
        (true, true, false, _) if prior_live => RecoveryAction::RolledBack,
        (true, false, true, _) => {
            let prior = op.prior.as_ref().ok_or(NodeAppError::Storage)?;
            if rename(&paths.backup(&op.id), &paths.location(prior)).is_ok() {
                RecoveryAction::RolledBack
            } else {
                RecoveryAction::Blocked
            }
        }
        (true, true, _, false) if published => RecoveryAction::RolledForward,
        (true, true, false, true) if git_update && published => RecoveryAction::RolledForward,
        _ => RecoveryAction::Blocked,
    };
    if action != RecoveryAction::Blocked {
        paths.discard(&op.id)?;
    }
    Ok(action)
}
