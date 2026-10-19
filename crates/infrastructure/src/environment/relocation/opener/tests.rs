use super::*;
use crate::{
    environment::{manifest::FsManifestStore, source::Endpoints},
    shared::{
        download::Downloader,
        gate::TokioMaintenanceGate,
        paths::{read_bootstrap, resolve_root_with},
    },
};
use application::{
    environment::{InstallManifest, InstallManifestStore, MANIFEST_SCHEMA_VERSION, RunId},
    shared::GateError,
};
use domain::environment::{
    EnvId, EnvState, NewEnvironment, Ownership, PinSet, PythonEnvironment, PythonSourceKind,
    PythonVersion,
};

struct Fixture {
    _directory: tempfile::TempDir,
    root: PathBuf,
    pointer: PathBuf,
    gate: Arc<TokioMaintenanceGate>,
}

fn git(root: &Path, args: &[&str]) {
    let output = std::process::Command::new(locate_git())
        .current_dir(root)
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .expect("Git must be available for the open-existing tests");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn locate_git() -> PathBuf {
    let binary = if cfg!(windows) { "git.exe" } else { "git" };
    std::env::split_paths(&std::env::var_os("PATH").expect("Git must be on PATH"))
        .map(|directory| directory.join(binary))
        .find(|candidate| candidate.is_file())
        .expect("Git must be installed for the open-existing tests")
}

impl Fixture {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("candidate ไทย");
        fs::create_dir_all(root.join("data")).unwrap();
        fs::create_dir_all(root.join("ComfyUI/custom_nodes")).unwrap();
        fs::write(root.join("ComfyUI/main.py"), "# local checkout\n").unwrap();
        git(&root.join("ComfyUI"), &["init"]);
        git(&root.join("ComfyUI"), &["add", "main.py"]);
        git(&root.join("ComfyUI"), &["commit", "-m", "fixture"]);
        let db = Db::open(root.join("data/app.db")).await.unwrap();
        let env = PythonEnvironment::restore(
            NewEnvironment {
                id: EnvId::new("env-existing").unwrap(),
                path: root.join("envs/env-existing"),
                ownership: Ownership::AppOwned,
                source: PythonSourceKind::Managed,
                base_interpreter: None,
                python_version: PythonVersion::new(3, 13, 16),
                profile_id: "recorded-profile".into(),
                profile_revision: 7,
                authorized_location: root.join("envs/env-existing"),
                now: "t".into(),
            },
            EnvState::Verified,
            PinSet::new(),
            Some("verified".into()),
            "t".into(),
        );
        SqliteEnvironmentStore::new(db.clone())
            .save(&env)
            .await
            .unwrap();
        db.run(|connection| {
            connection
                .execute("UPDATE envs SET active = 1", [])
                .unwrap();
            connection
                .execute(
                    "INSERT INTO settings(key,value,updated_at) VALUES('theme','dark','t')",
                    [],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO wizard_draft(id,json,updated_at) VALUES(1,'candidate draft','t')",
                    [],
                )
                .unwrap();
            Ok(())
        })
        .await
        .unwrap();
        FsManifestStore::new()
            .publish(&InstallManifest {
                schema_version: MANIFEST_SCHEMA_VERSION,
                install_root: root.clone(),
                generation: 1,
                env_id: env.id.clone(),
                env_path: env.path.clone(),
                ownership: env.ownership,
                python_version: env.python_version,
                profile_id: env.profile_id.clone(),
                profile_revision: env.profile_revision,
                comfyui_reference: None,
                comfyui_commit: None,
                run_id: RunId::new("run-existing").unwrap(),
                activated_at: "t".into(),
                components: Default::default(),
            })
            .await
            .unwrap();
        drop(db);
        Self {
            pointer: directory.path().join("config/bootstrap.json"),
            root,
            gate: Arc::new(TokioMaintenanceGate::new()),
            _directory: directory,
        }
    }

    /// The candidate was copied from elsewhere: install.json still records the original root.
    fn relocate(&self) {
        let path = self.root.join("install.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        manifest["install_root"] =
            serde_json::to_value(self.root.with_file_name("recorded original")).unwrap();
        fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    }

    fn opener(&self) -> FsExistingInstallOpener {
        self.opener_with(None, None)
    }

    fn opener_with(
        &self,
        current: Option<RootInfo>,
        recorded_old: Option<PathBuf>,
    ) -> FsExistingInstallOpener {
        let mut opener = FsExistingInstallOpener::new(
            current,
            recorded_old,
            self.pointer.clone(),
            self.gate.clone(),
        );
        opener.source = GitSource::with(Downloader::new(), Endpoints::pinned(), Some(locate_git()));
        // Selection is asserted against the fixture's roots, not against whatever else the machine
        // is running: an unreadable python (one caught starting or exiting by another parallel
        // test) makes the strict inventory refuse, as designed.
        opener.snapshot = procscan::test_snapshot;
        opener
    }
}

#[tokio::test]
async fn open_existing_never_creates_a_database_or_copies_settings() {
    let fixture = Fixture::new().await;
    let before = fs::read(fixture.root.join("data/app.db")).unwrap();
    let opener = fixture.opener();
    opener.open_existing(&fixture.root).await.unwrap();
    assert_eq!(
        read_bootstrap(&fixture.pointer).unwrap(),
        Some(normalize_path(&fixture.root))
    );
    assert_eq!(before, fs::read(fixture.root.join("data/app.db")).unwrap());
    let db = Db::open_existing(fixture.root.join("data/app.db"))
        .await
        .unwrap();
    db.run(|connection| {
        let theme: String = connection
            .query_row("SELECT value FROM settings WHERE key='theme'", [], |row| {
                row.get(0)
            })
            .unwrap();
        let draft: String = connection
            .query_row("SELECT json FROM wizard_draft", [], |row| row.get(0))
            .unwrap();
        assert_eq!(theme, "dark");
        assert_eq!(draft, "candidate draft");
        assert!(
            connection
                .execute("UPDATE settings SET value='light'", [])
                .is_err()
        );
        Ok(())
    })
    .await
    .unwrap();
    assert_eq!(
        fixture.gate.try_acquire().await.err(),
        Some(GateError::Busy)
    );
    drop(opener);
    assert!(fixture.gate.try_acquire().await.is_ok());
    drop(db);

    fs::remove_file(fixture.root.join("data/app.db")).unwrap();
    assert!(matches!(
        fixture.opener().open_existing(&fixture.root).await,
        Err(RootSwitchError::InvalidDestination(_))
    ));
    assert!(!fixture.root.join("data/app.db").exists());
    assert!(
        Db::open_existing(fixture.root.join("data/app.db"))
            .await
            .is_err()
    );
    assert!(!fixture.root.join("data/app.db").exists());
}

#[tokio::test]
async fn selecting_a_new_root_creates_only_an_empty_first_run_destination() {
    let fixture = Fixture::new().await;
    let new_root = fixture.root.parent().unwrap().join("new parent/new root");
    fixture.opener().select_new(&new_root).await.unwrap();
    assert_eq!(
        read_bootstrap(&fixture.pointer).unwrap(),
        Some(normalize_path(&new_root))
    );
    assert!(new_root.is_dir());
    assert!(fs::read_dir(&new_root).unwrap().next().is_none());
    assert!(!new_root.join("data/app.db").exists());
    let resolved = resolve_root_with(None, &fixture.pointer, fixture.root.clone()).unwrap();
    assert_eq!(resolved.root, normalize_path(&new_root));
    assert_eq!(resolved.source, RootSource::Bootstrap);
}

#[tokio::test]
async fn new_selection_never_overwrites_an_existing_installation() {
    let fixture = Fixture::new().await;
    assert!(matches!(
        fixture.opener().select_new(&fixture.root).await,
        Err(RootSwitchError::InvalidDestination(_))
    ));
    assert!(!fixture.pointer.exists());
    assert!(fixture.root.join("install.json").is_file());
    assert!(fixture.root.join("data/app.db").is_file());
}

#[tokio::test]
async fn root_recovery_uses_the_shared_gate_and_rejects_override_mode() {
    let fixture = Fixture::new().await;
    let lease = fixture.gate.acquire().await.unwrap();
    assert_eq!(
        fixture.opener().open_existing(&fixture.root).await,
        Err(RootSwitchError::MaintenanceBusy)
    );
    assert_eq!(
        fixture.opener().select_new(&fixture.root).await,
        Err(RootSwitchError::MaintenanceBusy)
    );
    drop(lease);
    let opener = FsExistingInstallOpener::new(
        Some(RootInfo {
            root: fixture.root.clone(),
            source: RootSource::DebugOverride,
        }),
        None,
        fixture.pointer.clone(),
        fixture.gate.clone(),
    );
    assert_eq!(
        opener.open_existing(&fixture.root).await,
        Err(RootSwitchError::FixedByOverride)
    );
    assert_eq!(
        opener.select_new(&fixture.root).await,
        Err(RootSwitchError::FixedByOverride)
    );
    assert!(!fixture.pointer.exists());
}

#[tokio::test]
async fn open_existing_refuses_incomplete_publication_and_activation_journals() {
    let fixture = Fixture::new().await;
    fixture.relocate();
    for relative in [
        ".ComfyUI.publish.json",
        ".ComfyUI.cloning",
        "ComfyUI/custom_nodes/.ComfyUI-Manager.publish.json",
    ] {
        let journal = fixture.root.join(relative);
        fs::write(&journal, "interrupted").unwrap();
        assert_eq!(
            fixture.opener().open_existing(&fixture.root).await,
            Err(RootSwitchError::RelocationRecoveryRequired),
            "{relative}"
        );
        fs::remove_file(journal).unwrap();
    }
    let db = Db::open(fixture.root.join("data/app.db")).await.unwrap();
    db.run(|connection| {
        connection.execute("INSERT INTO provisioning_runs(id,status,mode,target_env_id,doc_json,activation_phase,created_at,updated_at) VALUES('run-incomplete','running','first_run','env-next','{}','intent','t','t')", []).unwrap();
        Ok(())
    }).await.unwrap();
    drop(db);
    assert_eq!(
        fixture.opener().open_existing(&fixture.root).await,
        Err(RootSwitchError::RelocationRecoveryRequired)
    );
    let db = Db::open(fixture.root.join("data/app.db")).await.unwrap();
    db.run(|connection| {
        connection.execute("UPDATE provisioning_runs SET activation_phase = 'committed'", []).unwrap();
        connection.execute("INSERT INTO comfy_update_runs(id,json,phase) VALUES('update-incomplete','{}','intent')", []).unwrap();
        Ok(())
    }).await.unwrap();
    drop(db);
    assert_eq!(
        fixture.opener().open_existing(&fixture.root).await,
        Err(RootSwitchError::RelocationRecoveryRequired)
    );
    assert!(!fixture.pointer.exists());
}

#[tokio::test]
async fn open_existing_allows_an_unmoved_install_with_startup_recoverable_journals() {
    let fixture = Fixture::new().await;
    for relative in [
        ".ComfyUI.publish.json",
        ".ComfyUI.cloning",
        "ComfyUI/custom_nodes/.ComfyUI-Manager.publish.json",
    ] {
        fs::write(fixture.root.join(relative), "interrupted").unwrap();
    }
    let db = Db::open(fixture.root.join("data/app.db")).await.unwrap();
    db.run(|connection| {
        connection.execute("INSERT INTO provisioning_runs(id,status,mode,target_env_id,doc_json,activation_phase,created_at,updated_at) VALUES('run-incomplete','running','first_run','env-next','{}','intent','t','t')", []).unwrap();
        connection.execute("INSERT INTO comfy_update_runs(id,json,phase) VALUES('update-incomplete','{}','intent')", []).unwrap();
        Ok(())
    }).await.unwrap();
    drop(db);

    fixture.opener().open_existing(&fixture.root).await.unwrap();
    assert_eq!(
        read_bootstrap(&fixture.pointer).unwrap(),
        Some(normalize_path(&fixture.root))
    );
    for relative in [".ComfyUI.publish.json", ".ComfyUI.cloning"] {
        assert!(fixture.root.join(relative).is_file(), "{relative}");
    }
}

#[tokio::test]
async fn e3_destinations_inside_or_around_the_current_or_recorded_root_are_refused() {
    let fixture = Fixture::new().await;
    let parent = fixture.root.parent().unwrap().to_owned();
    let current = parent.join("current install");
    let recorded = parent.join("recorded install");
    fs::create_dir_all(&current).unwrap();
    let opener = fixture.opener_with(
        Some(RootInfo {
            root: current.clone(),
            source: RootSource::Bootstrap,
        }),
        Some(recorded.clone()),
    );
    for destination in [
        current.clone(),
        current.join("nested/new"),
        recorded.clone(),
        recorded.join("inside"),
        parent.clone(),
    ] {
        assert!(
            matches!(
                opener.select_new(&destination).await,
                Err(RootSwitchError::InvalidDestination(_))
            ),
            "select_new {destination:?}"
        );
        assert!(
            matches!(
                opener.open_existing(&destination).await,
                Err(RootSwitchError::InvalidDestination(_))
            ),
            "open_existing {destination:?}"
        );
    }
    assert!(!current.join("nested").exists());
    assert!(!recorded.exists());
    assert!(!fixture.pointer.exists());

    let separate = parent.join("separate new install");
    opener.select_new(&separate).await.unwrap();
    assert_eq!(
        read_bootstrap(&fixture.pointer).unwrap(),
        Some(normalize_path(&separate))
    );
}
