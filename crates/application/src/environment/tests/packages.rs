use std::sync::{Arc, atomic::Ordering};

use domain::environment::{EnvState, PinChange, PythonVersion};

use super::fakes::*;
use crate::{comfy_runtime::RuntimeBlock, environment::*, test_support::block_on};

struct Fixture {
    world: Arc<World>,
    deps: EnvironmentDeps,
}

fn fixture() -> Fixture {
    let world = World::new();
    world.seed_active_env("env-old", PythonVersion::new(3, 13, 16));
    let deps = world.deps();
    Fixture { world, deps }
}

impl Fixture {
    fn preview(&self, operation: PackageOperation) -> Result<PackagePlan, EnvironmentAppError> {
        block_on(PreviewPackageChange::new(self.deps.clone()).execute(operation))
    }

    fn apply(
        &self,
        plan: &PackagePlan,
        confirmed: Vec<PinChange>,
    ) -> Result<ApplyOutcome, EnvironmentAppError> {
        block_on(ApplyPackageChange::new(self.deps.clone()).execute(&plan.id, confirmed))
    }

    fn queue_preview(&self, result: Result<ResolutionPreview, ProvisionError>) {
        self.world.previews.lock().unwrap().push_back(result);
    }
}

fn installs(packages: &[(&str, &str)]) -> ResolutionPreview {
    ResolutionPreview {
        installs: packages.iter().map(|(n, v)| package(n, v)).collect(),
        removals: Vec::new(),
    }
}

fn torch_change(to: &str) -> PinChange {
    PinChange {
        name: name("torch"),
        from: Some("2.14.1+cu130".into()),
        to: Some(to.into()),
    }
}

fn torch_upgrade_preview() -> ResolutionPreview {
    ResolutionPreview {
        installs: vec![package("torch", "2.15.0")],
        removals: vec![package("torch", "2.14.1+cu130")],
    }
}

#[test]
fn package_preview_and_apply_keep_the_recorded_profile_revision_constraints() {
    let fixture = fixture();
    let extras = vec!["transformers==5.5.0".to_owned()];
    {
        let mut profiles = fixture.world.profiles.lock().unwrap();
        assert_eq!(
            fixture.world.env("env-old").profile_revision,
            profiles[0].revision
        );
        profiles[0].constraints = extras.clone();
        let mut current = profiles[0].clone();
        current.revision += 1;
        current.constraints = vec!["transformers==5.20.2".into()];
        profiles.insert(0, current);
    }
    let plan = fixture
        .preview(PackageOperation::Install(vec!["requests".into()]))
        .unwrap();
    assert_eq!(
        fixture.world.constraint_extras.lock().unwrap().as_slice(),
        std::slice::from_ref(&extras)
    );
    fixture.apply(&plan, Vec::new()).unwrap();
    assert_eq!(
        *fixture.world.constraint_extras.lock().unwrap(),
        [extras.clone(), extras.clone(), extras]
    );
}

#[test]
fn package_preview_errors_when_the_recorded_profile_revision_is_missing() {
    let fixture = fixture();
    fixture.world.profiles.lock().unwrap()[0].revision += 1;
    assert!(matches!(
        fixture.preview(PackageOperation::Install(vec!["requests".into()])),
        Err(EnvironmentAppError::ProfileRevisionUnavailable)
    ));
    assert!(fixture.world.constraint_extras.lock().unwrap().is_empty());
    assert_eq!(fixture.world.count("preview:constrained"), 0);
}

#[test]
fn package_apply_errors_when_the_recorded_profile_revision_is_missing() {
    let fixture = fixture();
    let plan = fixture
        .preview(PackageOperation::Install(vec!["requests".into()]))
        .unwrap();
    fixture.world.profiles.lock().unwrap()[0].revision += 1;
    assert!(matches!(
        fixture.apply(&plan, Vec::new()),
        Err(EnvironmentAppError::ProfileRevisionUnavailable)
    ));
    assert_eq!(fixture.world.constraint_extras.lock().unwrap().len(), 1);
    assert!(fixture.world.installs.lock().unwrap().is_empty());
    assert_eq!(fixture.world.env("env-old").state(), EnvState::Verified);
}

