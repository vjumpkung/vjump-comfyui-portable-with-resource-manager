use std::sync::Arc;

use domain::environment::EnvState;

use super::{
    fakes::*,
    setup::{execute, register, request},
};
use crate::{environment::*, test_support::block_on};

fn reconcile(world: &Arc<World>) -> Result<ReconcileReport, EnvironmentAppError> {
    block_on(ReconcileStartup::new(world.deps()).execute())
}

fn assert_activated(world: &Arc<World>, run_id: &RunId) {
    let run = world.run(run_id);
    assert_eq!(run.activation, ActivationPhase::Committed);
    assert_eq!(run.status, RunStatus::Succeeded);
    assert_eq!(world.active_id().as_deref(), Some(run.target_env.as_str()));
    match &*world.manifest.lock().unwrap() {
        ManifestState::Valid(manifest) => assert_eq!(manifest.env_id, run.target_env),
        other => panic!("manifest must name the new environment: {other:?}"),
    }
    assert_eq!(
        world.env(run.target_env.as_str()).state(),
        EnvState::Verified
    );
}

/// Runs setup with one injected failure and returns the run id plus the failed outcome.
fn crashed_run(world: &Arc<World>) -> RunId {
    let run_id = register(world, request()).unwrap();
    let (outcome, _) = execute(world, &run_id);
    let RunOutcome::Failed(failure) = outcome else {
        panic!("the injected failure must stop the run")
    };
    assert_eq!(failure.step, StepId::FinalVerify);
    run_id
}

#[test]
fn failure_before_the_intent_is_recorded_changes_nothing_and_retry_activates() {
    let world = World::new();
    world.fail_repo("run_save:intent");
    let run_id = crashed_run(&world);
    let run = world.run(&run_id);
    assert_eq!(run.activation, ActivationPhase::None);
    assert!(world.active_id().is_none());
    assert_eq!(*world.manifest.lock().unwrap(), ManifestState::Absent);
    // The candidate itself verified; only the activation did not start.
    assert_eq!(
        world.env(run.target_env.as_str()).state(),
        EnvState::Verified
    );

    block_on(StartSetup::new(world.deps()).execute(SetupAction::Retry {
        run_id: run_id.clone(),
        manager_consent: None,
        curated_decisions: Vec::new(),
    }))
    .unwrap();
    let (outcome, _) = execute(&world, &run_id);
    assert_eq!(outcome, RunOutcome::Succeeded);
    assert_activated(&world, &run_id);
}

#[test]
fn restart_after_the_intent_rolls_the_activation_forward() {
    let world = World::new();
    world.fail_repo("publish_manifest");
    let run_id = crashed_run(&world);
    assert_eq!(world.run(&run_id).activation, ActivationPhase::Intent);
    assert!(world.active_id().is_none());
    assert_eq!(*world.manifest.lock().unwrap(), ManifestState::Absent);

    let report = reconcile(&world).unwrap();
    assert_eq!(report.activations_completed, 1);
    assert_activated(&world, &run_id);
}

#[test]
fn restart_after_the_manifest_was_replaced_but_before_the_phase_was_saved() {
    let world = World::new();
    world.fail_repo("run_save:manifest_published");
    let run_id = crashed_run(&world);
    assert_eq!(world.run(&run_id).activation, ActivationPhase::Intent);
    assert!(
        matches!(&*world.manifest.lock().unwrap(), ManifestState::Valid(_)),
        "the manifest was already replaced"
    );
    assert!(world.active_id().is_none());

    reconcile(&world).unwrap();
    assert_activated(&world, &run_id);
    assert_eq!(
        world.count("publish_manifest"),
        2,
        "publishing is idempotent"
    );
}

#[test]
fn restart_after_the_manifest_phase_but_before_the_database_switch() {
    let world = World::new();
    world.fail_repo("commit_activation");
    let run_id = crashed_run(&world);
    assert_eq!(
        world.run(&run_id).activation,
        ActivationPhase::ManifestPublished
    );
    assert!(world.active_id().is_none());

    let report = reconcile(&world).unwrap();
    assert_eq!(report.activations_completed, 1);
    assert_activated(&world, &run_id);
    // Nothing is left to do on the next start.
    assert_eq!(reconcile(&world).unwrap(), ReconcileReport::default());
}

