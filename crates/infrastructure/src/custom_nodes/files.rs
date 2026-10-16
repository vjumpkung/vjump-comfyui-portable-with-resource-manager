//! Filesystem adapter for node installation. All paths derive from the install root plus validated
//! names and operation IDs; staging and backups live under `custom_nodes/.disabled/`, which ComfyUI
//! never loads. Mutations are no-replace renames inside that tree, never unchecked deletes.
use super::{
    git::{GitNodes, RefKind, Resolved},
    provenance::{self, MARKER, TRACKING, repository_folder, safe_folder},
    registry::RegistryHttp,
    scan,
    tree::{self, is_link, read_text, rename_no_replace, sha256_file, validate_tree},
};
use crate::{
    custom_nodes::compat::evaluate,
    shared::{
        archive::{ArchiveError, ArchiveKind, ExtractLimits, extract},
        download::sha256_hex,
    },
};
use application::{custom_nodes::*, environment::StepContext};
use async_trait::async_trait;
use domain::{custom_nodes::*, shared::model_values::SafeFileName};
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};

pub type Recycler = Arc<dyn Fn(&Path) -> io::Result<()> + Send + Sync>;

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

struct Paths {
    root: PathBuf,
    cn: PathBuf,
    disabled: PathBuf,
}

impl Paths {
    fn new(root: &Path) -> Self {
        let cn = root.join("ComfyUI").join("custom_nodes");
        Self {
            root: root.to_owned(),
            disabled: cn.join(".disabled"),
            cn,
        }
    }
    fn staging(&self, op: &NodeId) -> PathBuf {
        self.disabled
            .join(format!(".vjump-staging-{}", op.as_str()))
    }
    fn staging_zip(&self, op: &NodeId) -> PathBuf {
        self.disabled
            .join(format!(".vjump-staging-{}.zip", op.as_str()))
    }
    fn staging_hoist(&self, op: &NodeId) -> PathBuf {
        self.disabled
            .join(format!(".vjump-staging-{}-hoist", op.as_str()))
    }
    fn backup(&self, op: &NodeId) -> PathBuf {
        self.disabled.join(format!(".vjump-backup-{}", op.as_str()))
    }
    fn hooks(&self) -> PathBuf {
        self.disabled.join(".vjump-no-hooks")
    }
    fn location(&self, name: &SafeFileName, enabled: bool) -> PathBuf {
        if enabled {
            self.cn.join(name.as_str())
        } else {
            self.disabled.join(name.as_str())
        }
    }
    fn live(&self, node: &InstalledNode) -> PathBuf {
        self.location(&node.name, node.enabled)
    }
    /// The folder, validated: real directories only, never a link.
    fn ensure_layout(&self) -> Result<(), NodeAppError> {
        let comfy = self.root.join("ComfyUI");
        if !comfy.is_dir() {
            return Err(NodeAppError::Filesystem);
        }
        for directory in [&self.cn, &self.disabled] {
            match directory.symlink_metadata() {
                Ok(meta) if is_link(&meta) || !meta.is_dir() => {
                    return Err(NodeAppError::UnsafeSource);
                }
                Ok(_) => {}
                Err(_) => fs::create_dir(directory).map_err(|_| NodeAppError::Filesystem)?,
            }
        }
        fs::create_dir_all(self.hooks()).map_err(|_| NodeAppError::Filesystem)?;
        Ok(())
    }
}

fn exists(path: &Path) -> bool {
    path.symlink_metadata().is_ok()
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

fn archive_error(error: ArchiveError) -> NodeAppError {
    match error {
        ArchiveError::Io(_) | ArchiveError::StagingNotFresh => NodeAppError::Filesystem,
        _ => NodeAppError::UnsafeSource,
    }
}

fn remove_owned(path: &Path) {
    match path.symlink_metadata() {
        Ok(meta) if is_link(&meta) || meta.is_file() => {
            let _ = fs::remove_file(path).or_else(|_| fs::remove_dir(path));
        }
        Ok(_) => {
            let _ = fs::remove_dir_all(path);
        }
        Err(_) => {}
    }
}

fn parse_requirement_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .take(500)
        .map(|l| l.chars().take(300).collect())
        .collect()
}

