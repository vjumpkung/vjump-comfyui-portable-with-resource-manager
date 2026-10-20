use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use domain::custom_nodes::{InstallConsent, NodeSource};
use domain::environment::{
    AcceleratorSource, DistName, Eligibility, EnvState, NewEnvironment, PinSet, PythonEnvironment,
    SetupMode, evaluate,
};

use super::{
    activation,
    deps::EnvironmentDeps,
    error::EnvironmentAppError,
    model::{
        ComfyRef, ComponentStatus, ConsentRequest, CuratedNodeSpec, CuratedStatus, FailureKind,
        ProvisioningRun, ResolvedGit, RunId, RunStatus, StepFailure, StepId, StepPolicy,
        StepStatus,
    },
    ports::{
        Checkout, EnvRepoError, InstallSpec, ProvisionError, SetupEvent, SetupNodeError,
        SetupNodes, SetupProgressSink, StepContext,
    },
};
use crate::{
    comfy_runtime::RuntimeBlock,
    custom_nodes::{CommitDecision, PlanReview, SETUP_HOOK_WARNING},
    shared::{CancelToken, MaintenanceLease},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunOutcome {
    Succeeded,
    /// Repair verified the selected environment; nothing was rebuilt.
    NothingToRepair,
    Failed(Box<StepFailure>),
    Cancelled,
}

enum StepOutcome {
    Done,
    Skipped(String),
    Warning(String),
    NothingToRepair,
}

/// Prepare/commit passes the curated step makes when the request carries the review-page
/// consent, so plans that keep going stale cannot loop forever.
const MAX_CURATED_PASSES: usize = 3;

const CURATED_DAMAGE_MESSAGE: &str = "The published node no longer matches its reviewed install or its candidate dependencies failed. Retry setup to repair it; this step cannot be skipped.";

fn curated_hook_note(message: Option<&str>) -> Option<String> {
    message?
        .lines()
        .find(|line| line.contains(SETUP_HOOK_WARNING))
        .map(str::to_owned)
}

/// Runs a registered setup run to completion, failure or cancellation. The maintenance gate is
/// acquired once and held until the run finished and all cleanup is done.
pub struct ExecuteSetup {
    deps: EnvironmentDeps,
    nodes: Arc<dyn SetupNodes>,
    restart_lease: Mutex<Option<Box<dyn MaintenanceLease>>>,
}

impl ExecuteSetup {
    /// `nodes` installs the curated custom nodes against the run's candidate environment. It is
    /// injected here, not through `EnvironmentDeps`, because the node service itself builds on
    /// the environment dependencies.
    pub fn new(deps: EnvironmentDeps, nodes: Arc<dyn SetupNodes>) -> Self {
        Self {
            deps,
            nodes,
            restart_lease: Mutex::new(None),
        }
    }

    pub async fn execute(
        &self,
        run_id: &RunId,
        sink: Arc<dyn SetupProgressSink>,
    ) -> Result<RunOutcome, EnvironmentAppError> {
        self.execute_inner(run_id, sink, false).await
    }

    /// The desktop retains successful rebuild maintenance ownership until restart.
    pub async fn execute_for_restart(
        &self,
        run_id: &RunId,
        sink: Arc<dyn SetupProgressSink>,
    ) -> Result<RunOutcome, EnvironmentAppError> {
        self.execute_inner(run_id, sink, true).await
    }

    async fn execute_inner(
        &self,
        run_id: &RunId,
        sink: Arc<dyn SetupProgressSink>,
        retain_on_success: bool,
    ) -> Result<RunOutcome, EnvironmentAppError> {
        let deps = &self.deps;
        let cancel = deps
            .active_runs
            .get(run_id)
            .unwrap_or_else(|| deps.active_runs.register(run_id));
        let mut lease = match deps.gate.acquire().await {
            Ok(lease) => lease,
            Err(error) => {
                deps.active_runs.remove(run_id);
                return Err(error.into());
            }
        };
        let result = self.run_locked(run_id, sink, cancel, &mut *lease).await;
        deps.installation.invalidate();
        deps.active_runs.remove(run_id);
        if retain_on_success && matches!(result, Ok(RunOutcome::Succeeded)) {
            *self.restart_lease.lock().unwrap_or_else(|p| p.into_inner()) = Some(lease);
        }
        result
    }

    async fn run_locked(
        &self,
        run_id: &RunId,
        sink: Arc<dyn SetupProgressSink>,
        cancel: CancelToken,
        lease: &mut dyn MaintenanceLease,
    ) -> Result<RunOutcome, EnvironmentAppError> {
        let run = self
            .deps
            .runs
            .get(run_id)
            .await?
            .ok_or(EnvironmentAppError::RunNotFound)?;
        if !super::same_location(&run.request.install_root, &self.deps.root.current().root) {
            return Err(EnvironmentAppError::RebuildRequired);
        }
        if run.status != RunStatus::Registered {
            return Err(EnvironmentAppError::NotRunnable);
        }
        if run.request.mode == SetupMode::Rebuild {
            self.deps
                .relocation_runtime
                .ensure_stopped(&run.request.install_root)
                .await?;
            if let super::ManifestState::Valid(manifest) =
                self.deps.manifests.read(&run.request.install_root).await?
            {
                self.deps
                    .relocation_runtime
                    .ensure_stopped(&manifest.install_root)
                    .await?;
            }
        }
        let retained = lease.retained();
        Execution {
            deps: self.deps.clone(),
            nodes: self.nodes.clone(),
            run,
            env: None,
            constraints: None,
            sink,
            cancel,
            retained,
            damaged: false,
        }
        .drive(lease)
        .await
    }
}

struct Execution {
    deps: EnvironmentDeps,
    nodes: Arc<dyn SetupNodes>,
    run: ProvisioningRun,
    env: Option<PythonEnvironment>,
    constraints: Option<PathBuf>,
    sink: Arc<dyn SetupProgressSink>,
    cancel: CancelToken,
    retained: super::Retained,
    /// Set when a published custom node left the candidate environment damaged.
    damaged: bool,
}

fn storage(error: EnvRepoError) -> ProvisionError {
    ProvisionError::Failed(format!("environment storage failed: {error}"))
}

fn failed(message: impl Into<String>) -> ProvisionError {
    ProvisionError::Failed(message.into())
}

impl Execution {
    fn root(&self) -> PathBuf {
        self.run.request.install_root.clone()
    }

    fn env_path(&self) -> PathBuf {
        self.run.python_target.env_path.clone()
    }

    fn ctx(&self, step: StepId) -> StepContext {
        StepContext {
            run_id: self.run.id.clone(),
            step,
            sink: self.sink.clone(),
            cancel: self.cancel.clone(),
            allow_source_builds: self.run.request.allow_source_builds,
            retained: self.retained.clone(),
        }
    }

    fn emit(&self, event: SetupEvent) {
        self.sink.emit(event);
    }

    fn check_cancel(&self) -> Result<(), ProvisionError> {
        if self.cancel.is_cancelled() {
            Err(ProvisionError::Cancelled)
        } else {
            Ok(())
        }
    }

    fn env_mut(&mut self) -> Result<&mut PythonEnvironment, ProvisionError> {
        self.env
            .as_mut()
            .ok_or_else(|| failed("the target environment was not prepared"))
    }

    async fn save_run(&mut self) -> Result<(), EnvironmentAppError> {
        self.run.updated_at = self.deps.clock.now();
        self.deps.runs.save(&self.run).await?;
        Ok(())
    }

    async fn save_env(&self) -> Result<(), ProvisionError> {
        match &self.env {
            Some(env) => self.deps.envs.save(env).await.map_err(storage),
            None => Ok(()),
        }
    }

    fn set_step(&mut self, step: StepId, status: StepStatus, message: Option<String>) {
        let record = self.run.step_mut(step);
        record.status = status;
        record.message = message.clone();
        let attempt = record.attempts;
        self.emit(SetupEvent::Step {
            run_id: self.run.id.clone(),
            step,
            status,
            attempt,
            message,
        });
    }

    async fn set_run_status(&mut self, status: RunStatus) -> Result<(), EnvironmentAppError> {
        self.run.status = status;
        self.save_run().await?;
        self.emit(SetupEvent::RunStatus {
            run_id: self.run.id.clone(),
            status,
        });
        Ok(())
    }

    async fn drive(
        mut self,
        lease: &mut dyn MaintenanceLease,
    ) -> Result<RunOutcome, EnvironmentAppError> {
        self.set_run_status(RunStatus::Running).await?;
        for step in StepId::ALL {
            if self.cancel.is_cancelled() {
                return self.finish_cancelled().await;
            }
            if self.run.step(step).status == StepStatus::Skipped {
                continue;
            }
            self.run.step_mut(step).attempts += 1;
            self.set_step(step, StepStatus::Running, None);
            self.save_run().await?;
            match self.run_step(step, &mut *lease).await {
                Ok(StepOutcome::Done) => self.set_step(step, StepStatus::Succeeded, None),
                Ok(StepOutcome::Skipped(reason)) => {
                    self.set_step(step, StepStatus::Skipped, Some(reason))
                }
                Ok(StepOutcome::Warning(message)) => {
                    self.set_step(step, StepStatus::Warning, Some(message))
                }
                Ok(StepOutcome::NothingToRepair) => return self.finish_nothing_to_repair().await,
                Err(ProvisionError::Cancelled) => return self.finish_cancelled().await,
                Err(error)
                    if step.policy() == StepPolicy::BestEffort
                        && step != StepId::Accelerators
                        && !matches!(error, ProvisionError::PinDrift(_)) =>
                {
                    let message = describe(&error);
                    self.set_step(step, StepStatus::Warning, Some(message));
                }
                Err(error) => return self.finish_failed(step, error).await,
            }
            self.save_run().await?;
        }
        self.set_run_status(RunStatus::Succeeded).await?;
        Ok(RunOutcome::Succeeded)
    }

    async fn finish_failed(
        &mut self,
        step: StepId,
        error: ProvisionError,
    ) -> Result<RunOutcome, EnvironmentAppError> {
        let pin_drift = matches!(error, ProvisionError::PinDrift(_));
        let mut failure = to_failure(step, error);
        if self.damaged {
            failure.can_skip = false;
        }
        self.set_step(step, StepStatus::Failed, Some(failure.message.clone()));
        self.run.failure = Some(failure.clone());
        if step.policy() == StepPolicy::Required || pin_drift || self.damaged {
            self.fail_env().await;
        }
        self.emit(SetupEvent::Failure {
            run_id: self.run.id.clone(),
            failure: Box::new(failure.clone()),
        });
        self.set_run_status(RunStatus::Failed).await?;
        Ok(RunOutcome::Failed(Box::new(failure)))
    }

    async fn finish_cancelled(&mut self) -> Result<RunOutcome, EnvironmentAppError> {
        self.run.cancel_requested = true;
        for record in &mut self.run.steps {
            if record.status == StepStatus::Running {
                record.status = StepStatus::Interrupted;
            }
        }
        self.fail_env().await;
        self.set_run_status(RunStatus::Cancelled).await?;
        Ok(RunOutcome::Cancelled)
    }

    async fn finish_nothing_to_repair(&mut self) -> Result<RunOutcome, EnvironmentAppError> {
        for record in &mut self.run.steps {
            if record.status == StepStatus::Pending {
                record.status = StepStatus::Skipped;
                record.message = Some("Nothing needed repair.".into());
            }
        }
        self.set_run_status(RunStatus::Succeeded).await?;
        Ok(RunOutcome::NothingToRepair)
    }

    /// A required failure marks the new environment failed; the previous one stays active.
    async fn fail_env(&mut self) {
        let now = self.deps.clock.now();
        if let Some(env) = self.env.as_mut()
            && env.state() == EnvState::Provisioning
            && env.mark_failed(&now).is_ok()
        {
            let _ = self.deps.envs.save(env).await;
        }
    }

    async fn run_step(
        &mut self,
        step: StepId,
        lease: &mut dyn MaintenanceLease,
    ) -> Result<StepOutcome, ProvisionError> {
        match step {
            StepId::Preflight => self.preflight().await,
            StepId::Tools => self.tools().await,
            StepId::PythonVenv => self.python_venv().await,
            StepId::Seed => self.seed().await,
            StepId::CloneComfyUi => self.clone_comfyui().await,
            StepId::InstallTorch => self.install_torch().await,
            StepId::VerifyTorch => self.verify_torch().await,
            StepId::Triton => self.triton().await,
            StepId::Accelerators => self.accelerators().await,
            StepId::ComfyRequirements => self.comfy_requirements().await,
            StepId::Manager => {
                if self.run.request.mode == SetupMode::Rebuild {
                    return Ok(StepOutcome::Skipped(
                        "The existing Manager and node requirements were retained.".into(),
                    ));
                }
                let result = self.manager().await;
                if let Some(manager) = &mut self.run.components.manager {
                    manager.source_commit = self
                        .run
                        .resolved
                        .manager
                        .as_ref()
                        .map(|git| git.commit.clone());
                    manager.status = match &result {
                        Ok(StepOutcome::Skipped(_)) => ComponentStatus::Skipped,
                        Ok(_) => ComponentStatus::Installed,
                        Err(_) => ComponentStatus::Failed,
                    };
                    manager.message = result.as_ref().err().map(describe);
                }
                let saved = self.save_components().await;
                if matches!(result, Err(ProvisionError::PinDrift(_))) {
                    return result;
                }
                saved?;
                result
            }
            StepId::CuratedNodes => self.curated_nodes(lease).await,
            StepId::FinalVerify => self.final_verify(lease).await,
            StepId::CacheClean => self.cache_clean().await,
        }
    }

    // ----------------------------------------------------------------------------- steps

    async fn preflight(&mut self) -> Result<StepOutcome, ProvisionError> {
        let deps = self.deps.clone();
        let root = self.root();
        if let Err(block) = deps.runtime.ensure_stopped(&root).await {
            return Err(failed(match block {
                RuntimeBlock::Running => {
                    "ComfyUI from this installation is running. Stop it, then retry."
                }
                RuntimeBlock::CannotAssess => {
                    "The state of ComfyUI could not be determined, so setup was not started."
                }
            }));
        }
        let hardware = deps
            .hardware
            .probe(
                &root,
                super::setup::separate_env_path(
                    &self.run.python_target,
                    &self.run.request.install_root,
                ),
            )
            .await;
        if let Eligibility::Disabled(reasons) = evaluate(&self.run.profile, &hardware) {
            let text: Vec<String> = reasons.iter().map(ToString::to_string).collect();
            return Err(failed(format!(
                "The selected profile is no longer available: {}",
                text.join("; ")
            )));
        }
        if self.run.request.mode == SetupMode::Repair && self.repair_check().await? {
            return Ok(StepOutcome::NothingToRepair);
        }
        self.ensure_env().await?;
        Ok(StepOutcome::Done)
    }

    /// Repair verifies the selected environment first. True when nothing needs rebuilding.
    async fn repair_check(&mut self) -> Result<bool, ProvisionError> {
        let deps = self.deps.clone();
        let root = self.root();
        let Some(active) = deps.envs.active().await.map_err(storage)? else {
            return Ok(false);
        };
        if active.state() != EnvState::Verified {
            return Ok(false);
        }
        let ctx = self.ctx(StepId::Preflight);
        match deps
            .verifier
            .verify_final(&root, &active.path, &self.run.profile.verify, &ctx)
            .await
        {
            Err(ProvisionError::Cancelled) => Err(ProvisionError::Cancelled),
            Err(_) => Ok(false),
            Ok(_) => {
                let installed = deps
                    .packages
                    .list(&root, &active.path, Some(&self.ctx(StepId::Preflight)))
                    .await
                    .map_err(|error| failed(describe(&error)))?;
                let pins = pins_of(&installed, &self.run.profile.protected_distributions());
                if pins != active.pins {
                    return Ok(false);
                }
                if deps
                    .node_requirements
                    .needs_repair(&active.id, &root, self.retained.clone())
                    .await?
                {
                    return Ok(false);
                }
                Ok(true)
            }
        }
    }

    async fn ensure_env(&mut self) -> Result<(), ProvisionError> {
        let deps = self.deps.clone();
        let now = deps.clock.now();
        let existing = deps.envs.get(&self.run.target_env).await.map_err(storage)?;
        let env = match existing {
            Some(mut env) => {
                if env.state() == EnvState::Failed {
                    env.begin_retry(&now)
                        .map_err(|error| failed(error.to_string()))?;
                    deps.envs.save(&env).await.map_err(storage)?;
                }
                env
            }
            None => {
                let target = &self.run.python_target;
                let env = PythonEnvironment::provisioning(NewEnvironment {
                    id: self.run.target_env.clone(),
                    path: target.env_path.clone(),
                    ownership: target.ownership,
                    source: target.source,
                    base_interpreter: target.interpreter.clone(),
                    python_version: self.run.profile.python,
                    profile_id: self.run.profile.id.clone(),
                    profile_revision: self.run.profile.revision,
                    authorized_location: target.env_path.clone(),
                    now,
                });
                deps.envs.save(&env).await.map_err(storage)?;
                env
            }
        };
        if !env.pins.is_empty() {
            self.constraints = Some(
                deps.packages
                    .write_constraints(
                        &self.root(),
                        &env.id,
                        &env.pins,
                        &self.run.profile.constraints,
                    )
                    .await?,
            );
        }
        self.env = Some(env);
        self.save_components().await?;
        Ok(())
    }

    async fn tools(&mut self) -> Result<StepOutcome, ProvisionError> {
        let deps = self.deps.clone();
        let root = self.root();
        let ctx = self.ctx(StepId::Tools);
        deps.tools.ensure_uv(&root, &ctx).await?;
        self.check_cancel()?;
        deps.tools.ensure_git(&root, &ctx).await?;
        self.check_cancel()?;
        if self.run.request.mode == SetupMode::Rebuild {
            deps.tools.recreate_hf_tool(&root, &ctx).await?;
        } else {
            deps.tools.ensure_hf_tool(&root, &ctx).await?;
        }
        Ok(StepOutcome::Done)
    }

    async fn python_venv(&mut self) -> Result<StepOutcome, ProvisionError> {
        let deps = self.deps.clone();
        let root = self.root();
        let ctx = self.ctx(StepId::PythonVenv);
        let target = self.run.python_target.clone();
        let profile_python = self.run.profile.python;
        let info = if target.create_venv {
            let interpreter = match &target.interpreter {
                Some(path) => path.clone(),
                None => {
                    deps.python
                        .install_managed_python(&root, profile_python, &ctx)
                        .await?
                }
            };
            self.check_cancel()?;
            deps.python
                .create_venv(&root, &interpreter, &target.env_path, &ctx)
                .await?
        } else {
            deps.python.inspect_venv(&root, &target.env_path).await?
        };
        if !info.version.same_minor(profile_python)
            || (self.run.request.mode == SetupMode::Rebuild && info.version != profile_python)
        {
            return Err(failed(format!(
                "The environment uses Python {} but the profile needs {}.{}.",
                info.version, profile_python.major, profile_python.minor
            )));
        }
        let env = self.env_mut()?;
        env.python_version = info.version;
        env.base_interpreter = info.base_interpreter.or(env.base_interpreter.take());
        self.save_env().await?;
        Ok(StepOutcome::Done)
    }

    async fn seed(&mut self) -> Result<StepOutcome, ProvisionError> {
        let ctx = self.ctx(StepId::Seed);
        let result = self
            .deps
            .python
            .seed(&self.root(), &self.env_path(), self.run.profile.os, &ctx)
            .await;
        self.reassert_pins(&ctx).await?;
        result?;
        Ok(StepOutcome::Done)
    }

    async fn clone_comfyui(&mut self) -> Result<StepOutcome, ProvisionError> {
        let deps = self.deps.clone();
        let root = self.root();
        let ctx = self.ctx(StepId::CloneComfyUi);
        let keep_current = matches!(self.run.request.comfyui_ref, ComfyRef::KeepCurrent);
        let checkout = deps.source.inspect_checkout(&root).await?;
        if self.run.request.mode == SetupMode::Rebuild && !matches!(checkout, Checkout::Repo { .. })
        {
            return Err(failed(
                "The existing ComfyUI checkout is missing or unsafe; rebuild cannot replace it.",
            ));
        }
        if self.run.request.mode == SetupMode::Rebuild {
            deps.installation.validate_rebuild_checkout(&root).await?;
        }
        let mut resolved = match self.run.resolved.comfyui.clone() {
            Some(resolved) => resolved,
            None if keep_current => match &checkout {
                Checkout::Repo { commit } => ResolvedGit {
                    url: String::new(),
                    reference: "current".into(),
                    branch: None,
                    commit: commit.clone(),
                },
                _ => return Err(failed("There is no existing ComfyUI checkout to keep.")),
            },
            None => {
                deps.source
                    .resolve(&root, &self.run.request.comfyui_ref)
                    .await?
            }
        };
        self.check_cancel()?;
        match checkout {
            Checkout::NotGit => {
                return Err(failed(
                    "The ComfyUI folder exists but is not a git checkout. It was left unchanged.",
                ));
            }
            Checkout::Repo { commit } if commit != resolved.commit => {
                return Err(failed(format!(
                    "The existing ComfyUI checkout is at {} but {} was requested. It was left \
                     unchanged; updating the checkout is a separate action.",
                    short(&commit),
                    short(&resolved.commit)
                )));
            }
            Checkout::Repo { .. } => {}
            Checkout::Absent => {
                deps.source.clone_comfyui(&root, &resolved, &ctx).await?;
                if let Checkout::Repo { commit } = deps.source.inspect_checkout(&root).await?
                    && commit != resolved.commit
                {
                    resolved.commit = commit;
                }
            }
        }
        self.run.resolved.comfyui = Some(resolved);
        self.save_run()
            .await
            .map_err(|_| failed("the run could not be saved"))?;
        Ok(StepOutcome::Done)
    }

    fn torch_present(&self, pins: &PinSet) -> bool {
        let profile = &self.run.profile;
        let version = |name: &str| {
            DistName::new(name)
                .ok()
                .and_then(|n| pins.get(&n).map(str::to_owned))
        };
        let prefix_ok = |name: &str, expected: &str| {
            version(name).is_some_and(|installed| {
                installed == expected || installed.starts_with(&format!("{expected}+"))
            })
        };
        let torch_ok = version("torch")
            .is_some_and(|installed| installed.starts_with(&profile.verify.torch_version_prefix));
        torch_ok
            && prefix_ok("torchvision", &profile.torch.torchvision)
            && profile
                .torch
                .torchaudio
                .as_deref()
                .is_none_or(|audio| prefix_ok("torchaudio", audio))
    }

    async fn installed_pins(&self) -> Result<PinSet, ProvisionError> {
        self.installed_pins_in(&self.ctx(StepId::ComfyRequirements))
            .await
    }

    async fn installed_pins_in(&self, ctx: &StepContext) -> Result<PinSet, ProvisionError> {
        let installed = self
            .deps
            .packages
            .list(&self.root(), &self.env_path(), Some(ctx))
            .await?;
        Ok(pins_of(
            &installed,
            &self.run.profile.protected_distributions(),
        ))
    }

    /// Stores the pins on the environment and rewrites the constraints file from them.
    async fn record_pins(&mut self, pins: PinSet) -> Result<(), ProvisionError> {
        let deps = self.deps.clone();
        let root = self.root();
        let extra = self.run.profile.constraints.clone();
        let now = deps.clock.now();
        let env = self.env_mut()?;
        env.record_pins(pins.clone(), &now);
        let id = env.id.clone();
        self.save_env().await?;
        self.constraints = Some(
            deps.packages
                .write_constraints(&root, &id, &pins, &extra)
                .await?,
        );
        Ok(())
    }

    fn constraint_file(&self) -> Result<PathBuf, ProvisionError> {
        self.constraints
            .clone()
            .ok_or_else(|| failed("the constraints file was not prepared"))
    }

    async fn save_components(&mut self) -> Result<(), ProvisionError> {
        self.save_run()
            .await
            .map_err(|_| failed("the component results could not be saved"))?;
        self.deps
            .envs
            .save_components(&self.run.target_env, &self.run.components)
            .await
            .map_err(storage)
    }

    /// Cleanup retains the maintenance lease and uses a fresh cancellation token.
    async fn reassert_pins(&self, ctx: &StepContext) -> Result<(), ProvisionError> {
        let expected = &self
            .env
            .as_ref()
            .ok_or_else(|| failed("the target environment was not prepared"))?
            .pins;
        if expected.is_empty() {
            return Ok(());
        }
        let drift = |error: ProvisionError| ProvisionError::PinDrift(describe(&error));
        let mut cleanup = ctx.clone();
        cleanup.cancel = CancelToken::new();
        let installed = self.installed_pins_in(&cleanup).await.map_err(drift)?;
        let changes: Vec<_> = expected
            .iter()
            .filter(|(name, version)| installed.get(name) != Some(*version))
            .collect();
        if changes.is_empty() {
            return Ok(());
        }
        for (name, version) in changes {
            cleanup.log(format!("Restoring protected package {name}=={version}."));
            let spec = InstallSpec {
                requirements: vec![format!("{name}=={version}")],
                index_url: self
                    .run
                    .profile
                    .torch
                    .protected_distributions()
                    .contains(name)
                    .then(|| self.run.profile.torch.index_url.clone()),
                constraint_file: Some(self.constraint_file().map_err(drift)?),
                no_deps: true,
                ..InstallSpec::default()
            };
            self.deps
                .packages
                .install(&self.root(), &self.env_path(), &spec, &cleanup)
                .await
                .map_err(|error| {
                    ProvisionError::PinDrift(format!("{name}: {}", describe(&error)))
                })?;
        }
        let restored = self.installed_pins_in(&cleanup).await.map_err(drift)?;
        let remaining: Vec<_> = expected
            .iter()
            .filter(|(name, version)| restored.get(name) != Some(*version))
            .map(|(name, _)| name.to_string())
            .collect();
        if !remaining.is_empty() {
            return Err(ProvisionError::PinDrift(remaining.join(", ")));
        }
        Ok(())
    }

    async fn guarded_install(
        &self,
        spec: &InstallSpec,
        ctx: &StepContext,
    ) -> Result<(), ProvisionError> {
        let result = self
            .deps
            .packages
            .install(&self.root(), &self.env_path(), spec, ctx)
            .await;
        self.reassert_pins(ctx).await?;
        result
    }

    async fn install_torch(&mut self) -> Result<StepOutcome, ProvisionError> {
        let ctx = self.ctx(StepId::InstallTorch);
        let mut pins = self.installed_pins().await?;
        if !self.torch_present(&pins) {
            let torch = &self.run.profile.torch;
            let spec = InstallSpec {
                requirements: torch.requirements(),
                index_url: Some(torch.index_url.clone()),
                allow_source_builds: false,
                ..InstallSpec::default()
            };
            self.guarded_install(&spec, &ctx).await?;
            pins = self.installed_pins().await?;
        }
        if !self.torch_present(&pins) {
            return Err(failed(
                "PyTorch was installed but the installed versions do not match the profile.",
            ));
        }
        self.record_pins(pins).await?;
        Ok(StepOutcome::Done)
    }

    async fn verify_torch(&mut self) -> Result<StepOutcome, ProvisionError> {
        let ctx = self.ctx(StepId::VerifyTorch);
        let report = self
            .deps
            .verifier
            .verify_torch(
                &self.root(),
                &self.env_path(),
                &self.run.profile.verify,
                &ctx,
            )
            .await?;
        ctx.log(format!(
            "torch {} (CUDA {})",
            report.version,
            if report.cuda_available {
                "available"
            } else {
                "unavailable"
            }
        ));
        Ok(StepOutcome::Done)
    }

    async fn triton(&mut self) -> Result<StepOutcome, ProvisionError> {
        let deps = self.deps.clone();
        let root = self.root();
        let ctx = self.ctx(StepId::Triton);
        if self.run.python_target.development_files_ok == Some(false) {
            return Err(failed(
                "The selected Python has no development headers or import library, which \
                 Triton needs. Choose a Python that includes them, or the managed Python.",
            ));
        }
        let spec = InstallSpec {
            requirements: vec![self.run.profile.triton.requirement()],
            constraint_file: Some(self.constraint_file()?),
            allow_source_builds: false,
            ..InstallSpec::default()
        };
        self.guarded_install(&spec, &ctx).await?;
        let pins = self.installed_pins().await?;
        if !pins.contains(&self.run.profile.triton.distribution) {
            return Err(failed("Triton was installed but could not be found."));
        }
        self.record_pins(pins).await?;
        let version = deps
            .verifier
            .verify_triton(&root, &self.env_path(), &ctx)
            .await?;
        ctx.log(format!("triton {version}"));
        Ok(StepOutcome::Done)
    }

    async fn accelerators(&mut self) -> Result<StepOutcome, ProvisionError> {
        let accelerators = self.run.profile.accelerators.clone();
        if self.run.request.accelerators.is_empty() {
            return Ok(StepOutcome::Skipped(
                "No accelerators were selected.".into(),
            ));
        }
        let ctx = self.ctx(StepId::Accelerators);
        let mut warnings = Vec::new();
        for accelerator in accelerators {
            if !self.run.request.accelerators.contains(&accelerator.id) {
                continue;
            }
            self.check_cancel()?;
            let result = self.install_accelerator(&accelerator, &ctx).await;
            let outcome = self
                .run
                .components
                .accelerators
                .iter_mut()
                .find(|r| r.id == accelerator.id)
                .ok_or_else(|| failed("the component selection was not recorded"))?;
            outcome.status = if result.is_ok() {
                ComponentStatus::Installed
            } else {
                ComponentStatus::Failed
            };
            outcome.importable = None;
            outcome.message = result.as_ref().err().map(describe);
            let saved = self.save_components().await;
            if let Err(error @ ProvisionError::PinDrift(_)) = result {
                return Err(error);
            }
            saved?;
            match result {
                Ok(()) => {}
                Err(error @ (ProvisionError::Cancelled | ProvisionError::PinDrift(_))) => {
                    return Err(error);
                }
                Err(error) if !accelerator.non_fatal => return Err(error),
                Err(error) => warnings.push(format!("{}: {}", accelerator.name, describe(&error))),
            }
        }
        if warnings.is_empty() {
            Ok(StepOutcome::Done)
        } else {
            Ok(StepOutcome::Warning(warnings.join("; ")))
        }
    }

    async fn install_accelerator(
        &self,
        accelerator: &domain::environment::AcceleratorSpec,
        ctx: &StepContext,
    ) -> Result<(), ProvisionError> {
        let requirements = match &accelerator.source {
            AcceleratorSource::Wheel { .. } => vec![
                self.deps
                    .wheels
                    .fetch(&self.root(), accelerator, ctx)
                    .await?
                    .to_str()
                    .ok_or_else(|| failed("the wheel path is not valid Unicode"))?
                    .to_owned(),
            ],
            AcceleratorSource::Packages { requirements } => requirements.clone(),
        };
        self.guarded_install(
            &InstallSpec {
                requirements,
                constraint_file: Some(self.constraint_file()?),
                no_deps: accelerator.no_deps,
                ..InstallSpec::default()
            },
            ctx,
        )
        .await
    }

    async fn accelerator_imports(
        &mut self,
        ctx: &StepContext,
    ) -> Result<Vec<String>, ProvisionError> {
        let mut warnings = Vec::new();
        for spec in self.run.profile.accelerators.clone() {
            let Some(index) = self.run.components.accelerators.iter().position(|r| {
                r.id == spec.id && r.selected && r.status == ComponentStatus::Installed
            }) else {
                continue;
            };
            if spec.modules.is_empty() {
                self.run.components.accelerators[index].message =
                    Some(format!("Import untested: {}", spec.provenance));
                self.save_components().await?;
                continue;
            }
            let mut failures = Vec::new();
            for module in spec.modules {
                match self
                    .deps
                    .verifier
                    .verify_accelerator(&self.root(), &self.env_path(), &module, ctx)
                    .await
                {
                    Ok(()) => {}
                    Err(ProvisionError::Cancelled) => return Err(ProvisionError::Cancelled),
                    Err(error) => failures.push(format!("{module}: {}", describe(&error))),
                }
                // Imported third-party code also runs under the original pin guard.
                self.reassert_pins(ctx).await?;
            }
            let result = &mut self.run.components.accelerators[index];
            result.importable = Some(failures.is_empty());
            result.message = if failures.is_empty() {
                None
            } else {
                Some(failures.join("; "))
            };
            if let Some(message) = &result.message {
                warnings.push(message.clone());
            }
            self.save_components().await?;
        }
        Ok(warnings)
    }

    async fn comfy_requirements(&mut self) -> Result<StepOutcome, ProvisionError> {
        let deps = self.deps.clone();
        let root = self.root();
        let ctx = self.ctx(StepId::ComfyRequirements);
        let files = deps.source.requirement_files(&root).await?;
        let spec = InstallSpec {
            requirement_files: vec![files.comfyui],
            constraint_file: Some(self.constraint_file()?),
            allow_source_builds: ctx.allow_source_builds,
            ..InstallSpec::default()
        };
        self.guarded_install(&spec, &ctx).await?;
        for set in deps
            .node_requirements
            .requirement_sets(&root, ctx.retained.clone())
            .await?
        {
            self.check_cancel()?;
            let spec = InstallSpec {
                requirements: set.extra.clone(),
                requirement_files: set.file.iter().map(|file| file.path.clone()).collect(),
                guarded: set.file.clone(),
                constraint_file: Some(self.constraint_file()?),
                allow_source_builds: ctx.allow_source_builds,
                ..InstallSpec::default()
            };
            let installed = self.guarded_install(&spec, &ctx).await;
            let state = if installed.is_ok() {
                domain::custom_nodes::DependencyState::Ok
            } else {
                domain::custom_nodes::DependencyState::Failed
            };
            let recorded = deps
                .node_requirements
                .record_outcome(&self.run.target_env, &set, state)
                .await;
            installed?;
            recorded?;
        }
        Ok(StepOutcome::Done)
    }

    async fn manager(&mut self) -> Result<StepOutcome, ProvisionError> {
        if !self.run.request.install_manager {
            return Ok(StepOutcome::Skipped(
                "ComfyUI-Manager was not requested.".into(),
            ));
        }
        let deps = self.deps.clone();
        let root = self.root();
        let ctx = self.ctx(StepId::Manager);
        let resolved = match self.run.resolved.manager.clone() {
            Some(resolved) => resolved,
            None => {
                let resolved = deps.source.resolve_manager(&root).await?;
                self.run.resolved.manager = Some(resolved.clone());
                self.save_run()
                    .await
                    .map_err(|_| failed("the run could not be saved"))?;
                resolved
            }
        };
        if self.run.manager_consent.as_deref() != Some(resolved.commit.as_str()) {
            if !self.run.request.third_party_consent {
                return Err(ProvisionError::ConsentRequired(
                    super::model::ConsentRequest {
                        source: resolved.url.clone(),
                        commit: resolved.commit.clone(),
                    },
                ));
            }
            self.run.manager_consent = Some(resolved.commit.clone());
            self.save_run()
                .await
                .map_err(|_| failed("the run could not be saved"))?;
            ctx.log(format!(
                "ComfyUI-Manager {} at commit {} was approved on the setup review page.",
                resolved.url, resolved.commit
            ));
        }
        let requirement_files = deps.source.install_manager(&root, &resolved, &ctx).await?;
        for file in requirement_files {
            self.check_cancel()?;
            let spec = InstallSpec {
                requirement_files: vec![file],
                constraint_file: Some(self.constraint_file()?),
                allow_source_builds: ctx.allow_source_builds,
                ..InstallSpec::default()
            };
            self.guarded_install(&spec, &ctx).await?;
        }
        Ok(StepOutcome::Done)
    }

    /// Installs the selected curated nodes into the candidate environment through the node
    /// workflow, before the final verification and the activation. Every node is prepared at the
    /// latest commit of its repository's default branch. Then the run pauses for the user's
    /// exact consent to each reviewed plan, unless the request carries the review-stage consent:
    /// that consent is turned into one decision per plan, and the plans are committed one after
    /// another.
    async fn curated_nodes(
        &mut self,
        lease: &mut dyn MaintenanceLease,
    ) -> Result<StepOutcome, ProvisionError> {
        if self.run.curated.is_empty() {
            return Ok(StepOutcome::Skipped(
                "No curated custom nodes were selected.".into(),
            ));
        }
        let ctx = self.ctx(StepId::CuratedNodes);
        let catalog = self.deps.curated.list();
        let nodes = self.nodes.clone();
        let mut decisions = self.deps.active_runs.take_decisions(&self.run.id);
        let auto = self.run.request.third_party_consent;
        let mut passes = 0;
        loop {
            if auto && passes >= MAX_CURATED_PASSES {
                return Err(failed("The curated node plans kept changing. Retry setup."));
            }
            passes += 1;
            self.prepare_curated(&catalog, &ctx, lease).await?;
            let prepared: Vec<usize> = (0..self.run.curated.len())
                .filter(|&index| self.run.curated[index].status == CuratedStatus::Prepared)
                .collect();
            if prepared.is_empty() {
                break;
            }
            let reviews = nodes
                .pending(&self.run.id)
                .map_err(|error| self.node_failure(error))?;
            if auto {
                // Review-page consent answers each current review, including its current hook.
                for &index in &prepared {
                    let Some(review) = reviews
                        .iter()
                        .find(|review| Some(&review.id) == self.run.curated[index].plan.as_ref())
                    else {
                        continue;
                    };
                    if !review.protected_changes.is_empty() {
                        continue;
                    }
                    decisions.retain(|decision| decision.consent.plan_id != review.id);
                    for warning in &review.warnings {
                        ctx.log(format!(
                            "{}: {warning} (acknowledged on the setup review page).",
                            review.name
                        ));
                    }
                    decisions.push(setup_review_decision(review));
                }
            }
            // Nothing is installed until every reviewed plan has its own exact decision.
            let covered = |plan: &Option<String>| {
                reviews
                    .iter()
                    .find(|review| Some(&review.id) == plan.as_ref())
                    .is_some_and(|review| decisions.iter().any(|decision| covers(decision, review)))
            };
            if prepared
                .iter()
                .any(|&index| !covered(&self.run.curated[index].plan))
            {
                return Err(curated_review_pause());
            }
            let mut refresh = false;
            for index in prepared {
                self.check_cancel()?;
                let Some(plan) = self.run.curated[index].plan.clone() else {
                    continue;
                };
                let Some(decision) = decisions
                    .iter()
                    .find(|decision| decision.consent.plan_id == plan)
                    .cloned()
                else {
                    continue;
                };
                if let Some(review) = reviews.iter().find(|review| review.id == plan) {
                    if let NodeSource::Git {
                        repository, commit, ..
                    } = &review.source
                    {
                        ctx.log(format!(
                            "{}: installing from {repository} at commit {commit}.",
                            review.name
                        ));
                    }
                    ctx.log(format!("Installing {}.", review.name));
                }
                match nodes
                    .commit(&self.run.id, &plan, decision, &ctx, lease)
                    .await
                {
                    Ok(outcome) => {
                        let entry = &mut self.run.curated[index];
                        entry.status = CuratedStatus::Installed;
                        entry.plan = None;
                        entry.operation = Some(outcome.operation.as_str().to_owned());
                        entry.message = curated_hook_note(entry.message.as_deref());
                        // The journal operation is on record before the next node starts.
                        self.save_curated().await?;
                    }
                    Err(SetupNodeError::PlanStale) => {
                        let entry = &mut self.run.curated[index];
                        entry.status = CuratedStatus::Pending;
                        entry.plan = None;
                        entry.message =
                            Some("The reviewed plan changed. Review the new plan.".into());
                        self.save_curated().await?;
                        refresh = true;
                        break;
                    }
                    Err(SetupNodeError::ConsentRequired) => return Err(curated_review_pause()),
                    Err(error) => {
                        return Err(self.curated_failure(index, error).await);
                    }
                }
            }
            // A refreshed plan has no decision yet, so the next pass pauses for consent.
            if !refresh {
                break;
            }
        }
        let hooks: Vec<_> = self
            .run
            .curated
            .iter()
            .filter(|entry| entry.status == CuratedStatus::Installed)
            .filter_map(|entry| curated_hook_note(entry.message.as_deref()))
            .collect();
        if hooks.is_empty() {
            Ok(StepOutcome::Done)
        } else {
            Ok(StepOutcome::Warning(hooks.join("\n")))
        }
    }

    /// Prepares every node that has no live reviewed plan. A node that is already installed from
    /// its repository with successful candidate dependencies is recognised, not fetched again.
    async fn prepare_curated(
        &mut self,
        catalog: &[CuratedNodeSpec],
        ctx: &StepContext,
        lease: &mut dyn MaintenanceLease,
    ) -> Result<(), ProvisionError> {
        let nodes = self.nodes.clone();
        let live: Vec<String> = nodes
            .pending(&self.run.id)
            .map_err(|error| self.node_failure(error))?
            .into_iter()
            .map(|review| review.id)
            .collect();
        for index in 0..self.run.curated.len() {
            let entry = &self.run.curated[index];
            let current = match entry.status {
                CuratedStatus::Installed | CuratedStatus::Skipped => continue,
                CuratedStatus::Prepared => {
                    entry.plan.as_ref().is_some_and(|plan| live.contains(plan))
                }
                CuratedStatus::Pending | CuratedStatus::Failed => false,
            };
            let published = entry.status == CuratedStatus::Failed
                || entry.operation.is_some()
                || if let Some(plan) = &entry.plan {
                    nodes.publication_started(plan).await
                } else {
                    false
                };
            if current && !published {
                continue;
            }
            let spec = catalog
                .iter()
                .find(|spec| spec.id == entry.id)
                .cloned()
                .ok_or_else(|| failed("A selected curated node is no longer in the catalog."))?;
            self.check_cancel()?;
            let recognised = match nodes.installed(&self.run.id, &spec, lease).await {
                Ok(recognised) => recognised,
                Err(error) => {
                    let error = if published {
                        SetupNodeError::Damaged(error.to_string())
                    } else {
                        error
                    };
                    return Err(self.curated_failure(index, error).await);
                }
            };
            if recognised {
                ctx.log(format!("{} is already installed.", spec.name));
                let entry = &mut self.run.curated[index];
                entry.status = CuratedStatus::Installed;
                entry.plan = None;
                entry.message = curated_hook_note(entry.message.as_deref())
                    .or_else(|| Some("Already installed.".into()));
                self.save_curated().await?;
                continue;
            }
            if published {
                return Err(self
                    .curated_failure(
                        index,
                        SetupNodeError::Damaged(CURATED_DAMAGE_MESSAGE.into()),
                    )
                    .await);
            }
            ctx.log(format!(
                "Preparing {} from the latest commit of its default branch.",
                spec.name
            ));
            let review = match nodes.prepare(&self.run.id, &spec, ctx, lease).await {
                Ok(review) => review,
                Err(error) => {
                    return Err(self.curated_failure(index, error).await);
                }
            };
            if let NodeSource::Git {
                repository,
                commit,
                branch,
            } = &review.source
            {
                ctx.log(format!(
                    "{}: {} from {} at commit {}.",
                    spec.name,
                    branch.as_deref().unwrap_or("default branch"),
                    repository,
                    commit
                ));
            }
            let entry = &mut self.run.curated[index];
            entry.status = CuratedStatus::Prepared;
            entry.plan = Some(review.id);
            entry.message = review
                .warnings
                .iter()
                .any(|warning| warning == SETUP_HOOK_WARNING)
                .then(|| format!("{}: {SETUP_HOOK_WARNING}", review.name));
            // The plan is also the journal operation, so it is on record before the next node.
            self.save_curated().await?;
        }
        Ok(())
    }

    /// Final activation requires every published curated node to still match its install and to
    /// have successful dependency outcomes for this candidate, with no unresolved failure.
    async fn verify_curated(
        &mut self,
        lease: &mut dyn MaintenanceLease,
    ) -> Result<(), ProvisionError> {
        let installed: Vec<usize> = self
            .run
            .curated
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                matches!(
                    entry.status,
                    CuratedStatus::Installed | CuratedStatus::Failed
                )
            })
            .map(|(index, _)| index)
            .collect();
        if installed.is_empty() {
            return Ok(());
        }
        let catalog = self.deps.curated.list();
        let nodes = self.nodes.clone();
        for index in installed {
            let entry = &self.run.curated[index];
            if entry.status == CuratedStatus::Failed {
                return Err(self
                    .curated_failure(
                        index,
                        SetupNodeError::Damaged(CURATED_DAMAGE_MESSAGE.into()),
                    )
                    .await);
            }
            let spec = catalog
                .iter()
                .find(|spec| spec.id == entry.id)
                .ok_or_else(|| failed("A selected curated node is no longer in the catalog."))?;
            let intact = match nodes.installed(&self.run.id, spec, lease).await {
                Ok(intact) => intact,
                Err(error) => {
                    return Err(self
                        .curated_failure(index, SetupNodeError::Damaged(error.to_string()))
                        .await);
                }
            };
            if !intact {
                return Err(self
                    .curated_failure(
                        index,
                        SetupNodeError::Damaged(CURATED_DAMAGE_MESSAGE.into()),
                    )
                    .await);
            }
        }
        Ok(())
    }

    async fn save_curated(&mut self) -> Result<(), ProvisionError> {
        self.save_run()
            .await
            .map_err(|_| failed("the curated node progress could not be saved"))
    }

    async fn curated_failure(&mut self, index: usize, error: SetupNodeError) -> ProvisionError {
        let entry = &mut self.run.curated[index];
        if matches!(
            error,
            SetupNodeError::Damaged(_) | SetupNodeError::PinDrift(_)
        ) {
            self.damaged = true;
            entry.status = CuratedStatus::Failed;
            entry.operation = entry.operation.take().or_else(|| entry.plan.take());
            entry.plan = None;
            entry.message = Some(match curated_hook_note(entry.message.as_deref()) {
                Some(note) => format!("{CURATED_DAMAGE_MESSAGE}\n{note}"),
                None => CURATED_DAMAGE_MESSAGE.into(),
            });
        } else {
            entry.message = Some(match curated_hook_note(entry.message.as_deref()) {
                Some(note) => format!("{error}\n{note}"),
                None => error.to_string(),
            });
        }
        if let Err(error) = self.save_curated().await {
            return error;
        }
        self.node_failure(error)
    }

    /// Maps a node-service stop to the setup failure. Damage to the candidate cannot be skipped.
    fn node_failure(&mut self, error: SetupNodeError) -> ProvisionError {
        match error {
            SetupNodeError::Cancelled => ProvisionError::Cancelled,
            SetupNodeError::PinDrift(message) => ProvisionError::PinDrift(message),
            SetupNodeError::ConsentRequired => curated_review_pause(),
            SetupNodeError::Damaged(message) => {
                self.damaged = true;
                failed(message)
            }
            SetupNodeError::NotApplied(message) => failed(message),
            SetupNodeError::PlanStale => failed("The reviewed plan changed. Retry to review it."),
        }
    }

    async fn final_verify(
        &mut self,
        lease: &mut dyn MaintenanceLease,
    ) -> Result<StepOutcome, ProvisionError> {
        let deps = self.deps.clone();
        let root = self.root();
        let ctx = self.ctx(StepId::FinalVerify);
        if self.run.request.mode == SetupMode::Rebuild {
            deps.relocation_runtime
                .ensure_stopped(&root)
                .await
                .map_err(|error| failed(error.to_string()))?;
            if let super::ManifestState::Valid(manifest) = deps
                .manifests
                .read(&root)
                .await
                .map_err(|error| failed(error.to_string()))?
            {
                deps.relocation_runtime
                    .ensure_stopped(&manifest.install_root)
                    .await
                    .map_err(|error| failed(error.to_string()))?;
            }
        }
        self.verify_curated(lease).await?;
        let pins = self
            .installed_pins()
            .await
            .map_err(|error| ProvisionError::PinDrift(describe(&error)))?;
        let expected = &self
            .env
            .as_ref()
            .ok_or_else(|| failed("the target environment was not prepared"))?
            .pins;
        if expected.is_empty() || !expected.diff(&pins).is_empty() {
            return Err(ProvisionError::PinDrift(
                "final verification differs from the recorded protected pins".into(),
            ));
        }
        let warnings = self.accelerator_imports(&ctx).await?;
        let report = deps
            .verifier
            .verify_final(&root, &self.env_path(), &self.run.profile.verify, &ctx)
            .await?;
        let after = self
            .installed_pins()
            .await
            .map_err(|error| ProvisionError::PinDrift(describe(&error)))?;
        if !pins.diff(&after).is_empty() {
            return Err(ProvisionError::PinDrift(
                "protected versions changed during final verification".into(),
            ));
        }
        let now = deps.clock.now();
        let summary = report.summary();
        let env = self.env_mut()?;
        match env.state() {
            EnvState::Provisioning => env
                .mark_verified(pins, summary, &now)
                .map_err(|error| failed(error.to_string()))?,
            _ => {
                env.pins = pins;
                env.verification = Some(summary);
            }
        }
        self.save_env().await?;
        let candidate = self
            .env
            .clone()
            .ok_or_else(|| failed("the target environment was not prepared"))?;
        match activation::activate(&deps, &mut self.run, &candidate).await {
            Ok(()) if warnings.is_empty() => Ok(StepOutcome::Done),
            Ok(()) => Ok(StepOutcome::Warning(warnings.join("; "))),
            Err(EnvironmentAppError::RecoveryRequired) => Err(failed(
                "install.json is unreadable or newer than this app; setup did not replace it.",
            )),
            Err(error) => Err(failed(format!(
                "Activation could not be completed ({error}). It resumes on retry or restart."
            ))),
        }
    }

    async fn cache_clean(&mut self) -> Result<StepOutcome, ProvisionError> {
        if !self.run.request.clean_cache {
            return Ok(StepOutcome::Skipped(
                "Cache cleaning was not requested.".into(),
            ));
        }
        let ctx = self.ctx(StepId::CacheClean);
        self.deps.packages.clean_cache(&self.root(), &ctx).await?;
        Ok(StepOutcome::Done)
    }
}

