use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use domain::{
    custom_nodes::DependencyState,
    environment::{EnvState, Ownership, PythonSourceKind, PythonVersion, SetupMode},
};

use super::{
    fakes::*,
    setup::{execute, register, request},
};
use crate::shared::MaintenanceGate;
use crate::{environment::*, test_support::block_on};

fn provisioned_installation() -> (Arc<World>, RunId) {
    let world = World::new();
    world.profiles.lock().unwrap()[0].accelerators =
        vec![accelerator("selected"), accelerator("unselected")];
    let id = register(
        &world,
        SetupRequest {
            install_manager: true,
            accelerators: vec!["selected".into()],
            ..request()
        },
    )
    .unwrap();
    let RunOutcome::Failed(failure) = execute(&world, &id).0 else {
        panic!("Manager requires consent before the original installation activates")
    };
    assert_eq!(failure.step, StepId::Manager);
    let consent = failure.consent.unwrap();
    block_on(StartSetup::new(world.deps()).execute(SetupAction::Retry {
        run_id: id.clone(),
        manager_consent: Some(consent.commit),
        curated_decisions: Vec::new(),
    }))
    .unwrap();
    assert_eq!(execute(&world, &id).0, RunOutcome::Succeeded);
    world.calls.lock().unwrap().clear();
    world.installs.lock().unwrap().clear();
    world.install_envs.lock().unwrap().clear();
    (world, id)
}

fn rebuild_request() -> SetupRequest {
    SetupRequest {
        mode: SetupMode::Rebuild,
        comfyui_ref: ComfyRef::KeepCurrent,
        ..request()
    }
}

fn assert_no_checkout_or_node_publication(world: &World) {
    for call in [
        "resolve",
        "clone_comfyui",
        "resolve_manager",
        "install_manager",
    ] {
        assert_eq!(world.count(call), 0, "{call}: {:?}", world.calls());
    }
    assert!(
        world
            .calls()
            .iter()
            .all(|call| !call.starts_with("setup_nodes:")),
        "rebuild must not resolve or publish curated nodes: {:?}",
        world.calls()
    );
}

#[test]
fn copied_run_cannot_publish_a_manifest_into_the_old_root() {
    let (world, original) = provisioned_installation();
    let original_manifest = world.manifest.lock().unwrap().clone();
    let mut copied = world.run(&original);
    copied.id = RunId::new("copied-old-root-run").unwrap();
    copied.request.install_root = PathBuf::from("/recorded-original-installation");
    copied.status = RunStatus::Failed;
    copied.activation = ActivationPhase::Intent;
    world
        .runs
        .lock()
        .unwrap()
        .insert(copied.id.as_str().to_owned(), copied.clone());

    assert!(matches!(
        block_on(StartSetup::new(world.deps()).execute(SetupAction::Retry {
            run_id: copied.id.clone(),
            manager_consent: None,
            curated_decisions: Vec::new(),
        })),
        Err(EnvironmentAppError::RebuildRequired)
    ));
    assert!(matches!(
        block_on(
            ExecuteSetup::new(world.deps(), world.nodes())
                .execute(&copied.id, Arc::new(RecordingSink::default()),)
        ),
        Err(EnvironmentAppError::RebuildRequired)
    ));
    assert_eq!(
        block_on(ReconcileStartup::new(world.deps()).execute()).unwrap(),
        ReconcileReport::default()
    );
    let env = world.env(copied.target_env.as_str());
    assert!(matches!(
        block_on(crate::environment::activation::complete_activation(
            &world.deps(),
            &mut copied,
            &env,
        )),
        Err(EnvironmentAppError::RebuildRequired)
    ));
    assert_eq!(world.run(&copied.id), copied, "historical run is retained");
    assert_eq!(*world.manifest.lock().unwrap(), original_manifest);
    assert_eq!(world.count("publish_manifest"), 0);
    assert_eq!(world.count("commit_activation"), 0);
    assert_eq!(world.count("ensure_uv"), 0);
}

#[test]
fn rebuild_never_inherits_third_party_consent() {
    let world = World::new();
    let original = register(
        &world,
        SetupRequest {
            install_manager: true,
            third_party_consent: true,
            ..request()
        },
    )
    .unwrap();
    assert_eq!(execute(&world, &original).0, RunOutcome::Succeeded);
    let recorded = world.run(&original);
    assert!(recorded.request.third_party_consent);
    world.installed.lock().unwrap().clear();

    // The recorded request seeds the rebuild, but its consent does not carry over.
    let rebuilt = register(&world, rebuild_request()).unwrap();
    let run = world.run(&rebuilt);
    assert!(run.request.install_manager);
    assert!(!run.request.third_party_consent);
    assert!(world.run(&original).request.third_party_consent);
}

