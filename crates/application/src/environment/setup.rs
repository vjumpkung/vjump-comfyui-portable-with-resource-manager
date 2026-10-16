use std::path::PathBuf;

use domain::environment::{Eligibility, EnvId, Ownership, PythonSourceKind, SetupMode, evaluate};

use super::{
    deps::{EnvironmentDeps, same_location},
    error::EnvironmentAppError,
    model::{
        ActivationPhase, ComfyRef, ManifestState, ProvisioningRun, PythonSourceInput, PythonTarget,
        ResolvedSources, RunId, RunStatus, SetupRequest, StepId, StepPolicy, StepStatus,
    },
    ports::{PythonValidationReport, ValidationTarget},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupAction {
    New(Box<SetupRequest>),
    /// Resumes a failed or interrupted run. `manager_consent` approves the exact
    /// ComfyUI-Manager commit that will execute code.
    Retry {
        run_id: RunId,
        manager_consent: Option<String>,
    },
    /// Skips a failed optional step and resumes the run.
    SkipStep {
        run_id: RunId,
        step: StepId,
    },
}

/// Durably registers a setup run (or resumes one) and returns its id. The caller then starts
/// [`super::ExecuteSetup`] on a background task.
pub struct StartSetup {
    deps: EnvironmentDeps,
}

impl StartSetup {
    pub fn new(deps: EnvironmentDeps) -> Self {
        Self { deps }
    }

    pub async fn execute(&self, action: SetupAction) -> Result<RunId, EnvironmentAppError> {
        match action {
            SetupAction::New(request) => self.register(*request).await,
            SetupAction::Retry {
                run_id,
                manager_consent,
            } => self.retry(run_id, manager_consent).await,
            SetupAction::SkipStep { run_id, step } => self.skip(run_id, step).await,
        }
    }

    async fn ensure_idle(&self) -> Result<(), EnvironmentAppError> {
        let busy = self
            .deps
            .runs
            .unfinished()
            .await?
            .iter()
            .any(|run| run.status.is_active());
        if busy {
            Err(EnvironmentAppError::SetupInProgress)
        } else {
            Ok(())
        }
    }

    async fn register(&self, request: SetupRequest) -> Result<RunId, EnvironmentAppError> {
        let deps = &self.deps;
        let profile = deps
            .catalog
            .get(&request.profile_id)
            .ok_or(EnvironmentAppError::ProfileNotFound)?;
        match deps.manifests.read(&request.install_root).await? {
            ManifestState::Corrupt | ManifestState::Newer => {
                return Err(EnvironmentAppError::RecoveryRequired);
            }
            ManifestState::Absent | ManifestState::Valid(_) => {}
        }
        let active = deps.envs.active().await?;
        request.mode.check(active.as_ref(), &profile)?;
        if request.mode != SetupMode::FirstRun
            && !matches!(request.comfyui_ref, ComfyRef::KeepCurrent)
        {
            return Err(EnvironmentAppError::RefChangeNotAllowed);
        }
        if request.mode == SetupMode::SwitchVenv
            && !matches!(request.python_source, PythonSourceInput::CustomVenv(_))
        {
            return Err(EnvironmentAppError::PythonSourceRejected(
                "switching needs a custom virtual environment path".into(),
            ));
        }
        self.ensure_idle().await?;

        let run_id = RunId::new(deps.ids.new_id())?;
        let env_id = EnvId::new(format!("env-{}", deps.ids.new_id()))?;
        let target = self.resolve_target(&request, &profile, &env_id).await?;
        if active
            .as_ref()
            .is_some_and(|active| same_location(&active.path, &target.env_path))
        {
            return Err(domain::environment::EnvironmentError::TargetIsActiveEnvironment.into());
        }
        if target.ownership == Ownership::Adopted && !request.confirm_adopted_changes {
            return Err(
                domain::environment::EnvironmentError::AdoptedRebuildNeedsConfirmation.into(),
            );
        }

        let hardware = deps
            .hardware
            .probe(
                &request.install_root,
                separate_env_path(&target, &request.install_root),
            )
            .await;
        if let Eligibility::Disabled(reasons) = evaluate(&profile, &hardware) {
            return Err(EnvironmentAppError::ProfileDisabled(reasons));
        }

        let now = deps.clock.now();
        let run = ProvisioningRun {
            id: run_id.clone(),
            request,
            profile,
            target_env: env_id,
            previous_env: active.map(|env| env.id),
            python_target: target,
            resolved: ResolvedSources::default(),
            steps: ProvisioningRun::fresh_steps(),
            status: RunStatus::Registered,
            cancel_requested: false,
            failure: None,
            activation: ActivationPhase::None,
            manager_consent: None,
            created_at: now.clone(),
            updated_at: now,
        };
        deps.runs.insert(&run).await?;
        deps.active_runs.register(&run_id);
        Ok(run_id)
    }

    async fn resolve_target(
        &self,
        request: &SetupRequest,
        profile: &domain::environment::SetupProfile,
        env_id: &EnvId,
    ) -> Result<PythonTarget, EnvironmentAppError> {
        let managed_path = EnvironmentDeps::envs_dir(&request.install_root).join(env_id.as_str());
        if matches!(request.python_source, PythonSourceInput::Managed) {
            return Ok(PythonTarget {
                ownership: Ownership::AppOwned,
                source: PythonSourceKind::Managed,
                env_path: managed_path,
                interpreter: None,
                create_venv: true,
                development_files_ok: None,
            });
        }
        let report = self
            .deps
            .python
            .validate(&request.install_root, &request.python_source)
            .await?;
        let rejected = |reason: &str| EnvironmentAppError::PythonSourceRejected(reason.into());
        if !report.usable() {
            let detail = report
                .checks
                .iter()
                .find(|check| !check.passed)
                .map_or("the selected location is not usable", |check| {
                    check.detail.as_str()
                });
            return Err(rejected(detail));
        }
        if let Some(version) = report.python_version
            && !version.same_minor(profile.python)
            && report.target != ValidationTarget::EmptyFolder
        {
            return Err(EnvironmentAppError::PythonSourceRejected(format!(
                "the profile needs Python {}.{} but the selected Python is {version}",
                profile.python.major, profile.python.minor
            )));
        }
        target_from_report(&request.python_source, report, managed_path)
    }

    async fn retry(
        &self,
        run_id: RunId,
        manager_consent: Option<String>,
    ) -> Result<RunId, EnvironmentAppError> {
        let mut run = self.load(&run_id).await?;
        if !run.status.is_retryable() {
            return Err(EnvironmentAppError::NotRetryable);
        }
        self.ensure_idle().await?;
        if let Some(commit) = manager_consent {
            run.manager_consent = Some(commit);
        }
        self.requeue(run, None).await
    }

    async fn skip(&self, run_id: RunId, step: StepId) -> Result<RunId, EnvironmentAppError> {
        let mut run = self.load(&run_id).await?;
        let skippable = run.status == RunStatus::Failed
            && step.policy() == StepPolicy::Optional
            && run
                .failure
                .as_ref()
                .is_some_and(|failure| failure.step == step);
        if !skippable {
            return Err(EnvironmentAppError::StepNotSkippable);
        }
        self.ensure_idle().await?;
        let record = run.step_mut(step);
        record.status = StepStatus::Skipped;
        record.message = Some("Skipped by the user.".into());
        self.requeue(run, Some(step)).await
    }

    async fn requeue(
        &self,
        mut run: ProvisioningRun,
        keep: Option<StepId>,
    ) -> Result<RunId, EnvironmentAppError> {
        for record in &mut run.steps {
            if Some(record.step) != keep
                && matches!(
                    record.status,
                    StepStatus::Failed | StepStatus::Interrupted | StepStatus::Running
                )
            {
                record.status = StepStatus::Pending;
            }
        }
        run.failure = None;
        run.cancel_requested = false;
        run.status = RunStatus::Registered;
        run.updated_at = self.deps.clock.now();
        self.deps.runs.save(&run).await?;
        self.deps.active_runs.register(&run.id);
        Ok(run.id)
    }

    async fn load(&self, id: &RunId) -> Result<ProvisioningRun, EnvironmentAppError> {
        self.deps
            .runs
            .get(id)
            .await?
            .ok_or(EnvironmentAppError::RunNotFound)
    }
}

/// The environment folder when it lives outside the install root (so its volume is checked).
pub(crate) fn separate_env_path<'a>(
    target: &'a PythonTarget,
    install_root: &std::path::Path,
) -> Option<&'a std::path::Path> {
    (target.create_venv && !target.env_path.starts_with(install_root))
        .then_some(target.env_path.as_path())
}

