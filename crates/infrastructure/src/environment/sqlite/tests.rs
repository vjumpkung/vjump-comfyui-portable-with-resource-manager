use application::environment::{ProfileCatalog, StepId};

use super::*;
use crate::environment::profiles::BundledProfiles;

async fn store() -> (SqliteEnvironmentStore, tempfile::TempDir) {
    let directory = tempfile::tempdir().unwrap();
    let db = Db::open(directory.path().join("app.db")).await.unwrap();
    (SqliteEnvironmentStore::new(db), directory)
}

fn pins(torch: &str) -> PinSet {
    let mut pins = PinSet::new();
    let name = |value: &str| domain::environment::DistName::new(value).unwrap();
    pins.insert(name("torch"), torch);
    pins.insert(name("torchvision"), "0.29.1+cu130");
    pins.insert(name("triton-windows"), "3.8.0.post29");
    pins
}

fn environment(id: &str, ownership: Ownership) -> PythonEnvironment {
    let mut env = PythonEnvironment::restore(
        NewEnvironment {
            id: EnvId::new(id).unwrap(),
            path: PathBuf::from("D:\\ComfyUI ไทย").join("envs").join(id),
            ownership,
            source: PythonSourceKind::CustomVenv,
            base_interpreter: Some(PathBuf::from("C:\\Python\\python.exe")),
            python_version: PythonVersion::new(3, 13, 16),
            profile_id: "win-py313-torch2141-cu130".into(),
            profile_revision: 1,
            authorized_location: PathBuf::from("D:\\ComfyUI ไทย").join("envs").join(id),
            now: "2026-10-16T00:00:00Z".into(),
        },
        EnvState::Verified,
        pins("2.14.1+cu130"),
        Some("Python 3.13.16, torch 2.14.1+cu130".into()),
        "2026-10-15T00:00:00Z".into(),
    );
    env.updated_at = "2026-10-16T01:00:00Z".into();
    env
}

fn run(id: &str, target: &str, created: &str) -> ProvisioningRun {
    let profile = BundledProfiles::load()
        .unwrap()
        .get("win-py313-torch2141-cu130")
        .unwrap();
    ProvisioningRun {
        id: RunId::new(id).unwrap(),
        request: SetupRequest {
            mode: SetupMode::ChangeTorch,
            install_root: PathBuf::from("D:\\ComfyUI ไทย"),
            python_source: PythonSourceInput::CustomVenv(PathBuf::from("E:\\venvs\\my venv")),
            profile_id: profile.id.clone(),
            comfyui_ref: ComfyRef::Tag("v0.40.0".into()),
            install_manager: true,
            accelerators: Vec::new(),
            curated_nodes: vec!["comfyui-kjnodes".into(), "rgthree-comfy".into()],
            clean_cache: true,
            confirm_adopted_changes: true,
            allow_source_builds: false,
            third_party_consent: false,
        },
        profile,
        target_env: EnvId::new(target).unwrap(),
        previous_env: Some(EnvId::new("env-old").unwrap()),
        python_target: PythonTarget {
            ownership: Ownership::Adopted,
            source: PythonSourceKind::CustomVenv,
            env_path: PathBuf::from("E:\\venvs\\my venv"),
            interpreter: Some(PathBuf::from("C:\\Python\\python.exe")),
            create_venv: false,
            development_files_ok: Some(false),
        },
        components: Default::default(),
        resolved: ResolvedSources {
            comfyui: Some(ResolvedGit {
                url: "https://github.com/Comfy-Org/ComfyUI.git".into(),
                reference: "v0.40.0".into(),
                branch: None,
                commit: "a".repeat(40),
            }),
            manager: Some(ResolvedGit {
                url: "https://github.com/Comfy-Org/ComfyUI-Manager.git".into(),
                reference: "HEAD".into(),
                branch: Some("main".into()),
                commit: "b".repeat(40),
            }),
        },
        steps: {
            let mut steps = ProvisioningRun::fresh_steps();
            steps[3].status = StepStatus::Succeeded;
            steps[3].attempts = 2;
            steps[5].status = StepStatus::Failed;
            steps[5].message = Some("boom".into());
            steps
        },
        status: RunStatus::Failed,
        cancel_requested: true,
        failure: Some(StepFailure {
            step: StepId::Manager,
            kind: FailureKind::ConsentRequired,
            message: "approve".into(),
            command: Some("uv pip install x".into()),
            exit_code: Some(2),
            tail: vec!["line one".into(), "ไทย line".into()],
            consent: Some(ConsentRequest {
                source: "https://example.test/repo.git".into(),
                commit: "c".repeat(40),
            }),
            can_skip: false,
        }),
        activation: ActivationPhase::Intent,
        manager_consent: Some("d".repeat(40)),
        curated: vec![
            CuratedProgress {
                id: "comfyui-kjnodes".into(),
                status: CuratedStatus::Installed,
                plan: None,
                operation: Some("node-0123".into()),
                message: None,
            },
            CuratedProgress {
                id: "rgthree-comfy".into(),
                status: CuratedStatus::Prepared,
                plan: Some("node-4567".into()),
                operation: None,
                message: Some("waiting".into()),
            },
        ],
        created_at: created.into(),
        updated_at: created.into(),
    }
}

