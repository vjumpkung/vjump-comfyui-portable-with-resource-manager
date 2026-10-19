use super::*;

fn manifest(root: &Path, generation: u64) -> InstallManifest {
    InstallManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        install_root: root.to_owned(),
        generation,
        env_id: EnvId::new("env-abc123").unwrap(),
        env_path: root.join("envs").join("env-abc123"),
        ownership: Ownership::AppOwned,
        python_version: PythonVersion::new(3, 13, 16),
        profile_id: "win-py313-torch2141-cu130".into(),
        profile_revision: 1,
        comfyui_reference: Some("v0.40.0".into()),
        comfyui_commit: Some("a".repeat(40)),
        run_id: RunId::new("run-1").unwrap(),
        activated_at: "2026-10-16T00:00:00Z".into(),
        components: Default::default(),
    }
}

#[tokio::test]
async fn a_manifest_round_trips_with_unicode_paths_and_replaces_atomically() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("install ไทย root");
    let store = FsManifestStore::new();
    assert_eq!(store.read(&root).await.unwrap(), ManifestState::Absent);

    store.publish(&manifest(&root, 1)).await.unwrap();
    assert_eq!(
        store.read(&root).await.unwrap(),
        ManifestState::Valid(manifest(&root, 1))
    );
    store.publish(&manifest(&root, 2)).await.unwrap();
    match store.read(&root).await.unwrap() {
        ManifestState::Valid(read) => assert_eq!(read.generation, 2),
        other => panic!("{other:?}"),
    }
    let names: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["install.json"], "no temporary file is left behind");
}

#[tokio::test]
async fn a_leftover_temporary_file_from_a_crash_does_not_block_publishing() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("install.json.tmp"), b"half written").unwrap();
    FsManifestStore::new()
        .publish(&manifest(&root, 1))
        .await
        .unwrap();
    assert!(matches!(
        FsManifestStore::new().read(&root).await.unwrap(),
        ManifestState::Valid(_)
    ));
}

#[tokio::test]
async fn unreadable_invalid_and_newer_manifests_are_reported_not_replaced() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    fs::create_dir_all(&root).unwrap();
    let store = FsManifestStore::new();
    let valid = serde_json::to_value(ManifestDoc::from(&manifest(&root, 1))).unwrap();
    let with = |change: &dyn Fn(&mut serde_json::Value)| {
        let mut value = valid.clone();
        change(&mut value);
        value.to_string()
    };
    for (text, expected) in [
        ("not json".to_owned(), ManifestState::Corrupt),
        ("{}".to_owned(), ManifestState::Corrupt),
        ("[]".to_owned(), ManifestState::Corrupt),
        (
            with(&|v| v["schema_version"] = 0.into()),
            ManifestState::Corrupt,
        ),
        (
            with(&|v| v["schema_version"] = "1".into()),
            ManifestState::Corrupt,
        ),
        (
            with(&|v| v["env_id"] = "BAD ID".into()),
            ManifestState::Corrupt,
        ),
        (
            with(&|v| v["ownership"] = "stolen".into()),
            ManifestState::Corrupt,
        ),
        (
            with(&|v| v["python_version"] = "3.13".into()),
            ManifestState::Corrupt,
        ),
        (
            with(&|v| v["unexpected"] = 1.into()),
            ManifestState::Corrupt,
        ),
        (
            with(&|v| v["schema_version"] = 3.into()),
            ManifestState::Newer,
        ),
        (
            r#"{"schema_version":99,"anything":true}"#.to_owned(),
            ManifestState::Newer,
        ),
    ] {
        fs::write(root.join("install.json"), &text).unwrap();
        assert_eq!(store.read(&root).await.unwrap(), expected, "{text}");
        assert_eq!(
            fs::read_to_string(root.join("install.json")).unwrap(),
            text,
            "reading never modifies the file"
        );
    }
    // A folder where the file should be, or invalid UTF-8, is unreadable.
    fs::remove_file(root.join("install.json")).unwrap();
    fs::create_dir(root.join("install.json")).unwrap();
    assert_eq!(store.read(&root).await.unwrap(), ManifestState::Corrupt);
    fs::remove_dir(root.join("install.json")).unwrap();
    fs::write(root.join("install.json"), [0xff, 0xfe, 0x00]).unwrap();
    assert_eq!(store.read(&root).await.unwrap(), ManifestState::Corrupt);
}

#[test]
fn schema_one_manifests_have_unknown_components() {
    let mut value =
        serde_json::to_value(ManifestDoc::from(&manifest(Path::new("/install"), 1))).unwrap();
    value["schema_version"] = 1.into();
    value.as_object_mut().unwrap().remove("components");
    let ManifestState::Valid(parsed) = parse_manifest(&value.to_string()) else {
        panic!("legacy manifest must remain readable")
    };
    assert_eq!(parsed.components, Default::default());
    value["schema_version"] = 2.into();
    assert_eq!(parse_manifest(&value.to_string()), ManifestState::Corrupt);
}