fn target_from_report(
    source: &PythonSourceInput,
    report: PythonValidationReport,
    managed_path: PathBuf,
) -> Result<PythonTarget, EnvironmentAppError> {
    let development_files_ok = report.development_files_ok();
    match (source, report.target) {
        (PythonSourceInput::CustomInterpreter(_), ValidationTarget::Interpreter) => {
            Ok(PythonTarget {
                ownership: Ownership::AppOwned,
                source: PythonSourceKind::CustomInterpreter,
                env_path: managed_path,
                interpreter: Some(report.resolved_path),
                create_venv: true,
                development_files_ok,
            })
        }
        (PythonSourceInput::CustomVenv(_), ValidationTarget::EmptyFolder) => Ok(PythonTarget {
            ownership: Ownership::AppOwned,
            source: PythonSourceKind::CustomVenv,
            env_path: report.resolved_path,
            interpreter: None,
            create_venv: true,
            development_files_ok: None,
        }),
        (PythonSourceInput::CustomVenv(_), ValidationTarget::ExistingVenv) => Ok(PythonTarget {
            ownership: Ownership::Adopted,
            source: PythonSourceKind::CustomVenv,
            env_path: report.resolved_path,
            interpreter: None,
            create_venv: false,
            development_files_ok,
        }),
        _ => Err(EnvironmentAppError::PythonSourceRejected(
            "the selected location does not match the chosen Python source".into(),
        )),
    }
}

