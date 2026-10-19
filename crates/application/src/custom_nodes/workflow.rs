use super::model::PreparedPlan;
use super::*;
use crate::{
    environment::*,
    shared::{CancelToken, MaintenanceLease},
};
const MAX_PLANS: usize = 128;
use domain::{
    custom_nodes::*,
    environment::{DistName, EnvState, PinSet, PythonEnvironment},
    shared::model_values::SafeFileName,
};

/// The requirement file of a guarded location, read from its protected snapshot.
fn guarded_files(guard: Option<&dyn NodeLocationGuard>) -> InstallSpec {
    let Some((guard, text)) = guard.and_then(|guard| Some((guard, guard.requirements()?))) else {
        return InstallSpec::default();
    };
    let path = guard.directory().join("requirements.txt");
    InstallSpec {
        requirement_files: vec![path.clone()],
        guarded: Some(GuardedRequirements {
            path,
            directory: guard.directory().to_path_buf(),
            text,
            retained: guard.retained(),
        }),
        ..Default::default()
    }
}

impl NodeService {
    fn step(&self, id: &NodeId, ctx: &NodeContext, retained: Retained) -> StepContext {
        StepContext {
            run_id: RunId::new(id.as_str()).expect("validated node operation ID"),
            step: StepId::ComfyRequirements,
            sink: ctx.sink.clone(),
            cancel: ctx.cancel.clone(),
            allow_source_builds: false,
            retained,
        }
    }
    async fn selected(&self, repair: bool) -> Result<PythonEnvironment, NodeAppError> {
        let env = self
            .deps
            .environment
            .envs
            .active()
            .await
            .map_err(|_| NodeAppError::Storage)?
            .ok_or(NodeAppError::EnvironmentUnavailable)?;
        if env.state() != EnvState::Verified && !(repair && env.state() == EnvState::RepairRequired)
        {
            return Err(NodeAppError::EnvironmentUnavailable);
        }
        Ok(env)
    }
    pub async fn prepare_held(
        &self,
        request: PrepareRequest,
        ctx: &NodeContext,
        lease: &mut dyn MaintenanceLease,
    ) -> Result<PlanReview, NodeAppError> {
        self.ensure_reconciled().await?;
        if self
            .deps
            .plans
            .0
            .lock()
            .map_err(|_| NodeAppError::Storage)?
            .len()
            >= MAX_PLANS
        {
            return Err(NodeAppError::Busy);
        }
        let d = &self.deps.environment;
        let root = d.root.current().root;
        d.runtime
            .ensure_stopped(&root)
            .await
            .map_err(|_| NodeAppError::RuntimeRunning)?;
        let repair = matches!(
            request,
            PrepareRequest::RetryDependencies { .. }
                | PrepareRequest::SetEnabled { .. }
                | PrepareRequest::Uninstall { .. }
        );
        let env = self.selected(repair).await?;
        let id =
            NodeId::new(format!("node-{}", d.ids.new_id())).map_err(|_| NodeAppError::Storage)?;
        let target = match &request {
            PrepareRequest::Update { node, .. }
            | PrepareRequest::RetryDependencies { node }
            | PrepareRequest::SetEnabled { node, .. }
            | PrepareRequest::Uninstall { node } => Some(node),
            _ => None,
        };
        let mut prior = if let Some(target) = target {
            Some(
                self.deps
                    .store
                    .nodes()
                    .await?
                    .into_iter()
                    .find(|n| &n.id == target)
                    .ok_or(NodeAppError::NotFound)?,
            )
        } else {
            None
        };
        if let Some(node) = &mut prior {
            // A node whose disk provenance disagrees with the stored record can still be disabled
            // or removed, but nothing may update it or run its dependency steps.
            if node.conflict
                && matches!(
                    request,
                    PrepareRequest::Update { .. } | PrepareRequest::RetryDependencies { .. }
                )
            {
                return Err(NodeAppError::UnsafeSource);
            }
            node.tree_digest = self
                .deps
                .files
                .snapshot(&root, node, None, lease.retained())
                .await?
                .digest;
        }
        let kind = match &request {
            PrepareRequest::Update { .. } => OperationKind::Update,
            PrepareRequest::RetryDependencies { .. } => OperationKind::RetryDependencies,
            PrepareRequest::SetEnabled { enabled: true, .. } => OperationKind::Enable,
            PrepareRequest::SetEnabled { enabled: false, .. } => OperationKind::Disable,
            PrepareRequest::Uninstall { .. } => OperationKind::Uninstall,
            _ => OperationKind::Install,
        };
        let node = prior.clone().unwrap_or(InstalledNode {
            id: id.clone(),
            name: SafeFileName::new(format!("pending-{}", id.as_str()))
                .map_err(|_| NodeAppError::Storage)?,
            source: NodeSource::Manual,
            origin: ProvenanceOrigin::App,
            enabled: true,
            single_file: false,
            tree_digest: String::new(),
            dependencies: DependencyState::NotRun,
            conflict: false,
            dirty: None,
        });
        let mut op = NodeOperation {
            id: id.clone(),
            kind,
            phase: OperationPhase::Prepared,
            node,
            prior,
            environment: env.id.clone(),
            pins: env.pins.clone(),
            binding: d.ids.new_id(),
            artifact_digest: None,
            requirements_digest: None,
            hook_digest: None,
            consent_identity: None,
            failure: None,
            staging: format!(".disabled/.vjump-staging-{}", id.as_str()),
            backup: format!(".disabled/.vjump-backup-{}", id.as_str()),
            dependency_requirements: Vec::new(),
        };
        self.deps.store.save_operation(&op).await?;
        let staged = matches!(kind, OperationKind::Install | OperationKind::Update);
        let mut warnings = Vec::new();
        let requirement_lines: Vec<String>;
        if staged {
            let prepared = match self
                .deps
                .files
                .prepare(
                    &root,
                    &id,
                    &request,
                    op.prior.as_ref(),
                    &self.step(&id, ctx, lease.retained()),
                )
                .await
            {
                Ok(value) => value,
                Err(error) => {
                    // The adapter cleans only entries acquired by its failed prepare attempt.
                    op.phase = OperationPhase::Failed;
                    op.failure = Some(error.to_string());
                    self.deps.store.save_operation(&op).await?;
                    return Err(error);
                }
            };
            op.node.name = prepared.name;
            op.node.source = prepared.source;
            op.node.tree_digest = prepared.tree_digest;
            op.node.dependencies = DependencyState::NotRun;
            op.artifact_digest = prepared.artifact_digest;
            op.requirements_digest = prepared.requirements_digest;
            op.hook_digest = prepared.hook_digest;
            op.dependency_requirements = prepared.dependencies;
            warnings = prepared.warnings;
            requirement_lines = prepared.requirement_lines;
        } else {
            let snapshot = self
                .deps
                .files
                .snapshot(&root, &op.node, None, lease.retained())
                .await?;
            op.requirements_digest = snapshot.requirements_digest;
            op.hook_digest = snapshot.hook_digest;
            requirement_lines = snapshot.requirement_lines;
            // Retain registry-only requirements for retry even when no requirements.txt exists.
            if kind == OperationKind::RetryDependencies
                && let NodeSource::Registry { id, version, .. } = &op.node.source
            {
                op.dependency_requirements = self
                    .deps
                    .registry
                    .install(id, Some(version))
                    .await?
                    .dependencies;
            }
            if kind == OperationKind::Enable {
                op.node.enabled = true;
            } else if kind == OperationKind::Disable {
                op.node.enabled = false;
            }
        }
        let needs_deps = matches!(
            kind,
            OperationKind::Install | OperationKind::Update | OperationKind::RetryDependencies
        );
        let package = if needs_deps {
            match self
                .preview_packages(
                    &root,
                    &env,
                    &op,
                    staged,
                    &self.step(&id, ctx, lease.retained()),
                )
                .await
            {
                Ok(package) => package,
                Err(error) => {
                    self.abandon(&root, &mut op, &error).await;
                    return Err(error);
                }
            }
        } else {
            None
        };
        let trust = match op.node.source.trust_namespace() {
            Some(namespace) => match self.deps.store.trust(&namespace).await {
                Ok(trust) => trust,
                Err(error) => {
                    self.abandon(&root, &mut op, &error).await;
                    return Err(error);
                }
            },
            None => None,
        };
        if op.hook_digest.is_some() && needs_deps {
            warnings.push("Root install.py executes third-party code only if explicitly selected; a venv is not a sandbox".into());
        }
        let mut requirements = op.dependency_requirements.clone();
        requirements.extend(requirement_lines);
        let review = PlanReview {
            id: id.as_str().into(),
            binding: op.binding.clone(),
            kind,
            source: op.node.source.clone(),
            name: op.node.name.as_str().into(),
            prior_source: op.prior.as_ref().map(|p| p.source.clone()),
            requirements,
            artifact_digest: op.artifact_digest.clone(),
            tree_digest: op.node.tree_digest.clone(),
            requirements_digest: op.requirements_digest.clone(),
            hook_digest: op.hook_digest.clone(),
            environment: env.id,
            pins: env.pins,
            protected_changes: package
                .as_ref()
                .map(|p| p.protected_changes.clone())
                .unwrap_or_default(),
            warnings,
            trusted_source: trust.is_some(),
            trusted_install_py: trust.is_some_and(|t| t.allow_install_py),
            has_requirements: op.requirements_digest.is_some()
                || !op.dependency_requirements.is_empty(),
            has_install_py: op.hook_digest.is_some() && needs_deps,
        };
        if let Err(error) = self.deps.store.save_operation(&op).await {
            self.abandon(&root, &mut op, &error).await;
            return Err(error);
        }
        let mut plans = self
            .deps
            .plans
            .0
            .lock()
            .map_err(|_| NodeAppError::Storage)?;
        plans.insert(
            review.id.clone(),
            PreparedPlan {
                review: review.clone(),
                operation: op,
                root,
                package,
            },
        );
        Ok(review)
    }
    /// Guards the node location and refuses it unless its tree and inputs are the reviewed ones.
    async fn pin(
        &self,
        root: &std::path::Path,
        op: &NodeOperation,
        staged: Option<&NodeId>,
        hook: bool,
    ) -> Result<Box<dyn NodeLocationGuard>, NodeAppError> {
        let guard = self.deps.files.pin_location(root, &op.node, staged).await?;
        let snapshot = guard.snapshot();
        if snapshot.digest != op.node.tree_digest
            || snapshot.requirements_digest != op.requirements_digest
            || (hook && snapshot.hook_digest != op.hook_digest)
        {
            return Err(NodeAppError::StalePlan);
        }
        Ok(guard)
    }
    /// Removes owned staging of a plan that could not be completed and records the failure.
    async fn abandon(&self, root: &std::path::Path, op: &mut NodeOperation, error: &NodeAppError) {
        let _ = self.deps.files.discard_staging(root, &op.id).await;
        op.phase = OperationPhase::Failed;
        op.failure = Some(error.to_string());
        let _ = self.deps.store.save_operation(op).await;
    }
    async fn preview_packages(
        &self,
        root: &std::path::Path,
        env: &PythonEnvironment,
        op: &NodeOperation,
        staged: bool,
        step: &StepContext,
    ) -> Result<Option<PackagePlan>, NodeAppError> {
        if op.requirements_digest.is_none() && op.dependency_requirements.is_empty() {
            return Ok(None);
        }
        let d = &self.deps.environment;
        let extra = d
            .catalog
            .get(&env.profile_id)
            .ok_or(NodeAppError::EnvironmentUnavailable)?
            .constraints;
        let constraint = d
            .packages
            .write_constraints(root, &env.id, &env.pins, &extra)
            .await
            .map_err(|_| NodeAppError::ResolutionFailed)?;
        // The guard spans both previews and ends when this function returns.
        let guard = if op.requirements_digest.is_some() {
            Some(self.pin(root, op, staged.then_some(&op.id), false).await?)
        } else {
            None
        };
        let base = InstallSpec {
            requirements: op.dependency_requirements.clone(),
            constraint_file: Some(constraint),
            ..guarded_files(guard.as_deref())
        };
        // Node dependencies resolve under the protected-package constraints or not at all: there
        // is no unconstrained retry, because a node operation must never move a protected package.
        let preview = d
            .packages
            .preview_install(root, &env.path, &base, step)
            .await
            .map_err(|_| NodeAppError::ResolutionFailed)?;
        let mut after = env.pins.clone();
        for p in &preview.removals {
            let name = DistName::new(&p.name).map_err(|_| NodeAppError::ResolutionFailed)?;
            after = after.without(&[name]);
        }
        for p in &preview.installs {
            let name = DistName::new(&p.name).map_err(|_| NodeAppError::ResolutionFailed)?;
            if env.pins.contains(&name) {
                after.insert(name, p.version.clone());
            }
        }
        // A preview that would change any protected package is not offered as a node plan; the
        // separate package-management workflow owns explicit protected-package confirmation.
        if !env.pins.diff(&after).is_empty() {
            return Err(NodeAppError::ResolutionFailed);
        }
        Ok(Some(PackagePlan {
            id: PlanId::new(op.id.as_str()).map_err(|_| NodeAppError::Storage)?,
            env_id: env.id.clone(),
            operation: PackageOperation::Install(op.dependency_requirements.clone()),
            current_pins: env.pins.clone(),
            resolution: PlanResolution::Constrained,
            installs: preview.installs,
            removals: preview.removals,
            protected_changes: Vec::new(),
        }))
    }
    pub async fn commit_held(
        &self,
        id: &str,
        decision: CommitDecision,
        ctx: &NodeContext,
        lease: &mut dyn MaintenanceLease,
    ) -> Result<NodeOutcome, NodeAppError> {
        let mut plan = self
            .deps
            .plans
            .0
            .lock()
            .map_err(|_| NodeAppError::Storage)?
            .get(id)
            .cloned()
            .ok_or(NodeAppError::StalePlan)?;
        let d = &self.deps.environment;
        let root = d.root.current().root;
        d.runtime
            .ensure_stopped(&root)
            .await
            .map_err(|_| NodeAppError::RuntimeRunning)?;
        self.ensure_reconciled().await?;
        let env = self
            .selected(matches!(
                plan.operation.kind,
                OperationKind::RetryDependencies
                    | OperationKind::Enable
                    | OperationKind::Disable
                    | OperationKind::Uninstall
            ))
            .await?;
        if root != plan.root || env.id != plan.review.environment || env.pins != plan.review.pins {
            return Err(NodeAppError::StalePlan);
        }
        if ctx.cancel.is_cancelled() {
            return Err(NodeAppError::Cancelled);
        }
        decision
            .consent
            .check(
                id,
                &plan.review.binding,
                !plan.review.warnings.is_empty(),
                plan.review.has_install_py,
            )
            .map_err(|_| NodeAppError::ConsentRequired)?;
        if decision.consent.allow_source_builds {
            return Err(NodeAppError::ConsentRequired);
        } // Wheel-only resolver previews must never authorize a later build.
        // Node operations never authorize protected-package changes.
        if !plan.review.protected_changes.is_empty() {
            return Err(NodeAppError::ResolutionFailed);
        }
        if !decision.protected_changes.is_empty() {
            return Err(NodeAppError::ConsentRequired);
        }
        let step = self.step(&plan.operation.id, ctx, lease.retained());
        self.assert_pins(&root, &env, &env.pins, &step).await?;
        let staged = matches!(
            plan.operation.kind,
            OperationKind::Install | OperationKind::Update
        );
        let snapshot = self
            .deps
            .files
            .snapshot(
                &root,
                &plan.operation.node,
                staged.then_some(&plan.operation.id),
                step.retained.clone(),
            )
            .await?;
        if snapshot.digest != plan.review.tree_digest
            || snapshot.requirements_digest != plan.review.requirements_digest
            || snapshot.hook_digest != plan.review.hook_digest
        {
            return Err(NodeAppError::StalePlan);
        }
        if let Some(prior) = &plan.operation.prior {
            let current = self
                .deps
                .store
                .nodes()
                .await?
                .into_iter()
                .find(|n| n.id == prior.id)
                .ok_or(NodeAppError::StalePlan)?;
            if current.source != prior.source
                || current.enabled != prior.enabled
                || self
                    .deps
                    .files
                    .snapshot(&root, prior, None, lease.retained())
                    .await?
                    .digest
                    != prior.tree_digest
            {
                return Err(NodeAppError::StalePlan);
            }
        }
        self.deps.take_plan(id)?;
        plan.operation.consent_identity = Some(format!(
            "{}:{}:hook={}:builds=false:warnings={}:pins={}",
            id,
            decision.consent.binding,
            decision.consent.run_install_py,
            decision.consent.acknowledge_warnings,
            decision.protected_changes.len()
        ));
        self.phase(&mut plan.operation, OperationPhase::PublishIntent)
            .await?;
        let mut op = plan.operation;
        if let Err(error) = self.mutate_files(&root, &mut op, lease.retained()).await {
            // Nothing may stay half-moved: settle the files from what is actually on disk.
            let _ = self.settle(&root, op, lease.retained()).await;
            return Err(error);
        }
        self.phase(&mut op, OperationPhase::Published).await?;
        if matches!(
            op.kind,
            OperationKind::Install | OperationKind::Update | OperationKind::RetryDependencies
        ) {
            self.deps.store.save_node(&op.node).await?;
            if let Err(error) = self
                .dependencies(&root, &mut op, env, plan.package.as_ref(), &decision, &step)
                .await
            {
                op.node.dependencies = DependencyState::Failed;
                op.failure = Some(error.to_string());
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
                // The environment stays RepairRequired and any recovery backup is retained.
                op.phase = OperationPhase::Failed;
                self.deps.store.save_operation(&op).await?;
                return Err(error);
            }
            op.node.dependencies = DependencyState::Ok;
            self.deps
                .store
                .save_dependency(&DependencyOutcome {
                    node: op.node.id.clone(),
                    environment: op.environment.clone(),
                    source_identity: op.node.source.identity(),
                    state: DependencyState::Ok,
                })
                .await?;
        }
        self.phase(&mut op, OperationPhase::Verified).await?;
        if op.kind == OperationKind::Uninstall {
            self.deps.store.remove_node(&op.node.id).await?;
        } else {
            self.deps.store.save_node(&op.node).await?;
        }
        if decision.consent.remember_source
            && let Some(namespace) = op.node.source.trust_namespace()
        {
            self.deps
                .store
                .save_trust(&Trust {
                    namespace,
                    allow_install_py: decision.consent.remember_install_py,
                })
                .await?;
        }
        self.phase(&mut op, OperationPhase::CleanupPending).await?;
        let cleanup_pending = self.deps.files.recycle(&root, &op).await.is_err();
        if !cleanup_pending {
            self.deps.files.discard_staging(&root, &op.id).await?;
            self.phase(&mut op, OperationPhase::Completed).await?;
        }
        Ok(NodeOutcome {
            operation: op.id,
            node: op.node,
            cleanup_pending,
        })
    }
    async fn mutate_files(
        &self,
        root: &std::path::Path,
        op: &mut NodeOperation,
        retained: Retained,
    ) -> Result<(), NodeAppError> {
        match op.kind {
            OperationKind::Install | OperationKind::Update => {
                if op.prior.is_some() {
                    self.deps.files.backup(root, op).await?;
                    self.phase(op, OperationPhase::OldBackedUp).await?;
                }
                self.deps.files.publish(root, op, retained).await?;
            }
            OperationKind::Enable | OperationKind::Disable => {
                let prior = op.prior.clone().ok_or(NodeAppError::StalePlan)?;
                self.deps
                    .files
                    .move_enabled(root, &prior, op.node.enabled)
                    .await?
            }
            OperationKind::Uninstall => {
                self.deps.files.backup(root, op).await?;
                self.phase(op, OperationPhase::OldBackedUp).await?;
            }
            OperationKind::RetryDependencies => {}
        }
        Ok(())
    }
    async fn phase(
        &self,
        op: &mut NodeOperation,
        phase: OperationPhase,
    ) -> Result<(), NodeAppError> {
        op.phase = phase;
        self.deps.store.save_operation(op).await
    }
    async fn assert_pins(
        &self,
        root: &std::path::Path,
        env: &PythonEnvironment,
        expected: &PinSet,
        ctx: &StepContext,
    ) -> Result<(), NodeAppError> {
        let installed = self
            .deps
            .environment
            .packages
            .list(root, &env.path, Some(ctx))
            .await
            .map_err(|_| NodeAppError::PinDrift)?;
        for (name, version) in expected.iter() {
            if !installed
                .iter()
                .any(|p| DistName::new(&p.name).ok().as_ref() == Some(name) && p.version == version)
            {
                return Err(NodeAppError::PinDrift);
            }
        }
        Ok(())
    }
    /// Reassert pins even after a failed/cancelled hook, using a fresh cleanup token under the same lease.
    async fn reassert(
        &self,
        root: &std::path::Path,
        env: &PythonEnvironment,
        pins: &PinSet,
        ctx: &StepContext,
    ) -> Result<(), NodeAppError> {
        let d = &self.deps.environment;
        if self.assert_pins(root, env, pins, ctx).await.is_err() {
            let constraints = d
                .packages
                .write_constraints(root, &env.id, pins, &[])
                .await
                .map_err(|_| NodeAppError::PinDrift)?;
            let spec = InstallSpec {
                requirements: pins.iter().map(|(n, v)| format!("{n}=={v}")).collect(),
                constraint_file: Some(constraints),
                ..Default::default()
            };
            let cleanup = StepContext {
                cancel: CancelToken::new(),
                ..ctx.clone()
            };
            d.packages
                .install(root, &env.path, &spec, &cleanup)
                .await
                .map_err(|_| NodeAppError::PinDrift)?;
        }
        self.assert_pins(root, env, pins, ctx).await
    }
    async fn dependencies(
        &self,
        root: &std::path::Path,
        op: &mut NodeOperation,
        mut env: PythonEnvironment,
        package: Option<&PackagePlan>,
        decision: &CommitDecision,
        step: &StepContext,
    ) -> Result<(), NodeAppError> {
        let d = &self.deps.environment;
        // Acquired after publication and held through the awaited cleanup below, so a refused
        // or substituted location fails before the environment is marked for repair.
        let guard = if op.requirements_digest.is_some() || decision.consent.run_install_py {
            Some(
                self.pin(root, op, None, decision.consent.run_install_py)
                    .await?,
            )
        } else {
            None
        };
        let was_verified = env.state() == EnvState::Verified;
        // Node operations keep the protected pins exactly as recorded.
        let run_pins = env.pins.clone();
        // Persist both intent records before spawning the first package/hook mutation.
        self.phase(op, OperationPhase::DependenciesInProgress)
            .await?;
        if was_verified {
            env.mark_repair_required(&d.clock.now())
                .map_err(|_| NodeAppError::EnvironmentUnavailable)?;
            d.envs.save(&env).await.map_err(|_| NodeAppError::Storage)?;
        }
        let profile = d
            .catalog
            .get(&env.profile_id)
            .ok_or(NodeAppError::EnvironmentUnavailable)?;
        let constraint = d
            .packages
            .write_constraints(root, &env.id, &run_pins, &profile.constraints)
            .await
            .map_err(|_| NodeAppError::DependenciesFailed)?;
        if let Some(package) = package {
            // Freeze every resolved distribution to the previewed version. Keep root requirements in
            // the command too, so already-satisfied requirements remain enforced by the resolver.
            let mut requirements = op.dependency_requirements.clone();
            for p in &package.installs {
                let name = DistName::new(&p.name).map_err(|_| NodeAppError::ResolutionFailed)?;
                if p.version.is_empty()
                    || p.version
                        .chars()
                        .any(|c: char| c.is_control() || c.is_whitespace())
                {
                    return Err(NodeAppError::ResolutionFailed);
                }
                requirements.push(format!("{name}=={}", p.version));
            }
            let spec = InstallSpec {
                requirements,
                constraint_file: Some(constraint),
                ..guarded_files(guard.as_deref())
            };
            let result = d.packages.install(root, &env.path, &spec, step).await;
            let guard = self.reassert(root, &env, &run_pins, step).await;
            if result.is_err() {
                return Err(NodeAppError::DependenciesFailed);
            }
            guard?;
        }
        if decision.consent.run_install_py {
            let location = guard.as_deref().ok_or(NodeAppError::UnsafeSource)?;
            let result = self.deps.hook.run(root, &env, location, step).await;
            let guard = self.reassert(root, &env, &run_pins, step).await;
            if result.is_err() {
                return Err(NodeAppError::DependenciesFailed);
            }
            guard?;
        }
        if step.cancel.is_cancelled() {
            return Err(NodeAppError::Cancelled);
        }
        self.assert_pins(root, &env, &run_pins, step).await?;
        let mut expectations = profile.verify;
        if let Some(v) = run_pins.get(&DistName::new("torch").expect("static distribution")) {
            expectations.torch_version_prefix = v.into();
        }
        let report = d
            .verifier
            .verify_final(root, &env.path, &expectations, step)
            .await
            .map_err(|_| NodeAppError::DependenciesFailed)?;
        env.record_pins(run_pins.clone(), &d.clock.now());
        if was_verified {
            env.mark_repaired(run_pins, report.summary(), &d.clock.now())
                .map_err(|_| NodeAppError::DependenciesFailed)?;
        }
        d.envs.save(&env).await.map_err(|_| NodeAppError::Storage)?;
        Ok(())
    }
}