#[tokio::test]
async fn environments_round_trip_and_updates_keep_the_active_flag() {
    let (store, _directory) = store().await;
    let env = environment("env-a", Ownership::Adopted);
    EnvironmentRepository::save(&store, &env).await.unwrap();
    assert_eq!(
        EnvironmentRepository::get(&store, &env.id).await.unwrap(),
        Some(env.clone())
    );
    assert_eq!(
        EnvironmentRepository::list(&store).await.unwrap(),
        vec![env.clone()]
    );
    assert_eq!(
        store.active().await.unwrap(),
        None,
        "saving never activates"
    );

    // The derived columns are filled from the pins.
    let (torch, cuda, triton): (String, String, String) = store
        .db
        .run(|connection| {
            connection
                .query_row(
                    "SELECT torch_version, cuda_variant, triton_dist FROM envs WHERE id = 'env-a'",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .map_err(sql)
        })
        .await
        .unwrap();
    assert_eq!(
        (torch.as_str(), cuda.as_str(), triton.as_str()),
        ("2.14.1+cu130", "cu130", "triton-windows")
    );

    let run_id = run("run-1", "env-a", "2026-10-16T00:00:01Z");
    ProvisioningRunRepository::insert(&store, &run_id)
        .await
        .unwrap();
    ActivationStore::commit_activation(&store, &run_id, &env.id)
        .await
        .unwrap();
    let mut changed = env.clone();
    changed.record_pins(pins("2.15.0"), "2026-10-16T02:00:00Z");
    EnvironmentRepository::save(&store, &changed).await.unwrap();
    let active = store.active().await.unwrap().unwrap();
    assert_eq!(active.id, env.id, "updating an environment keeps it active");
    assert_eq!(
        active
            .pins
            .get(&domain::environment::DistName::new("torch").unwrap()),
        Some("2.15.0")
    );
    assert_eq!(
        active.created_at, "2026-10-15T00:00:00Z",
        "creation time is kept"
    );
}

#[tokio::test]
async fn exactly_one_environment_is_active_after_each_switch() {
    let (store, _directory) = store().await;
    for id in ["env-a", "env-b"] {
        EnvironmentRepository::save(&store, &environment(id, Ownership::AppOwned))
            .await
            .unwrap();
    }
    let first = run("run-1", "env-a", "2026-10-16T00:00:01Z");
    let second = run("run-2", "env-b", "2026-10-16T00:00:02Z");
    ProvisioningRunRepository::insert(&store, &first)
        .await
        .unwrap();
    ProvisioningRunRepository::insert(&store, &second)
        .await
        .unwrap();

    ActivationStore::commit_activation(&store, &first, &EnvId::new("env-a").unwrap())
        .await
        .unwrap();
    assert_eq!(store.active().await.unwrap().unwrap().id.as_str(), "env-a");
    ActivationStore::commit_activation(&store, &second, &EnvId::new("env-b").unwrap())
        .await
        .unwrap();
    assert_eq!(store.active().await.unwrap().unwrap().id.as_str(), "env-b");
    let active_rows: u32 = store
        .db
        .run(|connection| {
            connection
                .query_row("SELECT COUNT(*) FROM envs WHERE active = 1", [], |row| {
                    row.get(0)
                })
                .map_err(sql)
        })
        .await
        .unwrap();
    assert_eq!(active_rows, 1);

    // Activating an unknown environment fails and changes nothing.
    assert!(
        ActivationStore::commit_activation(&store, &second, &EnvId::new("env-missing").unwrap())
            .await
            .is_err()
    );
    assert_eq!(store.active().await.unwrap().unwrap().id.as_str(), "env-b");

    // The active environment cannot be deleted; an inactive one can.
    assert_eq!(
        EnvironmentRepository::delete(&store, &EnvId::new("env-b").unwrap()).await,
        Err(EnvRepoError::Conflict)
    );
    EnvironmentRepository::delete(&store, &EnvId::new("env-a").unwrap())
        .await
        .unwrap();
    assert_eq!(EnvironmentRepository::list(&store).await.unwrap().len(), 1);
}

#[tokio::test]
async fn the_activation_commit_is_one_transaction() {
    let (store, _directory) = store().await;
    for id in ["env-a", "env-b"] {
        EnvironmentRepository::save(&store, &environment(id, Ownership::AppOwned))
            .await
            .unwrap();
    }
    let first = run("run-1", "env-a", "2026-10-16T00:00:01Z");
    let second = run("run-2", "env-b", "2026-10-16T00:00:02Z");
    ProvisioningRunRepository::insert(&store, &first)
        .await
        .unwrap();
    ProvisioningRunRepository::insert(&store, &second)
        .await
        .unwrap();
    ActivationStore::commit_activation(&store, &first, &EnvId::new("env-a").unwrap())
        .await
        .unwrap();

    // Make the run update fail after the active selection already switched inside the
    // transaction: nothing may persist.
    store
        .db
        .run(|connection| {
            connection
                .execute_batch(
                    "CREATE TRIGGER fail_run_update BEFORE UPDATE ON provisioning_runs \
                     BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
                )
                .map_err(sql)
        })
        .await
        .unwrap();
    assert!(
        ActivationStore::commit_activation(&store, &second, &EnvId::new("env-b").unwrap())
            .await
            .is_err()
    );
    assert_eq!(
        store.active().await.unwrap().unwrap().id.as_str(),
        "env-a",
        "the previous active selection survives a failed commit"
    );
}

#[tokio::test]
async fn corrupt_environment_rows_are_reported_not_repaired() {
    let (store, _directory) = store().await;
    EnvironmentRepository::save(&store, &environment("env-a", Ownership::AppOwned))
        .await
        .unwrap();
    store
        .db
        .run(|connection| {
            connection
                .execute(
                    "UPDATE envs SET pins_json = 'not json' WHERE id = 'env-a'",
                    [],
                )
                .map_err(sql)
        })
        .await
        .unwrap();
    assert_eq!(
        EnvironmentRepository::get(&store, &EnvId::new("env-a").unwrap()).await,
        Err(EnvRepoError::Corrupt)
    );
    assert_eq!(
        EnvironmentRepository::list(&store).await,
        Err(EnvRepoError::Corrupt)
    );
}

#[tokio::test]
async fn runs_round_trip_with_every_field() {
    let (store, _directory) = store().await;
    let original = run("run-1", "env-a", "2026-10-16T00:00:01Z");
    ProvisioningRunRepository::insert(&store, &original)
        .await
        .unwrap();
    let loaded = ProvisioningRunRepository::get(&store, &original.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded, original);

    let mut updated = original.clone();
    updated.status = RunStatus::Succeeded;
    updated.activation = ActivationPhase::Committed;
    updated.failure = None;
    updated.curated[1].status = CuratedStatus::Failed;
    updated.curated[1].plan = None;
    updated.curated[1].operation = Some("node-4567".into());
    updated.curated[1].message = Some("Published dependencies failed.".into());
    updated.updated_at = "2026-10-16T00:05:00Z".into();
    ProvisioningRunRepository::save(&store, &updated)
        .await
        .unwrap();
    assert_eq!(
        ProvisioningRunRepository::get(&store, &original.id)
            .await
            .unwrap()
            .unwrap(),
        updated
    );
    assert_eq!(
        ProvisioningRunRepository::get(&store, &RunId::new("run-none").unwrap())
            .await
            .unwrap(),
        None
    );
    // Saving a run that was never inserted is a conflict, not a silent insert.
    let ghost = run("run-ghost", "env-a", "2026-10-16T00:00:09Z");
    assert_eq!(
        ProvisioningRunRepository::save(&store, &ghost).await,
        Err(EnvRepoError::Conflict)
    );

    // The other source and reference variants survive too.
    let mut variants = run("run-2", "env-b", "2026-10-16T00:00:02Z");
    variants.request.python_source = PythonSourceInput::Managed;
    variants.request.comfyui_ref = ComfyRef::KeepCurrent;
    ProvisioningRunRepository::insert(&store, &variants)
        .await
        .unwrap();
    assert_eq!(
        ProvisioningRunRepository::get(&store, &variants.id)
            .await
            .unwrap()
            .unwrap(),
        variants
    );
    variants.id = RunId::new("run-3").unwrap();
    variants.request.python_source =
        PythonSourceInput::CustomInterpreter("/usr/bin/python3".into());
    variants.request.comfyui_ref = ComfyRef::Branch("master".into());
    ProvisioningRunRepository::insert(&store, &variants)
        .await
        .unwrap();
    assert_eq!(
        ProvisioningRunRepository::get(&store, &variants.id)
            .await
            .unwrap()
            .unwrap(),
        variants
    );
}

#[tokio::test]
async fn latest_and_unfinished_follow_creation_order_and_state() {
    let (store, _directory) = store().await;
    assert_eq!(
        ProvisioningRunRepository::latest(&store).await.unwrap(),
        None
    );
    let mut specs = vec![
        (
            "run-1",
            RunStatus::Succeeded,
            ActivationPhase::Committed,
            false,
        ),
        ("run-2", RunStatus::Cancelled, ActivationPhase::None, false),
        ("run-3", RunStatus::Failed, ActivationPhase::None, true),
        (
            "run-4",
            RunStatus::Interrupted,
            ActivationPhase::ManifestPublished,
            true,
        ),
        ("run-5", RunStatus::Succeeded, ActivationPhase::Intent, true),
        ("run-6", RunStatus::Running, ActivationPhase::None, true),
        ("run-7", RunStatus::Registered, ActivationPhase::None, true),
    ];
    for (index, (id, status, phase, _)) in specs.iter().enumerate() {
        let mut item = run(id, "env-a", &format!("2026-10-16T00:00:{:02}Z", index + 1));
        item.status = *status;
        item.activation = *phase;
        ProvisioningRunRepository::insert(&store, &item)
            .await
            .unwrap();
    }
    assert_eq!(
        ProvisioningRunRepository::latest(&store)
            .await
            .unwrap()
            .unwrap()
            .id
            .as_str(),
        "run-7"
    );
    let unfinished: Vec<String> = ProvisioningRunRepository::unfinished(&store)
        .await
        .unwrap()
        .into_iter()
        .map(|run| run.id.to_string())
        .collect();
    let expected: Vec<String> = specs
        .drain(..)
        .filter(|spec| spec.3)
        .map(|spec| spec.0.to_owned())
        .collect();
    assert_eq!(unfinished, expected);
}

#[tokio::test]
async fn corrupt_run_documents_are_reported() {
    let (store, _directory) = store().await;
    let item = run("run-1", "env-a", "2026-10-16T00:00:01Z");
    ProvisioningRunRepository::insert(&store, &item)
        .await
        .unwrap();
    store
        .db
        .run(|connection| {
            connection
                .execute(
                    "UPDATE provisioning_runs SET doc_json = '{\"request\":1}'",
                    [],
                )
                .map_err(sql)
        })
        .await
        .unwrap();
    assert_eq!(
        ProvisioningRunRepository::get(&store, &item.id).await,
        Err(EnvRepoError::Corrupt)
    );
}

#[tokio::test]
async fn everything_survives_a_restart_of_the_database() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("app.db");
    {
        let store = SqliteEnvironmentStore::new(Db::open(&path).await.unwrap());
        let env = environment("env-a", Ownership::AppOwned);
        EnvironmentRepository::save(&store, &env).await.unwrap();
        let item = run("run-1", "env-a", "2026-10-16T00:00:01Z");
        ProvisioningRunRepository::insert(&store, &item)
            .await
            .unwrap();
        ActivationStore::commit_activation(&store, &item, &env.id)
            .await
            .unwrap();
        WizardDraftStore::save(&store, "{\"step\":3}")
            .await
            .unwrap();
    }
    let reopened = SqliteEnvironmentStore::new(Db::open(&path).await.unwrap());
    assert_eq!(
        reopened.active().await.unwrap().unwrap().id.as_str(),
        "env-a"
    );
    assert_eq!(
        ProvisioningRunRepository::latest(&reopened)
            .await
            .unwrap()
            .unwrap()
            .id
            .as_str(),
        "run-1"
    );
    assert_eq!(
        WizardDraftStore::load(&reopened).await.unwrap().as_deref(),
        Some("{\"step\":3}")
    );
}

