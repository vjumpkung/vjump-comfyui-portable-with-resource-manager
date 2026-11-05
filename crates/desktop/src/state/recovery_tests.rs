use std::{fs, path::Path};

use application::{
    comfy_runtime::{
        FrozenUpdate, RuntimeSession, RuntimeStore, SessionId, UpdatePhase, UpdateRun,
    },
    environment::{
        ActivationPhase, ActivationStore, ComfyRef, EnvironmentRepository, InstallManifest,
        InstallManifestStore, MANIFEST_SCHEMA_VERSION, ProfileCatalog, ProvisioningRun,
        ProvisioningRunRepository, PythonSourceInput, PythonTarget, RebuildReason, RecoveryReason,
        ResolvedSources, RootSwitchError, RunId, RunStatus, SetupPhase, SetupRequest, StepStatus,
    },
};
use domain::{
    comfy_runtime::{LaunchOptions, Revision},
    environment::{
        EnvId, EnvState, NewEnvironment, Ownership, PinSet, PythonEnvironment, PythonSourceKind,
        SetupMode, SetupProfile,
    },
};

use super::*;

struct NoPicker;

#[async_trait::async_trait]
impl FilePicker for NoPicker {
    async fn pick_folder(&self, _: Option<String>) -> Option<PathBuf> {
        None
    }

    async fn pick_file(&self, _: Option<String>, _: Vec<String>) -> Option<PathBuf> {
        None
    }
}

fn environment(root: &Path, id: &str, profile: &SetupProfile) -> PythonEnvironment {
    PythonEnvironment::restore(
        NewEnvironment {
            id: EnvId::new(id).unwrap(),
            path: root.join("envs").join(id),
            ownership: Ownership::AppOwned,
            source: PythonSourceKind::Managed,
            base_interpreter: None,
            python_version: profile.python,
            profile_id: profile.id.clone(),
            profile_revision: profile.revision,
            authorized_location: root.join("envs").join(id),
            now: "2026-10-19T00:00:00Z".into(),
        },
        EnvState::Verified,
        PinSet::new(),
        Some("recorded verification".into()),
        "2026-10-19T00:00:00Z".into(),
    )
}

fn completed_run(root: &Path, env: &PythonEnvironment, profile: SetupProfile) -> ProvisioningRun {
    ProvisioningRun {
        id: RunId::new("run-complete").unwrap(),
        request: SetupRequest {
            mode: SetupMode::FirstRun,
            install_root: root.to_owned(),
            python_source: PythonSourceInput::Managed,
            profile_id: profile.id.clone(),
            comfyui_ref: ComfyRef::KeepCurrent,
            install_manager: false,
            accelerators: Vec::new(),
            curated_nodes: Vec::new(),
            clean_cache: false,
            confirm_adopted_changes: false,
            allow_source_builds: false,
            third_party_consent: false,
        },
        profile,
        target_env: env.id.clone(),
        previous_env: None,
        python_target: PythonTarget {
            ownership: env.ownership,
            source: env.source,
            env_path: env.path.clone(),
            interpreter: None,
            create_venv: true,
            development_files_ok: None,
        },
        resolved: ResolvedSources::default(),
        components: Default::default(),
        steps: ProvisioningRun::fresh_steps()
            .into_iter()
            .map(|mut step| {
                step.status = StepStatus::Succeeded;
                step
            })
            .collect(),
        status: RunStatus::Succeeded,
        cancel_requested: false,
        failure: None,
        activation: ActivationPhase::Committed,
        manager_consent: None,
        curated: Vec::new(),
        created_at: "2026-10-19T00:00:00Z".into(),
        updated_at: "2026-10-19T00:00:00Z".into(),
    }
}