#[test]
fn rebuild_uses_recorded_profile_revision_and_components() {
    let (world, original) = provisioned_installation();
    let recorded = world.run(&original);
    let mut newest = recorded.profile.clone();
    newest.revision += 1;
    newest.python = PythonVersion::new(3, 12, 15);
    newest.accelerators = vec![accelerator("newest-only")];
    *world.profiles.lock().unwrap() = vec![newest];
    let checkout = world.checkout.lock().unwrap().clone();
    world.installed.lock().unwrap().clear();

    let rebuilt = register(
        &world,
        SetupRequest {
            profile_id: "client-profile-must-be-ignored".into(),
            python_source: PythonSourceInput::CustomVenv(PathBuf::from("/client-venv")),
            comfyui_ref: ComfyRef::Tag("client-ref-must-be-ignored".into()),
            accelerators: vec!["newest-only".into()],
            curated_nodes: vec!["alpha".into()],
            install_manager: false,
            allow_source_builds: true,
            ..rebuild_request()
        },
    )
    .unwrap();
    let run = world.run(&rebuilt);
    assert_eq!(run.profile, recorded.profile);
    assert_eq!(run.components, recorded.components);
    assert_eq!(run.request.profile_id, recorded.request.profile_id);
    assert_eq!(run.request.accelerators, recorded.request.accelerators);
    assert_eq!(
        run.request.install_manager,
        recorded.request.install_manager
    );
    assert_eq!(run.request.python_source, PythonSourceInput::Managed);
    assert_eq!(run.request.comfyui_ref, ComfyRef::KeepCurrent);
    assert!(!run.request.allow_source_builds);
    assert!(run.request.curated_nodes.is_empty());
    assert!(run.curated.is_empty());
    assert_eq!(run.resolved.manager, recorded.resolved.manager);

    assert_eq!(execute(&world, &rebuilt).0, RunOutcome::Succeeded);
    assert_eq!(
        world.env(run.target_env.as_str()).python_version,
        recorded.profile.python
    );
    assert_eq!(
        world.env(run.target_env.as_str()).profile_revision,
        recorded.profile.revision
    );
    assert_eq!(world.run(&original), recorded);
    assert_eq!(*world.checkout.lock().unwrap(), checkout);
    assert_eq!(
        world.count(&format!(
            "install_managed_python_version:{}",
            recorded.profile.python
        )),
        1
    );
    assert_eq!(world.count("recreate_hf_tool"), 1);
    assert_no_checkout_or_node_publication(&world);
}

#[test]
fn rebuild_missing_recorded_snapshot_refuses_newest_profile_revision() {
    let (world, original) = provisioned_installation();
    let mut newest = world.run(&original).profile;
    newest.revision += 1;
    *world.profiles.lock().unwrap() = vec![newest];
    world.runs.lock().unwrap().remove(original.as_str());

    assert!(matches!(
        register(&world, rebuild_request()),
        Err(EnvironmentAppError::ProfileRevisionUnavailable)
    ));
    assert_eq!(world.count("relocation.accept"), 0);
    assert_eq!(world.count("create_venv"), 0);
    assert_eq!(world.count("publish_manifest"), 0);
}

#[test]
fn rebuild_preserves_checkout_and_user_data() {
    let (world, original) = provisioned_installation();
    let previous = world.env(world.run(&original).target_env.as_str());
    let checkout = world.checkout.lock().unwrap().clone();
    let requirements = vec![
        PathBuf::from(ROOT).join("ComfyUI/custom_nodes/enabled/requirements.txt"),
        PathBuf::from(ROOT).join("ComfyUI/custom_nodes.disabled/disabled/requirements.txt"),
    ];
    *world.node_requirements.lock().unwrap() = requirements.clone();
    *world.node_installed.lock().unwrap() = vec!["Node-alpha".into(), "user-node".into()];
    *world.draft.lock().unwrap() = Some("retained user options and external roots".into());
    let draft = world.draft.lock().unwrap().clone();
    let nodes = world.node_installed.lock().unwrap().clone();
    world.installed.lock().unwrap().clear();

    let rebuilt = register(&world, rebuild_request()).unwrap();
    assert_eq!(execute(&world, &rebuilt).0, RunOutcome::Succeeded);
    let run = world.run(&rebuilt);
    assert_ne!(run.target_env, previous.id);
    assert_ne!(run.python_target.env_path, previous.path);
    assert_eq!(world.env(previous.id.as_str()), previous);
    assert_eq!(*world.checkout.lock().unwrap(), checkout);
    assert_eq!(*world.node_requirements.lock().unwrap(), requirements);
    assert_eq!(*world.node_installed.lock().unwrap(), nodes);
    assert_eq!(*world.draft.lock().unwrap(), draft);
    assert_eq!(world.count("install:node"), 2);
    for node in ["enabled", "disabled"] {
        assert_eq!(
            world.node_outcome(&run.target_env, node),
            Some(DependencyState::Ok)
        );
    }
    assert!(
        world
            .install_envs
            .lock()
            .unwrap()
            .iter()
            .all(|path| path == &run.python_target.env_path),
        "all package mutations target only the new environment"
    );
    assert!(
        world
            .calls()
            .iter()
            .all(|call| !call.starts_with("remove_environment:"))
    );
    assert_no_checkout_or_node_publication(&world);
}

