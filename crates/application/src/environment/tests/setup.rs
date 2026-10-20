use std::{path::PathBuf, sync::Arc};

use domain::custom_nodes::DependencyState;
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
        accelerators: Vec::new(),
        curated_nodes: Vec::new(),
        clean_cache: false,
        confirm_adopted_changes: false,
        allow_source_builds: false,
        third_party_consent: false,
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
    let outcome =
        block_on(ExecuteSetup::new(world.deps(), world.nodes()).execute(run_id, sink.clone()))
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
        "ensure_gdown_tool",
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
        curated_decisions: Vec::new(),
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
        curated_decisions: Vec::new(),
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
        ExecuteSetup::new(deps.clone(), world.nodes())
            .execute(&run_id, Arc::new(RecordingSink::default())),
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
        ExecuteSetup::new(deps.clone(), world.nodes())
            .execute(&run_id, Arc::new(RecordingSink::default())),
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
            accelerators: Vec::new(),
            curated_nodes: Vec::new(),
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
        curated_decisions: Vec::new(),
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
fn first_run_consent_installs_manager_without_pausing() {
    let world = World::new();
    let run_id = register(
        &world,
        SetupRequest {
            install_manager: true,
            third_party_consent: true,
            ..request()
        },
    )
    .unwrap();
    assert!(world.run(&run_id).request.third_party_consent);
    let (outcome, sink) = execute(&world, &run_id);
    assert_eq!(outcome, RunOutcome::Succeeded);
    assert_eq!(world.count("install_manager"), 1);
    let run = world.run(&run_id);
    assert_eq!(run.step(StepId::Manager).status, StepStatus::Succeeded);
    let manager = run.resolved.manager.expect("the Manager was resolved");
    assert_eq!(
        run.manager_consent.as_deref(),
        Some(manager.commit.as_str()),
        "the approved commit is persisted with the run"
    );
    assert!(sink.log_lines().contains(&format!(
        "ComfyUI-Manager {} at commit {} was approved on the setup review page.",
        manager.url, manager.commit
    )));

    // A pre-approved commit is not logged as approved again, and a run without the flag
    // still pauses (see `optional_manager_step_needs_consent_for_the_resolved_commit`).
    let world = World::new();
    let run_id = register(
        &world,
        SetupRequest {
            install_manager: true,
            third_party_consent: true,
            ..request()
        },
    )
    .unwrap();
    world
        .runs
        .lock()
        .unwrap()
        .get_mut(run_id.as_str())
        .unwrap()
        .manager_consent = Some("mgr0000000000000000000000000000000000001".into());
    let (outcome, sink) = execute(&world, &run_id);
    assert_eq!(outcome, RunOutcome::Succeeded);
    assert!(
        sink.log_lines()
            .iter()
            .all(|line| !line.contains("was approved on the setup review page"))
    );
}

#[test]
fn third_party_consent_is_refused_outside_first_run() {
    for mode in [
        SetupMode::Repair,
        SetupMode::ChangeTorch,
        SetupMode::ChangePython,
        SetupMode::SwitchVenv,
        SetupMode::Rebuild,
    ] {
        let world = World::new();
        let result = register(
            &world,
            SetupRequest {
                mode,
                third_party_consent: true,
                ..request()
            },
        );
        assert!(
            matches!(result, Err(EnvironmentAppError::ThirdPartyConsentInvalid)),
            "{mode:?}: {result:?}"
        );
        assert!(world.runs.lock().unwrap().is_empty(), "{mode:?}");
    }
    // The first run accepts it.
    let world = World::new();
    assert!(
        register(
            &world,
            SetupRequest {
                third_party_consent: true,
                ..request()
            }
        )
        .is_ok()
    );
}