#[tokio::test]
async fn the_wizard_draft_is_a_single_overwritten_row() {
    let (store, _directory) = store().await;
    assert_eq!(WizardDraftStore::load(&store).await.unwrap(), None);
    WizardDraftStore::save(&store, "first").await.unwrap();
    WizardDraftStore::save(&store, &"ไทย".repeat(10_000))
        .await
        .unwrap();
    assert_eq!(
        WizardDraftStore::load(&store).await.unwrap().unwrap(),
        "ไทย".repeat(10_000)
    );
}

#[tokio::test]
async fn components_survive_manifest_and_database_round_trip() {
    use crate::environment::manifest::FsManifestStore;
    use application::environment::{
        InstallManifest, InstallManifestStore, MANIFEST_SCHEMA_VERSION, ManifestState,
    };
    let (store, directory) = store().await;
    let first = environment("env-first", Ownership::AppOwned);
    let second = environment("env-second", Ownership::AppOwned);
    EnvironmentRepository::save(&store, &first).await.unwrap();
    EnvironmentRepository::save(&store, &second).await.unwrap();
    assert_eq!(
        store.load_components(&first.id).await.unwrap(),
        InstalledComponents::default()
    );
    let components = InstalledComponents {
        manager: Some(ManagerResult {
            selected: true,
            status: ComponentStatus::Installed,
            source_commit: Some("a".repeat(40)),
            message: None,
        }),
        accelerators: vec![
            AcceleratorResult {
                id: "sageattention".into(),
                selected: true,
                status: ComponentStatus::Installed,
                importable: Some(false),
                message: Some("DLL import failed".into()),
            },
            AcceleratorResult {
                id: "flashattention-3".into(),
                selected: true,
                status: ComponentStatus::Installed,
                importable: None,
                message: Some("Module unverified".into()),
            },
            AcceleratorResult {
                id: "onnxruntime-gpu".into(),
                selected: false,
                status: ComponentStatus::Skipped,
                importable: None,
                message: None,
            },
        ],
    };
    store.save_components(&first.id, &components).await.unwrap();
    EnvironmentRepository::save(&store, &first).await.unwrap();
    assert_eq!(store.load_components(&first.id).await.unwrap(), components);
    assert_eq!(
        store.load_components(&second.id).await.unwrap(),
        InstalledComponents::default()
    );
    let mut recorded = run("component-run", first.id.as_str(), "now");
    recorded.components = components.clone();
    ProvisioningRunRepository::insert(&store, &recorded)
        .await
        .unwrap();
    assert_eq!(
        ProvisioningRunRepository::get(&store, &recorded.id)
            .await
            .unwrap(),
        Some(recorded.clone())
    );
    let manifest = InstallManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        install_root: directory.path().to_owned(),
        generation: 1,
        env_id: first.id.clone(),
        env_path: first.path.clone(),
        ownership: first.ownership,
        python_version: first.python_version,
        profile_id: first.profile_id.clone(),
        profile_revision: first.profile_revision,
        comfyui_reference: None,
        comfyui_commit: None,
        run_id: recorded.id.clone(),
        activated_at: "now".into(),
        components: components.clone(),
    };
    FsManifestStore::new().publish(&manifest).await.unwrap();
    assert_eq!(
        FsManifestStore::new().read(directory.path()).await.unwrap(),
        ManifestState::Valid(manifest)
    );
    drop(store);
    let reopened =
        SqliteEnvironmentStore::new(Db::open(directory.path().join("app.db")).await.unwrap());
    assert_eq!(
        reopened.load_components(&first.id).await.unwrap(),
        components
    );
    assert_eq!(
        reopened.load_components(&second.id).await.unwrap(),
        InstalledComponents::default()
    );
}

