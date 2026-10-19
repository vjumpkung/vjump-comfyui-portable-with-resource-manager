use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};

use application::{comfy_runtime::RuntimeStore, shared::Retained};
use domain::{
    comfy_runtime::{Attention, LaunchOptions, Port, PreviewMethod, VramMode},
    custom_nodes::DependencyState,
    environment::{NewEnvironment, Ownership, PythonSourceKind},
};
use infrastructure::{
    comfy_runtime::sqlite::SqliteRuntimeStore, custom_nodes::sqlite::SqliteNodes,
};

use super::*;

fn write(root: &Path, relative: &str, bytes: &[u8]) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut directories = vec![root.to_owned()];
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let kind = entry.file_type().unwrap();
            assert!(
                !kind.is_symlink(),
                "the fixture contains only owned ordinary files"
            );
            if kind.is_dir() {
                directories.push(entry.path());
            } else {
                files.insert(
                    entry.path().strip_prefix(root).unwrap().to_owned(),
                    fs::read(entry.path()).unwrap(),
                );
            }
        }
    }
    files
}

fn directories(root: &Path) -> BTreeSet<PathBuf> {
    let mut found = BTreeSet::new();
    let mut pending = vec![root.to_owned()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                found.insert(entry.path().strip_prefix(root).unwrap().to_owned());
                pending.push(entry.path());
            }
        }
    }
    found
}

