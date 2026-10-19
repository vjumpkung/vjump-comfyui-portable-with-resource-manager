use super::*;

struct Fixture {
    _directory: tempfile::TempDir,
    old: PathBuf,
    new: PathBuf,
    db: Db,
    store: SqliteRelocationStore,
}

impl Fixture {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let old = directory.path().join("old-root");
        let new = directory.path().join("new-root");
        fs::create_dir_all(new.join("data")).unwrap();
        let db = Db::open(new.join("data/app.db")).await.unwrap();
        let store = SqliteRelocationStore::new(db.clone());
        Self {
            _directory: directory,
            old,
            new,
            db,
            store,
        }
    }

    async fn environment(
        &self,
        id: &str,
        path: &Path,
        base: Option<&Path>,
        authorized: &Path,
        owned: bool,
        state: &str,
    ) {
        let id = id.to_owned();
        let path = path_text(path).unwrap();
        let base = base.map(|path| path_text(path).unwrap());
        let authorized = path_text(authorized).unwrap();
        let state = state.to_owned();
        self.db
            .run(move |connection| {
                connection
                    .execute(
                        "INSERT INTO envs(id, path, owned, source, base_interpreter, python_version, \
                         profile_id, profile_revision, pins_json, authorized_location, state, \
                         created_at, updated_at) \
                         VALUES (?1, ?2, ?3, ?4, ?5, '3.13.16', 'recorded', 1, '{}', ?6, ?7, 't', 't')",
                        params![
                            id,
                            path,
                            owned,
                            if owned { "managed" } else { "custom_venv" },
                            base,
                            authorized,
                            state
                        ],
                    )
                    .unwrap();
                Ok(())
            })
            .await
            .unwrap();
    }

    async fn environment_paths(&self, id: &str) -> (String, Option<String>, String, String) {
        let id = id.to_owned();
        self.db
            .run(move |connection| {
                Ok(connection
                    .query_row(
                        "SELECT path, base_interpreter, authorized_location, state FROM envs WHERE id = ?1",
                        [id],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                    )
                    .unwrap())
            })
            .await
            .unwrap()
    }

    async fn scalar(&self, sql: &str) -> i64 {
        let sql = sql.to_owned();
        self.db
            .run(move |connection| Ok(connection.query_row(&sql, [], |row| row.get(0)).unwrap()))
            .await
            .unwrap()
    }
}

/// The text a rebased location is stored as: the non-verbatim `normalize_path` spelling.
fn stored(path: &Path) -> String {
    path_text(&normalize_path(path)).unwrap()
}