#[test]
fn a_constrained_change_needs_no_confirmation_and_runs_under_the_guard() {
    let fixture = fixture();
    fixture.queue_preview(Ok(installs(&[("requests", "2.34.2")])));
    let plan = fixture
        .preview(PackageOperation::Install(vec!["requests".into()]))
        .unwrap();
    assert_eq!(plan.resolution, PlanResolution::Constrained);
    assert!(!plan.requires_confirmation());
    assert_eq!(plan.installs[0].name, "requests");
    assert!(
        fixture
            .world
            .calls()
            .contains(&"preview:constrained".to_owned())
    );

    let outcome = fixture.apply(&plan, Vec::new()).unwrap();
    assert_eq!(outcome.env_state, EnvState::Verified);
    assert!(outcome.protected_changes.is_empty());
    let installs = fixture.world.installs.lock().unwrap().clone();
    assert_eq!(installs.len(), 1);
    assert!(installs[0].constraint_file.is_some());
    assert!(!installs[0].upgrade);
    assert_eq!(fixture.world.env("env-old").state(), EnvState::Verified);
    assert_eq!(fixture.world.gate.try_acquires.load(Ordering::SeqCst), 1);
}

#[test]
fn a_pin_conflict_is_previewed_and_applied_only_with_the_exact_confirmation() {
    let fixture = fixture();
    fixture.queue_preview(Err(ProvisionError::Failed(
        "conflicting torch requirement".into(),
    )));
    fixture.queue_preview(Ok(torch_upgrade_preview()));
    let plan = fixture
        .preview(PackageOperation::Upgrade(vec!["torch".into()]))
        .unwrap();
    assert_eq!(plan.resolution, PlanResolution::Unconstrained);
    assert_eq!(plan.protected_changes, vec![torch_change("2.15.0")]);
    assert!(plan.requires_confirmation());
    let calls = fixture.world.calls();
    assert!(calls.contains(&"preview:constrained".to_owned()));
    assert!(calls.contains(&"preview:unconstrained".to_owned()));

    // Nothing was installed by previewing.
    assert!(fixture.world.installs.lock().unwrap().is_empty());

    // No confirmation, or a confirmation for different versions, is refused without mutation.
    for wrong in [Vec::new(), vec![torch_change("9.9.9")]] {
        assert!(matches!(
            fixture.apply(&plan, wrong),
            Err(EnvironmentAppError::ConfirmationMismatch)
        ));
    }
    assert!(fixture.world.installs.lock().unwrap().is_empty());

    fixture
        .world
        .install_overrides
        .lock()
        .unwrap()
        .push(("torch".into(), "2.15.0".into()));
    let outcome = fixture.apply(&plan, vec![torch_change("2.15.0")]).unwrap();
    assert_eq!(outcome.protected_changes, vec![torch_change("2.15.0")]);
    assert_eq!(outcome.env_state, EnvState::Verified);
    let env = fixture.world.env("env-old");
    assert_eq!(env.pins.get(&name("torch")), Some("2.15.0"));
    assert_eq!(env.pins.get(&name("torchvision")), Some("0.29.1+cu130"));
    // The approved change is pinned to the previewed version; every other pin is kept.
    let first_constraints = fixture.world.constraint_writes.lock().unwrap()[1].clone();
    assert_eq!(first_constraints.get(&name("torch")), Some("2.15.0"));
    assert_eq!(
        first_constraints.get(&name("torchvision")),
        Some("0.29.1+cu130")
    );
    // The plan is single use.
    assert!(matches!(
        fixture.apply(&plan, vec![torch_change("2.15.0")]),
        Err(EnvironmentAppError::PlanNotFound)
    ));
}

#[test]
fn a_constrained_failure_alone_is_not_reported_as_a_pin_conflict() {
    let fixture = fixture();
    fixture.queue_preview(Err(ProvisionError::Failed(
        "no matching distribution".into(),
    )));
    fixture.queue_preview(Ok(installs(&[("whatever", "1.0")])));
    match fixture.preview(PackageOperation::Install(vec!["whatever".into()])) {
        Err(EnvironmentAppError::ResolutionFailed(message)) => {
            assert!(message.contains("no matching distribution"));
        }
        other => panic!("expected a resolution failure, got {other:?}"),
    }
    // Neither a failed constrained nor a failed unconstrained resolution creates a plan.
    fixture.queue_preview(Err(ProvisionError::Failed("first".into())));
    fixture.queue_preview(Err(ProvisionError::Failed("second".into())));
    assert!(matches!(
        fixture.preview(PackageOperation::Install(vec!["x".into()])),
        Err(EnvironmentAppError::ResolutionFailed(message)) if message.contains("first")
    ));
}