/// Whether the decision answers exactly this review: its plan and binding, the acknowledgement
/// that node code runs, every warning acknowledged and no protected package change.
fn covers(decision: &CommitDecision, review: &PlanReview) -> bool {
    let consent = &decision.consent;
    consent.plan_id == review.id
        && consent.binding == review.binding
        && consent.execute_node_code
        && (review.warnings.is_empty() || consent.acknowledge_warnings)
        && decision.protected_changes.is_empty()
}

/// The decision a review-page consent stands for: node code runs and the plan's warnings are
/// acknowledged and its hook runs if present. Source builds, remembered trust and protected
/// package changes stay refused. It is bound to the current plan.
fn setup_review_decision(review: &PlanReview) -> CommitDecision {
    CommitDecision {
        consent: InstallConsent {
            plan_id: review.id.clone(),
            binding: review.binding.clone(),
            execute_node_code: true,
            run_install_py: review.has_install_py,
            allow_source_builds: false,
            acknowledge_warnings: true,
            remember_source: false,
            remember_install_py: false,
        },
        protected_changes: Vec::new(),
    }
}

/// The run waits for the user to answer the curated node reviews.
fn curated_review_pause() -> ProvisionError {
    ProvisionError::ConsentRequired(ConsentRequest {
        source: String::new(),
        commit: String::new(),
    })
}

