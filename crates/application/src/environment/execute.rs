use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use domain::environment::{
    DistName, Eligibility, EnvState, NewEnvironment, PinSet, PythonEnvironment, SetupMode, evaluate,
};

use super::{
    activation,
    deps::EnvironmentDeps,
    error::EnvironmentAppError,
    model::{
        ComfyRef, FailureKind, ProvisioningRun, ResolvedGit, RunId, RunStatus, StepFailure, StepId,
        StepPolicy, StepStatus,
    },
    ports::{
        Checkout, EnvRepoError, InstallSpec, ProvisionError, SetupEvent, SetupProgressSink,
        StepContext,
    },
};
use crate::{comfy_runtime::RuntimeBlock, shared::CancelToken};

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

/// Runs a registered setup run to completion, failure or cancellation. The maintenance gate is
/// acquired once and held until the run finished and all cleanup is done.
pub struct ExecuteSetup {
    deps: EnvironmentDeps,
}

impl ExecuteSetup {
    pub fn new(deps: EnvironmentDeps) -> Self {
        Self { deps }
    }

    pub async fn execute(
        &self,
        run_id: &RunId,
        sink: Arc<dyn SetupProgressSink>,
    ) -> Result<RunOutcome, EnvironmentAppError> {
        let deps = &self.deps;
        let cancel = deps
            .active_runs
            .get(run_id)
            .unwrap_or_else(|| deps.active_runs.register(run_id));
        let lease = match deps.gate.acquire().await {
            Ok(lease) => lease,
            Err(error) => {
                deps.active_runs.remove(run_id);
                return Err(error.into());
            }
        };
        let result = self.run_locked(run_id, sink, cancel).await;
        deps.active_runs.remove(run_id);
        drop(lease);
        result
    }

    async fn run_locked(
        &self,
        run_id: &RunId,
        sink: Arc<dyn SetupProgressSink>,
        cancel: CancelToken,
    ) -> Result<RunOutcome, EnvironmentAppError> {
        let run = self
            .deps
            .runs
            .get(run_id)
            .await?
            .ok_or(EnvironmentAppError::RunNotFound)?;
        if run.status != RunStatus::Registered {
            return Err(EnvironmentAppError::NotRunnable);
        }
        Execution {
            deps: self.deps.clone(),
            run,
            env: None,
            constraints: None,
            sink,
            cancel,
        }
        .drive()
        .await
    }
}

struct Execution {
    deps: EnvironmentDeps,
    run: ProvisioningRun,
    env: Option<PythonEnvironment>,
    constraints: Option<PathBuf>,
    sink: Arc<dyn SetupProgressSink>,
    cancel: CancelToken,
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

