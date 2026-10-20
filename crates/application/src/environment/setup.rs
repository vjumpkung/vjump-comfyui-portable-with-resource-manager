use std::path::PathBuf;

use domain::environment::{Eligibility, EnvId, Ownership, PythonSourceKind, SetupMode, evaluate};

use super::{
    deps::{EnvironmentDeps, same_location},
    error::EnvironmentAppError,
    model::{
        ActivationPhase, ComfyRef, CuratedProgress, CuratedStatus, ManifestState, ProvisioningRun,
        PythonSourceInput, PythonTarget, ResolvedSources, RunId, RunStatus, SetupRequest, StepId,
        StepPolicy, StepStatus,
    },
    ports::{PythonValidationReport, ValidationTarget},
};
use crate::custom_nodes::CommitDecision;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupAction {
    New(Box<SetupRequest>),
    /// Resumes a failed or interrupted run. `manager_consent` approves the exact
    /// ComfyUI-Manager commit that will execute code. `curated_decisions` answer the reviews of
    /// the curated custom nodes, one decision per reviewed plan.
    Retry {
        run_id: RunId,
        manager_consent: Option<String>,
        curated_decisions: Vec<CommitDecision>,
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
                curated_decisions,
            } => self.retry(run_id, manager_consent, curated_decisions).await,
            SetupAction::SkipStep { run_id, step } => self.skip(run_id, step).await,
        }
    }

    async fn ensure_idle(&self) -> Result<(), EnvironmentAppError> {
        let busy = self.deps.runs.unfinished().await?.iter().any(|run| {
            run.status.is_active()
                && same_location(&run.request.install_root, &self.deps.root.current().root)
        });
        if busy {
            Err(EnvironmentAppError::SetupInProgress)
        } else {
            Ok(())
        }
    }

    async fn register(&self, mut request: SetupRequest) -> Result<RunId, EnvironmentAppError> {
        let deps = &self.deps;
        if !same_location(&request.install_root, &deps.root.current().root) {
            return Err(EnvironmentAppError::RebuildRequired);
        }
        let _rebuild_lease = if request.mode == SetupMode::Rebuild {
            Some(deps.gate.try_acquire().await?)
        } else {
            None
        };
        let manifest = deps.manifests.read(&request.install_root).await?;
        if matches!(manifest, ManifestState::Corrupt | ManifestState::Newer) {
            return Err(EnvironmentAppError::RecoveryRequired);
        }
        let active = deps.envs.active().await?;
        if request.mode != SetupMode::Rebuild
            && let (ManifestState::Valid(manifest), Some(active)) = (&manifest, &active)
            && matches!(
                deps.installation
                    .inspect(&request.install_root, manifest, active)
                    .await?,
                super::InstallationHealth::RebuildRequired(super::RebuildReason::Moved)
            )
        {
            return Err(EnvironmentAppError::RebuildRequired);
        }
        let mut rebuild_sources = None;
        let mut rebuild_components = None;
        let mut profile = if request.mode == SetupMode::Rebuild {
            let manifest = match &manifest {
                ManifestState::Valid(manifest) => manifest,
                _ => return Err(EnvironmentAppError::RecoveryRequired),
            };
            deps.relocation
                .check(&manifest.install_root, &request.install_root)
                .await?;
            let active = active
                .as_ref()
                .ok_or(EnvironmentAppError::NoActiveEnvironment)?;
            let recorded = deps
                .runs
                .get(&manifest.run_id)
                .await?
                .filter(|run| {
                    run.status == RunStatus::Succeeded
                        && run.target_env == manifest.env_id
                        && run.target_env == active.id
                        && run.profile.id == manifest.profile_id
                        && run.profile.id == active.profile_id
                        && run.profile.revision == manifest.profile_revision
                        && run.profile.revision == active.profile_revision
                        && active.python_version == manifest.python_version
                })
                .ok_or(EnvironmentAppError::ProfileRevisionUnavailable)?;
            let checkout = deps
                .source
                .inspect_checkout(&request.install_root)
                .await
                .map_err(|_| EnvironmentAppError::RebuildCheckoutInvalid)?;
            let commit = match checkout {
                super::Checkout::Absent => return Err(EnvironmentAppError::RebuildCheckoutMissing),
                super::Checkout::NotGit => return Err(EnvironmentAppError::RebuildCheckoutInvalid),
                super::Checkout::Repo { commit } => commit,
            };
            deps.installation
                .validate_rebuild_checkout(&request.install_root)
                .await
                .map_err(|_| EnvironmentAppError::RebuildCheckoutInvalid)?;
            let current_root = request.install_root.clone();
            request = recorded.request.clone();
            request.mode = SetupMode::Rebuild;
            request.install_root = current_root;
            request.python_source = PythonSourceInput::Managed;
            request.comfyui_ref = ComfyRef::KeepCurrent;
            request.curated_nodes.clear();
            request.allow_source_builds = false;
            request.confirm_adopted_changes = false;
            rebuild_sources = Some(ResolvedSources {
                comfyui: Some(super::ResolvedGit {
                    url: String::new(),
                    reference: "current".into(),
                    branch: None,
                    commit,
                }),
                manager: recorded.resolved.manager.clone(),
            });
            rebuild_components = Some(recorded.components.clone());
            recorded.profile
        } else if request.mode == SetupMode::Repair {
            let env = active
                .as_ref()
                .ok_or(EnvironmentAppError::NoActiveEnvironment)?;
            let recorded = match &manifest {
                ManifestState::Valid(manifest) if manifest.env_id == env.id => {
                    deps.runs.get(&manifest.run_id).await?
                }
                _ => None,
            };
            if let Some(run) = recorded.filter(|run| {
                run.target_env == env.id
                    && run.profile.id == env.profile_id
                    && run.profile.revision == env.profile_revision
            }) {
                request.profile_id = run.profile.id.clone();
                request.accelerators = run.request.accelerators;
                request.install_manager = run.request.install_manager;
                run.profile
            } else {
                let profile = deps
                    .catalog
                    .get_revision(&env.profile_id, env.profile_revision)
                    .ok_or(EnvironmentAppError::ProfileRevisionUnavailable)?;
                let components = deps.envs.load_components(&env.id).await?;
                request.profile_id = profile.id.clone();
                request.accelerators = components
                    .accelerators
                    .iter()
                    .filter(|r| r.selected)
                    .map(|r| r.id.clone())
                    .collect();
                request.install_manager = components.manager.is_some_and(|r| r.selected);
                profile
            }
        } else {
            deps.catalog
                .get(&request.profile_id)
                .ok_or(EnvironmentAppError::ProfileNotFound)?
        };
        if request.mode == SetupMode::Rebuild
            && let ManifestState::Valid(manifest) = &manifest
        {
            profile.python = manifest.python_version;
        }
        let mut selected = std::collections::HashSet::new();
        if request.accelerators.iter().any(|id| {
            !selected.insert(id) || !profile.accelerators.iter().any(|spec| &spec.id == id)
        }) {
            return Err(EnvironmentAppError::AcceleratorSelectionInvalid);
        }
        request.mode.check(active.as_ref(), &profile)?;
        let curated = self.curated_progress(&request)?;
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

        if request.mode == SetupMode::Rebuild
            && let ManifestState::Valid(manifest) = &manifest
        {
            deps.relocation_runtime
                .ensure_stopped(&request.install_root)
                .await?;
            deps.relocation_runtime
                .ensure_stopped(&manifest.install_root)
                .await?;
            deps.relocation
                .accept(&manifest.install_root, &request.install_root)
                .await?;
        }

        let now = deps.clock.now();
        let components = rebuild_components
            .unwrap_or_else(|| super::model::InstalledComponents::selected(&request, &profile));
        let run = ProvisioningRun {
            id: run_id.clone(),
            components,
            request,
            profile,
            target_env: env_id,
            previous_env: active.map(|env| env.id),
            python_target: target,
            resolved: rebuild_sources.unwrap_or_default(),
            steps: ProvisioningRun::fresh_steps(),
            status: RunStatus::Registered,
            cancel_requested: false,
            failure: None,
            activation: ActivationPhase::None,
            manager_consent: None,
            curated,
            created_at: now.clone(),
            updated_at: now,
        };
        deps.runs.insert(&run).await?;
        deps.active_runs.register(&run_id);
        Ok(run_id)
    }

    /// Validates the curated selection and lists it in catalog order; an unavailable entry cannot
    /// be selected. A selection is not consent: the nodes are reviewed and confirmed when the run
    /// reaches them.
    fn curated_progress(
        &self,
        request: &SetupRequest,
    ) -> Result<Vec<CuratedProgress>, EnvironmentAppError> {
        if request.curated_nodes.is_empty() {
            return Ok(Vec::new());
        }
        let catalog = self.deps.curated.list();
        let mut selected = std::collections::HashSet::new();
        if request.mode != SetupMode::FirstRun
            || request.curated_nodes.iter().any(|id| {
                !selected.insert(id)
                    || !catalog
                        .iter()
                        .any(|entry| &entry.id == id && entry.unavailable.is_none())
            })
        {
            return Err(EnvironmentAppError::CuratedSelectionInvalid);
        }
        Ok(catalog
            .iter()
            .filter(|entry| request.curated_nodes.contains(&entry.id))
            .map(|entry| CuratedProgress::pending(entry.id.clone()))
            .collect())
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
        curated_decisions: Vec<CommitDecision>,
    ) -> Result<RunId, EnvironmentAppError> {
        let mut run = self.load(&run_id).await?;
        if !run.status.is_retryable() {
            return Err(EnvironmentAppError::NotRetryable);
        }
        self.ensure_idle().await?;
        if let Some(commit) = manager_consent {
            run.manager_consent = Some(commit);
        }
        let id = self.requeue(run, None).await?;
        // The decisions answer reviews that live in this process; they are never stored.
        self.deps.active_runs.set_decisions(&id, curated_decisions);
        Ok(id)
    }

    async fn skip(&self, run_id: RunId, step: StepId) -> Result<RunId, EnvironmentAppError> {
        let mut run = self.load(&run_id).await?;
        let skippable = run.status == RunStatus::Failed
            && step.policy() == StepPolicy::Optional
            && run
                .failure
                .as_ref()
                .is_some_and(|failure| failure.step == step && failure.can_skip);
        let damaged = self
            .deps
            .envs
            .get(&run.target_env)
            .await?
            .is_some_and(|env| env.state() == domain::environment::EnvState::Failed);
        if !skippable
            || damaged
            || run
                .curated
                .iter()
                .any(|entry| entry.status == CuratedStatus::Failed)
        {
            return Err(EnvironmentAppError::StepNotSkippable);
        }
        self.ensure_idle().await?;
        let record = run.step_mut(step);
        record.status = StepStatus::Skipped;
        record.message = Some("Skipped by the user.".into());
        if step == StepId::Manager
            && let Some(manager) = &mut run.components.manager
        {
            manager.status = super::model::ComponentStatus::Skipped;
            manager.message = Some("Skipped by the user.".into());
        }
        if step == StepId::CuratedNodes {
            for entry in &mut run.curated {
                if entry.status != CuratedStatus::Installed {
                    entry.status = CuratedStatus::Skipped;
                    entry.plan = None;
                    entry.message = Some("Skipped by the user.".into());
                }
            }
        }
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
        let run = self
            .deps
            .runs
            .get(id)
            .await?
            .ok_or(EnvironmentAppError::RunNotFound)?;
        if !same_location(&run.request.install_root, &self.deps.root.current().root) {
            return Err(EnvironmentAppError::RebuildRequired);
        }
        Ok(run)
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