#[test]
fn rebuild_node_dependency_failure_preserves_previous_selection() {
    let (world, original) = provisioned_installation();
    let previous = world.env(world.run(&original).target_env.as_str());
    let manifest = world.manifest.lock().unwrap().clone();
    *world.node_requirements.lock().unwrap() = vec![
        PathBuf::from(ROOT).join("ComfyUI/custom_nodes/enabled/requirements.txt"),
        PathBuf::from(ROOT).join("ComfyUI/custom_nodes.disabled/disabled/requirements.txt"),
    ];
    world.installed.lock().unwrap().clear();
    world.succeed_first("install:node", 1);
    world.fail(
        "install:node",
        ProvisionError::Failed("disabled node dependency failed".into()),
    );

    let rebuilt = register(&world, rebuild_request()).unwrap();
    let RunOutcome::Failed(failure) = execute(&world, &rebuilt).0 else {
        panic!("every node requirement failure must prevent rebuild activation")
    };
    let run = world.run(&rebuilt);
    assert_eq!(failure.step, StepId::ComfyRequirements);
    assert!(!failure.can_skip);
    assert_eq!(run.activation, ActivationPhase::None);
    assert_eq!(world.env(run.target_env.as_str()).state(), EnvState::Failed);
    assert_eq!(
        world.node_outcome(&run.target_env, "enabled"),
        Some(DependencyState::Ok)
    );
    assert_eq!(
        world.node_outcome(&run.target_env, "disabled"),
        Some(DependencyState::Failed)
    );
    assert_eq!(world.active_id().as_deref(), Some(previous.id.as_str()));
    assert_eq!(world.env(previous.id.as_str()), previous);
    assert_eq!(*world.manifest.lock().unwrap(), manifest);
    assert_eq!(world.count("publish_manifest"), 0);
    assert_eq!(world.count("commit_activation"), 0);
    assert_no_checkout_or_node_publication(&world);
}

#[test]
fn rebuilding_an_adopted_environment_selects_a_new_owned_target() {
    let (world, original) = provisioned_installation();
    let mut previous = world.env(world.run(&original).target_env.as_str());
    previous.path = PathBuf::from("/external/adopted-venv");
    previous.authorized_location = previous.path.clone();
    previous.ownership = Ownership::Adopted;
    previous.source = PythonSourceKind::CustomVenv;
    previous.python_version = PythonVersion::new(3, 13, 17);
    world
        .envs
        .lock()
        .unwrap()
        .insert(previous.id.as_str().to_owned(), previous.clone());
    if let ManifestState::Valid(manifest) = &mut *world.manifest.lock().unwrap() {
        manifest.env_path = previous.path.clone();
        manifest.ownership = previous.ownership;
        manifest.python_version = previous.python_version;
    }
    *world.venv_version.lock().unwrap() = previous.python_version;
    world.installed.lock().unwrap().clear();

    let rebuilt = register(&world, rebuild_request()).unwrap();
    let run = world.run(&rebuilt);
    assert_eq!(run.python_target.ownership, Ownership::AppOwned);
    assert_eq!(run.python_target.source, PythonSourceKind::Managed);
    assert!(run.python_target.create_venv);
    assert_ne!(run.target_env, previous.id);
    assert_eq!(
        run.python_target.env_path,
        PathBuf::from(ROOT)
            .join("envs")
            .join(run.target_env.as_str())
    );
    assert_eq!(run.profile.python, previous.python_version);
    assert_eq!(execute(&world, &rebuilt).0, RunOutcome::Succeeded);
    assert_eq!(world.env(previous.id.as_str()), previous);
    assert_eq!(world.count("inspect_venv"), 0);
    assert_eq!(
        world.count(&format!(
            "install_managed_python_version:{}",
            previous.python_version
        )),
        1
    );
    assert!(
        world
            .install_envs
            .lock()
            .unwrap()
            .iter()
            .all(|path| path == &run.python_target.env_path)
    );
    assert!(
        world
            .calls()
            .iter()
            .all(|call| !call.starts_with("remove_environment:"))
    );
}

