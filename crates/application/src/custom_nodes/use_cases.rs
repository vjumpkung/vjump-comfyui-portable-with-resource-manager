use super::*;
use crate::shared::MaintenanceLease;
use domain::{custom_nodes::*, environment::EnvState};

#[derive(Clone)]
pub struct NodeService {
    pub(crate) deps: NodeDeps,
}

impl NodeService {
    pub fn new(deps: NodeDeps) -> Self {
        Self { deps }
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
        self.deps.store.nodes().await
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
        let mut lease = self
            .deps
            .environment
            .gate
            .try_acquire()
            .await
            .map_err(|_| NodeAppError::Busy)?;
        self.prepare_held(request, ctx, &mut *lease).await
    }
    pub async fn commit(
        &self,
        id: &str,
        decision: CommitDecision,
        ctx: &NodeContext,
    ) -> Result<NodeOutcome, NodeAppError> {
        let mut lease = self
            .deps
            .environment
            .gate
            .try_acquire()
            .await
            .map_err(|_| NodeAppError::Busy)?;
        self.commit_held(id, decision, ctx, &mut *lease).await
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
        let mut lease = self
            .deps
            .environment
            .gate
            .try_acquire()
            .await
            .map_err(|_| NodeAppError::Busy)?;
        let review = self.prepare_held(request, ctx, &mut *lease).await?;
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
        self.commit_held(&review.id, decision, ctx, &mut *lease).await
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
        let mut lease = self
            .deps
            .environment
            .gate
            .try_acquire()
            .await
            .map_err(|_| NodeAppError::Busy)?;
        {
            let plans = self
                .deps
                .plans
                .0
                .lock()
                .map_err(|_| NodeAppError::Storage)?;
            for (id, _) in &decisions {
                match plans.get(id) {
                    Some(plan) if plan.operation.kind == OperationKind::Update => {}
                    _ => return Err(NodeAppError::StalePlan),
                }
            }
        }
        let mut batch = BatchOutcome {
            results: Vec::new(),
            stopped_for_repair: false,
        };
        let mut remaining: Vec<String> = decisions.iter().map(|(id, _)| id.clone()).collect();
        for (id, decision) in decisions {
            remaining.retain(|r| r != &id);
            let outcome = self.commit_held(&id, decision, ctx, &mut *lease).await;
            let damaged = self
                .deps
                .environment
                .envs
                .active()
                .await
                .map_err(|_| NodeAppError::Storage)?
                .is_none_or(|env| env.state() != EnvState::Verified);
            batch.results.push((id, outcome));
            if damaged || ctx.cancel.is_cancelled() {
                batch.stopped_for_repair = damaged;
                break;
            }
        }
        // Targets that did not run keep no staging: the batch stopped before they were committed.
        for id in remaining {
            batch
                .results
                .push((id.clone(), Err(NodeAppError::Cancelled)));
            let _ = self.discard_held(&id, &mut *lease).await;
        }
        Ok(batch)
    }
    /// Desktop calls this after a verified stop. Required reconciliation is awaited before scanning.
    pub async fn rescan(&self) -> Result<Vec<InstalledNode>, NodeAppError> {
        let mut lease = self
            .deps
            .environment
            .gate
            .try_acquire()
            .await
            .map_err(|_| NodeAppError::Busy)?;
        self.reconcile_held(&mut *lease).await?;
        self.scan_held(&mut *lease).await
    }
    pub async fn scan_held(
        &self,
        _lease: &mut dyn MaintenanceLease,
    ) -> Result<Vec<InstalledNode>, NodeAppError> {
        let root = self.deps.environment.root.current().root;
        let observed = self.deps.files.scan(&root).await?;
        let old = self.deps.store.nodes().await?;
        let mut result = Vec::new();
        for mut node in observed {
            if let Some(stored) = old
                .iter()
                .find(|n| n.name == node.name && n.single_file == node.single_file)
            {
                node.id = stored.id.clone();
                node.origin = stored.origin;
                if stored.source.same_origin(&node.source) {
                    // Manager or a manual pull may have moved the revision; record what is on disk.
                    let source = stored.source.with_observed_revision(&node.source);
                    node.dependencies = if source == stored.source {
                        stored.dependencies
                    } else {
                        DependencyState::NotRun
                    };
                    node.source = source;
                } else {
                    // Observation never authorizes replacement of authoritative recorded provenance.
                    node.conflict = true;
                    node.source = stored.source.clone();
                    node.dependencies = stored.dependencies;
                }
            }
            self.deps.store.save_node(&node).await?;
            result.push(node);
        }
        for stored in old {
            if !result.iter().any(|n| n.id == stored.id) {
                self.deps.store.remove_node(&stored.id).await?;
            }
        }
        Ok(result)
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
    pub async fn reconcile_held(&self, _lease: &mut dyn MaintenanceLease) -> Result<(), NodeAppError> {
        let d = &self.deps.environment;
        let root = d.root.current().root;
        d.runtime
            .ensure_stopped(&root)
            .await
            .map_err(|_| NodeAppError::RuntimeRunning)?;
        for mut op in self.deps.store.operations().await? {
            if matches!(op.phase, OperationPhase::Completed | OperationPhase::Failed) {
                continue;
            }
            // Plans are process-local; do not discard a still-reviewable staged plan during rescan.
            if op.phase == OperationPhase::Prepared
                && self
                    .deps
                    .plans
                    .0
                    .lock()
                    .map_err(|_| NodeAppError::Storage)?
                    .contains_key(op.id.as_str())
            {
                continue;
            }
            if op.phase == OperationPhase::DependenciesInProgress {
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
                        source_identity: op.node.source.identity(),
                        state: DependencyState::Failed,
                    })
                    .await?;
                op.failure = Some("Interrupted dependency mutation; use environment Repair".into());
                op.phase = OperationPhase::Failed;
            } else {
                self.settle(&root, op).await?;
                continue;
            }
            self.deps.store.save_operation(&op).await?;
        }
        self.ensure_reconciled().await
    }
    /// Reconciles one interrupted operation's files from what is actually on disk, applies the
    /// outcome to the stored records and persists the operation.
    pub(crate) async fn settle(
        &self,
        root: &std::path::Path,
        mut op: NodeOperation,
    ) -> Result<NodeOperation, NodeAppError> {
        let before = op.phase;
        match self.deps.files.recover(root, &op).await? {
            RecoveryAction::Blocked => {}
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
            .any(|op| op.phase.blocks_start())
        {
            Err(NodeAppError::RecoveryRequired)
        } else {
            Ok(())
        }
    }
    /// M1 Repair consumes every root requirement file, including disabled nodes. Hooks need new consent.
    pub async fn repair_requirements(&self) -> Result<Vec<std::path::PathBuf>, NodeAppError> {
        let root = self.deps.environment.root.current().root;
        let mut result = Vec::new();
        for node in self.deps.store.nodes().await? {
            if node.single_file {
                continue;
            }
            let snapshot = self.deps.files.snapshot(&root, &node, None).await?;
            if snapshot.requirements_digest.is_some() {
                result.push(
                    self.deps
                        .files
                        .path(&root, &node, None)
                        .join("requirements.txt"),
                );
            }
        }
        Ok(result)
    }
}
