use super::*;
use crate::{
    environment::{InstallSpec, ManifestState, NullSink, RunId, StepContext, StepId},
    shared::CancelToken,
};
use domain::{
    comfy_runtime::*,
    environment::{DistName, EnvState, PinSet, PythonEnvironment},
};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct UpdateRequest {
    pub target: UpdateTarget,
    pub stop_running: bool,
    /// Retries the exact persisted target, never resolves "latest" a second time.
    pub retry: Option<SessionId>,
}

pub struct UpdateComfy {
    deps: RuntimeDeps,
}
impl UpdateComfy {
    pub fn new(deps: RuntimeDeps) -> Self {
        Self { deps }
    }
    pub async fn execute(
        &self,
        input: UpdateRequest,
        cancel: CancelToken,
    ) -> Result<UpdateRun, RuntimeError> {
        let deps = &self.deps;
        let mut lease = deps
            .environment
            .gate
            .try_acquire()
            .await
            .map_err(|_| RuntimeError::Busy)?;
        let root = deps.environment.root.current().root;
        match deps.supervisor.inventory(&root).await? {
            InventoryState::Stopped => {}
            InventoryState::OwnedRunning if input.stop_running => {
                deps.supervisor.stop().await?;
                deps.nodes.after_stop(&mut *lease).await?;
            }
            InventoryState::OwnedRunning => return Err(RuntimeError::NeedsStop),
            _ => {
                return Err(RuntimeError::RecoveryRequired(
                    "unknown process ownership blocks source/package changes".into(),
                ));
            }
        }
        deps.supervisor
            .ensure_stopped(&root)
            .await
            .map_err(|_| RuntimeError::NeedsStop)?;
        let mut env = super::use_cases::selected(deps, input.retry.is_some()).await?;
        let pending = deps.store.unfinished_update().await?;
        let mut run = match (input.retry, pending) {
            (Some(id), Some(run))
                if run.id == id && run.env_id == env.id && run.pins == env.pins =>
            {
                run
            }
            (None, None) => {
                let revision = deps.repository.revision(&root).await?;
                self.check_repo(&revision, &input.target).await?;
                let ctx = context("update-fetch", cancel.clone(), lease.retained())?;
                deps.repository.fetch(&root, &input.target, &ctx).await?;
                self.check_repo(&revision, &input.target).await?;
                let target = deps.repository.freeze(&root, &input.target).await?;
                let run = UpdateRun {
                    id: SessionId::new(deps.environment.ids.new_id())?,
                    env_id: env.id.clone(),
                    old_commit: revision.commit,
                    target,
                    pins: env.pins.clone(),
                    phase: UpdatePhase::Intent,
                    failure: None,
                    created_at: deps.environment.clock.now(),
                };
                deps.store.save_update(&run).await?;
                run
            }
            _ => {
                return Err(RuntimeError::RecoveryRequired(
                    "retry must name the pending update and unchanged environment/pins".into(),
                ));
            }
        };
        run.failure = None;
        let ctx = context(run.id.as_str(), cancel, lease.retained())?;
        let result = self.finish(&mut run, &mut env, &ctx).await;
        if let Err(error) = result {
            // The phase is deliberately retained. Source checkout is never reset on failure.
            run.failure = Some(error.to_string());
            deps.store.save_update(&run).await?;
            return Err(error);
        }
        Ok(run)
    }

    async fn check_repo(
        &self,
        revision: &Revision,
        target: &UpdateTarget,
    ) -> Result<(), RuntimeError> {
        let root = self.deps.environment.root.current().root;
        let state = self.deps.repository.status(&root).await?;
        can_update(&state, revision, target).map_err(|reason| RuntimeError::UpdateRefused {
            reason,
            status: state.status,
        })
    }