#[test]
fn successful_rebuild_holds_maintenance_until_the_controlled_restart() {
    let (world, _) = provisioned_installation();
    world.installed.lock().unwrap().clear();
    let id = register(&world, rebuild_request()).unwrap();
    let executor = ExecuteSetup::new(world.deps(), world.nodes());
    assert_eq!(
        block_on(executor.execute_for_restart(&id, Arc::new(NullSink))).unwrap(),
        RunOutcome::Succeeded
    );
    assert!(block_on(world.gate.try_acquire()).is_err());
    drop(executor);
    assert!(block_on(world.gate.try_acquire()).is_ok());
}

/// Records the installation as copied: the manifest still names the original root.
fn record_as_moved(world: &World) -> PathBuf {
    let old = PathBuf::from("/recorded-original-installation");
    if let ManifestState::Valid(manifest) = &mut *world.manifest.lock().unwrap() {
        manifest.install_root = old.clone();
    }
    old
}

fn seed_run(
    world: &World,
    template: &ProvisioningRun,
    id: &str,
    root: &Path,
    status: RunStatus,
    created_at: &str,
) -> ProvisioningRun {
    let mut run = template.clone();
    run.id = RunId::new(id).unwrap();
    run.request.install_root = root.to_owned();
    run.status = status;
    run.created_at = created_at.into();
    run.updated_at = created_at.into();
    world
        .runs
        .lock()
        .unwrap()
        .insert(id.to_owned(), run.clone());
    run
}

#[test]
fn reconcile_startup_processes_current_root_runs_when_moved() {
    let (world, original) = provisioned_installation();
    let old = record_as_moved(&world);
    let template = world.run(&original);
    let mut current = seed_run(
        &world,
        &template,
        "current-root-rebuild",
        Path::new(ROOT),
        RunStatus::Running,
        "2026-10-17T00:00:01Z",
    );
    current.steps[0].status = StepStatus::Running;
    world
        .runs
        .lock()
        .unwrap()
        .insert(current.id.as_str().to_owned(), current.clone());
    let copied_running = seed_run(
        &world,
        &template,
        "copied-running",
        &old,
        RunStatus::Running,
        "2026-10-17T00:00:02Z",
    );
    let mut copied_intent = seed_run(
        &world,
        &template,
        "copied-intent",
        &old,
        RunStatus::Failed,
        "2026-10-17T00:00:03Z",
    );
    copied_intent.activation = ActivationPhase::Intent;
    world
        .runs
        .lock()
        .unwrap()
        .insert(copied_intent.id.as_str().to_owned(), copied_intent.clone());
    let manifest = world.manifest.lock().unwrap().clone();

    let report = block_on(ReconcileStartup::new(world.deps()).execute()).unwrap();
    assert_eq!(
        report,
        ReconcileReport {
            interrupted_runs: 1,
            ..ReconcileReport::default()
        }
    );
    let interrupted = world.run(&current.id);
    assert_eq!(interrupted.status, RunStatus::Interrupted);
    assert_eq!(interrupted.steps[0].status, StepStatus::Interrupted);
    assert_eq!(world.run(&copied_running.id), copied_running);
    assert_eq!(world.run(&copied_intent.id), copied_intent);
    assert_eq!(*world.manifest.lock().unwrap(), manifest);
    assert_eq!(world.count("publish_manifest"), 0);

    // The interrupted rebuild is retryable even though copied runs remain unfinished.
    block_on(StartSetup::new(world.deps()).execute(SetupAction::Retry {
        run_id: current.id.clone(),
        manager_consent: None,
        curated_decisions: Vec::new(),
    }))
    .unwrap();
}

#[test]
fn rebuild_status_reports_run_in_progress_while_rebuilding() {
    let (world, original) = provisioned_installation();
    let old = record_as_moved(&world);
    let status = || block_on(GetSetupStatus::new(world.deps()).execute()).unwrap();
    assert_eq!(
        status().phase,
        SetupPhase::RebuildRequired(RebuildReason::Moved)
    );

    let template = world.run(&original);
    seed_run(
        &world,
        &template,
        "copied-running",
        &old,
        RunStatus::Running,
        "2026-10-17T00:00:01Z",
    );
    assert_eq!(
        status().phase,
        SetupPhase::RebuildRequired(RebuildReason::Moved),
        "a run bound to the original root is history, not progress"
    );

    seed_run(
        &world,
        &template,
        "current-root-rebuild",
        Path::new(ROOT),
        RunStatus::Running,
        "2026-10-17T00:00:02Z",
    );
    assert_eq!(status().phase, SetupPhase::RunInProgress);
}