#[tokio::test]
async fn relocation_is_detected_before_any_reconciliation() {
    let directory = tempfile::tempdir().unwrap();
    let old_root = directory.path().join("recorded old install");
    let current_root = directory.path().join("moved install ไทย");
    fs::create_dir_all(current_root.join("data")).unwrap();
    fs::create_dir_all(current_root.join("ComfyUI/custom_nodes")).unwrap();
    let node = current_root.join("ComfyUI/custom_nodes/manual-node.py");
    fs::write(&node, "# retain manual node\n").unwrap();
    let publication = current_root.join(".ComfyUI.publish.json");
    fs::write(&publication, "interrupted publication; retain for recovery").unwrap();

    let profile = BundledProfiles::load()
        .unwrap()
        .get("win-py313-torch2141-cu130")
        .unwrap();
    let active = environment(&old_root, "env-active", &profile);
    let published = environment(&old_root, "env-published", &profile);
    let db = Db::open(current_root.join("data/app.db")).await.unwrap();
    let environments = SqliteEnvironmentStore::new(db.clone());
    EnvironmentRepository::save(&environments, &active)
        .await
        .unwrap();
    EnvironmentRepository::save(&environments, &published)
        .await
        .unwrap();
    let complete = completed_run(&old_root, &active, profile.clone());
    environments.insert(&complete).await.unwrap();
    environments
        .commit_activation(&complete, &active.id)
        .await
        .unwrap();
    let mut interrupted = completed_run(&old_root, &published, profile);
    interrupted.id = RunId::new("run-interrupted").unwrap();
    interrupted.previous_env = Some(active.id.clone());
    interrupted.status = RunStatus::Running;
    interrupted.activation = ActivationPhase::Intent;
    interrupted.created_at = "2026-10-19T01:00:00Z".into();
    interrupted.updated_at = interrupted.created_at.clone();
    environments.insert(&interrupted).await.unwrap();

    let runtime = SqliteRuntimeStore::new(db.clone());
    let session = RuntimeSession {
        id: SessionId::new("session-before-move").unwrap(),
        env_id: active.id.clone(),
        interpreter: infrastructure::environment::layout::venv_python(&active.path),
        checkout: old_root.join("ComfyUI"),
        revision: Revision {
            commit: "a".repeat(40),
            tag: None,
            branch: None,
        },
        options: LaunchOptions::default(),
        started_at: "2026-10-19T00:00:00Z".into(),
        identity: None,
    };
    runtime.save_session(&session, false).await.unwrap();
    let update = UpdateRun {
        id: SessionId::new("update-before-move").unwrap(),
        env_id: active.id.clone(),
        old_commit: "a".repeat(40),
        target: FrozenUpdate {
            commit: "b".repeat(40),
            tag: None,
            branch: None,
            fast_forward: false,
        },
        pins: PinSet::new(),
        phase: UpdatePhase::Publishing,
        failure: None,
        created_at: "2026-10-19T00:00:00Z".into(),
    };
    runtime.save_update(&update).await.unwrap();

    FsManifestStore::new()
        .publish(&InstallManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            install_root: current_root.clone(),
            generation: 2,
            env_id: published.id.clone(),
            env_path: published.path.clone(),
            ownership: published.ownership,
            python_version: published.python_version,
            profile_id: published.profile_id.clone(),
            profile_revision: published.profile_revision,
            comfyui_reference: None,
            comfyui_commit: Some("a".repeat(40)),
            run_id: interrupted.id.clone(),
            activated_at: "2026-10-19T01:00:00Z".into(),
            components: Default::default(),
        })
        .await
        .unwrap();
    let manifest_path = current_root.join("install.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["install_root"] = serde_json::to_value(&old_root).unwrap();
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let manifest_before = fs::read(&manifest_path).unwrap();

    let state = AppState::compose(
        &ResolvedRoot {
            root: current_root.clone(),
            source: RootSource::Bootstrap,
        },
        Arc::new(NoPicker),
    )
    .await
    .unwrap();

    assert_eq!(state.services().err().unwrap().code, "rebuild_required");
    let setup = state.setup_services().unwrap();
    assert!(Arc::ptr_eq(&state.gate, &setup.deps.gate));
    assert!(setup.setup_status.execute().await.is_ok());
    let mut rebuild = interrupted.request.clone();
    rebuild.mode = SetupMode::Rebuild;
    rebuild.install_root = current_root.clone();
    let refusal = setup
        .start_setup
        .execute(application::environment::SetupAction::New(Box::new(
            rebuild,
        )))
        .await
        .unwrap_err();
    assert_eq!(IpcError::from(refusal).code, "relocation_recovery_required");
    assert!(state.opener().is_ok());
    assert_eq!(environments.active().await.unwrap(), Some(active));
    assert_eq!(
        EnvironmentRepository::get(&environments, &published.id)
            .await
            .unwrap(),
        Some(published)
    );
    assert_eq!(
        ProvisioningRunRepository::get(&environments, &complete.id)
            .await
            .unwrap(),
        Some(complete)
    );
    assert_eq!(
        ProvisioningRunRepository::get(&environments, &interrupted.id)
            .await
            .unwrap(),
        Some(interrupted)
    );
    assert_eq!(runtime.unfinished_sessions().await.unwrap(), vec![session]);
    assert_eq!(runtime.unfinished_update().await.unwrap(), Some(update));
    assert_eq!(fs::read(&manifest_path).unwrap(), manifest_before);
    assert_eq!(
        fs::read_to_string(&publication).unwrap(),
        "interrupted publication; retain for recovery"
    );
    assert_eq!(fs::read_to_string(node).unwrap(), "# retain manual node\n");
    assert!(!old_root.exists());
    for relative in ["data/logs", "data/downloads", "ComfyUI/models"] {
        assert!(!current_root.join(relative).exists(), "{relative}");
    }
}

