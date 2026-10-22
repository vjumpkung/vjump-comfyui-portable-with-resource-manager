use std::{path::PathBuf, str::FromStr};

use super::{error::EnvironmentError, pins::PinSet, profile::SetupProfile, version::PythonVersion};

/// The identifier of a Python environment: lowercase letters, digits and `-`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EnvId(String);

impl EnvId {
    pub fn new(value: impl Into<String>) -> Result<Self, EnvironmentError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= 64
            && !value.starts_with('-')
            && value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
        if valid {
            Ok(Self(value))
        } else {
            Err(EnvironmentError::InvalidEnvId)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for EnvId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ownership {
    AppOwned,
    Adopted,
}

impl Ownership {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AppOwned => "app_owned",
            Self::Adopted => "adopted",
        }
    }
}

impl FromStr for Ownership {
    type Err = EnvironmentError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "app_owned" => Ok(Self::AppOwned),
            "adopted" => Ok(Self::Adopted),
            _ => Err(EnvironmentError::InvalidOwnership),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnvState {
    Provisioning,
    Verified,
    Failed,
    RepairRequired,
}

impl EnvState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Provisioning => "provisioning",
            Self::Verified => "verified",
            Self::Failed => "failed",
            Self::RepairRequired => "repair_required",
        }
    }
}

impl FromStr for EnvState {
    type Err = EnvironmentError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "provisioning" => Ok(Self::Provisioning),
            "verified" => Ok(Self::Verified),
            "failed" => Ok(Self::Failed),
            "repair_required" => Ok(Self::RepairRequired),
            _ => Err(EnvironmentError::InvalidEnvState),
        }
    }
}

/// How the interpreter and venv came to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PythonSourceKind {
    Managed,
    CustomVenv,
    CustomInterpreter,
}

impl PythonSourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Managed => "managed",
            Self::CustomVenv => "custom_venv",
            Self::CustomInterpreter => "custom_interpreter",
        }
    }
}

impl FromStr for PythonSourceKind {
    type Err = EnvironmentError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "managed" => Ok(Self::Managed),
            "custom_venv" => Ok(Self::CustomVenv),
            "custom_interpreter" => Ok(Self::CustomInterpreter),
            _ => Err(EnvironmentError::InvalidPythonSource),
        }
    }
}

/// A Python environment ComfyUI runs in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonEnvironment {
    pub id: EnvId,
    pub path: PathBuf,
    pub ownership: Ownership,
    pub source: PythonSourceKind,
    /// The interpreter the venv was created from, when known.
    pub base_interpreter: Option<PathBuf>,
    pub python_version: PythonVersion,
    pub profile_id: String,
    pub profile_revision: u32,
    /// The exact installed versions of the protected distributions.
    pub pins: PinSet,
    /// A human-readable summary of the last verification, if any.
    pub verification: Option<String>,
    /// The only location the app may modify or delete for this environment.
    pub authorized_location: PathBuf,
    state: EnvState,
    pub created_at: String,
    pub updated_at: String,
}

pub struct NewEnvironment {
    pub id: EnvId,
    pub path: PathBuf,
    pub ownership: Ownership,
    pub source: PythonSourceKind,
    pub base_interpreter: Option<PathBuf>,
    pub python_version: PythonVersion,
    pub profile_id: String,
    pub profile_revision: u32,
    pub authorized_location: PathBuf,
    pub now: String,
}

impl PythonEnvironment {
    pub fn provisioning(new: NewEnvironment) -> Self {
        Self {
            id: new.id,
            path: new.path,
            ownership: new.ownership,
            source: new.source,
            base_interpreter: new.base_interpreter,
            python_version: new.python_version,
            profile_id: new.profile_id,
            profile_revision: new.profile_revision,
            pins: PinSet::new(),
            verification: None,
            authorized_location: new.authorized_location,
            state: EnvState::Provisioning,
            created_at: new.now.clone(),
            updated_at: new.now,
        }
    }