/// Requests cancellation of a run, or aborts a run that is waiting for the user.
///
/// This never acquires the maintenance gate: the executing workflow owns it and releases it only
/// after the process tree is gone and cleanup finished.
pub struct CancelSetup {
    deps: EnvironmentDeps,
}

impl CancelSetup {
    pub fn new(deps: EnvironmentDeps) -> Self {
        Self { deps }
    }

    pub async fn execute(&self, run_id: &RunId) -> Result<(), EnvironmentAppError> {
        let deps = &self.deps;
        let mut run = deps
            .runs
            .get(run_id)
            .await?
            .ok_or(EnvironmentAppError::RunNotFound)?;
        if run.status.is_active() {
            // The executing workflow observes the token, tears the process tree down and
            // records the final state itself.
            return match deps.active_runs.get(run_id) {
                Some(token) => {
                    token.cancel();
                    Ok(())
                }
                None => Err(EnvironmentAppError::NotCancellable),
            };
        }
        if !run.status.is_retryable() {
            return Err(EnvironmentAppError::NotCancellable);
        }
        // Abort a run that stopped at a failed step: the partial environment stays on disk as a
        // failed environment that the user can delete.
        run.status = RunStatus::Cancelled;
        run.cancel_requested = true;
        run.failure = None;
        run.updated_at = deps.clock.now();
        if let Some(mut env) = deps.envs.get(&run.target_env).await?
            && env.state() == domain::environment::EnvState::Provisioning
        {
            env.mark_failed(&run.updated_at)?;
            deps.envs.save(&env).await?;
        }
        deps.runs.save(&run).await?;
        deps.active_runs.remove(run_id);
        Ok(())
    }
}
