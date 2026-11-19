use super::*;
use crate::shared::{CancelToken, ExplorerTarget, MaintenanceLease, Retained, WithNodeLock};
use domain::{custom_nodes::*, environment::EnvState};
use std::time::Duration;

/// How long a node operation waits for a cancelled scan to release the node lock.
pub(crate) const SCAN_YIELD_WAIT: Duration = Duration::from_secs(5);

/// A scan's settle step: the maintenance lease it just took plus the node lock it already holds,
/// so subprocess survivors retain both.
struct BothLeases<'a>(&'a mut dyn MaintenanceLease, &'a dyn MaintenanceLease);

impl MaintenanceLease for BothLeases<'_> {
    fn retained(&self) -> Retained {
        self.0.retained().merge(self.1.retained())
    }
}

#[derive(Clone)]
pub struct NodeService {
    pub(crate) deps: NodeDeps,
}

impl NodeService {
    pub fn new(deps: NodeDeps) -> Self {
        Self { deps }
    }
    pub fn scans(&self) -> &std::sync::Arc<ScanControl> {
        &self.deps.scans
    }
    /// Takes the node lock for an operation that already holds the maintenance gate (or for
    /// setup, which holds it for the whole run). A running scan is cancelled and new ones stay
    /// off while this waits; the wait is bounded, and Busy when the lock stays taken.
    ///
    /// Lock order: maintenance gate, then node lock. Whoever holds the node lock only ever
    /// try-acquires the maintenance gate, so this wait cannot be part of a cycle.
    pub(crate) async fn lock_nodes(&self) -> Result<Box<dyn MaintenanceLease>, NodeAppError> {
        let _yield = self.deps.scans.yield_to_operation();
        self.deps
            .node_lock
            .acquire_within(SCAN_YIELD_WAIT)
            .await
            .map_err(|_| NodeAppError::Busy)
    }
    pub async fn search(
        &self,
        query: RegistryQuery,
        refresh: bool,
        ttl_seconds: u64,
    ) -> Result<CachedPage, NodeAppError> {
        self.deps.registry.search(query, refresh, ttl_seconds).await
    }
    pub async fn versions(&self, id: &str) -> Result<Vec<RegistryRelease>, NodeAppError> {
        self.deps.registry.versions(id).await
    }
    pub async fn list(&self) -> Result<Vec<InstalledNode>, NodeAppError> {
        let mut nodes = self.deps.store.nodes().await?;
        nodes.retain(|node| !(node.enabled && is_comfyui_shipped(node.name.as_str())));
        self.project_dependencies(&mut nodes).await?;
        Ok(nodes)
    }
    async fn project_dependencies(&self, nodes: &mut [InstalledNode]) -> Result<(), NodeAppError> {
        let active = self
            .deps
            .environment
            .envs
            .active()
            .await
            .map_err(|_| NodeAppError::Storage)?;
        for node in nodes {
            if node.conflict || node.tree_digest.is_empty() {
                node.dependencies = DependencyState::NotRun;
                continue;
            }
            node.dependencies = match &active {
                Some(env) => self
                    .deps
                    .store
                    .dependency(&node.id, &env.id, &node.dependency_identity())
                    .await?
                    .unwrap_or(DependencyState::NotRun),
                None => DependencyState::NotRun,
            };
        }
        Ok(())
    }
    pub async fn git_status(&self, id: &NodeId) -> Result<NodeGitStatus, NodeAppError> {
        let lease = self
            .deps
            .environment
            .gate
            .try_acquire()
            .await
            .map_err(|_| NodeAppError::Busy)?;
        let node = self
            .deps
            .store
            .nodes()
            .await?
            .into_iter()
            .find(|node| &node.id == id)
            .ok_or(NodeAppError::NotFound)?;
        if node.single_file || !matches!(node.source, NodeSource::Git { .. }) {
            return Err(NodeAppError::InvalidRequest);
        }
        let root = self.deps.environment.root.current().root;
        self.deps
            .files
            .git_status(&root, &node, lease.retained())
            .await
    }
    /// Process-local reviews only; discovering a review never restores consent.
    pub fn pending_reviews(&self) -> Result<Vec<PlanReview>, NodeAppError> {
        let plans = self
            .deps
            .plans
            .0
            .lock()
            .map_err(|_| NodeAppError::Storage)?;
        let mut reviews: Vec<_> = plans
            .values()
            .filter(|plan| {
                // Setup plans belong to their run; only that run's review commits them.
                plan.target == NodeTarget::Active
                    && matches!(
                        plan.review.kind,
                        OperationKind::Install
                            | OperationKind::Update
                            | OperationKind::RetryDependencies
                    )
            })
            .map(|plan| plan.review.clone())
            .collect();
        reviews.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(reviews)
    }
    /// Newest first, bounded to the latest 16 terminal journal records.
    pub async fn recent_operations(&self) -> Result<Vec<NodeOperation>, NodeAppError> {
        Ok(self
            .deps
            .store
            .operations()
            .await?
            .into_iter()
            .rev()
            .filter(|op| matches!(op.phase, OperationPhase::Completed | OperationPhase::Failed))
            .take(16)
            .collect())
    }
    pub async fn operation_status(&self, id: &str) -> Result<NodeOperation, NodeAppError> {
        self.deps
            .store
            .operations()
            .await?
            .into_iter()
            .find(|op| op.id.as_str() == id)
            .ok_or(NodeAppError::NotFound)
    }
    pub async fn resolve_operation(&self, id: &str) -> Result<(), NodeAppError> {
        let mut gate = self
            .deps
            .environment
            .gate
            .try_acquire()
            .await
            .map_err(|_| NodeAppError::Busy)?;
        let root = self.deps.environment.root.current().root;
        self.deps
            .environment
            .runtime
            .ensure_stopped(&root)
            .await
            .map_err(|_| NodeAppError::RuntimeRunning)?;
        let mut held = WithNodeLock {
            lease: &mut *gate,
            nodes: self.lock_nodes().await?,
        };
        let mut op = self.operation_status(id).await?;
        if !op.blocks_start() {
            return Ok(());
        }
        self.deps
            .plans
            .0
            .lock()
            .map_err(|_| NodeAppError::Storage)?
            .remove(id);
        if op.phase == OperationPhase::DependenciesInProgress {
            self.mark_environment_repair_required(&op.environment)
                .await?;
        }
        op.phase = OperationPhase::Failed;
        op.failure = Some("Closed by the user. The files were kept as they are; any preserved copies stay in custom_nodes/.disabled.".into());
        self.deps.store.save_operation(&op).await?;
        let Some(activity) = self.deps.scans.begin(&held) else {
            self.deps.scans.mark_incomplete();
            return Err(NodeAppError::Cancelled);
        };
        let result = self.commit_observation(&mut held, activity.token()).await;
        activity.finish(result).map(|_| ())
    }
    /// True when the selected environment needs repair; node operations are limited until then.
    pub async fn repair_required(&self) -> Result<bool, NodeAppError> {
        let active = self
            .deps
            .environment
            .envs
            .active()
            .await
            .map_err(|_| NodeAppError::Storage)?;
        Ok(active.is_some_and(|env| env.state() == EnvState::RepairRequired))
    }
    /// Operations that are neither completed nor failed.
    pub async fn unresolved(&self) -> Result<Vec<NodeOperation>, NodeAppError> {
        Ok(self
            .deps
            .store
            .operations()
            .await?
            .into_iter()
            .filter(|op| !matches!(op.phase, OperationPhase::Completed | OperationPhase::Failed))
            .collect())
    }
    pub async fn prepare(
        &self,
        request: PrepareRequest,
        ctx: &NodeContext,
    ) -> Result<PlanReview, NodeAppError> {
        let mut gate = self
            .deps
            .environment
            .gate
            .try_acquire()
            .await
            .map_err(|_| NodeAppError::Busy)?;
        let mut held = WithNodeLock {
            lease: &mut *gate,
            nodes: self.lock_nodes().await?,
        };
        self.prepare_held(request, ctx, &mut held).await
    }
    pub async fn commit(
        &self,
        id: &str,
        decision: CommitDecision,
        ctx: &NodeContext,
    ) -> Result<NodeOutcome, NodeAppError> {
        let mut gate = self
            .deps
            .environment
            .gate
            .try_acquire()
            .await
            .map_err(|_| NodeAppError::Busy)?;
        let mut held = WithNodeLock {
            lease: &mut *gate,
            nodes: self.lock_nodes().await?,
        };
        self.commit_held(id, decision, ctx, &mut held).await
    }
    /// Enable, disable and uninstall: the user's click is the confirmation, because the plan has
    /// no source to review, no hook and no packages. One lease spans prepare and commit.
    pub async fn run_direct(
        &self,
        request: PrepareRequest,
        ctx: &NodeContext,
    ) -> Result<NodeOutcome, NodeAppError> {
        if !matches!(
            request,
            PrepareRequest::SetEnabled { .. } | PrepareRequest::Uninstall { .. }
        ) {
            return Err(NodeAppError::InvalidRequest);
        }
        let mut gate = self
            .deps
            .environment
            .gate
            .try_acquire()
            .await
            .map_err(|_| NodeAppError::Busy)?;
        let mut held = WithNodeLock {
            lease: &mut *gate,
            nodes: self.lock_nodes().await?,
        };
        let review = self.prepare_held(request, ctx, &mut held).await?;
        let decision = CommitDecision {
            consent: InstallConsent {
                plan_id: review.id.clone(),
                binding: review.binding.clone(),
                execute_node_code: true,
                run_install_py: false,
                allow_source_builds: false,
                acknowledge_warnings: !review.warnings.is_empty(),
                remember_source: false,
                remember_install_py: false,
            },
            protected_changes: review.protected_changes.clone(),
        };
        self.commit_held(&review.id, decision, ctx, &mut held).await
    }
    /// Drops a reviewed-but-unused plan and removes its owned staging.
    pub async fn discard(&self, id: &str) -> Result<(), NodeAppError> {
        let mut lease = self
            .deps
            .environment
            .gate
            .try_acquire()
            .await
            .map_err(|_| NodeAppError::Busy)?;
        self.discard_held(id, &mut *lease).await
    }
    pub async fn discard_held(
        &self,
        id: &str,
        _lease: &mut dyn MaintenanceLease,
    ) -> Result<(), NodeAppError> {
        let setup = self
            .deps
            .plans
            .0
            .lock()
            .map_err(|_| NodeAppError::Storage)?
            .get(id)
            .is_some_and(|plan| plan.target != NodeTarget::Active);
        if setup {
            return Err(NodeAppError::StalePlan);
        }
        self.discard_plan(id).await
    }
    /// Drops a reviewed plan of any target and removes its owned staging.
    pub(crate) async fn discard_plan(&self, id: &str) -> Result<(), NodeAppError> {
        let plan = self.deps.take_plan(id)?;
        let mut op = plan.operation;
        self.deps.files.discard_staging(&plan.root, &op.id).await?;
        op.phase = OperationPhase::Failed;
        op.failure = Some("The reviewed plan was discarded".into());
        self.deps.store.save_operation(&op).await
    }
    /// All plans were prepared and reviewed first. The lease spans the complete sequential batch.
    pub async fn update_all(
        &self,
        decisions: Vec<(String, CommitDecision)>,
        ctx: &NodeContext,
    ) -> Result<BatchOutcome, NodeAppError> {
        let mut gate = self
            .deps
            .environment
            .gate
            .try_acquire()
            .await
            .map_err(|_| NodeAppError::Busy)?;
        let mut held = WithNodeLock {
            lease: &mut *gate,
            nodes: self.lock_nodes().await?,
        };
        let mut changes_packages = false;
        {
            let plans = self
                .deps
                .plans
                .0
                .lock()
                .map_err(|_| NodeAppError::Storage)?;
            for (id, decision) in &decisions {
                match plans.get(id) {
                    Some(plan)
                        if plan.operation.kind == OperationKind::Update
                            && plan.target == NodeTarget::Active =>
                    {
                        changes_packages |=
                            plan.package.is_some() || decision.consent.run_install_py;
                    }
                    _ => return Err(NodeAppError::StalePlan),
                }
            }
        }
        // Checked for the whole batch up front, so nothing is committed or discarded when a
        // program outside this app runs Python from the environment the updates would change.
        if changes_packages
            && let Some(env) = self
                .deps
                .environment
                .envs
                .active()
                .await
                .map_err(|_| NodeAppError::Storage)?
            && let Some(user) = self.deps.environment.env_users.first_user(&env.path).await
        {
            return Err(NodeAppError::EnvironmentProcessRunning(user));
        }
        let mut batch = BatchOutcome {
            results: Vec::new(),
            stopped_for_repair: false,
        };
        let mut remaining: Vec<String> = decisions.iter().map(|(id, _)| id.clone()).collect();
        let mut storage_error = None;
        for (id, decision) in decisions {
            remaining.retain(|r| r != &id);
            let outcome = self.commit_held(&id, decision, ctx, &mut held).await;
            let blocked = match self.ensure_reconciled().await {
                Ok(()) => false,
                Err(NodeAppError::RecoveryRequired) => true,
                Err(error) => {
                    storage_error = Some(error);
                    false
                }
            };
            let damaged = match self.deps.environment.envs.active().await {
                Ok(env) => env.is_none_or(|env| env.state() != EnvState::Verified),
                Err(_) => {
                    storage_error = Some(NodeAppError::Storage);
                    false
                }
            };
            batch.results.push((id, outcome));
            if damaged || blocked || ctx.cancel.is_cancelled() || storage_error.is_some() {
                batch.stopped_for_repair = damaged;
                break;
            }
        }
        // Targets that did not run keep no staging: the batch stopped before they were committed.
        for id in remaining {
            batch
                .results
                .push((id.clone(), Err(NodeAppError::Cancelled)));
            let _ = self.discard_held(&id, &mut held).await;
        }
        if let Some(error) = storage_error {
            return Err(error);
        }
        Ok(batch)
    }
    /// Desktop calls this from the Rescan button. It takes the node lock only, never the
    /// maintenance gate for its whole run, so Start, Stop, Restart and Update ComfyUI neither
    /// wait for it nor fail because of it; a node operation cancels it instead. Busy when a node
    /// operation, setup or another scan holds the node lock.
    ///
    /// Interrupted operations are settled first, but only when ComfyUI is verified stopped and
    /// the maintenance gate is free right now, because settling moves folders. Otherwise the scan
    /// only adds and updates rows and reuses stored digests, so it never deletes a node whose
    /// folder a running ComfyUI may be touching.
    pub async fn rescan(&self) -> Result<Vec<InstalledNode>, NodeAppError> {
        let nodes = self
            .deps
            .node_lock
            .try_acquire()
            .await
            .map_err(|_| NodeAppError::Busy)?;
        self.rescan_held(nodes).await
    }
    /// Same as [`Self::rescan`] for the background scan: it queues behind a node operation that
    /// got the node lock first instead of being dropped.
    pub async fn rescan_when_free(&self) -> Result<Vec<InstalledNode>, NodeAppError> {
        let nodes = self
            .deps
            .node_lock
            .acquire()
            .await
            .map_err(|_| NodeAppError::Busy)?;
        self.rescan_held(nodes).await
    }
    async fn rescan_held(
        &self,
        nodes: Box<dyn MaintenanceLease>,
    ) -> Result<Vec<InstalledNode>, NodeAppError> {
        let Some(activity) = self.deps.scans.begin(&*nodes) else {
            self.deps.scans.mark_incomplete();
            return Err(NodeAppError::Cancelled);
        };
        let result = self.observe_and_apply(&*nodes, activity.token()).await;
        activity.finish(result)
    }
    /// The scan proper, under the node lock. Rows are only removed when ComfyUI was verified
    /// stopped before and after the observation, no start was recorded (or in progress) meanwhile
    /// and every unfinished operation is settled.
    async fn observe_and_apply(
        &self,
        nodes: &dyn MaintenanceLease,
        cancel: &CancelToken,
    ) -> Result<Vec<InstalledNode>, NodeAppError> {
        let d = &self.deps.environment;
        let scans = &self.deps.scans;
        let root = d.root.current().root;
        // Odd while a start is in progress: ComfyUI may not be visible yet, so a stopped reading
        // proves nothing and the process state is not even asked.
        let since = scans.starts();
        let stopped_before =
            since.is_multiple_of(2) && d.runtime.ensure_stopped(&root).await.is_ok();
        let mut settled = !self.needs_settling().await?;
        if !settled && stopped_before {
            // Reported to Start before the gate is tried, so Start waits for this brief hold
            // instead of failing. Declared before the lease: it ends after the gate is released.
            let _settling = scans.enter_settling();
            if let Ok(mut lease) = d.gate.try_acquire().await {
                let mut both = BothLeases(&mut *lease, nodes);
                match self.reconcile_with(&mut both, true).await {
                    Ok(()) => settled = true,
                    // Still unsettled: the scan only adds and updates rows.
                    Err(NodeAppError::RuntimeRunning | NodeAppError::RecoveryRequired) => {}
                    Err(error) => return Err(error),
                }
            }
        }
        // The maintenance lease is released before the observation starts.
        let (observed, removed) = self
            .observe_held(nodes, cancel, stopped_before.then_some(since))
            .await?;
        if cancel.is_cancelled() {
            return Err(NodeAppError::Cancelled);
        }
        let prune = settled
            && stopped_before
            && scans.starts() == since
            && d.runtime.ensure_stopped(&root).await.is_ok();
        let removed = if prune { removed } else { Vec::new() };
        self.deps.store.apply_scan(&observed, &removed).await?;
        Ok(observed)
    }
    pub async fn scan_held(
        &self,
        lease: &mut dyn MaintenanceLease,
    ) -> Result<Vec<InstalledNode>, NodeAppError> {
        let (nodes, removed) = self
            .observe_held(&*lease, &CancelToken::new(), Some(self.deps.scans.starts()))
            .await?;
        self.deps.store.apply_scan(&nodes, &removed).await?;
        Ok(nodes)
    }
    pub(super) async fn commit_observation(
        &self,
        lease: &mut dyn MaintenanceLease,
        cancel: &CancelToken,
    ) -> Result<Vec<InstalledNode>, NodeAppError> {
        let (nodes, removed) = self
            .observe_held(&*lease, cancel, Some(self.deps.scans.starts()))
            .await?;
        if cancel.is_cancelled() {
            return Err(NodeAppError::Cancelled);
        }
        self.deps.store.apply_scan(&nodes, &removed).await?;
        Ok(nodes)
    }
    /// `hash_since` is the start count the caller saw while ComfyUI was verified stopped. Files
    /// are only hashed while no start was recorded since; otherwise ComfyUI may be writing, so a
    /// stored digest is reused for every node whose id and source did not change.
    async fn observe_held(
        &self,
        lease: &dyn MaintenanceLease,
        cancel: &CancelToken,
        hash_since: Option<u64>,
    ) -> Result<(Vec<InstalledNode>, Vec<NodeId>), NodeAppError> {
        let root = self.deps.environment.root.current().root;
        let observed = self
            .deps
            .files
            .scan(&root, lease.retained(), cancel)
            .await?;
        let old = self.deps.store.nodes().await?;
        let mut result = Vec::new();
        for mut node in observed {
            if cancel.is_cancelled() {
                return Err(NodeAppError::Cancelled);
            }
            super::requirements::merge_observed(&mut node, &old);
            if !node.conflict {
                let stored = if hash_since == Some(self.deps.scans.starts()) {
                    None
                } else {
                    old.iter().find(|prior| {
                        prior.id == node.id
                            && prior.source == node.source
                            && !prior.tree_digest.is_empty()
                    })
                };
                node.tree_digest = match stored {
                    Some(prior) => prior.tree_digest.clone(),
                    None => {
                        self.deps
                            .files
                            .observe_digest(&root, &node, lease.retained(), cancel)
                            .await?
                    }
                };
            }
            self.project_dependencies(std::slice::from_mut(&mut node))
                .await?;
            result.push(node);
        }
        let removed = old
            .into_iter()
            .filter(|stored| !result.iter().any(|node| node.id == stored.id))
            .map(|stored| stored.id)
            .collect();
        Ok((result, removed))
    }
    pub async fn reconcile(&self) -> Result<(), NodeAppError> {
        let mut lease = self
            .deps
            .environment
            .gate
            .try_acquire()
            .await
            .map_err(|_| NodeAppError::Busy)?;
        self.reconcile_held(&mut *lease).await
    }
    /// Settles interrupted work under the maintenance lease the caller holds. The node lock is
    /// taken here, and only when something has to be settled.
    pub async fn reconcile_held(
        &self,
        lease: &mut dyn MaintenanceLease,
    ) -> Result<(), NodeAppError> {
        self.reconcile_with(lease, false).await
    }
    /// `node_held` says the caller already holds the node lock (a scan or setup), and `lease`
    /// already retains it. Settling moves folders, so it needs ComfyUI verified stopped; when
    /// nothing has to be settled, the process state is not looked at.
    pub(crate) async fn reconcile_with(
        &self,
        lease: &mut dyn MaintenanceLease,
        node_held: bool,
    ) -> Result<(), NodeAppError> {
        self.discard_obsolete_setup_plans().await?;
        if !self.needs_settling().await? {
            return self.ensure_reconciled().await;
        }
        let root = self.deps.environment.root.current().root;
        self.deps
            .environment
            .runtime
            .ensure_stopped(&root)
            .await
            .map_err(|_| NodeAppError::RuntimeRunning)?;
        if node_held {
            self.settle_all(&root, lease).await?;
        } else {
            // Survivors of the settle steps retain both locks.
            let mut held = WithNodeLock {
                lease,
                nodes: self.lock_nodes().await?,
            };
            self.settle_all(&root, &mut held).await?;
        }
        self.ensure_reconciled().await
    }
    async fn settle_all(
        &self,
        root: &std::path::Path,
        lease: &mut dyn MaintenanceLease,
    ) -> Result<(), NodeAppError> {
        for op in self.deps.store.operations().await? {
            if self.should_settle(&op)? {
                self.settle(root, op, lease).await?;
            }
        }
        Ok(())
    }
    /// Whether any stored operation has to be settled.
    async fn needs_settling(&self) -> Result<bool, NodeAppError> {
        for op in self.deps.store.operations().await? {
            if self.should_settle(&op)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
    /// Whether reconciliation has to settle `op`: everything unfinished except a staged plan that
    /// is still reviewable (plans are process-local, so a live one is not discarded by a rescan).
    fn should_settle(&self, op: &NodeOperation) -> Result<bool, NodeAppError> {
        if matches!(op.phase, OperationPhase::Completed | OperationPhase::Failed) {
            return Ok(false);
        }
        if op.phase == OperationPhase::Prepared {
            return Ok(!self
                .deps
                .plans
                .0
                .lock()
                .map_err(|_| NodeAppError::Storage)?
                .contains_key(op.id.as_str()));
        }
        Ok(true)
    }
    async fn interrupted_dependencies(&self, op: &mut NodeOperation) -> Result<(), NodeAppError> {
        let d = &self.deps.environment;
        let mut env = d
            .envs
            .get(&op.environment)
            .await
            .map_err(|_| NodeAppError::Storage)?
            .ok_or(NodeAppError::RecoveryRequired)?;
        if env.state() == EnvState::Verified {
            env.mark_repair_required(&d.clock.now())
                .map_err(|_| NodeAppError::EnvironmentUnavailable)?;
            d.envs.save(&env).await.map_err(|_| NodeAppError::Storage)?;
        }
        op.node.dependencies = DependencyState::Failed;
        self.deps.store.save_node(&op.node).await?;
        self.deps
            .store
            .save_dependency(&DependencyOutcome {
                node: op.node.id.clone(),
                environment: op.environment.clone(),
                source_identity: op.node.dependency_identity(),
                state: DependencyState::Failed,
            })
            .await?;
        op.failure = Some("Interrupted dependency mutation; use environment Repair".into());
        op.phase = OperationPhase::Failed;
        Ok(())
    }
    async fn mark_environment_repair_required(
        &self,
        id: &domain::environment::EnvId,
    ) -> Result<(), NodeAppError> {
        let d = &self.deps.environment;
        if let Some(mut env) = d.envs.get(id).await.map_err(|_| NodeAppError::Storage)?
            && env.state() == EnvState::Verified
        {
            env.mark_repair_required(&d.clock.now())
                .map_err(|_| NodeAppError::EnvironmentUnavailable)?;
            d.envs.save(&env).await.map_err(|_| NodeAppError::Storage)?;
        }
        Ok(())
    }
    /// Reconciles one interrupted operation's files from what is actually on disk, applies the
    /// outcome to the stored records and persists the operation.
    pub(crate) async fn settle(
        &self,
        root: &std::path::Path,
        mut op: NodeOperation,
        lease: &mut dyn MaintenanceLease,
    ) -> Result<NodeOperation, NodeAppError> {
        let before = op.phase;
        let outcome = self
            .deps
            .files
            .recover(root, &op, lease.retained())
            .await
            .unwrap_or(RecoveryOutcome {
                action: RecoveryAction::Blocked,
                tree_digest: None,
            });
        let action = outcome.action;
        if action != RecoveryAction::Blocked {
            op.failure = None;
        }
        if matches!(
            action,
            RecoveryAction::RolledForward | RecoveryAction::CleanupPending
        ) && let Some(digest) = outcome.tree_digest
        {
            op.node.tree_digest = digest;
        }
        if before == OperationPhase::DependenciesInProgress {
            // Even an ambiguous publication may have mutated the shared environment.
            // Only a safely established publication authorizes updating the node record.
            if action == RecoveryAction::RolledForward {
                self.interrupted_dependencies(&mut op).await?;
                self.deps.store.save_operation(&op).await?;
                return Ok(op);
            }
            self.mark_environment_repair_required(&op.environment)
                .await?;
        }
        match action {
            RecoveryAction::Superseded => {
                self.scan_held(lease).await?;
                op.phase = OperationPhase::Failed;
                op.failure = Some("The node folder was changed outside the app and is now at another commit. The update was closed without installing dependencies.".into());
            }
            RecoveryAction::Blocked => {
                op.failure = Some("Node recovery could not establish file identity".into());
            }
            RecoveryAction::Discarded | RecoveryAction::RolledBack => {
                match &op.prior {
                    Some(prior) => self.deps.store.save_node(prior).await?,
                    None if op.kind == OperationKind::Install => {
                        self.deps.store.remove_node(&op.node.id).await?
                    }
                    None => {}
                }
                op.phase = OperationPhase::Failed;
                op.failure = Some("The interrupted operation was rolled back".into());
            }
            action @ (RecoveryAction::RolledForward | RecoveryAction::CleanupPending) => {
                let finished = matches!(
                    before,
                    OperationPhase::Verified | OperationPhase::CleanupPending
                );
                if op.kind == OperationKind::Uninstall {
                    self.deps.store.remove_node(&op.node.id).await?;
                } else {
                    if !finished
                        && matches!(op.kind, OperationKind::Install | OperationKind::Update)
                    {
                        // The files moved but dependencies never ran for this revision.
                        op.node.dependencies = DependencyState::NotRun;
                    }
                    self.deps.store.save_node(&op.node).await?;
                }
                if action == RecoveryAction::CleanupPending {
                    op.phase = OperationPhase::CleanupPending;
                } else if finished
                    || matches!(
                        op.kind,
                        OperationKind::Enable | OperationKind::Disable | OperationKind::Uninstall
                    )
                {
                    op.phase = OperationPhase::Completed;
                } else {
                    op.phase = OperationPhase::Failed;
                    op.failure = Some(
                        "Files were published but dependencies did not run; use Retry dependencies"
                            .into(),
                    );
                }
            }
        }
        self.deps.store.save_operation(&op).await?;
        Ok(op)
    }
    /// Wired into the runtime's launch prerequisites, also when there is no active mutation.
    pub async fn ensure_reconciled(&self) -> Result<(), NodeAppError> {
        if self
            .deps
            .store
            .operations()
            .await?
            .iter()
            .any(NodeOperation::blocks_start)
        {
            Err(NodeAppError::RecoveryRequired)
        } else {
            Ok(())
        }
    }
}

/// Resolves where "open in Explorer" should go for a node or the custom nodes folder. It only
/// reads, so it does not take the maintenance gate.
pub struct LocateNode {
    deps: NodeDeps,
}
impl LocateNode {
    pub fn new(deps: NodeDeps) -> Self {
        Self { deps }
    }
    pub async fn execute(&self, id: Option<&NodeId>) -> Result<ExplorerTarget, NodeAppError> {
        let node = match id {
            None => None,
            Some(id) => Some(
                self.deps
                    .store
                    .nodes()
                    .await?
                    .into_iter()
                    .find(|node| &node.id == id)
                    .ok_or(NodeAppError::NotFound)?,
            ),
        };
        let root = self.deps.environment.root.current().root;
        self.deps.files.explorer_target(&root, node.as_ref()).await
    }
}