    async fn finish(
        &self,
        run: &mut UpdateRun,
        env: &mut PythonEnvironment,
        ctx: &StepContext,
    ) -> Result<(), RuntimeError> {
        let deps = &self.deps;
        let root = deps.environment.root.current().root;
        if ctx.cancel.is_cancelled() {
            return Err(RuntimeError::Cancelled);
        }
        let current = deps.repository.revision(&root).await?;
        // The source may have changed immediately before a crash persisted SourceChanged.
        if current.commit != run.old_commit && current.commit != run.target.commit {
            return Err(RuntimeError::RecoveryRequired(
                "HEAD changed outside the recorded update; inspect the checkout".into(),
            ));
        }
        let policy_target = run.target.policy_target()?;
        self.check_repo(&current, &policy_target).await?;
        if current.commit != run.target.commit {
            if run.phase != UpdatePhase::Intent {
                return Err(RuntimeError::RecoveryRequired(
                    "the update source no longer matches its frozen revision".into(),
                ));
            }
            deps.repository.apply(&root, &run.target, ctx).await?;
        } else if run.phase == UpdatePhase::Intent
            && (policy_target == UpdateTarget::LatestCommit
                || (current.branch.is_none() && !run.target.fast_forward))
        {
            // LatestCommit always establishes its frozen branch/upstream at Intent, even
            // when already attached at the target. Tags retain detached re-placement.
            deps.repository.apply(&root, &run.target, ctx).await?;
        }
        if deps.repository.revision(&root).await?.commit != run.target.commit {
            return Err(RuntimeError::Failed(
                "the source did not reach the frozen target".into(),
            ));
        }
        if run.phase == UpdatePhase::Intent {
            run.phase = UpdatePhase::SourceChanged;
            deps.store.save_update(run).await?;
        }
        let profile = deps
            .environment
            .catalog
            .get(&env.profile_id)
            .ok_or(RuntimeError::EnvironmentNotUsable)?;
        let constraints = deps
            .environment
            .packages
            .write_constraints(&root, &env.id, &run.pins, &profile.constraints)
            .await?;
        let spec = InstallSpec {
            requirement_files: vec![root.join("ComfyUI/requirements.txt")],
            constraint_file: Some(constraints),
            ..Default::default()
        };
        // Never fall back to unconstrained resolution or source builds. Any protected change
        // must go through M1's exact package-plan confirmation before a fresh update.
        let preview = deps
            .environment
            .packages
            .preview_install(&root, &env.path, &spec, ctx)
            .await?;
        for package in preview.installs.iter().chain(&preview.removals) {
            if let Ok(name) = DistName::new(&package.name)
                && let Some(pin) = run.pins.get(&name)
            {
                let removed = preview.removals.iter().any(|p| p.name == package.name)
                    && !preview
                        .installs
                        .iter()
                        .any(|p| p.name == package.name && p.version == pin);
                if removed || package.version != pin {
                    return Err(RuntimeError::RecoveryRequired("requirements would change protected packages; use the environment package-plan confirmation".into()));
                }
            }
        }
        // Persist the failure state BEFORE any in-place package mutation.
        if env.state() == EnvState::Verified {
            env.mark_repair_required(&deps.environment.clock.now())
                .map_err(|_| RuntimeError::EnvironmentNotUsable)?;
            deps.environment
                .envs
                .save(env)
                .await
                .map_err(|_| RuntimeError::Storage)?;
        }
        run.phase = UpdatePhase::Dependencies;
        deps.store.save_update(run).await?;
        deps.environment
            .packages
            .install(&root, &env.path, &spec, ctx)
            .await?;
        let installed = deps
            .environment
            .packages
            .list(&root, &env.path, Some(ctx))
            .await?;
        let mut actual = PinSet::new();
        for package in installed {
            if let Ok(name) = DistName::new(&package.name)
                && run.pins.contains(&name)
            {
                actual.insert(name, package.version);
            }
        }
        if actual != run.pins {
            return Err(RuntimeError::RecoveryRequired(
                "protected pins changed during dependency installation; repair the environment"
                    .into(),
            ));
        }
        run.phase = UpdatePhase::Verifying;
        deps.store.save_update(run).await?;
        let mut expectations = profile.verify;
        expectations.torch_version_prefix = env
            .pins
            .get(&DistName::new("torch").expect("static distribution"))
            .ok_or(RuntimeError::EnvironmentNotUsable)?
            .to_owned();
        let report = deps
            .environment
            .verifier
            .verify_final(&root, &env.path, &expectations, ctx)
            .await?;
        let actual_revision = deps.repository.revision(&root).await?;
        if actual_revision.commit != run.target.commit {
            return Err(RuntimeError::RecoveryRequired(
                "HEAD changed during verification".into(),
            ));
        }
        run.phase = UpdatePhase::Publishing;
        deps.store.save_update(run).await?;
        let ManifestState::Valid(mut manifest) = deps
            .environment
            .manifests
            .read(&root)
            .await
            .map_err(|_| RuntimeError::Storage)?
        else {
            return Err(RuntimeError::InvalidRecord);
        };
        if manifest.env_id != env.id {
            return Err(RuntimeError::RecoveryRequired(
                "the active manifest changed during update".into(),
            ));
        }
        manifest.comfyui_commit = Some(run.target.commit.clone());
        manifest.comfyui_reference = run.target.tag.clone().or(actual_revision.branch);
        manifest.generation = manifest
            .generation
            .checked_add(1)
            .ok_or(RuntimeError::InvalidRecord)?;
        deps.environment
            .manifests
            .publish(&manifest)
            .await
            .map_err(|_| RuntimeError::Storage)?;
        env.mark_verified(
            run.pins.clone(),
            report.summary(),
            &deps.environment.clock.now(),
        )
        .map_err(|_| RuntimeError::EnvironmentNotUsable)?;
        deps.environment
            .envs
            .save(env)
            .await
            .map_err(|_| RuntimeError::Storage)?;
        run.phase = UpdatePhase::Complete;
        deps.store.save_update(run).await
    }
}

fn context(
    id: &str,
    cancel: CancelToken,
    retained: crate::shared::Retained,
) -> Result<StepContext, RuntimeError> {
    Ok(StepContext {
        run_id: RunId::new(id).map_err(|_| RuntimeError::InvalidRecord)?,
        step: StepId::ComfyRequirements,
        sink: Arc::new(NullSink),
        cancel,
        allow_source_builds: false,
        retained,
    })
}