#[tokio::test]
async fn startup_failure_preserves_debug_override_for_root_recovery() {
    let directory = tempfile::tempdir().unwrap();
    let resolved = ResolvedRoot {
        root: directory.path().join("fixed debug root"),
        source: RootSource::DebugOverride,
    };
    let recovery = RecoveryInfo::startup_failed_for(&resolved);
    assert_eq!(recovery.root, Some(services_root(&resolved)));
    let pointer = directory.path().join("bootstrap.json");
    let opener = FsExistingInstallOpener::new(
        recovery.root,
        None,
        pointer.clone(),
        Arc::new(TokioMaintenanceGate::new()),
    );
    let candidate = directory.path().join("candidate");
    assert_eq!(
        opener.open_existing(&candidate).await,
        Err(RootSwitchError::FixedByOverride)
    );
    assert_eq!(
        opener.select_new(&candidate).await,
        Err(RootSwitchError::FixedByOverride)
    );
    assert!(!pointer.exists());
    assert!(!candidate.exists());
}
fn bundled_profile() -> SetupProfile {
    BundledProfiles::load()
        .unwrap()
        .get("win-py313-torch2141-cu130")
        .unwrap()
}

async fn compose_at(root: &Path) -> AppState {
    let state = AppState::compose(
        &ResolvedRoot {
            root: root.to_owned(),
            source: RootSource::Bootstrap,
        },
        Arc::new(NoPicker),
    )
    .await
    .unwrap();
    state.start_services();
    state
}