    async fn drive(mut self) -> Result<RunOutcome, EnvironmentAppError> {
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
            match self.run_step(step).await {
                Ok(StepOutcome::Done) => self.set_step(step, StepStatus::Succeeded, None),
                Ok(StepOutcome::Skipped(reason)) => {
                    self.set_step(step, StepStatus::Skipped, Some(reason))
                }
                Ok(StepOutcome::Warning(message)) => {
                    self.set_step(step, StepStatus::Warning, Some(message))
                }
                Ok(StepOutcome::NothingToRepair) => return self.finish_nothing_to_repair().await,
                Err(ProvisionError::Cancelled) => return self.finish_cancelled().await,
                Err(error) if step.policy() == StepPolicy::BestEffort => {
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
        let failure = to_failure(step, error);
        self.set_step(step, StepStatus::Failed, Some(failure.message.clone()));
        self.run.failure = Some(failure.clone());
        if step.policy() == StepPolicy::Required {
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

    async fn run_step(&mut self, step: StepId) -> Result<StepOutcome, ProvisionError> {
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
            StepId::Manager => self.manager().await,
            StepId::CuratedNodes => Ok(StepOutcome::Skipped(
                "Curated custom nodes arrive in a later milestone.".into(),
            )),
            StepId::FinalVerify => self.final_verify().await,
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
        let Some(mut active) = deps.envs.active().await.map_err(storage)? else {
            return Ok(false);
        };
        let ctx = self.ctx(StepId::Preflight);
        match deps
            .verifier
            .verify_final(&root, &active.path, &self.run.profile.verify, &ctx)
            .await
        {
            Err(ProvisionError::Cancelled) => Err(ProvisionError::Cancelled),
            Err(_) => Ok(false),
            Ok(report) => {
                let installed = deps
                    .packages
                    .list(&root, &active.path)
                    .await
                    .map_err(|error| failed(describe(&error)))?;
                let pins = pins_of(&installed, &self.run.profile.protected_distributions());
                if pins != active.pins {
                    return Ok(false);
                }
                let now = deps.clock.now();
                match active.state() {
                    EnvState::RepairRequired => active
                        .mark_repaired(pins, report.summary(), &now)
                        .map_err(|error| failed(error.to_string()))?,
                    EnvState::Verified => {}
                    _ => return Ok(false),
                }
                deps.envs.save(&active).await.map_err(storage)?;
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
        self.env = Some(env);
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
        deps.tools.ensure_hf_tool(&root, &ctx).await?;
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
        if !info.version.same_minor(profile_python) {
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
        self.deps
            .python
            .seed(&self.root(), &self.env_path(), self.run.profile.os, &ctx)
            .await?;
        Ok(StepOutcome::Done)
    }

    async fn clone_comfyui(&mut self) -> Result<StepOutcome, ProvisionError> {
        let deps = self.deps.clone();
        let root = self.root();
        let ctx = self.ctx(StepId::CloneComfyUi);
        let keep_current = matches!(self.run.request.comfyui_ref, ComfyRef::KeepCurrent);
        let checkout = deps.source.inspect_checkout(&root).await?;
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
        let installed = self
            .deps
            .packages
            .list(&self.root(), &self.env_path())
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

    /// The pin guard: protected versions must be exactly what was recorded.
    async fn reassert_pins(&self) -> Result<(), ProvisionError> {
        let now = self.installed_pins().await?;
        let expected = self
            .env
            .as_ref()
            .map(|env| env.pins.clone())
            .unwrap_or_default();
        let changes = expected.diff(&now);
        if changes.is_empty() {
            return Ok(());
        }
        let names: Vec<String> = changes
            .iter()
            .map(|change| change.name.to_string())
            .collect();
        Err(failed(format!(
            "Protected packages changed during an install: {}.",
            names.join(", ")
        )))
    }

    async fn install_torch(&mut self) -> Result<StepOutcome, ProvisionError> {
        let deps = self.deps.clone();
        let root = self.root();
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
            deps.packages
                .install(&root, &self.env_path(), &spec, &ctx)
                .await?;
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
        deps.packages
            .install(&root, &self.env_path(), &spec, &ctx)
            .await?;
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
        if accelerators.is_empty() {
            return Ok(StepOutcome::Skipped(
                "This profile defines no accelerators yet.".into(),
            ));
        }
        let deps = self.deps.clone();
        let root = self.root();
        let ctx = self.ctx(StepId::Accelerators);
        let mut warnings = Vec::new();
        for accelerator in accelerators {
            self.check_cancel()?;
            let spec = InstallSpec {
                requirements: vec![accelerator.requirement.clone()],
                constraint_file: Some(self.constraint_file()?),
                allow_source_builds: false,
                ..InstallSpec::default()
            };
            match deps
                .packages
                .install(&root, &self.env_path(), &spec, &ctx)
                .await
            {
                Ok(()) => self.reassert_pins().await?,
                Err(ProvisionError::Cancelled) => return Err(ProvisionError::Cancelled),
                Err(error) => warnings.push(format!("{}: {}", accelerator.name, describe(&error))),
            }
        }
        if warnings.is_empty() {
            Ok(StepOutcome::Done)
        } else {
            Ok(StepOutcome::Warning(warnings.join("; ")))
        }
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
        deps.packages
            .install(&root, &self.env_path(), &spec, &ctx)
            .await?;
        self.reassert_pins().await?;
        // Existing custom nodes keep working in a replacement environment: their requirements are
        // installed under the same pin guard. Failures are warnings for the affected node.
        let mut warnings = Vec::new();
        for file in files.nodes {
            self.check_cancel()?;
            let spec = InstallSpec {
                requirement_files: vec![file.clone()],
                constraint_file: Some(self.constraint_file()?),
                allow_source_builds: ctx.allow_source_builds,
                ..InstallSpec::default()
            };
            match deps
                .packages
                .install(&root, &self.env_path(), &spec, &ctx)
                .await
            {
                Ok(()) => self.reassert_pins().await?,
                Err(ProvisionError::Cancelled) => return Err(ProvisionError::Cancelled),
                Err(error) => warnings.push(format!("{}: {}", node_label(&file), describe(&error))),
            }
        }
        if warnings.is_empty() {
            Ok(StepOutcome::Done)
        } else {
            Ok(StepOutcome::Warning(format!(
                "Some custom node requirements could not be installed: {}",
                warnings.join("; ")
            )))
        }
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
            return Err(ProvisionError::ConsentRequired(
                super::model::ConsentRequest {
                    source: resolved.url.clone(),
                    commit: resolved.commit.clone(),
                },
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
            deps.packages
                .install(&root, &self.env_path(), &spec, &ctx)
                .await?;
            self.reassert_pins().await?;
        }
        Ok(StepOutcome::Done)
    }

    async fn final_verify(&mut self) -> Result<StepOutcome, ProvisionError> {
        let deps = self.deps.clone();
        let root = self.root();
        let ctx = self.ctx(StepId::FinalVerify);
        let pins = self.installed_pins().await?;
        self.record_pins(pins.clone()).await?;
        let report = deps
            .verifier
            .verify_final(&root, &self.env_path(), &self.run.profile.verify, &ctx)
            .await?;
        for (name, present) in &report.accelerators {
            ctx.log(format!(
                "{name}: {}",
                if *present { "present" } else { "not installed" }
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
            Ok(()) => Ok(StepOutcome::Done),
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

fn node_label(requirements: &Path) -> String {
    requirements.parent().and_then(Path::file_name).map_or_else(
        || "custom node".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

pub(crate) fn describe(error: &ProvisionError) -> String {
    match error {
        ProvisionError::Cancelled => "The operation was cancelled.".into(),
        ProvisionError::CommandFailed(failure) => match failure.exit_code {
            Some(code) => format!("A command failed with exit code {code}."),
            None => "A command failed.".into(),
        },
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
    };
    match error {
        ProvisionError::CommandFailed(command) => {
            failure.kind = FailureKind::CommandFailed;
            failure.command = Some(command.command);
            failure.exit_code = command.exit_code;
            failure.tail = command.tail;
        }
        ProvisionError::Retryable(_) => failure.kind = FailureKind::Retryable,
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
        ProvisionError::Failed(_) | ProvisionError::Cancelled => {}
    }
    failure
}
