//! Curated custom nodes during first-run setup (FR-SETUP-9). The nodes install through the same
//! prepare/review/commit workflow as every other node, but against the setup candidate: the
//! target is resolved from the persisted run under the lease the run already holds, bound into
//! each plan, and never taken from caller-supplied paths.
use super::{workflow::Bound, *};
use crate::{
    environment::{
        CuratedNodeSpec, ProvisioningRun, RunId, RunStatus, SetupNodeError, SetupNodes,
        StepContext, StepId, StepStatus, same_location,
    },
    shared::MaintenanceLease,
};
use async_trait::async_trait;
use domain::{
    custom_nodes::*,
    environment::{EnvState, SetupMode},
};

/// The repository a prepared curated plan must resolve to: the catalog repository in the form
/// a Git node source records it.
fn expected_repository(entry: &CuratedNodeSpec) -> String {
    normalize_repository(&entry.repository).to_owned()
}

fn normalize_repository(repository: &str) -> &str {
    let repository = repository.trim_end_matches('/');
    repository.strip_suffix(".git").unwrap_or(repository)
}

/// Whether `source` is a resolved head of the catalog repository: any branch, and a full
/// lowercase commit hash.
fn is_curated_source(source: &NodeSource, entry: &CuratedNodeSpec) -> bool {
    matches!(
        source,
        NodeSource::Git { repository, commit, .. }
            if *repository == expected_repository(entry)
                && commit.len() == 40
                && commit.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    )
}

fn prepare_error(error: NodeAppError) -> SetupNodeError {
    match error {
        NodeAppError::Cancelled => SetupNodeError::Cancelled,
        NodeAppError::PinDrift => SetupNodeError::PinDrift(error.to_string()),
        NodeAppError::StalePlan => SetupNodeError::PlanStale,
        error => SetupNodeError::NotApplied(error.to_string()),
    }
}

impl NodeService {
    /// Derive the content exception only from the persisted review-page consent and selection.
    pub(crate) async fn setup_prepare_policy(
        &self,
        run: &RunId,
        request: &PrepareRequest,
    ) -> Result<NodePreparePolicy, NodeAppError> {
        let run = self.setup_run(run).await?;
        if !run.request.third_party_consent {
            return Ok(NodePreparePolicy::Strict);
        }
        let PrepareRequest::Git {
            repository,
            reference: None,
        } = request
        else {
            return Err(NodeAppError::InvalidRequest);
        };
        if !self.deps.environment.curated.list().iter().any(|entry| {
            run.request.curated_nodes.contains(&entry.id)
                && normalize_repository(repository) == expected_repository(entry)
        }) {
            return Err(NodeAppError::InvalidRequest);
        }
        Ok(NodePreparePolicy::ConsentedFirstRun)
    }

    pub(crate) async fn revalidate_setup_policy(
        &self,
        run: &RunId,
        source: &NodeSource,
    ) -> Result<(), NodeAppError> {
        let NodeSource::Git { repository, .. } = source else {
            return Err(NodeAppError::StalePlan);
        };
        let policy = self
            .setup_prepare_policy(
                run,
                &PrepareRequest::Git {
                    repository: repository.clone(),
                    reference: None,
                },
            )
            .await
            .map_err(|error| match error {
                NodeAppError::InvalidRequest => NodeAppError::StalePlan,
                other => other,
            })?;
        if policy != NodePreparePolicy::ConsentedFirstRun {
            return Err(NodeAppError::StalePlan);
        }
        Ok(())
    }

    /// The persisted first run a setup operation belongs to.
    async fn setup_run(&self, run: &RunId) -> Result<ProvisioningRun, NodeAppError> {
        let run = self
            .deps
            .environment
            .runs
            .get(run)
            .await
            .map_err(|_| NodeAppError::Storage)?
            .ok_or(NodeAppError::StalePlan)?;
        if run.request.mode != SetupMode::FirstRun {
            return Err(NodeAppError::StalePlan);
        }
        Ok(run)
    }