#[test]
fn startup_publishes_before_watching_and_starting_services() {
    use application::comfy_runtime::{RuntimeEvents, RuntimeSubscription};
    use std::sync::{Mutex, OnceLock, mpsc};
    use tauri::{
        Manager, RunEvent,
        test::{mock_builder, mock_context, noop_assets},
    };

    struct WatchEvents {
        app: tauri::AppHandle<tauri::test::MockRuntime>,
        hub: RuntimeLogHub,
        observed: mpsc::Sender<()>,
    }
    impl RuntimeEvents for WatchEvents {
        fn subscribe(&self, after: Option<u64>) -> RuntimeSubscription {
            assert!(
                self.app.try_state::<AppState>().is_some(),
                "watch must follow manage"
            );
            self.observed.send(()).unwrap();
            self.hub.subscribe(after)
        }
        fn unsubscribe(&self, id: u64) {
            self.hub.unsubscribe(id);
        }
    }

    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().to_owned();
    let worker = Arc::new(Mutex::new(None));
    let event_worker = worker.clone();
    let (observed, observation) = mpsc::channel();
    let mut checked = false;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let mut context = mock_context(noop_assets());
    context
        .config_mut()
        .app
        .windows
        .push(tauri::utils::config::WindowConfig {
            label: crate::MANAGER_WINDOW.into(),
            ..Default::default()
        });
    crate::run_with_runtime(
        mock_builder(),
        context,
        Arc::new(OnceLock::new()),
        worker.clone(),
        move |app| {
            let mut state = tauri::async_runtime::block_on(AppState::compose(
                &ResolvedRoot {
                    root,
                    source: RootSource::DebugOverride,
                },
                Arc::new(NoPicker),
            ))?;
            let services = state.services.as_mut().unwrap();
            assert!(
                services.library.run.lock().unwrap().is_none(),
                "compose must not start scans"
            );
            assert!(
                services.downloads.run.lock().unwrap().is_none(),
                "compose must not start downloads"
            );
            services.comfy.events = Arc::new(WatchEvents {
                app: app.clone(),
                hub: services.comfy.hub.clone(),
                observed,
            });
            Ok(state)
        },
        |_, code| panic!("unexpected startup exit {code}"),
        move |app, event| match event {
            RunEvent::MainEventsCleared if !checked => {
                assert!(
                    std::time::Instant::now() < deadline,
                    "services did not start"
                );
                if !event_worker
                    .lock()
                    .unwrap()
                    .as_ref()
                    .is_some_and(|worker| worker.is_finished())
                    || observation.try_recv().is_err()
                {
                    return;
                }
                checked = true;
                let state = app.state::<AppState>();
                let services = state.services().unwrap();
                assert!(services.library.run.lock().unwrap().is_some());
                assert!(services.downloads.run.lock().unwrap().is_some());
                app.get_webview_window(crate::MANAGER_WINDOW)
                    .unwrap()
                    .destroy()
                    .unwrap();
            }
            RunEvent::ExitRequested { .. } => {}
            event => crate::on_run_event(app, event),
        },
    );
    worker.lock().unwrap().take().unwrap().join().unwrap();
}

#[tokio::test]
async fn startup_exit_discards_composed_state_without_starting_services() {
    let directory = tempfile::tempdir().unwrap();
    let state = AppState::compose(
        &ResolvedRoot {
            root: directory.path().to_owned(),
            source: RootSource::DebugOverride,
        },
        Arc::new(NoPicker),
    )
    .await
    .unwrap();
    let weak = Arc::downgrade(&state.close);
    let gate = crate::startup::StartupGate::default();
    assert!(gate.begin_exit());
    let state = gate
        .publish(state, |_| panic!("publication after exit"))
        .err()
        .unwrap();
    let services = state.services().unwrap();
    assert!(services.library.run.lock().unwrap().is_none());
    assert!(services.downloads.run.lock().unwrap().is_none());
    drop(state);
    assert!(weak.upgrade().is_none());
}

/// Publishes `install.json` in `root`; `recorded` is the root the manifest names, which differs
/// after the installation was copied.
async fn publish_manifest(
    root: &Path,
    recorded: &Path,
    env: &PythonEnvironment,
    run: &ProvisioningRun,
    generation: u64,
) {
    FsManifestStore::new()
        .publish(&InstallManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            install_root: root.to_owned(),
            generation,
            env_id: env.id.clone(),
            env_path: env.path.clone(),
            ownership: env.ownership,
            python_version: env.python_version,
            profile_id: env.profile_id.clone(),
            profile_revision: env.profile_revision,
            comfyui_reference: None,
            comfyui_commit: Some("a".repeat(40)),
            run_id: run.id.clone(),
            activated_at: "2026-10-19T01:00:00Z".into(),
            components: Default::default(),
        })
        .await
        .unwrap();
    if recorded != root {
        let path = root.join("install.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        manifest["install_root"] = serde_json::to_value(recorded).unwrap();
        fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    }
}

