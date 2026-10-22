//! Workflow tests with in-memory fakes. The environment side reuses the M1 `World` fake machine,
//! so protected-package behavior is exercised against the same pin guard M1 tests use.
use super::*;
use crate::{
    comfy_runtime::{NodeLifecycle, RuntimeBlock, RuntimeError},
    environment::{
        EnvironmentRepository, ProvisionError, ResolutionPreview,
        tests::fakes::{ROOT, World},
    },
    shared::CancelToken,
    test_support::block_on,
};
use async_trait::async_trait;
use domain::{
    custom_nodes::*,
    environment::{DistName, EnvId, EnvState, PinSet, PythonEnvironment, PythonVersion},
    shared::model_values::SafeFileName,
};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[derive(Default)]
struct FilesState {
    prepare_policies: Vec<NodePreparePolicy>,
    requirements_text: Option<Arc<[u8]>>,
    staged: Option<StagedNode>,
    digest: String,
    requirements_digest: Option<String>,
    hook_digest: Option<String>,
    publish_error: Option<NodeAppError>,
    recover: Option<RecoveryAction>,
    recycle_fails: bool,
    scan: Vec<InstalledNode>,
    scan_error: Option<NodeAppError>,
    guard_resources: Vec<std::sync::Weak<()>>,
    scan_retained: Vec<bool>,
    snapshots: std::collections::HashMap<String, String>,
    /// Overrides the tree digest a guarded location reports (simulated drift).
    pin_digest: Option<String>,
    pin_error: Option<NodeAppError>,
    pin_attempts: Vec<(NodeId, bool)>,
    snapshot_attempts: Vec<NodeId>,
    final_snapshot: Option<Result<TreeSnapshot, NodeAppError>>,
    hook_final_digest: Option<String>,
    backup_retained: bool,
    final_snapshot_env: Option<EnvState>,
    /// Git requests stage the requested repository and commit instead of the fixed node.
    curated: bool,
    curated_warnings: Vec<String>,
    /// The `(branch, commit)` a curated preparation without a reference resolves to; it
    /// defaults to `main` at `aaaa…`.
    default_head: Option<(String, String)>,
    /// The repository a curated preparation stages instead of the requested one.
    repository_override: Option<String>,
    /// The reference of every curated Git preparation, in order.
    references: Vec<Option<String>>,
    /// One-shot preparation failures keyed by the repository folder.
    prepare_failures: std::collections::HashMap<String, NodeAppError>,
}

struct FakeFiles {
    world: Arc<World>,
    state: Mutex<FilesState>,
}

/// Records when the workflow releases a guarded location.
struct FakeGuard {
    world: Arc<World>,
    directory: PathBuf,
    snapshot: TreeSnapshot,
    resources: Arc<()>,
    requirements: Option<Arc<[u8]>>,
}

impl NodeLocationGuard for FakeGuard {
    fn directory(&self) -> &Path {
        &self.directory
    }
    fn requirements(&self) -> Option<Arc<[u8]>> {
        self.requirements.clone()
    }
    fn install_script(&self) -> Option<Arc<[u8]>> {
        self.snapshot.hook_digest.as_ref().map(|_| {
            Arc::from(
                &b"print('hook')
"[..],
            )
        })
    }
    fn snapshot(&self) -> &TreeSnapshot {
        &self.snapshot
    }
    fn retained(&self) -> crate::environment::Retained {
        crate::environment::Retained::new(self.resources.clone())
    }
}

impl Drop for FakeGuard {
    fn drop(&mut self) {
        self.world.call("guard:drop");
    }
}

#[async_trait]
impl NodeFiles for FakeFiles {
    async fn git_status(
        &self,
        root: &Path,
        node: &InstalledNode,
        _retained: crate::shared::Retained,
    ) -> Result<NodeGitStatus, NodeAppError> {
        assert_eq!(root, Path::new(ROOT));
        assert!(
            self.world
                .gate
                .busy
                .load(std::sync::atomic::Ordering::SeqCst)
        );
        self.world
            .call(&format!("files:git_status:{}", node.id.as_str()));
        Ok(NodeGitStatus {
            revision: domain::comfy_runtime::Revision {
                commit: "a".repeat(40),
                branch: Some("main".into()),
                tag: None,
            },
            state: domain::comfy_runtime::RepoState {
                dirty: true,
                status: " M requirements.txt\n".into(),
                ..Default::default()
            },
        })
    }
    async fn prepare(
        &self,
        _root: &Path,
        _operation: &NodeId,
        request: &PrepareRequest,
        _prior: Option<&InstalledNode>,
        policy: NodePreparePolicy,
        _ctx: &crate::environment::StepContext,
    ) -> Result<StagedNode, NodeAppError> {
        self.world.call("files:prepare");
        self.state.lock().unwrap().prepare_policies.push(policy);
        if let PrepareRequest::Git {
            repository,
            reference,
        } = request
        {
            let mut state = self.state.lock().unwrap();
            if state.curated {
                let repository = repository.trim_end_matches('/');
                let repository = repository.strip_suffix(".git").unwrap_or(repository);
                let folder = repository.rsplit('/').next().unwrap_or(repository);
                self.world.call(&format!("files:prepare:{folder}"));
                if let Some(error) = state.prepare_failures.remove(folder) {
                    return Err(error);
                }
                state.references.push(reference.clone());
                // A reference stages that commit detached; none resolves the default branch.
                let (branch, commit) = match reference {
                    Some(reference) => (None, reference.clone()),
                    None => state
                        .default_head
                        .clone()
                        .map(|(branch, commit)| (Some(branch), commit))
                        .unwrap_or_else(|| (Some("main".into()), "a".repeat(40))),
                };
                return Ok(StagedNode {
                    source: NodeSource::Git {
                        repository: state
                            .repository_override
                            .clone()
                            .unwrap_or_else(|| repository.to_owned()),
                        commit,
                        branch,
                    },
                    name: SafeFileName::new(folder).unwrap(),
                    tree_digest: state.digest.clone(),
                    artifact_digest: None,
                    requirement_lines: vec!["numpy".into()],
                    requirements_digest: state.requirements_digest.clone(),
                    hook_digest: state.hook_digest.clone(),
                    dependencies: Vec::new(),
                    warnings: state.curated_warnings.clone(),
                });
            }
        }
        self.state
            .lock()
            .unwrap()
            .staged
            .clone()
            .ok_or(NodeAppError::InvalidRequest)
    }
    async fn snapshot(
        &self,
        _root: &Path,
        node: &InstalledNode,
        staged: Option<&NodeId>,
        _retained: crate::shared::Retained,
    ) -> Result<TreeSnapshot, NodeAppError> {
        let mut state = self.state.lock().unwrap();
        state.snapshot_attempts.push(node.id.clone());
        if staged.is_none() && self.world.count("verify_final") > 0 {
            self.world.call("files:final_snapshot");
            state.final_snapshot_env = self
                .world
                .envs
                .lock()
                .unwrap()
                .get("env-1")
                .map(PythonEnvironment::state);
            if let Some(snapshot) = &state.final_snapshot {
                return snapshot.clone();
            }
        }
        let digest = if staged.is_none() {
            state
                .snapshots
                .get(node.name.as_str())
                .cloned()
                .unwrap_or_else(|| "tree:installed".into())
        } else {
            state.digest.clone()
        };
        Ok(TreeSnapshot {
            digest,
            requirements_digest: state.requirements_digest.clone(),
            hook_digest: state.hook_digest.clone(),
            requirement_lines: vec!["numpy".into()],
        })
    }
    fn path(&self, _root: &Path, node: &InstalledNode, staged: Option<&NodeId>) -> PathBuf {
        let base = PathBuf::from(ROOT).join("ComfyUI/custom_nodes");
        match staged {
            Some(id) => base.join(format!(".disabled/.vjump-staging-{}", id.as_str())),
            None if !node.enabled => base.join(".disabled").join(node.name.as_str()),
            None => base.join(node.name.as_str()),
        }
    }
    async fn pin_location(
        &self,
        _root: &Path,
        node: &InstalledNode,
        staged: Option<&NodeId>,
    ) -> Result<Box<dyn NodeLocationGuard>, NodeAppError> {
        let mut state = self.state.lock().unwrap();
        state.pin_attempts.push((node.id.clone(), staged.is_some()));
        if let Some(error) = &state.pin_error {
            return Err(error.clone());
        }
        self.world.call(if staged.is_some() {
            "guard:pin:staged"
        } else {
            "guard:pin:live"
        });
        let resources = Arc::new(());
        state.guard_resources.push(Arc::downgrade(&resources));
        Ok(Box::new(FakeGuard {
            world: self.world.clone(),
            directory: self.path(Path::new(ROOT), node, staged),
            snapshot: TreeSnapshot {
                digest: state
                    .pin_digest
                    .clone()
                    .or_else(|| {
                        staged
                            .is_none()
                            .then(|| state.snapshots.get(node.name.as_str()).cloned())
                            .flatten()
                    })
                    .unwrap_or_else(|| node.tree_digest.clone()),
                requirements_digest: state.requirements_digest.clone(),
                hook_digest: state.hook_digest.clone(),
                requirement_lines: vec!["numpy".into()],
            },
            resources,
            requirements: state.requirements_digest.as_ref().map(|_| {
                state
                    .requirements_text
                    .clone()
                    .unwrap_or_else(|| Arc::from(&b"numpy\n"[..]))
            }),
        }))
    }
    async fn publish(
        &self,
        _root: &Path,
        op: &NodeOperation,
        _retained: crate::shared::Retained,
    ) -> Result<(), NodeAppError> {
        self.world.call("files:publish");
        let mut state = self.state.lock().unwrap();
        match state.publish_error.take() {
            Some(error) => Err(error),
            None => {
                let digest = state.digest.clone();
                state.snapshots.insert(op.node.name.as_str().into(), digest);
                Ok(())
            }
        }
    }
    async fn backup(&self, _root: &Path, _op: &NodeOperation) -> Result<(), NodeAppError> {
        self.world.call("files:backup");
        self.state.lock().unwrap().backup_retained = true;
        Ok(())
    }
    async fn move_enabled(
        &self,
        _root: &Path,
        _prior: &InstalledNode,
        enabled: bool,
    ) -> Result<(), NodeAppError> {
        self.world.call(&format!("files:move_enabled:{enabled}"));
        Ok(())
    }
    async fn recycle(&self, _root: &Path, _op: &NodeOperation) -> Result<(), NodeAppError> {
        self.world.call("files:recycle");
        let mut state = self.state.lock().unwrap();
        if state.recycle_fails {
            Err(NodeAppError::Filesystem)
        } else {
            state.backup_retained = false;
            Ok(())
        }
    }
    async fn discard_staging(&self, _root: &Path, _op: &NodeId) -> Result<(), NodeAppError> {
        self.world.call("files:discard");
        Ok(())
    }
    async fn recover(
        &self,
        _root: &Path,
        _op: &NodeOperation,
        _retained: crate::shared::Retained,
    ) -> Result<RecoveryAction, NodeAppError> {
        self.world.call("files:recover");
        Ok(self
            .state
            .lock()
            .unwrap()
            .recover
            .unwrap_or(RecoveryAction::Blocked))
    }
    async fn scan(
        &self,
        _root: &Path,
        retained: crate::shared::Retained,
    ) -> Result<Vec<InstalledNode>, NodeAppError> {
        self.world.call("files:scan");
        let mut state = self.state.lock().unwrap();
        state.scan_retained.push(!retained.is_empty());
        match &state.scan_error {
            Some(error) => Err(error.clone()),
            None => Ok(state.scan.clone()),
        }
    }
    async fn explorer_target(
        &self,
        root: &Path,
        node: Option<&InstalledNode>,
    ) -> Result<crate::shared::ExplorerTarget, NodeAppError> {
        assert_eq!(root, Path::new(ROOT));
        let busy = self
            .world
            .gate
            .busy
            .load(std::sync::atomic::Ordering::SeqCst);
        self.world.call(&format!(
            "files:explorer_target:{}:{busy}",
            node.map_or("custom_nodes", |node| node.id.as_str())
        ));
        let cn = root.join("ComfyUI/custom_nodes");
        Ok(match node {
            None => crate::shared::ExplorerTarget::OpenFolder(cn),
            Some(node) => {
                let parent = if node.enabled {
                    cn
                } else {
                    cn.join(".disabled")
                };
                let path = parent.join(node.name.as_str());
                if node.single_file {
                    crate::shared::ExplorerTarget::RevealItem(path)
                } else {
                    crate::shared::ExplorerTarget::OpenFolder(path)
                }
            }
        })
    }
}

#[test]
fn git_status_resolves_stored_nodes_under_the_gate_and_rejects_non_git() {
    let h = Harness::new();
    let mut node = interrupted(&h, OperationPhase::Completed, OperationKind::Install).node;
    assert_eq!(
        block_on(h.service.git_status(&node.id)),
        Err(NodeAppError::NotFound)
    );
    block_on(h.store.save_node(&node)).unwrap();
    assert_eq!(
        block_on(h.service.git_status(&node.id)),
        Err(NodeAppError::InvalidRequest)
    );
    node.source = NodeSource::Git {
        repository: "https://github.com/example/node".into(),
        commit: "a".repeat(40),
        branch: Some("main".into()),
    };
    node.single_file = true;
    block_on(h.store.save_node(&node)).unwrap();
    assert_eq!(
        block_on(h.service.git_status(&node.id)),
        Err(NodeAppError::InvalidRequest)
    );
    node.single_file = false;
    block_on(h.store.save_node(&node)).unwrap();
    assert!(
        block_on(h.service.git_status(&node.id))
            .unwrap()
            .state
            .dirty
    );
    assert_eq!(h.world.calls(), ["files:git_status:n-1"]);
    let _lease = block_on(h.service.deps.environment.gate.try_acquire()).unwrap();
    assert_eq!(
        block_on(h.service.git_status(&node.id)),
        Err(NodeAppError::Busy)
    );
}

#[test]
fn locating_a_node_resolves_stored_nodes_without_the_gate() {
    use crate::shared::ExplorerTarget;
    let h = Harness::new();
    let locate = LocateNode::new(h.service.deps.clone());
    let cn = PathBuf::from(ROOT).join("ComfyUI/custom_nodes");
    assert_eq!(
        block_on(locate.execute(None)),
        Ok(ExplorerTarget::OpenFolder(cn.clone()))
    );
    let mut node = interrupted(&h, OperationPhase::Completed, OperationKind::Install).node;
    assert_eq!(
        block_on(locate.execute(Some(&node.id))),
        Err(NodeAppError::NotFound)
    );
    block_on(h.store.save_node(&node)).unwrap();
    assert_eq!(
        block_on(locate.execute(Some(&node.id))),
        Ok(ExplorerTarget::OpenFolder(cn.join(node.name.as_str())))
    );
    node.enabled = false;
    block_on(h.store.save_node(&node)).unwrap();
    assert_eq!(
        block_on(locate.execute(Some(&node.id))),
        Ok(ExplorerTarget::OpenFolder(
            cn.join(".disabled").join(node.name.as_str())
        ))
    );
    node.enabled = true;
    node.single_file = true;
    block_on(h.store.save_node(&node)).unwrap();
    // Held by someone else, the gate does not matter for a read.
    let _lease = block_on(h.service.deps.environment.gate.try_acquire()).unwrap();
    assert_eq!(
        block_on(locate.execute(Some(&node.id))),
        Ok(ExplorerTarget::RevealItem(cn.join(node.name.as_str())))
    );
    assert_eq!(
        h.world.calls(),
        [
            "files:explorer_target:custom_nodes:false".to_owned(),
            format!("files:explorer_target:{}:false", node.id.as_str()),
            format!("files:explorer_target:{}:false", node.id.as_str()),
            format!("files:explorer_target:{}:true", node.id.as_str()),
        ]
    );
}

#[test]
fn pending_reviews_exclude_direct_operations_and_restore_no_consent() {
    let h = Harness::new();
    let review = h.prepare();
    {
        let mut plans = h.service.deps.plans.0.lock().unwrap();
        let template = plans.get(&review.id).unwrap().clone();
        plans.clear();
        for (id, kind) in [
            ("review-c", OperationKind::RetryDependencies),
            ("review-a", OperationKind::Install),
            ("review-b", OperationKind::Update),
            ("direct-enable", OperationKind::Enable),
            ("direct-disable", OperationKind::Disable),
            ("direct-uninstall", OperationKind::Uninstall),
        ] {
            let mut plan = template.clone();
            plan.review.id = id.into();
            plan.review.kind = kind;
            plan.operation.kind = kind;
            plans.insert(id.into(), plan);
        }
        plans.insert(review.id.clone(), template);
    }
    let mut reviews = h.service.pending_reviews().unwrap();
    let ids: Vec<_> = reviews.iter().map(|r| r.id.clone()).collect();
    let mut expected = vec![
        review.id.clone(),
        "review-a".into(),
        "review-b".into(),
        "review-c".into(),
    ];
    expected.sort();
    assert_eq!(ids, expected);
    reviews[0].binding = "changed".into();
    assert_ne!(h.service.pending_reviews().unwrap()[0].binding, "changed");
    let mut decision = Harness::decision(&review);
    decision.consent.execute_node_code = false;
    assert_eq!(
        block_on(h.service.commit(&review.id, decision, &h.ctx())).unwrap_err(),
        NodeAppError::ConsentRequired
    );
    let mut deps = h.service.deps.clone();
    deps.plans = Arc::default();
    assert!(NodeService::new(deps).pending_reviews().unwrap().is_empty());
}

#[test]
fn recent_operations_are_terminal_newest_first_and_bounded() {
    let h = Harness::new();
    for index in 0..20 {
        let mut op = interrupted(
            &h,
            if index % 2 == 0 {
                OperationPhase::Completed
            } else {
                OperationPhase::Failed
            },
            OperationKind::Install,
        );
        op.id = NodeId::new(format!("op-{index:02}")).unwrap();
        block_on(h.store.save_operation(&op)).unwrap();
    }
    block_on(h.store.save_operation(&interrupted(
        &h,
        OperationPhase::CleanupPending,
        OperationKind::Install,
    )))
    .unwrap();
    let recent = block_on(h.service.recent_operations()).unwrap();
    assert_eq!(recent.len(), 16);
    assert_eq!(recent[0].id.as_str(), "op-19");
    assert_eq!(recent[15].id.as_str(), "op-04");
    assert!(recent.iter().all(|op| !op.blocks_start()));
}

struct FakeRegistry {
    world: Arc<World>,
}

#[async_trait]
impl RegistryClient for FakeRegistry {
    async fn search(&self, _: RegistryQuery, _: bool, _: u64) -> Result<CachedPage, NodeAppError> {
        Err(NodeAppError::Offline)
    }
    async fn node(&self, _: &str) -> Result<RegistryNode, NodeAppError> {
        Err(NodeAppError::NotFound)
    }
    async fn versions(&self, _: &str) -> Result<Vec<RegistryRelease>, NodeAppError> {
        Ok(Vec::new())
    }
    async fn install(
        &self,
        id: &str,
        exact: Option<&str>,
    ) -> Result<RegistryRelease, NodeAppError> {
        self.world.call("registry:install");
        Ok(RegistryRelease {
            node_id: id.into(),
            version: exact.unwrap_or("1.0.0").into(),
            dependencies: vec!["registry-dep>=1".into()],
        })
    }
}

#[derive(Default)]
struct StoreState {
    nodes: Vec<InstalledNode>,
    operations: Vec<NodeOperation>,
    trusts: Vec<Trust>,
    dependencies: Vec<DependencyOutcome>,
    dependency_reads: usize,
    journal_error: Option<NodeAppError>,
    nodes_error: Option<NodeAppError>,
    verified: Vec<NodeOperation>,
}

struct FakeStore {
    world: Arc<World>,
    state: Mutex<StoreState>,
}