    /// Resolves the candidate environment from the persisted run and refuses anything but a
    /// running first-run setup at the curated nodes step, on the configured root, whose candidate
    /// is the run's own provisioning environment with its protected pins in place and with
    /// ComfyUI stopped. The caller already holds the maintenance lease.
    pub(crate) async fn setup_binding(&self, run_id: &RunId) -> Result<Bound, NodeAppError> {
        let run = self.setup_run(run_id).await?;
        if run.status != RunStatus::Running
            || run.step(StepId::CuratedNodes).status != StepStatus::Running
        {
            return Err(NodeAppError::StalePlan);
        }
        let d = &self.deps.environment;
        let root = d.root.current().root;
        if !same_location(&root, &run.request.install_root) {
            return Err(NodeAppError::StalePlan);
        }
        let env = d
            .envs
            .get(&run.target_env)
            .await
            .map_err(|_| NodeAppError::Storage)?
            .ok_or(NodeAppError::StalePlan)?;
        if env.id != run.target_env
            || !same_location(&env.path, &run.python_target.env_path)
            || env.profile_id != run.profile.id
            || env.profile_revision != run.profile.revision
        {
            return Err(NodeAppError::StalePlan);
        }
        if env.state() != EnvState::Provisioning
            || run
                .profile
                .protected_distributions()
                .iter()
                .any(|name| !env.pins.contains(name))
        {
            return Err(NodeAppError::EnvironmentUnavailable);
        }
        // The candidate is never the environment a node operation could otherwise select.
        if d.envs
            .active()
            .await
            .map_err(|_| NodeAppError::Storage)?
            .is_some_and(|active| active.id == env.id)
        {
            return Err(NodeAppError::StalePlan);
        }
        d.runtime
            .ensure_stopped(&root)
            .await
            .map_err(|_| NodeAppError::RuntimeRunning)?;
        Ok(Bound {
            root,
            env,
            profile: Some(run.profile),
        })
    }

    /// Prepares a Git install for the candidate environment of `run`. Interrupted node work of an
    /// earlier attempt is settled first, because a retry must not be blocked by it.
    pub async fn prepare_for_setup_held(
        &self,
        run: &RunId,
        request: PrepareRequest,
        ctx: &NodeContext,
        lease: &mut dyn MaintenanceLease,
    ) -> Result<PlanReview, NodeAppError> {
        if !matches!(request, PrepareRequest::Git { .. }) {
            return Err(NodeAppError::InvalidRequest);
        }
        self.discard_obsolete_setup_plans().await?;
        if self.ensure_reconciled().await.is_err() {
            self.reconcile_held(lease).await?;
        }
        self.prepare_target(NodeTarget::Setup(run.clone()), request, ctx, lease)
            .await
    }

    /// Commits a setup plan. Hooks require persisted first-run consent; setup never builds
    /// from source or remembers trust. Warnings still need the user's exact acknowledgement.
    pub async fn commit_for_setup_held(
        &self,
        run: &RunId,
        id: &str,
        mut decision: CommitDecision,
        ctx: &NodeContext,
        lease: &mut dyn MaintenanceLease,
    ) -> Result<NodeOutcome, NodeAppError> {
        if !self.setup_run(run).await?.request.third_party_consent {
            decision.consent.run_install_py = false;
        }
        decision.consent.remember_install_py = false;
        decision.consent.remember_source = false;
        decision.consent.allow_source_builds = false;
        self.commit_target(NodeTarget::Setup(run.clone()), id, decision, ctx, lease)
            .await
    }

    /// The reviewed plans of `run` that exist in this process. A restart empties it.
    pub fn setup_reviews(&self, run: &RunId) -> Result<Vec<PlanReview>, NodeAppError> {
        let target = NodeTarget::Setup(run.clone());
        let plans = self
            .deps
            .plans
            .0
            .lock()
            .map_err(|_| NodeAppError::Storage)?;
        let mut reviews: Vec<_> = plans
            .values()
            .filter(|plan| plan.target == target)
            .map(|plan| plan.review.clone())
            .collect();
        reviews.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(reviews)
    }

    /// Drops the reviewed plans of runs that can no longer commit them (cancelled, finished or
    /// past the curated step) together with their staged clones.
    pub(crate) async fn discard_obsolete_setup_plans(&self) -> Result<(), NodeAppError> {
        let setup: Vec<(String, RunId)> = self
            .deps
            .plans
            .0
            .lock()
            .map_err(|_| NodeAppError::Storage)?
            .iter()
            .filter_map(|(id, plan)| match &plan.target {
                NodeTarget::Setup(run) => Some((id.clone(), run.clone())),
                NodeTarget::Active => None,
            })
            .collect();
        for (id, run_id) in setup {
            let reviewable = self
                .deps
                .environment
                .runs
                .get(&run_id)
                .await
                .map_err(|_| NodeAppError::Storage)?
                .is_some_and(|run| {
                    matches!(
                        run.status,
                        RunStatus::Registered
                            | RunStatus::Running
                            | RunStatus::Failed
                            | RunStatus::Interrupted
                    ) && !matches!(
                        run.step(StepId::CuratedNodes).status,
                        StepStatus::Skipped | StepStatus::Succeeded | StepStatus::Warning
                    )
                });
            if !reviewable {
                let _ = self.discard_plan(&id).await;
            }
        }
        Ok(())
    }
}