async fn stored(environments: &SqliteEnvironmentStore, run: &ProvisioningRun) -> ProvisioningRun {
    ProvisioningRunRepository::get(environments, &run.id)
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn interrupted_activation_is_reconciled_instead_of_restricted() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("same install");
    fs::create_dir_all(root.join("data")).unwrap();
    let profile = bundled_profile();
    let previous = environment(&root, "env-previous", &profile);
    let candidate = environment(&root, "env-candidate", &profile);
    let db = Db::open(root.join("data/app.db")).await.unwrap();
    let environments = SqliteEnvironmentStore::new(db.clone());
    EnvironmentRepository::save(&environments, &previous)
        .await
        .unwrap();
    EnvironmentRepository::save(&environments, &candidate)
        .await
        .unwrap();
    let complete = completed_run(&root, &previous, profile.clone());
    environments.insert(&complete).await.unwrap();
    environments
        .commit_activation(&complete, &previous.id)
        .await
        .unwrap();
    // The crash window: the manifest names the candidate, the database still names the previous.
    let mut interrupted = completed_run(&root, &candidate, profile);
    interrupted.id = RunId::new("run-interrupted").unwrap();
    interrupted.previous_env = Some(previous.id.clone());
    interrupted.status = RunStatus::Running;
    interrupted.activation = ActivationPhase::ManifestPublished;
    interrupted.created_at = "2026-10-19T01:00:00Z".into();
    interrupted.updated_at = interrupted.created_at.clone();
    environments.insert(&interrupted).await.unwrap();
    publish_manifest(&root, &root, &candidate, &interrupted, 2).await;

    let state = compose_at(&root).await;

    assert!(state.services().is_ok());
    assert_eq!(environments.active().await.unwrap(), Some(candidate));
    let run = ProvisioningRunRepository::get(&environments, &interrupted.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.status, RunStatus::Succeeded);
    assert_eq!(run.activation, ActivationPhase::Committed);
}

#[tokio::test]
async fn broken_interpreter_keeps_normal_composition() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("same install");
    fs::create_dir_all(root.join("data")).unwrap();
    let profile = bundled_profile();
    let active = environment(&root, "env-active", &profile);
    fs::create_dir_all(&active.path).unwrap();
    fs::write(
        active.path.join("pyvenv.cfg"),
        "home = /unavailable\nversion = 3.12.0\n",
    )
    .unwrap();
    let db = Db::open(root.join("data/app.db")).await.unwrap();
    let environments = SqliteEnvironmentStore::new(db.clone());
    EnvironmentRepository::save(&environments, &active)
        .await
        .unwrap();
    let complete = completed_run(&root, &active, profile);
    environments.insert(&complete).await.unwrap();
    environments
        .commit_activation(&complete, &active.id)
        .await
        .unwrap();
    publish_manifest(&root, &root, &active, &complete, 1).await;

    let state = compose_at(&root).await;

    let services = state.services().unwrap();
    assert_eq!(
        services.setup.setup_status.execute().await.unwrap().phase,
        SetupPhase::RebuildRequired(RebuildReason::InterpreterBroken)
    );
}