#[async_trait]
impl NodeRepository for FakeStore {
    async fn nodes(&self) -> Result<Vec<InstalledNode>, NodeAppError> {
        let state = self.state.lock().unwrap();
        match &state.nodes_error {
            Some(error) => Err(error.clone()),
            None => Ok(state.nodes.clone()),
        }
    }
    async fn save_node(&self, node: &InstalledNode) -> Result<(), NodeAppError> {
        let mut state = self.state.lock().unwrap();
        state.nodes.retain(|n| n.id != node.id);
        state.nodes.push(node.clone());
        Ok(())
    }
    async fn remove_node(&self, id: &NodeId) -> Result<(), NodeAppError> {
        self.state.lock().unwrap().nodes.retain(|n| &n.id != id);
        Ok(())
    }
    async fn save_operation(&self, op: &NodeOperation) -> Result<(), NodeAppError> {
        self.world.call(&format!("store:{}", op.phase.as_str()));
        let mut state = self.state.lock().unwrap();
        if op.phase == OperationPhase::Verified {
            if op.consent_identity.is_some()
                && matches!(
                    op.kind,
                    OperationKind::Install
                        | OperationKind::Update
                        | OperationKind::RetryDependencies
                )
            {
                assert_eq!(
                    state.nodes.iter().find(|node| node.id == op.node.id),
                    Some(&op.node)
                );
            }
            state.verified.push(op.clone());
        }
        if let Some(saved) = state.operations.iter_mut().find(|saved| saved.id == op.id) {
            *saved = op.clone();
        } else {
            state.operations.push(op.clone());
        }
        Ok(())
    }
    async fn operations(&self) -> Result<Vec<NodeOperation>, NodeAppError> {
        let state = self.state.lock().unwrap();
        match &state.journal_error {
            Some(error) => Err(error.clone()),
            None => Ok(state.operations.clone()),
        }
    }
    async fn dependency(
        &self,
        node: &NodeId,
        environment: &EnvId,
        source_identity: &str,
    ) -> Result<Option<DependencyState>, NodeAppError> {
        let mut state = self.state.lock().unwrap();
        state.dependency_reads += 1;
        Ok(state
            .dependencies
            .iter()
            .rev()
            .find(|outcome| {
                &outcome.node == node
                    && &outcome.environment == environment
                    && outcome.source_identity == source_identity
            })
            .map(|outcome| outcome.state))
    }
    async fn save_dependency(&self, outcome: &DependencyOutcome) -> Result<(), NodeAppError> {
        self.world.call(if outcome.state == DependencyState::Ok {
            "dependency:ok"
        } else {
            "dependency:failed"
        });
        self.state
            .lock()
            .unwrap()
            .dependencies
            .push(outcome.clone());
        Ok(())
    }
    async fn trust(&self, namespace: &str) -> Result<Option<Trust>, NodeAppError> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .trusts
            .iter()
            .find(|t| t.namespace == namespace)
            .cloned())
    }
    async fn save_trust(&self, trust: &Trust) -> Result<(), NodeAppError> {
        let mut state = self.state.lock().unwrap();
        state.trusts.retain(|t| t.namespace != trust.namespace);
        state.trusts.push(trust.clone());
        Ok(())
    }
}

struct FakeHook {
    world: Arc<World>,
    files: Arc<FakeFiles>,
    fails: Mutex<bool>,
    changes: Mutex<Vec<(String, String)>>,
    cancels: Mutex<bool>,
}

#[async_trait]
impl NodeHook for FakeHook {
    async fn run(
        &self,
        _root: &Path,
        _env: &PythonEnvironment,
        location: &dyn NodeLocationGuard,
        ctx: &crate::environment::StepContext,
    ) -> Result<(), NodeAppError> {
        assert!(location.install_script().is_some());
        self.world.call("hook:run");
        for (name, version) in self.changes.lock().unwrap().iter() {
            self.world.put_installed(name, version);
        }
        {
            let mut state = self.files.state.lock().unwrap();
            state.requirements_text = Some(Arc::from(&b"hook-generated-dependency\n"[..]));
            if let Some(digest) = state.hook_final_digest.clone() {
                state.snapshots.insert("pack".into(), digest);
            }
        }
        if *self.cancels.lock().unwrap() {
            ctx.cancel.cancel();
            return Err(NodeAppError::Cancelled);
        }
        if *self.fails.lock().unwrap() {
            // A failing script may leave a protected package changed.
            self.world.put_installed("torch", "2.15.0");
            Err(NodeAppError::DependenciesFailed)
        } else {
            Ok(())
        }
    }
}

struct Harness {
    world: Arc<World>,
    files: Arc<FakeFiles>,
    store: Arc<FakeStore>,
    hook: Arc<FakeHook>,
    service: NodeService,
}

fn registry_source(version: &str) -> NodeSource {
    NodeSource::Registry {
        id: "pack".into(),
        publisher: Some("pub".into()),
        version: version.into(),
        repository: Some("https://github.com/pub/pack".into()),
    }
}

fn staged(version: &str) -> StagedNode {
    StagedNode {
        source: registry_source(version),
        name: SafeFileName::new("pack").unwrap(),
        tree_digest: format!("tree:{version}"),
        artifact_digest: Some("artifact".into()),
        requirement_lines: vec!["numpy".into()],
        requirements_digest: Some("req".into()),
        hook_digest: None,
        dependencies: vec!["registry-dep>=1".into()],
        warnings: Vec::new(),
    }
}

impl Harness {
    fn new() -> Self {
        Self::build(true)
    }

    /// `active` seeds the verified environment `env-1`; a first run starts without one.
    fn build(active: bool) -> Self {
        let world = World::new();
        if active {
            world.seed_active_env("env-1", PythonVersion::new(3, 13, 16));
        }
        let files = Arc::new(FakeFiles {
            world: world.clone(),
            state: Mutex::new(FilesState {
                staged: Some(staged("1.0.0")),
                digest: "tree:1.0.0".into(),
                requirements_digest: Some("req".into()),
                ..Default::default()
            }),
        });
        let store = Arc::new(FakeStore {
            world: world.clone(),
            state: Mutex::default(),
        });
        let hook = Arc::new(FakeHook {
            world: world.clone(),
            files: files.clone(),
            fails: Mutex::new(false),
            changes: Mutex::default(),
            cancels: Mutex::new(false),
        });
        let mut environment = world.deps();
        environment.node_requirements =
            Arc::new(NodeRequirements::new(store.clone(), files.clone()));
        let service = NodeService::new(NodeDeps {
            environment,
            registry: Arc::new(FakeRegistry {
                world: world.clone(),
            }),
            store: store.clone(),
            files: files.clone(),
            hook: hook.clone(),
            plans: Arc::default(),
        });
        Self {
            world,
            files,
            store,
            hook,
            service,
        }
    }

    fn ctx(&self) -> NodeContext {
        NodeContext::default()
    }

    fn registry_request() -> PrepareRequest {
        PrepareRequest::Registry {
            id: "pack".into(),
            version: None,
        }
    }

    fn consent(review: &PlanReview) -> InstallConsent {
        InstallConsent {
            plan_id: review.id.clone(),
            binding: review.binding.clone(),
            execute_node_code: true,
            run_install_py: false,
            allow_source_builds: false,
            acknowledge_warnings: false,
            remember_source: false,
            remember_install_py: false,
        }
    }

    fn decision(review: &PlanReview) -> CommitDecision {
        CommitDecision {
            consent: Self::consent(review),
            protected_changes: review.protected_changes.clone(),
        }
    }

    fn prepare(&self) -> PlanReview {
        block_on(self.service.prepare(Self::registry_request(), &self.ctx())).unwrap()
    }

    /// Installs the node and returns it, as the starting point of update tests.
    fn installed(&self) -> InstalledNode {
        let review = self.prepare();
        let outcome = block_on(self.service.commit(
            &review.id,
            Self::decision(&review),
            &self.ctx(),
        ))
        .unwrap();
        self.world.calls.lock().unwrap().clear();
        outcome.node
    }

    fn env(&self) -> PythonEnvironment {
        self.world.env("env-1")
    }

    fn phases(&self) -> Vec<String> {
        self.world
            .calls()
            .into_iter()
            .filter_map(|c| c.strip_prefix("store:").map(str::to_owned))
            .collect()
    }
}

#[test]
fn an_install_follows_the_phases_and_records_the_exact_source() {
    let h = Harness::new();
    let review = h.prepare();
    assert_eq!(review.source, registry_source("1.0.0"));
    assert_eq!(review.requirements, ["registry-dep>=1", "numpy"]);
    assert!(review.has_requirements && !review.has_install_py);
    assert!(!review.trusted_source);
    let outcome = block_on(
        h.service
            .commit(&review.id, Harness::decision(&review), &h.ctx()),
    )
    .unwrap();
    assert!(!outcome.cleanup_pending);
    assert_eq!(outcome.node.dependencies, DependencyState::Ok);
    assert_eq!(
        h.phases(),
        [
            "prepared",
            "prepared",
            "publish_intent",
            "published",
            "dependencies_in_progress",
            "verified",
            "cleanup_pending",
            "completed"
        ]
    );
    let calls = h.world.calls();
    let position = |name: &str| {
        calls
            .iter()
            .position(|c| c == name)
            .unwrap_or_else(|| panic!("{name} in {calls:?}"))
    };
    // The dependency intent is journaled before the first package mutation is spawned.
    assert!(position("store:dependencies_in_progress") < position("install:node"));
    assert!(position("files:publish") < position("install:node"));
    assert_eq!(
        h.world.count("hook:run"),
        0,
        "install.py never runs without explicit consent"
    );
    assert_eq!(h.env().state(), EnvState::Verified);
    let stored = block_on(h.service.list()).unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].dependencies, DependencyState::Ok);
    assert_eq!(h.store.state.lock().unwrap().dependencies.len(), 1);
}

#[test]
fn consent_must_match_the_prepared_plan_exactly() {
    let h = Harness::new();
    let review = h.prepare();
    let mut cases: Vec<(&str, CommitDecision)> = Vec::new();
    let mut wrong_plan = Harness::decision(&review);
    wrong_plan.consent.plan_id = "node-other".into();
    cases.push(("other plan", wrong_plan));
    let mut wrong_binding = Harness::decision(&review);
    wrong_binding.consent.binding = "different".into();
    cases.push(("binding", wrong_binding));
    let mut no_code_consent = Harness::decision(&review);
    no_code_consent.consent.execute_node_code = false;
    cases.push(("execution not acknowledged", no_code_consent));
    let mut builds = Harness::decision(&review);
    builds.consent.allow_source_builds = true;
    cases.push(("source builds", builds));
    let mut hook = Harness::decision(&review);
    hook.consent.run_install_py = true;
    cases.push(("hook without a hook", hook));
    let mut remember = Harness::decision(&review);
    remember.consent.remember_install_py = true;
    cases.push(("remember install.py without running it", remember));
    let mut pins = Harness::decision(&review);
    pins.protected_changes = vec![domain::environment::PinChange {
        name: DistName::new("torch").unwrap(),
        from: Some("2.14.1+cu130".into()),
        to: Some("9.9.9".into()),
    }];
    cases.push(("unreviewed pin change", pins));
    for (label, decision) in cases {
        let result = block_on(h.service.commit(&review.id, decision, &h.ctx()));
        assert_eq!(
            result.unwrap_err(),
            NodeAppError::ConsentRequired,
            "{label}"
        );
    }
    assert_eq!(
        h.world.count("files:publish"),
        0,
        "a rejected consent mutates nothing"
    );
    assert_eq!(h.world.count("install:node"), 0);
    // The plan is still reviewable and commits with the right consent exactly once.
    block_on(
        h.service
            .commit(&review.id, Harness::decision(&review), &h.ctx()),
    )
    .unwrap();
    let again = block_on(
        h.service
            .commit(&review.id, Harness::decision(&review), &h.ctx()),
    );
    assert_eq!(again.unwrap_err(), NodeAppError::StalePlan);
}

#[test]
fn warnings_need_acknowledgement_and_unknown_plans_are_stale() {
    let h = Harness::new();
    h.files
        .state
        .lock()
        .unwrap()
        .staged
        .as_mut()
        .unwrap()
        .warnings = vec!["OS is incompatible".into()];
    let review = h.prepare();
    assert_eq!(review.warnings, ["OS is incompatible"]);
    let result = block_on(
        h.service
            .commit(&review.id, Harness::decision(&review), &h.ctx()),
    );
    assert_eq!(result.unwrap_err(), NodeAppError::ConsentRequired);
    let mut accepted = Harness::decision(&review);
    accepted.consent.acknowledge_warnings = true;
    block_on(h.service.commit(&review.id, accepted, &h.ctx())).unwrap();
    let unknown = block_on(
        h.service
            .commit("node-missing", Harness::decision(&review), &h.ctx()),
    );
    assert_eq!(unknown.unwrap_err(), NodeAppError::StalePlan);
}

#[test]
fn a_changed_tree_environment_or_pin_set_makes_the_plan_stale() {
    let h = Harness::new();
    // The staged tree changed after review.
    let review = h.prepare();
    h.files.state.lock().unwrap().digest = "tree:tampered".into();
    let result = block_on(
        h.service
            .commit(&review.id, Harness::decision(&review), &h.ctx()),
    );
    assert_eq!(result.unwrap_err(), NodeAppError::StalePlan);
    assert_eq!(h.world.count("files:publish"), 0);
    // The protected pins changed after review.
    h.files.state.lock().unwrap().digest = "tree:1.0.0".into();
    let review = h.prepare();
    let mut env = h.env();
    let mut pins = PinSet::new();
    pins.insert(DistName::new("torch").unwrap(), "2.15.0");
    env.record_pins(pins, "2026-10-16T00:00:00Z");
    block_on(EnvironmentRepository::save(&*h.world, &env)).unwrap();
    let result = block_on(
        h.service
            .commit(&review.id, Harness::decision(&review), &h.ctx()),
    );
    assert_eq!(result.unwrap_err(), NodeAppError::StalePlan);
    // A prior node whose files changed makes an update stale.
    let h = Harness::new();
    let node = h.installed();
    h.files.state.lock().unwrap().staged = Some(staged("1.1.0"));
    h.files.state.lock().unwrap().digest = "tree:1.1.0".into();
    let review = block_on(h.service.prepare(
        PrepareRequest::Update {
            node: node.id.clone(),
            reference: None,
        },
        &h.ctx(),
    ))
    .unwrap();
    h.files
        .state
        .lock()
        .unwrap()
        .snapshots
        .insert("pack".into(), "tree:edited-by-hand".into());
    let result = block_on(
        h.service
            .commit(&review.id, Harness::decision(&review), &h.ctx()),
    );
    assert_eq!(result.unwrap_err(), NodeAppError::StalePlan);
}

#[test]
fn node_operations_need_a_stopped_runtime_the_gate_and_a_verified_environment() {
    let h = Harness::new();
    *h.world.runtime_block.lock().unwrap() = Some(crate::comfy_runtime::RuntimeBlock::Running);
    let result = block_on(h.service.prepare(Harness::registry_request(), &h.ctx()));
    assert_eq!(result.unwrap_err(), NodeAppError::RuntimeRunning);
    *h.world.runtime_block.lock().unwrap() = Some(crate::comfy_runtime::RuntimeBlock::CannotAssess);
    let result = block_on(h.service.prepare(Harness::registry_request(), &h.ctx()));
    assert_eq!(
        result.unwrap_err(),
        NodeAppError::RuntimeRunning,
        "an unknown state is not stopped"
    );
    *h.world.runtime_block.lock().unwrap() = None;
    // The runtime starts after the plan was made.
    let review = h.prepare();
    *h.world.runtime_block.lock().unwrap() = Some(crate::comfy_runtime::RuntimeBlock::Running);
    let result = block_on(
        h.service
            .commit(&review.id, Harness::decision(&review), &h.ctx()),
    );
    assert_eq!(result.unwrap_err(), NodeAppError::RuntimeRunning);
    assert_eq!(h.world.count("files:publish"), 0);
    *h.world.runtime_block.lock().unwrap() = None;
    // Another maintenance operation holds the gate.
    h.world
        .gate
        .busy
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let result = block_on(h.service.prepare(Harness::registry_request(), &h.ctx()));
    assert_eq!(result.unwrap_err(), NodeAppError::Busy);
    h.world
        .gate
        .busy
        .store(false, std::sync::atomic::Ordering::SeqCst);
    // A damaged environment refuses installs.
    let mut env = h.env();
    env.mark_repair_required("2026-10-16T00:00:00Z").unwrap();
    block_on(EnvironmentRepository::save(&*h.world, &env)).unwrap();
    let result = block_on(h.service.prepare(Harness::registry_request(), &h.ctx()));
    assert_eq!(result.unwrap_err(), NodeAppError::EnvironmentUnavailable);
}

#[test]
fn a_dependency_failure_marks_the_node_and_the_environment_and_keeps_the_backup() {
    let h = Harness::new();
    let node = h.installed();
    h.files.state.lock().unwrap().staged = Some(staged("1.1.0"));
    h.files.state.lock().unwrap().digest = "tree:1.1.0".into();
    let review = block_on(h.service.prepare(
        PrepareRequest::Update {
            node: node.id.clone(),
            reference: None,
        },
        &h.ctx(),
    ))
    .unwrap();
    h.world.fail(
        "install:node",
        ProvisionError::Failed("pip exploded".into()),
    );
    let result = block_on(
        h.service
            .commit(&review.id, Harness::decision(&review), &h.ctx()),
    );
    assert_eq!(result.unwrap_err(), NodeAppError::DependenciesFailed);
    assert_eq!(h.env().state(), EnvState::RepairRequired);
    let stored = block_on(h.service.list()).unwrap();
    assert_eq!(stored[0].dependencies, DependencyState::Failed);
    assert_eq!(
        stored[0].source,
        registry_source("1.1.0"),
        "the replacement stays as deps-failed"
    );
    assert_eq!(h.world.count("files:backup"), 1);
    assert_eq!(
        h.world.count("files:recycle"),
        0,
        "the recovery backup is retained"
    );
    let op = block_on(h.service.operation_status(review.id.as_str())).unwrap();
    assert_eq!(op.phase, OperationPhase::Failed);
    let outcomes = h.store.state.lock().unwrap().dependencies.clone();
    assert_eq!(outcomes.last().unwrap().state, DependencyState::Failed);
    assert_eq!(
        outcomes.last().unwrap().environment,
        EnvId::new("env-1").unwrap()
    );
    assert_eq!(
        outcomes.last().unwrap().source_identity,
        op.node.dependency_identity()
    );
    // Nothing else can be installed until the environment is repaired.
    let blocked = block_on(h.service.prepare(Harness::registry_request(), &h.ctx()));
    assert_eq!(blocked.unwrap_err(), NodeAppError::EnvironmentUnavailable);
}

#[test]
fn cancellation_after_mutation_began_leaves_repair_required() {
    let h = Harness::new();
    let review = h.prepare();
    h.world.fail("install:node", ProvisionError::Cancelled);
    let ctx = h.ctx();
    let result = block_on(
        h.service
            .commit(&review.id, Harness::decision(&review), &ctx),
    );
    assert!(result.is_err());
    assert!(ctx.cancel.is_cancelled());
    assert_eq!(h.env().state(), EnvState::RepairRequired);
    // Cancelling before the commit touches nothing.
    let h = Harness::new();
    let review = h.prepare();
    let ctx = NodeContext {
        cancel: CancelToken::new(),
        ..Default::default()
    };
    ctx.cancel.cancel();
    let result = block_on(
        h.service
            .commit(&review.id, Harness::decision(&review), &ctx),
    );
    assert_eq!(result.unwrap_err(), NodeAppError::Cancelled);
    assert_eq!(h.world.count("files:publish"), 0);
    assert_eq!(h.env().state(), EnvState::Verified);
}

#[test]
fn protected_pins_are_reasserted_after_packages_and_after_a_failing_script() {
    let h = Harness::new();
    h.files
        .state
        .lock()
        .unwrap()
        .staged
        .as_mut()
        .unwrap()
        .hook_digest = Some("hook".into());
    h.files.state.lock().unwrap().hook_digest = Some("hook".into());
    let review = h.prepare();
    assert!(review.has_install_py);
    assert!(review.warnings.iter().any(|w| w.contains("install.py")));
    // Requirements drift torch; the pin guard restores it.
    h.world
        .drift_on_requirements
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let mut decision = Harness::decision(&review);
    decision.consent.acknowledge_warnings = true;
    decision.consent.run_install_py = true;
    // The script fails, but pins are still reasserted before the failure is reported.
    *h.hook.fails.lock().unwrap() = true;
    let result = block_on(h.service.commit(&review.id, decision, &h.ctx()));
    assert_eq!(result.unwrap_err(), NodeAppError::DependenciesFailed);
    let calls = h.world.calls();
    let hook = calls.iter().position(|c| c == "hook:run").unwrap();
    assert!(
        calls[hook..].iter().any(|c| c == "list"),
        "pins are checked after the script: {calls:?}"
    );
    assert_eq!(h.env().state(), EnvState::RepairRequired);
    let pins = h.world.installed.lock().unwrap().clone();
    assert!(
        pins.iter()
            .any(|p| p.name == "torch" && p.version == "2.14.1+cu130"),
        "{pins:?}"
    );
}