#[tokio::test]
async fn run_doc_without_third_party_consent_reads_false() {
    let (store, _directory) = store().await;
    let mut consenting = run("consent-run", "env-a", "now");
    consenting.request.third_party_consent = true;
    ProvisioningRunRepository::insert(&store, &consenting)
        .await
        .unwrap();
    let restored = ProvisioningRunRepository::get(&store, &consenting.id)
        .await
        .unwrap()
        .unwrap();
    assert!(restored.request.third_party_consent, "the flag round-trips");

    // A document stored before the field existed decodes to false.
    let mut doc: serde_json::Value =
        serde_json::from_str(&encode_run(&consenting).unwrap()).unwrap();
    doc["request"]
        .as_object_mut()
        .unwrap()
        .remove("third_party_consent")
        .unwrap();
    let json = doc.to_string();
    store
        .db
        .run(move |connection| {
            connection
                .execute(
                    "UPDATE provisioning_runs SET doc_json = ?1 WHERE id = 'consent-run'",
                    [json],
                )
                .map_err(sql)?;
            Ok(())
        })
        .await
        .unwrap();
    let legacy = ProvisioningRunRepository::get(&store, &consenting.id)
        .await
        .unwrap()
        .unwrap();
    assert!(!legacy.request.third_party_consent);
}