#[tokio::test]
async fn restricted_start_reconciles_only_current_root_runs() {
    let directory = tempfile::tempdir().unwrap();
    let old_root = directory.path().join("recorded old install");
    let current_root = directory.path().join("moved install");
    fs::create_dir_all(current_root.join("data")).unwrap();
    let profile = bundled_profile();
    let active = environment(&old_root, "env-active", &profile);
    let published = environment(&old_root, "env-published", &profile);
    let db = Db::open(current_root.join("data/app.db")).await.unwrap();
    let environments = SqliteEnvironmentStore::new(db.clone());
    EnvironmentRepository::save(&environments, &active)
        .await
        .unwrap();
    EnvironmentRepository::save(&environments, &published)
        .await
        .unwrap();
    let complete = completed_run(&old_root, &active, profile.clone());
    environments.insert(&complete).await.unwrap();
    environments
        .commit_activation(&complete, &active.id)
        .await
        .unwrap();
    publish_manifest(&current_root, &old_root, &active, &complete, 1).await;

    let mut copied_running = completed_run(&old_root, &published, profile.clone());
    copied_running.id = RunId::new("run-copied-running").unwrap();
    copied_running.status = RunStatus::Running;
    copied_running.activation = ActivationPhase::None;
    copied_running.created_at = "2026-10-19T01:00:00Z".into();
    copied_running.updated_at = copied_running.created_at.clone();
    let mut copied_intent = copied_running.clone();
    copied_intent.id = RunId::new("run-copied-intent").unwrap();
    copied_intent.previous_env = Some(active.id.clone());
    copied_intent.activation = ActivationPhase::Intent;
    // A rebuild that was running when the app died after the move.
    let rebuilt = environment(&current_root, "env-rebuilt", &profile);
    let mut rebuild = completed_run(&current_root, &rebuilt, profile.clone());
    rebuild.id = RunId::new("run-current-rebuild").unwrap();
    rebuild.request.mode = SetupMode::Rebuild;
    rebuild.previous_env = Some(active.id.clone());
    rebuild.status = RunStatus::Running;
    rebuild.steps[0].status = StepStatus::Running;
    rebuild.created_at = "2026-10-19T02:00:00Z".into();
    rebuild.updated_at = rebuild.created_at.clone();
    for run in [&copied_running, &copied_intent, &rebuild] {
        environments.insert(run).await.unwrap();
    }
    let manifest_path = current_root.join("install.json");
    let manifest_before = fs::read(&manifest_path).unwrap();

    let state = compose_at(&current_root).await;

    assert_eq!(state.services().err().unwrap().code, "rebuild_required");
    let setup = state.setup_services().unwrap();
    let interrupted = ProvisioningRunRepository::get(&environments, &rebuild.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(interrupted.status, RunStatus::Interrupted);
    assert_eq!(interrupted.steps[0].status, StepStatus::Interrupted);
    // The copied runs are history: untouched, whatever their status or activation phase.
    for run in [&copied_running, &copied_intent, &complete] {
        assert_eq!(&stored(&environments, run).await, run);
    }
    assert_eq!(fs::read(&manifest_path).unwrap(), manifest_before);
    assert_eq!(environments.active().await.unwrap(), Some(active.clone()));
    setup
        .start_setup
        .execute(application::environment::SetupAction::Retry {
            run_id: rebuild.id.clone(),
            manager_consent: None,
            curated_decisions: Vec::new(),
        })
        .await
        .unwrap();
    drop(state);

    // A rebuild that crashed after its candidate verified is rolled forward at the next start.
    let candidate = environment(&current_root, "env-candidate", &profile);
    EnvironmentRepository::save(&environments, &candidate)
        .await
        .unwrap();
    let mut intent = completed_run(&current_root, &candidate, profile);
    intent.id = RunId::new("run-current-intent").unwrap();
    intent.request.mode = SetupMode::Rebuild;
    intent.previous_env = Some(active.id.clone());
    intent.status = RunStatus::Running;
    intent.activation = ActivationPhase::Intent;
    intent.created_at = "2026-10-19T03:00:00Z".into();
    intent.updated_at = intent.created_at.clone();
    environments.insert(&intent).await.unwrap();

    let state = compose_at(&current_root).await;

    let ManifestState::Valid(manifest) = FsManifestStore::new().read(&current_root).await.unwrap()
    else {
        panic!("the reconciled activation must publish a manifest")
    };
    assert_eq!(manifest.env_id, candidate.id);
    assert!(application::environment::same_location(
        &manifest.install_root,
        &current_root
    ));
    assert_eq!(environments.active().await.unwrap(), Some(candidate));
    assert!(
        state.services().is_ok(),
        "the root no longer counts as moved"
    );
    for run in [&copied_running, &copied_intent] {
        assert_eq!(&stored(&environments, run).await, run);
    }
}

#[tokio::test]
async fn e3_valid_manifest_with_missing_database_recovers_instead_of_failing_startup() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("same install");
    fs::create_dir_all(&root).unwrap();
    let profile = bundled_profile();
    let env = environment(&root, "env-recorded", &profile);
    let run = completed_run(&root, &env, profile);
    publish_manifest(&root, &root, &env, &run, 1).await;
    assert!(!root.join("data").exists());

    let state = compose_at(&root).await;

    let services = state.services().unwrap();
    assert_eq!(
        services.setup.setup_status.execute().await.unwrap().phase,
        SetupPhase::Recovery(RecoveryReason::ManifestMismatch)
    );
}

