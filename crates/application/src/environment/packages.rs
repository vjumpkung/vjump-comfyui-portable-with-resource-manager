use std::sync::Arc;

use domain::environment::{DistName, EnvId, EnvState, PinChange, PinSet, PythonEnvironment};

use super::{
    deps::EnvironmentDeps,
    error::EnvironmentAppError,
    execute::describe,
    model::{InstalledPackage, RunId, StepId},
    ports::{InstallSpec, NullSink, ProvisionError, ResolutionPreview, StepContext},
};
use crate::shared::CancelToken;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlanId(String);

impl PlanId {
    pub fn new(value: impl Into<String>) -> Result<Self, EnvironmentAppError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
        if valid {
            Ok(Self(value))
        } else {
            Err(EnvironmentAppError::PlanNotFound)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageOperation {
    Install(Vec<String>),
    Upgrade(Vec<String>),
    Uninstall(Vec<String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanResolution {
    /// Resolved with the pin guard in force; no protected package changes.
    Constrained,
    /// Resolution only succeeds if protected packages change; the user must confirm exactly that.
    Unconstrained,
    /// An uninstall; no resolver runs.
    Removal,
}

/// A resolved, not yet applied package change. Confirmation is bound to this plan's identity,
/// the environment's pins at preview time and the exact protected changes it lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackagePlan {
    pub id: PlanId,
    pub env_id: EnvId,
    pub operation: PackageOperation,
    pub current_pins: PinSet,
    pub resolution: PlanResolution,
    pub installs: Vec<InstalledPackage>,
    pub removals: Vec<InstalledPackage>,
    pub protected_changes: Vec<PinChange>,
}

impl PackagePlan {
    pub fn requires_confirmation(&self) -> bool {
        !self.protected_changes.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyOutcome {
    pub env_state: EnvState,
    pub protected_changes: Vec<PinChange>,
}

const CORE_DISTRIBUTIONS: [&str; 4] = ["torch", "torchvision", "triton", "triton-windows"];

fn validate(operation: &PackageOperation) -> Result<(), EnvironmentAppError> {
    let (specs, names_only) = match operation {
        PackageOperation::Install(specs) => (specs, false),
        PackageOperation::Upgrade(specs) | PackageOperation::Uninstall(specs) => (specs, true),
    };
    if specs.is_empty() || specs.len() > 50 {
        return Err(EnvironmentAppError::PackageSpecInvalid);
    }
    for spec in specs {
        let spec = spec.trim();
        let bad = spec.is_empty()
            || spec.len() > 200
            || spec.starts_with('-')
            || spec.chars().any(char::is_control);
        if bad || (names_only && DistName::new(spec).is_err()) {
            return Err(EnvironmentAppError::PackageSpecInvalid);
        }
    }
    Ok(())
}

fn names_of(specs: &[String]) -> Result<Vec<DistName>, EnvironmentAppError> {
    specs
        .iter()
        .map(|spec| DistName::new(spec.trim()).map_err(|_| EnvironmentAppError::PackageSpecInvalid))
        .collect()
}

fn protected_names(env: &PythonEnvironment) -> Vec<DistName> {
    env.pins.iter().map(|(name, _)| name.clone()).collect()
}

fn pins_of(installed: &[InstalledPackage], protected: &[DistName]) -> PinSet {
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

/// The protected changes a resolver preview would cause.
fn preview_protected_changes(pins: &PinSet, preview: &ResolutionPreview) -> Vec<PinChange> {
    let protected: Vec<DistName> = pins.iter().map(|(name, _)| name.clone()).collect();
    let mut after = pins.clone();
    for removal in &preview.removals {
        if let Ok(name) = DistName::new(&removal.name)
            && protected.contains(&name)
        {
            after = after.without(&[name]);
        }
    }
    for install in &preview.installs {
        if let Ok(name) = DistName::new(&install.name)
            && protected.contains(&name)
        {
            after.insert(name, install.version.clone());
        }
    }
    pins.diff(&after)
}

fn silent_context() -> StepContext {
    StepContext {
        run_id: RunId::new("packages").expect("static id is valid"),
        step: StepId::ComfyRequirements,
        sink: Arc::new(NullSink),
        cancel: CancelToken::new(),
        allow_source_builds: false,
        retained: Default::default(),
    }
}

async fn selected_environment(
    deps: &EnvironmentDeps,
) -> Result<PythonEnvironment, EnvironmentAppError> {
    let env = deps
        .envs
        .active()
        .await?
        .ok_or(EnvironmentAppError::NoActiveEnvironment)?;
    match env.state() {
        EnvState::Verified | EnvState::RepairRequired => Ok(env),
        _ => Err(EnvironmentAppError::EnvironmentNotUsable),
    }
}

fn extra_constraints(
    deps: &EnvironmentDeps,
    env: &PythonEnvironment,
) -> Result<Vec<String>, EnvironmentAppError> {
    deps.catalog
        .get_revision(&env.profile_id, env.profile_revision)
        .map(|profile| profile.constraints)
        .ok_or(EnvironmentAppError::ProfileRevisionUnavailable)
}

/// Resolves a package change without mutating the environment (FR-ENV-3).
pub struct PreviewPackageChange {
    deps: EnvironmentDeps,
}

impl PreviewPackageChange {
    pub fn new(deps: EnvironmentDeps) -> Self {
        Self { deps }
    }

    pub async fn execute(
        &self,
        operation: PackageOperation,
    ) -> Result<PackagePlan, EnvironmentAppError> {
        validate(&operation)?;
        let deps = &self.deps;
        let env = selected_environment(deps).await?;
        let id = PlanId::new(format!("plan-{}", deps.ids.new_id()))?;
        let mut plan = PackagePlan {
            id,
            env_id: env.id.clone(),
            operation: operation.clone(),
            current_pins: env.pins.clone(),
            resolution: PlanResolution::Constrained,
            installs: Vec::new(),
            removals: Vec::new(),
            protected_changes: Vec::new(),
        };
        match &operation {
            PackageOperation::Uninstall(specs) => {
                plan.resolution = PlanResolution::Removal;
                for name in names_of(specs)? {
                    if let Some(version) = env.pins.get(&name) {
                        plan.protected_changes.push(PinChange {
                            name: name.clone(),
                            from: Some(version.to_owned()),
                            to: None,
                        });
                    }
                    plan.removals.push(InstalledPackage {
                        name: name.to_string(),
                        version: String::new(),
                    });
                }
                plan.protected_changes.sort();
            }
            PackageOperation::Install(specs) | PackageOperation::Upgrade(specs) => {
                let upgrade = matches!(operation, PackageOperation::Upgrade(_));
                self.resolve(&env, specs, upgrade, &mut plan).await?;
            }
        }
        deps.plans.insert(plan.clone());
        Ok(plan)
    }

    async fn resolve(
        &self,
        env: &PythonEnvironment,
        specs: &[String],
        upgrade: bool,
        plan: &mut PackagePlan,
    ) -> Result<(), EnvironmentAppError> {
        let deps = &self.deps;
        let root = deps.root.current().root;
        let ctx = silent_context();
        let constraint_file = deps
            .packages
            .write_constraints(&root, &env.id, &env.pins, &extra_constraints(deps, env)?)
            .await?;
        let base = InstallSpec {
            requirements: specs.iter().map(|spec| spec.trim().to_owned()).collect(),
            upgrade,
            allow_source_builds: false,
            ..InstallSpec::default()
        };
        let constrained = InstallSpec {
            constraint_file: Some(constraint_file),
            ..base.clone()
        };
        match deps
            .packages
            .preview_install(&root, &env.path, &constrained, &ctx)
            .await
        {
            Ok(preview) => {
                plan.protected_changes = preview_protected_changes(&env.pins, &preview);
                plan.resolution = PlanResolution::Constrained;
                plan.installs = preview.installs;
                plan.removals = preview.removals;
                Ok(())
            }
            Err(constrained_error) => {
                // A constrained failure alone does not prove a pin conflict: look at what an
                // unconstrained resolution would change, without mutating anything.
                match deps
                    .packages
                    .preview_install(&root, &env.path, &base, &ctx)
                    .await
                {
                    Ok(preview) => {
                        let changes = preview_protected_changes(&env.pins, &preview);
                        if changes.is_empty() {
                            return Err(EnvironmentAppError::ResolutionFailed(describe(
                                &constrained_error,
                            )));
                        }
                        plan.protected_changes = changes;
                        plan.resolution = PlanResolution::Unconstrained;
                        plan.installs = preview.installs;
                        plan.removals = preview.removals;
                        Ok(())
                    }
                    Err(_) => Err(EnvironmentAppError::ResolutionFailed(describe(
                        &constrained_error,
                    ))),
                }
            }
        }
    }
}

/// Applies a previewed plan under the maintenance gate. A failed mutation marks the selected
/// environment `repair_required`; the guard is never switched off globally.
pub struct ApplyPackageChange {
    deps: EnvironmentDeps,
}

impl ApplyPackageChange {
    pub fn new(deps: EnvironmentDeps) -> Self {
        Self { deps }
    }

    pub async fn execute(
        &self,
        plan_id: &PlanId,
        confirmed: Vec<PinChange>,
    ) -> Result<ApplyOutcome, EnvironmentAppError> {
        let deps = &self.deps;
        let plan = deps
            .plans
            .get(plan_id)
            .ok_or(EnvironmentAppError::PlanNotFound)?;
        let lease = deps.gate.try_acquire().await?;
        let ctx = StepContext {
            retained: lease.retained(),
            ..silent_context()
        };
        let root = deps.root.current().root;
        deps.runtime.ensure_stopped(&root).await?;

        let mut env = selected_environment(deps).await?;
        if env.id != plan.env_id || env.pins != plan.current_pins {
            return Err(EnvironmentAppError::PlanStale);
        }
        let protected = protected_names(&env);
        let installed = deps.packages.list(&root, &env.path, Some(&ctx)).await?;
        if pins_of(&installed, &protected) != plan.current_pins {
            return Err(EnvironmentAppError::PlanStale);
        }
        let mut approved = plan.protected_changes.clone();
        approved.sort();
        if plan.requires_confirmation() {
            let mut confirmed = confirmed;
            confirmed.sort();
            if confirmed != approved {
                return Err(EnvironmentAppError::ConfirmationMismatch);
            }
        }

        // The plan is single use: it is consumed only after every check passed.
        deps.plans
            .take(plan_id)
            .ok_or(EnvironmentAppError::PlanNotFound)?;

        // Pin everything except the changes the user approved, which are pinned to the
        // versions the plan showed.
        let changed: Vec<DistName> = approved.iter().map(|change| change.name.clone()).collect();
        let mut run_pins = env.pins.without(&changed);
        for change in &approved {
            if let Some(to) = &change.to {
                run_pins.insert(change.name.clone(), to.clone());
            }
        }
        let extra = extra_constraints(deps, &env)?;
        let constraint_file = deps
            .packages
            .write_constraints(&root, &env.id, &run_pins, &extra)
            .await?;
        // Persist before spawning: an interrupted installer may already have changed
        // packages. A restart must never mistake that environment for a verified one.
        let was_verified = env.state() == EnvState::Verified;
        if was_verified {
            env.mark_repair_required(&deps.clock.now())?;
            deps.envs.save(&env).await?;
        }
        let result = match &plan.operation {
            PackageOperation::Install(specs) | PackageOperation::Upgrade(specs) => {
                let spec = InstallSpec {
                    requirements: specs.iter().map(|spec| spec.trim().to_owned()).collect(),
                    constraint_file: Some(constraint_file),
                    upgrade: matches!(plan.operation, PackageOperation::Upgrade(_)),
                    allow_source_builds: false,
                    ..InstallSpec::default()
                };
                deps.packages.install(&root, &env.path, &spec, &ctx).await
            }
            PackageOperation::Uninstall(specs) => {
                deps.packages
                    .uninstall(&root, &env.path, &names_of(specs)?, &ctx)
                    .await
            }
        };
        if let Err(error) = result {
            return Err(match error {
                ProvisionError::Cancelled => EnvironmentAppError::PackageOperationFailed,
                other => other.into(),
            });
        }

        let after = match deps.packages.list(&root, &env.path, Some(&ctx)).await {
            Ok(installed) => pins_of(&installed, &protected),
            Err(error) => return Err(error.into()),
        };
        let mut drift = plan.current_pins.diff(&after);
        drift.sort();
        if drift != approved {
            return Err(EnvironmentAppError::PinDrift);
        }

        let now = deps.clock.now();
        env.record_pins(after.clone(), &now);
        let core_removed = drift.iter().any(|change| {
            change.to.is_none() && CORE_DISTRIBUTIONS.contains(&change.name.as_str())
        });
        deps.packages
            .write_constraints(&root, &env.id, &after, &extra)
            .await?;
        if was_verified && !core_removed {
            let mut expectations = deps
                .catalog
                .get(&env.profile_id)
                .ok_or(EnvironmentAppError::ProfileNotFound)?
                .verify;
            if let Some(torch) = after.get(&DistName::new("torch").expect("static name")) {
                expectations.torch_version_prefix = torch.to_owned();
            }
            let report = deps
                .verifier
                .verify_final(&root, &env.path, &expectations, &ctx)
                .await?;
            env.mark_verified(after, report.summary(), &now)?;
        }
        // A successful package change must not clear a pre-existing repair state.
        deps.envs.save(&env).await?;
        Ok(ApplyOutcome {
            env_state: env.state(),
            protected_changes: drift,
        })
    }
}
