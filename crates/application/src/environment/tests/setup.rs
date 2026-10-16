use std::{path::PathBuf, sync::Arc};

use domain::environment::{
    EnvState, EnvironmentError, IneligibleReason, Ownership, PythonSourceKind, PythonVersion,
    SetupMode,
};

use super::fakes::*;
use crate::{comfy_runtime::RuntimeBlock, environment::*, test_support::block_on};

pub fn request() -> SetupRequest {
    SetupRequest {
        mode: SetupMode::FirstRun,
        install_root: PathBuf::from(ROOT),
        python_source: PythonSourceInput::Managed,
        profile_id: "win-py313-torch2141-cu130".into(),
        comfyui_ref: ComfyRef::Latest,
        install_manager: false,
        clean_cache: false,
        confirm_adopted_changes: false,
        allow_source_builds: false,
    }
}

fn resetup_request(mode: SetupMode) -> SetupRequest {
    SetupRequest {
        mode,
        comfyui_ref: ComfyRef::KeepCurrent,
        ..request()
    }
}

pub fn register(world: &Arc<World>, request: SetupRequest) -> Result<RunId, EnvironmentAppError> {
    block_on(StartSetup::new(world.deps()).execute(SetupAction::New(Box::new(request))))
}

pub fn execute(world: &Arc<World>, run_id: &RunId) -> (RunOutcome, Arc<RecordingSink>) {
    let sink = Arc::new(RecordingSink::default());
    let outcome = block_on(ExecuteSetup::new(world.deps()).execute(run_id, sink.clone()))
        .expect("the run must execute");
    (outcome, sink)
}

fn validation(
    target: ValidationTarget,
    path: &str,
    version: PythonVersion,
    headers: bool,
) -> PythonValidationReport {
    PythonValidationReport {
        target,
        resolved_path: PathBuf::from(path),
        python_version: Some(version),
        checks: vec![ValidationCheck {
            id: CheckId::DevelopmentFiles,
            passed: headers,
            detail: "development files".into(),
            blocks: if headers {
                Vec::new()
            } else {
                vec![BlockedFeature::Triton]
            },
        }],
    }
}

#[test]
fn first_run_executes_the_steps_in_order_and_activates() {
    let world = World::new();
    let run_id = register(&world, request()).unwrap();
    let (outcome, sink) = execute(&world, &run_id);
    assert_eq!(outcome, RunOutcome::Succeeded);

    let order = [
        "ensure_stopped",
        "ensure_uv",
        "ensure_git",
        "ensure_hf_tool",
        "install_managed_python",
        "create_venv",
        "seed",
        "clone_comfyui",
        "install:torch",
        "verify_torch",
        "install:triton",
        "verify_triton",
        "install:requirements",
        "verify_final",
        "publish_manifest",
        "commit_activation",
    ];
    let positions: Vec<usize> = order.iter().map(|call| world.position(call)).collect();
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "unexpected order: {:?}",
        world.calls()
    );

    let run = world.run(&run_id);
    assert_eq!(run.status, RunStatus::Succeeded);
    assert_eq!(run.activation, ActivationPhase::Committed);
    assert_eq!(world.active_id().as_deref(), Some(run.target_env.as_str()));
    let env = world.env(run.target_env.as_str());
    assert_eq!(env.state(), EnvState::Verified);
    assert_eq!(env.ownership, Ownership::AppOwned);
    assert_eq!(
        env.pins.len(),
        3,
        "torch, torchvision and triton-windows are pinned"
    );
    match &*world.manifest.lock().unwrap() {
        ManifestState::Valid(manifest) => {
            assert_eq!(manifest.env_id, run.target_env);
            assert_eq!(manifest.comfyui_reference.as_deref(), Some("v0.40.0"));
            assert_eq!(manifest.generation, 1);
        }
        other => panic!("manifest not written: {other:?}"),
    }
    // Deferred and not requested steps are reported as skipped, not silently dropped.
    for step in [
        StepId::Accelerators,
        StepId::Manager,
        StepId::CuratedNodes,
        StepId::CacheClean,
    ] {
        assert_eq!(run.step(step).status, StepStatus::Skipped, "{step:?}");
    }
    assert!(!sink.0.lock().unwrap().is_empty());
}