#[test]
fn a_retry_instead_of_a_restart_also_resumes_from_the_persisted_phase() {
    let world = World::new();
    world.fail_repo("publish_manifest");
    let run_id = crashed_run(&world);
    block_on(StartSetup::new(world.deps()).execute(SetupAction::Retry {
        run_id: run_id.clone(),
        manager_consent: None,
        curated_decisions: Vec::new(),
    }))
    .unwrap();
    let (outcome, _) = execute(&world, &run_id);
    assert_eq!(outcome, RunOutcome::Succeeded);
    assert_activated(&world, &run_id);
    assert_eq!(
        world.envs.lock().unwrap().len(),
        1,
        "no duplicate environment"
    );
}

#[test]
fn a_previous_active_environment_is_only_replaced_by_the_commit() {
    let world = World::new();
    world.seed_active_env(
        "env-old",
        domain::environment::PythonVersion::new(3, 13, 16),
    );
    world.installed.lock().unwrap().clear();
    *world.checkout.lock().unwrap() = Checkout::Repo {
        commit: "abc1230000000000000000000000000000000000".into(),
    };
    world.fail_repo("commit_activation");
    let run_id = register(
        &world,
        SetupRequest {
            mode: domain::environment::SetupMode::ChangeTorch,
            comfyui_ref: ComfyRef::KeepCurrent,
            ..request()
        },
    )
    .unwrap();
    let (outcome, _) = execute(&world, &run_id);
    assert!(matches!(outcome, RunOutcome::Failed(_)));
    assert_eq!(world.active_id().as_deref(), Some("env-old"));
    assert_eq!(world.run(&run_id).previous_env.unwrap().as_str(), "env-old");

    reconcile(&world).unwrap();
    assert_activated(&world, &run_id);
    assert_eq!(
        world.env("env-old").state(),
        EnvState::Verified,
        "the old env is kept"
    );
    match &*world.manifest.lock().unwrap() {
        ManifestState::Valid(manifest) => assert_eq!(manifest.generation, 1),
        other => panic!("{other:?}"),
    }
}

#[test]
fn stale_active_runs_become_retryable_after_a_restart() {
    let world = World::new();
    let run_id = register(&world, request()).unwrap();
    {
        let mut runs = world.runs.lock().unwrap();
        let run = runs.get_mut(run_id.as_str()).unwrap();
        run.status = RunStatus::Running;
        run.step_mut(StepId::Tools).status = StepStatus::Running;
    }
    let report = reconcile(&world).unwrap();
    assert_eq!(report.interrupted_runs, 1);
    let run = world.run(&run_id);
    assert_eq!(run.status, RunStatus::Interrupted);
    assert_eq!(run.step(StepId::Tools).status, StepStatus::Interrupted);
    assert_eq!(run.failure.as_ref().unwrap().kind, FailureKind::Interrupted);
    assert_eq!(run.failure.as_ref().unwrap().step, StepId::Tools);

    block_on(StartSetup::new(world.deps()).execute(SetupAction::Retry {
        run_id: run_id.clone(),
        manager_consent: None,
        curated_decisions: Vec::new(),
    }))
    .unwrap();
    let (outcome, _) = execute(&world, &run_id);
    assert_eq!(outcome, RunOutcome::Succeeded);
}

#[test]
fn an_activation_whose_environment_is_gone_is_abandoned_not_forced() {
    let world = World::new();
    world.fail_repo("publish_manifest");
    let run_id = crashed_run(&world);
    let target = world.run(&run_id).target_env;
    world.envs.lock().unwrap().remove(target.as_str());
    let report = reconcile(&world).unwrap();
    assert_eq!(report.activations_abandoned, 1);
    assert!(world.active_id().is_none());
    assert_eq!(world.run(&run_id).activation, ActivationPhase::None);
}

#[test]
fn roll_forward_never_replaces_an_unreadable_manifest() {
    let world = World::new();
    world.fail_repo("publish_manifest");
    let run_id = crashed_run(&world);
    *world.manifest.lock().unwrap() = ManifestState::Corrupt;
    assert!(matches!(
        reconcile(&world),
        Err(EnvironmentAppError::RecoveryRequired)
    ));
    assert_eq!(*world.manifest.lock().unwrap(), ManifestState::Corrupt);
    assert!(world.active_id().is_none());
    assert_eq!(world.run(&run_id).activation, ActivationPhase::Intent);
}