    /// Rebuilds an environment from storage without re-running transitions.
    pub fn restore(
        new: NewEnvironment,
        state: EnvState,
        pins: PinSet,
        verification: Option<String>,
        created_at: String,
    ) -> Self {
        let mut env = Self::provisioning(new);
        env.state = state;
        env.pins = pins;
        env.verification = verification;
        env.created_at = created_at;
        env
    }

    pub fn state(&self) -> EnvState {
        self.state
    }

    fn transition(&mut self, to: EnvState, now: &str) -> Result<(), EnvironmentError> {
        use EnvState::*;
        let allowed = matches!(
            (self.state, to),
            (Provisioning, Verified)
                | (Provisioning, Failed)
                | (Verified, RepairRequired)
                | (RepairRequired, Verified)
                | (Failed, Provisioning)
        );
        if !allowed {
            return Err(EnvironmentError::InvalidTransition {
                from: self.state,
                to,
            });
        }
        self.state = to;
        self.updated_at = now.to_owned();
        Ok(())
    }

    pub fn mark_verified(
        &mut self,
        pins: PinSet,
        verification: String,
        now: &str,
    ) -> Result<(), EnvironmentError> {
        self.transition(EnvState::Verified, now)?;
        self.pins = pins;
        self.verification = Some(verification);
        Ok(())
    }

    pub fn mark_failed(&mut self, now: &str) -> Result<(), EnvironmentError> {
        self.transition(EnvState::Failed, now)
    }

    /// A failed retry resumes the same environment.
    pub fn begin_retry(&mut self, now: &str) -> Result<(), EnvironmentError> {
        self.transition(EnvState::Provisioning, now)
    }

    /// A failed in-place dependency operation leaves the environment selected but unusable
    /// until it is repaired.
    pub fn mark_repair_required(&mut self, now: &str) -> Result<(), EnvironmentError> {
        self.transition(EnvState::RepairRequired, now)
    }

    /// Repair verification passed without rebuilding.
    pub fn mark_repaired(
        &mut self,
        pins: PinSet,
        verification: String,
        now: &str,
    ) -> Result<(), EnvironmentError> {
        self.transition(EnvState::Verified, now)?;
        self.pins = pins;
        self.verification = Some(verification);
        Ok(())
    }

    /// Approved pin changes update the stored pins of a verified environment.
    pub fn record_pins(&mut self, pins: PinSet, now: &str) {
        self.pins = pins;
        self.updated_at = now.to_owned();
    }

    /// Only app-owned, inactive environments may be deleted.
    pub fn check_deletable(&self, is_active: bool) -> Result<(), EnvironmentError> {
        if self.ownership == Ownership::Adopted {
            return Err(EnvironmentError::AdoptedNotDeletable);
        }
        if is_active {
            return Err(EnvironmentError::ActiveNotDeletable);
        }
        Ok(())
    }

    /// Rebuilding an adopted environment in place needs an explicit, recorded confirmation.
    pub fn check_rebuild_confirmed(&self, confirmed: bool) -> Result<(), EnvironmentError> {
        if self.ownership == Ownership::Adopted && !confirmed {
            return Err(EnvironmentError::AdoptedRebuildNeedsConfirmation);
        }
        Ok(())
    }
}

