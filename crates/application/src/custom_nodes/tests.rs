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
    staged: Option<StagedNode>,
    digest: String,
    requirements_digest: Option<String>,
    hook_digest: Option<String>,
    publish_error: Option<NodeAppError>,
    recover: Option<RecoveryAction>,
    recycle_fails: bool,
    scan: Vec<InstalledNode>,
    scan_error: Option<NodeAppError>,
    snapshots: std::collections::HashMap<String, String>,
    /// Overrides the tree digest a guarded location reports (simulated drift).
    pin_digest: Option<String>,
    pin_error: Option<NodeAppError>,
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
}

impl NodeLocationGuard for FakeGuard {
    fn directory(&self) -> &Path {
        &self.directory
    }
    fn requirements(&self) -> Option<Arc<[u8]>> {
        self.snapshot.requirements_digest.as_ref().map(|_| {
            Arc::from(
                &b"numpy
"[..],
            )
        })
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
        _request: &PrepareRequest,
        _prior: Option<&InstalledNode>,
        _ctx: &crate::environment::StepContext,
    ) -> Result<StagedNode, NodeAppError> {
        self.world.call("files:prepare");
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
    ) -> Result<TreeSnapshot, NodeAppError> {
        let state = self.state.lock().unwrap();
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
            None => base.join(node.name.as_str()),
        }
    }
    async fn pin_location(
        &self,
        _root: &Path,
        node: &InstalledNode,
        staged: Option<&NodeId>,
    ) -> Result<Box<dyn NodeLocationGuard>, NodeAppError> {
        let state = self.state.lock().unwrap();
        if let Some(error) = &state.pin_error {
            return Err(error.clone());
        }
        self.world.call(if staged.is_some() {
            "guard:pin:staged"
        } else {
            "guard:pin:live"
        });
        Ok(Box::new(FakeGuard {
            world: self.world.clone(),
            directory: self.path(Path::new(ROOT), node, staged),
            snapshot: TreeSnapshot {
                digest: state
                    .pin_digest
                    .clone()
                    .unwrap_or_else(|| node.tree_digest.clone()),
                requirements_digest: state.requirements_digest.clone(),
                hook_digest: state.hook_digest.clone(),
                requirement_lines: vec!["numpy".into()],
            },
            resources: Arc::new(()),
        }))
    }
    async fn publish(&self, _root: &Path, _op: &NodeOperation) -> Result<(), NodeAppError> {
        self.world.call("files:publish");
        match self.state.lock().unwrap().publish_error.take() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
    async fn backup(&self, _root: &Path, _op: &NodeOperation) -> Result<(), NodeAppError> {
        self.world.call("files:backup");
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
        if self.state.lock().unwrap().recycle_fails {
            Err(NodeAppError::Filesystem)
        } else {
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
    ) -> Result<RecoveryAction, NodeAppError> {
        self.world.call("files:recover");
        Ok(self
            .state
            .lock()
            .unwrap()
            .recover
            .unwrap_or(RecoveryAction::Blocked))
    }
    async fn scan(&self, _root: &Path) -> Result<Vec<InstalledNode>, NodeAppError> {
        self.world.call("files:scan");
        let state = self.state.lock().unwrap();
        match &state.scan_error {
            Some(error) => Err(error.clone()),
            None => Ok(state.scan.clone()),
        }
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

struct FakeRegistry;

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
    journal_error: Option<NodeAppError>,
}

struct FakeStore {
    world: Arc<World>,
    state: Mutex<StoreState>,
}

#[async_trait]
impl NodeRepository for FakeStore {
    async fn nodes(&self) -> Result<Vec<InstalledNode>, NodeAppError> {
        Ok(self.state.lock().unwrap().nodes.clone())
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
        state.operations.retain(|o| o.id != op.id);
        state.operations.push(op.clone());
        Ok(())
    }
    async fn operations(&self) -> Result<Vec<NodeOperation>, NodeAppError> {
        let state = self.state.lock().unwrap();
        match &state.journal_error {
            Some(error) => Err(error.clone()),
            None => Ok(state.operations.clone()),
        }
    }
    async fn save_dependency(&self, outcome: &DependencyOutcome) -> Result<(), NodeAppError> {
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
    fails: Mutex<bool>,
}

#[async_trait]
impl NodeHook for FakeHook {
    async fn run(
        &self,
        _root: &Path,
        _env: &PythonEnvironment,
        location: &dyn NodeLocationGuard,
        _ctx: &crate::environment::StepContext,
    ) -> Result<(), NodeAppError> {
        assert!(location.install_script().is_some());
        self.world.call("hook:run");
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
        let world = World::new();
        world.seed_active_env("env-1", PythonVersion::new(3, 13, 16));
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
            fails: Mutex::new(false),
        });
        let service = NodeService::new(NodeDeps {
            environment: world.deps(),
            registry: Arc::new(FakeRegistry),
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
        registry_source("1.1.0").identity()
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
fn install_py_runs_only_with_consent_after_requirements_and_trust_is_namespaced() {
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
        calls.iter().position(|c| c == "install:node").unwrap()
            < calls.iter().position(|c| c == "hook:run").unwrap()
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
    assert_eq!(h.world.count("files:recover"), 0);
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
fn repair_collects_requirement_files_of_enabled_and_disabled_nodes() {
    let h = Harness::new();
    let enabled = h.installed();
    let mut disabled = enabled.clone();
    disabled.id = NodeId::new("n-off").unwrap();
    disabled.name = SafeFileName::new("off").unwrap();
    disabled.enabled = false;
    block_on(h.store.save_node(&disabled)).unwrap();
    let mut single = enabled.clone();
    single.id = NodeId::new("n-solo").unwrap();
    single.name = SafeFileName::new("solo.py").unwrap();
    single.single_file = true;
    block_on(h.store.save_node(&single)).unwrap();
    let files = block_on(h.service.repair_requirements()).unwrap();
    assert_eq!(
        files.len(),
        2,
        "single-file nodes have no requirements: {files:?}"
    );
    assert!(files.iter().all(|f| f.ends_with("requirements.txt")));
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
fn node_execution_guard_spans_preview_install_hook_and_cleanup() {
    let h = Harness::new();
    let review = hook_plan(&h);
    // A drifting requirements install and a failing script force the cleanup reassertion.
    h.world
        .drift_on_requirements
        .store(true, std::sync::atomic::Ordering::SeqCst);
    *h.hook.fails.lock().unwrap() = true;
    let result = block_on(
        h.service
            .commit(&review.id, hook_decision(&review), &h.ctx()),
    );
    assert_eq!(result.unwrap_err(), NodeAppError::DependenciesFailed);
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
    // The live guard is acquired after publication and outlives installation, the script and the
    // pin reassertion that follows it.
    let live = at("guard:pin:live", 0);
    assert!(at("files:publish", 0) < live);
    assert!(live < at("install:node", 0));
    assert!(at("install:node", 0) < at("hook:run", 0));
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
    assert!(!guarded.retained.is_empty());
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