fn pins_of(installed: &[super::model::InstalledPackage], protected: &[DistName]) -> PinSet {
    let named: Vec<(DistName, String)> = installed
        .iter()
        .filter_map(|package| {
            DistName::new(&package.name)
                .ok()
                .map(|name| (name, package.version.clone()))
        })
        .collect();
    PinSet::from_installed(
        named.iter().map(|(name, version)| (name, version.as_str())),
        protected,
    )
}

fn short(commit: &str) -> &str {
    commit.get(..10).unwrap_or(commit)
}

pub(crate) fn describe(error: &ProvisionError) -> String {
    match error {
        ProvisionError::Cancelled => "The operation was cancelled.".into(),
        ProvisionError::CommandFailed(failure) => match failure.exit_code {
            Some(code) => format!("A command failed with exit code {code}."),
            None => "A command failed.".into(),
        },
        ProvisionError::PinDrift(message) => format!("Protected package pin drift: {message}"),
        ProvisionError::Retryable(message) | ProvisionError::Failed(message) => message.clone(),
        ProvisionError::ConsentRequired(_) => "Explicit consent is required.".into(),
    }
}

fn to_failure(step: StepId, error: ProvisionError) -> StepFailure {
    let message = describe(&error);
    let mut failure = StepFailure {
        step,
        kind: FailureKind::Failed,
        message,
        command: None,
        exit_code: None,
        tail: Vec::new(),
        consent: None,
        can_skip: step.policy() == StepPolicy::Optional
            && !matches!(error, ProvisionError::PinDrift(_)),
    };
    match error {
        ProvisionError::CommandFailed(command) => {
            failure.kind = FailureKind::CommandFailed;
            failure.command = Some(command.command);
            failure.exit_code = command.exit_code;
            failure.tail = command.tail;
        }
        ProvisionError::Retryable(_) => failure.kind = FailureKind::Retryable,
        ProvisionError::ConsentRequired(_) if step == StepId::CuratedNodes => {
            failure.kind = FailureKind::ConsentRequired;
            failure.message =
                "Review the curated custom nodes and confirm each one to continue.".into();
        }
        ProvisionError::ConsentRequired(consent) => {
            failure.kind = FailureKind::ConsentRequired;
            failure.message = format!(
                "Installing ComfyUI-Manager runs third-party code from {} at commit {}. \
                 Approve it to continue.",
                consent.source,
                short(&consent.commit)
            );
            failure.consent = Some(consent);
        }
        ProvisionError::Failed(_) | ProvisionError::Cancelled | ProvisionError::PinDrift(_) => {}
    }
    failure
}