#[tokio::test]
async fn fresh_root_composes_without_creating_comfyui() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("fresh install");
    fs::create_dir_all(&root).unwrap();

    let state = compose_at(&root).await;

    // The setup clone must find no ComfyUI folder to merge into.
    assert!(
        matches!(
            fs::symlink_metadata(root.join("ComfyUI")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound
        ),
        "composition created ComfyUI"
    );
    // The wizard's services are available.
    let services = state.setup_services().unwrap();
    assert_eq!(
        services.setup_status.execute().await.unwrap().phase,
        SetupPhase::NeedsSetup
    );
    assert!(state.services().is_ok());
}

#[tokio::test]
async fn activated_root_without_comfyui_still_creates_models() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("same install");
    fs::create_dir_all(&root).unwrap();
    let profile = bundled_profile();
    let env = environment(&root, "env-recorded", &profile);
    let run = completed_run(&root, &env, profile);
    publish_manifest(&root, &root, &env, &run, 1).await;
    assert!(!root.join("ComfyUI").exists());

    let state = compose_at(&root).await;

    assert!(state.services().is_ok());
    assert!(root.join("ComfyUI").join("models").is_dir());
}

async fn compose_without_services(root: &Path) -> AppState {
    AppState::compose(
        &ResolvedRoot {
            root: root.to_owned(),
            source: RootSource::Bootstrap,
        },
        Arc::new(NoPicker),
    )
    .await
    .unwrap()
}

/// A root that already holds ComfyUI with one hand-made custom node.
fn root_with_manual_node(directory: &tempfile::TempDir) -> PathBuf {
    let root = directory.path().join("existing install");
    let pack = root
        .join("ComfyUI")
        .join("custom_nodes")
        .join("manual-pack");
    fs::create_dir_all(&pack).unwrap();
    fs::write(pack.join("__init__.py"), "# manual pack\n").unwrap();
    root
}

const WAIT: std::time::Duration = std::time::Duration::from_secs(30);

#[tokio::test]
async fn existing_root_composes_before_the_node_scan_and_scans_after_start_services() {
    let directory = tempfile::tempdir().unwrap();
    let root = root_with_manual_node(&directory);

    let state = compose_without_services(&root).await;

    let nodes = &state.services().unwrap().nodes;
    // Composition returned: readiness does not wait for the scan, which is only scheduled.
    assert!(nodes.scanning());
    assert!(nodes.service.list().await.unwrap().is_empty());
    state.start_services();
    tokio::time::timeout(WAIT, nodes.wait_for_scan())
        .await
        .unwrap();
    assert!(!nodes.scanning());
    let listed = nodes.service.list().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name.as_str(), "manual-pack");
}

#[tokio::test]
async fn fresh_root_schedules_no_node_scan() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("fresh install");
    fs::create_dir_all(&root).unwrap();

    let state = compose_without_services(&root).await;

    let nodes = &state.services().unwrap().nodes;
    assert!(!nodes.scanning());
    // Nothing is armed, so starting does nothing and never consumes the one-shot guard.
    nodes.start_scan();
    state.start_services();
    assert!(!nodes.scan_started.load(Ordering::SeqCst));
    tokio::time::timeout(WAIT, nodes.wait_for_scan())
        .await
        .unwrap();
}