#[tokio::test]
async fn relocation_rebases_authorized_locations_without_touching_external_paths() {
    let fixture = Fixture::new().await;
    let owned = fixture.old.join("envs/OwnedEnv");
    let base = fixture.old.join("data/cache/uv-python/python.exe");
    fixture
        .environment("owned", &owned, Some(&base), &owned, true, "verified")
        .await;
    let adopted = fixture.old.join("adopted/KeepMe");
    fixture
        .environment("adopted", &adopted, None, &adopted, false, "verified")
        .await;
    let pending = fixture.old.join("envs/pending");
    fixture
        .environment("pending", &pending, None, &pending, true, "provisioning")
        .await;
    let sibling = fixture
        .old
        .with_file_name("old-root-sibling")
        .join("external");
    fixture
        .environment(
            "external",
            &sibling,
            Some(&sibling),
            &sibling,
            true,
            "verified",
        )
        .await;
    let external = path_text(&sibling).unwrap();
    let local_model_root = path_text(&fixture.old.join("ComfyUI/models")).unwrap();
    fixture
        .db
        .run(move |connection| {
            for (id, path) in [("local", local_model_root), ("external", external), ("relative", "models".into())] {
                connection
                    .execute(
                        "INSERT INTO model_roots(id, path, status) VALUES (?1, ?2, 'ready')",
                        params![id, path],
                    )
                    .unwrap();
            }
            connection
                .execute(
                    "INSERT INTO models(root_id, path, category, size, mtime, file_identity) \
                     VALUES ('local', 'checkpoints/KeepMe.safetensors', 'checkpoint', 1, 't', 'id')",
                    [],
                )
                .unwrap();
            Ok(())
        })
        .await
        .unwrap();
    fs::create_dir_all(fixture.new.join("envs/OwnedEnv")).unwrap();
    fs::write(fixture.new.join("envs/OwnedEnv/keep.txt"), "kept").unwrap();

    fixture
        .store
        .accept(&fixture.old, &fixture.new)
        .await
        .unwrap();

    let (path, interpreter, authorized, state) = fixture.environment_paths("owned").await;
    assert_eq!(path, stored(&fixture.new.join("envs/OwnedEnv")));
    assert_eq!(path, authorized);
    assert_eq!(
        interpreter.as_deref(),
        Some(stored(&fixture.new.join("data/cache/uv-python/python.exe")).as_str())
    );
    for text in [&path, &authorized, interpreter.as_ref().unwrap()] {
        assert!(!text.starts_with(r"\\?\"), "{text}");
    }
    assert_eq!(state, "repair_required");
    let (path, _, authorized, state) = fixture.environment_paths("adopted").await;
    assert_eq!(path, stored(&fixture.new.join("adopted/KeepMe")));
    assert_eq!(path, authorized);
    assert!(!path.starts_with(r"\\?\"), "{path}");
    assert_eq!(state, "verified");
    assert_eq!(fixture.environment_paths("pending").await.3, "failed");
    let (path, interpreter, authorized, state) = fixture.environment_paths("external").await;
    assert_eq!(Path::new(&path), sibling);
    assert_eq!(interpreter.as_deref(), Some(path.as_str()));
    assert_eq!(authorized, path);
    assert_eq!(state, "verified");
    let expected = fixture.new.join("ComfyUI/models");
    let external = sibling.clone();
    fixture
        .db
        .run(move |connection| {
            let path: String = connection
                .query_row(
                    "SELECT path FROM model_roots WHERE id = 'local'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(path, stored(&expected));
            assert!(!path.starts_with(r"\\?\"), "{path}");
            let path: String = connection
                .query_row(
                    "SELECT path FROM model_roots WHERE id = 'external'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(Path::new(&path), external);
            let path: String = connection
                .query_row(
                    "SELECT path FROM model_roots WHERE id = 'relative'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(path, "models");
            let path: String = connection
                .query_row("SELECT path FROM models", [], |row| row.get(0))
                .unwrap();
            assert_eq!(path, "checkpoints/KeepMe.safetensors");
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(
        fs::read_to_string(fixture.new.join("envs/OwnedEnv/keep.txt")).unwrap(),
        "kept"
    );
    assert!(!fixture.old.exists());
}

#[tokio::test]
async fn relocation_is_idempotent_and_preserves_history() {
    let fixture = Fixture::new().await;
    let path = fixture.old.join("envs/previous");
    fixture
        .environment("previous", &path, None, &path, true, "verified")
        .await;
    let run_doc = format!(
        r#"{{"request":{{"install_root":{:?}}},"snapshot":"unchanged"}}"#,
        fixture.old.to_string_lossy()
    );
    let session_doc = format!(
        r#"{{"checkout":{:?},"history":"unchanged"}}"#,
        fixture.old.join("ComfyUI").to_string_lossy()
    );
    let expected_run = run_doc.clone();
    let expected_session = session_doc.clone();
    fixture.db.run(move |connection| {
        connection.execute("INSERT INTO provisioning_runs(id, status, mode, target_env_id, doc_json, activation_phase, created_at, updated_at) VALUES ('run', 'succeeded', 'first_run', 'previous', ?1, 'committed', 't', 't')", [run_doc]).unwrap();
        connection.execute("INSERT INTO comfy_runtime_sessions(id, json) VALUES ('session', ?1)", [session_doc]).unwrap();
        connection.execute("INSERT INTO comfy_launch_options(singleton, version, json) VALUES (1, 1, '{\"preserved\":true}')", []).unwrap();
        connection.execute("INSERT INTO wizard_draft(id, json, updated_at) VALUES (1, '{\"preserved\":true}', 't')", []).unwrap();
        Ok(())
    }).await.unwrap();

    fixture
        .store
        .accept(&fixture.old, &fixture.new)
        .await
        .unwrap();
    let paths = fixture.environment_paths("previous").await;
    fixture
        .store
        .accept(&fixture.old.join("."), &fixture.new.join("."))
        .await
        .unwrap();

    assert_eq!(fixture.environment_paths("previous").await, paths);
    assert_eq!(
        fixture
            .scalar("SELECT COUNT(*) FROM install_relocations")
            .await,
        1
    );
    assert_eq!(
        fixture
            .scalar("SELECT COUNT(*) FROM provisioning_runs")
            .await,
        1
    );
    assert_eq!(
        fixture
            .scalar("SELECT COUNT(*) FROM comfy_runtime_sessions WHERE finished = 1")
            .await,
        1
    );
    fixture
        .db
        .run(move |connection| {
            let run: String = connection
                .query_row(
                    "SELECT doc_json FROM provisioning_runs WHERE id = 'run'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(run, expected_run);
            let session: String = connection
                .query_row(
                    "SELECT json FROM comfy_runtime_sessions WHERE id = 'session'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(session, expected_session);
            for table in ["comfy_launch_options", "wizard_draft"] {
                let json: String = connection
                    .query_row(&format!("SELECT json FROM {table}"), [], |row| row.get(0))
                    .unwrap();
                assert_eq!(json, r#"{"preserved":true}"#);
            }
            Ok(())
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn relocation_refuses_unfinished_journals_without_mutation() {
    for journal in ["activation", "update", "model", "node", "download"] {
        let fixture = Fixture::new().await;
        let path = fixture.old.join("envs/previous");
        fixture
            .environment("previous", &path, None, &path, true, "verified")
            .await;
        fixture.db.run(move |connection| {
            connection.execute("INSERT INTO comfy_runtime_sessions(id, json) VALUES ('session', '{}')", []).unwrap();
            let sql = match journal {
                "activation" => "INSERT INTO provisioning_runs(id, status, mode, target_env_id, doc_json, activation_phase, created_at, updated_at) VALUES ('run', 'running', 'first_run', 'previous', '{}', 'manifest_published', 't', 't')",
                "update" => "INSERT INTO comfy_update_runs(id, json, phase) VALUES ('update', '{}', 'publishing')",
                "model" => "INSERT INTO model_operations(id, kind, phase, document, created_at) VALUES ('model', 'rename', 'recovery_required', '{}', 't')",
                "node" => "INSERT INTO node_operations(id, kind, phase, node_json, environment_id, pins_json, binding, staging, backup, requirements_json, created_at, updated_at) VALUES ('node', 'install', 'published', '{}', 'previous', '{}', 'binding', 'staging', 'backup', '[]', 't', 't')",
                "download" => "INSERT INTO jobs(id, provider_identity, root_id, relative_path, reservation, state, created_at, updated_at, finalize_phase, document) VALUES ('job', 'identity', 'models', 'relative.bin', 'reservation', 'failed', 0, 0, 'published', '{}')",
                _ => unreachable!(),
            };
            connection.execute(sql, []).unwrap();
            Ok(())
        }).await.unwrap();

        assert!(
            matches!(
                fixture.store.accept(&fixture.old, &fixture.new).await,
                Err(RelocationError::RecoveryRequired)
            ),
            "{journal}"
        );
        assert_eq!(
            fixture.environment_paths("previous").await.0,
            path_text(&path).unwrap()
        );
        assert_eq!(fixture.environment_paths("previous").await.3, "verified");
        assert_eq!(
            fixture
                .scalar("SELECT COUNT(*) FROM install_relocations")
                .await,
            0
        );
        assert_eq!(
            fixture
                .scalar("SELECT COUNT(*) FROM comfy_runtime_sessions WHERE finished = 0")
                .await,
            1
        );
    }
}

#[tokio::test]
async fn relocation_refuses_source_publication_journals_without_removing_them() {
    for relative in [
        ".ComfyUI.publish.json",
        "ComfyUI/custom_nodes/.ComfyUI-Manager.publish.json",
    ] {
        let fixture = Fixture::new().await;
        let journal = fixture.new.join(relative);
        fs::create_dir_all(journal.parent().unwrap()).unwrap();
        fs::write(&journal, "incomplete publication").unwrap();
        assert!(matches!(
            fixture.store.accept(&fixture.old, &fixture.new).await,
            Err(RelocationError::RecoveryRequired)
        ));
        assert_eq!(
            fs::read_to_string(journal).unwrap(),
            "incomplete publication"
        );
        assert_eq!(
            fixture
                .scalar("SELECT COUNT(*) FROM install_relocations")
                .await,
            0
        );
    }
}

#[tokio::test]
async fn e3_same_root_rebuild_check_and_accept_are_no_ops() {
    let fixture = Fixture::new().await;
    let path = fixture.new.join("envs/healthy");
    fixture
        .environment("healthy", &path, None, &path, true, "verified")
        .await;
    fixture.db.run(|connection| {
        connection.execute("INSERT INTO comfy_runtime_sessions(id, json) VALUES ('session', '{}')", []).unwrap();
        // Journals that startup reconciliation owns must not refuse an in-place rebuild.
        connection.execute("INSERT INTO comfy_update_runs(id, json, phase) VALUES ('update', '{}', 'intent')", []).unwrap();
        connection.execute("INSERT INTO model_operations(id, kind, phase, document, created_at) VALUES ('model', 'rename', 'recovery_required', '{}', 't')", []).unwrap();
        connection.execute("INSERT INTO jobs(id, provider_identity, root_id, relative_path, reservation, state, created_at, updated_at, finalize_phase, document) VALUES ('job', 'identity', 'models', 'relative.bin', 'reservation', 'failed', 0, 0, 'published', '{}')", []).unwrap();
        Ok(())
    }).await.unwrap();
    fs::write(fixture.new.join(".ComfyUI.publish.json"), "interrupted").unwrap();

    fixture
        .store
        .check(&fixture.new, &fixture.new.join("."))
        .await
        .unwrap();
    fixture
        .store
        .accept(&fixture.new, &fixture.new)
        .await
        .unwrap();

    assert_eq!(fixture.environment_paths("healthy").await.3, "verified");
    assert_eq!(
        fixture
            .scalar("SELECT COUNT(*) FROM install_relocations")
            .await,
        0
    );
    assert_eq!(
        fixture
            .scalar("SELECT COUNT(*) FROM comfy_runtime_sessions WHERE finished = 0")
            .await,
        1
    );
}

#[tokio::test]
async fn relocation_accepts_unpublished_plans_and_terminal_history() {
    let fixture = Fixture::new().await;
    fixture.db.run(|connection| {
        for phase in ["prepared", "completed", "failed"] {
            connection.execute(
                "INSERT INTO node_operations(id, kind, phase, node_json, environment_id, \
                 pins_json, binding, staging, backup, requirements_json, created_at, updated_at) \
                 VALUES (?1, 'install', ?1, '{}', 'previous', '{}', 'binding', 'staging', \
                 'backup', '[]', 't', 't')",
                [phase],
            ).unwrap();
        }
        connection.execute("INSERT INTO comfy_update_runs(id, json, phase) VALUES ('update', '{}', 'complete')", []).unwrap();
        for (id, state, phase) in [("history", "completed", Some("cataloged")), ("queued", "queued", None)] {
            connection.execute(
                "INSERT INTO jobs(id, provider_identity, root_id, relative_path, reservation, \
                 state, created_at, updated_at, finalize_phase, document) \
                 VALUES (?1, ?1, 'models', 'relative.bin', ?1, ?2, 0, 0, ?3, '{\"kept\":true}')",
                params![id, state, phase],
            ).unwrap();
        }
        Ok(())
    }).await.unwrap();

    fixture
        .store
        .accept(&fixture.old, &fixture.new)
        .await
        .unwrap();

    assert_eq!(
        fixture.scalar("SELECT COUNT(*) FROM node_operations").await,
        3
    );
    assert_eq!(
        fixture
            .scalar("SELECT COUNT(*) FROM comfy_update_runs")
            .await,
        1
    );
    assert_eq!(fixture.scalar("SELECT COUNT(*) FROM jobs WHERE relative_path = 'relative.bin' AND document = '{\"kept\":true}'").await, 2);
}

#[tokio::test]
async fn relocation_storage_failure_rolls_back_path_and_session_changes() {
    let fixture = Fixture::new().await;
    let path = fixture.old.join("envs/previous");
    fixture
        .environment("previous", &path, None, &path, true, "verified")
        .await;
    fixture
        .db
        .run(|connection| {
            connection
                .execute(
                    "INSERT INTO comfy_runtime_sessions(id, json) VALUES ('session', '{}')",
                    [],
                )
                .unwrap();
            connection
                .execute_batch(
                    "CREATE TRIGGER refuse_acceptance BEFORE INSERT ON install_relocations \
             BEGIN SELECT RAISE(ABORT, 'storage failure'); END;",
                )
                .unwrap();
            Ok(())
        })
        .await
        .unwrap();

    assert!(matches!(
        fixture.store.accept(&fixture.old, &fixture.new).await,
        Err(RelocationError::Storage)
    ));

    assert_eq!(
        fixture.environment_paths("previous").await.0,
        path_text(&path).unwrap()
    );
    assert_eq!(fixture.environment_paths("previous").await.3, "verified");
    assert_eq!(
        fixture
            .scalar("SELECT COUNT(*) FROM comfy_runtime_sessions WHERE finished = 0")
            .await,
        1
    );
    assert_eq!(
        fixture
            .scalar("SELECT COUNT(*) FROM install_relocations")
            .await,
        0
    );
}
#[tokio::test]
async fn chained_relocation_rebases_paths_from_every_recorded_root() {
    let fixture = Fixture::new().await;
    let owned = fixture.old.join("envs/OwnedEnv");
    let base = fixture.old.join("data/cache/uv-python/python.exe");
    fixture
        .environment("owned", &owned, Some(&base), &owned, true, "verified")
        .await;
    let adopted = fixture.old.join("adopted/KeepMe");
    fixture
        .environment("adopted", &adopted, None, &adopted, false, "verified")
        .await;
    let sibling = fixture
        .old
        .with_file_name("old-root-sibling")
        .join("external");
    fixture
        .environment(
            "external",
            &sibling,
            Some(&sibling),
            &sibling,
            true,
            "verified",
        )
        .await;
    let external = path_text(&sibling).unwrap();
    let local_models = path_text(&fixture.old.join("ComfyUI/models")).unwrap();
    fixture
        .db
        .run(move |connection| {
            for (id, path) in [("local", local_models), ("external", external)] {
                connection
                    .execute(
                        "INSERT INTO model_roots(id, path, status) VALUES (?1, ?2, 'ready')",
                        params![id, path],
                    )
                    .unwrap();
            }
            Ok(())
        })
        .await
        .unwrap();

    // The first rebuild is accepted and fails; the copy is then copied again.
    fixture
        .store
        .accept(&fixture.old, &fixture.new)
        .await
        .unwrap();
    let second = fixture.old.with_file_name("second-copy");
    fixture.store.accept(&fixture.old, &second).await.unwrap();

    let (path, interpreter, authorized, state) = fixture.environment_paths("owned").await;
    assert_eq!(path, stored(&second.join("envs/OwnedEnv")));
    assert_eq!(path, authorized);
    assert_eq!(
        interpreter.as_deref(),
        Some(stored(&second.join("data/cache/uv-python/python.exe")).as_str())
    );
    assert_eq!(state, "repair_required");
    let (path, _, authorized, state) = fixture.environment_paths("adopted").await;
    assert_eq!(path, stored(&second.join("adopted/KeepMe")));
    assert_eq!(path, authorized);
    assert_eq!(state, "verified");
    let (path, interpreter, authorized, _) = fixture.environment_paths("external").await;
    assert_eq!(Path::new(&path), sibling);
    assert_eq!(interpreter.as_deref(), Some(path.as_str()));
    assert_eq!(authorized, path);
    let expected = [
        ("local", stored(&second.join("ComfyUI/models"))),
        ("external", path_text(&sibling).unwrap()),
    ];
    fixture
        .db
        .run(move |connection| {
            for (id, expected) in expected {
                let path: String = connection
                    .query_row("SELECT path FROM model_roots WHERE id = ?1", [id], |row| {
                        row.get(0)
                    })
                    .unwrap();
                assert_eq!(path, expected, "{id}");
            }
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(
        fixture
            .scalar("SELECT COUNT(*) FROM install_relocations")
            .await,
        2
    );
}

/// A location that canonicalizes to an error other than "not found", as an offline share or
/// denied access does.
fn unreadable_location(base: &Path) -> PathBuf {
    if cfg!(windows) {
        base.join("unreadable*name")
    } else {
        let blocker = base.join("blocker");
        fs::write(&blocker, "a file, not a folder").unwrap();
        blocker.join("child")
    }
}

#[tokio::test]
async fn e3_unreadable_external_paths_are_left_unchanged() {
    let fixture = Fixture::new().await;
    let external_base = fixture.old.with_file_name("external-share");
    fs::create_dir_all(&external_base).unwrap();
    let unreadable = unreadable_location(&external_base);
    assert!(
        fs::canonicalize(&unreadable).is_err_and(|error| error.kind() != io::ErrorKind::NotFound),
        "the fixture must not look merely missing"
    );
    fixture
        .environment(
            "offline",
            &unreadable,
            Some(&unreadable),
            &unreadable,
            false,
            "verified",
        )
        .await;
    let owned = fixture.old.join("envs/OwnedEnv");
    fixture
        .environment("owned", &owned, None, &owned, true, "verified")
        .await;
    let model_root = path_text(&unreadable).unwrap();
    fixture
        .db
        .run(move |connection| {
            connection
                .execute(
                    "INSERT INTO model_roots(id, path, status) VALUES ('offline', ?1, 'ready')",
                    [model_root],
                )
                .unwrap();
            Ok(())
        })
        .await
        .unwrap();

    fixture
        .store
        .accept(&fixture.old, &fixture.new)
        .await
        .unwrap();

    let text = path_text(&unreadable).unwrap();
    let (path, interpreter, authorized, state) = fixture.environment_paths("offline").await;
    assert_eq!(path, text);
    assert_eq!(interpreter.as_deref(), Some(text.as_str()));
    assert_eq!(authorized, text);
    assert_eq!(state, "verified");
    assert_eq!(
        fixture.environment_paths("owned").await.0,
        stored(&fixture.new.join("envs/OwnedEnv"))
    );
    fixture
        .db
        .run(move |connection| {
            let path: String = connection
                .query_row(
                    "SELECT path FROM model_roots WHERE id = 'offline'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(path, text);
            Ok(())
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn e3_cloning_markers_require_recovery_before_relocation() {
    for marker in [".ComfyUI.cloning", ".Other.CLONING"] {
        for in_new_root in [true, false] {
            let fixture = Fixture::new().await;
            fs::create_dir_all(&fixture.old).unwrap();
            let root = if in_new_root {
                &fixture.new
            } else {
                &fixture.old
            };
            fs::create_dir(root.join(marker)).unwrap();
            assert!(
                matches!(
                    fixture.store.check(&fixture.old, &fixture.new).await,
                    Err(RelocationError::RecoveryRequired)
                ),
                "{marker} check"
            );
            assert!(
                matches!(
                    fixture.store.accept(&fixture.old, &fixture.new).await,
                    Err(RelocationError::RecoveryRequired)
                ),
                "{marker} accept"
            );
            assert!(root.join(marker).is_dir(), "{marker} is preserved");
            assert_eq!(
                fixture
                    .scalar("SELECT COUNT(*) FROM install_relocations")
                    .await,
                0
            );
        }
    }
}