#[test]
fn install_py_runs_only_with_consent_before_requirements_and_trust_is_namespaced() {
    let h = Harness::new();
    h.files
        .state
        .lock()
        .unwrap()
        .staged
        .as_mut()
        .unwrap()
        .hook_digest = Some("hook".into());
    h.files.state.lock().unwrap().hook_digest = Some("hook".into());
    let review = h.prepare();
    let mut decision = Harness::decision(&review);
    decision.consent.acknowledge_warnings = true;
    decision.consent.run_install_py = true;
    decision.consent.remember_source = true;
    decision.consent.remember_install_py = true;
    block_on(h.service.commit(&review.id, decision, &h.ctx())).unwrap();
    let calls = h.world.calls();
    assert!(
        calls.iter().position(|c| c == "hook:run").unwrap()
            < calls.iter().position(|c| c == "install:node").unwrap()
    );
    // The remembered permission belongs to this publisher/node/repository only.
    let remembered = registry_source("1.0.0").trust_namespace().unwrap();
    assert!(
        h.store
            .state
            .lock()
            .unwrap()
            .trusts
            .iter()
            .any(|t| t.namespace == remembered && t.allow_install_py)
    );
    let other = NodeSource::Registry {
        id: "other".into(),
        publisher: Some("pub".into()),
        version: "1".into(),
        repository: Some("https://github.com/pub/pack".into()),
    };
    assert!(
        h.store
            .state
            .lock()
            .unwrap()
            .trusts
            .iter()
            .all(|t| Some(&t.namespace) != other.trust_namespace().as_ref())
    );
    // A second plan for the same source shows the remembered decision but still needs consent.
    let again = block_on(h.service.prepare(
        PrepareRequest::Registry {
            id: "pack".into(),
            version: None,
        },
        &h.ctx(),
    ))
    .unwrap();
    assert!(again.trusted_source && again.trusted_install_py);
    let result = block_on(h.service.commit(
        &again.id,
        CommitDecision {
            consent: InstallConsent {
                execute_node_code: false,
                ..Harness::consent(&again)
            },
            protected_changes: Vec::new(),
        },
        &h.ctx(),
    ));
    assert_eq!(result.unwrap_err(), NodeAppError::ConsentRequired);
}

fn torch_preview(version: &str) -> ResolutionPreview {
    ResolutionPreview {
        installs: vec![crate::environment::InstalledPackage {
            name: "torch".into(),
            version: version.into(),
        }],
        removals: Vec::new(),
    }
}

#[test]
fn node_previews_refuse_protected_package_changes() {
    // A preview that would move a protected package is never offered as a node plan.
    let h = Harness::new();
    h.world
        .previews
        .lock()
        .unwrap()
        .push_back(Ok(torch_preview("2.15.0")));
    let result = block_on(h.service.prepare(Harness::registry_request(), &h.ctx()));
    assert_eq!(result.unwrap_err(), NodeAppError::ResolutionFailed);
    assert!(h.service.pending_reviews().unwrap().is_empty());
    assert_eq!(h.world.count("install:node"), 0);
    assert_eq!(h.world.count("files:publish"), 0);
    assert_eq!(
        h.world.count("files:discard"),
        1,
        "the abandoned plan removes its owned staging"
    );
    // Removing a protected package is refused the same way.
    let h = Harness::new();
    h.world
        .previews
        .lock()
        .unwrap()
        .push_back(Ok(ResolutionPreview {
            installs: Vec::new(),
            removals: vec![crate::environment::InstalledPackage {
                name: "torch".into(),
                version: "2.14.1+cu130".into(),
            }],
        }));
    let result = block_on(h.service.prepare(Harness::registry_request(), &h.ctx()));
    assert_eq!(result.unwrap_err(), NodeAppError::ResolutionFailed);
    // A preview that keeps every protected package as recorded is a normal plan, and the commit
    // refuses a confirmation that claims a protected-package change.
    let h = Harness::new();
    let review = h.prepare();
    assert!(review.protected_changes.is_empty());
    let mut claimed = Harness::decision(&review);
    claimed.protected_changes = vec![domain::environment::PinChange {
        name: DistName::new("torch").unwrap(),
        from: Some("2.14.1+cu130".into()),
        to: Some("2.15.0".into()),
    }];
    let result = block_on(h.service.commit(&review.id, claimed, &h.ctx()));
    assert_eq!(result.unwrap_err(), NodeAppError::ConsentRequired);
    assert_eq!(h.world.count("files:publish"), 0);
    block_on(
        h.service
            .commit(&review.id, Harness::decision(&review), &h.ctx()),
    )
    .unwrap();
    assert_eq!(
        h.env().pins.get(&DistName::new("torch").unwrap()),
        Some("2.14.1+cu130")
    );
    // An invalid retained review cannot authorize a protected change either.
    let h = Harness::new();
    let review = h.prepare();
    h.service
        .deps
        .plans
        .0
        .lock()
        .unwrap()
        .get_mut(&review.id)
        .unwrap()
        .review
        .protected_changes
        .push(domain::environment::PinChange {
            name: DistName::new("torch").unwrap(),
            from: Some("2.14.1+cu130".into()),
            to: Some("2.15.0".into()),
        });
    assert_eq!(
        block_on(
            h.service
                .commit(&review.id, Harness::decision(&review), &h.ctx())
        )
        .unwrap_err(),
        NodeAppError::ResolutionFailed
    );
    assert_eq!(h.world.count("files:publish"), 0);
    assert_eq!(h.world.count("install:node"), 0);
}

#[test]
fn node_preview_and_commit_keep_the_recorded_profile_revision_constraints() {
    let h = Harness::new();
    let extras = vec!["transformers==5.5.0".to_owned()];
    {
        let mut profiles = h.world.profiles.lock().unwrap();
        assert_eq!(h.env().profile_revision, profiles[0].revision);
        profiles[0].constraints = extras.clone();
        let mut current = profiles[0].clone();
        current.revision += 1;
        current.constraints = vec!["transformers==5.20.2".into()];
        profiles.insert(0, current);
    }
    let review = h.prepare();
    assert_eq!(
        h.world.constraint_extras.lock().unwrap().as_slice(),
        std::slice::from_ref(&extras)
    );
    block_on(
        h.service
            .commit(&review.id, Harness::decision(&review), &h.ctx()),
    )
    .unwrap();
    assert_eq!(
        *h.world.constraint_extras.lock().unwrap(),
        [extras.clone(), extras]
    );
}

#[test]
fn node_preview_errors_when_the_recorded_profile_revision_is_missing() {
    let h = Harness::new();
    h.world.profiles.lock().unwrap()[0].revision += 1;
    assert_eq!(
        block_on(h.service.prepare(Harness::registry_request(), &h.ctx())).unwrap_err(),
        NodeAppError::EnvironmentUnavailable
    );
    assert!(h.world.constraint_extras.lock().unwrap().is_empty());
    assert_eq!(h.world.count("preview:constrained"), 0);
    assert_eq!(h.world.count("install:node"), 0);
}

#[test]
fn node_preview_never_retries_without_constraints() {
    let h = Harness::new();
    {
        let mut previews = h.world.previews.lock().unwrap();
        previews.push_back(Err(ProvisionError::Failed("conflicting torch".into())));
        // An unconstrained retry would consume this and authorize a torch change.
        previews.push_back(Ok(torch_preview("2.15.0")));
    }
    let result = block_on(h.service.prepare(Harness::registry_request(), &h.ctx()));
    assert_eq!(result.unwrap_err(), NodeAppError::ResolutionFailed);
    assert_eq!(h.world.count("preview:constrained"), 1);
    assert_eq!(h.world.count("preview:unconstrained"), 0);
    assert_eq!(h.world.previews.lock().unwrap().len(), 1);
    assert_eq!(h.world.count("install:node"), 0);
    assert_eq!(h.world.count("files:publish"), 0);
    assert!(h.service.pending_reviews().unwrap().is_empty());
    let installed = h.world.installed.lock().unwrap().clone();
    assert!(
        installed
            .iter()
            .any(|p| p.name == "torch" && p.version == "2.14.1+cu130"),
        "{installed:?}"
    );
}

#[test]
fn retrying_dependencies_cannot_clear_an_unrelated_repair_requirement() {
    let h = Harness::new();
    let node = h.installed();
    let mut env = h.env();
    env.mark_repair_required("2026-10-16T00:00:00Z").unwrap();
    block_on(EnvironmentRepository::save(&*h.world, &env)).unwrap();
    let review = block_on(h.service.prepare(
        PrepareRequest::RetryDependencies {
            node: node.id.clone(),
        },
        &h.ctx(),
    ))
    .unwrap();
    let outcome = block_on(
        h.service
            .commit(&review.id, Harness::decision(&review), &h.ctx()),
    )
    .unwrap();
    assert_eq!(outcome.node.dependencies, DependencyState::Ok);
    assert_eq!(
        h.env().state(),
        EnvState::RepairRequired,
        "only environment repair clears it"
    );
    // Uninstall and disable alone never clear it either.
    let review = block_on(h.service.prepare(
        PrepareRequest::SetEnabled {
            node: node.id.clone(),
            enabled: false,
        },
        &h.ctx(),
    ))
    .unwrap();
    block_on(
        h.service
            .commit(&review.id, Harness::decision(&review), &h.ctx()),
    )
    .unwrap();
    assert_eq!(h.env().state(), EnvState::RepairRequired);
}

#[test]
fn enable_disable_and_uninstall_use_the_journal_and_recycle_the_backup() {
    let h = Harness::new();
    let node = h.installed();
    let review = block_on(h.service.prepare(
        PrepareRequest::SetEnabled {
            node: node.id.clone(),
            enabled: false,
        },
        &h.ctx(),
    ))
    .unwrap();
    let outcome = block_on(
        h.service
            .commit(&review.id, Harness::decision(&review), &h.ctx()),
    )
    .unwrap();
    assert!(!outcome.node.enabled);
    assert_eq!(h.world.count("files:move_enabled:false"), 1);
    assert_eq!(
        h.world.count("install:node"),
        0,
        "toggling never touches packages"
    );
    let review = block_on(h.service.prepare(
        PrepareRequest::Uninstall {
            node: node.id.clone(),
        },
        &h.ctx(),
    ))
    .unwrap();
    assert_eq!(review.kind, OperationKind::Uninstall);
    block_on(
        h.service
            .commit(&review.id, Harness::decision(&review), &h.ctx()),
    )
    .unwrap();
    assert!(block_on(h.service.list()).unwrap().is_empty());
    // Every completed operation asks the adapter to recycle its backup (a no-op without one).
    assert_eq!(h.world.count("files:recycle"), 2);
    assert_eq!(
        h.world.count("uninstall"),
        0,
        "uninstalling a node leaves its packages in place"
    );
    // A recycle failure keeps the operation pending without failing the uninstall.
    let h = Harness::new();
    let node = h.installed();
    h.files.state.lock().unwrap().recycle_fails = true;
    let review = block_on(h.service.prepare(
        PrepareRequest::Uninstall {
            node: node.id.clone(),
        },
        &h.ctx(),
    ))
    .unwrap();
    let outcome = block_on(
        h.service
            .commit(&review.id, Harness::decision(&review), &h.ctx()),
    )
    .unwrap();
    assert!(outcome.cleanup_pending);
    let op = block_on(h.service.operation_status(review.id.as_str())).unwrap();
    assert_eq!(op.phase, OperationPhase::CleanupPending);
    assert!(
        block_on(h.service.ensure_reconciled()).is_ok(),
        "a retained backup does not block start"
    );
}

#[test]
fn a_failed_publish_is_settled_from_the_disk_state() {
    let h = Harness::new();
    let review = h.prepare();
    h.files.state.lock().unwrap().publish_error = Some(NodeAppError::TargetExists);
    h.files.state.lock().unwrap().recover = Some(RecoveryAction::Discarded);
    let result = block_on(
        h.service
            .commit(&review.id, Harness::decision(&review), &h.ctx()),
    );
    assert_eq!(result.unwrap_err(), NodeAppError::TargetExists);
    assert_eq!(h.world.count("files:recover"), 1);
    let op = block_on(h.service.operation_status(review.id.as_str())).unwrap();
    assert_eq!(op.phase, OperationPhase::Failed);
    assert!(block_on(h.service.list()).unwrap().is_empty());
    // An ambiguous disk state blocks the next operation and ComfyUI start until resolved.
    let h = Harness::new();
    let review = h.prepare();
    h.files.state.lock().unwrap().publish_error = Some(NodeAppError::Filesystem);
    h.files.state.lock().unwrap().recover = Some(RecoveryAction::Blocked);
    let _ = block_on(
        h.service
            .commit(&review.id, Harness::decision(&review), &h.ctx()),
    );
    assert_eq!(
        block_on(h.service.ensure_reconciled()).unwrap_err(),
        NodeAppError::RecoveryRequired
    );
    let blocked = block_on(h.service.prepare(Harness::registry_request(), &h.ctx()));
    assert_eq!(blocked.unwrap_err(), NodeAppError::RecoveryRequired);
}

#[test]
fn git_post_apply_refusals_keep_dependencies_and_hooks_unexecuted() {
    for error in [
        NodeAppError::StalePlan,
        NodeAppError::UnsafeSource,
        NodeAppError::GitRefused {
            status: " M nodes/code.py".into(),
        },
    ] {
        let h = Harness::new();
        let source = |commit: String| NodeSource::Git {
            repository: "https://github.com/pub/pack".into(),
            commit,
            branch: Some("main".into()),
        };
        h.files
            .state
            .lock()
            .unwrap()
            .staged
            .as_mut()
            .unwrap()
            .source = source("a".repeat(40));
        let installed = h.installed();
        {
            let mut state = h.files.state.lock().unwrap();
            let mut target = staged("1.1.0");
            target.source = source("b".repeat(40));
            target.hook_digest = Some("hook".into());
            state.staged = Some(target);
            state.digest = "tree:1.1.0".into();
            state.hook_digest = Some("hook".into());
            state.publish_error = Some(error.clone());
            state.recover = Some(RecoveryAction::Blocked);
        }
        let review = block_on(h.service.prepare(
            PrepareRequest::Update {
                node: installed.id,
                reference: None,
            },
            &h.ctx(),
        ))
        .unwrap();
        let mut decision = Harness::decision(&review);
        decision.consent.run_install_py = true;
        decision.consent.acknowledge_warnings = true;
        assert_eq!(
            block_on(h.service.commit(&review.id, decision, &h.ctx())).unwrap_err(),
            error
        );
        assert_eq!(h.world.count("files:publish"), 1);
        assert_eq!(h.world.count("install:node"), 0);
        assert_eq!(h.world.count("hook:run"), 0);
        assert!(!h.phases().iter().any(|p| p == "dependencies_in_progress"));
        assert_eq!(block_on(h.service.unresolved()).unwrap().len(), 1);
        assert_eq!(
            block_on(h.service.ensure_reconciled()).unwrap_err(),
            NodeAppError::RecoveryRequired
        );
        assert_eq!(h.env().state(), EnvState::Verified);
    }
}

fn interrupted(h: &Harness, phase: OperationPhase, kind: OperationKind) -> NodeOperation {
    let node = InstalledNode {
        id: NodeId::new("n-1").unwrap(),
        name: SafeFileName::new("pack").unwrap(),
        source: registry_source("1.0.0"),
        origin: ProvenanceOrigin::App,
        enabled: true,
        single_file: false,
        tree_digest: "tree:1.0.0".into(),
        dependencies: DependencyState::NotRun,
        conflict: false,
        dirty: None,
    };
    let prior = matches!(kind, OperationKind::Update | OperationKind::Uninstall).then(|| {
        let mut p = node.clone();
        p.source = registry_source("0.9.0");
        p
    });
    let env = h.env();
    NodeOperation {
        id: NodeId::new("node-int").unwrap(),
        kind,
        phase,
        node,
        prior,
        environment: env.id.clone(),
        pins: env.pins.clone(),
        binding: "b".into(),
        artifact_digest: None,
        requirements_digest: None,
        hook_digest: None,
        consent_identity: None,
        failure: None,
        staging: ".disabled/.vjump-staging-node-int".into(),
        backup: ".disabled/.vjump-backup-node-int".into(),
        dependency_requirements: Vec::new(),
    }
}

#[test]
fn startup_reconciliation_applies_the_file_outcome_to_the_records() {
    // Interrupted dependency mutation: damaged environment, deps-failed node, no file guess.
    let h = Harness::new();
    h.files.state.lock().unwrap().recover = Some(RecoveryAction::RolledForward);
    let op = interrupted(
        &h,
        OperationPhase::DependenciesInProgress,
        OperationKind::Install,
    );
    block_on(h.store.save_operation(&op)).unwrap();
    block_on(h.service.reconcile()).unwrap();
    assert_eq!(h.env().state(), EnvState::RepairRequired);
    let node = &block_on(h.service.list()).unwrap()[0];
    assert_eq!(node.dependencies, DependencyState::Failed);
    assert_eq!(h.world.count("files:recover"), 1);
    // Rolled back: the previous record returns; a failed install leaves no record.
    let h = Harness::new();
    h.files.state.lock().unwrap().recover = Some(RecoveryAction::RolledBack);
    let op = interrupted(&h, OperationPhase::OldBackedUp, OperationKind::Update);
    block_on(h.store.save_operation(&op)).unwrap();
    block_on(h.store.save_node(&op.node)).unwrap();
    block_on(h.service.reconcile()).unwrap();
    assert_eq!(
        block_on(h.service.list()).unwrap()[0].source,
        registry_source("0.9.0")
    );
    assert_eq!(
        block_on(h.service.operation_status("node-int"))
            .unwrap()
            .phase,
        OperationPhase::Failed
    );
    // Rolled forward before dependencies ran: the node stays, with dependencies not run.
    let h = Harness::new();
    h.files.state.lock().unwrap().recover = Some(RecoveryAction::RolledForward);
    let op = interrupted(&h, OperationPhase::Published, OperationKind::Install);
    block_on(h.store.save_operation(&op)).unwrap();
    block_on(h.service.reconcile()).unwrap();
    let node = &block_on(h.service.list()).unwrap()[0];
    assert_eq!(node.dependencies, DependencyState::NotRun);
    let status = block_on(h.service.operation_status("node-int")).unwrap();
    assert_eq!(status.phase, OperationPhase::Failed);
    assert!(status.failure.unwrap().contains("Retry dependencies"));
    // A verified operation completes forward.
    let h = Harness::new();
    h.files.state.lock().unwrap().recover = Some(RecoveryAction::RolledForward);
    let op = interrupted(&h, OperationPhase::CleanupPending, OperationKind::Uninstall);
    block_on(h.store.save_operation(&op)).unwrap();
    block_on(h.store.save_node(&op.node)).unwrap();
    block_on(h.service.reconcile()).unwrap();
    assert!(block_on(h.service.list()).unwrap().is_empty());
    assert_eq!(
        block_on(h.service.operation_status("node-int"))
            .unwrap()
            .phase,
        OperationPhase::Completed
    );
    // An ambiguous state blocks and keeps its phase.
    let h = Harness::new();
    let op = interrupted(&h, OperationPhase::PublishIntent, OperationKind::Install);
    block_on(h.store.save_operation(&op)).unwrap();
    assert_eq!(
        block_on(h.service.reconcile()).unwrap_err(),
        NodeAppError::RecoveryRequired
    );
    assert_eq!(block_on(h.service.unresolved()).unwrap().len(), 1);
    // A prepared plan that is still in this process is not discarded by a rescan.
    let h = Harness::new();
    let review = h.prepare();
    block_on(h.service.reconcile()).unwrap();
    assert_eq!(h.world.count("files:recover"), 0, "{}", review.id);
}

