use domain::environment::{EnvState, PythonEnvironment, check_activation};

use super::{
    deps::EnvironmentDeps,
    error::EnvironmentAppError,
    model::{
        ActivationPhase, FailureKind, InstallManifest, MANIFEST_SCHEMA_VERSION, ManifestState,
        ProvisioningRun, RunStatus, StepFailure, StepId, StepStatus,
    },
};

/// Builds the `install.json` content that names `env` as the active environment of `run`.
pub(crate) async fn build_manifest(
    deps: &EnvironmentDeps,
    run: &ProvisioningRun,
    env: &PythonEnvironment,
) -> Result<InstallManifest, EnvironmentAppError> {
    let generation = match deps.manifests.read(&run.request.install_root).await? {
        ManifestState::Valid(existing) if existing.env_id == env.id => existing.generation,
        ManifestState::Valid(existing) => existing.generation + 1,
        ManifestState::Absent => 1,
        ManifestState::Corrupt | ManifestState::Newer => {
            return Err(EnvironmentAppError::RecoveryRequired);
        }
    };
    Ok(InstallManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        install_root: run.request.install_root.clone(),
        generation,
        env_id: env.id.clone(),
        env_path: env.path.clone(),
        ownership: env.ownership,
        python_version: env.python_version,
        profile_id: env.profile_id.clone(),
        profile_revision: env.profile_revision,
        comfyui_reference: run
            .resolved
            .comfyui
            .as_ref()
            .map(|resolved| resolved.reference.clone()),
        comfyui_commit: run
            .resolved
            .comfyui
            .as_ref()
            .map(|resolved| resolved.commit.clone()),
        run_id: run.id.clone(),
        activated_at: deps.clock.now(),
    })
}

/// Saves the next activation phase. When the save fails the in-memory phase stays at the
/// persisted one, so later saves cannot claim progress that was never recorded.
async fn persist_phase(
    deps: &EnvironmentDeps,
    run: &mut ProvisioningRun,
    next: ActivationPhase,
) -> Result<(), EnvironmentAppError> {
    let previous = run.activation;
    run.activation = next;
    run.updated_at = deps.clock.now();
    if let Err(error) = deps.runs.save(run).await {
        run.activation = previous;
        return Err(error.into());
    }
    Ok(())
}

/// Drives the activation phases for a verified candidate. Each phase is persisted before the
/// next one starts, so a crash at any point is resolved by [`complete_activation`].
///
/// 1. record the intent (old and new environment ids),
/// 2. atomically replace `install.json`,
/// 3. switch the active selection and complete the phase in one SQLite transaction.
pub(crate) async fn activate(
    deps: &EnvironmentDeps,
    run: &mut ProvisioningRun,
    candidate: &PythonEnvironment,
) -> Result<(), EnvironmentAppError> {
    if run.activation == ActivationPhase::Committed {
        return Ok(());
    }
    if run.activation == ActivationPhase::None {
        let current = deps.envs.active().await?;
        check_activation(candidate, current.as_ref())?;
        run.previous_env = current.map(|env| env.id);
        persist_phase(deps, run, ActivationPhase::Intent).await?;
    }
    complete_activation(deps, run, candidate).await
}

/// Rolls an interrupted activation forward from its persisted phase.
pub(crate) async fn complete_activation(
    deps: &EnvironmentDeps,
    run: &mut ProvisioningRun,
    candidate: &PythonEnvironment,
) -> Result<(), EnvironmentAppError> {
    if run.activation == ActivationPhase::Intent {
        let manifest = build_manifest(deps, run, candidate).await?;
        deps.manifests.publish(&manifest).await?;
        persist_phase(deps, run, ActivationPhase::ManifestPublished).await?;
    }
    if run.activation == ActivationPhase::ManifestPublished {
        run.activation = ActivationPhase::Committed;
        run.updated_at = deps.clock.now();
        if let Err(error) = deps.activation.commit_activation(run, &candidate.id).await {
            run.activation = ActivationPhase::ManifestPublished;
            return Err(error.into());
        }
    }
    Ok(())
}

/// Resolves state left behind by a crash or kill, before maintenance commands are exposed:
/// stale active runs become retryable and incomplete activations roll forward.
pub struct ReconcileStartup {
    deps: EnvironmentDeps,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ReconcileReport {
    pub interrupted_runs: usize,
    pub activations_completed: usize,
    pub activations_abandoned: usize,
}

impl ReconcileStartup {
    pub fn new(deps: EnvironmentDeps) -> Self {
        Self { deps }
    }

    pub async fn execute(&self) -> Result<ReconcileReport, EnvironmentAppError> {
        let deps = &self.deps;
        let _lease = deps.gate.acquire().await?;
        let mut report = ReconcileReport::default();
        for mut run in deps.runs.unfinished().await? {
            let now = deps.clock.now();
            if run.status.is_active() {
                run.status = RunStatus::Interrupted;
                for record in &mut run.steps {
                    if record.status == StepStatus::Running {
                        record.status = StepStatus::Interrupted;
                    }
                }
                let step = run
                    .steps
                    .iter()
                    .find(|record| record.status == StepStatus::Interrupted)
                    .map_or(StepId::Preflight, |record| record.step);
                run.failure = Some(StepFailure {
                    step,
                    kind: FailureKind::Interrupted,
                    message: "The app stopped while setup was running. Retry to resume it.".into(),
                    command: None,
                    exit_code: None,
                    tail: Vec::new(),
                    consent: None,
                });
                run.updated_at = now.clone();
                report.interrupted_runs += 1;
            }
            if run.activation.is_incomplete() {
                let candidate = deps.envs.get(&run.target_env).await?;
                match candidate {
                    Some(env) if env.state() == EnvState::Verified => {
                        complete_activation(deps, &mut run, &env).await?;
                        if let Some(record) =
                            run.steps.get_mut(usize::from(StepId::FinalVerify.number()))
                        {
                            record.status = StepStatus::Succeeded;
                        }
                        for record in &mut run.steps {
                            if matches!(
                                record.status,
                                StepStatus::Pending | StepStatus::Interrupted
                            ) {
                                record.status = StepStatus::Skipped;
                                record.message =
                                    Some("Skipped after the interrupted setup activated.".into());
                            }
                        }
                        run.status = RunStatus::Succeeded;
                        run.failure = None;
                        report.activations_completed += 1;
                    }
                    _ => {
                        run.activation = ActivationPhase::None;
                        report.activations_abandoned += 1;
                    }
                }
            }
            run.updated_at = now;
            deps.runs.save(&run).await?;
        }
        Ok(report)
    }
}