fn copy_tree(source: &Path, destination: &Path) {
    for (relative, bytes) in tree(source) {
        let path = destination.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
}

async fn moved_rebuild(original_present: bool) {
    let (directory, original) = fresh_root();
    let moved = directory.path().join("moved root ย้าย");
    let external_models = directory.path().join("external models ไทย");
    let adopted_directory = directory.path().join("external adopted venv");
    write(
        &external_models,
        "external.safetensors",
        b"external model bytes\0\xff",
    );
    write(
        &adopted_directory,
        "pyvenv.cfg",
        b"home = /external\nversion = 3.13.16\n",
    );
    write(
        &adopted_directory,
        "user-file.dat",
        b"adopted environment data\0\xff",
    );

    let sim = Arc::new(Sim::default());
    let machine = start(&original, sim.clone(), Faults::default()).await;
    let first = machine.register(machine.request()).await;
    assert_eq!(machine.run(&first).await, RunOutcome::Succeeded);
    let original_run = machine.deps.runs.get(&first).await.unwrap().unwrap();
    let previous = machine.deps.envs.active().await.unwrap().unwrap();

    let mut adopted = PythonEnvironment::provisioning(NewEnvironment {
        id: EnvId::new("external-adopted").unwrap(),
        path: adopted_directory.clone(),
        ownership: Ownership::Adopted,
        source: PythonSourceKind::CustomVenv,
        base_interpreter: None,
        python_version: previous.python_version,
        profile_id: previous.profile_id.clone(),
        profile_revision: previous.profile_revision,
        authorized_location: adopted_directory.clone(),
        now: "2026-10-19T00:00:00Z".into(),
    });
    adopted
        .mark_verified(
            PinSet::new(),
            "retained adopted environment".into(),
            "2026-10-19T00:00:01Z",
        )
        .unwrap();
    machine.deps.envs.save(&adopted).await.unwrap();

    let settings = Settings::new(
        7,
        HfEngine::Native,
        CloseBehavior::StopComfy,
        Theme::Dark,
        21,
        Some(1024),
    )
    .unwrap();
    machine.settings.save(&settings).await.unwrap();
    let options = LaunchOptions {
        port: Port::new(8291).unwrap(),
        vram: VramMode::Low,
        attention: Attention::Pytorch,
        preview: PreviewMethod::None,
        enable_manager: false,
        listen_lan: true,
        extra_args: vec!["--disable-smart-memory".into()],
    };
    {
        let db = Db::open(AppPaths::from_root(&original).db_path)
            .await
            .unwrap();
        SqliteRuntimeStore::new(db)
            .save_launch_options(&options)
            .await
            .unwrap();
    }

    for (relative, bytes) in [
        (
            "ComfyUI/main.py",
            b"# locally edited checkout\nprint('retained')\n".as_slice(),
        ),
        (
            "ComfyUI/.git/HEAD",
            b"ref: refs/heads/local-work\n".as_slice(),
        ),
        (
            "ComfyUI/requirements.txt",
            b"comfy-fixture==1.0\n".as_slice(),
        ),
        (
            "ComfyUI/models/checkpoints/model.safetensors",
            b"model bytes\0\xff".as_slice(),
        ),
        (
            "ComfyUI/user/default/workflows/user.json",
            b"{\"workflow\":\"user\"}\n".as_slice(),
        ),
        ("ComfyUI/input/input.png", b"input pixels\0\xff".as_slice()),
        (
            "ComfyUI/output/output.png",
            b"output pixels\0\xfe".as_slice(),
        ),
        (
            "ComfyUI/custom_nodes/enabled/__init__.py",
            b"# enabled node local edits\n".as_slice(),
        ),
        (
            "ComfyUI/custom_nodes/enabled/requirements.txt",
            b"enabled-fixture==1.0\n".as_slice(),
        ),
        (
            "ComfyUI/custom_nodes/.disabled/disabled/__init__.py",
            b"# disabled node local edits\n".as_slice(),
        ),
        (
            "ComfyUI/custom_nodes/.disabled/disabled/requirements.txt",
            b"disabled-fixture==2.0\n".as_slice(),
        ),
    ] {
        write(&original, relative, bytes);
    }
    write(
        &original,
        "ComfyUI/extra_model_paths.yaml",
        format!(
            "external:\n  base_path: '{}'\n  checkpoints: .\n",
            external_models.to_string_lossy().replace('\\', "/")
        )
        .as_bytes(),
    );
    write(
        &previous.path,
        "retained-owned-file.dat",
        b"original environment bytes\0\xff",
    );
    let checkout_before = tree(&original.join("ComfyUI"));
    let owned_before = tree(&previous.path);
    let adopted_before = tree(&adopted_directory);
    let external_before = tree(&external_models);
    let manifest_before = fs::read(original.join("install.json")).unwrap();
    drop(machine);

    // Every shared connection has been dropped before the database is copied.
    {
        let connection =
            rusqlite::Connection::open(AppPaths::from_root(&original).db_path).unwrap();
        connection
            .execute(
                "INSERT INTO model_roots(id, path, category, status) VALUES ('external-fixture', ?1, 'checkpoints', 'available')",
                [external_models.to_str().unwrap()],
            )
            .unwrap();
        connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
            .unwrap();
    }
    // The copied installation is the only thing a rebuild may change: the original root must
    // end up byte-for-byte identical, database included, with no new folders.
    let original_files = tree(&original);
    let original_directories = directories(&original);
    copy_tree(&original, &moved);
    if !original_present {
        assert!(original.starts_with(directory.path()));
        fs::remove_dir_all(&original).unwrap();
    }
    sim.calls.lock().unwrap().clear();
    sim.installed.lock().unwrap().clear();
    let machine = start(&moved, sim.clone(), Faults::default()).await;
    let rebuilt = machine
        .register(SetupRequest {
            mode: SetupMode::Rebuild,
            comfyui_ref: ComfyRef::KeepCurrent,
            ..machine.request()
        })
        .await;
    let target = machine.deps.runs.get(&rebuilt).await.unwrap().unwrap();
    assert_eq!(target.profile, original_run.profile);
    assert_ne!(target.target_env, previous.id);
    assert_eq!(target.python_target.ownership, Ownership::AppOwned);
    assert_eq!(
        target.python_target.env_path,
        moved.join("envs").join(target.target_env.as_str())
    );
    assert_eq!(machine.run(&rebuilt).await, RunOutcome::Succeeded);

    assert_eq!(tree(&moved.join("ComfyUI")), checkout_before);
    assert_eq!(
        tree(&moved.join("envs").join(previous.id.as_str())),
        owned_before
    );
    assert_eq!(tree(&adopted_directory), adopted_before);
    assert_eq!(tree(&external_models), external_before);
    assert_eq!(machine.settings.load().await.unwrap(), Some(settings));
    assert_eq!(
        machine.deps.envs.get(&adopted.id).await.unwrap(),
        Some(adopted)
    );
    assert_eq!(
        machine.deps.runs.get(&first).await.unwrap(),
        Some(original_run)
    );
    let copied_previous = machine.deps.envs.get(&previous.id).await.unwrap().unwrap();
    assert!(same_location(
        &copied_previous.path,
        &moved.join("envs").join(previous.id.as_str())
    ));
    assert!(same_location(
        &copied_previous.authorized_location,
        &copied_previous.path
    ));
    assert_eq!(copied_previous.state(), EnvState::RepairRequired);
    let ManifestState::Valid(manifest) = machine.deps.manifests.read(&moved).await.unwrap() else {
        panic!("the verified rebuild must publish its manifest")
    };
    assert!(same_location(&manifest.install_root, &moved));
    assert_eq!(manifest.env_id, target.target_env);
    assert_eq!(manifest.env_path, target.python_target.env_path);
    assert_eq!(manifest.profile_revision, previous.profile_revision);
    assert_eq!(sim.count("resolve"), 0);
    assert_eq!(sim.count("clone_comfyui"), 0);
    assert_eq!(
        sim.count("install_requirements"),
        3,
        "ComfyUI plus enabled and disabled nodes"
    );

    let sets = machine
        .deps
        .node_requirements
        .requirement_sets(&moved, Retained::default())
        .await
        .unwrap();
    assert_eq!(sets.len(), 2);
    let db = Db::open(AppPaths::from_root(&moved).db_path).await.unwrap();
    let node_store = SqliteNodes { db: db.clone() };
    for set in sets {
        assert_eq!(
            application::custom_nodes::NodeRepository::dependency(
                &node_store,
                &set.node_id,
                &target.target_env,
                &set.source_identity
            )
            .await
            .unwrap(),
            Some(DependencyState::Ok)
        );
    }
    assert_eq!(
        SqliteRuntimeStore::new(db).launch_options().await.unwrap(),
        options
    );
    let connection = rusqlite::Connection::open(AppPaths::from_root(&moved).db_path).unwrap();
    let external_path: String = connection
        .query_row(
            "SELECT path FROM model_roots WHERE id = 'external-fixture'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(PathBuf::from(external_path), external_models);
    if original_present {
        assert_eq!(
            fs::read(original.join("install.json")).unwrap(),
            manifest_before
        );
        assert_eq!(tree(&original.join("ComfyUI")), checkout_before);
        assert_eq!(tree(&previous.path), owned_before);
        assert_eq!(tree(&original), original_files);
        assert_eq!(directories(&original), original_directories);
    } else {
        assert!(
            !original.exists(),
            "a vanished old root must never be recreated"
        );
    }
}

#[tokio::test]
async fn moved_rebuild_preserves_files_and_launch_options() {
    for original_present in [true, false] {
        moved_rebuild(original_present).await;
    }
}