/// Activation policy: exactly one verified environment is active at a time.
pub fn check_activation(
    candidate: &PythonEnvironment,
    current_active: Option<&PythonEnvironment>,
) -> Result<(), EnvironmentError> {
    if candidate.state() != EnvState::Verified {
        return Err(EnvironmentError::NotActivatable);
    }
    if current_active.is_some_and(|active| active.id == candidate.id) {
        return Err(EnvironmentError::AlreadyActive);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SetupMode {
    FirstRun,
    Repair,
    ChangeTorch,
    ChangePython,
    SwitchVenv,
    Rebuild,
}

impl SetupMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FirstRun => "first_run",
            Self::Repair => "repair",
            Self::ChangeTorch => "change_torch",
            Self::ChangePython => "change_python",
            Self::SwitchVenv => "switch_venv",
            Self::Rebuild => "rebuild",
        }
    }

    /// Mode rules from FR-SETUP-10, checked against the selected profile.
    pub fn check(
        self,
        active: Option<&PythonEnvironment>,
        profile: &SetupProfile,
    ) -> Result<(), EnvironmentError> {
        match (self, active) {
            (Self::FirstRun, Some(_)) => Err(EnvironmentError::FirstRunWithActiveEnvironment),
            (Self::FirstRun, None) => Ok(()),
            (_, None) => Err(EnvironmentError::ModeNeedsActiveEnvironment),
            (Self::Repair | Self::ChangeTorch, Some(active)) => {
                if profile.python.same_minor(active.python_version) {
                    Ok(())
                } else {
                    Err(EnvironmentError::ModeNeedsSamePythonMinor)
                }
            }
            (Self::ChangePython, Some(active)) => {
                if profile.python.same_minor(active.python_version) {
                    Err(EnvironmentError::ModeNeedsDifferentPythonMinor)
                } else {
                    Ok(())
                }
            }
            (Self::SwitchVenv | Self::Rebuild, Some(_)) => Ok(()),
        }
    }
}

impl FromStr for SetupMode {
    type Err = EnvironmentError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "first_run" => Ok(Self::FirstRun),
            "repair" => Ok(Self::Repair),
            "change_torch" => Ok(Self::ChangeTorch),
            "change_python" => Ok(Self::ChangePython),
            "switch_venv" => Ok(Self::SwitchVenv),
            "rebuild" => Ok(Self::Rebuild),
            _ => Err(EnvironmentError::InvalidSetupMode),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::environment::{pins::DistName, profile::fixtures::profile};

    fn new_environment(id: &str, ownership: Ownership) -> NewEnvironment {
        NewEnvironment {
            id: EnvId::new(id).unwrap(),
            path: PathBuf::from("envs").join(id),
            ownership,
            source: PythonSourceKind::Managed,
            base_interpreter: None,
            python_version: PythonVersion::new(3, 13, 16),
            profile_id: "p".into(),
            profile_revision: 1,
            authorized_location: PathBuf::from("envs").join(id),
            now: "2026-10-16T00:00:00Z".into(),
        }
    }

    fn environment(id: &str, ownership: Ownership) -> PythonEnvironment {
        PythonEnvironment::provisioning(new_environment(id, ownership))
    }

    fn verified(id: &str, ownership: Ownership) -> PythonEnvironment {
        let mut env = environment(id, ownership);
        env.mark_verified(PinSet::new(), "ok".into(), "t").unwrap();
        env
    }