#[tokio::test]
async fn legacy_runs_recover_known_manager_selection_and_commit() {
    let (store, _directory) = store().await;
    let mut legacy = run("legacy-run", "env-a", "now");
    legacy.step_mut(StepId::Manager).status = StepStatus::Succeeded;
    let mut doc: serde_json::Value = serde_json::from_str(&encode_run(&legacy).unwrap()).unwrap();
    doc.as_object_mut().unwrap().remove("components");
    doc.as_object_mut().unwrap().remove("curated");
    doc["request"]
        .as_object_mut()
        .unwrap()
        .remove("accelerators");
    doc["request"]
        .as_object_mut()
        .unwrap()
        .remove("curated_nodes");
    doc["failure"].as_object_mut().unwrap().remove("can_skip");
    let json = doc.to_string();
    ProvisioningRunRepository::insert(&store, &legacy)
        .await
        .unwrap();
    store
        .db
        .run(move |connection| {
            connection
                .execute(
                    "UPDATE provisioning_runs SET doc_json = ?1 WHERE id = 'legacy-run'",
                    [json],
                )
                .map_err(sql)?;
            Ok(())
        })
        .await
        .unwrap();
    let restored = ProvisioningRunRepository::get(&store, &legacy.id)
        .await
        .unwrap()
        .unwrap();
    assert!(restored.request.accelerators.is_empty());
    assert!(restored.request.curated_nodes.is_empty() && restored.curated.is_empty());
    // A failure saved before the backend decided it can be skipped when its step is optional.
    assert!(restored.failure.unwrap().can_skip);
    let manager = restored.components.manager.unwrap();
    assert!(manager.selected);
    assert_eq!(manager.status, ComponentStatus::Installed);
    assert_eq!(
        manager.source_commit,
        legacy.resolved.manager.map(|git| git.commit)
    );
}