#[test]
fn discarding_a_plan_removes_its_staging_and_a_later_commit_is_stale() {
    let h = Harness::new();
    let review = h.prepare();
    block_on(h.service.discard(&review.id)).unwrap();
    assert_eq!(h.world.count("files:discard"), 1);
    let result = block_on(
        h.service
            .commit(&review.id, Harness::decision(&review), &h.ctx()),
    );
    assert_eq!(result.unwrap_err(), NodeAppError::StalePlan);
    assert_eq!(
        block_on(h.service.discard(&review.id)).unwrap_err(),
        NodeAppError::StalePlan
    );
    // A failed preparation cleans up what it staged.
    let h = Harness::new();
    for _ in 0..2 {
        h.world
            .previews
            .lock()
            .unwrap()
            .push_back(Err(ProvisionError::Failed("no resolution".into())));
    }
    let result = block_on(h.service.prepare(Harness::registry_request(), &h.ctx()));
    assert!(result.is_err());
    assert!(h.world.count("files:discard") >= 1);
}

#[test]
fn update_all_runs_sequentially_and_stops_when_the_environment_is_damaged() {
    let h = Harness::new();
    let first = h.installed();
    let mut second_node = first.clone();
    second_node.id = NodeId::new("n-2").unwrap();
    second_node.name = SafeFileName::new("second").unwrap();
    block_on(h.store.save_node(&second_node)).unwrap();
    let mut third_node = first.clone();
    third_node.id = NodeId::new("n-3").unwrap();
    third_node.name = SafeFileName::new("third").unwrap();
    block_on(h.store.save_node(&third_node)).unwrap();
    h.files.state.lock().unwrap().staged = Some(staged("1.1.0"));
    h.files.state.lock().unwrap().digest = "tree:1.1.0".into();
    let mut reviews = Vec::new();
    for node in [&first, &second_node, &third_node] {
        reviews.push(
            block_on(h.service.prepare(
                PrepareRequest::Update {
                    node: node.id.clone(),
                    reference: None,
                },
                &h.ctx(),
            ))
            .unwrap(),
        );
    }
    // Success of the first, dependency failure of the second, the third never runs.
    h.world.succeed_first("install:node", 1);
    h.world
        .fail("install:node", ProvisionError::Failed("resolver".into()));
    let decisions = reviews
        .iter()
        .map(|r| (r.id.clone(), Harness::decision(r)))
        .collect();
    let batch = block_on(h.service.update_all(decisions, &h.ctx())).unwrap();
    assert_eq!(batch.results.len(), 3);
    assert!(batch.results[0].1.is_ok());
    assert_eq!(
        batch.results[1].1.as_ref().unwrap_err(),
        &NodeAppError::DependenciesFailed
    );
    assert_eq!(
        batch.results[2].1.as_ref().unwrap_err(),
        &NodeAppError::Cancelled
    );
    assert!(batch.stopped_for_repair);
    assert_eq!(h.env().state(), EnvState::RepairRequired);
    // Only update plans are accepted, and none is consumed by a rejected batch.
    let h = Harness::new();
    let review = h.prepare();
    let result = block_on(h.service.update_all(
        vec![(review.id.clone(), Harness::decision(&review))],
        &h.ctx(),
    ));
    assert_eq!(result.unwrap_err(), NodeAppError::StalePlan);
}

#[test]
fn rescan_keeps_ids_flags_provenance_conflicts_and_prunes_missing_nodes() {
    let h = Harness::new();
    let stored = h.installed();
    let mut gone = stored.clone();
    gone.id = NodeId::new("n-gone").unwrap();
    gone.name = SafeFileName::new("gone").unwrap();
    block_on(h.store.save_node(&gone)).unwrap();
    let observed = |name: &str, source: NodeSource| InstalledNode {
        id: NodeId::new("scan-0123456789abcdef").unwrap(),
        name: SafeFileName::new(name).unwrap(),
        source,
        origin: ProvenanceOrigin::ExternallyObserved,
        enabled: true,
        single_file: false,
        tree_digest: String::new(),
        dependencies: DependencyState::NotRun,
        conflict: false,
        dirty: None,
    };
    // Manager moved the version: the observed revision is recorded, dependencies need a rerun.
    let moved = NodeSource::Registry {
        id: "pack".into(),
        publisher: None,
        version: "2.0.0".into(),
        repository: None,
    };
    h.files.state.lock().unwrap().scan = vec![
        observed("pack", moved),
        observed("manual", NodeSource::Manual),
    ];
    let nodes = block_on(h.service.rescan()).unwrap();
    assert_eq!(nodes.len(), 2);
    let pack = nodes.iter().find(|n| n.name.as_str() == "pack").unwrap();
    assert_eq!(pack.id, stored.id, "the stored id is kept");
    assert_eq!(pack.origin, ProvenanceOrigin::App);
    assert_eq!(pack.source, registry_source("2.0.0"));
    assert_eq!(pack.dependencies, DependencyState::NotRun);
    assert!(!pack.conflict);
    assert!(
        block_on(h.service.list())
            .unwrap()
            .iter()
            .all(|n| n.name.as_str() != "gone")
    );
    // A different origin on disk is a conflict; the stored record stays authoritative.
    let other = NodeSource::Git {
        repository: "https://github.com/evil/pack".into(),
        commit: "a".repeat(40),
        branch: None,
    };
    h.files.state.lock().unwrap().scan = vec![observed("pack", other)];
    let nodes = block_on(h.service.rescan()).unwrap();
    assert!(nodes[0].conflict);
    assert_eq!(nodes[0].source, registry_source("2.0.0"));
    // A node in conflict cannot be updated or have its dependencies retried.
    for request in [
        PrepareRequest::Update {
            node: nodes[0].id.clone(),
            reference: None,
        },
        PrepareRequest::RetryDependencies {
            node: nodes[0].id.clone(),
        },
    ] {
        let result = block_on(h.service.prepare(request, &h.ctx()));
        assert_eq!(result.unwrap_err(), NodeAppError::UnsafeSource);
    }
    // But it can still be disabled.
    assert!(
        block_on(h.service.prepare(
            PrepareRequest::SetEnabled {
                node: nodes[0].id.clone(),
                enabled: false
            },
            &h.ctx()
        ))
        .is_ok()
    );
}

#[test]
fn cached_pages_and_versions_pass_through_the_registry_port() {
    let h = Harness::new();
    assert_eq!(
        block_on(h.service.search(RegistryQuery::default(), false, 10)).unwrap_err(),
        NodeAppError::Offline
    );
    assert!(block_on(h.service.versions("pack")).unwrap().is_empty());
}

#[test]
fn runtime_node_checks_reconcile_and_scan_without_acquiring_again() {
    let h = Harness::new();
    let review = h.prepare();
    h.service.deps.plans.0.lock().unwrap().clear();
    h.files.state.lock().unwrap().recover = Some(RecoveryAction::Discarded);
    let mut lease = block_on(h.world.deps().gate.try_acquire()).unwrap();
    let before = h
        .world
        .gate
        .try_acquires
        .load(std::sync::atomic::Ordering::SeqCst);
    block_on(h.service.before_start(&mut *lease)).unwrap();
    assert_eq!(h.world.count("files:recover"), 1);
    assert_eq!(h.world.count("files:scan"), 0);
    assert_eq!(
        block_on(h.service.operation_status(&review.id))
            .unwrap()
            .phase,
        OperationPhase::Failed
    );
    block_on(h.service.after_stop(&mut *lease)).unwrap();
    assert_eq!(h.world.count("files:scan"), 1);
    assert_eq!(
        h.world
            .gate
            .try_acquires
            .load(std::sync::atomic::Ordering::SeqCst),
        before
    );
    assert_eq!(
        h.world
            .gate
            .acquires
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );
}

#[test]
fn runtime_node_checks_map_recovery_scan_and_running_failures() {
    let h = Harness::new();
    let review = h.prepare();
    h.service.deps.plans.0.lock().unwrap().clear();
    h.store.state.lock().unwrap().operations[0].phase = OperationPhase::PublishIntent;
    let mut lease = block_on(h.world.deps().gate.try_acquire()).unwrap();
    assert_eq!(
        block_on(h.service.before_start(&mut *lease)),
        Err(RuntimeError::NodeRecoveryRequired)
    );
    assert_eq!(h.world.count("files:recover"), 1);
    assert_eq!(
        block_on(h.service.operation_status(&review.id))
            .unwrap()
            .phase,
        OperationPhase::PublishIntent
    );

    h.store.state.lock().unwrap().journal_error = Some(NodeAppError::Storage);
    assert_eq!(
        block_on(h.service.before_start(&mut *lease)),
        Err(RuntimeError::NodeRecoveryRequired)
    );
    assert_eq!(
        block_on(h.service.after_stop(&mut *lease)),
        Err(RuntimeError::NodeRescanFailed)
    );
    assert_eq!(h.world.count("files:scan"), 0);

    h.store.state.lock().unwrap().journal_error = None;
    h.store.state.lock().unwrap().operations.clear();
    h.files.state.lock().unwrap().scan_error = Some(NodeAppError::Filesystem);
    assert_eq!(
        block_on(h.service.after_stop(&mut *lease)),
        Err(RuntimeError::NodeRescanFailed)
    );
    assert_eq!(h.world.count("files:scan"), 1);

    for block in [RuntimeBlock::Running, RuntimeBlock::CannotAssess] {
        *h.world.runtime_block.lock().unwrap() = Some(block);
        assert_eq!(
            block_on(h.service.before_start(&mut *lease)),
            Err(RuntimeError::NeedsStop)
        );
        assert_eq!(
            block_on(h.service.after_stop(&mut *lease)),
            Err(RuntimeError::NeedsStop)
        );
    }
    assert_eq!(h.world.count("files:scan"), 1);
}

fn hook_plan(h: &Harness) -> PlanReview {
    h.files
        .state
        .lock()
        .unwrap()
        .staged
        .as_mut()
        .unwrap()
        .hook_digest = Some("hook".into());
    h.files.state.lock().unwrap().hook_digest = Some("hook".into());
    h.prepare()
}

fn hook_decision(review: &PlanReview) -> CommitDecision {
    let mut decision = Harness::decision(review);
    decision.consent.acknowledge_warnings = true;
    decision.consent.run_install_py = true;
    decision
}

#[test]
fn hook_failure_skips_requirements_and_retains_repair_state() {
    for cancelled in [false, true] {
        let h = Harness::new();
        let review = hook_plan(&h);
        h.hook
            .changes
            .lock()
            .unwrap()
            .push(("torch".into(), "2.15.0".into()));
        *h.hook.fails.lock().unwrap() = !cancelled;
        *h.hook.cancels.lock().unwrap() = cancelled;
        assert_eq!(
            block_on(
                h.service
                    .commit(&review.id, hook_decision(&review), &h.ctx())
            )
            .unwrap_err(),
            NodeAppError::DependenciesFailed
        );
        assert_eq!(h.world.count("install:node"), 0);
        assert_eq!(h.world.count("preview:constrained"), 1);
        assert_eq!(h.env().state(), EnvState::RepairRequired);
        assert!(h.world.position("hook:run") < h.world.position("install:torch"));
        let calls = h.world.calls();
        assert!(
            calls
                .iter()
                .rposition(|call| call == "install:torch")
                .unwrap()
                < calls.iter().rposition(|call| call == "guard:drop").unwrap()
        );
        assert!(
            h.world
                .installed
                .lock()
                .unwrap()
                .iter()
                .any(|package| package.name == "torch" && package.version == "2.14.1+cu130")
        );
        let state = h.store.state.lock().unwrap();
        assert_eq!(state.nodes[0].dependencies, DependencyState::Failed);
        assert_eq!(
            state.operations.last().unwrap().phase,
            OperationPhase::Failed
        );
        assert_eq!(
            state.dependencies.last().unwrap().state,
            DependencyState::Failed
        );
    }
}

#[test]
fn hook_pin_repair_restores_profile_constraints_before_requirements() {
    let h = Harness::new();
    let extras = vec!["numpy<3".to_owned()];
    h.world.profiles.lock().unwrap()[0].constraints = extras.clone();
    let review = hook_plan(&h);
    h.hook
        .changes
        .lock()
        .unwrap()
        .push(("torch".into(), "2.15.0".into()));
    block_on(
        h.service
            .commit(&review.id, hook_decision(&review), &h.ctx()),
    )
    .unwrap();
    let writes = h.world.constraint_extras.lock().unwrap();
    assert_eq!(
        writes.as_slice(),
        &[extras.clone(), extras.clone(), Vec::new(), extras]
    );
    let calls = h.world.calls();
    let hook = h.world.position("hook:run");
    let node = h.world.position("install:node");
    assert!(hook < h.world.position("install:torch"));
    let last_constraints = calls
        .iter()
        .rposition(|call| call == "write_constraints")
        .unwrap();
    let post_preview = calls
        .iter()
        .rposition(|call| call == "preview:constrained")
        .unwrap();
    assert!(last_constraints < post_preview && post_preview < node);
}

#[test]
fn requirements_after_hook_use_reviewed_snapshot_and_frozen_versions() {
    for refused in [None, Some(false), Some(true)] {
        let h = Harness::new();
        h.world
            .previews
            .lock()
            .unwrap()
            .push_back(Ok(ResolutionPreview {
                installs: vec![crate::environment::InstalledPackage {
                    name: "numpy".into(),
                    version: "2.2.0".into(),
                }],
                removals: Vec::new(),
            }));
        let review = hook_plan(&h);
        h.hook
            .changes
            .lock()
            .unwrap()
            .push(("numpy".into(), "99.0".into()));
        if let Some(protected) = refused {
            h.world.previews.lock().unwrap().push_back(if protected {
                Ok(torch_preview("2.15.0"))
            } else {
                Err(ProvisionError::Failed(
                    "requirements include/local-path preflight refused".into(),
                ))
            });
        }
        let result = block_on(
            h.service
                .commit(&review.id, hook_decision(&review), &h.ctx()),
        );
        let previews = h.world.preview_specs.lock().unwrap();
        assert_eq!(previews.len(), 2);
        let post_hook = &previews[1];
        assert_eq!(post_hook.requirements, ["registry-dep>=1", "numpy==2.2.0"]);
        assert_eq!(&*post_hook.guarded.as_ref().unwrap().text, b"numpy\n");
        assert_eq!(h.world.count("hook:run"), 1);
        if refused.is_some() {
            assert_eq!(result.unwrap_err(), NodeAppError::ResolutionFailed);
            assert_eq!(h.world.count("install:node"), 0);
            assert_eq!(h.env().state(), EnvState::RepairRequired);
        } else {
            result.unwrap();
            let installs = h.world.installs.lock().unwrap();
            let installed = installs.iter().find(|spec| spec.guarded.is_some()).unwrap();
            assert_eq!(installed.requirements, post_hook.requirements);
            assert_eq!(installed.requirement_files, post_hook.requirement_files);
            assert_eq!(installed.constraint_file, post_hook.constraint_file);
            assert_eq!(
                &*installed.guarded.as_ref().unwrap().text,
                &*post_hook.guarded.as_ref().unwrap().text
            );
        }
    }
    // Invalid frozen versions are refused before any hook executes.
    let h = Harness::new();
    let review = hook_plan(&h);
    h.service
        .deps
        .plans
        .0
        .lock()
        .unwrap()
        .get_mut(&review.id)
        .unwrap()
        .package
        .as_mut()
        .unwrap()
        .installs
        .push(crate::environment::InstalledPackage {
            name: "numpy".into(),
            version: "2.2.0 --bad".into(),
        });
    assert_eq!(
        block_on(
            h.service
                .commit(&review.id, hook_decision(&review), &h.ctx())
        )
        .unwrap_err(),
        NodeAppError::ResolutionFailed
    );
    assert_eq!(h.world.count("hook:run"), 0);

    // A hook cannot introduce requirements that were absent from the reviewed source.
    let h = Harness::new();
    {
        let mut files = h.files.state.lock().unwrap();
        files.requirements_digest = None;
        let staged = files.staged.as_mut().unwrap();
        staged.requirements_digest = None;
        staged.dependencies.clear();
        staged.requirement_lines.clear();
    }
    let review = hook_plan(&h);
    assert!(!review.has_requirements);
    block_on(
        h.service
            .commit(&review.id, hook_decision(&review), &h.ctx()),
    )
    .unwrap();
    assert_eq!(h.world.count("hook:run"), 1);
    assert_eq!(h.world.count("preview:constrained"), 0);
    assert_eq!(h.world.count("install:node"), 0);
}

#[test]
fn node_execution_guard_spans_preview_hook_install_and_cleanup() {
    let h = Harness::new();
    let review = hook_plan(&h);
    // Both mutations drift a pin and force cleanup while the live guard is retained.
    h.world
        .drift_on_requirements
        .store(true, std::sync::atomic::Ordering::SeqCst);
    h.hook
        .changes
        .lock()
        .unwrap()
        .push(("torch".into(), "2.15.0".into()));
    let result = block_on(
        h.service
            .commit(&review.id, hook_decision(&review), &h.ctx()),
    );
    result.unwrap();
    let calls = h.world.calls();
    let at = |name: &str, nth: usize| {
        calls
            .iter()
            .enumerate()
            .filter(|(_, call)| *call == name)
            .nth(nth)
            .map(|(index, _)| index)
            .unwrap_or_else(|| panic!("`{name}` #{nth} missing: {calls:?}"))
    };
    // The preview guard covers the awaited previews and is released before publication.
    assert!(at("guard:pin:staged", 0) < at("preview:constrained", 0));
    assert!(at("preview:constrained", 0) < at("guard:drop", 0));
    assert!(at("guard:drop", 0) < at("files:publish", 0));
    // The live guard outlives hook, pin cleanup, post-hook preview and requirements installation.
    let live = at("guard:pin:live", 0);
    assert!(at("files:publish", 0) < live);
    assert!(live < at("hook:run", 0));
    assert!(at("hook:run", 0) < at("preview:constrained", 1));
    assert!(at("preview:constrained", 1) < at("install:node", 0));
    let hook = at("hook:run", 0);
    let cleanup = calls[hook..]
        .iter()
        .position(|call| call == "install:torch")
        .map(|offset| hook + offset)
        .expect("pins are reinstalled after the script");
    assert!(cleanup < at("guard:drop", 1));
    assert_eq!(h.world.count("guard:pin:staged"), 1);
    assert_eq!(h.world.count("guard:pin:live"), 1);
    assert_eq!(h.world.count("guard:drop"), 2);
    // Installation consumed the protected snapshot, not a path to reopen.
    let installs = h.world.installs.lock().unwrap().clone();
    let node_install = installs
        .iter()
        .find(|spec| spec.guarded.is_some())
        .expect("the node install uses the guarded snapshot");
    let guarded = node_install.guarded.as_ref().unwrap();
    assert_eq!(&*guarded.text, b"numpy\n");
    assert_eq!(
        node_install.requirement_files,
        std::slice::from_ref(&guarded.path)
    );
    assert_eq!(
        h.world.node_install_retained.lock().unwrap().last(),
        Some(&(true, true))
    );
}

#[test]
fn a_drifted_guarded_location_is_refused_before_anything_runs() {
    let h = Harness::new();
    let review = hook_plan(&h);
    h.files.state.lock().unwrap().pin_digest = Some("tree:changed".into());
    assert_eq!(
        block_on(
            h.service
                .commit(&review.id, hook_decision(&review), &h.ctx())
        )
        .unwrap_err(),
        NodeAppError::StalePlan
    );
    assert_eq!(h.world.count("install:node"), 0);
    assert_eq!(h.world.count("hook:run"), 0);
    // The refusal happens before the environment is marked for repair.
    assert_eq!(h.env().state(), EnvState::Verified);
    assert_eq!(
        h.world.count("guard:pin:live"),
        h.world.count("guard:drop") - 1
    );
}

#[test]
fn consented_hook_final_digest_is_saved_before_verified() {
    let h = Harness::new();
    {
        let mut state = h.files.state.lock().unwrap();
        state.hook_digest = Some("reviewed-hook".into());
        state.staged.as_mut().unwrap().hook_digest = state.hook_digest.clone();
        state.hook_final_digest = Some("tree:after-hook".into());
    }
    let review = h.prepare();
    let mut decision = Harness::decision(&review);
    decision.consent.run_install_py = true;
    decision.consent.acknowledge_warnings = true;
    let outcome = block_on(h.service.commit(&review.id, decision, &h.ctx())).unwrap();
    assert_eq!(outcome.node.tree_digest, "tree:after-hook");
    assert_eq!(outcome.node.source, review.source);
    let state = h.store.state.lock().unwrap();
    let verified = state.verified.last().unwrap();
    assert_eq!(verified.node.tree_digest, "tree:after-hook");
    assert_eq!(verified.node.source, review.source);
    assert_eq!(verified.requirements_digest, review.requirements_digest);
    assert_eq!(verified.hook_digest, review.hook_digest);
    assert_eq!(state.nodes[0].tree_digest, "tree:after-hook");
    assert_eq!(
        state.dependencies.last().unwrap().state,
        DependencyState::Ok
    );
    assert_eq!(
        h.files.state.lock().unwrap().final_snapshot_env,
        Some(EnvState::RepairRequired)
    );
    let calls = h.world.calls();
    let at = |call| calls.iter().position(|c| c == call).unwrap();
    assert!(at("hook:run") < at("files:final_snapshot"));
    assert!(at("files:final_snapshot") < at("dependency:ok"));
    assert!(at("files:final_snapshot") < at("store:verified"));
    assert_eq!(h.env().state(), EnvState::Verified);
}