    #[test]
    fn ids_are_restricted() {
        assert!(EnvId::new("env-0a1b").is_ok());
        for bad in ["", "-x", "UPPER", "a b", "a/b", &"x".repeat(65)] {
            assert!(EnvId::new(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn only_documented_transitions_are_allowed() {
        use EnvState::*;
        let mut env = environment("a", Ownership::AppOwned);
        assert!(env.mark_repair_required("t").is_err());
        env.mark_failed("t").unwrap();
        assert_eq!(env.state(), Failed);
        assert!(env.mark_verified(PinSet::new(), "x".into(), "t").is_err());
        env.begin_retry("t").unwrap();
        env.mark_verified(PinSet::new(), "ok".into(), "t").unwrap();
        assert!(env.mark_failed("t").is_err());
        env.mark_repair_required("t").unwrap();
        assert!(env.mark_failed("t").is_err());
        env.mark_repaired(PinSet::new(), "ok".into(), "t").unwrap();
        assert_eq!(env.state(), Verified);
        assert!(env.begin_retry("t").is_err());
    }

    #[test]
    fn only_verified_inactive_environments_activate() {
        let candidate = verified("new", Ownership::AppOwned);
        let active = verified("old", Ownership::AppOwned);
        assert!(check_activation(&candidate, Some(&active)).is_ok());
        assert!(check_activation(&candidate, None).is_ok());
        assert_eq!(
            check_activation(&candidate, Some(&candidate)),
            Err(EnvironmentError::AlreadyActive)
        );
        let provisioning = environment("p", Ownership::AppOwned);
        assert_eq!(
            check_activation(&provisioning, None),
            Err(EnvironmentError::NotActivatable)
        );
        let mut broken = verified("b", Ownership::AppOwned);
        broken.mark_repair_required("t").unwrap();
        assert_eq!(
            check_activation(&broken, None),
            Err(EnvironmentError::NotActivatable)
        );
    }

    #[test]
    fn adopted_environments_are_never_deleted_and_need_rebuild_confirmation() {
        let adopted = verified("a", Ownership::Adopted);
        assert_eq!(
            adopted.check_deletable(false),
            Err(EnvironmentError::AdoptedNotDeletable)
        );
        assert_eq!(
            adopted.check_rebuild_confirmed(false),
            Err(EnvironmentError::AdoptedRebuildNeedsConfirmation)
        );
        assert!(adopted.check_rebuild_confirmed(true).is_ok());
        let owned = verified("o", Ownership::AppOwned);
        assert_eq!(
            owned.check_deletable(true),
            Err(EnvironmentError::ActiveNotDeletable)
        );
        assert!(owned.check_deletable(false).is_ok());
        assert!(owned.check_rebuild_confirmed(false).is_ok());
    }

    #[test]
    fn setup_modes_follow_the_resetup_rules() {
        let active = verified("old", Ownership::AppOwned);
        let same_minor = profile();
        let mut other_minor = profile();
        other_minor.python = PythonVersion::new(3, 12, 15);

        assert!(SetupMode::FirstRun.check(None, &same_minor).is_ok());
        assert_eq!(
            SetupMode::FirstRun.check(Some(&active), &same_minor),
            Err(EnvironmentError::FirstRunWithActiveEnvironment)
        );
        for mode in [
            SetupMode::Repair,
            SetupMode::ChangeTorch,
            SetupMode::ChangePython,
            SetupMode::SwitchVenv,
        ] {
            assert_eq!(
                mode.check(None, &same_minor),
                Err(EnvironmentError::ModeNeedsActiveEnvironment)
            );
        }
        assert!(SetupMode::Repair.check(Some(&active), &same_minor).is_ok());
        assert!(
            SetupMode::ChangeTorch
                .check(Some(&active), &same_minor)
                .is_ok()
        );
        assert_eq!(
            SetupMode::ChangeTorch.check(Some(&active), &other_minor),
            Err(EnvironmentError::ModeNeedsSamePythonMinor)
        );
        assert!(
            SetupMode::ChangePython
                .check(Some(&active), &other_minor)
                .is_ok()
        );
        assert_eq!(
            SetupMode::ChangePython.check(Some(&active), &same_minor),
            Err(EnvironmentError::ModeNeedsDifferentPythonMinor)
        );
        assert!(
            SetupMode::SwitchVenv
                .check(Some(&active), &same_minor)
                .is_ok()
        );
    }

    #[test]
    fn ids_expose_and_display_their_value() {
        let id = EnvId::new("env-0a1b").unwrap();
        assert_eq!(id.as_str(), "env-0a1b");
        assert_eq!(id.to_string(), "env-0a1b");
    }

    #[test]
    fn ownership_round_trips_and_rejects_unknown() {
        for (ownership, text) in [
            (Ownership::AppOwned, "app_owned"),
            (Ownership::Adopted, "adopted"),
        ] {
            assert_eq!(ownership.as_str(), text);
            assert_eq!(text.parse::<Ownership>(), Ok(ownership));
        }
        assert_eq!(
            "AppOwned".parse::<Ownership>(),
            Err(EnvironmentError::InvalidOwnership)
        );
        assert_eq!(
            "".parse::<Ownership>(),
            Err(EnvironmentError::InvalidOwnership)
        );
    }

    #[test]
    fn env_state_round_trips_and_rejects_unknown() {
        for (state, text) in [
            (EnvState::Provisioning, "provisioning"),
            (EnvState::Verified, "verified"),
            (EnvState::Failed, "failed"),
            (EnvState::RepairRequired, "repair_required"),
        ] {
            assert_eq!(state.as_str(), text);
            assert_eq!(text.parse::<EnvState>(), Ok(state));
        }
        assert_eq!(
            "repairRequired".parse::<EnvState>(),
            Err(EnvironmentError::InvalidEnvState)
        );
    }

    #[test]
    fn python_source_kind_round_trips_and_rejects_unknown() {
        for (source, text) in [
            (PythonSourceKind::Managed, "managed"),
            (PythonSourceKind::CustomVenv, "custom_venv"),
            (PythonSourceKind::CustomInterpreter, "custom_interpreter"),
        ] {
            assert_eq!(source.as_str(), text);
            assert_eq!(text.parse::<PythonSourceKind>(), Ok(source));
        }
        assert_eq!(
            "conda".parse::<PythonSourceKind>(),
            Err(EnvironmentError::InvalidPythonSource)
        );
    }

    #[test]
    fn setup_mode_round_trips_and_rejects_unknown() {
        for (mode, text) in [
            (SetupMode::FirstRun, "first_run"),
            (SetupMode::Repair, "repair"),
            (SetupMode::ChangeTorch, "change_torch"),
            (SetupMode::ChangePython, "change_python"),
            (SetupMode::SwitchVenv, "switch_venv"),
            (SetupMode::Rebuild, "rebuild"),
        ] {
            assert_eq!(mode.as_str(), text);
            assert_eq!(text.parse::<SetupMode>(), Ok(mode));
        }
        assert_eq!(
            "reinstall".parse::<SetupMode>(),
            Err(EnvironmentError::InvalidSetupMode)
        );
    }

    #[test]
    fn restore_keeps_stored_state_without_running_transitions() {
        let mut pins = PinSet::new();
        pins.insert(DistName::new("torch").unwrap(), "2.14.1+cu130");
        let restored = PythonEnvironment::restore(
            new_environment("old", Ownership::Adopted),
            EnvState::RepairRequired,
            pins.clone(),
            Some("torch drifted".into()),
            "2026-01-02T03:04:05Z".into(),
        );
        assert_eq!(restored.state(), EnvState::RepairRequired);
        assert_eq!(restored.pins, pins);
        assert_eq!(restored.verification.as_deref(), Some("torch drifted"));
        assert_eq!(restored.created_at, "2026-01-02T03:04:05Z");
        assert_eq!(restored.updated_at, "2026-10-16T00:00:00Z");
        assert_eq!(restored.id.as_str(), "old");
        assert_eq!(restored.ownership, Ownership::Adopted);
    }

    #[test]
    fn restore_without_verification_leaves_it_unset() {
        let restored = PythonEnvironment::restore(
            new_environment("failed", Ownership::AppOwned),
            EnvState::Failed,
            PinSet::new(),
            None,
            "2026-01-02T03:04:05Z".into(),
        );
        assert_eq!(restored.state(), EnvState::Failed);
        assert!(restored.pins.is_empty());
        assert_eq!(restored.verification, None);
    }

    #[test]
    fn recording_pins_replaces_them_and_touches_only_the_update_time() {
        let mut env = verified("a", Ownership::AppOwned);
        let created_at = env.created_at.clone();
        let mut pins = PinSet::new();
        pins.insert(DistName::new("torch").unwrap(), "2.15.0");
        env.record_pins(pins.clone(), "later");
        assert_eq!(env.pins, pins);
        assert_eq!(env.updated_at, "later");
        assert_eq!(env.created_at, created_at);
        assert_eq!(env.state(), EnvState::Verified);
        assert_eq!(env.verification.as_deref(), Some("ok"));
    }
}
