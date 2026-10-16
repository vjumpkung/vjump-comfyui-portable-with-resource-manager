use std::path::{Path, PathBuf};

use domain::environment::{
    Eligibility, EnvId, EnvState, HardwareObservation, PythonEnvironment, SetupProfile, evaluate,
};

use super::{
    deps::EnvironmentDeps,
    error::EnvironmentAppError,
    model::{
        EnvironmentSummary, InstalledPackage, ManifestState, ProvisioningRun, PythonSourceInput,
    },
    ports::{PythonValidationReport, RootInfo},
};

pub fn summarize(env: &PythonEnvironment) -> EnvironmentSummary {
    EnvironmentSummary {
        id: env.id.clone(),
        path: env.path.clone(),
        ownership: env.ownership,
        source: env.source,
        python_version: env.python_version,
        profile_id: env.profile_id.clone(),
        profile_revision: env.profile_revision,
        state: env.state(),
        pins: env.pins.clone(),
        verification: env.verification.clone(),
    }
}

/// Observes the machine for the hardware step (FR-SETUP-2).
pub struct DetectHardware {
    deps: EnvironmentDeps,
}

impl DetectHardware {
    pub fn new(deps: EnvironmentDeps) -> Self {
        Self { deps }
    }

    pub async fn execute(
        &self,
        install_root: &Path,
        env_path: Option<&Path>,
    ) -> HardwareObservation {
        self.deps.hardware.probe(install_root, env_path).await
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileOffer {
    pub profile: SetupProfile,
    pub eligibility: Eligibility,
}

/// Lists the bundled profiles for this machine, each enabled or disabled with its reasons.
pub struct ListProfiles {
    deps: EnvironmentDeps,
}

impl ListProfiles {
    pub fn new(deps: EnvironmentDeps) -> Self {
        Self { deps }
    }

    pub async fn execute(&self, install_root: &Path, env_path: Option<&Path>) -> Vec<ProfileOffer> {
        let hardware = self.deps.hardware.probe(install_root, env_path).await;
        self.deps
            .catalog
            .list()
            .into_iter()
            .filter(|profile| profile.os == hardware.os)
            .map(|profile| ProfileOffer {
                eligibility: evaluate(&profile, &hardware),
                profile,
            })
            .collect()
    }
}

pub struct ValidatePython {
    deps: EnvironmentDeps,
}

impl ValidatePython {
    pub fn new(deps: EnvironmentDeps) -> Self {
        Self { deps }
    }

    pub async fn execute(
        &self,
        install_root: &Path,
        source: &PythonSourceInput,
    ) -> Result<PythonValidationReport, EnvironmentAppError> {
        Ok(self.deps.python.validate(install_root, source).await?)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryReason {
    ManifestCorrupt,
    ManifestNewer,
    ManifestMissing,
    ManifestMismatch,
}

impl RecoveryReason {
    pub fn code(self) -> &'static str {
        match self {
            Self::ManifestCorrupt => "manifest_corrupt",
            Self::ManifestNewer => "manifest_newer",
            Self::ManifestMissing => "manifest_missing",
            Self::ManifestMismatch => "manifest_mismatch",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupPhase {
    /// No valid install: the first-run wizard applies.
    NeedsSetup,
    Ready,
    /// The selected environment needs repair before ComfyUI may start.
    RepairRequired,
    RunInProgress,
    RunFailed,
    /// `install.json` and the database disagree or are unusable; nothing is changed silently.
    Recovery(RecoveryReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupStatus {
    pub phase: SetupPhase,
    pub root: RootInfo,
    pub active_env: Option<EnvironmentSummary>,
    /// The most recent run, reconstructable after a reload.
    pub run: Option<ProvisioningRun>,
    pub draft: Option<String>,
}

pub struct GetSetupStatus {
    deps: EnvironmentDeps,
}

impl GetSetupStatus {
    pub fn new(deps: EnvironmentDeps) -> Self {
        Self { deps }
    }

    pub async fn execute(&self) -> Result<SetupStatus, EnvironmentAppError> {
        let deps = &self.deps;
        let root = deps.root.current();
        let manifest = deps.manifests.read(&root.root).await?;
        let active = deps.envs.active().await?;
        let run = deps.runs.latest().await?;
        let draft = deps.drafts.load().await?;

        let phase = match (&manifest, &active) {
            (ManifestState::Corrupt, _) => SetupPhase::Recovery(RecoveryReason::ManifestCorrupt),
            (ManifestState::Newer, _) => SetupPhase::Recovery(RecoveryReason::ManifestNewer),
            (ManifestState::Absent, Some(_)) => {
                SetupPhase::Recovery(RecoveryReason::ManifestMissing)
            }
            (ManifestState::Valid(_), None) => {
                SetupPhase::Recovery(RecoveryReason::ManifestMismatch)
            }
            (ManifestState::Valid(manifest), Some(active)) if manifest.env_id != active.id => {
                SetupPhase::Recovery(RecoveryReason::ManifestMismatch)
            }
            _ => match (&run, &active) {
                (Some(run), _) if run.status.is_active() => SetupPhase::RunInProgress,
                (Some(run), None) if run.status.is_retryable() => SetupPhase::RunFailed,
                (_, Some(active)) if active.state() == EnvState::RepairRequired => {
                    SetupPhase::RepairRequired
                }
                (_, Some(_)) => SetupPhase::Ready,
                (_, None) => SetupPhase::NeedsSetup,
            },
        };
        Ok(SetupStatus {
            phase,
            root,
            active_env: active.as_ref().map(summarize),
            run,
            draft,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentInfo {
    pub env: EnvironmentSummary,
    pub profile_label: Option<String>,
    pub cuda_variant: Option<String>,
    pub torch_version: Option<String>,
    pub triton_version: Option<String>,
    pub comfyui_reference: Option<String>,
    /// The commit currently checked out, when it can be read.
    pub comfyui_commit: Option<String>,
    pub accelerators: Vec<String>,
}

/// FR-ENV-1: the selected environment.
pub struct GetEnvironmentInfo {
    deps: EnvironmentDeps,
}

impl GetEnvironmentInfo {
    pub fn new(deps: EnvironmentDeps) -> Self {
        Self { deps }
    }

    pub async fn execute(&self) -> Result<EnvironmentInfo, EnvironmentAppError> {
        let deps = &self.deps;
        let env = deps
            .envs
            .active()
            .await?
            .ok_or(EnvironmentAppError::NoActiveEnvironment)?;
        let root = deps.root.current().root;
        let profile = deps.catalog.get(&env.profile_id);
        let manifest_reference = match deps.manifests.read(&root).await? {
            ManifestState::Valid(manifest) => manifest.comfyui_reference,
            _ => None,
        };
        let comfyui_commit = match deps.source.inspect_checkout(&root).await {
            Ok(super::ports::Checkout::Repo { commit }) => Some(commit),
            _ => None,
        };
        let pin = |name: &str| {
            domain::environment::DistName::new(name)
                .ok()
                .and_then(|name| env.pins.get(&name).map(str::to_owned))
        };
        let triton_version = pin("triton-windows").or_else(|| pin("triton"));
        Ok(EnvironmentInfo {
            profile_label: profile.as_ref().map(|profile| profile.label.clone()),
            cuda_variant: profile
                .as_ref()
                .map(|profile| profile.torch.cuda.as_str().to_owned()),
            torch_version: pin("torch"),
            triton_version,
            comfyui_reference: manifest_reference,
            comfyui_commit,
            accelerators: Vec::new(),
            env: summarize(&env),
        })
    }
}

/// FR-ENV-2: the packages installed in the selected environment.
pub struct ListPackages {
    deps: EnvironmentDeps,
}

impl ListPackages {
    pub fn new(deps: EnvironmentDeps) -> Self {
        Self { deps }
    }

    pub async fn execute(&self) -> Result<Vec<InstalledPackage>, EnvironmentAppError> {
        let deps = &self.deps;
        let env = deps
            .envs
            .active()
            .await?
            .ok_or(EnvironmentAppError::NoActiveEnvironment)?;
        let root = deps.root.current().root;
        let mut packages = deps.packages.list(&root, &env.path).await?;
        packages.sort_by_key(|package| package.name.to_lowercase());
        Ok(packages)
    }
}

/// Deletes an app-owned environment that is not active. Adopted environments are never deleted.
pub struct DeleteEnvironment {
    deps: EnvironmentDeps,
}

impl DeleteEnvironment {
    pub fn new(deps: EnvironmentDeps) -> Self {
        Self { deps }
    }

    pub async fn execute(&self, id: &EnvId) -> Result<(), EnvironmentAppError> {
        let deps = &self.deps;
        let _lease = deps.gate.try_acquire().await?;
        let root = deps.root.current().root;
        deps.runtime.ensure_stopped(&root).await?;
        let env = deps
            .envs
            .get(id)
            .await?
            .ok_or(EnvironmentAppError::EnvironmentNotFound)?;
        let is_active = deps
            .envs
            .active()
            .await?
            .is_some_and(|active| active.id == env.id);
        env.check_deletable(is_active)?;
        let in_use = deps
            .runs
            .unfinished()
            .await?
            .iter()
            .any(|run| run.target_env == env.id);
        if in_use {
            return Err(EnvironmentAppError::EnvironmentInUse);
        }
        deps.python.remove_environment(&env).await?;
        deps.envs.delete(&env.id).await?;
        Ok(())
    }
}

/// Selecting a different install root is only possible before setup. The app restarts to use
/// the new root's database and logging.
pub struct SelectInstallRoot {
    deps: EnvironmentDeps,
}

impl SelectInstallRoot {
    pub fn new(deps: EnvironmentDeps) -> Self {
        Self { deps }
    }

    pub async fn execute(
        &self,
        new_root: PathBuf,
        draft: Option<String>,
    ) -> Result<(), EnvironmentAppError> {
        let deps = &self.deps;
        let _lease = deps.gate.try_acquire().await?;
        if deps.envs.active().await?.is_some() {
            return Err(EnvironmentAppError::RootChangeNotAllowed);
        }
        if deps
            .runs
            .unfinished()
            .await?
            .iter()
            .any(|run| run.status.is_active())
        {
            return Err(EnvironmentAppError::SetupInProgress);
        }
        deps.root.switch_root(&new_root, draft.as_deref()).await?;
        Ok(())
    }
}