#[test]
fn a_failed_mutation_marks_the_selected_environment_repair_required() {
    let fixture = fixture();
    fixture.queue_preview(Ok(installs(&[("broken", "1.0")])));
    let plan = fixture
        .preview(PackageOperation::Install(vec!["broken".into()]))
        .unwrap();
    fixture.world.fail(
        "install:other",
        ProvisionError::CommandFailed(CommandFailure {
            command: "uv pip install broken".into(),
            exit_code: Some(1),
            tail: Vec::new(),
        }),
    );
    assert!(matches!(
        fixture.apply(&plan, Vec::new()),
        Err(EnvironmentAppError::Provision(
            ProvisionError::CommandFailed(_)
        ))
    ));
    assert_eq!(
        fixture.world.env("env-old").state(),
        EnvState::RepairRequired
    );
    assert_eq!(
        fixture.world.active_id().as_deref(),
        Some("env-old"),
        "the environment stays selected"
    );
    // Previews still work on a repair-required environment, so the user can fix it.
    fixture.queue_preview(Ok(installs(&[("fixed", "1.0")])));
    assert!(
        fixture
            .preview(PackageOperation::Install(vec!["fixed".into()]))
            .is_ok()
    );
}

#[test]
fn unapproved_drift_of_a_protected_package_marks_the_environment_repair_required() {
    let fixture = fixture();
    fixture.queue_preview(Ok(installs(&[("sneaky", "1.0")])));
    let plan = fixture
        .preview(PackageOperation::Install(vec!["sneaky".into()]))
        .unwrap();
    fixture
        .world
        .install_overrides
        .lock()
        .unwrap()
        .push(("torch".into(), "2.15.0".into()));
    assert!(matches!(
        fixture.apply(&plan, Vec::new()),
        Err(EnvironmentAppError::PinDrift)
    ));
    assert_eq!(
        fixture.world.env("env-old").state(),
        EnvState::RepairRequired
    );
    assert_eq!(
        fixture.world.env("env-old").pins.get(&name("torch")),
        Some("2.14.1+cu130"),
        "the recorded pins are not rewritten by an unapproved change"
    );
}

#[test]
fn a_plan_is_stale_once_the_environment_changed() {
    let fixture = fixture();
    fixture.queue_preview(Ok(installs(&[("requests", "2.34.2")])));
    let plan = fixture
        .preview(PackageOperation::Install(vec!["requests".into()]))
        .unwrap();
    fixture.world.put_installed("torch", "2.15.0");
    assert!(matches!(
        fixture.apply(&plan, Vec::new()),
        Err(EnvironmentAppError::PlanStale)
    ));
    assert!(fixture.world.installs.lock().unwrap().is_empty());
}

#[test]
fn uninstalling_a_protected_package_needs_confirmation_and_leaves_repair_required() {
    let fixture = fixture();
    let plan = fixture
        .preview(PackageOperation::Uninstall(vec!["torch".into()]))
        .unwrap();
    assert_eq!(plan.resolution, PlanResolution::Removal);
    assert_eq!(
        plan.protected_changes,
        vec![PinChange {
            name: name("torch"),
            from: Some("2.14.1+cu130".into()),
            to: None
        }]
    );
    assert!(matches!(
        fixture.apply(&plan, Vec::new()),
        Err(EnvironmentAppError::ConfirmationMismatch)
    ));
    assert_eq!(fixture.world.count("uninstall"), 0);
    let outcome = fixture
        .apply(&plan, plan.protected_changes.clone())
        .unwrap();
    assert_eq!(
        outcome.env_state,
        EnvState::RepairRequired,
        "a removed core package cannot stay runnable"
    );
    assert_eq!(fixture.world.count("uninstall"), 1);

    // An ordinary package is removed without confirmation.
    let fixture = fixture_with_clean_numpy();
    let plan = fixture
        .preview(PackageOperation::Uninstall(vec!["numpy".into()]))
        .unwrap();
    assert!(!plan.requires_confirmation());
    assert_eq!(
        fixture.apply(&plan, Vec::new()).unwrap().env_state,
        EnvState::Verified
    );
}

fn fixture_with_clean_numpy() -> Fixture {
    fixture()
}