/// Digests of a node directory: its tree, root requirements and root install hook.
fn read_tree(dir: &Path, git_node: bool) -> Result<TreeSnapshot, NodeAppError> {
    not_link(dir)?;
    let digest = if git_node && let Some(commit) = scan::read_git(dir).and_then(|m| m.commit) {
        format!("git:{commit}")
    } else {
        tree::tree_digest(dir)?
    };
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
fn hoist_single_root(staging: &Path, hoist: &Path) -> Result<(), NodeAppError> {
    let entries: Vec<_> = fs::read_dir(staging)
        .map_err(|_| NodeAppError::Filesystem)?
        .flatten()
        .collect();
    let [only] = entries.as_slice() else {
        return Ok(());
    };
    let meta = only
        .path()
        .symlink_metadata()
        .map_err(|_| NodeAppError::Filesystem)?;
    if !meta.is_dir() || is_link(&meta) {
        return Ok(());
    }
    rename_no_replace(&only.path(), hoist).map_err(|_| NodeAppError::Filesystem)?;
    for child in fs::read_dir(hoist)
        .map_err(|_| NodeAppError::Filesystem)?
        .flatten()
    {
        rename_no_replace(&child.path(), &staging.join(child.file_name()))
            .map_err(|_| NodeAppError::Filesystem)?;
    }
    fs::remove_dir(hoist).map_err(|_| NodeAppError::Filesystem)
}

impl LocalNodeFiles {
    fn paths(&self, root: &Path) -> Paths {
        Paths::new(root)
    }

    async fn prepare_registry(
        &self,
        root: &Path,
        op: &NodeId,
        id: &str,
        version: Option<&str>,
        prior: Option<&InstalledNode>,
        ctx: &StepContext,
    ) -> Result<StagedNode, NodeAppError> {
        let paths = self.paths(root);
        let name = match prior {
            Some(prior) => prior.name.clone(),
            None => safe_folder(id).ok_or(NodeAppError::InvalidRequest)?,
        };
        {
            let layout = Paths::new(root);
            blocking(move || layout.ensure_layout()).await?;
        }
        if prior.is_none()
            && (exists(&paths.location(&name, true)) || exists(&paths.location(&name, false)))
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
        let node = self.registry.node(id).await.ok();
        let (publisher, repository) = match (&node, prior.map(|p| &p.source)) {
            (Some(n), _) => (n.publisher.clone(), n.repository.clone()),
            (
                None,
                Some(NodeSource::Registry {
                    publisher,
                    repository,
                    ..
                }),
            ) => (publisher.clone(), repository.clone()),
            _ => (None, None),
        };
        let source = NodeSource::Registry {
            id: id.to_owned(),
            publisher,
            version: exact.clone(),
            repository,
        };
        ctx.log(format!("Downloading {id} {exact}"));
        let zip = paths.staging_zip(op);
        let staging = paths.staging(op);
        let hoist = paths.staging_hoist(op);
        let result = async {
            let artifact = self.registry.download(&url, &zip, &ctx.cancel).await?;
            let limits = self.limits;
            let (zip_path, staging_path, hoist_path, marker_source, marker_name, digest) = (
                zip.clone(),
                staging.clone(),
                hoist.clone(),
                source.clone(),
                name.as_str().to_owned(),
                artifact.sha256.clone(),
            );
            let tree = blocking(move || {
                fs::create_dir(&staging_path).map_err(|_| NodeAppError::Filesystem)?;
                extract(&zip_path, ArchiveKind::Zip, &staging_path, limits)
                    .map_err(archive_error)?;
                hoist_single_root(&staging_path, &hoist_path)?;
                // The app owns these two files; an archive may not supply them.
                if exists(&staging_path.join(MARKER)) {
                    return Err(NodeAppError::UnsafeSource);
                }
                validate_tree(&staging_path)?;
                remove_owned(&staging_path.join(TRACKING));
                let snapshot = read_tree(&staging_path, false)?;
                provenance::write_tracking(&staging_path).map_err(|_| NodeAppError::Filesystem)?;
                provenance::write_marker(
                    &staging_path,
                    &marker_name,
                    &marker_source,
                    Some(&digest),
                )
                .map_err(|_| NodeAppError::Filesystem)?;
                let _ = fs::remove_file(&zip_path);
                Ok(snapshot)
            })
            .await?;
            Ok::<_, NodeAppError>((artifact, tree))
        }
        .await;
        remove_owned(&zip);
        let (artifact, tree) = match result {
            Ok(value) => value,
            Err(error) => {
                remove_owned(&staging);
                remove_owned(&hoist);
                return Err(error);
            }
        };
        let host = self.registry.host();
        let mut warnings = evaluate(&info.metadata, &host).warnings();
        if info.metadata.deprecated == Some(true)
            && !warnings.iter().any(|w| w.contains("deprecated"))
        {
            warnings.push("This node version is deprecated".into());
        }
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
        let paths = self.paths(root);
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
        {
            let layout = Paths::new(root);
            blocking(move || layout.ensure_layout()).await?;
        }
        if exists(&paths.location(&name, true)) || exists(&paths.location(&name, false)) {
            return Err(NodeAppError::TargetExists);
        }
        let hooks = paths.hooks();
        ctx.log("Resolving the repository reference");
        let resolved = self
            .git
            .resolve(root, &hooks, &remote, reference, &ctx.cancel)
            .await?;
        let staging = paths.staging(op);
        ctx.log(format!("Cloning at commit {}", resolved.commit));
        let cloned = async {
            self.git
                .clone_checkout(root, &hooks, &remote, &resolved, &staging, ctx)
                .await?;
            let staging = staging.clone();
            let source = git_source(&remote.identity, &resolved);
            let (marker_source, marker_name) = (source.clone(), name.as_str().to_owned());
            let tree = blocking(move || {
                validate_tree(&staging)?;
                // Keep the app's marker out of `git status`.
                let exclude = staging.join(".git").join("info").join("exclude");
                if let Some(parent) = exclude.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let mut text = fs::read_to_string(&exclude).unwrap_or_default();
                text.push_str(&format!("\n{MARKER}\n{TRACKING}\n"));
                fs::write(&exclude, text).map_err(|_| NodeAppError::Filesystem)?;
                provenance::write_marker(&staging, &marker_name, &marker_source, None)
                    .map_err(|_| NodeAppError::Filesystem)?;
                read_tree(&staging, true)
            })
            .await?;
            Ok::<_, NodeAppError>((source, tree))
        }
        .await;
        let (source, tree) = match cloned {
            Ok(value) => value,
            Err(error) => {
                remove_owned(&staging);
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
        let paths = self.paths(root);
        {
            let layout = Paths::new(root);
            blocking(move || layout.ensure_layout()).await?;
        }
        let dir = paths.live(prior);
        let NodeSource::Git {
            repository, branch, ..
        } = &prior.source
        else {
            return Err(NodeAppError::InvalidRequest);
        };
        {
            let dir = dir.clone();
            blocking(move || not_link(&dir)).await?;
        }
        if scan::read_git(&dir).is_none() {
            return Err(NodeAppError::GitRefused);
        }
        let hooks = paths.hooks();
        ctx.log("Fetching and freezing the update target");
        let target = self
            .git
            .freeze_update(root, &hooks, &dir, reference, ctx)
            .await?;
        // Dependencies are previewed from the target's files, read without checking them out.
        let staging = paths.staging(op);
        let requirements = self
            .git
            .show_blob(root, &hooks, &dir, &target, "requirements.txt")
            .await;
        let hook = self
            .git
            .show_blob(root, &hooks, &dir, &target, "install.py")
            .await;
        let stub = staging.clone();
        let (requirements_text, hook_text) = (requirements.clone(), hook.clone());
        blocking(move || {
            fs::create_dir(&stub).map_err(|_| NodeAppError::Filesystem)?;
            if let Some(text) = &requirements_text {
                fs::write(stub.join("requirements.txt"), text)
                    .map_err(|_| NodeAppError::Filesystem)?;
            }
            if let Some(text) = &hook_text {
                fs::write(stub.join("install.py"), text).map_err(|_| NodeAppError::Filesystem)?;
            }
            Ok(())
        })
        .await?;
        let mut warnings = Vec::new();
        if self
            .git
            .show_blob(root, &hooks, &dir, &target, ".gitmodules")
            .await
            .is_some()
        {
            warnings.push("This repository declares submodules; they are not updated".into());
        }
        Ok(StagedNode {
            source: NodeSource::Git {
                repository: repository.clone(),
                commit: target.clone(),
                branch: branch.clone(),
            },
            name: prior.name.clone(),
            tree_digest: format!("git:{target}"),
            artifact_digest: None,
            requirement_lines: requirements
                .as_deref()
                .map(parse_requirement_lines)
                .unwrap_or_default(),
            requirements_digest: requirements.as_ref().map(|t| sha256_hex(t.as_bytes())),
            hook_digest: hook.as_ref().map(|t| sha256_hex(t.as_bytes())),
            dependencies: Vec::new(),
            warnings,
        })
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
        let paths = self.paths(root);
        let location = paths.live(node);
        let checked = location.clone();
        let enabled = node.enabled;
        let validation = Paths::new(root);
        blocking(move || {
            for path in [
                &validation.root,
                &validation.root.join("ComfyUI"),
                &validation.cn,
            ] {
                not_link(path)?;
            }
            if !enabled {
                not_link(&validation.disabled)?;
            }
            not_link(&checked)?;
            not_link(&checked.join(".git"))?;
            validate_tree(&checked)
        })
        .await?;
        let mut inspection = self.git.inspect(root, &paths.hooks(), &location).await?;
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
        if outcome.is_err() {
            self.discard_staging(root, operation).await.ok();
        }
        outcome
    }

    async fn snapshot(
        &self,
        root: &Path,
        node: &InstalledNode,
        staged: Option<&NodeId>,
    ) -> Result<TreeSnapshot, NodeAppError> {
        let paths = self.paths(root);
        let node = node.clone();
        let location = match staged {
            Some(op) => paths.staging(op),
            None => paths.live(&node),
        };
        let git_update_stub = staged.is_some() && is_git(&node) && !location.join(".git").exists();
        blocking(move || {
            if node.single_file {
                not_link(&location)?;
                let digest = sha256_file(&location).map_err(|_| NodeAppError::Filesystem)?;
                return Ok(TreeSnapshot {
                    digest: format!("file:{digest}"),
                    requirements_digest: None,
                    hook_digest: None,
                    requirement_lines: Vec::new(),
                });
            }
            if git_update_stub {
                // The stub holds only the target's requirement and hook files.
                let mut snapshot = read_tree_files_only(&location)?;
                if let NodeSource::Git { commit, .. } = &node.source {
                    snapshot.digest = format!("git:{commit}");
                }
                return Ok(snapshot);
            }
            read_tree(&location, is_git(&node))
        })
        .await
    }

    fn path(&self, root: &Path, node: &InstalledNode, staged: Option<&NodeId>) -> PathBuf {
        let paths = self.paths(root);
        match staged {
            Some(op) => paths.staging(op),
            None => paths.live(node),
        }
    }

    async fn publish(&self, root: &Path, operation: &NodeOperation) -> Result<(), NodeAppError> {
        let paths = self.paths(root);
        let node = &operation.node;
        if let (Some(prior), true) = (&operation.prior, is_git(node)) {
            // Git updates fast-forward in place; there is nothing staged to move.
            let (NodeSource::Git { commit: head, .. }, NodeSource::Git { commit: target, .. }) =
                (&prior.source, &node.source)
            else {
                return Err(NodeAppError::InvalidRequest);
            };
            let dir = paths.live(prior);
            let ctx = quiet_context(operation);
            return self
                .git
                .apply(root, &paths.hooks(), &dir, head, target, &ctx)
                .await;
        }
        let (staging, destination) = (paths.staging(&operation.id), paths.live(node));
        let layout = Paths::new(root);
        blocking(move || {
            layout.ensure_layout()?;
            if exists(&destination) {
                return Err(NodeAppError::TargetExists);
            }
            not_link(&staging)?;
            rename_no_replace(&staging, &destination).map_err(|error| {
                if error.kind() == io::ErrorKind::AlreadyExists {
                    NodeAppError::TargetExists
                } else {
                    NodeAppError::Filesystem
                }
            })
        })
        .await
    }

    async fn backup(&self, root: &Path, operation: &NodeOperation) -> Result<(), NodeAppError> {
        let paths = self.paths(root);
        let prior = operation.prior.as_ref().ok_or(NodeAppError::NotFound)?;
        if operation.kind == OperationKind::Update && is_git(prior) {
            return Ok(());
        }
        let (source, backup) = (paths.live(prior), paths.backup(&operation.id));
        let layout = Paths::new(root);
        blocking(move || {
            layout.ensure_layout()?;
            not_link(&source)?;
            rename_no_replace(&source, &backup).map_err(|_| NodeAppError::Filesystem)
        })
        .await
    }

    async fn move_enabled(
        &self,
        root: &Path,
        prior: &InstalledNode,
        enabled: bool,
    ) -> Result<(), NodeAppError> {
        let paths = self.paths(root);
        let (from, to) = (paths.live(prior), paths.location(&prior.name, enabled));
        let layout = Paths::new(root);
        blocking(move || {
            layout.ensure_layout()?;
            not_link(&from)?;
            rename_no_replace(&from, &to).map_err(|error| {
                if error.kind() == io::ErrorKind::AlreadyExists {
                    NodeAppError::TargetExists
                } else {
                    NodeAppError::Filesystem
                }
            })
        })
        .await
    }

    async fn recycle(&self, root: &Path, operation: &NodeOperation) -> Result<(), NodeAppError> {
        let backup = self.paths(root).backup(&operation.id);
        let recycler = self.recycler.clone();
        blocking(move || {
            if !exists(&backup) {
                return Ok(());
            }
            recycler(&backup).map_err(|_| NodeAppError::Filesystem)?;
            if exists(&backup) {
                return Err(NodeAppError::Filesystem);
            }
            Ok(())
        })
        .await
    }

    async fn discard_staging(&self, root: &Path, operation: &NodeId) -> Result<(), NodeAppError> {
        let paths = self.paths(root);
        let operation = operation.clone();
        blocking(move || {
            for path in [
                paths.staging(&operation),
                paths.staging_zip(&operation),
                paths.staging_hoist(&operation),
            ] {
                remove_owned(&path);
            }
            Ok(())
        })
        .await
    }

    async fn recover(
        &self,
        root: &Path,
        operation: &NodeOperation,
    ) -> Result<RecoveryAction, NodeAppError> {
        let paths = self.paths(root);
        let op = operation.clone();
        let git_head = match (&op.prior, is_git(&op.node)) {
            (Some(prior), true) => scan::read_git(&paths.live(prior)).and_then(|m| m.commit),
            _ => None,
        };
        let recycler = self.recycler.clone();
        blocking(move || recover_blocking(&paths, &op, git_head, &recycler)).await
    }

    async fn scan(&self, root: &Path) -> Result<Vec<InstalledNode>, NodeAppError> {
        let paths = self.paths(root);
        let cn = paths.cn.clone();
        let mut nodes = blocking(move || Ok(scan::scan(&cn))).await?;
        if self.git.available(root) {
            let hooks = paths.hooks();
            if hooks.is_dir() {
                for node in &mut nodes {
                    if matches!(node.source, NodeSource::Git { .. }) && !node.single_file {
                        node.dirty = self.git.is_dirty(root, &hooks, &paths.live(node)).await;
                    }
                }
            }
        }
        Ok(nodes)
    }
}

/// Requirement and hook files of an update stub; the digest is filled in by the caller.
fn read_tree_files_only(stub: &Path) -> Result<TreeSnapshot, NodeAppError> {
    not_link(stub)?;
    let requirements = read_text(&stub.join("requirements.txt"), 256 * 1024);
    let hook = read_text(&stub.join("install.py"), 1 << 20);
    Ok(TreeSnapshot {
        digest: String::new(),
        requirements_digest: requirements.as_ref().map(|t| sha256_hex(t.as_bytes())),
        hook_digest: hook.as_ref().map(|t| sha256_hex(t.as_bytes())),
        requirement_lines: requirements
            .as_deref()
            .map(parse_requirement_lines)
            .unwrap_or_default(),
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

fn digest_of(path: &Path, node: &InstalledNode) -> Option<String> {
    if node.single_file {
        return sha256_file(path).ok().map(|d| format!("file:{d}"));
    }
    read_tree(path, is_git(node)).ok().map(|s| s.digest)
}

/// Conservative reconciliation: every action is derived from what exists on disk now, compared
/// with the identities recorded in the journal. Anything ambiguous blocks and keeps all copies.
fn recover_blocking(
    paths: &Paths,
    op: &NodeOperation,
    git_head: Option<String>,
    recycler: &Recycler,
) -> Result<RecoveryAction, NodeAppError> {
    let staging = paths.staging(&op.id);
    let backup = paths.backup(&op.id);
    let discard = |paths: &Paths| {
        remove_owned(&paths.staging(&op.id));
        remove_owned(&paths.staging_zip(&op.id));
        remove_owned(&paths.staging_hoist(&op.id));
    };
    if matches!(
        op.phase,
        OperationPhase::Verified | OperationPhase::CleanupPending
    ) {
        discard(paths);
        if exists(&backup) && (recycler(&backup).is_err() || exists(&backup)) {
            return Ok(RecoveryAction::CleanupPending);
        }
        return Ok(RecoveryAction::RolledForward);
    }
    if op.phase == OperationPhase::Prepared || op.kind == OperationKind::RetryDependencies {
        discard(paths);
        return Ok(RecoveryAction::Discarded);
    }
    paths.ensure_layout().ok();
    let restore = |from: &Path, to: &Path| rename_no_replace(from, to).is_ok();
    match op.kind {
        OperationKind::Enable | OperationKind::Disable => {
            let prior = op.prior.as_ref().ok_or(NodeAppError::Storage)?;
            let (from, to) = (
                paths.live(prior),
                paths.location(&prior.name, op.node.enabled),
            );
            Ok(match (exists(&from), exists(&to)) {
                (true, false) => RecoveryAction::RolledBack,
                (false, true) => RecoveryAction::RolledForward,
                _ => RecoveryAction::Blocked,
            })
        }
        OperationKind::Uninstall => {
            let prior = op.prior.as_ref().ok_or(NodeAppError::Storage)?;
            let live = paths.live(prior);
            Ok(match (exists(&live), exists(&backup)) {
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
                discard(paths);
                return Ok(match git_head {
                    Some(head) if head == *new => RecoveryAction::RolledForward,
                    Some(head) if head == *old => RecoveryAction::RolledBack,
                    _ => RecoveryAction::Blocked,
                });
            }
            let final_path = paths.live(&op.node);
            let (f, b, s) = (exists(&final_path), exists(&backup), exists(&staging));
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
                    discard(paths);
                    Ok(RecoveryAction::Discarded)
                }
                (false, false, _, false) => Ok(RecoveryAction::Discarded),
                (false, true, _, false) => Ok(if published() {
                    RecoveryAction::RolledForward
                } else {
                    RecoveryAction::Blocked
                }),
                (true, true, false, true) => {
                    discard(paths);
                    Ok(RecoveryAction::RolledBack)
                }
                (true, false, true, true) | (true, false, true, false) => {
                    if restore(&backup, &final_path) {
                        discard(paths);
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