#[test]
fn the_gate_is_acquired_exactly_once_for_the_whole_run() {
    let world = World::new();
    let run_id = register(&world, request()).unwrap();
    execute(&world, &run_id);
    assert_eq!(
        world
            .gate
            .acquires
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert_eq!(
        world
            .gate
            .try_acquires
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );
    assert!(!world.gate.busy.load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn a_running_comfyui_blocks_before_any_mutation() {
    let world = World::new();
    *world.runtime_block.lock().unwrap() = Some(RuntimeBlock::Running);
    let run_id = register(&world, request()).unwrap();
    let (outcome, _) = execute(&world, &run_id);
    let RunOutcome::Failed(failure) = outcome else {
        panic!("expected a failure")
    };
    assert_eq!(failure.step, StepId::Preflight);
    assert!(world.envs.lock().unwrap().is_empty());
    assert!(!world.calls().contains(&"ensure_uv".to_owned()));
}

#[test]
fn a_required_failure_keeps_the_previous_environment_and_retry_resumes_the_same_run() {
    let world = World::new();
    world.seed_active_env("env-old", PythonVersion::new(3, 13, 16));
    // The replacement environment starts empty.
    world.installed.lock().unwrap().clear();
    *world.checkout.lock().unwrap() = Checkout::Repo {
        commit: "abc1230000000000000000000000000000000000".into(),
    };
    let run_id = register(&world, resetup_request(SetupMode::ChangeTorch)).unwrap();
    world.fail(
        "install:torch",
        ProvisionError::CommandFailed(CommandFailure {
            command: "uv pip install torch".into(),
            exit_code: Some(2),
            tail: vec!["network down".into()],
        }),
    );
    let (outcome, _) = execute(&world, &run_id);
    let RunOutcome::Failed(failure) = outcome else {
        panic!("expected a failure")
    };
    assert_eq!(failure.step, StepId::InstallTorch);
    assert_eq!(failure.kind, FailureKind::CommandFailed);
    assert_eq!(failure.exit_code, Some(2));
    assert_eq!(failure.tail, vec!["network down".to_owned()]);

    let target = world.run(&run_id).target_env;
    assert_eq!(world.active_id().as_deref(), Some("env-old"));
    assert_eq!(world.env(target.as_str()).state(), EnvState::Failed);
    assert_eq!(world.envs.lock().unwrap().len(), 2);

    // Retry resumes the same run and the same target environment.
    let retried = block_on(StartSetup::new(world.deps()).execute(SetupAction::Retry {
        run_id: run_id.clone(),
        manager_consent: None,
    }))
    .unwrap();
    assert_eq!(retried, run_id);
    let (outcome, _) = execute(&world, &run_id);
    assert_eq!(outcome, RunOutcome::Succeeded);
    assert_eq!(
        world.envs.lock().unwrap().len(),
        2,
        "no duplicate environment"
    );
    assert_eq!(world.active_id().as_deref(), Some(target.as_str()));
    assert_eq!(world.run(&run_id).step(StepId::InstallTorch).attempts, 2);
}

#[test]
fn latest_is_resolved_once_and_survives_a_retry() {
    let world = World::new();
    let run_id = register(&world, request()).unwrap();
    world.fail("install:torch", ProvisionError::Failed("offline".into()));
    execute(&world, &run_id);
    block_on(StartSetup::new(world.deps()).execute(SetupAction::Retry {
        run_id: run_id.clone(),
        manager_consent: None,
    }))
    .unwrap();
    // After the failed attempt the clone is already in place; the retry reuses the same commit.
    let (outcome, _) = execute(&world, &run_id);
    assert_eq!(outcome, RunOutcome::Succeeded);
    assert_eq!(
        world
            .resolve_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert_eq!(world.count("clone_comfyui"), 1);
}

#[test]
fn cancellation_holds_the_gate_through_cleanup_and_leaves_the_old_environment_active() {
    let world = World::new();
    world.seed_active_env("env-old", PythonVersion::new(3, 13, 16));
    // The replacement environment starts empty.
    world.installed.lock().unwrap().clear();
    *world.checkout.lock().unwrap() = Checkout::Repo {
        commit: "abc1230000000000000000000000000000000000".into(),
    };
    let run_id = register(&world, resetup_request(SetupMode::ChangeTorch)).unwrap();
    world.fail("install:torch", ProvisionError::Cancelled);
    let (outcome, _) = execute(&world, &run_id);
    assert_eq!(outcome, RunOutcome::Cancelled);
    assert_eq!(
        *world.gate_busy_at_cancel_cleanup.lock().unwrap(),
        Some(true),
        "the gate must still be held while the adapter cleans up"
    );
    assert!(!world.gate.busy.load(std::sync::atomic::Ordering::SeqCst));
    let run = world.run(&run_id);
    assert_eq!(run.status, RunStatus::Cancelled);
    assert_eq!(world.active_id().as_deref(), Some("env-old"));
    assert_eq!(world.env(run.target_env.as_str()).state(), EnvState::Failed);
    assert_eq!(run.activation, ActivationPhase::None);
}

#[test]
fn cancel_setup_signals_a_registered_run_before_any_step_runs() {
    let world = World::new();
    let deps = world.deps();
    let run_id =
        block_on(StartSetup::new(deps.clone()).execute(SetupAction::New(Box::new(request()))))
            .unwrap();
    let token = deps.active_runs.get(&run_id).unwrap();
    block_on(CancelSetup::new(deps.clone()).execute(&run_id)).unwrap();
    assert!(token.is_cancelled());
    let outcome = block_on(
        ExecuteSetup::new(deps.clone()).execute(&run_id, Arc::new(RecordingSink::default())),
    )
    .unwrap();
    assert_eq!(outcome, RunOutcome::Cancelled);
    assert_eq!(
        world.count("ensure_uv"),
        0,
        "no step ran after cancellation"
    );
    assert!(world.run(&run_id).cancel_requested);
}

#[test]
fn cancel_setup_aborts_a_failed_run_without_taking_the_gate() {
    let world = World::new();
    world.fail("seed", ProvisionError::Failed("boom".into()));
    let deps = world.deps();
    let run_id =
        block_on(StartSetup::new(deps.clone()).execute(SetupAction::New(Box::new(request()))))
            .unwrap();
    let outcome = block_on(
        ExecuteSetup::new(deps.clone()).execute(&run_id, Arc::new(RecordingSink::default())),
    )
    .unwrap();
    assert!(matches!(outcome, RunOutcome::Failed(_)));
    let acquires = world
        .gate
        .acquires
        .load(std::sync::atomic::Ordering::SeqCst);
    let cancel = CancelSetup::new(deps.clone());
    block_on(cancel.execute(&run_id)).unwrap();
    let run = world.run(&run_id);
    assert_eq!(run.status, RunStatus::Cancelled);
    assert_eq!(world.env(run.target_env.as_str()).state(), EnvState::Failed);
    assert_eq!(
        world
            .gate
            .acquires
            .load(std::sync::atomic::Ordering::SeqCst),
        acquires
    );
    assert_eq!(
        world
            .gate
            .try_acquires
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );
    assert!(matches!(
        block_on(cancel.execute(&run_id)),
        Err(EnvironmentAppError::NotCancellable)
    ));
}

#[test]
fn only_one_run_may_be_active_at_a_time() {
    let world = World::new();
    let deps = world.deps();
    let start = StartSetup::new(deps);
    block_on(start.execute(SetupAction::New(Box::new(request())))).unwrap();
    assert!(matches!(
        block_on(start.execute(SetupAction::New(Box::new(request())))),
        Err(EnvironmentAppError::SetupInProgress)
    ));
}

#[test]
fn disabled_profiles_cannot_be_selected() {
    let world = World::new();
    world.hardware.lock().unwrap().driver_cuda = Some(domain::environment::CudaVersion::new(12, 9));
    match register(&world, request()) {
        Err(EnvironmentAppError::ProfileDisabled(reasons)) => {
            assert!(matches!(reasons[0], IneligibleReason::DriverTooOld { .. }));
        }
        other => panic!("expected a disabled profile, got {other:?}"),
    }
    assert!(world.runs.lock().unwrap().is_empty());
    assert!(matches!(
        register(
            &world,
            SetupRequest {
                profile_id: "nope".into(),
                ..request()
            }
        ),
        Err(EnvironmentAppError::ProfileNotFound)
    ));
}

#[test]
fn an_unreadable_manifest_refuses_a_new_run() {
    let world = World::new();
    *world.manifest.lock().unwrap() = ManifestState::Corrupt;
    assert!(matches!(
        register(&world, request()),
        Err(EnvironmentAppError::RecoveryRequired)
    ));
    *world.manifest.lock().unwrap() = ManifestState::Newer;
    assert!(matches!(
        register(&world, request()),
        Err(EnvironmentAppError::RecoveryRequired)
    ));
}

#[test]
fn optional_manager_step_needs_consent_for_the_resolved_commit() {
    let world = World::new();
    let run_id = register(
        &world,
        SetupRequest {
            install_manager: true,
            ..request()
        },
    )
    .unwrap();
    let (outcome, _) = execute(&world, &run_id);
    let RunOutcome::Failed(failure) = outcome else {
        panic!("expected a consent pause")
    };
    assert_eq!(failure.step, StepId::Manager);
    assert_eq!(failure.kind, FailureKind::ConsentRequired);
    let consent = failure.consent.unwrap();
    assert!(consent.commit.starts_with("mgr"));
    assert!(!world.calls().contains(&"install_manager".to_owned()));
    // An optional failure does not fail the environment.
    let target = world.run(&run_id).target_env;
    assert_eq!(world.env(target.as_str()).state(), EnvState::Provisioning);

    block_on(StartSetup::new(world.deps()).execute(SetupAction::Retry {
        run_id: run_id.clone(),
        manager_consent: Some(consent.commit),
    }))
    .unwrap();
    let (outcome, _) = execute(&world, &run_id);
    assert_eq!(outcome, RunOutcome::Succeeded);
    assert_eq!(world.count("install_manager"), 1);
    assert_eq!(
        world.run(&run_id).step(StepId::Manager).status,
        StepStatus::Succeeded
    );
}

#[test]
fn a_failed_optional_step_can_be_skipped_and_required_ones_cannot() {
    let world = World::new();
    let run_id = register(
        &world,
        SetupRequest {
            install_manager: true,
            ..request()
        },
    )
    .unwrap();
    world.fail(
        "install_manager",
        ProvisionError::Failed("clone failed".into()),
    );
    // Pre-approve the Manager commit so the failure comes from the install itself.
    {
        let mut runs = world.runs.lock().unwrap();
        runs.get_mut(run_id.as_str()).unwrap().manager_consent =
            Some("mgr0000000000000000000000000000000000001".into());
    }
    let (outcome, _) = execute(&world, &run_id);
    assert!(matches!(&outcome, RunOutcome::Failed(f) if f.step == StepId::Manager));
    assert!(matches!(
        block_on(
            StartSetup::new(world.deps()).execute(SetupAction::SkipStep {
                run_id: run_id.clone(),
                step: StepId::InstallTorch
            })
        ),
        Err(EnvironmentAppError::StepNotSkippable)
    ));
    block_on(
        StartSetup::new(world.deps()).execute(SetupAction::SkipStep {
            run_id: run_id.clone(),
            step: StepId::Manager,
        }),
    )
    .unwrap();
    let (outcome, _) = execute(&world, &run_id);
    assert_eq!(outcome, RunOutcome::Succeeded);
    assert_eq!(
        world.run(&run_id).step(StepId::Manager).status,
        StepStatus::Skipped
    );
}

#[test]
fn best_effort_failures_are_warnings() {
    let world = World::new();
    world.fail("clean_cache", ProvisionError::Failed("locked".into()));
    let run_id = register(
        &world,
        SetupRequest {
            clean_cache: true,
            ..request()
        },
    )
    .unwrap();
    let (outcome, _) = execute(&world, &run_id);
    assert_eq!(outcome, RunOutcome::Succeeded);
    assert_eq!(
        world.run(&run_id).step(StepId::CacheClean).status,
        StepStatus::Warning
    );
}

#[test]
fn accelerators_are_best_effort_when_a_profile_defines_them() {
    let world = World::new();
    world.profiles.lock().unwrap()[0].accelerators = vec![accelerator("sageattention")];
    let run_id = register(&world, request()).unwrap();
    let (outcome, _) = execute(&world, &run_id);
    assert_eq!(outcome, RunOutcome::Succeeded);
    assert_eq!(
        world.run(&run_id).step(StepId::Accelerators).status,
        StepStatus::Succeeded
    );
}

#[test]
fn a_pin_change_during_requirements_fails_the_step_and_the_new_environment() {
    let world = World::new();
    world
        .drift_on_requirements
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let run_id = register(&world, request()).unwrap();
    let (outcome, _) = execute(&world, &run_id);
    let RunOutcome::Failed(failure) = outcome else {
        panic!("expected a pin failure")
    };
    assert_eq!(failure.step, StepId::ComfyRequirements);
    assert!(failure.message.contains("torch"));
    let target = world.run(&run_id).target_env;
    assert_eq!(world.env(target.as_str()).state(), EnvState::Failed);
    assert!(world.active_id().is_none());
}

#[test]
fn installs_after_torch_use_the_constraints_file() {
    let world = World::new();
    let run_id = register(&world, request()).unwrap();
    execute(&world, &run_id);
    let installs = world.installs.lock().unwrap().clone();
    assert!(
        installs[0].constraint_file.is_none(),
        "torch itself is unconstrained"
    );
    assert!(installs[0].index_url.is_some());
    for spec in &installs[1..] {
        assert!(spec.constraint_file.is_some(), "{spec:?}");
    }
    // The last constraints rewrite holds the complete pin set including Triton.
    let writes = world.constraint_writes.lock().unwrap();
    assert_eq!(writes.last().unwrap().len(), 3);
}

#[test]
fn replacement_environments_receive_existing_node_requirements_under_the_pin_guard() {
    let world = World::new();
    world.seed_active_env("env-old", PythonVersion::new(3, 13, 16));
    *world.checkout.lock().unwrap() = Checkout::Repo {
        commit: "abc1230000000000000000000000000000000000".into(),
    };
    world.node_requirements.lock().unwrap().extend([
        PathBuf::from("/install/ComfyUI/custom_nodes/good/requirements.txt"),
        PathBuf::from("/install/ComfyUI/custom_nodes/bad/requirements.txt"),
    ]);
    let run_id = register(&world, resetup_request(SetupMode::ChangeTorch)).unwrap();
    // The first node installs; the second one fails and only produces a warning.
    world.succeed_first("install:node", 1);
    world.fail("install:node", ProvisionError::Failed("no wheel".into()));
    let (outcome, _) = execute(&world, &run_id);
    assert_eq!(outcome, RunOutcome::Succeeded);
    let installs = world.installs.lock().unwrap().clone();
    let node_installs: Vec<_> = installs
        .iter()
        .filter(|spec| {
            spec.requirement_files
                .iter()
                .any(|file| file.to_string_lossy().contains("custom_nodes"))
        })
        .collect();
    assert_eq!(node_installs.len(), 2);
    assert!(
        node_installs
            .iter()
            .all(|spec| spec.constraint_file.is_some())
    );
    let step = world.run(&run_id).step(StepId::ComfyRequirements).clone();
    assert_eq!(step.status, StepStatus::Warning);
    assert!(step.message.unwrap().contains("bad"));
}

#[test]
fn an_existing_checkout_at_another_commit_or_without_git_is_never_altered() {
    let world = World::new();
    *world.checkout.lock().unwrap() = Checkout::Repo {
        commit: "deadbeef00000000000000000000000000000000".into(),
    };
    let run_id = register(&world, request()).unwrap();
    let (outcome, _) = execute(&world, &run_id);
    let RunOutcome::Failed(failure) = outcome else {
        panic!("expected a mismatch failure")
    };
    assert_eq!(failure.step, StepId::CloneComfyUi);
    assert!(failure.message.contains("left unchanged"));
    assert_eq!(world.count("clone_comfyui"), 0);

    let world = World::new();
    *world.checkout.lock().unwrap() = Checkout::NotGit;
    let run_id = register(&world, request()).unwrap();
    let (outcome, _) = execute(&world, &run_id);
    assert!(matches!(&outcome, RunOutcome::Failed(f) if f.step == StepId::CloneComfyUi));
    assert_eq!(world.count("clone_comfyui"), 0);
}

// ----------------------------------------------------------------- Python source selection

#[test]
fn an_empty_custom_folder_creates_an_app_owned_venv_there_with_the_managed_python() {
    let world = World::new();
    *world.validation.lock().unwrap() = Some(validation(
        ValidationTarget::EmptyFolder,
        "/custom/venv",
        PythonVersion::new(3, 13, 16),
        true,
    ));
    let run_id = register(
        &world,
        SetupRequest {
            python_source: PythonSourceInput::CustomVenv("/custom/venv".into()),
            ..request()
        },
    )
    .unwrap();
    let run = world.run(&run_id);
    assert_eq!(run.python_target.ownership, Ownership::AppOwned);
    assert_eq!(run.python_target.source, PythonSourceKind::CustomVenv);
    assert!(run.python_target.create_venv);
    let (outcome, _) = execute(&world, &run_id);
    assert_eq!(outcome, RunOutcome::Succeeded);
    assert!(
        world
            .calls()
            .contains(&"create_venv_at:/custom/venv".to_owned())
    );
    assert_eq!(world.count("install_managed_python"), 1);
    assert_eq!(
        world.env(run.target_env.as_str()).ownership,
        Ownership::AppOwned
    );
}

#[test]
fn an_existing_venv_is_adopted_after_explicit_confirmation_and_never_recreated() {
    let world = World::new();
    *world.validation.lock().unwrap() = Some(validation(
        ValidationTarget::ExistingVenv,
        "/custom/existing",
        PythonVersion::new(3, 13, 4),
        true,
    ));
    let source = PythonSourceInput::CustomVenv("/custom/existing".into());
    assert!(matches!(
        register(
            &world,
            SetupRequest {
                python_source: source.clone(),
                ..request()
            }
        ),
        Err(EnvironmentAppError::Domain(
            EnvironmentError::AdoptedRebuildNeedsConfirmation
        ))
    ));
    let run_id = register(
        &world,
        SetupRequest {
            python_source: source,
            confirm_adopted_changes: true,
            ..request()
        },
    )
    .unwrap();
    let (outcome, _) = execute(&world, &run_id);
    assert_eq!(outcome, RunOutcome::Succeeded);
    assert_eq!(world.count("create_venv"), 0);
    assert_eq!(world.count("inspect_venv"), 1);
    let env = world.env(world.run(&run_id).target_env.as_str());
    assert_eq!(env.ownership, Ownership::Adopted);
    assert!(env.check_deletable(false).is_err());
}

#[test]
fn a_custom_interpreter_creates_a_new_app_owned_venv_under_envs() {
    let world = World::new();
    *world.validation.lock().unwrap() = Some(validation(
        ValidationTarget::Interpreter,
        "/opt/python3.13/python",
        PythonVersion::new(3, 13, 2),
        true,
    ));
    let run_id = register(
        &world,
        SetupRequest {
            python_source: PythonSourceInput::CustomInterpreter("/opt/python3.13/python".into()),
            ..request()
        },
    )
    .unwrap();
    let run = world.run(&run_id);
    assert!(run.python_target.env_path.starts_with("/install/envs"));
    let (outcome, _) = execute(&world, &run_id);
    assert_eq!(outcome, RunOutcome::Succeeded);
    assert!(
        world
            .calls()
            .contains(&"create_venv_from:/opt/python3.13/python".to_owned())
    );
    assert_eq!(world.count("install_managed_python"), 0);
}

#[test]
fn unusable_or_mismatched_python_sources_are_rejected() {
    let world = World::new();
    *world.validation.lock().unwrap() = Some(PythonValidationReport {
        target: ValidationTarget::Rejected,
        resolved_path: "/custom/other".into(),
        python_version: None,
        checks: vec![ValidationCheck {
            id: CheckId::TargetLocation,
            passed: false,
            detail: "the folder is not empty and is not a virtual environment".into(),
            blocks: vec![BlockedFeature::Setup],
        }],
    });
    let custom = |path: &str| SetupRequest {
        python_source: PythonSourceInput::CustomVenv(path.into()),
        ..request()
    };
    assert!(matches!(
        register(&world, custom("/custom/other")),
        Err(EnvironmentAppError::PythonSourceRejected(_))
    ));
    *world.validation.lock().unwrap() = Some(validation(
        ValidationTarget::ExistingVenv,
        "/custom/old",
        PythonVersion::new(3, 12, 4),
        true,
    ));
    let mismatch = SetupRequest {
        confirm_adopted_changes: true,
        ..custom("/custom/old")
    };
    assert!(matches!(
        register(&world, mismatch),
        Err(EnvironmentAppError::PythonSourceRejected(message)) if message.contains("3.13")
    ));
    // A failed development-files check does not block setup itself; Triton fails later.
    *world.validation.lock().unwrap() = Some(validation(
        ValidationTarget::Interpreter,
        "/opt/py/python",
        PythonVersion::new(3, 13, 2),
        false,
    ));
    let run_id = register(
        &world,
        SetupRequest {
            python_source: PythonSourceInput::CustomInterpreter("/opt/py/python".into()),
            ..request()
        },
    )
    .unwrap();
    let (outcome, _) = execute(&world, &run_id);
    let RunOutcome::Failed(failure) = outcome else {
        panic!("expected Triton to be blocked")
    };
    assert_eq!(failure.step, StepId::Triton);
    assert!(failure.message.contains("development"));
    assert_eq!(world.count("install:torch"), 1, "unrelated steps still ran");
}

// ----------------------------------------------------------------- re-setup modes

#[test]
fn resetup_modes_follow_the_documented_rules() {
    let world = World::new();
    assert!(matches!(
        register(&world, resetup_request(SetupMode::ChangeTorch)),
        Err(EnvironmentAppError::Domain(
            EnvironmentError::ModeNeedsActiveEnvironment
        ))
    ));

    let world = World::new();
    world.seed_active_env("env-old", PythonVersion::new(3, 13, 16));
    assert!(matches!(
        register(&world, request()),
        Err(EnvironmentAppError::Domain(
            EnvironmentError::FirstRunWithActiveEnvironment
        ))
    ));
    // Changing torch keeps the Python minor.
    assert!(matches!(
        register(
            &world,
            SetupRequest {
                profile_id: "win-py312-torch2100-cu130".into(),
                ..resetup_request(SetupMode::ChangeTorch)
            }
        ),
        Err(EnvironmentAppError::Domain(
            EnvironmentError::ModeNeedsSamePythonMinor
        ))
    ));
    // Changing Python needs another minor.
    assert!(matches!(
        register(&world, resetup_request(SetupMode::ChangePython)),
        Err(EnvironmentAppError::Domain(
            EnvironmentError::ModeNeedsDifferentPythonMinor
        ))
    ));
    assert!(
        register(
            &world,
            SetupRequest {
                profile_id: "win-py312-torch2100-cu130".into(),
                ..resetup_request(SetupMode::ChangePython)
            }
        )
        .is_ok()
    );
}

#[test]
fn resetup_cannot_change_the_shared_comfyui_checkout() {
    let world = World::new();
    world.seed_active_env("env-old", PythonVersion::new(3, 13, 16));
    assert!(matches!(
        register(
            &world,
            SetupRequest {
                comfyui_ref: ComfyRef::Latest,
                ..resetup_request(SetupMode::ChangeTorch)
            }
        ),
        Err(EnvironmentAppError::RefChangeNotAllowed)
    ));
}

#[test]
fn switching_to_the_active_venv_is_rejected() {
    let world = World::new();
    let active = world.seed_active_env("env-old", PythonVersion::new(3, 13, 16));
    *world.validation.lock().unwrap() = Some(validation(
        ValidationTarget::ExistingVenv,
        &active.path.to_string_lossy(),
        PythonVersion::new(3, 13, 16),
        true,
    ));
    let result = register(
        &world,
        SetupRequest {
            python_source: PythonSourceInput::CustomVenv(active.path.clone()),
            confirm_adopted_changes: true,
            ..resetup_request(SetupMode::SwitchVenv)
        },
    );
    assert!(matches!(
        result,
        Err(EnvironmentAppError::Domain(
            EnvironmentError::TargetIsActiveEnvironment
        ))
    ));
    // Switching needs a custom venv source at all.
    assert!(matches!(
        register(&world, resetup_request(SetupMode::SwitchVenv)),
        Err(EnvironmentAppError::PythonSourceRejected(_))
    ));
}

#[test]
fn repair_does_nothing_when_the_selected_environment_verifies() {
    let world = World::new();
    let mut env = world.seed_active_env("env-old", PythonVersion::new(3, 13, 16));
    env.mark_repair_required("t").unwrap();
    world.envs.lock().unwrap().insert("env-old".into(), env);
    let run_id = register(&world, resetup_request(SetupMode::Repair)).unwrap();
    let (outcome, _) = execute(&world, &run_id);
    assert_eq!(outcome, RunOutcome::NothingToRepair);
    assert_eq!(world.env("env-old").state(), EnvState::Verified);
    assert_eq!(
        world.envs.lock().unwrap().len(),
        1,
        "no replacement was built"
    );
    assert_eq!(world.count("ensure_uv"), 0);
    assert_eq!(world.run(&run_id).status, RunStatus::Succeeded);
}

#[test]
fn repair_builds_a_replacement_when_verification_fails() {
    let world = World::new();
    let mut env = world.seed_active_env("env-old", PythonVersion::new(3, 13, 16));
    env.mark_repair_required("t").unwrap();
    world.envs.lock().unwrap().insert("env-old".into(), env);
    *world.checkout.lock().unwrap() = Checkout::Repo {
        commit: "abc1230000000000000000000000000000000000".into(),
    };
    // The first verification (of the selected environment) fails, the replacement then passes.
    world.fail(
        "verify_final",
        ProvisionError::Failed("import torch failed".into()),
    );
    let run_id = register(&world, resetup_request(SetupMode::Repair)).unwrap();
    let (outcome, _) = execute(&world, &run_id);
    assert_eq!(outcome, RunOutcome::Succeeded);
    let run = world.run(&run_id);
    assert_ne!(run.target_env.as_str(), "env-old");
    assert_eq!(world.active_id().as_deref(), Some(run.target_env.as_str()));
    assert_eq!(
        world.env("env-old").state(),
        EnvState::RepairRequired,
        "the old env is kept"
    );
}