#[test]
fn a_failed_optional_step_can_be_skipped_and_required_ones_cannot() {
    let world = World::new();
    let run_id = register(
        &world,
        SetupRequest {
            install_manager: true,
            accelerators: Vec::new(),
            curated_nodes: Vec::new(),
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
    let mut selected = request();
    selected.accelerators = vec!["sageattention".into()];
    let run_id = register(&world, selected).unwrap();
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
    world
        .refuse_pin_restore
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
    // The first node installs; the second one's failure prevents activation.
    world.succeed_first("install:node", 1);
    world.fail("install:node", ProvisionError::Failed("no wheel".into()));
    let (outcome, _) = execute(&world, &run_id);
    assert!(matches!(
        outcome,
        RunOutcome::Failed(failure) if failure.step == StepId::ComfyRequirements
    ));
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
    assert!(node_installs.iter().all(|spec| {
        spec.guarded.as_ref().is_some_and(|file| {
            file.text.as_ref() == b"fixture-dependency==1.0.0\n"
                && spec.requirement_files.as_slice() == std::slice::from_ref(&file.path)
        })
    }));
    assert!(
        world
            .node_scan_retained
            .lock()
            .unwrap()
            .iter()
            .all(|held| *held)
    );
    assert_eq!(
        *world.node_install_retained.lock().unwrap(),
        [(true, true), (true, true)]
    );
    let step = world.run(&run_id).step(StepId::ComfyRequirements).clone();
    assert_eq!(step.status, StepStatus::Failed);
    let run = world.run(&run_id);
    assert_eq!(world.active_id().as_deref(), Some("env-old"));
    assert_eq!(run.activation, ActivationPhase::None);
    assert_eq!(world.env(run.target_env.as_str()).state(), EnvState::Failed);
    assert_eq!(
        world.node_outcome(&run.target_env, "good"),
        Some(DependencyState::Ok)
    );
    assert_eq!(
        world.node_outcome(&run.target_env, "bad"),
        Some(DependencyState::Failed)
    );
    assert_eq!(world.count("commit_activation"), 0);
    assert!(!world.gate.busy.load(std::sync::atomic::Ordering::SeqCst));
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
    let env = world.seed_active_env("env-old", PythonVersion::new(3, 13, 16));
    world.node_requirements.lock().unwrap().push(PathBuf::from(
        "/install/ComfyUI/custom_nodes/ready/requirements.txt",
    ));
    world.seed_node_outcome(&env.id, "ready", DependencyState::Ok);
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
fn repair_installs_missing_download_tools() {
    let world = World::new();
    let env = world.seed_active_env("env-old", PythonVersion::new(3, 13, 16));
    world.node_requirements.lock().unwrap().push(PathBuf::from(
        "/install/ComfyUI/custom_nodes/ready/requirements.txt",
    ));
    world.seed_node_outcome(&env.id, "ready", DependencyState::Ok);
    // The replacement environment Repair builds keeps the existing ComfyUI checkout.
    *world.checkout.lock().unwrap() = Checkout::Repo {
        commit: "abc1230000000000000000000000000000000000".into(),
    };
    world
        .download_tools_missing
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let run_id = register(&world, resetup_request(SetupMode::Repair)).unwrap();
    let (outcome, _) = execute(&world, &run_id);
    assert_eq!(outcome, RunOutcome::Succeeded);
    assert_eq!(world.count("ensure_gdown_tool"), 1);
}

#[test]
fn repair_builds_a_replacement_when_verification_fails() {
    let world = World::new();
    world.seed_active_env("env-old", PythonVersion::new(3, 13, 16));
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
        EnvState::Verified,
        "the old env is kept"
    );
}

#[test]
fn repair_required_always_builds_a_replacement() {
    let world = World::new();
    let mut old = world.seed_active_env("env-old", PythonVersion::new(3, 13, 16));
    old.mark_repair_required("t").unwrap();
    world.envs.lock().unwrap().insert("env-old".into(), old);
    *world.checkout.lock().unwrap() = Checkout::Repo {
        commit: "abc1230000000000000000000000000000000000".into(),
    };
    let run_id = register(&world, resetup_request(SetupMode::Repair)).unwrap();
    let (outcome, _) = execute(&world, &run_id);
    assert_eq!(outcome, RunOutcome::Succeeded);
    let run = world.run(&run_id);
    assert_ne!(run.target_env.as_str(), "env-old");
    assert_eq!(world.active_id().as_deref(), Some(run.target_env.as_str()));
    assert_eq!(world.env("env-old").state(), EnvState::RepairRequired);
    assert_eq!(world.count("create_venv"), 1);
    assert_eq!(
        world.count("verify_final"),
        1,
        "only the replacement is verified"
    );
}

#[test]
fn repair_failure_preserves_active_environment_and_failed_outcomes() {
    let world = World::new();
    let old = world.seed_active_env("env-old", PythonVersion::new(3, 13, 16));
    *world.checkout.lock().unwrap() = Checkout::Repo {
        commit: "abc1230000000000000000000000000000000000".into(),
    };
    world.node_requirements.lock().unwrap().push(PathBuf::from(
        "/install/ComfyUI/custom_nodes/broken/requirements.txt",
    ));
    world.seed_node_outcome(&old.id, "broken", DependencyState::Failed);
    world.fail("install:node", ProvisionError::Failed("no wheel".into()));
    let run_id = register(&world, resetup_request(SetupMode::Repair)).unwrap();
    let (outcome, _) = execute(&world, &run_id);
    assert!(matches!(
        outcome,
        RunOutcome::Failed(failure) if failure.step == StepId::ComfyRequirements
    ));
    let run = world.run(&run_id);
    assert_eq!(world.active_id().as_deref(), Some("env-old"));
    assert_eq!(world.env("env-old"), old);
    assert_eq!(world.env(run.target_env.as_str()).state(), EnvState::Failed);
    assert_eq!(run.activation, ActivationPhase::None);
    assert_eq!(
        world.node_outcome(&old.id, "broken"),
        Some(DependencyState::Failed)
    );
    assert_eq!(
        world.node_outcome(&run.target_env, "broken"),
        Some(DependencyState::Failed)
    );
    let calls = world.calls();
    let attempted = world.position("install:node");
    assert_eq!(
        calls[attempted + 1],
        "list",
        "pins are checked after failure"
    );
    assert!(world.position("node_outcome:broken:Failed") > attempted + 1);
    assert_eq!(world.count("commit_activation"), 0);
}

#[test]
fn node_requirement_provider_failure_never_reports_repaired_or_activates() {
    for (mode, repair_required) in [
        (SetupMode::Repair, false),
        (SetupMode::Repair, true),
        (SetupMode::ChangeTorch, false),
        (SetupMode::ChangePython, false),
        (SetupMode::SwitchVenv, false),
    ] {
        let world = World::new();
        let mut old = world.seed_active_env("env-old", PythonVersion::new(3, 13, 16));
        if repair_required {
            old.mark_repair_required("t").unwrap();
            world
                .envs
                .lock()
                .unwrap()
                .insert("env-old".into(), old.clone());
        }
        *world.checkout.lock().unwrap() = Checkout::Repo {
            commit: "abc1230000000000000000000000000000000000".into(),
        };
        world.node_requirements.lock().unwrap().push(PathBuf::from(
            "/install/ComfyUI/custom_nodes/broken/requirements.txt",
        ));
        world.seed_node_outcome(&old.id, "broken", DependencyState::Failed);
        let old_outcomes = world.node_outcomes.lock().unwrap().clone();
        let mut request = resetup_request(mode);
        if mode == SetupMode::ChangePython {
            request.profile_id = "win-py312-torch2100-cu130".into();
            *world.venv_version.lock().unwrap() = PythonVersion::new(3, 12, 15);
        } else if mode == SetupMode::SwitchVenv {
            request.python_source = PythonSourceInput::CustomVenv("/custom/replacement".into());
            request.confirm_adopted_changes = true;
            *world.validation.lock().unwrap() = Some(validation(
                ValidationTarget::ExistingVenv,
                "/custom/replacement",
                PythonVersion::new(3, 13, 16),
                true,
            ));
        }
        let run_id = register(&world, request).unwrap();
        world.fail(
            "node_requirement_sets",
            ProvisionError::Failed(
                "Custom node requirements could not be read or recorded.".into(),
            ),
        );
        let expected_step = if mode == SetupMode::Repair && !repair_required {
            StepId::Preflight
        } else {
            StepId::ComfyRequirements
        };
        assert!(
            matches!(
                execute(&world, &run_id).0,
                RunOutcome::Failed(failure) if failure.step == expected_step
                    && failure.message == "Custom node requirements could not be read or recorded."
            ),
            "{mode:?}, repair_required={repair_required}"
        );
        let run = world.run(&run_id);
        assert_eq!(run.status, RunStatus::Failed);
        assert_eq!(run.activation, ActivationPhase::None);
        assert_eq!(world.active_id().as_deref(), Some("env-old"));
        assert_eq!(world.env("env-old"), old);
        assert_eq!(*world.node_outcomes.lock().unwrap(), old_outcomes);
        assert_eq!(world.node_outcome(&run.target_env, "broken"), None);
        assert_eq!(world.count("node_requirement_sets"), 1);
        assert!(
            world
                .node_scan_retained
                .lock()
                .unwrap()
                .iter()
                .all(|held| *held)
        );
        for call in [
            "install:node",
            "hook:run",
            "publish_manifest",
            "commit_activation",
        ] {
            assert_eq!(world.count(call), 0, "{mode:?}: {call}");
        }
        if expected_step == StepId::Preflight {
            assert_eq!(world.envs.lock().unwrap().len(), 1);
            assert_eq!(world.count("node_needs_repair"), 1);
            assert_eq!(world.count("ensure_uv"), 0);
        } else {
            assert_eq!(world.env(run.target_env.as_str()).state(), EnvState::Failed);
            assert_eq!(world.count("node_needs_repair"), 0);
        }
    }
}

#[test]
fn accelerator_selection_is_explicit_and_profile_bound() {
    let world = World::new();
    let mut a = accelerator("sageattention");
    a.default_selected = true;
    world.profiles.lock().unwrap()[0].accelerators = vec![a];
    for selection in [
        vec!["unknown".into()],
        vec!["sageattention".into(), "sageattention".into()],
    ] {
        let mut req = request();
        req.accelerators = selection;
        assert!(matches!(
            register(&world, req),
            Err(EnvironmentAppError::AcceleratorSelectionInvalid)
        ));
    }
    let mut other = request();
    other.profile_id = "win-py312-torch2100-cu130".into();
    other.accelerators = vec!["sageattention".into()];
    assert!(matches!(
        register(&world, other),
        Err(EnvironmentAppError::AcceleratorSelectionInvalid)
    ));
    let id = register(&world, request()).unwrap();
    assert_eq!(execute(&world, &id).0, RunOutcome::Succeeded);
    assert_eq!(world.count("install:other"), 0);
    assert_eq!(world.count("fetch_wheel"), 0);
    let result = &world.run(&id).components.accelerators[0];
    assert!(!result.selected);
    assert_eq!(result.status, ComponentStatus::Skipped);
    assert_eq!(result.importable, None);
}

#[test]
fn wheel_hash_failure_never_runs_uv() {
    use domain::{
        environment::AcceleratorSource,
        shared::model_values::{SafeFileName, Sha256},
    };
    let world = World::new();
    let mut spec = accelerator("sageattention");
    spec.source = AcceleratorSource::Wheel {
        url: "https://example.test/sage.whl".into(),
        filename: SafeFileName::new("sage-1-py3-none-any.whl").unwrap(),
        sha256: Sha256::new(&"0".repeat(64)).unwrap(),
        size: None,
    };
    world.profiles.lock().unwrap()[0].accelerators = vec![spec];
    world.fail(
        "fetch_wheel",
        ProvisionError::Failed("pinned SHA-256 mismatch".into()),
    );
    let mut req = request();
    req.accelerators = vec!["sageattention".into()];
    let id = register(&world, req).unwrap();
    assert_eq!(execute(&world, &id).0, RunOutcome::Succeeded);
    assert_eq!(world.count("install:other"), 0);
    let result = &world.run(&id).components.accelerators[0];
    assert_eq!(result.status, ComponentStatus::Failed);
    assert_eq!(result.importable, None);
    assert!(result.message.as_ref().unwrap().contains("SHA-256"));
}

#[test]
fn accelerator_failure_reasserts_original_pins() {
    for error in [
        ProvisionError::Failed("original wheel install failure".into()),
        ProvisionError::Cancelled,
    ] {
        let world = World::new();
        world.profiles.lock().unwrap()[0].accelerators = vec![accelerator("sageattention")];
        world
            .drift_on_accelerator
            .store(true, std::sync::atomic::Ordering::SeqCst);
        world.fail("install:other", error.clone());
        let mut req = request();
        req.accelerators = vec!["sageattention".into()];
        let id = register(&world, req).unwrap();
        let outcome = execute(&world, &id).0;
        assert_eq!(world.count("restore_pins"), 1);
        assert!(world.position("install:other") < world.position("restore_pins"));
        assert!(
            world
                .installed
                .lock()
                .unwrap()
                .iter()
                .any(|p| p.name == "torch" && p.version == "2.14.1+cu130")
        );
        let result = &world.run(&id).components.accelerators[0];
        assert_eq!(result.status, ComponentStatus::Failed);
        if error == ProvisionError::Cancelled {
            assert_eq!(outcome, RunOutcome::Cancelled);
        } else {
            assert_eq!(outcome, RunOutcome::Succeeded);
            assert_eq!(
                result.message.as_deref(),
                Some("original wheel install failure")
            );
        }
    }
}

#[test]
fn replacement_node_dependency_failure_never_activates() {
    for mode in [
        SetupMode::Repair,
        SetupMode::ChangeTorch,
        SetupMode::ChangePython,
        SetupMode::SwitchVenv,
    ] {
        let world = World::new();
        world.seed_active_env("env-old", PythonVersion::new(3, 13, 16));
        *world.checkout.lock().unwrap() = Checkout::Repo {
            commit: "abc1230000000000000000000000000000000000".into(),
        };
        world.node_requirements.lock().unwrap().push(PathBuf::from(
            "/install/ComfyUI/custom_nodes/broken/requirements.txt",
        ));
        let mut request = resetup_request(mode);
        if mode == SetupMode::ChangePython {
            request.profile_id = "win-py312-torch2100-cu130".into();
            *world.venv_version.lock().unwrap() = PythonVersion::new(3, 12, 15);
        } else if mode == SetupMode::SwitchVenv {
            request.python_source = PythonSourceInput::CustomVenv("/custom/replacement".into());
            request.confirm_adopted_changes = true;
            *world.validation.lock().unwrap() = Some(validation(
                ValidationTarget::ExistingVenv,
                "/custom/replacement",
                PythonVersion::new(3, 13, 16),
                true,
            ));
        }
        let run_id = register(&world, request).unwrap();
        world.fail("install:node", ProvisionError::Failed("no wheel".into()));
        let (outcome, _) = execute(&world, &run_id);
        assert!(
            matches!(
                outcome,
                RunOutcome::Failed(failure) if failure.step == StepId::ComfyRequirements
            ),
            "{mode:?}"
        );
        let run = world.run(&run_id);
        assert_eq!(world.active_id().as_deref(), Some("env-old"), "{mode:?}");
        assert_eq!(
            world.env(run.target_env.as_str()).state(),
            EnvState::Failed,
            "{mode:?}"
        );
        assert_eq!(run.activation, ActivationPhase::None, "{mode:?}");
        assert_eq!(
            world.node_outcome(&run.target_env, "broken"),
            Some(DependencyState::Failed),
            "{mode:?}"
        );
        let calls = world.calls();
        assert_eq!(
            calls[world.position("install:node") + 1],
            "list",
            "{mode:?}"
        );
        assert_eq!(world.count("publish_manifest"), 0, "{mode:?}");
        assert_eq!(world.count("commit_activation"), 0, "{mode:?}");
    }
}

#[test]
fn successful_repair_records_new_environment_outcomes_and_preserves_pins() {
    for previous in [
        None,
        Some(DependencyState::NotRun),
        Some(DependencyState::Failed),
    ] {
        let world = World::new();
        let old = world.seed_active_env("env-old", PythonVersion::new(3, 13, 16));
        *world.checkout.lock().unwrap() = Checkout::Repo {
            commit: "abc1230000000000000000000000000000000000".into(),
        };
        world.node_requirements.lock().unwrap().push(PathBuf::from(
            "/install/ComfyUI/custom_nodes/ready/requirements.txt",
        ));
        if let Some(state) = previous {
            world.seed_node_outcome(&old.id, "ready", state);
        }
        let run_id = register(&world, resetup_request(SetupMode::Repair)).unwrap();
        let (outcome, _) = execute(&world, &run_id);
        assert_eq!(outcome, RunOutcome::Succeeded);
        let target = world.run(&run_id).target_env;
        assert_eq!(world.active_id().as_deref(), Some(target.as_str()));
        assert_eq!(world.env(target.as_str()).pins, old.pins);
        assert_eq!(
            world.node_outcome(&target, "ready"),
            Some(DependencyState::Ok)
        );
        assert_eq!(world.node_outcome(&old.id, "ready"), previous);
        assert_eq!(world.env("env-old"), old);
        assert_eq!(*world.node_install_retained.lock().unwrap(), [(true, true)]);
        let calls = world.calls();
        let attempted = world.position("install:node");
        assert_eq!(calls[attempted + 1], "list");
        assert!(world.position("node_outcome:ready:Ok") > attempted + 1);
        let next = register(&world, resetup_request(SetupMode::Repair)).unwrap();
        assert_eq!(execute(&world, &next).0, RunOutcome::NothingToRepair);
        assert_eq!(world.count("install:node"), 1);
        assert!(!world.gate.busy.load(std::sync::atomic::Ordering::SeqCst));
    }
}

#[test]
fn optional_pin_drift_prevents_activation() {
    let world = World::new();
    world.seed_active_env("env-old", PythonVersion::new(3, 13, 16));
    *world.checkout.lock().unwrap() = Checkout::Repo {
        commit: "abc1230000000000000000000000000000000000".into(),
    };
    let mut req = resetup_request(SetupMode::ChangeTorch);
    req.install_manager = true;
    let id = register(&world, req).unwrap();
    // First pause requests exact Manager consent, then an optional dependency attempt cannot prove its pins.
    assert!(matches!(execute(&world, &id).0, RunOutcome::Failed(_)));
    let commit = world.run(&id).resolved.manager.unwrap().commit;
    block_on(StartSetup::new(world.deps()).execute(SetupAction::Retry {
        run_id: id.clone(),
        manager_consent: Some(commit),
        curated_decisions: Vec::new(),
    }))
    .unwrap();
    world
        .drift_on_node
        .store(true, std::sync::atomic::Ordering::SeqCst);
    world
        .refuse_pin_restore
        .store(true, std::sync::atomic::Ordering::SeqCst);
    world.fail(
        "install:node",
        ProvisionError::Failed("dependency attempt failed".into()),
    );
    let RunOutcome::Failed(failure) = execute(&world, &id).0 else {
        panic!("pin drift must be fatal")
    };
    assert_eq!(failure.step, StepId::Manager);
    assert_eq!(
        world.env(world.run(&id).target_env.as_str()).state(),
        EnvState::Failed
    );
    assert_eq!(world.active_id().as_deref(), Some("env-old"));
    assert!(matches!(
        block_on(
            StartSetup::new(world.deps()).execute(SetupAction::SkipStep {
                run_id: id,
                step: StepId::Manager
            })
        ),
        Err(EnvironmentAppError::StepNotSkippable)
    ));
    let world = World::new();
    world.profiles.lock().unwrap()[0].accelerators = vec![accelerator("sageattention")];
    world
        .drift_on_accelerator
        .store(true, std::sync::atomic::Ordering::SeqCst);
    world
        .refuse_pin_restore
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let mut req = request();
    req.accelerators = vec!["sageattention".into()];
    let id = register(&world, req).unwrap();
    assert!(
        matches!(execute(&world, &id).0, RunOutcome::Failed(_)),
        "BestEffort also cannot hide drift"
    );
    assert!(world.active_id().is_none());
}

#[test]
fn final_verification_cannot_rebaseline_drift() {
    for before in [true, false] {
        let world = World::new();
        if before {
            world
                .drift_before_final_verify
                .store(true, std::sync::atomic::Ordering::SeqCst);
        } else {
            world
                .drift_on_final_verify
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
        let id = register(&world, request()).unwrap();
        let RunOutcome::Failed(failure) = execute(&world, &id).0 else {
            panic!("drift must fail")
        };
        assert_eq!(failure.step, StepId::FinalVerify);
        let env = world.env(world.run(&id).target_env.as_str());
        assert_eq!(env.pins.get(&name("torch")), Some("2.14.1+cu130"));
        assert_eq!(env.verification, None);
        assert!(world.active_id().is_none());
        assert_eq!(world.count("publish_manifest"), 0);
        if before {
            assert_eq!(world.count("verify_final"), 0);
        }
    }
}

#[test]
fn replacement_node_dependency_cancellation_preserves_active_environment() {
    let world = World::new();
    world.seed_active_env("env-old", PythonVersion::new(3, 13, 16));
    *world.checkout.lock().unwrap() = Checkout::Repo {
        commit: "abc1230000000000000000000000000000000000".into(),
    };
    world.node_requirements.lock().unwrap().push(PathBuf::from(
        "/install/ComfyUI/custom_nodes/cancelled/requirements.txt",
    ));
    let run_id = register(&world, resetup_request(SetupMode::ChangeTorch)).unwrap();
    world.fail("install:node", ProvisionError::Cancelled);
    assert_eq!(execute(&world, &run_id).0, RunOutcome::Cancelled);
    let run = world.run(&run_id);
    assert_eq!(run.status, RunStatus::Cancelled);
    assert_eq!(world.active_id().as_deref(), Some("env-old"));
    assert_eq!(world.env(run.target_env.as_str()).state(), EnvState::Failed);
    assert_eq!(
        world.node_outcome(&run.target_env, "cancelled"),
        Some(DependencyState::Failed)
    );
    assert_eq!(
        *world.gate_busy_at_cancel_cleanup.lock().unwrap(),
        Some(true)
    );
    assert_eq!(world.calls()[world.position("install:node") + 1], "list");
    assert_eq!(world.count("commit_activation"), 0);
    assert!(!world.gate.busy.load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn replacement_node_pin_failure_records_failed_outcome() {
    let world = World::new();
    world.seed_active_env("env-old", PythonVersion::new(3, 13, 16));
    *world.checkout.lock().unwrap() = Checkout::Repo {
        commit: "abc1230000000000000000000000000000000000".into(),
    };
    world.node_requirements.lock().unwrap().push(PathBuf::from(
        "/install/ComfyUI/custom_nodes/drift/requirements.txt",
    ));
    world
        .drift_on_node_requirements
        .store(true, std::sync::atomic::Ordering::SeqCst);
    world
        .refuse_pin_restore
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let run_id = register(&world, resetup_request(SetupMode::ChangeTorch)).unwrap();
    assert!(matches!(
        execute(&world, &run_id).0,
        RunOutcome::Failed(failure) if failure.step == StepId::ComfyRequirements
            && failure.message.contains("torch")
    ));
    let run = world.run(&run_id);
    assert_eq!(
        world.node_outcome(&run.target_env, "drift"),
        Some(DependencyState::Failed)
    );
    assert_eq!(world.active_id().as_deref(), Some("env-old"));
    assert_eq!(world.count("commit_activation"), 0);
}

#[test]
fn replacement_node_pin_drift_is_restored_before_the_outcome_is_recorded() {
    let world = World::new();
    world.seed_active_env("env-old", PythonVersion::new(3, 13, 16));
    *world.checkout.lock().unwrap() = Checkout::Repo {
        commit: "abc1230000000000000000000000000000000000".into(),
    };
    world.node_requirements.lock().unwrap().push(PathBuf::from(
        "/install/ComfyUI/custom_nodes/drift/requirements.txt",
    ));
    world
        .drift_on_node_requirements
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let run_id = register(&world, resetup_request(SetupMode::ChangeTorch)).unwrap();
    assert_eq!(execute(&world, &run_id).0, RunOutcome::Succeeded);
    let run = world.run(&run_id);
    assert_eq!(world.count("restore_pins"), 1);
    assert!(world.position("install:node") < world.position("restore_pins"));
    assert!(world.position("restore_pins") < world.position("node_outcome:drift:Ok"));
    assert_eq!(
        world.node_outcome(&run.target_env, "drift"),
        Some(DependencyState::Ok)
    );
    assert_eq!(
        world.env(run.target_env.as_str()).pins.get(&name("torch")),
        Some("2.14.1+cu130")
    );
}

#[test]
fn accelerator_import_failure_is_reported_separately() {
    let world = World::new();
    let mut fa3 = accelerator("flashattention-3");
    fa3.modules.clear();
    fa3.provenance = "Module unverified; candidate flash_attn_interface".into();
    world.profiles.lock().unwrap()[0].accelerators = vec![accelerator("sageattention"), fa3];
    world.fail(
        "verify_accelerator",
        ProvisionError::Failed("DLL import failed".into()),
    );
    let mut req = request();
    req.accelerators = vec!["sageattention".into(), "flashattention-3".into()];
    let id = register(&world, req).unwrap();
    assert_eq!(execute(&world, &id).0, RunOutcome::Succeeded);
    let run = world.run(&id);
    assert_eq!(
        run.components.accelerators[0].status,
        ComponentStatus::Installed
    );
    assert_eq!(run.components.accelerators[0].importable, Some(false));
    assert!(
        run.components.accelerators[0]
            .message
            .as_ref()
            .unwrap()
            .contains("DLL import failed")
    );
    assert_eq!(run.components.accelerators[1].importable, None);
    assert!(
        run.components.accelerators[1]
            .message
            .as_ref()
            .unwrap()
            .contains("unverified")
    );
    assert_eq!(world.count("import:flash_attn_interface"), 0);
    assert_eq!(run.step(StepId::FinalVerify).status, StepStatus::Warning);
}

#[test]
fn repair_uses_the_recorded_profile_snapshot_and_component_selections() {
    let world = World::new();
    world.profiles.lock().unwrap()[0].accelerators = vec![accelerator("sageattention")];
    let mut req = request();
    req.accelerators = vec!["sageattention".into()];
    let id = register(&world, req).unwrap();
    execute(&world, &id);
    let original = world.run(&id);
    world.profiles.lock().unwrap()[0].revision = 2;
    world.profiles.lock().unwrap()[0].accelerators.clear();
    let repair = register(&world, resetup_request(SetupMode::Repair)).unwrap();
    let run = world.run(&repair);
    assert_eq!(run.profile, original.profile);
    assert_eq!(run.request.accelerators, original.request.accelerators);
    assert_eq!(
        run.request.install_manager,
        original.request.install_manager
    );
    let world = World::new();
    world.seed_active_env("env-old", PythonVersion::new(3, 13, 16));
    world.profiles.lock().unwrap()[0].revision = 2;
    assert!(matches!(
        register(&world, resetup_request(SetupMode::Repair)),
        Err(EnvironmentAppError::ProfileRevisionUnavailable)
    ));
}

#[test]
fn wheel_components_install_verified_local_paths_with_constraints_and_no_deps() {
    use domain::{
        environment::AcceleratorSource,
        shared::model_values::{SafeFileName, Sha256},
    };
    let world = World::new();
    let mut spec = accelerator("onnxruntime-gpu");
    spec.no_deps = true;
    spec.source = AcceleratorSource::Wheel {
        url: "https://example.test/ort.whl".into(),
        filename: SafeFileName::new("ort-1-py3-none-any.whl").unwrap(),
        sha256: Sha256::new(&"0".repeat(64)).unwrap(),
        size: None,
    };
    world.profiles.lock().unwrap()[0].accelerators = vec![spec];
    let mut req = request();
    req.accelerators = vec!["onnxruntime-gpu".into()];
    let id = register(&world, req).unwrap();
    assert_eq!(execute(&world, &id).0, RunOutcome::Succeeded);
    let installs = world.installs.lock().unwrap();
    let wheel = installs
        .iter()
        .find(|s| s.requirements.iter().any(|r| r.ends_with(".whl")))
        .unwrap();
    assert!(wheel.no_deps);
    assert!(!wheel.allow_source_builds);
    assert!(wheel.constraint_file.is_some());
    assert!(PathBuf::from(&wheel.requirements[0]).starts_with("/install/data/cache/wheels"));
    assert_eq!(world.count("fetch_wheel"), 1);
}
// ------------------------------------------------------------------ curated custom nodes

fn curated_request(ids: &[&str]) -> SetupRequest {
    SetupRequest {
        curated_nodes: ids.iter().map(|id| (*id).to_owned()).collect(),
        ..request()
    }
}

/// The user's answer to a review: every warning acknowledged.
fn answer(review: &crate::custom_nodes::PlanReview) -> crate::custom_nodes::CommitDecision {
    crate::custom_nodes::CommitDecision {
        consent: domain::custom_nodes::InstallConsent {
            plan_id: review.id.clone(),
            binding: review.binding.clone(),
            execute_node_code: true,
            run_install_py: false,
            allow_source_builds: false,
            acknowledge_warnings: true,
            remember_source: false,
            remember_install_py: false,
        },
        protected_changes: Vec::new(),
    }
}

fn failure_of(outcome: RunOutcome) -> StepFailure {
    match outcome {
        RunOutcome::Failed(failure) => *failure,
        other => panic!("the run must fail, not {other:?}"),
    }
}

/// One set of dependencies for registering, retrying and executing, because decisions travel
/// from the retry to the executor through the shared active runs.
struct Curated {
    world: Arc<World>,
    deps: EnvironmentDeps,
}

impl Curated {
    fn new() -> Self {
        let world = World::new();
        let deps = world.deps();
        Self { world, deps }
    }

    fn register(&self, ids: &[&str]) -> RunId {
        block_on(
            StartSetup::new(self.deps.clone())
                .execute(SetupAction::New(Box::new(curated_request(ids)))),
        )
        .unwrap()
    }

    fn register_consenting(&self, ids: &[&str]) -> RunId {
        block_on(
            StartSetup::new(self.deps.clone()).execute(SetupAction::New(Box::new(SetupRequest {
                third_party_consent: true,
                ..curated_request(ids)
            }))),
        )
        .unwrap()
    }

    fn execute(&self, id: &RunId) -> RunOutcome {
        block_on(
            ExecuteSetup::new(self.deps.clone(), self.world.nodes())
                .execute(id, Arc::new(RecordingSink::default())),
        )
        .unwrap()
    }

    fn retry(&self, id: &RunId, decisions: Vec<crate::custom_nodes::CommitDecision>) {
        block_on(
            StartSetup::new(self.deps.clone()).execute(SetupAction::Retry {
                run_id: id.clone(),
                manager_consent: None,
                curated_decisions: decisions,
            }),
        )
        .unwrap();
    }

    fn skip(&self, id: &RunId) -> Result<RunId, EnvironmentAppError> {
        block_on(
            StartSetup::new(self.deps.clone()).execute(SetupAction::SkipStep {
                run_id: id.clone(),
                step: StepId::CuratedNodes,
            }),
        )
    }

    fn reviews(&self) -> Vec<crate::custom_nodes::PlanReview> {
        self.world
            .node_reviews
            .lock()
            .unwrap()
            .values()
            .cloned()
            .collect()
    }
}

#[test]
fn curated_selection_is_validated_and_listed_in_catalog_order() {
    for ids in [vec!["unknown"], vec!["alpha", "alpha"], vec!["alpha", ""]] {
        let world = World::new();
        assert!(
            matches!(
                register(&world, curated_request(&ids)),
                Err(EnvironmentAppError::CuratedSelectionInvalid)
            ),
            "{ids:?}"
        );
        assert!(world.runs.lock().unwrap().is_empty());
    }
    // Only a first run offers the nodes.
    let world = World::new();
    world.seed_active_env("env-old", PythonVersion::new(3, 13, 16));
    let repair = SetupRequest {
        curated_nodes: vec!["alpha".into()],
        ..resetup_request(SetupMode::Repair)
    };
    assert!(matches!(
        register(&world, repair),
        Err(EnvironmentAppError::CuratedSelectionInvalid)
    ));
    // The selection is stored in catalog order, nothing is prepared and nothing is consented.
    let world = World::new();
    let id = register(&world, curated_request(&["beta", "alpha"])).unwrap();
    let run = world.run(&id);
    let listed: Vec<_> = run.curated.iter().map(|entry| entry.id.as_str()).collect();
    assert_eq!(listed, ["alpha", "beta"]);
    assert!(run.curated.iter().all(|entry| {
        entry.status == CuratedStatus::Pending && entry.plan.is_none() && entry.operation.is_none()
    }));
    assert_eq!(world.count("setup_nodes:prepare:alpha"), 0);
}

#[test]
fn first_run_rejects_unavailable_curated_selection() {
    let world = World::new();
    world.curated.lock().unwrap()[1].unavailable = Some("ships compiled cache files".into());
    // An unavailable entry is refused alone or beside an available one, and nothing is stored.
    for ids in [vec!["beta"], vec!["alpha", "beta"]] {
        assert!(
            matches!(
                register(&world, curated_request(&ids)),
                Err(EnvironmentAppError::CuratedSelectionInvalid)
            ),
            "{ids:?}"
        );
        assert!(world.runs.lock().unwrap().is_empty());
    }
    // The available entries of the same catalog are still selectable.
    let id = register(&world, curated_request(&["alpha"])).unwrap();
    let listed: Vec<_> = world
        .run(&id)
        .curated
        .iter()
        .map(|entry| entry.id.clone())
        .collect();
    assert_eq!(listed, ["alpha"]);
}

#[test]
fn curated_step_reviews_every_node_and_installs_only_with_exact_decisions() {
    let c = Curated::new();
    let id = c.register(&["alpha", "beta"]);
    let failure = failure_of(c.execute(&id));
    assert_eq!(failure.step, StepId::CuratedNodes);
    assert_eq!(failure.kind, FailureKind::ConsentRequired);
    assert!(failure.consent.is_none() && failure.can_skip);
    // Both nodes are prepared before anything is installed, in catalog order.
    let nodes_calls: Vec<_> = c
        .world
        .calls()
        .into_iter()
        .filter(|call| call.starts_with("setup_nodes:"))
        .collect();
    assert_eq!(
        nodes_calls,
        [
            "setup_nodes:installed:alpha",
            "setup_nodes:prepare:alpha",
            "setup_nodes:installed:beta",
            "setup_nodes:prepare:beta",
        ]
    );
    let run = c.world.run(&id);
    assert_eq!(run.status, RunStatus::Failed);
    assert!(
        run.curated
            .iter()
            .all(|entry| entry.status == CuratedStatus::Prepared && entry.plan.is_some())
    );
    let reviews = c.reviews();
    assert_eq!(reviews.len(), 2);

    // A decision for one plan is not enough: nothing is installed.
    c.retry(&id, vec![answer(&reviews[0])]);
    assert_eq!(
        failure_of(c.execute(&id)).kind,
        FailureKind::ConsentRequired
    );
    assert!(c.world.node_installed.lock().unwrap().is_empty());

    // A decision that does not cover the exact review asks again and installs nothing at all.
    let mut wrong = answer(&reviews[1]);
    wrong.consent.binding = "different".into();
    c.retry(&id, vec![answer(&reviews[0]), wrong]);
    assert_eq!(
        failure_of(c.execute(&id)).kind,
        FailureKind::ConsentRequired
    );
    let mut unacknowledged = answer(&reviews[1]);
    unacknowledged.consent.execute_node_code = false;
    c.retry(&id, vec![answer(&reviews[0]), unacknowledged]);
    assert_eq!(
        failure_of(c.execute(&id)).kind,
        FailureKind::ConsentRequired
    );
    assert!(c.world.node_installed.lock().unwrap().is_empty());
    assert_eq!(c.world.count("setup_nodes:commit:node-alpha"), 0);

    // Exact decisions for every plan install them, one after another, then the run activates.
    c.retry(&id, reviews.iter().map(answer).collect());
    assert_eq!(c.execute(&id), RunOutcome::Succeeded);
    let run = c.world.run(&id);
    assert!(run.curated.iter().all(|entry| {
        entry.status == CuratedStatus::Installed
            && entry.plan.is_none()
            && entry.operation.is_some()
    }));
    assert_eq!(run.step(StepId::CuratedNodes).status, StepStatus::Succeeded);
    assert_eq!(c.world.node_installed.lock().unwrap().len(), 2);
    assert_eq!(
        c.world.active_id().as_deref(),
        Some(run.target_env.as_str())
    );
    // Final activation re-checks the installed nodes.
    assert!(c.world.position("setup_nodes:installed:beta") < c.world.position("publish_manifest"));
}

#[test]
fn curated_failures_are_classified_before_they_reach_the_user() {
    // Nothing was applied: the user may retry or skip, and the candidate is not condemned.
    let c = Curated::new();
    c.world
        .fail_nodes("prepare", SetupNodeError::NotApplied("network down".into()));
    let id = c.register(&["alpha"]);
    let failure = failure_of(c.execute(&id));
    assert_eq!(failure.message, "network down");
    assert!(failure.can_skip);
    let run = c.world.run(&id);
    assert_eq!(
        c.world.env(run.target_env.as_str()).state(),
        EnvState::Provisioning
    );
    assert_eq!(run.curated[0].message.as_deref(), Some("network down"));
    c.skip(&id).unwrap();
    assert_eq!(c.world.run(&id).curated[0].status, CuratedStatus::Skipped);
    assert_eq!(c.execute(&id), RunOutcome::Succeeded);
    assert_eq!(
        c.world.run(&id).step(StepId::CuratedNodes).status,
        StepStatus::Skipped
    );

    // Damage to the candidate and protected-package drift cannot be skipped.
    for error in [
        SetupNodeError::Damaged("dependencies failed".into()),
        SetupNodeError::PinDrift("torch".into()),
    ] {
        let c = Curated::new();
        let id = c.register(&["alpha"]);
        failure_of(c.execute(&id));
        c.world.fail_nodes("commit", error);
        c.retry(&id, c.reviews().iter().map(answer).collect());
        let failure = failure_of(c.execute(&id));
        assert!(!failure.can_skip, "{failure:?}");
        let run = c.world.run(&id);
        assert_eq!(run.curated[0].status, CuratedStatus::Failed);
        assert_eq!(
            c.world.env(run.target_env.as_str()).state(),
            EnvState::Failed
        );
        assert!(matches!(
            c.skip(&id),
            Err(EnvironmentAppError::StepNotSkippable)
        ));
        assert_eq!(c.world.count("publish_manifest"), 0);
        // Even if the curated step is bypassed in persisted progress, final verification refuses it.
        c.world
            .runs
            .lock()
            .unwrap()
            .get_mut(id.as_str())
            .unwrap()
            .step_mut(StepId::CuratedNodes)
            .status = StepStatus::Skipped;
        c.retry(&id, Vec::new());
        let failure = failure_of(c.execute(&id));
        assert_eq!(failure.step, StepId::FinalVerify);
        assert!(!failure.can_skip);
        assert_eq!(c.world.run(&id).curated[0].status, CuratedStatus::Failed);
        assert_eq!(c.world.count("publish_manifest"), 0);
    }

    // Cancellation inside the node service cancels the run.
    let c = Curated::new();
    c.world.fail_nodes("prepare", SetupNodeError::Cancelled);
    let id = c.register(&["alpha"]);
    assert_eq!(c.execute(&id), RunOutcome::Cancelled);

    // A plan that went stale is prepared again and reviewed again.
    let c = Curated::new();
    let id = c.register(&["alpha"]);
    failure_of(c.execute(&id));
    c.world.fail_nodes("commit", SetupNodeError::PlanStale);
    c.retry(&id, c.reviews().iter().map(answer).collect());
    let failure = failure_of(c.execute(&id));
    assert_eq!(failure.kind, FailureKind::ConsentRequired);
    assert_eq!(c.world.count("setup_nodes:prepare:alpha"), 2);
    assert!(c.world.node_installed.lock().unwrap().is_empty());
}

#[test]
fn curated_auto_consent_stops_when_plans_keep_changing() {
    let c = Curated::new();
    for _ in 0..5 {
        c.world.fail_nodes("commit", SetupNodeError::PlanStale);
    }
    let id = c.register_consenting(&["alpha"]);
    let failure = failure_of(c.execute(&id));
    assert_eq!(failure.step, StepId::CuratedNodes);
    assert_eq!(failure.kind, FailureKind::Failed);
    assert_eq!(
        failure.message,
        "The curated node plans kept changing. Retry setup."
    );
    // Three passes prepared and tried the plan; a fourth never started.
    assert_eq!(c.world.count("setup_nodes:prepare:alpha"), 3);
    let commits = c
        .world
        .calls()
        .iter()
        .filter(|call| call.starts_with("setup_nodes:commit:"))
        .count();
    assert_eq!(commits, 3);
    assert!(c.world.node_installed.lock().unwrap().is_empty());
}
