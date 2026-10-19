//! Library adapters over a real temporary install root: scanning, catalog preservation,
//! compare-and-apply, recoverable operations on real files, reservations and migration upgrade.
use super::{identify::CivitaiClient, sqlite::SqliteJournal, stack::LibraryStack};
use crate::{
    models::acquisition::{http::ArtifactHttp, sqlite::SqliteDownloads},
    settings::secrets::{CredentialSnapshot, CredentialVault},
    shared::{
        path_locks::PathLocks,
        sqlite::Db,
        test_http::{Route, TestServer},
    },
};
use application::{
    comfy_runtime::{ExtraModelPaths, ModelRoot, RuntimeError},
    models::library::*,
};
use async_trait::async_trait;
use domain::{
    models::library::ModelLocation, settings::CredentialKind,
    shared::model_values::RelativeModelPath,
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

struct NoCredentials;
#[async_trait]
impl CredentialVault for NoCredentials {
    async fn read(
        &self,
        _: CredentialKind,
    ) -> std::result::Result<CredentialSnapshot, application::settings::CredentialStoreError> {
        Ok(CredentialSnapshot {
            generation: 0,
            value: None,
        })
    }
    async fn generation(
        &self,
        _: CredentialKind,
    ) -> std::result::Result<u64, application::settings::CredentialStoreError> {
        Ok(0)
    }
}
struct Extras(Vec<ModelRoot>);
#[async_trait]
impl ExtraModelPaths for Extras {
    async fn read(&self, _: &Path) -> std::result::Result<Vec<ModelRoot>, RuntimeError> {
        Ok(self.0.clone())
    }
}

struct Fx {
    _dir: tempfile::TempDir,
    models: PathBuf,
    db: Db,
    stack: LibraryStack,
}
impl Fx {
    async fn new() -> Self {
        Self::with_extras(|_| Vec::new()).await
    }
    async fn with_extras(extras: impl FnOnce(&Path) -> Vec<ModelRoot>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let install = dir.path().join("install ไทย");
        let models = install.join("ComfyUI").join("models");
        fs::create_dir_all(&models).unwrap();
        let extras = extras(&models);
        let db = Db::open(dir.path().join("app.db")).await.unwrap();
        let stack = LibraryStack::open(
            &install,
            db.clone(),
            Arc::new(NoCredentials),
            Arc::new(PathLocks::default()),
            Arc::new(Extras(extras)),
            Arc::new(SqliteDownloads { db: db.clone() }),
        )
        .unwrap();
        Self {
            _dir: dir,
            models,
            db,
            stack,
        }
    }
    fn write(&self, relative: &str, bytes: &[u8]) {
        let path = self.models.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    async fn scan(&self) {
        self.stack.supervisor.scan_now(None).await.unwrap();
    }
    async fn list(&self) -> Vec<String> {
        self.stack
            .deps
            .catalog
            .list()
            .await
            .unwrap()
            .iter()
            .map(|m| format!("{}:{}", m.location.root_id, m.location.path.as_string()))
            .collect()
    }
    async fn revision(&self) -> u64 {
        self.stack.deps.catalog.revision().await.unwrap()
    }
    fn names(&self, directory: &str) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.models.join(directory))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
}
fn loc(path: &str) -> ModelLocation {
    ModelLocation::new("models", RelativeModelPath::new(path).unwrap())
}

#[tokio::test]
async fn scanning_catalogs_models_hides_sidecars_and_a_cached_rescan_changes_nothing() {
    let fx = Fx::new().await;
    fx.write("checkpoints/a.safetensors", b"aaaa");
    fx.write("checkpoints/a.preview.png", b"png");
    fx.write("loras/sub/b.pt", b"bbbb");
    fx.scan().await;
    assert_eq!(
        fx.list().await,
        ["models:checkpoints/a.safetensors", "models:loras/sub/b.pt"]
    );
    let model = fx
        .stack
        .deps
        .catalog
        .get(&loc("checkpoints/a.safetensors"))
        .await
        .unwrap()
        .unwrap();
    assert!(model.sidecars.preview);
    assert_eq!(model.category.as_str(), "checkpoints");
    let revision = fx.revision().await;
    fx.scan().await;
    assert_eq!(
        fx.revision().await,
        revision,
        "an unchanged scan mutates nothing"
    );
}

#[tokio::test]
async fn provenance_survives_a_rescan_but_not_a_replaced_file_or_a_failed_scan() {
    let fx = Fx::new().await;
    fx.write("checkpoints/a.safetensors", b"aaaa");
    fx.write("checkpoints/b.safetensors", b"bbbb");
    fx.scan().await;
    fx.db
        .run(|c| {
            c.execute(
                "UPDATE models SET sha256=?1, source_job_id='job-1', civitai_version_id=9",
                ["a".repeat(64)],
            )
            .unwrap();
            Ok(())
        })
        .await
        .unwrap();
    fx.scan().await;
    let before = fx
        .stack
        .deps
        .catalog
        .get(&loc("checkpoints/a.safetensors"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(before.source_job_id.as_ref().unwrap().as_str(), "job-1");
    // A replacement at the same pathname must not inherit the previous file's provenance.
    fx.write("checkpoints/a.safetensors", b"a different and longer file");
    fx.scan().await;
    let after = fx
        .stack
        .deps
        .catalog
        .get(&loc("checkpoints/a.safetensors"))
        .await
        .unwrap()
        .unwrap();
    assert!(after.sha256.is_none() && after.source_job_id.is_none());
    assert_eq!(after.civitai_version_id, None);
    // An incomplete scan never erases rows it did not see.
    let revision = fx.revision().await;
    let applied = fx
        .stack
        .catalog
        .apply_scan(
            revision,
            vec![super::scan::RootScan {
                root_id: "models".into(),
                complete: false,
                files: Vec::new(),
                warnings: Vec::new(),
            }],
            "t".into(),
        )
        .await
        .unwrap();
    assert!(applied);
    assert_eq!(fx.list().await.len(), 2);
    // A complete scan removes a deleted file.
    fs::remove_file(fx.models.join("checkpoints/b.safetensors")).unwrap();
    fx.scan().await;
    assert_eq!(fx.list().await, ["models:checkpoints/a.safetensors"]);
}

#[tokio::test]
async fn a_stale_scan_cannot_overwrite_a_newer_catalog_change() {
    let fx = Fx::new().await;
    fx.write("checkpoints/a.safetensors", b"aaaa");
    let started_at = fx.revision().await;
    // A download finishes while the (old) scan is still walking.
    fx.db
        .run(|c| {
            c.execute(
                "INSERT INTO models(root_id,path,category,size,mtime,file_identity,meta_json) VALUES ('models','checkpoints/new.safetensors','checkpoints',1,'1','{}','{}')",
                [],
            )
            .unwrap();
            Ok(())
        })
        .await
        .unwrap();
    let observations =
        super::scan::scan_root(&fx.stack.roots.spec("models").unwrap(), &Default::default());
    let applied = fx
        .stack
        .catalog
        .apply_scan(started_at, vec![observations], "t".into())
        .await
        .unwrap();
    assert!(!applied, "stale observations are discarded");
    assert_eq!(fx.list().await, ["models:checkpoints/new.safetensors"]);
    // A fresh scan then reconciles both.
    fx.scan().await;
    assert_eq!(fx.list().await, ["models:checkpoints/a.safetensors"]);
}

#[tokio::test]
async fn rename_moves_real_sidecars_updates_the_catalog_and_survives_a_rescan() {
    let fx = Fx::new().await;
    for name in [
        "foo.safetensors",
        "foo.preview.png",
        "foo.civitai.info",
        "foo.json",
        "foo.metadata.json",
    ] {
        fx.write(&format!("checkpoints/{name}"), name.as_bytes());
    }
    fx.scan().await;
    let placed = fx
        .stack
        .deps
        .rename(
            &loc("checkpoints/foo.safetensors"),
            "ไทย bar.safetensors",
            false,
        )
        .await
        .unwrap();
    assert_eq!(placed.location.display(), "checkpoints/ไทย bar.safetensors");
    assert_eq!(
        fx.names("checkpoints"),
        [
            "foo.metadata.json",
            "ไทย bar.civitai.info",
            "ไทย bar.json",
            "ไทย bar.preview.png",
            "ไทย bar.safetensors",
        ]
    );
    assert!(fx.stack.deps.journal.unfinished().await.unwrap().is_empty());
    fx.scan().await;
    let list = fx.list().await;
    assert!(list.contains(&"models:checkpoints/ไทย bar.safetensors".to_owned()));
    assert!(!list.iter().any(|p| p.contains("foo.safetensors")));
}

#[cfg(windows)]
#[tokio::test]
async fn a_case_only_rename_changes_the_name_on_a_case_insensitive_volume() {
    let fx = Fx::new().await;
    fx.write("checkpoints/model.safetensors", b"m");
    fx.write("checkpoints/model.json", b"{}");
    fx.scan().await;
    fx.stack
        .deps
        .rename(
            &loc("checkpoints/model.safetensors"),
            "Model.safetensors",
            false,
        )
        .await
        .unwrap();
    assert_eq!(fx.names("checkpoints"), ["Model.json", "Model.safetensors"]);
    assert!(fx.stack.deps.journal.unfinished().await.unwrap().is_empty());
    assert_eq!(fx.list().await, ["models:checkpoints/Model.safetensors"]);
}

#[tokio::test]
async fn conflicts_ambiguous_stems_and_traversal_names_leave_files_untouched() {
    let fx = Fx::new().await;
    fx.write("checkpoints/a.safetensors", b"a");
    fx.write("checkpoints/b.safetensors", b"b");
    fx.write("checkpoints/c.safetensors", b"c");
    fx.write("checkpoints/c.ckpt", b"c2");
    fx.write("checkpoints/c.json", b"{}");
    fx.scan().await;
    let deps = &fx.stack.deps;
    for (from, to, code) in [
        (
            "a.safetensors",
            "b.safetensors",
            LibraryErrorCode::TargetExists,
        ),
        (
            "a.safetensors",
            "..\\..\\evil.dll",
            LibraryErrorCode::InvalidName,
        ),
        (
            "a.safetensors",
            "CON.safetensors",
            LibraryErrorCode::InvalidName,
        ),
        (
            "c.safetensors",
            "d.safetensors",
            LibraryErrorCode::AmbiguousSidecar,
        ),
    ] {
        let error = deps
            .rename(&loc(&format!("checkpoints/{from}")), to, true)
            .await
            .unwrap_err();
        assert_eq!(error.0, code, "{from} -> {to}");
    }
    assert_eq!(
        fx.names("checkpoints"),
        [
            "a.safetensors",
            "b.safetensors",
            "c.ckpt",
            "c.json",
            "c.safetensors"
        ]
    );
}

#[tokio::test]
async fn move_creates_folders_and_a_download_reservation_blocks_the_destination() {
    let fx = Fx::new().await;
    fx.write("checkpoints/a.safetensors", b"a");
    fx.write("checkpoints/a.json", b"{}");
    fx.scan().await;
    // An unfinished download job owns loras/a.safetensors.
    fx.db
        .run(|c| {
            c.execute(
                "INSERT INTO jobs(id,provider_identity,root_id,relative_path,reservation,state,generation,created_at,updated_at,document) VALUES ('job','direct:x','models','loras/a.safetensors','models:loras/a.safetensors','queued',0,1,1,'{}')",
                [],
            )
            .unwrap();
            Ok(())
        })
        .await
        .unwrap();
    let blocked = fx
        .stack
        .deps
        .move_to(
            &loc("checkpoints/a.safetensors"),
            "models",
            RelativeModelPath::new("loras").unwrap(),
        )
        .await
        .unwrap_err();
    assert_eq!(blocked.0, LibraryErrorCode::BusyReserved);
    assert!(fx.models.join("checkpoints/a.safetensors").exists());
    let moved = fx
        .stack
        .deps
        .move_to(
            &loc("checkpoints/a.safetensors"),
            "models",
            RelativeModelPath::new("loras/new folder").unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(moved.location.display(), "loras/new folder/a.safetensors");
    assert!(fx.models.join("loras/new folder/a.json").exists());
    assert!(!fx.models.join("checkpoints/a.json").exists());
    assert_eq!(
        fx.stack
            .deps
            .catalog
            .get(&moved.location)
            .await
            .unwrap()
            .unwrap()
            .category
            .as_str(),
        "loras"
    );
}

#[tokio::test]
async fn startup_reconciliation_finishes_a_crashed_rename_on_real_files() {
    let fx = Fx::new().await;
    fx.write("checkpoints/a.safetensors", b"a");
    fx.write("checkpoints/a.json", b"{}");
    fx.scan().await;
    // Crash window: the sidecar was already renamed, the model was not.
    fs::rename(
        fx.models.join("checkpoints/a.json"),
        fx.models.join("checkpoints/b.json"),
    )
    .unwrap();
    SqliteJournal { db: fx.db.clone() }
        .begin(&OperationRecord {
            id: "crashed".into(),
            kind: OperationKind::Rename,
            phase: OperationPhase::Intent,
            members: vec![
                OperationMember {
                    from: loc("checkpoints/a.safetensors"),
                    to: loc("checkpoints/b.safetensors"),
                    via: None,
                    model: true,
                },
                OperationMember {
                    from: loc("checkpoints/a.json"),
                    to: loc("checkpoints/b.json"),
                    via: None,
                    model: false,
                },
            ],
            created_at: "t".into(),
        })
        .await
        .unwrap();
    // New mutations of those paths are refused until recovery finished.
    assert_eq!(
        fx.stack
            .deps
            .rename(&loc("checkpoints/a.safetensors"), "z.safetensors", false)
            .await
            .unwrap_err()
            .0,
        LibraryErrorCode::RecoveryRequired
    );
    let report = fx.stack.deps.reconcile().await.unwrap();
    assert_eq!(report.completed, 1);
    assert_eq!(fx.names("checkpoints"), ["b.json", "b.safetensors"]);
    assert_eq!(fx.list().await, ["models:checkpoints/b.safetensors"]);
    assert!(fx.stack.deps.journal.unfinished().await.unwrap().is_empty());
}

#[tokio::test]
async fn permanent_delete_removes_the_file_and_row_only_after_success() {
    let fx = Fx::new().await;
    fx.write("vae/v.pt", b"vae");
    fx.write("vae/v.json", b"{}");
    fx.scan().await;
    let deps = &fx.stack.deps;
    let plan = deps
        .plan_delete(vec![loc("vae/v.pt")], DeleteKind::Permanent)
        .await
        .unwrap();
    assert_eq!(plan.total_bytes, 3);
    let report = deps
        .execute_delete(&plan.id, DeleteKind::Permanent)
        .await
        .unwrap();
    assert_eq!(report.results[0].outcome, DeleteOutcome::Deleted);
    // FR-MODEL-3 does not authorize sidecar cleanup.
    assert_eq!(fx.names("vae"), ["v.json"]);
    assert!(fx.list().await.is_empty() || fx.list().await == ["models:vae/v.json"]);
}

#[tokio::test]
async fn extra_roots_get_a_fixed_category_overlaps_collapse_and_missing_roots_warn() {
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("e.safetensors"), b"extra").unwrap();
    let external = outside.path().to_owned();
    let fx = Fx::with_extras(|models| {
        vec![
            ModelRoot {
                section: "shared".into(),
                category: "loras".into(),
                path: external.clone(),
                is_default: false,
            },
            ModelRoot {
                section: "gone".into(),
                category: "vae".into(),
                path: external.join("does-not-exist"),
                is_default: false,
            },
            ModelRoot {
                section: "inner".into(),
                category: "loras".into(),
                path: models.join("loras"),
                is_default: false,
            },
        ]
    })
    .await;
    // A root below the default root is scanned through it and listed once.
    fx.write("loras/inner.pt", b"i");
    fx.scan().await;
    let list = fx.list().await;
    assert!(
        list.iter()
            .any(|p| p.starts_with("extra-") && p.ends_with(":e.safetensors"))
    );
    assert!(list.contains(&"models:loras/inner.pt".to_owned()));
    let status = fx.stack.deps.control.status();
    assert_eq!(status.roots.len(), 3);
    assert!(status.roots.iter().any(|r| r.state == RootState::Missing));
    assert!(
        status
            .warnings
            .iter()
            .any(|w| w.code == ScanWarningCode::RootMissing)
    );
    // Deleting the extra folder's content in a failed (missing) root never erased others.
    let extra_row = fx
        .stack
        .deps
        .catalog
        .list()
        .await
        .unwrap()
        .into_iter()
        .find(|m| m.location.root_id.starts_with("extra-"))
        .unwrap();
    assert_eq!(extra_row.category.as_str(), "loras");
    // Operations inside an extra root are contained in it.
    let renamed = fx
        .stack
        .deps
        .rename(&extra_row.location, "renamed.safetensors", false)
        .await
        .unwrap();
    assert!(outside.path().join("renamed.safetensors").exists());
    assert_eq!(renamed.location.root_id, extra_row.location.root_id);
    // A move across roots stays within authorized roots; the default root needs a category.
    let moved = fx
        .stack
        .deps
        .move_to(
            &renamed.location,
            "models",
            RelativeModelPath::new("loras/from extra").unwrap(),
        )
        .await
        .unwrap();
    assert!(
        fx.models
            .join("loras/from extra/renamed.safetensors")
            .exists()
    );
    assert_eq!(moved.location.root_id, "models");
    assert_eq!(
        fx.stack
            .deps
            .move_to(&moved.location, "models", RelativeModelPath::default())
            .await
            .unwrap_err()
            .0,
        LibraryErrorCode::OutsideRoot
    );
}

#[tokio::test]
async fn identification_writes_sidecars_and_a_second_run_uses_the_hash_cache() {
    let fx = Fx::new().await;
    fx.write("loras/x.safetensors", b"identify me");
    fx.scan().await;
    let server = TestServer::start().await;
    let mut deps = fx.stack.deps.clone();
    deps.civitai = Arc::new(CivitaiClient::with_base(
        ArtifactHttp::new(Arc::new(NoCredentials)).unwrap(),
        url::Url::parse(&server.base_url).unwrap(),
    ));
    let image = {
        let image = image::RgbImage::from_pixel(4, 4, image::Rgb([1, 2, 3]));
        let mut out = Vec::new();
        image
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    };
    server.route("/preview.png", Route::ok(image));
    // The fixture serves a response for whatever the file's real hash is.
    let hash = {
        let live = deps
            .files
            .inspect(&loc("loras/x.safetensors"))
            .await
            .unwrap()
            .unwrap();
        deps.files
            .hash(&loc("loras/x.safetensors"), &live)
            .await
            .unwrap()
    };
    server.route(
        &format!("/api/v1/model-versions/by-hash/{}", hash.as_str()),
        Route::ok(
            serde_json::json!({
                "id": 5, "modelId": 6, "name": "v1", "baseModel": "Flux",
                "trainedWords": ["x"], "model": {"name": "X"},
                "images": [{"type": "image", "url": format!("{}/preview.png", server.base_url)}]
            })
            .to_string()
            .into_bytes(),
        ),
    );
    let outcome = deps
        .identify(&loc("loras/x.safetensors"), false)
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        IdentifyOutcome::Identified {
            preview: PreviewOutcome::Written,
            ..
        }
    ));
    assert_eq!(
        fx.names("loras"),
        ["x.civitai.info", "x.preview.png", "x.safetensors"]
    );
    let info = fs::read_to_string(fx.models.join("loras/x.civitai.info")).unwrap();
    assert!(info.contains("\"versionId\": 5") && !info.contains("http"));
    let row = deps
        .catalog
        .get(&loc("loras/x.safetensors"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.civitai_version_id, Some(5));
    assert_eq!(row.sha256, Some(hash));
    fx.scan().await;
    let details = deps
        .details
        .read(&loc("loras/x.safetensors"))
        .await
        .unwrap();
    assert_eq!(details.civitai.unwrap().model_name, "X");
    assert_eq!(&details.preview_png.unwrap()[..4], b"\x89PNG");
    // The sidecars now exist, so a repeat asks for a replacement decision.
    assert!(matches!(
        deps.identify(&loc("loras/x.safetensors"), false)
            .await
            .unwrap(),
        IdentifyOutcome::SidecarsExist { .. }
    ));
}

#[tokio::test]
async fn an_m3_database_upgrades_and_keeps_its_catalog_rows() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    {
        let mut connection = rusqlite::Connection::open(&path).unwrap();
        let first_four = rusqlite_migration::Migrations::new(vec![
            rusqlite_migration::M::up(include_str!("../../../migrations/001_settings.sql")),
            rusqlite_migration::M::up(include_str!("../../../migrations/002_environment.sql")),
            rusqlite_migration::M::up(include_str!("../../../migrations/003_comfy_runtime.sql")),
            rusqlite_migration::M::up(include_str!("../../../migrations/004_downloads.sql")),
        ]);
        first_four.to_latest(&mut connection).unwrap();
        connection
            .execute(
                "INSERT INTO models(root_id,path,category,size,mtime,file_identity,sha256,source_job_id,civitai_version_id,meta_json) VALUES ('models','checkpoints/m3.safetensors','checkpoints',4,'7','{\"size\":4,\"modified_ns\":\"7\",\"platform_id\":\"p\",\"digest\":null}',?1,'job-m3',12,'{}')",
                ["c".repeat(64)],
            )
            .unwrap();
    }
    let db = Db::open(&path).await.unwrap();
    let (rows, sidecars, revision) = db
        .run(|c| {
            let rows: u32 = c
                .query_row("SELECT COUNT(*) FROM models", [], |r| r.get(0))
                .unwrap();
            let sidecars: i64 = c
                .query_row("SELECT sidecars FROM models", [], |r| r.get(0))
                .unwrap();
            let revision = super::sqlite::revision_of(c)?;
            Ok((rows, sidecars, revision))
        })
        .await
        .unwrap();
    assert_eq!((rows, sidecars), (1, 0));
    assert_eq!(revision, 0);
}

#[tokio::test]
async fn the_supervisor_scans_on_start_rescans_on_demand_and_announces_changes() {
    let fx = Fx::new().await;
    fx.write("checkpoints/first.safetensors", b"1");
    let mut changes = fx.stack.supervisor.subscribe();
    let task = fx.stack.start();
    fx.stack.deps.control.rescan().await.unwrap();
    assert_eq!(fx.list().await, ["models:checkpoints/first.safetensors"]);
    assert!(changes.has_changed().unwrap());
    changes.borrow_and_update();
    fx.write("checkpoints/second.safetensors", b"2");
    fx.stack.deps.control.rescan().await.unwrap();
    assert_eq!(fx.list().await.len(), 2);
    assert!(changes.has_changed().unwrap());
    let status = fx.stack.deps.control.status();
    assert!(!status.scanning);
    assert!(status.sequence > 0);
    assert_ne!(status.watcher, WatcherState::Unavailable);
    task.abort();
}

#[tokio::test]
async fn library_shutdown_waits_for_the_in_flight_scan_and_catalog_write() {
    #[derive(Default)]
    struct HeldRoots {
        entered: tokio::sync::Notify,
        resume: tokio::sync::Notify,
    }

    #[async_trait]
    impl ExtraModelPaths for HeldRoots {
        async fn read(&self, _: &Path) -> std::result::Result<Vec<ModelRoot>, RuntimeError> {
            self.entered.notify_one();
            self.resume.notified().await;
            Ok(Vec::new())
        }
    }

    let fx = Fx::new().await;
    fx.write("checkpoints/first.safetensors", b"1");
    let held = Arc::new(HeldRoots::default());
    let roots = super::roots::LibraryRoots::new(
        fx.models.parent().unwrap().parent().unwrap(),
        held.clone(),
        Arc::new(PathLocks::default()),
    )
    .unwrap();
    let supervisor =
        super::supervisor::LibrarySupervisor::new(fx.stack.catalog.clone(), Arc::new(roots));
    let (shutdown, stopped) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(supervisor.clone().run_until(stopped));
    held.entered.notified().await;
    shutdown.send(()).unwrap();
    tokio::task::yield_now().await;
    assert!(!task.is_finished(), "shutdown detached an in-flight scan");
    held.resume.notify_one();
    tokio::time::timeout(std::time::Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fx.list().await, ["models:checkpoints/first.safetensors"]);
    assert!(!supervisor.status().scanning);
    assert_eq!(supervisor.status().watcher, WatcherState::Unavailable);
}

#[tokio::test]
async fn library_shutdown_drops_the_watcher_and_a_new_run_rescans_and_watches() {
    let fx = Fx::new().await;
    fx.write("checkpoints/first.safetensors", b"1");
    for expected_count in [1, 2] {
        let (shutdown, stopped) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(fx.stack.supervisor.clone().run_until(stopped));
        fx.stack.deps.control.rescan().await.unwrap();
        assert_eq!(fx.list().await.len(), expected_count);
        assert_ne!(
            fx.stack.deps.control.status().watcher,
            WatcherState::Unavailable
        );
        shutdown.send(()).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        assert!(!fx.stack.deps.control.status().scanning);
        assert_eq!(
            fx.stack.deps.control.status().watcher,
            WatcherState::Unavailable
        );
        fx.write("checkpoints/second.safetensors", b"2");
    }
}