fn prepare_final_validation_update(h: &Harness) -> PlanReview {
    let installed = h.installed();
    {
        let mut state = h.files.state.lock().unwrap();
        state.staged = Some(staged("2.0.0"));
        state.digest = "tree:2.0.0".into();
    }
    block_on(h.service.prepare(
        PrepareRequest::Update {
            node: installed.id,
            reference: None,
        },
        &h.ctx(),
    ))
    .unwrap()
}

#[test]
fn unexpected_tree_mutation_without_a_hook_blocks_completion() {
    let h = Harness::new();
    let review = prepare_final_validation_update(&h);
    h.files.state.lock().unwrap().final_snapshot = Some(Ok(TreeSnapshot {
        digest: "tree:unexpected-mutation".into(),
        requirements_digest: review.requirements_digest.clone(),
        hook_digest: None,
        requirement_lines: Vec::new(),
    }));
    assert_eq!(
        block_on(
            h.service
                .commit(&review.id, Harness::decision(&review), &h.ctx())
        )
        .unwrap_err(),
        NodeAppError::StalePlan
    );
    assert_final_validation_failed(&h, &review);
    assert_eq!(h.world.count("hook:run"), 0);
}

fn assert_final_validation_failed(h: &Harness, review: &PlanReview) {
    let op = block_on(h.service.operation_status(&review.id)).unwrap();
    assert_eq!(op.phase, OperationPhase::DependenciesInProgress);
    assert!(op.failure.is_some());
    assert!(op.blocks_start());
    assert_eq!(op.node.tree_digest, review.tree_digest);
    assert_eq!(op.node.dependencies, DependencyState::Failed);
    assert_eq!(op.node.source, review.source);
    assert_eq!(h.env().state(), EnvState::RepairRequired);
    assert!(h.files.state.lock().unwrap().backup_retained);
    assert_eq!(h.world.count("files:recycle"), 0);
    assert_eq!(h.world.count("store:verified"), 0);
    let state = h.store.state.lock().unwrap();
    assert_eq!(
        state.dependencies.last().unwrap().state,
        DependencyState::Failed
    );
    assert_eq!(state.nodes[0], op.node);
    drop(state);
    assert_eq!(
        block_on(h.service.ensure_reconciled()),
        Err(NodeAppError::RecoveryRequired)
    );
}

#[test]
fn final_snapshot_failure_preserves_repair_and_backup() {
    for error in [
        NodeAppError::UnsafeSource,
        NodeAppError::Filesystem,
        NodeAppError::StalePlan,
    ] {
        let h = Harness::new();
        let review = prepare_final_validation_update(&h);
        h.files.state.lock().unwrap().final_snapshot = Some(Err(error.clone()));
        assert_eq!(
            block_on(
                h.service
                    .commit(&review.id, Harness::decision(&review), &h.ctx())
            )
            .unwrap_err(),
            error
        );
        assert_final_validation_failed(&h, &review);
    }
}

#[test]
fn blocked_verified_cleanup_blocks_start_but_trash_failure_does_not() {
    for phase in [OperationPhase::Verified, OperationPhase::CleanupPending] {
        let h = Harness::new();
        let op = interrupted(&h, phase, OperationKind::Update);
        block_on(h.store.save_operation(&op)).unwrap();
        h.files.state.lock().unwrap().recover = Some(RecoveryAction::Blocked);
        assert_eq!(
            block_on(h.service.reconcile()),
            Err(NodeAppError::RecoveryRequired)
        );
        let blocked = block_on(h.service.operation_status(op.id.as_str())).unwrap();
        assert_eq!(blocked.phase, phase);
        assert_eq!(
            blocked.failure.as_deref(),
            Some("Node recovery could not establish file identity")
        );
        assert!(blocked.blocks_start());
        h.files.state.lock().unwrap().recover = Some(RecoveryAction::CleanupPending);
        block_on(h.service.reconcile()).unwrap();
        let cleanup = block_on(h.service.operation_status(op.id.as_str())).unwrap();
        assert_eq!(cleanup.phase, OperationPhase::CleanupPending);
        assert!(cleanup.failure.is_none());
        assert!(!cleanup.blocks_start());
        h.files.state.lock().unwrap().recover = Some(RecoveryAction::RolledForward);
        block_on(h.service.reconcile()).unwrap();
        assert_eq!(
            block_on(h.service.operation_status(op.id.as_str()))
                .unwrap()
                .phase,
            OperationPhase::Completed
        );
    }
}

#[test]
fn interrupted_dependency_mutation_preserves_failure_state() {
    for action in [RecoveryAction::RolledForward, RecoveryAction::Blocked] {
        let h = Harness::new();
        let op = interrupted(
            &h,
            OperationPhase::DependenciesInProgress,
            OperationKind::Update,
        );
        h.files.state.lock().unwrap().backup_retained = true;
        h.files.state.lock().unwrap().recover = Some(action);
        block_on(h.store.save_operation(&op)).unwrap();
        let result = block_on(h.service.reconcile());
        assert_eq!(h.world.count("files:recover"), 1);
        assert_eq!(h.env().state(), EnvState::RepairRequired);
        assert!(h.files.state.lock().unwrap().backup_retained);
        assert_eq!(h.world.count("files:recycle"), 0);
        let saved = block_on(h.service.operation_status(op.id.as_str())).unwrap();
        assert!(saved.failure.is_some());
        if action == RecoveryAction::Blocked {
            assert_eq!(result, Err(NodeAppError::RecoveryRequired));
            assert_eq!(saved.phase, OperationPhase::DependenciesInProgress);
            assert!(block_on(h.service.list()).unwrap().is_empty());
            assert!(h.store.state.lock().unwrap().dependencies.is_empty());
        } else {
            result.unwrap();
            assert_eq!(saved.phase, OperationPhase::Failed);
            assert_eq!(saved.node.dependencies, DependencyState::Failed);
            assert_eq!(block_on(h.service.list()).unwrap()[0], saved.node);
            assert_eq!(
                h.store.state.lock().unwrap().dependencies[0].state,
                DependencyState::Failed
            );
        }
    }
}

impl Harness {
    fn requirements(&self) -> NodeRequirements {
        NodeRequirements::new(self.store.clone(), self.files.clone())
    }

    fn repair(
        &self,
    ) -> (
        crate::environment::RunOutcome,
        crate::environment::ProvisioningRun,
    ) {
        use crate::environment::*;
        let mut env = self.env();
        if env.state() == EnvState::Verified {
            env.mark_repair_required("t").unwrap();
            block_on(EnvironmentRepository::save(&*self.world, &env)).unwrap();
        }
        *self.world.checkout.lock().unwrap() = Checkout::Repo {
            commit: "abc1230000000000000000000000000000000000".into(),
        };
        let request = SetupRequest {
            mode: domain::environment::SetupMode::Repair,
            install_root: PathBuf::from(ROOT),
            python_source: PythonSourceInput::Managed,
            profile_id: "win-py313-torch2141-cu130".into(),
            comfyui_ref: ComfyRef::KeepCurrent,
            install_manager: false,
            accelerators: Vec::new(),
            curated_nodes: Vec::new(),
            clean_cache: false,
            confirm_adopted_changes: false,
            allow_source_builds: false,
            third_party_consent: false,
        };
        let deps = self.service.deps.environment.clone();
        let id =
            block_on(StartSetup::new(deps.clone()).execute(SetupAction::New(Box::new(request))))
                .unwrap();
        let result = block_on(
            ExecuteSetup::new(deps, Arc::new(self.service.clone()))
                .execute(&id, Arc::new(NullSink)),
        )
        .unwrap();
        (result, self.world.run(&id))
    }
}

#[test]
fn repair_replays_registry_dependencies_for_enabled_and_disabled_nodes() {
    use crate::environment::{NodeRequirementSource, RunOutcome};
    let h = Harness::new();
    let enabled = h.installed();
    let mut disabled = enabled.clone();
    disabled.id = NodeId::new("disabled-pack").unwrap();
    disabled.name = SafeFileName::new("off").unwrap();
    disabled.enabled = false;
    block_on(h.store.save_node(&disabled)).unwrap();
    let original = h.store.state.lock().unwrap().operations[0].clone();
    for (id, node, extras) in [
        ("older", enabled.clone(), vec!["old-extra==1".into()]),
        ("newest", enabled.clone(), vec!["enabled-extra==2".into()]),
        (
            "disabled",
            disabled.clone(),
            vec!["disabled-extra==3".into()],
        ),
    ] {
        let mut op = original.clone();
        op.id = NodeId::new(id).unwrap();
        op.node = node;
        op.dependency_requirements = extras;
        block_on(h.store.save_operation(&op)).unwrap();
    }
    // Later records without consent or of a different kind cannot replace the consented extras.
    for (id, kind, consent) in [
        ("unconsented", OperationKind::Install, None),
        ("toggle", OperationKind::Disable, Some("consent".into())),
    ] {
        let mut op = original.clone();
        op.id = NodeId::new(id).unwrap();
        op.kind = kind;
        op.consent_identity = consent;
        op.dependency_requirements = vec!["wrong-extra==9".into()];
        block_on(h.store.save_operation(&op)).unwrap();
    }
    let mut single = enabled.clone();
    single.id = NodeId::new("single").unwrap();
    single.name = SafeFileName::new("single.py").unwrap();
    single.single_file = true;
    let mut observed = enabled.clone();
    observed.id = NodeId::new("scan-id").unwrap();
    observed.origin = ProvenanceOrigin::ExternallyObserved;
    if let NodeSource::Registry {
        publisher,
        repository,
        ..
    } = &mut observed.source
    {
        *publisher = None;
        *repository = None;
    }
    {
        let mut files = h.files.state.lock().unwrap();
        files.scan = vec![observed, disabled.clone(), single];
        files.pin_digest = Some("current-content".into());
    }
    let sets = block_on(
        h.requirements()
            .requirement_sets(Path::new(ROOT), Default::default()),
    )
    .unwrap();
    assert_eq!(sets.len(), 2);
    assert_eq!(sets[0].extra, ["enabled-extra==2"]);
    assert_eq!(sets[1].extra, ["disabled-extra==3"]);
    assert!(
        sets[1]
            .file
            .as_ref()
            .unwrap()
            .directory
            .ends_with(".disabled/off")
    );
    let mut current = enabled.clone();
    current.tree_digest = "current-content".into();
    assert_eq!(sets[0].source_identity, current.dependency_identity());
    assert_eq!(&*sets[0].file.as_ref().unwrap().text, b"numpy\n");
    let (result, run) = h.repair();
    assert_eq!(result, RunOutcome::Succeeded);
    let installs = h.world.installs.lock().unwrap();
    for set in &sets {
        assert!(installs.iter().any(|spec| spec.requirements == set.extra
            && spec.guarded.is_some()
            && spec.constraint_file.is_some()));
        assert_eq!(
            block_on(
                h.store
                    .dependency(&set.node_id, &run.target_env, &set.source_identity)
            )
            .unwrap(),
            Some(DependencyState::Ok)
        );
    }
}

#[test]
fn repair_source_mismatch_uses_only_current_file_requirements() {
    use crate::environment::{NodeRequirementSource, RunOutcome};
    let h = Harness::new();
    let mut observed = h.installed();
    observed.source = registry_source("externally-changed");
    observed.tree_digest = "changed-content".into();
    h.files.state.lock().unwrap().scan = vec![observed.clone()];
    h.files.state.lock().unwrap().pin_digest = Some(observed.tree_digest.clone());
    let sets = block_on(
        h.requirements()
            .requirement_sets(Path::new(ROOT), Default::default()),
    )
    .unwrap();
    assert_eq!(sets.len(), 1);
    assert!(sets[0].extra.is_empty());
    assert!(sets[0].file.is_some());
    assert_eq!(sets[0].source_identity, observed.dependency_identity());
    h.world.installs.lock().unwrap().clear();
    let (result, _) = h.repair();
    assert_eq!(result, RunOutcome::Succeeded);
    let installs = h.world.installs.lock().unwrap();
    let node = installs.iter().find(|spec| spec.guarded.is_some()).unwrap();
    assert!(node.requirements.is_empty());
    assert_eq!(&*node.guarded.as_ref().unwrap().text, b"numpy\n");
    assert_eq!(h.world.count("registry:install"), 0);
}

#[test]
fn repair_never_resolves_registry_or_runs_install_py() {
    use crate::environment::RunOutcome;
    let h = Harness::new();
    let review = hook_plan(&h);
    let mut decision = Harness::decision(&review);
    decision.consent.run_install_py = true;
    decision.consent.acknowledge_warnings = true;
    let node = block_on(h.service.commit(&review.id, decision, &h.ctx()))
        .unwrap()
        .node;
    assert_eq!(h.world.count("hook:run"), 1);
    h.files.state.lock().unwrap().scan = vec![node];
    h.world.calls.lock().unwrap().clear();
    let (result, _) = h.repair();
    assert_eq!(result, RunOutcome::Succeeded);
    assert_eq!(h.world.count("hook:run"), 0);
    assert_eq!(h.world.count("registry:install"), 0);
    assert_eq!(h.world.count("files:prepare"), 0);
    assert_eq!(h.world.count("files:publish"), 0);
}

#[test]
fn node_dependency_previews_validate_guard_before_omitting_requirements() {
    for kind in [
        OperationKind::Install,
        OperationKind::Update,
        OperationKind::RetryDependencies,
    ] {
        for registry_extras in [false, true] {
            let h = Harness::new();
            let mut node = interrupted(&h, OperationPhase::Completed, kind).node;
            if kind == OperationKind::RetryDependencies && !registry_extras {
                node.source = NodeSource::Manual;
            }
            let request = match kind {
                OperationKind::Install => Harness::registry_request(),
                OperationKind::Update => PrepareRequest::Update {
                    node: node.id.clone(),
                    reference: None,
                },
                OperationKind::RetryDependencies => PrepareRequest::RetryDependencies {
                    node: node.id.clone(),
                },
                _ => unreachable!(),
            };
            if kind != OperationKind::Install {
                block_on(h.store.save_node(&node)).unwrap();
            }
            {
                let mut files = h.files.state.lock().unwrap();
                files.requirements_digest = None;
                files.pin_error = Some(NodeAppError::UnsafeSource);
                let staged = files.staged.as_mut().unwrap();
                staged.requirements_digest = None;
                staged.requirement_lines.clear();
                if !registry_extras {
                    staged.dependencies.clear();
                }
            }
            let old_nodes = block_on(h.store.nodes()).unwrap();
            let old_envs = h.world.envs.lock().unwrap().clone();
            let old_active = h.world.active_id();
            assert_eq!(
                block_on(h.service.prepare(request, &h.ctx())),
                Err(NodeAppError::UnsafeSource),
                "{kind:?}, registry_extras={registry_extras}"
            );
            let files = h.files.state.lock().unwrap();
            assert_eq!(files.pin_attempts.len(), 1);
            assert_eq!(
                files.pin_attempts[0].1,
                kind != OperationKind::RetryDependencies
            );
            assert!(h.service.pending_reviews().unwrap().is_empty());
            assert_eq!(block_on(h.store.nodes()).unwrap(), old_nodes);
            assert_eq!(*h.world.envs.lock().unwrap(), old_envs);
            assert_eq!(h.world.active_id(), old_active);
            assert!(h.store.state.lock().unwrap().dependencies.is_empty());
            assert!(h.world.installs.lock().unwrap().is_empty());
            assert!(h.world.constraint_writes.lock().unwrap().is_empty());
            for call in [
                "preview:constrained",
                "preview:unconstrained",
                "hook:run",
                "files:publish",
                "files:backup",
                "store:dependencies_in_progress",
            ] {
                assert_eq!(h.world.count(call), 0, "{kind:?}: {call}");
            }
        }
    }
}

#[test]
fn node_dependency_state_follows_active_environment_and_content() {
    let h = Harness::new();
    let installed = h.installed();
    assert_eq!(
        block_on(h.service.list()).unwrap()[0].dependencies,
        DependencyState::Ok
    );
    h.world
        .seed_active_env("env-2", PythonVersion::new(3, 13, 16));
    assert_eq!(
        block_on(h.service.list()).unwrap()[0].dependencies,
        DependencyState::NotRun
    );
    block_on(h.store.save_dependency(&DependencyOutcome {
        node: installed.id.clone(),
        environment: EnvId::new("env-2").unwrap(),
        source_identity: installed.dependency_identity(),
        state: DependencyState::Failed,
    }))
    .unwrap();
    assert_eq!(
        block_on(h.service.list()).unwrap()[0].dependencies,
        DependencyState::Failed
    );
    let mut manual = installed.clone();
    manual.id = NodeId::new("manual").unwrap();
    manual.name = SafeFileName::new("manual").unwrap();
    manual.source = NodeSource::Manual;
    manual.dependencies = DependencyState::Ok;
    block_on(h.store.save_node(&manual)).unwrap();
    assert_eq!(
        block_on(h.service.list()).unwrap()[1].dependencies,
        DependencyState::NotRun
    );
    {
        let mut files = h.files.state.lock().unwrap();
        files.scan = vec![installed.clone(), manual];
        files
            .snapshots
            .insert("pack".into(), "manually-edited".into());
    }
    let nodes = block_on(h.service.rescan()).unwrap();
    let current = nodes.iter().find(|node| node.id == installed.id).unwrap();
    assert_eq!(current.tree_digest, "manually-edited");
    assert_eq!(current.dependencies, DependencyState::NotRun);
    block_on(h.store.save_dependency(&DependencyOutcome {
        node: current.id.clone(),
        environment: EnvId::new("env-2").unwrap(),
        source_identity: current.dependency_identity(),
        state: DependencyState::Ok,
    }))
    .unwrap();
    assert_eq!(
        block_on(h.service.list()).unwrap()[0].dependencies,
        DependencyState::Ok
    );
    let mut observed = installed.clone();
    observed.source = registry_source("2.0.0");
    h.files.state.lock().unwrap().scan = vec![observed];
    assert_eq!(
        block_on(h.service.rescan()).unwrap()[0].dependencies,
        DependencyState::NotRun
    );
    *h.world.active.lock().unwrap() = Some(EnvId::new("env-1").unwrap());
    assert_eq!(
        block_on(h.service.list()).unwrap()[0].dependencies,
        DependencyState::NotRun
    );
    assert_eq!(
        block_on(h.store.dependency(
            &installed.id,
            &EnvId::new("env-1").unwrap(),
            &installed.dependency_identity()
        ))
        .unwrap(),
        Some(DependencyState::Ok)
    );
}

#[test]
fn conflicted_nodes_never_project_successful_dependency_outcomes() {
    for (source, origin) in [
        (registry_source("1.0.0"), ProvenanceOrigin::App),
        (NodeSource::Manual, ProvenanceOrigin::ExternallyObserved),
    ] {
        let h = Harness::new();
        let mut node = interrupted(&h, OperationPhase::Completed, OperationKind::Install).node;
        node.source = source;
        node.origin = origin;
        node.dependencies = DependencyState::Ok;
        block_on(h.store.save_node(&node)).unwrap();
        let outcome = DependencyOutcome {
            node: node.id.clone(),
            environment: h.env().id,
            source_identity: node.dependency_identity(),
            state: DependencyState::Ok,
        };
        block_on(h.store.save_dependency(&outcome)).unwrap();
        let mut observed = node.clone();
        observed.id = NodeId::new("observed").unwrap();
        observed.origin = ProvenanceOrigin::ExternallyObserved;
        observed.conflict = true;
        {
            let mut files = h.files.state.lock().unwrap();
            files.scan = vec![observed];
            files.pin_error = Some(NodeAppError::UnsafeSource);
        }
        let mut expected = node.clone();
        expected.conflict = true;
        expected.dependencies = DependencyState::NotRun;
        assert!(!expected.tree_digest.is_empty());
        assert_eq!(block_on(h.service.rescan()).unwrap(), [expected.clone()]);
        assert_eq!(block_on(h.service.list()).unwrap(), [expected]);
        let files = h.files.state.lock().unwrap();
        assert!(files.snapshot_attempts.is_empty());
        assert!(files.pin_attempts.is_empty());
        let store = h.store.state.lock().unwrap();
        assert_eq!(store.dependency_reads, 0);
        assert_eq!(store.dependencies, [outcome]);
    }
}

