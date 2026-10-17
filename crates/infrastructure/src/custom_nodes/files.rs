//! Filesystem adapter for node installation. All paths derive from the install root plus validated
//! names and operation IDs; staging and backups live under `custom_nodes/.disabled/`, which ComfyUI
//! never loads. Mutations are no-replace renames inside that tree, never unchecked deletes.
use super::{
    git::{GitNodes, RefKind, Resolved, refused},
    paths::{
        Entry, NodePaths, PinnedLocation, entry_path, exists as entry_exists, filesystem,
        read_input, remove, rename, safe_name,
    },
    provenance::{MARKER, TRACKING, repository_folder, safe_folder},
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
use application::{custom_nodes::*, environment::StepContext};
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
const REQUIREMENTS_LIMIT: u64 = 256 * 1024;
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

#[cfg(test)]
thread_local! {
    static REGISTRY_BOUNDARY: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
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

/// A Git node's `.git` must be a real directory. The tree walk leaves it to Git, so a link or
/// special entry there would otherwise go unchecked. A missing one is not refused: the node can
/// still be inspected, disabled and removed.
fn validate_git_dir(dir: &Path) -> Result<(), NodeAppError> {
    match dir.join(".git").symlink_metadata() {
        Ok(meta) if meta.is_dir() && !is_link(&meta) => Ok(()),
        Ok(_) => Err(NodeAppError::UnsafeSource),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(filesystem(error)),
    }
}

/// Review warnings of an exact release, completed by the node's own metadata. A dimension the
/// release leaves unknown falls back to the node's, and a flag or deprecation on either applies.
/// The warnings come from one merged evaluation, so none repeats and the order is fixed.
fn review_warnings(release: Compatibility, node: &Compatibility) -> Vec<String> {
    let known = |own: Support, other: Support| {
        if own == Support::Unknown { other } else { own }
    };
    Compatibility {
        os: known(release.os, node.os),
        accelerator: known(release.accelerator, node.accelerator),
        comfyui: known(release.comfyui, node.comfyui),
        flagged: release.flagged || node.flagged,
        deprecated: release.deprecated || node.deprecated,
    }
    .warnings()
}

/// A Git node's `.git` must be a real directory. The tree walk leaves it to Git, so a link or
/// tree digest covers every file, tracked or not, except the app's own files, the top-level
/// `.git` and `__pycache__`; a Git node's exact commit is carried by its source instead.
fn read_tree(dir: &Path, git_node: bool) -> Result<TreeSnapshot, NodeAppError> {
    not_link(dir)?;
    if git_node {
        validate_git_dir(dir)?;
    }
    validate_tree(dir)?;
    let digest = tree::tree_digest(dir)?;
    let requirements = read_text(&dir.join("requirements.txt"), 256 * 1024);
    let hook = {
        let path = dir.join("install.py");
        match path.symlink_metadata() {
            Ok(meta) if meta.is_file() && !is_link(&meta) => sha256_file(&path).ok(),
            _ => None,
        }
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
    let hoist = paths.staging_hoist(op);
    rename(&(directory.clone(), name), &hoist)?;
    drop(file);
    let hoisted = hoist.0.child(&hoist.1, false).map_err(filesystem)?;
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
    remove(&hoist)
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

fn write_tracking(directory: &PinnedDirectory) -> Result<(), NodeAppError> {
    let path = directory.anchored_path();
    let mut lines = Vec::new();
    for entry in walkdir::WalkDir::new(&path)
        .follow_links(false)
        .sort_by_file_name()
    {
        let entry = entry.map_err(|_| NodeAppError::Filesystem)?;
        if entry.depth() == 0 || !entry.file_type().is_file() {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(&path)
            .map_err(|_| NodeAppError::Filesystem)?
            .to_string_lossy()
            .replace('\\', "/");
        if relative != MARKER && relative != TRACKING {
            lines.push(relative);
        }
    }
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
                if entry_exists(&staging)? {
                    return Err(NodeAppError::Filesystem);
                }
                fs::create_dir(entry_path(&staging)).map_err(filesystem)?;
                let directory = staging.0.child(&staging.1, false).map_err(filesystem)?;
                let owned = identity(&directory.file).map_err(filesystem)?;
                let extracted = (|| {
                    tree::extract_zip(archive, &directory, limits)?;
                    hoist_single_root(&layout, &operation)?;
                    let staging_path = directory.anchored_path();
                    if entry_exists(&(directory.clone(), safe_name(MARKER)))? {
                        return Err(NodeAppError::UnsafeSource);
                    }
                    validate_tree(&staging_path)?;
                    remove(&(directory.clone(), safe_name(TRACKING)))?;
                    let snapshot = read_tree(&staging_path, false)?;
                    write_tracking(&directory)?;
                    write_marker(&directory, &marker_name, &marker_source, Some(&digest))?;
                    Ok(snapshot)
                })();
                drop(directory);
                if extracted.is_err() {
                    remove_owned(&staging, &owned)?;
                    // A refused foreign hoist entry was rejected before this attempt began, so
                    // any hoist entry present now belongs to this extraction.
                    remove(&layout.staging_hoist(&operation))?;
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
        let warnings = review_warnings(
            evaluate(&info.metadata, &self.registry.host()),
            &node.evaluated,
        );
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
        ctx.log("Resolving the repository reference");
        let resolved = self
            .git
            .resolve(
                &paths.root.anchored_path(),
                &hooks,
                &remote,
                reference,
                &ctx.cancel,
            )
            .await?;
        let staged = paths.staging(op);
        if entry_exists(&staged)? {
            return Err(NodeAppError::Filesystem);
        }
        let directory = staged.0.child(&staged.1, true).map_err(filesystem)?;
        let staging = directory.anchored_path();
        ctx.log(format!("Cloning at commit {}", resolved.commit));
        let cloned = async {
            self.git
                .clone_checkout(
                    &paths.root.anchored_path(),
                    &hooks,
                    &remote,
                    &resolved,
                    &staging,
                    ctx,
                )
                .await?;
            let checkout = staging.clone();
            let directory = directory.clone();
            let source = git_source(&remote.identity, &resolved);
            let (marker_source, marker_name) = (source.clone(), name.as_str().to_owned());
            let tree = blocking(move || {
                validate_tree(&checkout)?;
                // Keep the app's marker out of `git status`.
                let git = directory
                    .child(&safe_name(".git"), false)
                    .map_err(filesystem)?;
                let info = git.child(&safe_name("info"), true).map_err(filesystem)?;
                let exclude = safe_name("exclude");
                let mut text = read_text(&info.anchored_path().join(exclude.as_str()), 1 << 20)
                    .unwrap_or_default();
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
                read_tree(&checkout, true)
            })
            .await?;
            // The reviewed tree is exactly the checkout of the resolved commit.
            self.git
                .require_clean(&paths.root.anchored_path(), &hooks, &staging)
                .await?;
            Ok::<_, NodeAppError>((source, tree))
        }
        .await;
        let (source, tree) = match cloned {
            Ok(value) => value,
            Err(error) => {
                drop(directory);
                remove(&staged)?;
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
        {
            let (dir, live_directory) = (dir.clone(), live_directory.clone());
            blocking(move || {
                not_link(&dir)?;
                pin_git_dir(&live_directory)
            })
            .await?;
        }
        let hooks = paths.hooks();
        ctx.log("Fetching and freezing the update target");
        let target = self
            .git
            .freeze_update(&paths.root.anchored_path(), &hooks, &dir, reference, ctx)
            .await?;
        // The reviewed tree comes from the repository the node was installed from.
        let remote = self
            .git
            .origin(&paths.root.anchored_path(), &hooks, &dir)
            .await?;
        if remote.identity != *repository {
            return Err(NodeAppError::UnsafeSource);
        }
        // The live update stays an in-place fast-forward; this complete checkout of the frozen
        // target is the evidence that is reviewed and later compared with the live result.
        let staged = paths.staging(op);
        if entry_exists(&staged)? {
            return Err(NodeAppError::Filesystem);
        }
        let directory = staged.0.child(&staged.1, true).map_err(filesystem)?;
        let staging = directory.anchored_path();
        ctx.log(format!("Staging the complete tree at commit {target}"));
        let frozen = Resolved {
            commit: target.clone(),
            branch: None,
            kind: RefKind::Commit,
        };
        let cloned = async {
            self.git
                .clone_checkout(
                    &paths.root.anchored_path(),
                    &hooks,
                    &remote,
                    &frozen,
                    &staging,
                    ctx,
                )
                .await?;
            let checkout = staging.clone();
            let tree = blocking(move || read_tree(&checkout, true)).await?;
            self.git
                .require_clean(&paths.root.anchored_path(), &hooks, &staging)
                .await?;
            Ok::<_, NodeAppError>(tree)
        }
        .await;
        let tree = match cloned {
            Ok(tree) => tree,
            Err(error) => {
                // The staging directory was created by this attempt after the foreign-entry check.
                drop(directory);
                remove(&staged)?;
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

    /// Checks a live Git update against the review. Nothing is reset or cleaned on failure: edits
    /// stay in place and recovery keeps the operation unresolved until the files are the reviewed
    /// ones.
    async fn verify_applied(
        &self,
        paths: &NodePaths,
        directory: &Arc<PinnedDirectory>,
        operation: &NodeOperation,
    ) -> Result<(), NodeAppError> {
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
        let applied = self.git.require_clean(&root, &hooks, &dir).await?;
        if applied.revision.commit != *commit {
            return Err(NodeAppError::Failed(
                "The repository is not at the reviewed commit".into(),
            ));
        }
        if self.git.origin(&root, &hooks, &dir).await?.identity != *repository {
            return Err(NodeAppError::UnsafeSource);
        }
        let checked = directory.clone();
        let digest = blocking(move || {
            let path = checked.anchored_path();
            validate_git_dir(&path)?;
            validate_tree(&path)?;
            tree::tree_digest(&path)
        })
        .await?;
        if digest != operation.node.tree_digest {
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
            validate_tree(&checked.anchored_path())
        })
        .await?;
        let mut inspection = self
            .git
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
    ) -> Result<TreeSnapshot, NodeAppError> {
        let paths = Arc::new(NodePaths::open(root, true)?);
        let node = node.clone();
        let entry = match staged {
            Some(op) => paths.staging(op),
            None => paths.location(&node),
        };
        let require_clean = staged.is_some() && is_git(&node);
        let (snapshot, directory) = blocking(move || {
            if node.single_file {
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
            let snapshot = snapshot_directory(&directory, &node)?;
            Ok((snapshot, Some(directory)))
        })
        .await?;
        // A staged Git checkout is review evidence: it must still be exactly its commit. Live
        // nodes are never required to be clean, so dirty ones stay inspectable and removable.
        if let (true, Some(directory)) = (require_clean, directory) {
            self.git
                .require_clean(
                    &paths.root.anchored_path(),
                    &paths.hooks(),
                    &directory.anchored_path(),
                )
                .await?;
        }
        Ok(snapshot)
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
            let requirements = read_input(&directory, "requirements.txt", REQUIREMENTS_LIMIT)?
                .and_then(|(bytes, file)| {
                    inputs.push(file);
                    bytes
                });
            let script = match read_input(&directory, "install.py", SCRIPT_LIMIT)? {
                Some((Some(bytes), file)) => {
                    inputs.push(file);
                    Some(bytes)
                }
                Some(_) => return Err(NodeAppError::UnsafeSource),
                None => None,
            };
            let mut snapshot = snapshot_directory(&directory, &node)?;
            // Execution consumes the bytes just read, so the plan is bound to exactly those.
            let requirements = requirements.filter(|bytes| std::str::from_utf8(bytes).is_ok());
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

    async fn publish(&self, root: &Path, operation: &NodeOperation) -> Result<(), NodeAppError> {
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
            let dir = directory.anchored_path();
            let ctx = quiet_context(operation);
            self.git
                .apply(
                    &paths.root.anchored_path(),
                    &paths.hooks(),
                    &dir,
                    head,
                    target,
                    &ctx,
                )
                .await?;
            // Before any dependency or hook runs, the live result must be the reviewed one.
            return self.verify_applied(&paths, &directory, operation).await;
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
    ) -> Result<RecoveryAction, NodeAppError> {
        let paths = Arc::new(NodePaths::open(root, true)?);
        let op = operation.clone();
        let recycler = self.recycler.clone();
        blocking(move || {
            let git_head = match (&op.prior, is_git(&op.node)) {
                (Some(prior), true) => {
                    let entry = paths.location(prior);
                    if entry_exists(&entry)? {
                        let directory = entry.0.child(&entry.1, false).map_err(filesystem)?;
                        scan::read_git(&directory.anchored_path()).and_then(|m| m.commit)
                    } else {
                        None
                    }
                }
                _ => None,
            };
            recover_blocking(&paths, &op, git_head, &recycler)
        })
        .await
    }

    async fn scan(&self, root: &Path) -> Result<Vec<InstalledNode>, NodeAppError> {
        let paths = Arc::new(NodePaths::open(root, true)?);
        let cn = paths.cn.clone();
        let mut nodes = blocking(move || Ok(scan::scan(&cn.anchored_path()))).await?;
        if self.git.available(&paths.root.anchored_path()) {
            let hooks = paths.hooks();
            for node in &mut nodes {
                if matches!(node.source, NodeSource::Git { .. }) && !node.single_file {
                    let entry = paths.location(node);
                    let directory = entry.0.child(&entry.1, false).map_err(filesystem)?;
                    node.dirty = self
                        .git
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
    read_tree(&directory.anchored_path(), is_git(node))
}

/// Pins a Git node's `.git` without following links. Missing repository metadata is refused with
/// an explanation; it is never replaced by a guess.
fn pin_git_dir(directory: &Arc<PinnedDirectory>) -> Result<(), NodeAppError> {
    directory
        .child(&safe_name(".git"), false)
        .map(drop)
        .map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                refused("", "The repository metadata (.git) is missing.")
            } else {
                filesystem(error)
            }
        })
}

fn quiet_context(operation: &NodeOperation) -> StepContext {
    StepContext {
        run_id: application::environment::RunId::new(operation.id.as_str())
            .expect("operation IDs are valid run IDs"),
        step: application::environment::StepId::ComfyRequirements,
        sink: Arc::new(application::environment::NullSink),
        cancel: application::shared::CancelToken::new(),
        allow_source_builds: false,
    }
}

fn digest_of(entry: &Entry, node: &InstalledNode) -> Option<String> {
    if node.single_file {
        let mut file = entry.0.open(&entry.1, false, false).ok()?;
        return crate::shared::model_paths::digest(&mut file)
            .ok()
            .map(|d| format!("file:{}", d.as_str()));
    }
    let directory = entry.0.child(&entry.1, false).ok()?;
    read_tree(&directory.anchored_path(), is_git(node))
        .ok()
        .map(|s| s.digest)
}

/// Conservative reconciliation: every action is derived from what exists on disk now, compared
/// with the identities recorded in the journal. Anything ambiguous blocks and keeps all copies.
fn recover_blocking(
    paths: &NodePaths,
    op: &NodeOperation,
    git_head: Option<String>,
    recycler: &Recycler,
) -> Result<RecoveryAction, NodeAppError> {
    let staging = paths.staging(&op.id);
    let backup = paths.backup(&op.id);
    if matches!(
        op.phase,
        OperationPhase::Verified | OperationPhase::CleanupPending
    ) {
        paths.discard(&op.id)?;
        match recycle_backup(paths, &op.id, recycler) {
            Ok(()) => {}
            Err(NodeAppError::UnsafeSource) => return Err(NodeAppError::UnsafeSource),
            Err(_) => return Ok(RecoveryAction::CleanupPending),
        }
        return Ok(RecoveryAction::RolledForward);
    }
    if op.phase == OperationPhase::Prepared || op.kind == OperationKind::RetryDependencies {
        paths.discard(&op.id)?;
        return Ok(RecoveryAction::Discarded);
    }
    let restore = |from: &Entry, to: &Entry| rename(from, to).is_ok();
    match op.kind {
        OperationKind::Enable | OperationKind::Disable => {
            let prior = op.prior.as_ref().ok_or(NodeAppError::Storage)?;
            let (from, to) = (
                paths.location(prior),
                paths.named_location(&prior.name, op.node.enabled),
            );
            Ok(match (entry_exists(&from)?, entry_exists(&to)?) {
                (true, false) => RecoveryAction::RolledBack,
                (false, true) => RecoveryAction::RolledForward,
                _ => RecoveryAction::Blocked,
            })
        }
        OperationKind::Uninstall => {
            let prior = op.prior.as_ref().ok_or(NodeAppError::Storage)?;
            let live = paths.location(prior);
            Ok(match (entry_exists(&live)?, entry_exists(&backup)?) {
                (true, false) => RecoveryAction::RolledBack,
                (false, true) => {
                    if restore(&backup, &live) {
                        RecoveryAction::RolledBack
                    } else {
                        RecoveryAction::Blocked
                    }
                }
                (false, false) => RecoveryAction::RolledForward,
                (true, true) => RecoveryAction::Blocked,
            })
        }
        OperationKind::Install | OperationKind::Update => {
            if let (Some(prior), true) = (&op.prior, is_git(&op.node)) {
                let (NodeSource::Git { commit: old, .. }, NodeSource::Git { commit: new, .. }) =
                    (&prior.source, &op.node.source)
                else {
                    return Ok(RecoveryAction::Blocked);
                };
                paths.discard(&op.id)?;
                return Ok(match git_head {
                    // The files moved: they are only accepted when they are the reviewed ones.
                    Some(head)
                        if head == *new
                            && digest_of(&paths.location(prior), &op.node).as_deref()
                                == Some(op.node.tree_digest.as_str()) =>
                    {
                        RecoveryAction::RolledForward
                    }
                    Some(head) if head == *old => RecoveryAction::RolledBack,
                    _ => RecoveryAction::Blocked,
                });
            }
            let final_path = paths.location(&op.node);
            let (f, b, s) = (
                entry_exists(&final_path)?,
                entry_exists(&backup)?,
                entry_exists(&staging)?,
            );
            let new_digest = &op.node.tree_digest;
            let published =
                || digest_of(&final_path, &op.node).as_deref() == Some(new_digest.as_str());
            let is_prior = || {
                op.prior.as_ref().is_some_and(|p| {
                    digest_of(&final_path, p).as_deref() == Some(p.tree_digest.as_str())
                })
            };
            match (op.prior.is_some(), f, b, s) {
                // Nothing was published: the staging copy is the only thing to remove.
                (false, false, _, true) => {
                    paths.discard(&op.id)?;
                    Ok(RecoveryAction::Discarded)
                }
                (false, false, _, false) => Ok(RecoveryAction::Discarded),
                (false, true, _, false) => Ok(if published() {
                    RecoveryAction::RolledForward
                } else {
                    RecoveryAction::Blocked
                }),
                (true, true, false, true) => {
                    paths.discard(&op.id)?;
                    Ok(RecoveryAction::RolledBack)
                }
                (true, false, true, true) | (true, false, true, false) => {
                    if restore(&backup, &final_path) {
                        paths.discard(&op.id)?;
                        Ok(RecoveryAction::RolledBack)
                    } else {
                        Ok(RecoveryAction::Blocked)
                    }
                }
                (true, true, true, false) => Ok(if published() {
                    RecoveryAction::RolledForward
                } else {
                    RecoveryAction::Blocked
                }),
                (true, true, false, false) => Ok(if published() {
                    RecoveryAction::RolledForward
                } else if is_prior() {
                    RecoveryAction::RolledBack
                } else {
                    RecoveryAction::Blocked
                }),
                _ => Ok(RecoveryAction::Blocked),
            }
        }
        OperationKind::RetryDependencies => Ok(RecoveryAction::Discarded),
    }
}