#[async_trait]
impl SetupNodes for NodeService {
    async fn prepare(
        &self,
        run: &RunId,
        entry: &CuratedNodeSpec,
        ctx: &StepContext,
        lease: &mut dyn MaintenanceLease,
    ) -> Result<PlanReview, SetupNodeError> {
        let request = PrepareRequest::Git {
            repository: entry.repository.clone(),
            reference: None,
        };
        let node_ctx = NodeContext {
            cancel: ctx.cancel.clone(),
            sink: ctx.sink.clone(),
        };
        let review = self
            .prepare_for_setup_held(run, request, &node_ctx, lease)
            .await
            .map_err(prepare_error)?;
        if !is_curated_source(&review.source, entry) {
            // The plan can never be committed: drop it and its staging.
            let _ = self.discard_plan(&review.id).await;
            return Err(SetupNodeError::NotApplied(
                "The repository did not resolve to the curated repository's default branch.".into(),
            ));
        }
        Ok(review)
    }

    async fn commit(
        &self,
        run: &RunId,
        plan: &str,
        decision: CommitDecision,
        ctx: &StepContext,
        lease: &mut dyn MaintenanceLease,
    ) -> Result<NodeOutcome, SetupNodeError> {
        let node_ctx = NodeContext {
            cancel: ctx.cancel.clone(),
            sink: ctx.sink.clone(),
        };
        let error = match self
            .commit_for_setup_held(run, plan, decision, &node_ctx, lease)
            .await
        {
            Ok(outcome) => return Ok(outcome),
            Err(error) => error,
        };
        let published = self.publication_started(plan).await;
        Err(match error {
            NodeAppError::Cancelled => SetupNodeError::Cancelled,
            NodeAppError::PinDrift => SetupNodeError::PinDrift(error.to_string()),
            error if published => SetupNodeError::Damaged(error.to_string()),
            NodeAppError::ConsentRequired => SetupNodeError::ConsentRequired,
            NodeAppError::StalePlan => {
                // A plan that no longer matches can never commit: prepare it again.
                let _ = self.discard_plan(plan).await;
                SetupNodeError::PlanStale
            }
            error => SetupNodeError::NotApplied(error.to_string()),
        })
    }

    fn pending(&self, run: &RunId) -> Result<Vec<PlanReview>, SetupNodeError> {
        self.setup_reviews(run)
            .map_err(|error| SetupNodeError::NotApplied(error.to_string()))
    }

    async fn publication_started(&self, plan: &str) -> bool {
        match self.operation_status(plan).await {
            Ok(op) => match op.phase {
                OperationPhase::Prepared => false,
                OperationPhase::Failed => {
                    op.node.dependencies == DependencyState::Failed
                        || self
                            .deps
                            .store
                            .nodes()
                            .await
                            .map(|nodes| nodes.iter().any(|node| node.id == op.node.id))
                            .unwrap_or(true)
                }
                _ => true,
            },
            Err(NodeAppError::NotFound) => false,
            Err(_) => true,
        }
    }

    async fn installed(
        &self,
        run: &RunId,
        entry: &CuratedNodeSpec,
        lease: &mut dyn MaintenanceLease,
    ) -> Result<bool, SetupNodeError> {
        let run = self.setup_run(run).await.map_err(prepare_error)?;
        let d = &self.deps.environment;
        let root = d.root.current().root;
        if !same_location(&root, &run.request.install_root) {
            return Ok(false);
        }
        let repository = expected_repository(entry);
        let stored = self.deps.store.nodes().await.map_err(prepare_error)?;
        let Some(node) = stored.into_iter().find(|node| {
            !node.conflict
                && !node.single_file
                && matches!(
                    &node.source,
                    NodeSource::Git { repository: recorded, .. } if *recorded == repository
                )
        }) else {
            return Ok(false);
        };
        // The record only counts while the files on disk still are the recorded tree.
        let Ok(snapshot) = self
            .deps
            .files
            .snapshot(&root, &node, None, lease.retained())
            .await
        else {
            return Ok(false);
        };
        if snapshot.digest != node.tree_digest {
            return Ok(false);
        }
        let outcome = self
            .deps
            .store
            .dependency(&node.id, &run.target_env, &node.dependency_identity())
            .await
            .map_err(prepare_error)?;
        Ok(outcome == Some(DependencyState::Ok))
    }
}