#[test]
fn node_dependency_projection_requires_a_content_digest() {
    let h = Harness::new();
    let mut node = interrupted(&h, OperationPhase::Completed, OperationKind::Install).node;
    node.tree_digest.clear();
    node.dependencies = DependencyState::Ok;
    block_on(h.store.save_node(&node)).unwrap();
    let outcome = DependencyOutcome {
        node: node.id.clone(),
        environment: h.env().id,
        source_identity: node.dependency_identity(),
        state: DependencyState::Ok,
    };
    block_on(h.store.save_dependency(&outcome)).unwrap();
    node.dependencies = DependencyState::NotRun;
    assert_eq!(block_on(h.service.list()).unwrap(), [node]);
    let store = h.store.state.lock().unwrap();
    assert_eq!(store.dependency_reads, 0);
    assert_eq!(store.dependencies, [outcome]);
}

#[test]
fn node_requirements_retain_guarded_bytes_and_the_execution_lease() {
    use crate::environment::NodeRequirementSource;
    let h = Harness::new();
    let node = h.installed();
    h.files.state.lock().unwrap().scan = vec![node];
    let lease = block_on(h.service.deps.environment.gate.try_acquire()).unwrap();
    let sets = block_on(
        h.requirements()
            .requirement_sets(Path::new(ROOT), lease.retained()),
    )
    .unwrap();
    let guard = h
        .files
        .state
        .lock()
        .unwrap()
        .guard_resources
        .last()
        .unwrap()
        .clone();
    drop(lease);
    assert!(guard.upgrade().is_some());
    assert!(h.world.gate.busy.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(
        h.files.state.lock().unwrap().scan_retained.last(),
        Some(&true)
    );
    assert!(!sets[0].file.as_ref().unwrap().retained.is_empty());
    drop(sets);
    assert!(guard.upgrade().is_none());
    assert!(!h.world.gate.busy.load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn node_requirements_refuse_blocking_operations_and_provenance_conflicts() {
    use crate::environment::NodeRequirementSource;
    let h = Harness::new();
    let node = h.installed();
    h.files.state.lock().unwrap().scan = vec![node.clone()];
    let mut op = interrupted(&h, OperationPhase::Published, OperationKind::Install);
    block_on(h.store.save_operation(&op)).unwrap();
    let error = block_on(
        h.requirements()
            .requirement_sets(Path::new(ROOT), Default::default()),
    )
    .unwrap_err();
    assert!(matches!(error, ProvisionError::Failed(_)));
    op.phase = OperationPhase::Failed;
    block_on(h.store.save_operation(&op)).unwrap();
    h.files.state.lock().unwrap().scan[0].source = NodeSource::Manual;
    assert!(
        block_on(
            h.requirements()
                .requirement_sets(Path::new(ROOT), Default::default())
        )
        .is_err()
    );
    assert!(
        block_on(
            h.requirements()
                .needs_repair(&h.env().id, Path::new(ROOT), Default::default())
        )
        .is_err()
    );
    let mut observed = node.clone();
    observed.conflict = true;
    assert_eq!(observed.source.identity(), node.source.identity());
    h.files.state.lock().unwrap().scan = vec![observed];
    assert!(
        block_on(
            h.requirements()
                .requirement_sets(Path::new(ROOT), Default::default())
        )
        .is_err()
    );
    assert!(
        block_on(
            h.requirements()
                .needs_repair(&h.env().id, Path::new(ROOT), Default::default())
        )
        .is_err()
    );
}

#[test]
fn node_requirements_check_exact_outcomes_and_include_registry_only_extras() {
    use crate::environment::NodeRequirementSource;
    let h = Harness::new();
    let node = h.installed();
    {
        let mut files = h.files.state.lock().unwrap();
        files.scan = vec![node.clone()];
        files.requirements_digest = None;
    }
    let provider = h.requirements();
    let env = EnvId::new("replacement").unwrap();
    let sets = block_on(provider.requirement_sets(Path::new(ROOT), Default::default())).unwrap();
    assert_eq!(sets.len(), 1);
    assert!(sets[0].file.is_none());
    assert_eq!(sets[0].extra, ["registry-dep>=1"]);
    assert!(block_on(provider.needs_repair(&env, Path::new(ROOT), Default::default())).unwrap());
    for state in [
        DependencyState::NotRun,
        DependencyState::Failed,
        DependencyState::Ok,
    ] {
        block_on(provider.record_outcome(&env, &sets[0], state)).unwrap();
        assert_eq!(
            block_on(provider.needs_repair(&env, Path::new(ROOT), Default::default())).unwrap(),
            state != DependencyState::Ok
        );
    }
    h.files.state.lock().unwrap().pin_digest = Some("new-contents".into());
    assert!(block_on(provider.needs_repair(&env, Path::new(ROOT), Default::default())).unwrap());
    h.files.state.lock().unwrap().scan[0].source = registry_source("2.0.0");
    assert!(
        block_on(provider.requirement_sets(Path::new(ROOT), Default::default()))
            .unwrap()
            .is_empty()
    );
    assert!(!block_on(provider.needs_repair(&env, Path::new(ROOT), Default::default())).unwrap());
}

#[test]
fn node_requirement_provider_errors_use_fixed_provision_descriptions() {
    use crate::environment::NodeRequirementSource;
    let h = Harness::new();
    h.store.state.lock().unwrap().journal_error =
        Some(NodeAppError::Failed("sensitive storage detail".into()));
    assert_eq!(
        block_on(
            h.requirements()
                .requirement_sets(Path::new(ROOT), Default::default())
        ),
        Err(ProvisionError::Failed(
            "Custom node requirements could not be read or recorded.".into()
        ))
    );
    assert_eq!(
        block_on(h.service.unresolved()),
        Err(NodeAppError::Failed("sensitive storage detail".into()))
    );
    h.store.state.lock().unwrap().journal_error = None;
    let node = interrupted(&h, OperationPhase::Completed, OperationKind::Install).node;
    {
        let mut files = h.files.state.lock().unwrap();
        files.scan = vec![node];
        files.pin_error = Some(NodeAppError::UnsafeSource);
    }
    let expected =
        ProvisionError::Failed("Custom node requirements could not be read or recorded.".into());
    assert_eq!(
        block_on(
            h.requirements()
                .requirement_sets(Path::new(ROOT), Default::default())
        ),
        Err(expected.clone())
    );
    assert_eq!(
        block_on(
            h.requirements()
                .needs_repair(&h.env().id, Path::new(ROOT), Default::default())
        ),
        Err(expected)
    );
    assert_eq!(h.files.state.lock().unwrap().pin_attempts.len(), 2);
}
/// Curated custom nodes during first-run setup, against the real node workflow and the fake
/// machine.
mod curated_setup {
    use super::*;
    use crate::environment::{
        ComfyRef, EnvironmentAppError, ExecuteSetup, FailureKind, NullSink, PythonSourceInput,
        PythonTarget, ResolvedSources, RunId, RunOutcome, RunStatus, SetupAction, SetupNodeError,
        SetupNodes, SetupRequest, StartSetup, StepContext, StepId, StepStatus,
        tests::fakes::profile,
    };
    use domain::environment::{NewEnvironment, Ownership, PythonSourceKind, SetupMode};

    const CANDIDATE: &str = "env-candidate";
    const RUN: &str = "run-setup";
    const DETACHED: &str = "This install is detached; it will not track a branch";
    const HOOK_GUIDANCE: &str = super::super::workflow::SETUP_HOOK_WARNING;

    #[test]
    fn publication_started_fails_closed_on_storage_errors() {
        let h = Harness::new();
        assert!(!block_on(h.service.publication_started("missing")));
        h.store.state.lock().unwrap().journal_error = Some(NodeAppError::Storage);
        assert!(block_on(h.service.publication_started("missing")));
        h.store.state.lock().unwrap().journal_error = None;
        let op = interrupted(&h, OperationPhase::Failed, OperationKind::Install);
        block_on(h.store.save_operation(&op)).unwrap();
        h.store.state.lock().unwrap().nodes_error = Some(NodeAppError::Storage);
        assert!(block_on(h.service.publication_started(op.id.as_str())));
    }

    #[test]
    fn failed_operation_with_a_stored_node_counts_as_published() {
        let h = Harness::new();
        let op = interrupted(&h, OperationPhase::Failed, OperationKind::Install);
        assert_eq!(op.node.dependencies, DependencyState::NotRun);
        block_on(h.store.save_operation(&op)).unwrap();
        assert!(!block_on(h.service.publication_started(op.id.as_str())));
        block_on(h.store.save_node(&op.node)).unwrap();
        assert!(block_on(h.service.publication_started(op.id.as_str())));
    }

    fn candidate_path() -> PathBuf {
        PathBuf::from(ROOT).join("envs").join(CANDIDATE)
    }

    fn candidate_pins() -> PinSet {
        let mut pins = PinSet::new();
        pins.insert(DistName::new("torch").unwrap(), "2.14.1+cu130");
        pins.insert(DistName::new("torchvision").unwrap(), "0.29.1+cu130");
        pins.insert(DistName::new("triton-windows").unwrap(), "3.8.0.post29");
        pins
    }

    fn request(selected: &[&str]) -> SetupRequest {
        SetupRequest {
            mode: SetupMode::FirstRun,
            install_root: PathBuf::from(ROOT),
            python_source: PythonSourceInput::Managed,
            profile_id: "win-py313-torch2141-cu130".into(),
            comfyui_ref: ComfyRef::Latest,
            install_manager: false,
            accelerators: Vec::new(),
            curated_nodes: selected.iter().map(|id| (*id).to_owned()).collect(),
            clean_cache: false,
            confirm_adopted_changes: false,
            allow_source_builds: false,
            third_party_consent: false,
        }
    }

    /// The request of a wizard whose review stage carried the user's consent.
    fn consenting_request(selected: &[&str]) -> SetupRequest {
        SetupRequest {
            third_party_consent: true,
            ..request(selected)
        }
    }

    fn git_request(id: &str, commit: char) -> PrepareRequest {
        PrepareRequest::Git {
            repository: format!("https://github.com/example/{id}.git"),
            reference: Some(commit.to_string().repeat(40)),
        }
    }

    fn spec(id: &str) -> crate::environment::CuratedNodeSpec {
        crate::environment::tests::fakes::curated_spec(id)
    }

    fn step_context(run: &RunId) -> StepContext {
        StepContext {
            run_id: run.clone(),
            step: StepId::CuratedNodes,
            sink: Arc::new(NullSink),
            cancel: CancelToken::new(),
            allow_source_builds: false,
            retained: Default::default(),
        }
    }

    /// Acknowledges the warnings the review shows, as the user does in the review dialog.
    fn approve(review: &PlanReview) -> CommitDecision {
        let mut decision = Harness::decision(review);
        decision.consent.acknowledge_warnings = !review.warnings.is_empty();
        decision
    }

    impl Harness {
        /// Git requests stage the requested repository at the requested commit.
        fn curated(&self) {
            let mut files = self.files.state.lock().unwrap();
            files.curated = true;
            files.curated_warnings = vec![DETACHED.into()];
        }

        /// A persisted first run at the curated step with a provisioning candidate whose
        /// protected pins are recorded, next to whatever environment is active.
        fn setup_run(&self, selected: &[&str]) -> RunId {
            let profile = profile();
            let mut env = PythonEnvironment::provisioning(NewEnvironment {
                id: EnvId::new(CANDIDATE).unwrap(),
                path: candidate_path(),
                ownership: Ownership::AppOwned,
                source: PythonSourceKind::Managed,
                base_interpreter: None,
                python_version: profile.python,
                profile_id: profile.id.clone(),
                profile_revision: profile.revision,
                authorized_location: candidate_path(),
                now: "t".into(),
            });
            env.record_pins(candidate_pins(), "t");
            self.world
                .envs
                .lock()
                .unwrap()
                .insert(CANDIDATE.into(), env.clone());
            let mut steps = crate::environment::ProvisioningRun::fresh_steps();
            steps[usize::from(StepId::CuratedNodes.number())].status = StepStatus::Running;
            let run = crate::environment::ProvisioningRun {
                id: RunId::new(RUN).unwrap(),
                request: request(selected),
                profile,
                target_env: env.id.clone(),
                previous_env: None,
                python_target: PythonTarget {
                    ownership: Ownership::AppOwned,
                    source: PythonSourceKind::Managed,
                    env_path: candidate_path(),
                    interpreter: None,
                    create_venv: true,
                    development_files_ok: None,
                },
                resolved: ResolvedSources::default(),
                components: Default::default(),
                steps,
                status: RunStatus::Running,
                cancel_requested: false,
                failure: None,
                activation: crate::environment::ActivationPhase::None,
                manager_consent: None,
                curated: Vec::new(),
                created_at: "t".into(),
                updated_at: "t".into(),
            };
            self.world
                .runs
                .lock()
                .unwrap()
                .insert(RUN.into(), run.clone());
            run.id
        }

        fn edit_run(&self, change: impl FnOnce(&mut crate::environment::ProvisioningRun)) {
            change(self.world.runs.lock().unwrap().get_mut(RUN).unwrap());
        }

        fn edit_candidate(&self, change: impl FnOnce(&mut PythonEnvironment)) {
            change(self.world.envs.lock().unwrap().get_mut(CANDIDATE).unwrap());
        }

        fn prepare_setup(&self, run: &RunId, id: &str) -> Result<PlanReview, NodeAppError> {
            let mut lease = block_on(self.service.deps.environment.gate.try_acquire()).unwrap();
            block_on(self.service.prepare_for_setup_held(
                run,
                git_request(id, 'a'),
                &self.ctx(),
                &mut *lease,
            ))
        }

        fn commit_setup(
            &self,
            run: &RunId,
            review: &PlanReview,
            decision: CommitDecision,
        ) -> Result<NodeOutcome, NodeAppError> {
            let mut lease = block_on(self.service.deps.environment.gate.try_acquire()).unwrap();
            block_on(self.service.commit_for_setup_held(
                run,
                &review.id,
                decision,
                &self.ctx(),
                &mut *lease,
            ))
        }
    }

    #[test]
    fn setup_content_policy_requires_persisted_consent_and_selected_curated_source() {
        let h = Harness::new();
        h.curated();
        h.prepare();
        assert_eq!(
            h.files.state.lock().unwrap().prepare_policies,
            [NodePreparePolicy::Strict]
        );
        let run = h.setup_run(&["alpha"]);
        h.prepare_setup(&run, "alpha").unwrap();
        assert_eq!(
            h.files.state.lock().unwrap().prepare_policies.last(),
            Some(&NodePreparePolicy::Strict)
        );
        h.edit_run(|run| run.request.third_party_consent = true);
        let prepare = |request| {
            let mut lease = block_on(h.service.deps.environment.gate.try_acquire()).unwrap();
            block_on(
                h.service
                    .prepare_for_setup_held(&run, request, &h.ctx(), &mut *lease),
            )
        };
        let review = prepare(PrepareRequest::Git {
            repository: "https://github.com/example/alpha.git/".into(),
            reference: None,
        })
        .unwrap();
        assert_eq!(
            h.files.state.lock().unwrap().prepare_policies.last(),
            Some(&NodePreparePolicy::ConsentedFirstRun)
        );
        let prepared = h.world.count("files:prepare");
        for request in [
            PrepareRequest::Git {
                repository: spec("beta").repository,
                reference: None,
            },
            PrepareRequest::Git {
                repository: spec("alpha").repository,
                reference: Some("main".into()),
            },
        ] {
            assert_eq!(prepare(request).unwrap_err(), NodeAppError::InvalidRequest);
            assert_eq!(h.world.count("files:prepare"), prepared);
        }
        h.edit_run(|run| run.request.third_party_consent = false);
        assert_eq!(
            h.commit_setup(&run, &review, approve(&review)).unwrap_err(),
            NodeAppError::StalePlan
        );
        assert_eq!(h.world.count("files:publish"), 0);
        h.edit_run(|run| {
            run.request.third_party_consent = true;
            run.request.curated_nodes.clear();
        });
        assert_eq!(
            h.commit_setup(&run, &review, approve(&review)).unwrap_err(),
            NodeAppError::StalePlan
        );
        assert_eq!(h.world.count("files:publish"), 0);
    }

    #[test]
    fn setup_node_target_never_selects_the_active_environment() {
        let h = Harness::new();
        h.curated();
        let run = h.setup_run(&["alpha"]);
        let active = h.env();
        let review = h.prepare_setup(&run, "alpha").unwrap();
        assert_eq!(review.environment.as_str(), CANDIDATE);
        assert_ne!(review.environment, active.id);
        assert_eq!(review.pins, candidate_pins());
        let outcome = h
            .commit_setup(&run, &review, approve(&review))
            .expect("the plan commits against its candidate");
        assert_eq!(outcome.node.dependencies, DependencyState::Ok);
        // Every package mutation ran in the candidate; the old interpreter saw none.
        let envs = h.world.install_envs.lock().unwrap().clone();
        assert!(!envs.is_empty());
        assert!(
            envs.iter().all(|path| *path == candidate_path()),
            "{envs:?}"
        );
        assert_eq!(
            h.env(),
            active,
            "the active environment record is untouched"
        );
        assert_eq!(h.env().state(), EnvState::Verified);
        assert_eq!(h.world.active_id().as_deref(), Some("env-1"));
        // Outcomes belong to the candidate, which is neither activated nor repaired.
        let outcomes = h.store.state.lock().unwrap().dependencies.clone();
        assert!(!outcomes.is_empty());
        assert!(
            outcomes
                .iter()
                .all(|outcome| outcome.environment.as_str() == CANDIDATE)
        );
        assert_eq!(h.world.env(CANDIDATE).state(), EnvState::Provisioning);
        // The ordinary workflow keeps selecting the active environment.
        let ordinary = h.prepare();
        assert_eq!(ordinary.environment, active.id);
    }

    #[test]
    fn setup_node_plans_bind_run_root_candidate_and_pins() {
        let h = Harness::new();
        h.curated();
        let run = h.setup_run(&["alpha"]);
        let review = h.prepare_setup(&run, "alpha").unwrap();
        let template = h
            .service
            .deps
            .plans
            .0
            .lock()
            .unwrap()
            .get(&review.id)
            .unwrap()
            .clone();
        assert_eq!(template.target, NodeTarget::Setup(run.clone()));
        assert_eq!(template.root, PathBuf::from(ROOT));
        // The ordinary workflow neither lists, commits, discards nor batches a setup plan.
        assert!(h.service.pending_reviews().unwrap().is_empty());
        assert_eq!(
            block_on(h.service.commit(&review.id, approve(&review), &h.ctx())).unwrap_err(),
            NodeAppError::StalePlan
        );
        assert_eq!(
            block_on(h.service.discard(&review.id)).unwrap_err(),
            NodeAppError::StalePlan
        );
        assert_eq!(
            block_on(
                h.service
                    .update_all(vec![(review.id.clone(), approve(&review))], &h.ctx())
            )
            .unwrap_err(),
            NodeAppError::StalePlan
        );
        let other_run = RunId::new("run-other").unwrap();
        assert_eq!(
            h.commit_setup(&other_run, &review, approve(&review))
                .unwrap_err(),
            NodeAppError::StalePlan
        );

        type Change = Box<dyn Fn(&mut PreparedPlan)>;
        let plan_changes: Vec<(&str, Change)> = vec![
            (
                "root",
                Box::new(|plan| plan.root = PathBuf::from("/elsewhere")),
            ),
            (
                "candidate",
                Box::new(|plan| plan.review.environment = EnvId::new("env-other").unwrap()),
            ),
            (
                "pins",
                Box::new(|plan| {
                    plan.review
                        .pins
                        .insert(DistName::new("torch").unwrap(), "9.9.9")
                }),
            ),
            (
                "run",
                Box::new(|plan| plan.target = NodeTarget::Setup(RunId::new("run-other").unwrap())),
            ),
            (
                "target kind",
                Box::new(|plan| plan.target = NodeTarget::Active),
            ),
        ];
        for (label, change) in plan_changes {
            let mut changed = template.clone();
            change(&mut changed);
            h.service
                .deps
                .plans
                .0
                .lock()
                .unwrap()
                .insert(review.id.clone(), changed);
            assert_eq!(
                h.commit_setup(&run, &review, approve(&review)).unwrap_err(),
                NodeAppError::StalePlan,
                "{label}"
            );
            assert!(
                h.service
                    .deps
                    .plans
                    .0
                    .lock()
                    .unwrap()
                    .contains_key(&review.id),
                "{label}: a refused commit keeps the plan"
            );
        }
        h.service
            .deps
            .plans
            .0
            .lock()
            .unwrap()
            .insert(review.id.clone(), template);

        // The persisted run and candidate are checked again at commit time.
        type Stale = Box<dyn Fn(&Harness)>;
        let stale: Vec<(&str, Stale)> = vec![
            (
                "run finished",
                Box::new(|h| h.edit_run(|run| run.status = RunStatus::Failed)),
            ),
            (
                "another step",
                Box::new(|h| {
                    h.edit_run(|run| {
                        run.step_mut(StepId::CuratedNodes).status = StepStatus::Pending
                    })
                }),
            ),
            (
                "not a first run",
                Box::new(|h| h.edit_run(|run| run.request.mode = SetupMode::Repair)),
            ),
            (
                "run root",
                Box::new(|h| h.edit_run(|run| run.request.install_root = PathBuf::from("/moved"))),
            ),
            (
                "run candidate",
                Box::new(|h| {
                    h.edit_run(|run| run.target_env = EnvId::new("env-1").unwrap());
                }),
            ),
            (
                "run candidate path",
                Box::new(|h| {
                    h.edit_run(|run| run.python_target.env_path = PathBuf::from("/install/envs/x"))
                }),
            ),
            (
                "run profile revision",
                Box::new(|h| h.edit_run(|run| run.profile.revision += 1)),
            ),
        ];
        for (label, change) in stale {
            change(&h);
            assert_eq!(
                h.commit_setup(&run, &review, approve(&review)).unwrap_err(),
                NodeAppError::StalePlan,
                "{label}"
            );
            // Undo the change.
            h.setup_run(&["alpha"]);
        }
        // A candidate that is not provisioning, lacks its protected pins, or is the active
        // environment is refused too, and a running ComfyUI blocks the commit.
        h.edit_candidate(|env| env.mark_failed("t").unwrap());
        assert_eq!(
            h.commit_setup(&run, &review, approve(&review)).unwrap_err(),
            NodeAppError::EnvironmentUnavailable
        );
        h.setup_run(&["alpha"]);
        h.edit_candidate(|env| env.pins = PinSet::new());
        assert_eq!(
            h.commit_setup(&run, &review, approve(&review)).unwrap_err(),
            NodeAppError::EnvironmentUnavailable
        );
        h.setup_run(&["alpha"]);
        *h.world.active.lock().unwrap() = Some(EnvId::new(CANDIDATE).unwrap());
        assert_eq!(
            h.commit_setup(&run, &review, approve(&review)).unwrap_err(),
            NodeAppError::StalePlan
        );
        *h.world.active.lock().unwrap() = Some(EnvId::new("env-1").unwrap());
        *h.world.runtime_block.lock().unwrap() = Some(RuntimeBlock::Running);
        assert_eq!(
            h.commit_setup(&run, &review, approve(&review)).unwrap_err(),
            NodeAppError::RuntimeRunning
        );
        *h.world.runtime_block.lock().unwrap() = None;
        assert_eq!(h.world.count("files:publish"), 0, "nothing was published");
        // The exact binding commits.
        h.commit_setup(&run, &review, approve(&review)).unwrap();
        assert_eq!(h.world.count("files:publish"), 1);
    }

    #[test]
    fn curated_repository_mismatch_never_publishes() {
        let h = Harness::new();
        h.curated();
        let run = h.setup_run(&["alpha"]);
        let lease = &mut *block_on(h.service.deps.environment.gate.try_acquire()).unwrap();
        let ctx = step_context(&run);
        // Another repository, and a head that is not a full lowercase commit hash, are refused.
        let refused = [
            (Some("https://github.com/example/other"), None),
            (None, Some(("main", "A".repeat(40)))),
            (None, Some(("main", "abc".to_owned()))),
            (None, Some(("main", String::new()))),
        ];
        for (index, (repository, head)) in refused.into_iter().enumerate() {
            {
                let mut files = h.files.state.lock().unwrap();
                files.repository_override = repository.map(str::to_owned);
                files.default_head = head.map(|(branch, commit)| (branch.to_owned(), commit));
            }
            let result = block_on(SetupNodes::prepare(
                &h.service,
                &run,
                &spec("alpha"),
                &ctx,
                lease,
            ));
            assert_eq!(
                result.unwrap_err(),
                SetupNodeError::NotApplied(
                    "The repository did not resolve to the curated repository's default branch."
                        .into()
                ),
                "case {index}"
            );
            assert!(h.service.setup_reviews(&run).unwrap().is_empty());
            assert_eq!(
                h.world.count("files:discard"),
                index + 1,
                "the staging is dropped"
            );
            assert_eq!(h.world.count("files:publish"), 0);
        }
        // The curated repository at a resolved head prepares a review.
        {
            let mut files = h.files.state.lock().unwrap();
            files.repository_override = None;
            files.default_head = None;
        }
        let review = block_on(SetupNodes::prepare(
            &h.service,
            &run,
            &spec("alpha"),
            &ctx,
            lease,
        ))
        .expect("the staged source is the curated repository");
        assert_eq!(
            review.source,
            NodeSource::Git {
                repository: "https://github.com/example/alpha".into(),
                commit: "a".repeat(40),
                branch: Some("main".into())
            }
        );
        assert_eq!(h.service.setup_reviews(&run).unwrap().len(), 1);
        assert_eq!(h.world.count("files:publish"), 0);
    }

    #[test]
    fn curated_setup_prepares_the_default_branch_head() {
        let h = Harness::new();
        h.curated();
        let run = h.setup_run(&["alpha"]);
        let lease = &mut *block_on(h.service.deps.environment.gate.try_acquire()).unwrap();
        let ctx = step_context(&run);
        // The repository's default branch is whatever it is today, and its head is not pinned.
        let head = "0123456789abcdef0123456789abcdef01234567".to_owned();
        h.files.state.lock().unwrap().default_head = Some(("develop".into(), head.clone()));
        let review = block_on(SetupNodes::prepare(
            &h.service,
            &run,
            &spec("alpha"),
            &ctx,
            lease,
        ))
        .expect("any default branch is accepted");
        assert_eq!(
            h.files.state.lock().unwrap().references,
            [None],
            "the preparation asked for the default head, not for a commit"
        );
        assert_eq!(
            review.source,
            NodeSource::Git {
                repository: "https://github.com/example/alpha".into(),
                commit: head,
                branch: Some("develop".into())
            }
        );

        // Once installed the node is recognised by its repository, whichever head it tracks.
        let node = block_on(SetupNodes::commit(
            &h.service,
            &run,
            &review.id,
            approve(&review),
            &ctx,
            lease,
        ))
        .expect("the reviewed head installs")
        .node;
        assert!(matches!(
            node.source,
            NodeSource::Git { branch: Some(ref branch), .. } if branch == "develop"
        ));
        h.files.state.lock().unwrap().default_head = Some(("main".into(), "f".repeat(40)));
        assert!(
            block_on(SetupNodes::installed(
                &h.service,
                &run,
                &spec("alpha"),
                lease
            ))
            .unwrap(),
            "a node installed from the repository counts, even if the branch head moved on"
        );
        let other = crate::environment::tests::fakes::curated_spec("beta");
        assert!(!block_on(SetupNodes::installed(&h.service, &run, &other, lease)).unwrap());
    }

    #[test]
    fn curated_warnings_require_exact_review_acknowledgement() {
        let h = Harness::new();
        h.curated();
        let run = h.setup_run(&["alpha"]);
        let review = h.prepare_setup(&run, "alpha").unwrap();
        assert_eq!(review.warnings, [DETACHED]);
        let mut cases: Vec<(&str, CommitDecision)> = Vec::new();
        cases.push(("warnings not acknowledged", Harness::decision(&review)));
        let mut other_plan = approve(&review);
        other_plan.consent.plan_id = "node-other".into();
        cases.push(("another plan", other_plan));
        let mut other_binding = approve(&review);
        other_binding.consent.binding = "different".into();
        cases.push(("another binding", other_binding));
        let mut no_consent = approve(&review);
        no_consent.consent.execute_node_code = false;
        cases.push(("node code not acknowledged", no_consent));
        for (label, decision) in cases {
            assert_eq!(
                h.commit_setup(&run, &review, decision).unwrap_err(),
                NodeAppError::ConsentRequired,
                "{label}"
            );
        }
        assert_eq!(h.world.count("files:publish"), 0);
        assert_eq!(h.service.setup_reviews(&run).unwrap().len(), 1);
        h.commit_setup(&run, &review, approve(&review)).unwrap();
        assert_eq!(h.world.count("files:publish"), 1);
        // Through the setup port a refused decision keeps the plan and asks for consent again.
        let second = h.prepare_setup(&run, "beta").unwrap();
        let lease = &mut *block_on(h.service.deps.environment.gate.try_acquire()).unwrap();
        let refused = block_on(SetupNodes::commit(
            &h.service,
            &run,
            &second.id,
            Harness::decision(&second),
            &step_context(&run),
            lease,
        ));
        assert_eq!(refused.unwrap_err(), SetupNodeError::ConsentRequired);
        assert_eq!(h.service.setup_reviews(&run).unwrap().len(), 1);

        // The executor never acknowledges for the user: one unacknowledged warning keeps the
        // whole batch uninstalled, including the nodes before it.
        let flow = Flow::new();
        let id = flow.register(&["alpha", "beta"]);
        let reviews = flow.review_all(&id);
        let mut decisions: Vec<_> = reviews.iter().map(approve).collect();
        decisions[1].consent.acknowledge_warnings = false;
        flow.retry(&id, decisions);
        let RunOutcome::Failed(failure) = flow.execute(&id) else {
            panic!("the batch must wait for the acknowledgement");
        };
        assert_eq!(failure.kind, FailureKind::ConsentRequired);
        assert_eq!(flow.h.world.count("files:publish"), 0);
        assert_eq!(flow.reviews(&id).len(), 2);
        let decisions: Vec<_> = reviews.iter().map(approve).collect();
        flow.retry(&id, decisions);
        assert_eq!(flow.execute(&id), RunOutcome::Succeeded);
        assert_eq!(flow.h.world.count("files:publish"), 2);
    }

    /// A first run without an active environment, driven through the real executor.
    struct Flow {
        h: Harness,
        service: NodeService,
    }

    impl Flow {
        fn new() -> Self {
            let h = Harness::build(false);
            h.curated();
            let service = h.service.clone();
            Self { h, service }
        }

        fn deps(&self) -> crate::environment::EnvironmentDeps {
            self.service.deps.environment.clone()
        }

        fn register(&self, selected: &[&str]) -> RunId {
            block_on(
                StartSetup::new(self.deps()).execute(SetupAction::New(Box::new(request(selected)))),
            )
            .unwrap()
        }

        /// Registers a first run whose review stage carried the user's consent.
        fn register_consenting(&self, selected: &[&str]) -> RunId {
            block_on(
                StartSetup::new(self.deps())
                    .execute(SetupAction::New(Box::new(consenting_request(selected)))),
            )
            .unwrap()
        }

        fn execute(&self, id: &RunId) -> RunOutcome {
            block_on(
                ExecuteSetup::new(self.deps(), Arc::new(self.service.clone()))
                    .execute(id, Arc::new(NullSink)),
            )
            .unwrap()
        }

        /// Like `execute`, with the log lines of the run.
        fn execute_logged(&self, id: &RunId) -> (RunOutcome, Vec<String>) {
            let sink = Arc::new(crate::environment::tests::fakes::RecordingSink::default());
            let outcome = block_on(
                ExecuteSetup::new(self.deps(), Arc::new(self.service.clone()))
                    .execute(id, sink.clone()),
            )
            .unwrap();
            (outcome, sink.log_lines())
        }

        fn retry(&self, id: &RunId, decisions: Vec<CommitDecision>) -> RunId {
            block_on(StartSetup::new(self.deps()).execute(SetupAction::Retry {
                run_id: id.clone(),
                manager_consent: None,
                curated_decisions: decisions,
            }))
            .unwrap()
        }

        fn skip(&self, id: &RunId) -> Result<RunId, EnvironmentAppError> {
            block_on(StartSetup::new(self.deps()).execute(SetupAction::SkipStep {
                run_id: id.clone(),
                step: StepId::CuratedNodes,
            }))
        }

        fn reviews(&self, id: &RunId) -> Vec<PlanReview> {
            self.service.setup_reviews(id).unwrap()
        }

        fn run(&self, id: &RunId) -> crate::environment::ProvisioningRun {
            self.h.world.run(id)
        }

        /// Runs until the nodes are reviewed and returns their reviews in catalog order.
        fn review_all(&self, id: &RunId) -> Vec<PlanReview> {
            let RunOutcome::Failed(failure) = self.execute(id) else {
                panic!("the run must pause for the review");
            };
            assert_eq!(failure.step, StepId::CuratedNodes);
            assert_eq!(failure.kind, FailureKind::ConsentRequired);
            assert!(failure.consent.is_none() && failure.can_skip);
            let run = self.run(id);
            let reviews = self.reviews(id);
            run.curated
                .iter()
                .map(|entry| {
                    reviews
                        .iter()
                        .find(|review| Some(&review.id) == entry.plan.as_ref())
                        .expect("every prepared entry has a live review")
                        .clone()
                })
                .collect()
        }

        fn failure(&self, id: &RunId) -> crate::environment::StepFailure {
            self.run(id).failure.expect("the run recorded a failure")
        }
    }

    #[test]
    fn curated_setup_never_runs_install_py() {
        let flow = Flow::new();
        flow.h.files.state.lock().unwrap().hook_digest = Some("hook".into());
        let id = flow.register(&["alpha"]);
        let reviews = flow.review_all(&id);
        let review = &reviews[0];
        // The hook is explained, not offered.
        assert!(review.hook_digest.is_some());
        assert!(!review.has_install_py && !review.trusted_install_py);
        assert!(
            review
                .warnings
                .iter()
                .any(|warning| warning == HOOK_GUIDANCE)
        );
        // Even a decision that asks for the hook, source builds and remembered trust runs none.
        let mut decision = approve(review);
        decision.consent.run_install_py = true;
        decision.consent.remember_install_py = true;
        decision.consent.remember_source = true;
        decision.consent.allow_source_builds = true;
        flow.retry(&id, vec![decision]);
        assert_eq!(flow.execute(&id), RunOutcome::Succeeded);
        assert_eq!(flow.h.world.count("hook:run"), 0);
        assert!(flow.h.store.state.lock().unwrap().trusts.is_empty());
        let operation = flow.h.store.state.lock().unwrap().operations[0].clone();
        assert!(
            operation
                .consent_identity
                .as_deref()
                .is_some_and(|identity| identity.contains("hook=false")),
            "{:?}",
            operation.consent_identity
        );
        for spec in flow.h.world.installs.lock().unwrap().iter() {
            assert!(!spec.allow_source_builds);
        }
        let run = flow.run(&id);
        let step = run.step(StepId::CuratedNodes);
        assert_eq!(step.status, StepStatus::Warning);
        assert!(
            step.message
                .as_deref()
                .is_some_and(|message| message == format!("alpha: {HOOK_GUIDANCE}"))
        );
        assert_eq!(run.curated[0].message, step.message);
    }

    #[test]
    fn curated_hook_note_survives_retry() {
        let flow = Flow::new();
        flow.h.files.state.lock().unwrap().hook_digest = Some("hook".into());
        let id = flow.register(&["alpha", "beta"]);
        let reviews = flow.review_all(&id);
        flow.h
            .world
            .fail("verify_final", ProvisionError::Failed("final".into()));
        flow.h.world.succeed_first("verify_final", 2);
        flow.retry(&id, reviews.iter().map(approve).collect());
        assert!(matches!(flow.execute(&id), RunOutcome::Failed(_)));
        assert_eq!(flow.failure(&id).step, StepId::FinalVerify);
        let before = flow.run(&id);
        let note = before.step(StepId::CuratedNodes).message.clone().unwrap();
        assert_eq!(
            before.step(StepId::CuratedNodes).status,
            StepStatus::Warning
        );
        assert_eq!(
            note,
            format!("alpha: {HOOK_GUIDANCE}\nbeta: {HOOK_GUIDANCE}")
        );
        for (entry, name) in before.curated.iter().zip(["alpha", "beta"]) {
            assert_eq!(entry.message, Some(format!("{name}: {HOOK_GUIDANCE}")));
        }
        let installs = flow.h.world.count("install:node");
        flow.retry(&id, Vec::new());
        assert_eq!(flow.execute(&id), RunOutcome::Succeeded);
        let after = flow.run(&id);
        assert_eq!(after.step(StepId::CuratedNodes).status, StepStatus::Warning);
        assert_eq!(
            after.step(StepId::CuratedNodes).message.as_deref(),
            Some(note.as_str())
        );
        assert_eq!(after.curated, before.curated);
        assert_eq!(flow.h.world.count("files:prepare"), 2);
        assert_eq!(flow.h.world.count("files:publish"), 2);
        assert_eq!(flow.h.world.count("install:node"), installs);
        assert_eq!(flow.h.world.count("hook:run"), 0);
    }

    /// A first run whose only node was published but whose dependencies failed.
    fn damaged() -> (Flow, RunId) {
        let flow = Flow::new();
        let id = flow.register(&["alpha"]);
        // An environment that is active by now must never be touched by the setup nodes.
        flow.h
            .world
            .seed_active_env("env-old", PythonVersion::new(3, 13, 16));
        let reviews = flow.review_all(&id);
        flow.h
            .world
            .fail("install:node", ProvisionError::Failed("boom".into()));
        flow.retry(&id, vec![approve(&reviews[0])]);
        assert!(matches!(flow.execute(&id), RunOutcome::Failed(_)));
        (flow, id)
    }

    #[test]
    fn curated_dependency_failure_prevents_activation() {
        let (flow, id) = damaged();
        let failure = flow.failure(&id);
        assert_eq!(failure.step, StepId::CuratedNodes);
        assert_eq!(failure.kind, FailureKind::Failed);
        assert!(!failure.can_skip);
        let run = flow.run(&id);
        assert_eq!(run.status, RunStatus::Failed);
        assert_eq!(
            run.curated[0].status,
            crate::environment::CuratedStatus::Failed
        );
        assert!(run.curated[0].plan.is_none() && run.curated[0].operation.is_some());
        let candidate = flow.h.world.env(run.target_env.as_str());
        assert_eq!(candidate.state(), EnvState::Failed);
        // Failed outcomes belong to the candidate; the previous environment stays healthy.
        let outcomes = flow.h.store.state.lock().unwrap().dependencies.clone();
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].environment, run.target_env);
        assert_eq!(outcomes[0].state, DependencyState::Failed);
        let node = block_on(flow.h.store.nodes()).unwrap().remove(0);
        assert_eq!(node.dependencies, DependencyState::Failed);
        assert_eq!(flow.h.world.env("env-old").state(), EnvState::Verified);
        assert_eq!(flow.h.world.active_id().as_deref(), Some("env-old"));
        assert_eq!(flow.h.world.count("verify_final"), 0);
        assert_eq!(flow.h.world.count("commit_activation"), 0);
        assert!(matches!(
            *flow.h.world.manifest.lock().unwrap(),
            crate::environment::ManifestState::Absent
        ));
        assert_eq!(run.step(StepId::FinalVerify).status, StepStatus::Pending);
    }

    #[test]
    fn skip_cannot_hide_published_dependency_damage() {
        let (flow, id) = damaged();
        assert!(matches!(
            flow.skip(&id),
            Err(EnvironmentAppError::StepNotSkippable)
        ));
        assert_eq!(flow.run(&id).status, RunStatus::Failed);
        flow.retry(&id, Vec::new());
        assert!(matches!(flow.execute(&id), RunOutcome::Failed(_)));
        assert!(!flow.failure(&id).can_skip);
        // Even an otherwise skippable failure on a provisioning candidate cannot hide Failed.
        let target = flow.run(&id).target_env;
        flow.h
            .world
            .envs
            .lock()
            .unwrap()
            .get_mut(target.as_str())
            .unwrap()
            .begin_retry("t")
            .unwrap();
        flow.h
            .world
            .runs
            .lock()
            .unwrap()
            .get_mut(id.as_str())
            .unwrap()
            .failure
            .as_mut()
            .unwrap()
            .can_skip = true;
        assert!(matches!(
            flow.skip(&id),
            Err(EnvironmentAppError::StepNotSkippable)
        ));
        assert_eq!(flow.h.world.count("commit_activation"), 0);

        // Protected-package drift is not skippable either.
        let flow = Flow::new();
        let id = flow.register(&["alpha"]);
        let reviews = flow.review_all(&id);
        // The node install moves a protected package and the restore is refused.
        flow.h
            .world
            .drift_on_node
            .store(true, std::sync::atomic::Ordering::SeqCst);
        flow.h
            .world
            .fail("install:torch", ProvisionError::Failed("refused".into()));
        flow.retry(&id, vec![approve(&reviews[0])]);
        assert!(matches!(flow.execute(&id), RunOutcome::Failed(_)));
        assert!(!flow.failure(&id).can_skip);
        assert!(matches!(
            flow.skip(&id),
            Err(EnvironmentAppError::StepNotSkippable)
        ));
        assert_eq!(
            flow.run(&id).curated[0].status,
            crate::environment::CuratedStatus::Failed
        );
        flow.h
            .world
            .drift_on_node
            .store(false, std::sync::atomic::Ordering::SeqCst);
        flow.retry(&id, Vec::new());
        assert!(matches!(flow.execute(&id), RunOutcome::Failed(_)));
        assert!(!flow.failure(&id).can_skip);
        assert!(matches!(
            flow.skip(&id),
            Err(EnvironmentAppError::StepNotSkippable)
        ));

        // A failure before anything was published can be skipped; the run then activates.
        let flow = Flow::new();
        flow.h
            .files
            .state
            .lock()
            .unwrap()
            .prepare_failures
            .insert("alpha".into(), NodeAppError::Offline);
        let id = flow.register(&["alpha"]);
        assert!(matches!(flow.execute(&id), RunOutcome::Failed(_)));
        assert!(flow.failure(&id).can_skip);
        flow.skip(&id).unwrap();
        assert_eq!(flow.execute(&id), RunOutcome::Succeeded);
        let run = flow.run(&id);
        assert_eq!(run.step(StepId::CuratedNodes).status, StepStatus::Skipped);
        assert_eq!(
            flow.h.world.active_id().as_deref(),
            Some(run.target_env.as_str())
        );
        assert_eq!(flow.h.world.count("files:publish"), 0);
    }

    #[test]
    fn curated_retry_cannot_skip_unrepaired_published_damage() {
        for changed_tree in [false, true] {
            let (flow, id) = damaged();
            let old = flow.h.world.env("env-old");
            if changed_tree {
                // A lost progress update still leaves the journal as evidence of publication.
                let mut runs = flow.h.world.runs.lock().unwrap();
                let entry = &mut runs.get_mut(id.as_str()).unwrap().curated[0];
                entry.status = crate::environment::CuratedStatus::Prepared;
                entry.plan = entry.operation.take();
            }
            let stored = block_on(flow.h.store.nodes()).unwrap();
            {
                let mut files = flow.h.files.state.lock().unwrap();
                files.scan = stored;
                if changed_tree {
                    files
                        .snapshots
                        .insert("alpha".into(), "unreviewed-tree".into());
                } else {
                    files.requirements_digest = None;
                }
            }
            let installs = flow.h.world.count("install:node");
            flow.retry(&id, Vec::new());
            assert!(matches!(flow.execute(&id), RunOutcome::Failed(_)));
            let run = flow.run(&id);
            assert_eq!(flow.failure(&id).step, StepId::CuratedNodes);
            assert!(!flow.failure(&id).can_skip);
            assert_eq!(
                run.curated[0].status,
                crate::environment::CuratedStatus::Failed
            );
            assert!(matches!(
                flow.skip(&id),
                Err(EnvironmentAppError::StepNotSkippable)
            ));
            assert_eq!(flow.h.world.count("files:prepare:alpha"), 1);
            assert_eq!(flow.h.world.count("files:publish"), 1);
            assert_eq!(
                flow.h.world.count("install:node"),
                installs + usize::from(changed_tree)
            );
            assert_eq!(flow.h.world.count("commit_activation"), 0);
            assert_eq!(flow.h.world.active_id().as_deref(), Some("env-old"));
            assert_eq!(flow.h.world.env("env-old"), old);
            assert_eq!(
                flow.h.world.env(run.target_env.as_str()).state(),
                EnvState::Failed
            );
        }
    }

    #[test]
    fn curated_retry_recognises_a_repaired_node_and_activates() {
        let (flow, id) = damaged();
        let old = flow.h.world.env("env-old");
        flow.h.files.state.lock().unwrap().scan = block_on(flow.h.store.nodes()).unwrap();
        let installs = flow.h.world.count("install:node");
        flow.retry(&id, Vec::new());
        assert_eq!(flow.execute(&id), RunOutcome::Succeeded);
        let run = flow.run(&id);
        assert_eq!(
            run.curated[0].status,
            crate::environment::CuratedStatus::Installed
        );
        assert!(run.curated[0].plan.is_none() && run.curated[0].operation.is_some());
        assert_eq!(flow.h.world.count("files:prepare:alpha"), 1);
        assert_eq!(flow.h.world.count("files:publish"), 1);
        assert_eq!(flow.h.world.count("install:node"), installs + 1);
        assert_eq!(flow.h.world.count("commit_activation"), 1);
        assert_eq!(
            flow.h.world.active_id().as_deref(),
            Some(run.target_env.as_str())
        );
        assert_eq!(flow.h.world.env("env-old"), old);
    }

    #[test]
    fn curated_retry_is_idempotent_after_committed_entries() {
        let flow = Flow::new();
        let id = flow.register(&["alpha", "beta"]);
        flow.h
            .files
            .state
            .lock()
            .unwrap()
            .prepare_failures
            .insert("beta".into(), NodeAppError::Offline);
        // The second node cannot be prepared: nothing is installed, the first plan is kept.
        assert!(matches!(flow.execute(&id), RunOutcome::Failed(_)));
        let failure = flow.failure(&id);
        assert_eq!(
            (failure.kind, failure.can_skip),
            (FailureKind::Failed, true)
        );
        assert_eq!(flow.reviews(&id).len(), 1);
        flow.retry(&id, Vec::new());
        let reviews = flow.review_all(&id);
        assert_eq!(reviews.len(), 2);
        assert_eq!(flow.h.world.count("files:prepare:alpha"), 1, "no new clone");
        let decisions: Vec<_> = reviews.iter().map(approve).collect();
        // The run fails after both installs; the retry installs nothing again.
        flow.h
            .world
            .fail("verify_final", ProvisionError::Failed("final".into()));
        flow.h.world.succeed_first("verify_final", 2);
        flow.retry(&id, decisions);
        assert!(matches!(flow.execute(&id), RunOutcome::Failed(_)));
        assert_eq!(flow.failure(&id).step, StepId::FinalVerify);
        let run = flow.run(&id);
        assert!(run.curated.iter().all(|entry| entry.status
            == crate::environment::CuratedStatus::Installed
            && entry.plan.is_none()
            && entry.operation.is_some()));
        let installs = flow.h.world.count("install:node");
        assert_eq!(installs, 2);
        flow.retry(&id, Vec::new());
        assert_eq!(flow.execute(&id), RunOutcome::Succeeded);
        assert_eq!(flow.h.world.count("files:prepare:alpha"), 1);
        assert_eq!(
            flow.h.world.count("files:prepare:beta"),
            2,
            "one failed attempt"
        );
        assert_eq!(flow.h.world.count("files:publish"), 2);
        assert_eq!(flow.h.world.count("install:node"), installs);
        assert_eq!(
            flow.h.world.active_id().as_deref(),
            Some(flow.run(&id).target_env.as_str())
        );

        // A node installed with successful candidate outcomes is recognised even when the
        // record of its commit was lost: it is neither cloned nor installed again.
        let flow = Flow::new();
        let id = flow.register(&["alpha"]);
        let reviews = flow.review_all(&id);
        flow.h
            .world
            .fail("verify_final", ProvisionError::Failed("final".into()));
        flow.h.world.succeed_first("verify_final", 1);
        flow.retry(&id, vec![approve(&reviews[0])]);
        assert!(matches!(flow.execute(&id), RunOutcome::Failed(_)));
        flow.h
            .world
            .runs
            .lock()
            .unwrap()
            .get_mut(id.as_str())
            .unwrap()
            .curated[0] = crate::environment::CuratedProgress {
            id: "alpha".into(),
            status: crate::environment::CuratedStatus::Prepared,
            plan: Some("node-obsolete".into()),
            operation: None,
            message: None,
        };
        flow.retry(&id, Vec::new());
        assert_eq!(flow.execute(&id), RunOutcome::Succeeded);
        assert_eq!(flow.h.world.count("files:prepare:alpha"), 1);
        assert_eq!(flow.h.world.count("files:publish"), 1);
        assert_eq!(flow.h.world.count("install:node"), 1);
    }

    #[test]
    fn plans_of_a_cancelled_or_skipped_run_are_discarded_with_their_staging() {
        let flow = Flow::new();
        let id = flow.register(&["alpha", "beta"]);
        flow.review_all(&id);
        // A run that waits for the user keeps its reviews through a reconcile.
        block_on(flow.service.reconcile()).unwrap();
        assert_eq!(flow.reviews(&id).len(), 2);
        assert_eq!(flow.h.world.count("files:discard"), 0);
        block_on(crate::environment::CancelSetup::new(flow.deps()).execute(&id)).unwrap();
        block_on(flow.service.reconcile()).unwrap();
        assert!(flow.reviews(&id).is_empty());
        assert_eq!(flow.h.world.count("files:discard"), 2);
        let operations = flow.h.store.state.lock().unwrap().operations.clone();
        assert!(
            operations
                .iter()
                .all(|op| op.phase == OperationPhase::Failed)
        );

        // Skipping the step abandons the reviews as well.
        let flow = Flow::new();
        flow.h
            .files
            .state
            .lock()
            .unwrap()
            .prepare_failures
            .insert("beta".into(), NodeAppError::Offline);
        let id = flow.register(&["alpha", "beta"]);
        assert!(matches!(flow.execute(&id), RunOutcome::Failed(_)));
        assert_eq!(flow.reviews(&id).len(), 1);
        flow.skip(&id).unwrap();
        block_on(flow.service.reconcile()).unwrap();
        assert!(flow.reviews(&id).is_empty());
        assert_eq!(flow.h.world.count("files:discard"), 1);
        assert_eq!(flow.h.world.count("files:publish"), 0);
    }

    #[test]
    fn backend_restart_requires_fresh_curated_consent() {
        let mut flow = Flow::new();
        let id = flow.register(&["alpha", "beta"]);
        let old = flow.review_all(&id);
        let old_decisions: Vec<_> = old.iter().map(approve).collect();
        assert_eq!(flow.run(&id).curated.len(), 2);

        // A restart empties the process-local plans and decisions, then settles the journal.
        let mut deps = flow.service.deps.clone();
        deps.plans = Arc::default();
        deps.environment.active_runs = Arc::default();
        flow.service = NodeService::new(deps);
        flow.h.files.state.lock().unwrap().recover = Some(RecoveryAction::Discarded);
        block_on(flow.service.reconcile()).unwrap();
        assert!(flow.reviews(&id).is_empty());
        let mut lease = block_on(flow.service.deps.environment.gate.try_acquire()).unwrap();
        for review in &old {
            let result = block_on(flow.service.commit_for_setup_held(
                &id,
                &review.id,
                approve(review),
                &flow.h.ctx(),
                &mut *lease,
            ));
            assert_eq!(result.unwrap_err(), NodeAppError::StalePlan);
        }
        drop(lease);

        // The obsolete decisions never replay: the nodes are reviewed again.
        flow.retry(&id, old_decisions);
        let RunOutcome::Failed(failure) = flow.execute(&id) else {
            panic!("fresh consent is required");
        };
        assert_eq!(failure.kind, FailureKind::ConsentRequired);
        assert_eq!(flow.h.world.count("files:publish"), 0);
        let fresh = flow.reviews(&id);
        assert_eq!(fresh.len(), 2);
        for review in &fresh {
            assert!(old.iter().all(|old| old.id != review.id));
            assert!(old.iter().all(|old| old.binding != review.binding));
        }
        let run = flow.run(&id);
        let planned: Vec<_> = run.curated.iter().filter_map(|e| e.plan.clone()).collect();
        assert!(fresh.iter().all(|review| planned.contains(&review.id)));

        // Only decisions for the new plans install anything.
        let decisions: Vec<_> = run
            .curated
            .iter()
            .map(|entry| {
                approve(
                    fresh
                        .iter()
                        .find(|review| Some(&review.id) == entry.plan.as_ref())
                        .unwrap(),
                )
            })
            .collect();
        flow.retry(&id, decisions);
        assert_eq!(flow.execute(&id), RunOutcome::Succeeded);
        assert_eq!(flow.h.world.count("files:publish"), 2);
    }

    #[test]
    fn curated_auto_consent_installs_every_node_and_logs_warnings() {
        let flow = Flow::new();
        flow.h.files.state.lock().unwrap().hook_digest = Some("hook".into());
        let cache_warning =
            "This repository ships __pycache__ content; consented first-run setup allows it.";
        let native_warning = "This node has top-level native-image entries that tools may load; consented first-run setup allows them.";
        flow.h
            .files
            .state
            .lock()
            .unwrap()
            .curated_warnings
            .extend([cache_warning.into(), native_warning.into()]);
        let id = flow.register_consenting(&["alpha", "beta"]);
        let (outcome, log) = flow.execute_logged(&id);
        // No pause: every reviewed plan is answered from the consent given on the review page.
        assert_eq!(outcome, RunOutcome::Succeeded);
        let run = flow.run(&id);
        assert!(run.request.third_party_consent);
        assert!(run.curated.iter().all(|entry| {
            entry.status == crate::environment::CuratedStatus::Installed
                && entry.plan.is_none()
                && entry.operation.is_some()
        }));
        assert_eq!(flow.h.world.count("files:publish"), 2);
        assert_eq!(flow.h.world.count("install:node"), 2);
        assert_eq!(
            flow.h.files.state.lock().unwrap().references,
            [None, None],
            "both nodes were prepared at their default branch head"
        );
        // Every present hook runs before requirements; source builds and trust remain disabled.
        assert_eq!(flow.h.world.count("hook:run"), 2);
        assert!(flow.h.store.state.lock().unwrap().trusts.is_empty());
        for operation in flow.h.store.state.lock().unwrap().operations.iter() {
            assert!(
                operation
                    .consent_identity
                    .as_deref()
                    .is_some_and(|identity| identity.contains("hook=true")),
                "{:?}",
                operation.consent_identity
            );
        }
        for spec in flow.h.world.installs.lock().unwrap().iter() {
            assert!(!spec.allow_source_builds);
        }
        // Content findings are acknowledged, and successful hooks leave no skipped-hook note.
        for name in ["alpha", "beta"] {
            for warning in [DETACHED, cache_warning, native_warning] {
                let line = format!("{name}: {warning} (acknowledged on the setup review page).");
                assert!(log.contains(&line), "{line}\n{log:#?}");
            }
            // The resolved head is logged before the node is installed.
            let resolved = format!(
                "Node-{name}: main from https://github.com/example/{name} at commit {}.",
                "a".repeat(40)
            );
            let installing = format!("Installing {name}.");
            let at = |wanted: &str| log.iter().position(|line| line == wanted);
            assert!(at(&resolved) < at(&installing), "{log:#?}");
            assert!(at(&resolved).is_some() && at(&installing).is_some());
            assert!(log.contains(&format!(
                "Preparing Node-{name} from the latest commit of its default branch."
            )));
            let hook = format!("{name}: running approved install.py before node requirements.");
            let checked = format!("{name}: checking reviewed requirements after install.py.");
            let requirements = format!("{name}: installing reviewed node requirements.");
            assert!(at(&hook).is_some() && at(&checked).is_some() && at(&requirements).is_some());
            assert!(at(&hook) < at(&checked) && at(&checked) < at(&requirements));
        }
        assert!(run.curated.iter().all(|entry| entry.message.is_none()));
        assert_eq!(run.step(StepId::CuratedNodes).status, StepStatus::Succeeded);
        assert!(log.iter().all(|line| !line.contains(HOOK_GUIDANCE)));
        let calls = flow.h.world.calls();
        for (hook, requirements) in calls
            .iter()
            .enumerate()
            .filter(|(_, call)| *call == "hook:run")
            .map(|(hook, _)| {
                (
                    hook,
                    calls
                        .iter()
                        .enumerate()
                        .skip(hook)
                        .find(|(_, call)| *call == "install:node")
                        .unwrap()
                        .0,
                )
            })
        {
            assert!(hook < requirements);
        }

        let no_hook = Flow::new();
        let id = no_hook.register_consenting(&["alpha"]);
        assert_eq!(no_hook.execute(&id), RunOutcome::Succeeded);
        assert_eq!(no_hook.h.world.count("hook:run"), 0);

        // A covering decision from an earlier attempt cannot suppress a current reviewed hook.
        let retry = Flow::new();
        retry.h.files.state.lock().unwrap().hook_digest = Some("hook".into());
        retry
            .h
            .files
            .state
            .lock()
            .unwrap()
            .prepare_failures
            .insert("beta".into(), NodeAppError::Offline);
        let id = retry.register_consenting(&["alpha", "beta"]);
        assert!(matches!(retry.execute(&id), RunOutcome::Failed(_)));
        let old = retry.reviews(&id);
        assert_eq!(old.len(), 1);
        assert!(old[0].has_install_py);
        let old_decision = approve(&old[0]);
        assert!(!old_decision.consent.run_install_py);
        retry.retry(&id, vec![old_decision]);
        assert_eq!(retry.execute(&id), RunOutcome::Succeeded);
        assert_eq!(retry.h.world.count("hook:run"), 2);
    }

    #[test]
    fn curated_auto_consent_resynthesizes_after_restart() {
        let mut flow = Flow::new();
        // The second node cannot be prepared, so the first plan waits uncommitted.
        flow.h
            .files
            .state
            .lock()
            .unwrap()
            .prepare_failures
            .insert("beta".into(), NodeAppError::Offline);
        let id = flow.register_consenting(&["alpha", "beta"]);
        assert!(matches!(flow.execute(&id), RunOutcome::Failed(_)));
        let old = flow.reviews(&id);
        assert_eq!(old.len(), 1);
        assert_eq!(flow.h.world.count("files:publish"), 0);

        // A restart empties the process-local plans and decisions, then settles the journal.
        let mut deps = flow.service.deps.clone();
        deps.plans = Arc::default();
        deps.environment.active_runs = Arc::default();
        flow.service = NodeService::new(deps);
        flow.h.files.state.lock().unwrap().recover = Some(RecoveryAction::Discarded);
        block_on(flow.service.reconcile()).unwrap();
        assert!(flow.reviews(&id).is_empty());

        // The flag is part of the persisted run: a plain retry installs without any decision.
        assert!(flow.run(&id).request.third_party_consent);
        flow.retry(&id, Vec::new());
        let (outcome, log) = flow.execute_logged(&id);
        assert_eq!(outcome, RunOutcome::Succeeded);
        assert_eq!(flow.h.world.count("files:publish"), 2);
        let run = flow.run(&id);
        assert!(
            run.curated
                .iter()
                .all(|entry| entry.status == crate::environment::CuratedStatus::Installed)
        );
        assert!(
            run.curated
                .iter()
                .all(|entry| entry.operation.as_deref() != Some(old[0].id.as_str())),
            "the nodes were reviewed again, not replayed from the lost plan"
        );
        assert!(log.contains(&format!(
            "alpha: {DETACHED} (acknowledged on the setup review page)."
        )));
    }
}