#[tokio::test]
async fn the_node_scan_waits_for_maintenance_and_never_runs_twice() {
    let directory = tempfile::tempdir().unwrap();
    let root = root_with_manual_node(&directory);
    let state = compose_without_services(&root).await;
    let services = state.services().unwrap();
    let nodes = &services.nodes;
    let mut events = nodes.stack.hub.subscribe();
    // Maintenance that got the gate first.
    let lease = services.gate.acquire().await.unwrap();

    nodes.start_scan();
    nodes.start_scan();

    assert!(nodes.scan_started.load(Ordering::SeqCst));
    let mut waiter = Box::pin(nodes.wait_for_scan());
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(300), &mut waiter)
            .await
            .is_err(),
        "the waiter must block while the scan waits for the gate"
    );
    assert!(nodes.scanning());
    assert!(nodes.service.list().await.unwrap().is_empty());

    drop(lease);
    tokio::time::timeout(WAIT, waiter).await.unwrap();
    assert!(!nodes.scanning());
    assert_eq!(nodes.service.list().await.unwrap().len(), 1);
    // Exactly one worker finished: one completion event, then silence.
    let (_, event) = tokio::time::timeout(WAIT, events.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(event, NodeEvent::Changed);
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(300), events.recv())
            .await
            .is_err()
    );
    // A later start finds nothing armed.
    nodes.start_scan();
    assert!(!nodes.scanning());
}

#[tokio::test]
async fn the_scan_flag_is_kept_without_subscribers_and_wakes_a_waiter() {
    let directory = tempfile::tempdir().unwrap();
    let root = root_with_manual_node(&directory);
    let state = compose_without_services(&root).await;
    let nodes = &state.services().unwrap().nodes;
    // Armed by composition although nothing ever subscribed.
    assert!(nodes.scanning());
    let mut waiter = Box::pin(nodes.wait_for_scan());
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), &mut waiter)
            .await
            .is_err()
    );
    nodes.scan.send_replace(false);
    tokio::time::timeout(WAIT, waiter).await.unwrap();
    // A waiter that starts after the end returns at once.
    tokio::time::timeout(std::time::Duration::from_secs(1), nodes.wait_for_scan())
        .await
        .unwrap();
}

#[tokio::test]
async fn a_startup_scan_skipped_while_closing_keeps_rows_and_clears_the_scan_flag() {
    let directory = tempfile::tempdir().unwrap();
    let root = root_with_manual_node(&directory);
    let state = compose_without_services(&root).await;
    let nodes = &state.services().unwrap().nodes;
    let pause = nodes.service.scans().pause();

    nodes.start_scan();
    tokio::time::timeout(WAIT, nodes.wait_for_scan())
        .await
        .unwrap();

    assert!(!nodes.scanning());
    assert!(nodes.service.scans().incomplete());
    assert!(nodes.service.list().await.unwrap().is_empty());
    drop(pause);
    assert!(!nodes.service.scans().closing());
    nodes.service.rescan().await.unwrap();
    assert!(!nodes.service.scans().incomplete());
    let listed = nodes.service.list().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name.as_str(), "manual-pack");
}

#[test]
fn e3_startup_logging_is_withheld_from_a_missing_bootstrap_root() {
    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join("vanished install");
    let bootstrap = |root: &Path| ResolvedRoot {
        root: root.to_owned(),
        source: RootSource::Bootstrap,
    };
    assert_eq!(startup_logs_dir(&bootstrap(&missing)), None);
    assert!(!missing.exists());

    let existing = directory.path().join("existing install");
    fs::create_dir_all(&existing).unwrap();
    assert_eq!(
        startup_logs_dir(&bootstrap(&existing)),
        Some(existing.join("data/logs"))
    );
    assert_eq!(fs::read_dir(&existing).unwrap().count(), 0);

    let first_run = ResolvedRoot {
        root: directory.path().join("new install"),
        source: RootSource::Default,
    };
    assert_eq!(
        startup_logs_dir(&first_run),
        Some(first_run.root.join("data/logs"))
    );
    assert!(!first_run.root.exists());
}