#[test]
fn mutations_wait_for_the_gate_and_a_stopped_runtime() {
    let fixture = fixture();
    fixture.queue_preview(Ok(installs(&[("requests", "2.34.2")])));
    let plan = fixture
        .preview(PackageOperation::Install(vec!["requests".into()]))
        .unwrap();

    fixture.world.gate.busy.store(true, Ordering::SeqCst);
    assert!(matches!(
        fixture.apply(&plan, Vec::new()),
        Err(EnvironmentAppError::Gate(crate::shared::GateError::Busy))
    ));
    fixture.world.gate.busy.store(false, Ordering::SeqCst);

    *fixture.world.runtime_block.lock().unwrap() = Some(RuntimeBlock::Running);
    assert!(matches!(
        fixture.apply(&plan, Vec::new()),
        Err(EnvironmentAppError::Runtime(RuntimeBlock::Running))
    ));
    assert!(fixture.world.installs.lock().unwrap().is_empty());

    // The plan survives refused attempts.
    *fixture.world.runtime_block.lock().unwrap() = None;
    assert!(fixture.apply(&plan, Vec::new()).is_ok());
}

#[test]
fn invalid_package_requests_are_rejected_before_resolution() {
    let fixture = fixture();
    let bad = [
        PackageOperation::Install(Vec::new()),
        PackageOperation::Install(vec!["".into()]),
        PackageOperation::Install(vec!["--index-url https://evil.example".into()]),
        PackageOperation::Install(vec!["pkg\nother".into()]),
        PackageOperation::Upgrade(vec!["numpy>=1".into()]),
        PackageOperation::Uninstall(vec!["bad name".into()]),
    ];
    for operation in bad {
        assert!(
            matches!(
                fixture.preview(operation.clone()),
                Err(EnvironmentAppError::PackageSpecInvalid)
            ),
            "{operation:?}"
        );
    }
    assert!(fixture.world.previews.lock().unwrap().is_empty());
    assert!(
        !fixture
            .world
            .calls()
            .iter()
            .any(|call| call.starts_with("preview"))
    );
}

#[test]
fn package_changes_need_a_usable_selected_environment() {
    let world = World::new();
    let deps = world.deps();
    assert!(matches!(
        block_on(
            PreviewPackageChange::new(deps).execute(PackageOperation::Install(vec!["x".into()]))
        ),
        Err(EnvironmentAppError::NoActiveEnvironment)
    ));
}

#[test]
fn mutation_refuses_to_start_if_the_recovery_state_cannot_be_saved() {
    let fixture = fixture();
    let plan = fixture
        .preview(PackageOperation::Install(vec!["requests".into()]))
        .unwrap();
    fixture.world.fail_repo("env_save");
    assert!(matches!(
        fixture.apply(&plan, Vec::new()),
        Err(EnvironmentAppError::Repository(_))
    ));
    assert!(fixture.world.installs.lock().unwrap().is_empty());
}

#[test]
fn lost_success_commit_leaves_the_durable_environment_needing_repair() {
    let fixture = fixture();
    let plan = fixture
        .preview(PackageOperation::Install(vec!["requests".into()]))
        .unwrap();
    fixture.world.succeed_first("env_save", 1);
    fixture.world.fail_repo("env_save");
    assert!(matches!(
        fixture.apply(&plan, Vec::new()),
        Err(EnvironmentAppError::Repository(_))
    ));
    assert_eq!(fixture.world.installs.lock().unwrap().len(), 1);
    assert_eq!(
        fixture.world.env("env-old").state(),
        EnvState::RepairRequired
    );
}

#[test]
fn failed_post_install_verification_keeps_the_repair_state() {
    let fixture = fixture();
    let plan = fixture
        .preview(PackageOperation::Install(vec!["requests".into()]))
        .unwrap();
    fixture
        .world
        .verify_final_fails
        .store(true, Ordering::SeqCst);
    assert!(fixture.apply(&plan, Vec::new()).is_err());
    assert_eq!(
        fixture.world.env("env-old").state(),
        EnvState::RepairRequired
    );
}

#[test]
fn successful_packages_do_not_clear_an_unrelated_repair_state() {
    let fixture = fixture();
    let mut env = fixture.world.env("env-old");
    env.mark_repair_required("2026-10-16T00:00:00Z").unwrap();
    block_on(fixture.deps.envs.save(&env)).unwrap();
    let plan = fixture
        .preview(PackageOperation::Install(vec!["requests".into()]))
        .unwrap();
    assert_eq!(
        fixture.apply(&plan, Vec::new()).unwrap().env_state,
        EnvState::RepairRequired
    );
    assert_eq!(fixture.world.count("verify_final"), 0);
}
